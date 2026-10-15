// The app end to end against a simulated kernel: real HTTP, the real identity
// verifier, a real journal on disk, and a fixed clock so check-ins can be
// walked through minute by minute. The kernel stand-in records the notices
// the app raises and retires.

use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use body_double_app::config::{AppConfig, PlanDefaults};
use body_double_app::{build_app, manifest, AppState, CHECKIN_CATEGORY};
use greenhouse_kernel::clock;
use greenhouse_kernel::identity::{create_identity, Identity, IdentityPurpose, IdentitySubject};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const CREDENTIAL: &str = "app-service-credential-0123456789abcdef";

struct Harness {
    app: Arc<AppState>,
    url: String,
    /// (path, body) of every call the app made to the kernel.
    kernel_calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// Make the kernel refuse notices.
    kernel_down: Arc<Mutex<bool>>,
    identity: Identity,
    http: reqwest::Client,
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

/// 2026-10-15 (a Thursday) at hh:mm UTC.
fn at(h: i64, m: i64) -> i64 {
    clock::days_from_civil(2026, 10, 15) * 86_400 + h * 3600 + m * 60
}

static HARNESSES: AtomicUsize = AtomicUsize::new(0);

async fn harness() -> Harness {
    harness_with(vec!["owner".into()]).await
}

async fn harness_with(notices_for: Vec<String>) -> Harness {
    let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let down = Arc::new(Mutex::new(false));
    let route = |path: &'static str| {
        let calls = calls.clone();
        let down = down.clone();
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let calls = calls.clone();
            let down = down.clone();
            async move {
                assert_eq!(
                    headers.get("authorization").unwrap().to_str().unwrap(),
                    format!("Bearer {CREDENTIAL}")
                );
                calls.lock().unwrap().push((path.to_string(), body));
                if *down.lock().unwrap() {
                    return (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error": "down"})),
                    );
                }
                (axum::http::StatusCode::OK, Json(json!({ "ok": true })))
            }
        })
    };
    let kernel_url = serve(
        Router::new()
            .route("/api/notify", route("/api/notify"))
            .route("/api/notify/retire", route("/api/notify/retire")),
    )
    .await;

    let state_dir = std::env::temp_dir().join(format!(
        "gh-body-double-test-{}-{}",
        std::process::id(),
        HARNESSES.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&state_dir);
    let cfg = AppConfig {
        kernel_url,
        state_dir,
        port: 0,
        service_token: CREDENTIAL.into(),
        timezone: "UTC".into(),
        notices_for,
        defaults: PlanDefaults::default(),
    };
    let (router, app) = build_app(cfg).unwrap();
    let identity = create_identity(&[7u8; 32]);
    *app.identity_pem.lock().unwrap() = Some(identity.public_key_pem.clone());
    app.fixed_now.store(at(14, 0), Ordering::SeqCst);
    let url = serve(router).await;
    Harness {
        app,
        url,
        kernel_calls: calls,
        kernel_down: down,
        identity,
        http: reqwest::Client::new(),
    }
}

impl Harness {
    fn set_now(&self, t: i64) {
        self.app.fixed_now.store(t, Ordering::SeqCst);
    }

    fn token(&self, subject: IdentitySubject, grants: &[&str]) -> String {
        let grants: Vec<String> = grants.iter().map(|g| g.to_string()).collect();
        self.identity
            .mint(subject, "body-double", IdentityPurpose::Tool, &grants, 300)
    }

