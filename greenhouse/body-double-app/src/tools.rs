// The tools: strict arguments in, compact JSON out. Each tool works on the
// calling person's rows only.

use crate::checkins::{self, Plan, Step};
use crate::config::{AppConfig, EVERY_MINUTES};
use crate::store::{Entry, Status, Store, StoreError, Task, Thread};
use crate::time;
use greenhouse_kernel::clock::Zone;
use greenhouse_kernel::error::{err, ApiError};
use serde_json::{json, Map, Value};

/// Entries `now` shows.
const RECENT: usize = 5;
/// Loose ends `now` lists before counting the rest.
const OPEN_THREADS_SHOWN: usize = 10;
/// Paused tasks `now` lists.
const PAUSED_SHOWN: usize = 5;
/// What `day` may spend on its entries, in bytes of JSON: about 1,300 tokens,
/// the iOS recap's budget. The newest come first; the rest are counted.
const DAY_ENTRY_BYTES: usize = 5 * 1024;
/// Entries `day` returns at most, however short they are.
const DAY_ENTRIES: usize = 60;
/// An entry in `day` is its summary, or else its words, cut to this (as the
/// iOS recap took `summary ?? text.prefix(120)`).
const DAY_TEXT: usize = 120;
/// Tasks and loose ends `day` lists, and how much of a title or a note.
const DAY_TASKS: usize = 10;
const DAY_THREADS: usize = 5;
const DAY_TITLE: usize = 60;
const DAY_NOTE: usize = 80;
/// The largest export, in bytes.
pub const EXPORT_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Check-ins one tick raises at most (the rest wait for the next tick).
pub const TICK_MAX: usize = 50;

pub fn store_error(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound(m) => err(404, m),
        StoreError::Conflict(m) => err(409, m),
        StoreError::Sql(e) => err(
            500,
            format!("the journal could not be read or written: {e}"),
        ),
    }
}

// ---- arguments -----------------------------------------------------------------------

/// A tool's arguments: an object with only the names it knows. Null means
/// absent, as models often send it for optional fields.
pub struct Args<'a> {
    map: &'a Map<String, Value>,
}

impl<'a> Args<'a> {
    pub fn new(value: &'a Value, allowed: &[&str]) -> Result<Args<'a>, ApiError> {
        let map = value
            .as_object()
            .ok_or_else(|| err(400, "arguments must be an object"))?;
        if let Some(unknown) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(err(
                400,
                if allowed.is_empty() {
                    format!("unknown argument '{unknown}'; this tool takes none")
                } else {
                    format!(
                        "unknown argument '{unknown}'; this tool takes {}",
                        allowed.join(", ")
                    )
                },
            ));
        }
        Ok(Args { map })
    }

    fn get(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key).filter(|v| !v.is_null())
    }

    /// Trimmed text of at most `max` characters; empty counts as absent.
    pub fn text(&self, key: &str, max: usize) -> Result<Option<String>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => {
                let s = s.trim();
                if s.chars().count() > max {
                    Err(err(400, format!("{key} must be at most {max} characters")))
                } else {
                    Ok(Some(s.to_string()).filter(|s| !s.is_empty()))
                }
            }
            Some(_) => Err(err(400, format!("{key} must be text"))),
        }
    }

    pub fn required_text(&self, key: &str, max: usize) -> Result<String, ApiError> {
        self.text(key, max)?
            .ok_or_else(|| err(400, format!("{key} is required")))
    }

    pub fn int(
        &self,
        key: &str,
        range: std::ops::RangeInclusive<i64>,
    ) -> Result<Option<i64>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_i64()
                .filter(|n| range.contains(n))
                .map(Some)
                .ok_or_else(|| {
                    err(
                        400,
                        format!(
                            "{key} must be a whole number from {} to {}",
                            range.start(),
                            range.end()
                        ),
                    )
                }),
        }
    }

    pub fn required_id(&self, key: &str) -> Result<i64, ApiError> {
        self.int(key, 1..=i64::MAX)?
            .ok_or_else(|| err(400, format!("{key} is required")))
    }

    pub fn boolean(&self, key: &str) -> Result<Option<bool>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(err(400, format!("{key} must be true or false"))),
        }
    }
}

