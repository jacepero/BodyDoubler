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
// call, and none of the person's words are in them.

pub mod checkins;
pub mod config;
pub mod store;
pub mod time;
pub mod tools;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use config::AppConfig;
use greenhouse_kernel::clock::Zone;
use greenhouse_kernel::error::{err, ApiError, ApiResult};
use greenhouse_kernel::identity::{
    verify_app_request, Claims, IdentityError, IdentityPurpose, IdentitySubjectKind,
};
use serde_json::{json, Value};
use std::collections::HashMap;
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
        "now" | "day" => Some("read"),
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
    json!({"type": "object", "properties": {}})
}

/// What the app registers. The descriptions steer the assistant the way the
/// iOS system prompt did, so they are part of the approved contract. Every
/// chat turn carries the person's callable tool schemas, so they are kept
/// short: the app itself refuses unknown or out-of-range arguments.
pub fn manifest() -> Value {
    // Plain types and lengths only: tool schemas may become sampling grammars
    // on the inference node, where regex patterns are not dependable.
    let id = json!({"type": "integer", "minimum": 1});
    let date = json!({"type": "string", "maxLength": 10});
    let clock = json!({"type": "string", "maxLength": 5});
    let text = |max: u32| json!({"type": "string", "maxLength": max});
    let object = |properties: Value, required: &[&str]| {
        let mut schema = json!({"type": "object", "properties": properties});
        if !required.is_empty() {
            schema["required"] = json!(required);
        }
        schema
    };
    json!({
        "id": APP_ID,
        "version": APP_VERSION,
        "health": "/health",
        "tools": [
            tool("now",
                "Body double: the person's current task and how long it has run, open loose ends, recent journal entries and check-ins. Read it when they say what they are doing; reply in one or two short sentences, without flattery or lectures.",
                no_arguments()),
            tool("day",
                "Body double journal for one day (date YYYY-MM-DD, default today), for recaps.",
                object(json!({"date": date}), &[])),
            tool("log",
                "Body double: record what the person says they are doing, in their own words (text), with a one-line summary.",
                object(json!({"text": text(2000), "summary": text(200)}), &["text"])),
            tool("set_task",
                "Body double: the person started or switched to a task; call before log. The previous one is paused.",
                object(json!({"title": text(200)}), &["title"])),
            tool("update_task",
                "Body double: set a task's status by id: done, dropped, paused, or active to resume it.",
                object(json!({"task_id": id, "status": {"type": "string", "enum": ["active", "paused", "done", "dropped"]}}), &["task_id", "status"])),
            tool("open_thread",
                "Body double: remember a loose end the person mentions.",
                object(json!({"note": text(300)}), &["note"])),
            tool("close_thread",
                "Body double: close a loose end by id.",
                object(json!({"thread_id": id}), &["thread_id"])),
            tool("checkins",
                "Body double check-ins: on, every_minutes, quiet hours (quiet_from, quiet_until as HH:MM; quiet false for none), timezone, or next_in_minutes for one at a set time.",
                object(json!({
                    "on": {"type": "boolean"},
                    "every_minutes": {"type": "integer", "minimum": 10, "maximum": 240},
                    "quiet": {"type": "boolean"},
                    "quiet_from": clock,
                    "quiet_until": clock,
                    "timezone": text(64),
                    "next_in_minutes": {"type": "integer", "minimum": 5, "maximum": 480}
                }), &[])),
            tool("checkin_tick",
                "Scheduled, not for chat: raises the body-double check-ins that are due.",
                no_arguments()),
        ],
        "events": { "emits": [], "subscribes": [] },
        // Every five minutes, as Home Assistant's house check: a check-in may
        // be up to five minutes late, and the kernel's audit is not flooded.
        "schedules": [{ "name": "checkins.tick", "cron": "*/5 * * * *", "action": "checkin_tick" }],
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
        if !self.cfg.notices(user) {
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
        .route("/export", get(export_file))
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
            _ => tools::day(&store, &ctx, &args),
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

/// The journal as a download, through the kernel's app proxy
/// (`/apps/body-double/export?format=markdown|json&from=&to=`), for the
/// person whose browser asks. A file, not a tool: no model context and no
/// tool log, and it changes nothing.
async fn export_file(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let claims = require_identity(&app, &headers, IdentityPurpose::Data, Some("read"))?;
    let args = Value::Object(
        query
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    );
    let now = app.now();
    let file = {
        let store = app.store.lock().unwrap();
        tools::export(&store, &app.ctx(&claims.u, now), &args)?
    };
    let disposition = format!("attachment; filename=\"{}\"", file.filename);
    let mut response = file.content.into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(file.content_type),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(|_| err(500, "bad file name"))?,
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

async fn run_tick(app: &AppState, args: &Value, now: i64) -> ApiResult<Value> {
    tools::Args::new(args, &[])?;
    let tick = {
        let mut store = app.store.lock().unwrap();
        tools::tick(&mut store, &app.cfg, &app.default_zone, now)?
    };
    let mut errors = tick.errors;
    let recorded = errors.is_empty();
    let mut notified = 0;
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
            // The check-in stays raised (the page and `now` show it due) and
            // its notice is sent again at the next tick.
            Err(e) => errors.push(format!("{}: {e}", notice.user)),
        }
    }
    let summary = format!(
        "{} due, {} raised, {} notified, {} held for quiet hours",
        tick.due, tick.raised, notified, tick.deferred
    );
    if !errors.is_empty() {
        // Not a success: the kernel's tool audit and Activity show the call
        // as failed, as for the house check.
        let message = format!("check-in tick: {summary}; {}", errors.join("; "));
        eprintln!("[body-double] {message}");
        return Err(err(if recorded { 502 } else { 500 }, message));
    }
    Ok(json!({
        "due": tick.due,
        "raised": tick.raised,
        "notified": notified,
        "deferred_for_quiet": tick.deferred,
        "paused": tick.paused,
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
        assert_eq!(tools.len(), 9);
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