    async fn post(&self, tool: &str, token: Option<String>, args: Value) -> (u16, Value) {
        let mut request = self.http.post(format!("{}/tools/{tool}", self.url));
        if let Some(token) = token {
            request = request.header("x-greenhouse-identity", token);
        }
        let response = request.json(&args).send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// A tool called as `user`, holding read and write as people do.
    async fn call_as(&self, user: &str, tool: &str, args: Value) -> (u16, Value) {
        let token = self.token(IdentitySubject::Human(user.into()), &["read", "write"]);
        self.post(tool, Some(token), args).await
    }

    async fn call(&self, tool: &str, args: Value) -> Value {
        let (status, body) = self.call_as("owner", tool, args).await;
        assert_eq!(status, 200, "{tool}: {body}");
        body
    }

    async fn tick(&self) -> Value {
        let token = self.token(IdentitySubject::Scheduler, &["command"]);
        let (status, body) = self.post("checkin_tick", Some(token), json!({})).await;
        assert_eq!(status, 200, "tick: {body}");
        body
    }

    fn notices(&self) -> Vec<Value> {
        self.kernel_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/notify")
            .map(|(_, body)| body.clone())
            .collect()
    }

    fn retired(&self) -> Vec<Value> {
        self.kernel_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/notify/retire")
            .map(|(_, body)| body.clone())
            .collect()
    }
}

#[tokio::test]
async fn narrating_a_switch_pauses_the_last_task_and_keeps_the_words() {
    let h = harness().await;
    h.call(
        "set_task",
        json!({"title": "Refactor IVR callback handler"}),
    )
    .await;
    h.call("log", json!({"text": "started refactor after standup"}))
        .await;

    // Two hours and ten minutes later the person switches, in chat.
    h.set_now(at(16, 10));
    let switched = h.call("set_task", json!({"title": "WGU assignment"})).await;
    assert_eq!(switched["current_task"]["title"], "WGU assignment");
    assert_eq!(switched["paused"]["title"], "Refactor IVR callback handler");
    assert_eq!(switched["paused"]["minutes"], 130);
    assert_eq!(switched["resumed"], false);
    let logged = h
        .call(
            "log",
            json!({"text": "ok switching to the WGU assignment for a bit", "summary": "Switched to WGU", "mood": null}),
        )
        .await;
    assert_eq!(
        logged["logged"]["text"],
        "ok switching to the WGU assignment for a bit"
    );
    assert_eq!(logged["logged"]["task"], "WGU assignment");
    assert_eq!(logged["logged"]["at"], "16:10");

    h.set_now(at(16, 53));
    let now = h.call("now", json!({})).await;
    assert_eq!(now["now"], "Thu 15 Oct 16:53");
    assert_eq!(now["current_task"]["title"], "WGU assignment");
    assert_eq!(now["current_task"]["for"], "43m");
    assert_eq!(now["current_task"]["since"], "16:10");
    assert_eq!(now["minutes_since_last_entry"], 43);
    assert_eq!(
        now["paused_tasks"][0]["title"],
        "Refactor IVR callback handler"
    );
    let recent: Vec<_> = now["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        recent,
        vec![
            "started refactor after standup",
            "ok switching to the WGU assignment for a bit"
        ]
    );
    assert_eq!(now["recent"][0]["at"], "14:00");

    // Back to the refactor by name: resumed, not duplicated.
    let back = h
        .call(
            "set_task",
            json!({"title": "refactor ivr callback handler"}),
        )
        .await;
    assert_eq!(back["resumed"], true);
    assert_eq!(back["paused"]["title"], "WGU assignment");
}

#[tokio::test]
async fn tasks_end_and_loose_ends_close_by_id() {
    let h = harness().await;
    let task = h.call("set_task", json!({"title": "Inbox zero"})).await;
    let id = task["current_task"]["id"].as_i64().unwrap();
    let thread = h
        .call(
            "open_thread",
            json!({"note": "email Dana re: schema review"}),
        )
        .await;
    assert_eq!(thread["open_threads"], 1);
    let thread_id = thread["thread"]["id"].as_i64().unwrap();

    let done = h
        .call("update_task", json!({"task_id": id, "status": "done"}))
        .await;
    assert_eq!(done["task"]["status"], "done");
    assert_eq!(done["current_task"], Value::Null);

    let closed = h
        .call("close_thread", json!({"thread_id": thread_id}))
        .await;
    assert_eq!(closed["open_threads"], 0);
    assert_eq!(closed["already_closed"], false);
    let again = h
        .call("close_thread", json!({"thread_id": thread_id}))
        .await;
    assert_eq!(again["already_closed"], true);

    let (status, body) = h
        .call_as(
            "owner",
            "update_task",
            json!({"task_id": 999, "status": "done"}),
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "no task 999");
}

#[tokio::test]
async fn the_day_reads_back_in_order_for_a_recap() {
    let h = harness().await;
    h.set_now(at(9, 2));
    h.call("set_task", json!({"title": "Standup notes"})).await;
    h.call(
        "log",
        json!({"text": "writing standup notes", "mood": "tired"}),
    )
    .await;
    h.set_now(at(9, 30));
    h.call("open_thread", json!({"note": "book dentist"})).await;
    h.call("set_task", json!({"title": "Review PR 41"})).await;
    h.call("log", json!({"text": "reviewing PR 41"})).await;

    let day = h.call("day", json!({})).await;
    assert_eq!(day["date"], "2026-10-15");
    assert_eq!(day["label"], "Thu 15 Oct");
    let entries = day["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["time"], "09:02");
    assert_eq!(entries[0]["mood"], "tired");
    assert_eq!(entries[0]["task"], "Standup notes");
    assert_eq!(entries[1]["task"], "Review PR 41");
    assert_eq!(day["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(day["threads_opened"][0]["note"], "book dentist");
    assert_eq!(day["open_threads"][0]["note"], "book dentist");
    assert_eq!(day["truncated"], false);

    let other = h.call("day", json!({"date": "2026-10-14"})).await;
    assert!(other["entries"].as_array().unwrap().is_empty());
    let (status, _) = h
        .call_as("owner", "day", json!({"date": "yesterday"}))
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn people_only_ever_see_their_own_journal() {
    let h = harness().await;
    h.call("set_task", json!({"title": "Owner's taxes"})).await;
    let thread = h
        .call("open_thread", json!({"note": "call the bank"}))
        .await;
    let thread_id = thread["thread"]["id"].as_i64().unwrap();

    let (status, theirs) = h.call_as("guest", "now", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(theirs["current_task"], Value::Null);
    assert!(theirs["open_threads"].as_array().unwrap().is_empty());

    let (status, body) = h
        .call_as("guest", "close_thread", json!({"thread_id": thread_id}))
        .await;
    assert_eq!(status, 404, "{body}");
    let mine = h.call("now", json!({})).await;
    assert_eq!(mine["open_threads"][0]["note"], "call the bank");
}

#[tokio::test]
async fn tools_need_the_right_effect_and_a_kernel_identity() {
    let h = harness().await;
    let owner = || IdentitySubject::Human("owner".into());

    // Effects are not cumulative: write does not imply read.
    let (status, _) = h
        .post("now", Some(h.token(owner(), &["write"])), json!({}))
        .await;
    assert_eq!(status, 403);
    let (status, _) = h
        .post(
            "log",
            Some(h.token(owner(), &["read"])),
            json!({"text": "hello"}),
        )
        .await;
    assert_eq!(status, 403);

    let (status, _) = h.post("now", None, json!({})).await;
    assert_eq!(status, 401, "no identity");
    let (status, _) = h
        .post("now", Some("v1.forged.token".into()), json!({}))
        .await;
    assert_eq!(status, 401);
    let elsewhere = h.identity.mint(
        owner(),
        "weather",
        IdentityPurpose::Tool,
        &["read".into()],
        300,
    );
    let (status, _) = h.post("now", Some(elsewhere), json!({})).await;
    assert_eq!(status, 403, "another app's audience");

    // Only the scheduler runs the tick, even for someone holding command; and
    // the scheduler cannot act as a person.
    let (status, _) = h
        .post(
            "checkin_tick",
            Some(h.token(owner(), &["command"])),
            json!({}),
        )
        .await;
    assert_eq!(status, 403);
    let (status, _) = h
        .post(
            "log",
            Some(h.token(IdentitySubject::Scheduler, &["command", "write"])),
            json!({"text": "x"}),
        )
        .await;
    assert_eq!(status, 403);

    let (status, body) = h.call_as("owner", "drop_tables", json!({})).await;
    assert_eq!(status, 404, "{body}");
}

#[tokio::test]
async fn invalid_arguments_are_refused_and_nothing_is_stored() {
    let h = harness().await;
    for (tool, args) in [
        ("log", json!({})),
        ("log", json!({"text": "   "})),
        ("log", json!({"text": "hi", "feeling": "fine"})),
        ("log", json!({"text": "x".repeat(2001)})),
        ("log", json!({"text": "hi", "mood": 3})),
        ("set_task", json!({"title": ""})),
        ("update_task", json!({"task_id": 1, "status": "finished"})),
        ("update_task", json!({"task_id": "1", "status": "done"})),
        ("close_thread", json!({"thread_id": 0})),
        ("checkins", json!({"every_minutes": 5})),
        (
            "checkins",
            json!({"quiet_from": "10pm", "quiet_until": "08:00"}),
        ),
        ("checkins", json!({"timezone": "Mars/Olympus_Mons"})),
        ("checkins", json!({"on": false, "next_in_minutes": 20})),
        ("now", json!({"verbose": true})),
        ("now", json!([1, 2])),
    ] {
        let (status, body) = h.call_as("owner", tool, args.clone()).await;
        assert_eq!(status, 400, "{tool} {args}: {body}");
        assert!(body["error"].is_string(), "{tool}: {body}");
    }
    let day = h.call("day", json!({})).await;
    assert!(day["entries"].as_array().unwrap().is_empty());
    assert_eq!(h.call("now", json!({})).await["current_task"], Value::Null);
}

#[tokio::test]
async fn a_quiet_person_gets_a_check_in_and_answering_retires_it() {
    let h = harness().await;
    h.call("set_task", json!({"title": "WGU assignment"})).await;
    h.call(
        "open_thread",
        json!({"note": "email Dana re: schema review"}),
    )
    .await;
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["on"], true);
    assert_eq!(now["checkins"]["next"], "14:45");
    assert_eq!(now["checkins"]["due"], false);
    assert_eq!(now["checkins"]["notices"], true);

    h.set_now(at(14, 44));
    assert_eq!(h.tick().await["raised"], 0);
    h.set_now(at(14, 45));
    let tick = h.tick().await;
    assert_eq!(tick["raised"], 1, "{tick}");
    assert_eq!(tick["notified"], 1, "{tick}");
    let notices = h.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["category"], CHECKIN_CATEGORY);
    assert_eq!(notices[0]["title"], "Check-in");
    assert_eq!(
        notices[0]["body"],
        "Still on WGU assignment? 45m in.\nLoose end: email Dana re: schema review"
    );
    assert_eq!(notices[0]["channels"], json!(["in-app"]));
    assert_eq!(notices[0]["subject"], "body-double/owner/checkin");

    // The same minute again raises nothing.
    assert_eq!(h.tick().await["raised"], 0);
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["due"], true);
    assert_eq!(now["checkins"]["next"], "15:30");

    // Saying anything answers it and takes the notice down.
    h.set_now(at(14, 50));
    let logged = h.call("log", json!({"text": "still on it"})).await;
    assert_eq!(logged["answered_checkin"], true);
    assert_eq!(
        h.retired(),
        vec![json!({"subject": "body-double/owner/checkin"})]
    );
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["due"], false);
    assert_eq!(now["checkins"]["next"], "15:35");
    let day = h.call("day", json!({})).await;
    assert_eq!(day["checkins"], json!({"raised": 1, "answered": 1}));
}

#[tokio::test]
async fn quiet_hours_hold_check_ins_until_morning() {
    let h = harness().await;
    h.set_now(at(21, 30));
    h.call("log", json!({"text": "winding down"})).await;
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["quiet"], "22:00-08:00");
    assert_eq!(now["checkins"]["next"], "Fri 16 Oct 08:00");
    h.set_now(at(22, 15));
    assert_eq!(h.tick().await["raised"], 0);
    assert!(h.notices().is_empty());

    // Asked for at a set time, it comes even in quiet hours.
    h.set_now(at(23, 0));
    let set = h.call("checkins", json!({"next_in_minutes": 25})).await;
    assert_eq!(set["checkins"]["next"], "23:25");
    h.set_now(at(23, 25));
    let tick = h.tick().await;
    assert_eq!(tick["raised"], 1, "{tick}");
    assert_eq!(h.notices()[0]["body"], "What are you working on?");
}

#[tokio::test]
async fn eight_unanswered_check_ins_and_the_chain_rests() {
    let h = harness().await;
    h.call("checkins", json!({"quiet": false, "every_minutes": 10}))
        .await;
    h.call("log", json!({"text": "starting"})).await;
    let mut t = at(14, 0);
    for _ in 0..8 {
        t += 600;
        h.set_now(t);
        assert_eq!(h.tick().await["raised"], 1);
    }
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["unanswered"], 8);
    assert_eq!(now["checkins"]["next"], Value::Null);
    h.set_now(t + 3600);
    assert_eq!(
        h.tick().await["raised"],
        0,
        "resting until the person is back"
    );
    h.call("log", json!({"text": "back"})).await;
    assert!(!h.call("now", json!({})).await["checkins"]["next"].is_null());
}