// ---- views ---------------------------------------------------------------------------

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut short: String = text.chars().take(max - 1).collect();
        short.push('…');
        short
    }
}

/// The current task: how long it has been current.
pub fn current_view(task: &Task, zone: &Zone, now: i64) -> Value {
    let since = task.active_since.unwrap_or(task.updated_at);
    let minutes = time::minutes_between(since, now);
    json!({
        "id": task.id,
        "title": task.title,
        "status": task.status,
        "since": time::when(zone, since, now),
        "since_ts": since,
        "minutes": minutes,
        "for": time::span(minutes),
    })
}

fn task_brief(task: &Task) -> Value {
    json!({ "id": task.id, "title": task.title, "status": task.status })
}

fn paused_view(paused: &Option<(Task, i64)>) -> Value {
    match paused {
        Some((task, ran)) => {
            json!({ "id": task.id, "title": task.title, "minutes": ran / 60 })
        }
        None => Value::Null,
    }
}

fn thread_view(thread: &Thread, zone: &Zone, now: i64) -> Value {
    let mut v = json!({
        "id": thread.id,
        "note": thread.note,
        "opened": time::when(zone, thread.opened_at, now),
        "opened_ts": thread.opened_at,
    });
    if let Some(closed) = thread.closed_at {
        v["closed"] = json!(time::when(zone, closed, now));
        v["closed_ts"] = json!(closed);
    }
    v
}

fn entry_view(entry: &Entry, zone: &Zone, now: i64) -> Value {
    json!({
        "id": entry.id,
        "at": time::when(zone, entry.at, now),
        "at_ts": entry.at,
        "text": entry.text,
        "summary": entry.summary,
        "mood": entry.mood,
        "task": entry.task,
    })
}

pub fn plan_view(plan: &Plan, zone: &Zone, now: i64, notices: bool) -> Value {
    json!({
        "on": plan.enabled,
        "every_minutes": plan.every_minutes,
        "quiet": plan.quiet.map(|(f, u)| format!("{}-{}", time::fmt_hhmm(f), time::fmt_hhmm(u))),
        "timezone": zone.name,
        "next": plan.next().map(|t| time::when(zone, t, now)),
        "next_ts": plan.next(),
        "due": plan.unanswered > 0,
        "unanswered": plan.unanswered,
        "notices": notices,
    })
}

// ---- the person's context --------------------------------------------------------------

/// Everything a tool needs to know about who is calling and when.
pub struct Ctx<'a> {
    pub cfg: &'a AppConfig,
    pub default_zone: &'a Zone,
    pub user: &'a str,
    pub now: i64,
}

impl Ctx<'_> {
    /// The person's plan, or what a new one would be (not saved).
    pub fn plan(&self, store: &Store) -> Result<Plan, ApiError> {
        Ok(store
            .plan(self.user)
            .map_err(store_error)?
            .unwrap_or_else(|| Plan::new(self.user, &self.cfg.defaults, "")))
    }

    pub fn zone(&self, plan: &Plan) -> Zone {
        time::zone_or(&plan.timezone, self.default_zone)
    }

    pub fn notices(&self) -> bool {
        self.cfg.notices(self.user)
    }
}

/// What a write did besides its result: whether a check-in notice should be
/// retired (the person answered it, or turned check-ins off).
pub struct Outcome {
    pub result: Value,
    pub retire: bool,
}

/// The person was here: open check-ins are answered and the clock restarts.
/// Whether a check-in was answered.
fn touch(store: &mut Store, ctx: &Ctx) -> Result<bool, ApiError> {
    let answered = store
        .answer_checkins(ctx.user, ctx.now)
        .map_err(store_error)?
        > 0;
    let mut plan = ctx.plan(store)?;
    let zone = ctx.zone(&plan);
    checkins::after_activity(&mut plan, &zone, ctx.now);
    store.save_plan(&plan).map_err(store_error)?;
    Ok(answered)
}

/// Activity after the main change is stored: a failure here must not turn a
/// stored change into an error the model would retry, so it is logged.
fn touch_logged(store: &mut Store, ctx: &Ctx) -> bool {
    match touch(store, ctx) {
        Ok(answered) => answered,
        Err(e) => {
            eprintln!(
                "[body-double] check-in bookkeeping for {} failed: {}",
                ctx.user, e.message
            );
            false
        }
    }
}

