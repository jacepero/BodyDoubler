// body-double — registered app: a body double for interstitial journaling,
// distilled from the owner's July 2026 iOS app (jacepero/BodyDoubler). The
// person says what they are doing; the persistent assistant keeps them
// oriented through these tools (the current task and how long it has run,
// loose ends, a journal of their own words), and a check-in comes when they
// have gone quiet (apps/body-double.md).
//
// On iOS one small on-device model did everything. Here the assistant is the
// voice and the kernel routes its inference; this app holds the state the
// iOS app kept in SQLite, and no model. Check-ins are templates: no model
// call, and moods are never part of them.

pub mod checkins;
pub mod config;
pub mod store;
pub mod time;
pub mod tools;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use config::AppConfig;
use greenhouse_kernel::clock::Zone;
use greenhouse_kernel::error::{err, ApiError, ApiResult};
use greenhouse_kernel::identity::{
    verify_app_request, Claims, IdentityError, IdentityPurpose, IdentitySubjectKind,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use store::Store;

pub const APP_ID: &str = "body-double";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The notification category of a check-in.
pub const CHECKIN_CATEGORY: &str = "body-double-checkin";
pub const DB_FILE: &str = "body-double.sqlite3";

/// The effect each tool needs. Unknown names are not tools.
pub fn tool_effect(name: &str) -> Option<&'static str> {
    match name {
        "now" | "day" | "export" => Some("read"),
        "log" | "set_task" | "update_task" | "open_thread" | "close_thread" | "checkins" => {
            Some("write")
        }
        "checkin_tick" => Some("command"),
        _ => None,
    }
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "parameters": parameters,
        "effect": tool_effect(name).expect("a declared tool"),
    })
}

fn no_arguments() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

/// What the app registers. The descriptions steer the assistant the way the
/// iOS system prompt did, so they are part of the approved contract.
pub fn manifest() -> Value {
    // Plain types and lengths only: tool schemas may become sampling grammars
    // on the inference node, where regex patterns are not dependable.
    let id = json!({"type": "integer", "minimum": 1});
    let date = json!({"type": "string", "maxLength": 10, "description": "YYYY-MM-DD"});
    let clock = json!({"type": "string", "maxLength": 5, "description": "HH:MM, 24-hour"});
    json!({
        "id": APP_ID,
        "version": APP_VERSION,
        "health": "/health",
        "tools": [
            tool("now",
                "Body double: where the person is right now: current task and how long it has run, open loose ends with their ids, the last few journal entries and check-in status. Read it when they say what they are doing or ask what they were up to. Answer in at most two short sentences: acknowledge, keep them oriented, mention elapsed time or a loose end only when it helps. No flattery, no lectures.",
                no_arguments()),
            tool("day",
                "Body double journal for one day (default today): entries in order, tasks touched, loose ends opened and closed, check-ins. Use it for recaps: what got done, what is still open, what stalled, in a few plain sentences. Mention moods only if asked.",
                json!({"type": "object", "properties": {"date": date}, "additionalProperties": false})),
            tool("export",
                "Body double: the person's whole journal, or the days from/to, as Markdown or JSON, to save a copy. Not for answering questions; use day for that.",
                json!({"type": "object", "properties": {
                    "format": {"type": "string", "enum": ["markdown", "json"]},
                    "from": date, "to": date
                }, "additionalProperties": false})),
            tool("log",
                "Body double journal: record what the person says they are doing, starting, finishing or feeling, in their own words (text). Log every such remark. Add a one-line summary; give mood only when they clearly express one. Logging answers a pending check-in.",
                json!({"type": "object", "properties": {
                    "text": {"type": "string", "minLength": 1, "maxLength": 2000},
                    "summary": {"type": "string", "maxLength": 200},
                    "mood": {"type": "string", "maxLength": 40}
                }, "required": ["text"], "additionalProperties": false})),
            tool("set_task",
                "Body double: the person started or switched to a task. Makes title the current task and pauses the previous one. An open task with the same title is resumed, not duplicated.",
                json!({"type": "object", "properties": {
                    "title": {"type": "string", "minLength": 1, "maxLength": 200}
                }, "required": ["title"], "additionalProperties": false})),
            tool("update_task",
                "Body double: change a task's status by its id from now or day. done or dropped ends it, paused sets it aside, active resumes it as the current task.",
                json!({"type": "object", "properties": {
                    "task_id": id,
                    "status": {"type": "string", "enum": ["active", "paused", "done", "dropped"]}
                }, "required": ["task_id", "status"], "additionalProperties": false})),
            tool("open_thread",
                "Body double: remember a loose end the person mentions (an email to send, something to check) so it can come up later.",
                json!({"type": "object", "properties": {
                    "note": {"type": "string", "minLength": 1, "maxLength": 300}
                }, "required": ["note"], "additionalProperties": false})),
            tool("close_thread",
                "Body double: a loose end is resolved. Close it by its id from now or day.",
                json!({"type": "object", "properties": {"thread_id": id},
                    "required": ["thread_id"], "additionalProperties": false})),
            tool("checkins",
                "Body double check-ins: turn them on or off, set every_minutes, quiet hours (quiet_from and quiet_until, or quiet false for none), the person's time zone, or next_in_minutes for one at a set time, such as when they start a time-boxed stretch.",
                json!({"type": "object", "properties": {
                    "on": {"type": "boolean"},
                    "every_minutes": {"type": "integer", "minimum": 10, "maximum": 240},
                    "quiet": {"type": "boolean"},
                    "quiet_from": clock,
                    "quiet_until": clock,
                    "timezone": {"type": "string", "maxLength": 64},
                    "next_in_minutes": {"type": "integer", "minimum": 5, "maximum": 480}
                }, "additionalProperties": false})),
            tool("checkin_tick",
                "Scheduled, not for chat: raises the body-double check-ins that are due.",
                no_arguments()),
        ],
        "events": { "emits": [], "subscribes": [] },
        "schedules": [{ "name": "checkins.tick", "cron": "* * * * *", "action": "checkin_tick" }],
        "permissions": {
            "secrets": [],
            "egress": [],
            "notify": ["in-app"],
            "notify_categories": [{ "category": CHECKIN_CATEGORY, "sensitivity": "private" }]
        }
    })
}