#[tokio::test]
async fn check_ins_can_be_turned_off_and_tuned() {
    let h = harness().await;
    h.call("log", json!({"text": "starting"})).await;
    let set = h
        .call(
            "checkins",
            json!({"every_minutes": 30, "quiet_from": "12:00", "quiet_until": "13:00", "timezone": "America/New_York"}),
        )
        .await;
    let c = &set["checkins"];
    assert_eq!(c["every_minutes"], 30);
    assert_eq!(c["quiet"], "12:00-13:00");
    assert_eq!(c["timezone"], "America/New_York");
    assert!(!c["next"].is_null());

    let off = h.call("checkins", json!({"on": false})).await;
    assert_eq!(off["checkins"]["on"], false);
    assert_eq!(off["checkins"]["next"], Value::Null);
    h.set_now(at(20, 0));
    assert_eq!(h.tick().await["raised"], 0);
    h.call("log", json!({"text": "still here"})).await;
    assert_eq!(
        h.call("now", json!({})).await["checkins"]["next"],
        Value::Null
    );

    let on = h.call("checkins", json!({"on": true})).await;
    assert_eq!(on["checkins"]["on"], true);
    assert!(!on["checkins"]["next"].is_null());
}

#[tokio::test]
async fn people_outside_notices_for_see_check_ins_only_in_their_own_view() {
    let h = harness_with(vec![]).await;
    h.call("log", json!({"text": "working"})).await;
    h.set_now(at(14, 45));
    let tick = h.tick().await;
    assert_eq!(tick["raised"], 1);
    assert_eq!(tick["notified"], 0);
    assert!(h.notices().is_empty(), "no household notice");
    let now = h.call("now", json!({})).await;
    assert_eq!(now["checkins"]["due"], true);
    assert_eq!(now["checkins"]["notices"], false);
    h.call("log", json!({"text": "here"})).await;
    assert!(h.retired().is_empty());
}