// ---- read tools ------------------------------------------------------------------------

pub fn now(store: &Store, ctx: &Ctx, args: &Value) -> Result<Value, ApiError> {
    Args::new(args, &[])?;
    let plan = ctx.plan(store)?;
    let zone = ctx.zone(&plan);
    let now = ctx.now;
    let current = store.active_task(ctx.user).map_err(store_error)?;
    let last = store.last_entry_at(ctx.user).map_err(store_error)?;
    let threads = store.open_threads(ctx.user).map_err(store_error)?;
    let paused = store
        .paused_tasks(ctx.user, PAUSED_SHOWN)
        .map_err(store_error)?;
    let recent = store
        .recent_entries(ctx.user, RECENT)
        .map_err(store_error)?;
    Ok(json!({
        "now": time::stamp(&zone, now),
        "timezone": zone.name,
        "current_task": current.as_ref().map(|t| current_view(t, &zone, now)),
        "minutes_since_last_entry": last.map(|t| time::minutes_between(t, now)),
        "open_threads": threads.iter().take(OPEN_THREADS_SHOWN).map(|t| thread_view(t, &zone, now)).collect::<Vec<_>>(),
        "more_open_threads": threads.len().saturating_sub(OPEN_THREADS_SHOWN),
        "paused_tasks": paused.iter().map(|t| json!({"id": t.id, "title": t.title})).collect::<Vec<_>>(),
        "recent": recent.iter().map(|e| json!({
            "id": e.id,
            "at": time::when(&zone, e.at, now),
            "at_ts": e.at,
            "text": cut(&e.text, 160),
            "summary": e.summary,
        })).collect::<Vec<_>>(),
        "checkins": plan_view(&plan, &zone, now, ctx.notices()),
    }))
}

/// An entry as `day` gives it: the time, the summary or else the words (cut),
/// and the mood and task when there are any.
fn day_entry(entry: &Entry, zone: &Zone) -> Value {
    let said = entry.summary.as_deref().unwrap_or(&entry.text);
    let mut v = json!({ "time": time::hhmm(zone, entry.at), "text": cut(said, DAY_TEXT) });
    if let Some(mood) = &entry.mood {
        v["mood"] = json!(mood);
    }
    if let Some(task) = &entry.task {
        v["task"] = json!(cut(task, DAY_TITLE));
    }
    v
}

fn day_thread(thread: &Thread, at: i64, zone: &Zone) -> Value {
    json!({ "id": thread.id, "note": cut(&thread.note, DAY_NOTE), "time": time::hhmm(zone, at) })
}

/// The newest `n` of a list kept oldest first, and how many were left out.
fn newest<T>(items: &[T], n: usize) -> (&[T], usize) {
    let left = items.len().saturating_sub(n);
    (&items[left..], left)
}