pub struct AppState {
    pub cfg: AppConfig,
    pub identity_pem: Mutex<Option<String>>,
    pub kernel: reqwest::Client,
    pub store: Mutex<Store>,
    default_zone: Zone,
    /// A fixed clock for tests; 0 means the real one.
    pub fixed_now: AtomicI64,
}

impl AppState {
    pub fn now(&self) -> i64 {
        match self.fixed_now.load(Ordering::SeqCst) {
            0 => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            fixed => fixed,
        }
    }

    fn ctx<'a>(&'a self, user: &'a str, now: i64) -> tools::Ctx<'a> {
        tools::Ctx {
            cfg: &self.cfg,
            default_zone: &self.default_zone,
            user,
            now,
        }
    }

    /// Raise one check-in notice with the kernel.
    async fn notify(&self, notice: &tools::Notice) -> Result<(), String> {
        let response = self
            .kernel
            .post(format!("{}/api/notify", self.cfg.kernel_url))
            .bearer_auth(&self.cfg.service_token)
            .json(&json!({
                "category": CHECKIN_CATEGORY,
                "title": notice.title,
                "body": notice.body,
                "channels": ["in-app"],
                "subject": checkins::notice_subject(&notice.user),
            }))
            .send()
            .await
            .map_err(|e| format!("the kernel could not be reached: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "the kernel refused the notice ({})",
                response.status().as_u16()
            ))
        }
    }

    /// The person answered (or turned check-ins off): their unread check-in
    /// notice comes down. Best effort; a newer check-in replaces it anyway.
    async fn retire(&self, user: &str) {
        if !self.cfg.notices_for.iter().any(|u| u == user) {
            return;
        }
        let result = self
            .kernel
            .post(format!("{}/api/notify/retire", self.cfg.kernel_url))
            .bearer_auth(&self.cfg.service_token)
            .json(&json!({ "subject": checkins::notice_subject(user) }))
            .send()
            .await;
        match result {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => eprintln!(
                "[body-double] the kernel refused to retire {user}'s check-in notice ({})",
                r.status().as_u16()
            ),
            Err(e) => eprintln!("[body-double] retiring {user}'s check-in notice failed: {e}"),
        }
    }
}

pub fn build_app(cfg: AppConfig) -> Result<(Router, Arc<AppState>), String> {
    std::fs::create_dir_all(&cfg.state_dir)
        .map_err(|e| format!("{}: {e}", cfg.state_dir.display()))?;
    let store = Store::open(&cfg.state_dir.join(DB_FILE))?;
    let default_zone = Zone::load(&cfg.timezone).unwrap_or_else(|_| Zone::utc());
    let app = Arc::new(AppState {
        identity_pem: Mutex::new(None),
        kernel: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("http client"),
        store: Mutex::new(store),
        default_zone,
        fixed_now: AtomicI64::new(0),
        cfg,
    });
    let router = Router::new()
        .route("/health", get(health))
        .route("/event", post(event))
        .route("/tools/{name}", post(call_tool))
        .with_state(app.clone());
    Ok((router, app))
}

async fn health() -> Json<Value> {
    Json(json!({}))
}

async fn event(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(_body): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_identity(&app, &headers, IdentityPurpose::Event, None)?;
    Ok(Json(json!({ "ok": true })))
}

fn require_identity(
    app: &AppState,
    headers: &HeaderMap,
    purpose: IdentityPurpose,
    effect: Option<&str>,
) -> Result<Claims, ApiError> {
    let pem = app
        .identity_pem
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| err(503, "not registered yet"))?;
    let token = headers
        .get("x-greenhouse-identity")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| err(401, "missing identity"))?;
    match verify_app_request(&pem, token, APP_ID, purpose, effect) {
        Ok(claims) => Ok(claims),
        Err(IdentityError::Invalid) => Err(err(401, "invalid identity token")),
        Err(IdentityError::Audience) => {
            Err(err(403, "identity audience does not match body-double"))
        }
        Err(IdentityError::Purpose) => Err(err(403, "identity endpoint purpose does not match")),
        Err(IdentityError::Effect) => Err(err(403, "identity lacks the required effect")),
        Err(IdentityError::Subject) => Err(err(
            403,
            "identity subject is not authorized for this endpoint purpose",
        )),
    }
}