#[tokio::test]
async fn a_refused_notice_still_leaves_the_check_in_raised() {
    let h = harness().await;
    h.call("log", json!({"text": "working"})).await;
    *h.kernel_down.lock().unwrap() = true;
    h.set_now(at(14, 45));
    let tick = h.tick().await;
    assert_eq!(tick["raised"], 1);
    assert_eq!(tick["notified"], 0);
    assert_eq!(tick["errors"].as_array().unwrap().len(), 1, "{tick}");
    assert_eq!(h.call("now", json!({})).await["checkins"]["due"], true);
}

#[tokio::test]
async fn the_journal_downloads_as_markdown_or_json_through_the_proxy() {
    let h = harness().await;
    h.set_now(at(9, 0));
    h.call("set_task", json!({"title": "Inbox"})).await;
    h.call(
        "log",
        json!({"text": "clearing email", "summary": "Email", "mood": "focused"}),
    )
    .await;
    h.call("open_thread", json!({"note": "reply to Sam"})).await;

    // As the kernel's proxy relays a browser's GET: purpose data, the
    // person's effects.
    let data = |grants: &[&str]| {
        let grants: Vec<String> = grants.iter().map(|g| g.to_string()).collect();
        h.identity.mint(
            IdentitySubject::Human("owner".into()),
            "body-double",
            IdentityPurpose::Data,
            &grants,
            300,
        )
    };
    let get = |query: &str, token: String| {
        h.http
            .get(format!("{}/export{query}", h.url))
            .header("x-greenhouse-identity", token)
            .send()
    };

    let md = get("", data(&["read", "write"])).await.unwrap();
    assert_eq!(md.status().as_u16(), 200);
    assert_eq!(
        md.headers()["content-disposition"],
        "attachment; filename=\"body-double-2026-10-15.md\""
    );
    assert_eq!(md.headers()["content-type"], "text/markdown; charset=utf-8");
    assert_eq!(md.headers()["cache-control"], "no-store");
    let content = md.text().await.unwrap();
    assert!(content.starts_with("# Body double journal"));
    assert!(content.contains("## 2026-10-15 (Thu 15 Oct)"));
    assert!(content.contains("- **09:00** clearing email — _Email_ `[focused]` (Inbox)"));
    assert!(content.contains("- [active] Inbox ← current"));
    assert!(content.contains("- [open] reply to Sam"));

    let js = get(
        "?format=json&from=2026-10-15&to=2026-10-15",
        data(&["read"]),
    )
    .await
    .unwrap();
    assert_eq!(
        js.headers()["content-disposition"],
        "attachment; filename=\"body-double-2026-10-15-to-2026-10-15.json\""
    );
    let doc: Value = js.json().await.unwrap();
    assert_eq!(doc["entries"][0]["at"], "2026-10-15T09:00:00Z");
    assert_eq!(doc["entries"][0]["task"], "Inbox");
    assert_eq!(doc["tasks"][0]["status"], "active");
    assert_eq!(doc["threads"][0]["closed_at"], Value::Null);
    assert_eq!(doc["timezone"], "UTC");

    // Read is needed; a tool token is not a browser's; arguments are checked.
    let denied = get("", data(&["write"])).await.unwrap();
    assert_eq!(denied.status().as_u16(), 403);
    let tool_token = h.token(IdentitySubject::Human("owner".into()), &["read"]);
    assert_eq!(get("", tool_token).await.unwrap().status().as_u16(), 403);
    let bad = get("?format=pdf", data(&["read"])).await.unwrap();
    assert_eq!(bad.status().as_u16(), 400);
    let unknown = get("?everything=1", data(&["read"])).await.unwrap();
    assert_eq!(unknown.status().as_u16(), 400);
    // Export is not a tool: it never goes into a chat turn.
    let (status, _) = h.call_as("owner", "export", json!({})).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn the_day_is_sized_for_a_chat_turn() {
    let h = harness().await;
    for i in 0..70 {
        h.set_now(at(8, 0) + i * 60);
        h.call(
            "log",
            json!({"text": format!("entry {i}: {}", "x".repeat(400))}),
        )
        .await;
    }
    let day = h.call("day", json!({})).await;
    let entries = day["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 60);
    assert!(
        entries[59]["text"]
            .as_str()
            .unwrap()
            .starts_with("entry 69:"),
        "the newest"
    );
    assert!(entries[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("entry 10:"));
    assert_eq!(entries[0]["text"].as_str().unwrap().chars().count(), 240);
    assert_eq!(day["earlier"], 10);
    assert_eq!(day["truncated"], true);
    assert!(
        day.to_string().len() < 24 * 1024,
        "{} bytes",
        day.to_string().len()
    );
}

#[tokio::test]
async fn the_journal_survives_a_restart() {
    let h = harness().await;
    h.call("set_task", json!({"title": "Survive restarts"}))
        .await;
    h.call("log", json!({"text": "before the restart"})).await;
    let cfg = h.app.cfg.clone();
    drop(h);
    let (_router, app) = build_app(cfg).unwrap();
    let store = app.store.lock().unwrap();
    assert_eq!(
        store.active_task("owner").unwrap().unwrap().title,
        "Survive restarts"
    );
    assert_eq!(store.recent_entries("owner", 5).unwrap().len(), 1);
}

#[test]
fn the_registered_manifest_validates() {
    let m = greenhouse_kernel::validate::validate_manifest(&manifest()).unwrap();
    let effects: Vec<(String, String)> = m
        .tools
        .iter()
        .map(|t| (t.name.clone(), t.effect.clone()))
        .collect();
    assert!(effects.contains(&("log".into(), "write".into())));
    assert!(effects.contains(&("now".into(), "read".into())));
    assert!(effects.contains(&("checkin_tick".into(), "command".into())));
    assert_eq!(
        m.permissions.notify_categories[0].category,
        CHECKIN_CATEGORY
    );
    assert_eq!(m.permissions.notify_categories[0].sensitivity, "private");
}

#[tokio::test]
async fn registration_sends_a_valid_manifest_and_learns_the_kernel_key() {
    let identity = create_identity(&[9u8; 32]);
    let pem = identity.public_key_pem.clone();
    let seen: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let kernel_seen = seen.clone();
    let kernel_url = serve(Router::new().route(
        "/api/apps/register",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let seen = kernel_seen.clone();
            let pem = pem.clone();
            async move {
                assert_eq!(
                    headers.get("authorization").unwrap().to_str().unwrap(),
                    format!("Bearer {CREDENTIAL}")
                );
                // The kernel validates what it is sent before anything else.
                greenhouse_kernel::validate::validate_manifest(&body["manifest"]).unwrap();
                *seen.lock().unwrap() = Some(body);
                Json(json!({"ok": true, "identity_public_key": pem}))
            }
        }),
    ))
    .await;
    let state_dir = std::env::temp_dir().join(format!(
        "gh-body-double-test-{}-{}",
        std::process::id(),
        HARNESSES.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&state_dir);
    let (router, app) = build_app(AppConfig {
        kernel_url,
        state_dir,
        port: 0,
        service_token: CREDENTIAL.into(),
        timezone: "UTC".into(),
        notices_for: vec![],
        defaults: PlanDefaults::default(),
    })
    .unwrap();
    let url = serve(router).await;

    // Before registering, the app cannot verify anyone.
    let token = identity.mint(
        IdentitySubject::Human("owner".into()),
        "body-double",
        IdentityPurpose::Tool,
        &["read".into()],
        300,
    );
    let http = reqwest::Client::new();
    let early = http
        .post(format!("{url}/tools/now"))
        .header("x-greenhouse-identity", token.clone())
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(early.status().as_u16(), 503);

    body_double_app::register(&app).await.unwrap();
    assert_eq!(
        seen.lock().unwrap().as_ref().unwrap()["manifest"]["id"],
        "body-double"
    );
    let after = http
        .post(format!("{url}/tools/now"))
        .header("x-greenhouse-identity", token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(after.status().as_u16(), 200);
    let health = http.get(format!("{url}/health")).send().await.unwrap();
    assert_eq!(health.status().as_u16(), 200, "health needs no identity");
}

#[tokio::test]
async fn a_chain_that_ended_restarts_only_on_activity_or_on() {
    let h = harness().await;
    h.call("checkins", json!({"quiet": false, "every_minutes": 10}))
        .await;
    h.call("log", json!({"text": "starting"})).await;
    let mut t = at(14, 0);
    for _ in 0..8 {
        t += 600;
        h.set_now(t);
        h.tick().await;
    }
    // Changing the interval does not restart it, so no ninth check-in.
    let changed = h.call("checkins", json!({"every_minutes": 30})).await;
    assert_eq!(changed["checkins"]["next"], Value::Null);
    h.set_now(t + 3600);
    assert_eq!(h.tick().await["raised"], 0);
    // "Start checking on me again" does, and answers what was open.
    let on = h.call("checkins", json!({"on": true})).await;
    assert!(!on["checkins"]["next"].is_null());
    assert_eq!(on["checkins"]["due"], false);
    assert_eq!(
        h.retired(),
        vec![json!({"subject": "body-double/owner/checkin"})]
    );
    let day = h.call("day", json!({})).await;
    assert_eq!(day["checkins"], json!({"raised": 8, "answered": 8}));
}

#[tokio::test]
async fn a_corrected_household_zone_reaches_people_who_never_chose_one() {
    let h = harness().await;
    h.call("log", json!({"text": "started"})).await;
    assert_eq!(h.call("now", json!({})).await["timezone"], "UTC");
    let mut cfg = h.app.cfg.clone();
    cfg.timezone = "America/New_York".into();
    drop(h);
    let (_router, app) = build_app(cfg).unwrap();
    let store = app.store.lock().unwrap();
    assert_eq!(
        store.plan("owner").unwrap().unwrap().timezone,
        "",
        "the plan keeps no copy of the household zone"
    );
}