pub fn day(store: &Store, ctx: &Ctx, args: &Value) -> Result<Value, ApiError> {
    let args = Args::new(args, &["date"])?;
    let plan = ctx.plan(store)?;
    let zone = ctx.zone(&plan);
    let day = match args.text("date", 10)? {
        Some(d) => {
            time::parse_date(&d).ok_or_else(|| err(400, "date must be a date like 2026-10-15"))?
        }
        None => time::day_of(&zone, ctx.now),
    };
    let (from, to) = (time::day_start(&zone, day), time::day_start(&zone, day + 1));
    // Sized for a chat turn: the kernel puts the result into the model's
    // context whole, on an 8,192-token runtime. The newest entries that fit a
    // byte budget, each its summary or its words cut short, and counts of
    // everything left out. A heavy day stays near 2,000 tokens.
    let candidates = store
        .newest_entries_between(ctx.user, from, to, DAY_ENTRIES)
        .map_err(store_error)?;
    let mut entries = Vec::new();
    let mut spent = 0;
    for entry in candidates.iter().rev() {
        let view = day_entry(entry, &zone);
        let size = view.to_string().len() + 1;
        if !entries.is_empty() && spent + size > DAY_ENTRY_BYTES {
            break;
        }
        spent += size;
        entries.push(view);
    }
    entries.reverse();
    let count = store
        .count_entries_between(ctx.user, from, to)
        .map_err(store_error)?;
    let earlier = (count - entries.len() as i64).max(0);
    let mut tasks = store
        .tasks_touched(ctx.user, from, to)
        .map_err(store_error)?;
    let more_tasks = tasks.len().saturating_sub(DAY_TASKS);
    tasks.truncate(DAY_TASKS);
    let (opened, closed) = store
        .threads_between(ctx.user, from, to)
        .map_err(store_error)?;
    let (opened, more_opened) = newest(&opened, DAY_THREADS);
    let (closed, more_closed) = newest(&closed, DAY_THREADS);
    let open = store.open_threads(ctx.user).map_err(store_error)?;
    let more_open = open.len().saturating_sub(DAY_THREADS);
    let (raised, answered) = store
        .checkin_counts(ctx.user, from, to)
        .map_err(store_error)?;
    Ok(json!({
        "date": time::iso_date(day),
        "label": time::date_label(day),
        "timezone": zone.name,
        "entries": entries,
        "earlier": earlier,
        "tasks": tasks.iter().map(|t| json!({"id": t.id, "title": cut(&t.title, DAY_TITLE), "status": t.status})).collect::<Vec<_>>(),
        "more_tasks": more_tasks,
        "threads_opened": opened.iter().map(|t| day_thread(t, t.opened_at, &zone)).collect::<Vec<_>>(),
        "more_threads_opened": more_opened,
        "threads_closed": closed.iter().map(|t| day_thread(t, t.closed_at.unwrap_or(t.opened_at), &zone)).collect::<Vec<_>>(),
        "more_threads_closed": more_closed,
        "open_threads": open.iter().take(DAY_THREADS).map(|t| json!({"id": t.id, "note": cut(&t.note, DAY_NOTE)})).collect::<Vec<_>>(),
        "more_open_threads": more_open,
        "checkins": {"raised": raised, "answered": answered},
        "truncated": earlier > 0 || more_tasks > 0 || more_opened > 0 || more_closed > 0 || more_open > 0,
    }))
}

/// A journal file to download.
pub struct Export {
    pub filename: String,
    pub content_type: &'static str,
    pub content: String,
}