async fn call_tool(
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(args): Json<Value>,
) -> ApiResult<Json<Value>> {
    let effect = tool_effect(&name).ok_or_else(|| err(404, format!("unknown tool '{name}'")))?;
    let claims = require_identity(&app, &headers, IdentityPurpose::Tool, Some(effect))?;
    let now = app.now();

    if name == "checkin_tick" {
        // Only the kernel's scheduler runs it, never a person, even one
        // holding command.
        if claims.s != IdentitySubjectKind::Scheduler {
            return Err(err(403, "checkin_tick is run by the kernel's scheduler"));
        }
        return run_tick(&app, &args, now).await.map(Json);
    }
    if claims.s != IdentitySubjectKind::Human {
        return Err(err(403, "body-double tools act for a person"));
    }

    let user = claims.u.as_str();
    let ctx = app.ctx(user, now);
    if effect == "read" {
        let store = app.store.lock().unwrap();
        let result = match name.as_str() {
            "now" => tools::now(&store, &ctx, &args),
            "day" => tools::day(&store, &ctx, &args),
            _ => tools::export(&store, &ctx, &args),
        };
        return result.map(Json);
    }

    let outcome = {
        let mut store = app.store.lock().unwrap();
        match name.as_str() {
            "log" => tools::log(&mut store, &ctx, &args),
            "set_task" => tools::set_task(&mut store, &ctx, &args),
            "update_task" => tools::update_task(&mut store, &ctx, &args),
            "open_thread" => tools::open_thread(&mut store, &ctx, &args),
            "close_thread" => tools::close_thread(&mut store, &ctx, &args),
            _ => tools::set_checkins(&mut store, &ctx, &args),
        }?
    };
    if outcome.retire {
        app.retire(user).await;
    }
    Ok(Json(outcome.result))
}

async fn run_tick(app: &AppState, args: &Value, now: i64) -> ApiResult<Value> {
    tools::Args::new(args, &[])?;
    let tick = {
        let mut store = app.store.lock().unwrap();
        tools::tick(&mut store, &app.cfg, &app.default_zone, now)?
    };
    let mut notified = 0;
    let mut errors = Vec::new();
    for notice in &tick.notices {
        match app.notify(notice).await {
            Ok(()) => {
                notified += 1;
                if let Err(e) = app.store.lock().unwrap().mark_notified(notice.checkin_id) {
                    errors.push(format!(
                        "{}: {}",
                        notice.user,
                        tools::store_error(e).message
                    ));
                }
            }
            // The check-in stays raised (the page and `now` show it due);
            // only the notice is missing.
            Err(e) => errors.push(format!("{}: {e}", notice.user)),
        }
    }
    Ok(json!({
        "due": tick.due,
        "raised": tick.raised,
        "notified": notified,
        "deferred_for_quiet": tick.deferred,
        "paused": tick.paused,
        "errors": errors,
    }))
}

pub async fn register(app: &Arc<AppState>) -> Result<(), String> {
    let response = app
        .kernel
        .post(format!("{}/api/apps/register", app.cfg.kernel_url))
        .bearer_auth(&app.cfg.service_token)
        .json(&json!({ "manifest": manifest() }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(value["error"]
            .as_str()
            .unwrap_or("registration failed")
            .to_string());
    }
    if let Some(pem) = value["identity_public_key"].as_str() {
        *app.identity_pem.lock().unwrap() = Some(pem.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_is_one_the_kernel_accepts() {
        let m = greenhouse_kernel::validate::validate_manifest(&manifest()).unwrap();
        assert_eq!(m.id, "body-double");
        assert!(m.permissions.egress.is_empty());
        assert!(m.permissions.secrets.is_empty());
        assert!(m.permissions.model_compute.is_empty());
        assert_eq!(m.permissions.notify, vec!["in-app"]);
        assert_eq!(m.schedules.len(), 1);
        assert_eq!(m.schedules[0].action, "checkin_tick");
        assert!(m.ui.is_none());
    }

    #[test]
    fn every_declared_tool_has_the_effect_the_router_enforces() {
        let m = manifest();
        let tools = m["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 10);
        for t in tools {
            let name = t["name"].as_str().unwrap();
            assert_eq!(t["effect"].as_str(), tool_effect(name), "{name}");
        }
        // Only the scheduled tick is a command; people need read and write.
        let commands: Vec<_> = tools
            .iter()
            .filter(|t| t["effect"] == "command")
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(commands, vec!["checkin_tick"]);
        assert_eq!(tool_effect("drop_tables"), None);
    }
}