/// The journal as a file, for the download route (not a tool: a tool's
/// result goes into the model's context whole).
pub fn export(store: &Store, ctx: &Ctx, args: &Value) -> Result<Export, ApiError> {
    let args = Args::new(args, &["format", "from", "to"])?;
    let format = args
        .text("format", 10)?
        .unwrap_or_else(|| "markdown".into());
    if format != "markdown" && format != "json" {
        return Err(err(400, "format must be markdown or json"));
    }
    let plan = ctx.plan(store)?;
    let zone = ctx.zone(&plan);
    let date = |key: &str| -> Result<Option<i64>, ApiError> {
        args.text(key, 10)?
            .map(|d| {
                time::parse_date(&d)
                    .ok_or_else(|| err(400, format!("{key} must be a date like 2026-10-15")))
            })
            .transpose()
    };
    let (from_day, to_day) = (date("from")?, date("to")?);
    if let (Some(f), Some(t)) = (from_day, to_day) {
        if f > t {
            return Err(err(400, "from must not be after to"));
        }
    }
    let from = from_day.map_or(i64::MIN, |d| time::day_start(&zone, d));
    let to = to_day.map_or(i64::MAX, |d| time::day_start(&zone, d + 1));
    let entries = store
        .entries_between(ctx.user, from, to, usize::MAX >> 1)
        .map_err(store_error)?;
    let tasks = store.all_tasks(ctx.user).map_err(store_error)?;
    let threads = store.all_threads(ctx.user).map_err(store_error)?;
    let checkins: Vec<_> = store
        .all_checkins(ctx.user)
        .map_err(store_error)?
        .into_iter()
        .filter(|c| c.raised_at >= from && c.raised_at < to)
        .collect();
    let today = time::day_of(&zone, ctx.now);
    let span_name = match (from_day, to_day) {
        (None, None) => time::iso_date(today),
        (f, t) => format!(
            "{}-to-{}",
            f.map_or("start".into(), time::iso_date),
            t.map_or(time::iso_date(today), time::iso_date)
        ),
    };

    let (content, ext) = if format == "json" {
        let doc = json!({
            "exported_at": time::iso_utc(ctx.now),
            "user": ctx.user,
            "timezone": zone.name,
            "from": from_day.map(time::iso_date),
            "to": to_day.map(time::iso_date),
            "entries": entries.iter().map(|e| json!({
                "id": e.id, "at": time::iso_utc(e.at), "text": e.text, "summary": e.summary,
                "mood": e.mood, "task_id": e.task_id, "task": e.task,
            })).collect::<Vec<_>>(),
            "tasks": tasks.iter().map(|t| json!({
                "id": t.id, "title": t.title, "status": t.status,
                "created_at": time::iso_utc(t.created_at), "updated_at": time::iso_utc(t.updated_at),
                "active_since": t.active_since.map(time::iso_utc),
            })).collect::<Vec<_>>(),
            "threads": threads.iter().map(|t| json!({
                "id": t.id, "note": t.note, "opened_at": time::iso_utc(t.opened_at),
                "closed_at": t.closed_at.map(time::iso_utc),
            })).collect::<Vec<_>>(),
            "checkins": checkins.iter().map(|c| json!({
                "raised_at": time::iso_utc(c.raised_at), "answered_at": c.answered_at.map(time::iso_utc),
                "notified": c.notified,
            })).collect::<Vec<_>>(),
            "checkin_plan": plan_view(&plan, &zone, ctx.now, ctx.notices()),
        });
        (
            serde_json::to_string_pretty(&doc).expect("serialize export"),
            "json",
        )
    } else {
        (markdown(ctx, &zone, &entries, &tasks, &threads), "md")
    };
    if content.len() > EXPORT_MAX_BYTES {
        return Err(err(
            400,
            format!(
                "the export would be {} MB, more than {} MB; give from and to to export a shorter span",
                content.len() / (1024 * 1024),
                EXPORT_MAX_BYTES / (1024 * 1024)
            ),
        ));
    }
    Ok(Export {
        filename: format!("body-double-{span_name}.{ext}"),
        content_type: if ext == "json" {
            "application/json; charset=utf-8"
        } else {
            "text/markdown; charset=utf-8"
        },
        content,
    })
}

/// The person's text as inline Markdown: one line, and literal. Line breaks
/// (any kind) become spaces, so text cannot start a heading or a list item,
/// and Markdown's and HTML's syntax characters are escaped, so a title like
/// `<img src=x onerror=…>` is shown, not run, by a viewer.
fn md_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '&' | '~' | '|' | '#' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn markdown(
    ctx: &Ctx,
    zone: &Zone,
    entries: &[Entry],
    tasks: &[Task],
    threads: &[Thread],
) -> String {
    let mut md = vec![
        "# Body double journal".to_string(),
        String::new(),
        format!(
            "Exported {} ({}) for {}.",
            time::stamp(zone, ctx.now),
            zone.name,
            md_text(ctx.user)
        ),
        String::new(),
    ];
    let mut day = None;
    for e in entries {
        let d = time::day_of(zone, e.at);
        if day != Some(d) {
            if day.is_some() {
                md.push(String::new());
            }
            md.push(format!(
                "## {} ({})",
                time::iso_date(d),
                time::date_label(d)
            ));
            md.push(String::new());
            day = Some(d);
        }
        let mut line = format!("- **{}** {}", time::hhmm(zone, e.at), md_text(&e.text));
        if let Some(s) = &e.summary {
            line.push_str(&format!(" — _{}_", md_text(s)));
        }
        if let Some(m) = &e.mood {
            line.push_str(&format!(" · mood: {}", md_text(m)));
        }
        if let Some(t) = &e.task {
            line.push_str(&format!(" ({})", md_text(t)));
        }
        md.push(line);
    }
    if !entries.is_empty() {
        md.push(String::new());
    }
    if !tasks.is_empty() {
        md.push("## Tasks".into());
        md.push(String::new());
        for t in tasks {
            let current = if t.status == "active" {
                " ← current"
            } else {
                ""
            };
            md.push(format!("- {}: {}{current}", t.status, md_text(&t.title)));
        }
        md.push(String::new());
    }
    if !threads.is_empty() {
        md.push("## Loose ends".into());
        md.push(String::new());
        for t in threads {
            let state = if t.closed_at.is_some() {
                "closed"
            } else {
                "open"
            };
            md.push(format!("- {state}: {}", md_text(&t.note)));
        }
        md.push(String::new());
    }
    md.join("\n")
}

// ---- write tools ------------------------------------------------------------------------

pub fn log(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(args, &["text", "summary", "mood"])?;
    let text = args.required_text("text", 2000)?;
    let summary = args.text("summary", 200)?;
    let mood = args.text("mood", 40)?;
    // The person's words first: nothing after this can lose them.
    let entry = store
        .insert_entry(
            ctx.user,
            ctx.now,
            &text,
            summary.as_deref(),
            mood.as_deref(),
        )
        .map_err(store_error)?;
    let answered = touch_logged(store, ctx);
    let plan = ctx.plan(store)?;
    let zone = ctx.zone(&plan);
    let current = store.active_task(ctx.user).map_err(store_error)?;
    Ok(Outcome {
        result: json!({
            "logged": entry_view(&entry, &zone, ctx.now),
            "current_task": current.as_ref().map(|t| current_view(t, &zone, ctx.now)),
            "answered_checkin": answered,
        }),
        retire: answered,
    })
}

pub fn set_task(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(args, &["title"])?;
    let title = args.required_text("title", 200)?;
    let switched = store
        .set_task(ctx.user, &title, ctx.now)
        .map_err(store_error)?;
    let answered = touch_logged(store, ctx);
    let zone = ctx.zone(&ctx.plan(store)?);
    Ok(Outcome {
        result: json!({
            "current_task": current_view(&switched.current, &zone, ctx.now),
            "paused": paused_view(&switched.paused),
            "resumed": switched.resumed,
            "unchanged": switched.unchanged,
        }),
        retire: answered,
    })
}

pub fn update_task(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(args, &["task_id", "status"])?;
    let id = args.required_id("task_id")?;
    let status = Status::parse(&args.required_text("status", 10)?)
        .ok_or_else(|| err(400, "status must be active, paused, done or dropped"))?;
    let updated = store
        .update_task(ctx.user, id, status, ctx.now)
        .map_err(store_error)?;
    let answered = touch_logged(store, ctx);
    let zone = ctx.zone(&ctx.plan(store)?);
    let current = store.active_task(ctx.user).map_err(store_error)?;
    Ok(Outcome {
        result: json!({
            "task": task_brief(&updated.task),
            "current_task": current.as_ref().map(|t| current_view(t, &zone, ctx.now)),
            "paused": paused_view(&updated.paused),
        }),
        retire: answered,
    })
}

pub fn open_thread(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(args, &["note"])?;
    let note = args.required_text("note", 300)?;
    let thread = store
        .open_thread(ctx.user, &note, ctx.now)
        .map_err(store_error)?;
    let answered = touch_logged(store, ctx);
    let zone = ctx.zone(&ctx.plan(store)?);
    let open = store.open_threads(ctx.user).map_err(store_error)?.len();
    Ok(Outcome {
        result: json!({
            "thread": thread_view(&thread, &zone, ctx.now),
            "open_threads": open,
        }),
        retire: answered,
    })
}

pub fn close_thread(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(args, &["thread_id"])?;
    let id = args.required_id("thread_id")?;
    let (thread, already) = store
        .close_thread(ctx.user, id, ctx.now)
        .map_err(store_error)?;
    let answered = touch_logged(store, ctx);
    let zone = ctx.zone(&ctx.plan(store)?);
    let open = store.open_threads(ctx.user).map_err(store_error)?.len();
    Ok(Outcome {
        result: json!({
            "thread": thread_view(&thread, &zone, ctx.now),
            "open_threads": open,
            "already_closed": already,
        }),
        retire: answered,
    })
}

pub fn set_checkins(store: &mut Store, ctx: &Ctx, args: &Value) -> Result<Outcome, ApiError> {
    let args = Args::new(
        args,
        &[
            "on",
            "every_minutes",
            "quiet",
            "quiet_from",
            "quiet_until",
            "timezone",
            "next_in_minutes",
        ],
    )?;
    let on = args.boolean("on")?;
    let every = args.int("every_minutes", EVERY_MINUTES)?;
    let quiet = args.boolean("quiet")?;
    let clock_arg = |key: &str| -> Result<Option<u32>, ApiError> {
        args.text(key, 5)?
            .map(|t| {
                time::parse_hhmm(&t)
                    .ok_or_else(|| err(400, format!("{key} must be a time like 22:00")))
            })
            .transpose()
    };
    let (quiet_from, quiet_until) = (clock_arg("quiet_from")?, clock_arg("quiet_until")?);
    let timezone = args.text("timezone", 64)?;
    let next_in = args.int("next_in_minutes", 5..=480)?;

    let mut plan = ctx.plan(store)?;
    let was = plan.clone();
    if let Some(tz) = &timezone {
        Zone::load(tz).map_err(|e| err(400, format!("timezone: {e}")))?;
        plan.timezone = tz.clone();
    }
    if let Some(every) = every {
        plan.every_minutes = every;
    }
    match (quiet, quiet_from, quiet_until) {
        (Some(false), None, None) => plan.quiet = None,
        (Some(false), _, _) => {
            return Err(err(
                400,
                "quiet false removes quiet hours; leave out quiet_from and quiet_until",
            ))
        }
        (_, None, None) => {
            if quiet == Some(true) && plan.quiet.is_none() {
                plan.quiet = ctx.cfg.defaults.quiet.or(Some((22 * 60, 8 * 60)));
            }
        }
        (_, from, until) => {
            let (from, until) =
                match (from, until, plan.quiet) {
                    (Some(f), Some(u), _) => (f, u),
                    (Some(f), None, Some((_, u))) => (f, u),
                    (None, Some(u), Some((f, _))) => (f, u),
                    _ => return Err(err(
                        400,
                        "give quiet_from and quiet_until together (there are no quiet hours yet)",
                    )),
                };
            plan.quiet = (from != until).then_some((from, until));
        }
    }
    let mut turned_on = false;
    match on {
        Some(false) => {
            if next_in.is_some() {
                return Err(err(
                    400,
                    "on false and next_in_minutes contradict each other",
                ));
            }
            plan.enabled = false;
            plan.next_due_at = None;
            plan.oneoff_at = None;
            plan.unanswered = 0;
        }
        Some(true) => {
            // "Start checking on me again" also restarts a chain that ended
            // after eight unanswered.
            turned_on = !plan.enabled || plan.unanswered >= checkins::MAX_UNANSWERED;
            plan.enabled = true;
            if turned_on {
                plan.unanswered = 0;
            }
        }
        None => {}
    }
    let zone = ctx.zone(&plan);
    // A chain that ended after eight unanswered stays ended through other
    // changes: only activity or on:true starts it again.
    let chain_changed = plan.enabled
        && plan.unanswered < checkins::MAX_UNANSWERED
        && (turned_on
            || plan.every_minutes != was.every_minutes
            || plan.quiet != was.quiet
            || plan.timezone != was.timezone
            || plan.next_due_at.is_none());
    if chain_changed {
        plan.next_due_at = Some(checkins::next_after(&plan, &zone, ctx.now));
    }
    if let Some(minutes) = next_in {
        // Asked for at a set time: it comes even inside quiet hours or with
        // the chain off.
        plan.oneoff_at = Some(ctx.now + minutes * 60);
    }
    store.save_plan(&plan).map_err(store_error)?;
    // Turning check-ins off, or back on after some went unanswered, answers
    // the open ones and takes their notice down.
    let retire = (on == Some(false) || (turned_on && was.unanswered > 0))
        && store
            .answer_checkins(ctx.user, ctx.now)
            .map_err(store_error)?
            > 0;
    Ok(Outcome {
        result: json!({
            "checkins": plan_view(&plan, &zone, ctx.now, ctx.notices()),
        }),
        retire,
    })
}

// ---- the scheduled tick ----------------------------------------------------------------

/// A notice the tick decided to send; sent after the journal is released.
pub struct Notice {
    pub checkin_id: i64,
    pub user: String,
    pub title: String,
    pub body: String,
}

#[derive(Default)]
pub struct Tick {
    pub due: usize,
    pub raised: usize,
    pub deferred: usize,
    pub paused: usize,
    pub notices: Vec<Notice>,
    /// People whose check-in could not be recorded; the next tick tries again.
    pub errors: Vec<String>,
}

/// Decide and record every due check-in; the notice goes out afterwards.
/// One person's failure does not stop the others'.
pub fn tick(
    store: &mut Store,
    cfg: &AppConfig,
    default_zone: &Zone,
    now: i64,
) -> Result<Tick, ApiError> {
    let due = store.due_plans(now).map_err(store_error)?;
    let mut out = Tick {
        due: due.len(),
        ..Tick::default()
    };
    for mut plan in due.into_iter().take(TICK_MAX) {
        let zone = time::zone_or(&plan.timezone, default_zone);
        let step = checkins::step(&plan, &zone, now);
        checkins::apply(&mut plan, &step, now);
        let recorded = match step {
            Step::Wait => Ok(()),
            Step::Defer { .. } => store.save_plan(&plan).map(|()| out.deferred += 1),
            Step::Raise { next } => store.raise_and_save(&plan, now).map(|_| {
                out.raised += 1;
                if next.is_none() {
                    out.paused += 1;
                }
            }),
        };
        if let Err(e) = recorded {
            out.errors
                .push(format!("{}: {}", plan.user_id, store_error(e).message));
        }
    }
    // The owner's notice: for the check-in just raised, or for an earlier one
    // whose notice did not go out.
    if let Some(owner) = &cfg.notices_for {
        match unsent_notice(store, owner, now) {
            Ok(notice) => out.notices.extend(notice),
            Err(e) => out
                .errors
                .push(format!("{owner}: {}", store_error(e).message)),
        }
    }
    Ok(out)
}

fn unsent_notice(store: &Store, user: &str, now: i64) -> Result<Option<Notice>, StoreError> {
    let Some((id, _)) = store.unsent_checkin(user)? else {
        return Ok(None);
    };
    let task = store.active_task(user)?;
    let loose_ends = store.open_threads(user)?.len();
    let (title, body) = checkins::notice_text(
        task.map(|t| time::minutes_between(t.active_since.unwrap_or(t.updated_at), now)),
        loose_ends,
    );
    Ok(Some(Notice {
        checkin_id: id,
        user: user.to_string(),
        title,
        body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PlanDefaults;

    fn cfg() -> AppConfig {
        AppConfig {
            kernel_url: String::new(),
            state_dir: std::path::PathBuf::new(),
            port: 0,
            service_token: String::new(),
            timezone: "UTC".into(),
            notices_for: Some("a".into()),
            defaults: PlanDefaults::default(),
        }
    }

    #[test]
    fn one_persons_failure_does_not_stop_the_others_check_ins() {
        let mut store = Store::in_memory().unwrap();
        for user in ["a", "b", "c"] {
            let mut p = Plan::new(user, &PlanDefaults::default(), "");
            p.quiet = None;
            p.next_due_at = Some(1000);
            store.save_plan(&p).unwrap();
        }
        store.fail_plan_updates_for("b");
        let tick = tick(&mut store, &cfg(), &Zone::utc(), 1000).unwrap();
        assert_eq!(tick.due, 3);
        assert_eq!(tick.raised, 2, "a and c");
        assert_eq!(tick.errors.len(), 1);
        assert!(tick.errors[0].starts_with("b: "), "{:?}", tick.errors);
        assert_eq!(tick.notices.len(), 1, "a's notice is not lost");
        assert_eq!(tick.notices[0].user, "a");
        assert!(
            store.all_checkins("b").unwrap().is_empty(),
            "b is decided again next tick"
        );
    }

    #[test]
    fn md_text_keeps_the_words_on_one_literal_line() {
        assert_eq!(
            md_text("plain words, 3 of them."),
            "plain words, 3 of them."
        );
        assert_eq!(md_text("a\r\nb\u{2028}c\td"), "a  b c d");
        assert_eq!(md_text("<b>*x*</b>"), "\\<b\\>\\*x\\*\\</b\\>");
        assert_eq!(md_text("[a](b) `c` #d"), "\\[a\\](b) \\`c\\` \\#d");
    }
}
