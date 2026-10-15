// The journal: SQLite in the app's state directory. Every row belongs to one
// person (the user in the kernel's identity token) and every query names that
// person, so one person's ids never reach another's data.
//
// The ids people and the model see are each person's own numbers (`seq`:
// 1, 2, 3 per person and table), not the shared row ids, which would let one
// person count another's writes. Row ids stay inside this file.
//
// The model is stateless and so is the assistant's view of this app: all
// memory is here, and each tool call reads what it needs.

use crate::checkins::Plan;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE entries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  at INTEGER NOT NULL,
  text TEXT NOT NULL,
  summary TEXT,
  mood TEXT,
  task_id INTEGER
);
CREATE INDEX entries_user_at ON entries(user_id, at);
CREATE UNIQUE INDEX entries_user_seq ON entries(user_id, seq);

CREATE TABLE tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','paused','done','dropped')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  active_since INTEGER
);
CREATE UNIQUE INDEX tasks_one_active ON tasks(user_id) WHERE status = 'active';
CREATE INDEX tasks_user_updated ON tasks(user_id, updated_at);
CREATE UNIQUE INDEX tasks_user_seq ON tasks(user_id, seq);

CREATE TABLE threads (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  note TEXT NOT NULL,
  opened_at INTEGER NOT NULL,
  closed_at INTEGER
);
CREATE INDEX threads_user ON threads(user_id, closed_at);
CREATE UNIQUE INDEX threads_user_seq ON threads(user_id, seq);

CREATE TABLE checkins (
  id INTEGER PRIMARY KEY,
  user_id TEXT NOT NULL,
  raised_at INTEGER NOT NULL,
  answered_at INTEGER,
  notified INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX checkins_user_raised ON checkins(user_id, raised_at);

CREATE TABLE plans (
  user_id TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL,
  every_minutes INTEGER NOT NULL,
  quiet_from INTEGER,
  quiet_until INTEGER,
  timezone TEXT NOT NULL,
  next_due_at INTEGER,
  oneoff_at INTEGER,
  unanswered INTEGER NOT NULL DEFAULT 0
);
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Active,
    Paused,
    Done,
    Dropped,
}

impl Status {
    pub fn parse(text: &str) -> Option<Status> {
        match text {
            "active" => Some(Status::Active),
            "paused" => Some(Status::Paused),
            "done" => Some(Status::Done),
            "dropped" => Some(Status::Dropped),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Paused => "paused",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The person's own number for it.
    pub id: i64,
    pub at: i64,
    pub text: String,
    pub summary: Option<String>,
    pub mood: Option<String>,
    /// The task's number (the person's own).
    pub task_id: Option<i64>,
    /// The task's title, joined for display.
    pub task: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    /// The person's own number for it.
    pub id: i64,
    /// The row id, never shown.
    key: i64,
    pub title: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub active_since: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    /// The person's own number for it.
    pub id: i64,
    /// The row id, never shown.
    key: i64,
    pub note: String,
    pub opened_at: i64,
    pub closed_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Checkin {
    pub id: i64,
    pub raised_at: i64,
    pub answered_at: Option<i64>,
    pub notified: bool,
}

/// What set_task did.
#[derive(Clone, Debug, PartialEq)]
pub struct Switched {
    pub current: Task,
    /// The task that was current before, now paused, and for how long it had
    /// been current (seconds).
    pub paused: Option<(Task, i64)>,
    pub resumed: bool,
    pub unchanged: bool,
}

/// What update_task did.
#[derive(Clone, Debug, PartialEq)]
pub struct Updated {
    pub task: Task,
    pub paused: Option<(Task, i64)>,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    Conflict(String),
    Sql(rusqlite::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

pub struct Store {
    conn: Connection,
}

/// An entry and its task, which is always the same person's.
const ENTRY_FROM: &str =
    "entries e LEFT JOIN tasks t ON t.id = e.task_id AND t.user_id = e.user_id";
const ENTRY_COLUMNS: &str = "e.seq, e.at, e.text, e.summary, e.mood, t.seq, t.title";
const TASK_COLUMNS: &str = "seq, title, status, created_at, updated_at, active_since, id";
const THREAD_COLUMNS: &str = "seq, note, opened_at, closed_at, id";

/// The person's next number in a table (inside the insert, so it is atomic).
fn next_seq(table: &str) -> String {
    format!("(SELECT COALESCE(MAX(seq), 0) + 1 FROM {table} WHERE user_id = ?1)")
}

fn entry_row(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        at: r.get(1)?,
        text: r.get(2)?,
        summary: r.get(3)?,
        mood: r.get(4)?,
        task_id: r.get(5)?,
        task: r.get(6)?,
    })
}

fn task_row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        status: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        active_since: r.get(5)?,
        key: r.get(6)?,
    })
}

fn thread_row(r: &Row) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: r.get(0)?,
        note: r.get(1)?,
        opened_at: r.get(2)?,
        closed_at: r.get(3)?,
        key: r.get(4)?,
    })
}

fn plan_row(r: &Row) -> rusqlite::Result<Plan> {
    let from: Option<i64> = r.get(3)?;
    let until: Option<i64> = r.get(4)?;
    Ok(Plan {
        user_id: r.get(0)?,
        enabled: r.get::<_, i64>(1)? != 0,
        every_minutes: r.get(2)?,
        quiet: match (from, until) {
            (Some(f), Some(u)) => Some((f as u32, u as u32)),
            _ => None,
        },
        timezone: r.get(5)?,
        next_due_at: r.get(6)?,
        oneoff_at: r.get(7)?,
        unanswered: r.get(8)?,
    })
}

fn active_in(tx: &Connection, user: &str) -> rusqlite::Result<Option<Task>> {
    tx.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE user_id = ?1 AND status = 'active'"),
        params![user],
        task_row,
    )
    .optional()
}

/// Pause the active task, if any, and say how long it had been current.
fn pause_active(tx: &Transaction, user: &str, now: i64) -> rusqlite::Result<Option<(Task, i64)>> {
    let Some(mut task) = active_in(tx, user)? else {
        return Ok(None);
    };
    let ran = now - task.active_since.unwrap_or(task.updated_at);
    tx.execute(
        "UPDATE tasks SET status = 'paused', active_since = NULL, updated_at = ?1 WHERE id = ?2",
        params![now, task.key],
    )?;
    task.status = "paused".into();
    task.active_since = None;
    task.updated_at = now;
    Ok(Some((task, ran.max(0))))
}

fn activate(tx: &Transaction, key: i64, now: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE tasks SET status = 'active', active_since = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, key],
    )?;
    Ok(())
}

/// Two task titles name the same task: any case, in any script.
fn same_title(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A task by the person's number for it.
fn task_in(conn: &Connection, user: &str, id: i64) -> rusqlite::Result<Option<Task>> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE user_id = ?1 AND seq = ?2"),
        params![user, id],
        task_row,
    )
    .optional()
}

fn task_by_key(conn: &Connection, key: i64) -> rusqlite::Result<Task> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
        params![key],
        task_row,
    )
}

fn save_plan_in(conn: &Connection, plan: &Plan) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO plans (user_id, enabled, every_minutes, quiet_from, quiet_until, timezone, next_due_at, oneoff_at, unanswered)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(user_id) DO UPDATE SET enabled = excluded.enabled, every_minutes = excluded.every_minutes,
           quiet_from = excluded.quiet_from, quiet_until = excluded.quiet_until, timezone = excluded.timezone,
           next_due_at = excluded.next_due_at, oneoff_at = excluded.oneoff_at, unanswered = excluded.unanswered",
        params![
            plan.user_id,
            plan.enabled as i64,
            plan.every_minutes,
            plan.quiet.map(|q| q.0 as i64),
            plan.quiet.map(|q| q.1 as i64),
            plan.timezone,
            plan.next_due_at,
            plan.oneoff_at,
            plan.unanswered
        ],
    )?;
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        let conn = Connection::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("journal mode: {e}"))?;
        Store::init(conn)
    }

    pub fn in_memory() -> Result<Store, String> {
        Store::init(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn init(conn: Connection) -> Result<Store, String> {
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "the journal is schema {version}, newer than this app ({SCHEMA_VERSION}); refusing to touch it"
            ));
        }
        if version < 1 {
            conn.execute_batch(&format!(
                "BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"
            ))
            .map_err(|e| format!("create the journal: {e}"))?;
        }
        Ok(Store { conn })
    }

    // ---- entries ----------------------------------------------------------------

    /// The person's words, stored first: nothing that follows can lose them.
    pub fn insert_entry(
        &mut self,
        user: &str,
        at: i64,
        text: &str,
        summary: Option<&str>,
        mood: Option<&str>,
    ) -> StoreResult<Entry> {
        let task_key = active_in(&self.conn, user)?.map(|t| t.key);
        self.conn.execute(
            &format!(
                "INSERT INTO entries (user_id, seq, at, text, summary, mood, task_id)
                 VALUES (?1, {}, ?2, ?3, ?4, ?5, ?6)",
                next_seq("entries")
            ),
            params![user, at, text, summary, mood, task_key],
        )?;
        let key = self.conn.last_insert_rowid();
        Ok(self.conn.query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM {ENTRY_FROM} WHERE e.id = ?1"),
            params![key],
            entry_row,
        )?)
    }

    pub fn last_entry_at(&self, user: &str) -> StoreResult<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT MAX(at) FROM entries WHERE user_id = ?1",
            params![user],
            |r| r.get(0),
        )?)
    }

    /// The last `n` entries, oldest first.
    pub fn recent_entries(&self, user: &str, n: usize) -> StoreResult<Vec<Entry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM {ENTRY_FROM} WHERE e.user_id = ?1 ORDER BY e.at DESC, e.id DESC LIMIT ?2"
        ))?;
        let mut rows = stmt
            .query_map(params![user, n as i64], entry_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Entries in [from, to), oldest first, at most `limit`.
    pub fn entries_between(
        &self,
        user: &str,
        from: i64,
        to: i64,
        limit: usize,
    ) -> StoreResult<Vec<Entry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM {ENTRY_FROM} WHERE e.user_id = ?1 AND e.at >= ?2 AND e.at < ?3
             ORDER BY e.at, e.id LIMIT ?4"
        ))?;
        let rows = stmt
            .query_map(params![user, from, to, limit as i64], entry_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The newest `limit` entries in [from, to), oldest first.
    pub fn newest_entries_between(
        &self,
        user: &str,
        from: i64,
        to: i64,
        limit: usize,
    ) -> StoreResult<Vec<Entry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM {ENTRY_FROM} WHERE e.user_id = ?1 AND e.at >= ?2 AND e.at < ?3
             ORDER BY e.at DESC, e.id DESC LIMIT ?4"
        ))?;
        let mut rows = stmt
            .query_map(params![user, from, to, limit as i64], entry_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn count_entries_between(&self, user: &str, from: i64, to: i64) -> StoreResult<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE user_id = ?1 AND at >= ?2 AND at < ?3",
            params![user, from, to],
            |r| r.get(0),
        )?)
    }

    // ---- tasks ------------------------------------------------------------------

    pub fn active_task(&self, user: &str) -> StoreResult<Option<Task>> {
        Ok(active_in(&self.conn, user)?)
    }

    pub fn task(&self, user: &str, id: i64) -> StoreResult<Option<Task>> {
        Ok(task_in(&self.conn, user, id)?)
    }

    /// The most recently touched paused tasks.
    pub fn paused_tasks(&self, user: &str, n: usize) -> StoreResult<Vec<Task>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE user_id = ?1 AND status = 'paused'
             ORDER BY updated_at DESC, seq DESC LIMIT ?2"
        ))?;
        let rows = stmt
            .query_map(params![user, n as i64], task_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Tasks a span of time touched: created or changed in it, or logged
    /// against. Most recently touched first.
    pub fn tasks_touched(&self, user: &str, from: i64, to: i64) -> StoreResult<Vec<Task>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE user_id = ?1 AND (
               (created_at >= ?2 AND created_at < ?3) OR (updated_at >= ?2 AND updated_at < ?3)
               OR id IN (SELECT task_id FROM entries WHERE user_id = ?1 AND at >= ?2 AND at < ?3))
             ORDER BY updated_at DESC, seq DESC"
        ))?;
        let rows = stmt
            .query_map(params![user, from, to], task_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn all_tasks(&self, user: &str) -> StoreResult<Vec<Task>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE user_id = ?1 ORDER BY created_at, seq"
        ))?;
        let rows = stmt
            .query_map(params![user], task_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Make a task current: the previous one is paused; an open task of the
    /// same title (any case) is resumed rather than duplicated.
    pub fn set_task(&mut self, user: &str, title: &str, now: i64) -> StoreResult<Switched> {
        let tx = self.conn.transaction()?;
        if let Some(current) = active_in(&tx, user)? {
            if same_title(&current.title, title) {
                return Ok(Switched {
                    current,
                    paused: None,
                    resumed: false,
                    unchanged: true,
                });
            }
        }
        let paused = pause_active(&tx, user, now)?;
        // Compared in Rust, not with SQLite's lower(), which folds ASCII only.
        let existing: Option<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id, title FROM tasks WHERE user_id = ?1 AND status = 'paused'
                 ORDER BY updated_at DESC, seq DESC",
            )?;
            let paused = stmt
                .query_map(params![user], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            paused
                .into_iter()
                .find(|(_, t)| same_title(t, title))
                .map(|(id, _)| id)
        };
        let (key, resumed) = match existing {
            Some(key) => {
                activate(&tx, key, now)?;
                (key, true)
            }
            None => {
                tx.execute(
                    &format!(
                        "INSERT INTO tasks (user_id, seq, title, status, created_at, updated_at, active_since)
                         VALUES (?1, {}, ?2, 'active', ?3, ?3, ?3)",
                        next_seq("tasks")
                    ),
                    params![user, title, now],
                )?;
                (tx.last_insert_rowid(), false)
            }
        };
        let current = task_by_key(&tx, key)?;
        tx.commit()?;
        Ok(Switched {
            current,
            paused,
            resumed,
            unchanged: false,
        })
    }

    /// Change a task's status. Active makes it current again, from paused,
    /// done or dropped (and pauses the current one); done and dropped end it.
    pub fn update_task(
        &mut self,
        user: &str,
        id: i64,
        status: Status,
        now: i64,
    ) -> StoreResult<Updated> {
        let tx = self.conn.transaction()?;
        let task =
            task_in(&tx, user, id)?.ok_or_else(|| StoreError::NotFound(format!("no task {id}")))?;
        let mut paused = None;
        if status == Status::Active {
            if task.status != "active" {
                paused = pause_active(&tx, user, now)?;
                activate(&tx, task.key, now)?;
            }
        } else {
            tx.execute(
                "UPDATE tasks SET status = ?1, active_since = NULL, updated_at = ?2 WHERE id = ?3",
                params![status.as_str(), now, task.key],
            )?;
        }
        let task = task_by_key(&tx, task.key)?;
        tx.commit()?;
        Ok(Updated { task, paused })
    }

    // ---- loose ends ---------------------------------------------------------------

    pub fn open_thread(&mut self, user: &str, note: &str, now: i64) -> StoreResult<Thread> {
        self.conn.execute(
            &format!(
                "INSERT INTO threads (user_id, seq, note, opened_at) VALUES (?1, {}, ?2, ?3)",
                next_seq("threads")
            ),
            params![user, note, now],
        )?;
        let key = self.conn.last_insert_rowid();
        Ok(self.conn.query_row(
            &format!("SELECT {THREAD_COLUMNS} FROM threads WHERE id = ?1"),
            params![key],
            thread_row,
        )?)
    }

    /// Close a loose end; closing a closed one changes nothing.
    pub fn close_thread(&mut self, user: &str, id: i64, now: i64) -> StoreResult<(Thread, bool)> {
        let tx = self.conn.transaction()?;
        let thread = tx
            .query_row(
                &format!("SELECT {THREAD_COLUMNS} FROM threads WHERE user_id = ?1 AND seq = ?2"),
                params![user, id],
                thread_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("no loose end {id}")))?;
        if thread.closed_at.is_some() {
            return Ok((thread, true));
        }
        tx.execute(
            "UPDATE threads SET closed_at = ?1 WHERE id = ?2",
            params![now, thread.key],
        )?;
        tx.commit()?;
        Ok((
            Thread {
                closed_at: Some(now),
                ..thread
            },
            false,
        ))
    }

    /// Open loose ends, oldest first.
    pub fn open_threads(&self, user: &str) -> StoreResult<Vec<Thread>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM threads WHERE user_id = ?1 AND closed_at IS NULL
             ORDER BY opened_at, seq"
        ))?;
        let rows = stmt
            .query_map(params![user], thread_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Loose ends opened, and closed, in [from, to).
    pub fn threads_between(
        &self,
        user: &str,
        from: i64,
        to: i64,
    ) -> StoreResult<(Vec<Thread>, Vec<Thread>)> {
        let mut opened = self.conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM threads WHERE user_id = ?1 AND opened_at >= ?2 AND opened_at < ?3
             ORDER BY opened_at, seq"
        ))?;
        let opened = opened
            .query_map(params![user, from, to], thread_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut closed = self.conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM threads WHERE user_id = ?1 AND closed_at >= ?2 AND closed_at < ?3
             ORDER BY closed_at, seq"
        ))?;
        let closed = closed
            .query_map(params![user, from, to], thread_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((opened, closed))
    }

    pub fn all_threads(&self, user: &str) -> StoreResult<Vec<Thread>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM threads WHERE user_id = ?1 ORDER BY opened_at, seq"
        ))?;
        let rows = stmt
            .query_map(params![user], thread_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- check-ins -----------------------------------------------------------------

    pub fn plan(&self, user: &str) -> StoreResult<Option<Plan>> {
        Ok(self
            .conn
            .query_row(
                "SELECT user_id, enabled, every_minutes, quiet_from, quiet_until, timezone, next_due_at, oneoff_at, unanswered
                 FROM plans WHERE user_id = ?1",
                params![user],
                plan_row,
            )
            .optional()?)
    }

    pub fn save_plan(&mut self, plan: &Plan) -> StoreResult<()> {
        Ok(save_plan_in(&self.conn, plan)?)
    }

    /// Plans with a check-in due at `now`: the chain's (when on) or one asked
    /// for.
    pub fn due_plans(&self, now: i64) -> StoreResult<Vec<Plan>> {
        let mut stmt = self.conn.prepare(
            "SELECT user_id, enabled, every_minutes, quiet_from, quiet_until, timezone, next_due_at, oneoff_at, unanswered
             FROM plans WHERE (enabled = 1 AND next_due_at <= ?1) OR oneoff_at <= ?1
             ORDER BY user_id",
        )?;
        let rows = stmt
            .query_map(params![now], plan_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn raise_checkin(&mut self, user: &str, at: i64) -> StoreResult<i64> {
        self.conn.execute(
            "INSERT INTO checkins (user_id, raised_at) VALUES (?1, ?2)",
            params![user, at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Raise a check-in and save the plan that moved past it, together: a
    /// failure leaves neither, so the next tick decides again.
    pub fn raise_and_save(&mut self, plan: &Plan, at: i64) -> StoreResult<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO checkins (user_id, raised_at) VALUES (?1, ?2)",
            params![plan.user_id, at],
        )?;
        let id = tx.last_insert_rowid();
        save_plan_in(&tx, plan)?;
        tx.commit()?;
        Ok(id)
    }

    /// The person's newest check-in, if it is unanswered and its notice has
    /// not gone out: (id, raised_at). An older one is replaced by it anyway.
    pub fn unsent_checkin(&self, user: &str) -> StoreResult<Option<(i64, i64)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, raised_at, notified FROM checkins
                 WHERE user_id = ?1 AND answered_at IS NULL ORDER BY raised_at DESC, id DESC LIMIT 1",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?)),
            )
            .optional()?
            .filter(|(_, _, notified)| *notified == 0)
            .map(|(id, at, _)| (id, at)))
    }

    pub fn mark_notified(&mut self, id: i64) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE checkins SET notified = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Answer every open check-in; how many there were.
    pub fn answer_checkins(&mut self, user: &str, at: i64) -> StoreResult<usize> {
        Ok(self.conn.execute(
            "UPDATE checkins SET answered_at = ?2 WHERE user_id = ?1 AND answered_at IS NULL",
            params![user, at],
        )?)
    }

    /// (raised, answered) among check-ins raised in [from, to).
    pub fn checkin_counts(&self, user: &str, from: i64, to: i64) -> StoreResult<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COUNT(answered_at) FROM checkins WHERE user_id = ?1 AND raised_at >= ?2 AND raised_at < ?3",
            params![user, from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    pub fn all_checkins(&self, user: &str) -> StoreResult<Vec<Checkin>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, raised_at, answered_at, notified FROM checkins WHERE user_id = ?1 ORDER BY raised_at, id",
        )?;
        let rows = stmt
            .query_map(params![user], |r| {
                Ok(Checkin {
                    id: r.get(0)?,
                    raised_at: r.get(1)?,
                    answered_at: r.get(2)?,
                    notified: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
impl Store {
    /// Make every plan update for `user` fail, as a full disk would.
    pub fn fail_plan_updates_for(&mut self, user: &str) {
        self.conn
            .execute_batch(&format!(
                "CREATE TRIGGER fail_{user} BEFORE UPDATE ON plans WHEN NEW.user_id = '{user}'
                 BEGIN SELECT RAISE(ABORT, 'disk full'); END;"
            ))
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PlanDefaults;

    fn store() -> Store {
        Store::in_memory().unwrap()
    }

    #[test]
    fn one_task_is_current_and_switching_pauses_the_last() {
        let mut s = store();
        let a = s
            .set_task("owner", "Refactor IVR callback handler", 1000)
            .unwrap();
        assert!(!a.resumed && !a.unchanged && a.paused.is_none());
        let b = s.set_task("owner", "WGU assignment", 1000 + 7800).unwrap();
        let (paused, ran) = b.paused.unwrap();
        assert_eq!(paused.id, a.current.id);
        assert_eq!(paused.status, "paused");
        assert_eq!(ran, 7800);
        assert_eq!(s.active_task("owner").unwrap().unwrap().id, b.current.id);

        // Back to the first by name, in another case: resumed, not duplicated.
        let c = s
            .set_task("owner", "refactor ivr callback handler", 9000)
            .unwrap();
        assert!(c.resumed);
        assert_eq!(c.current.id, a.current.id);
        assert_eq!(c.current.active_since, Some(9000));
        assert_eq!(s.all_tasks("owner").unwrap().len(), 2);

        // The same title again changes nothing.
        let d = s
            .set_task("owner", "Refactor IVR callback handler", 9100)
            .unwrap();
        assert!(d.unchanged);
        assert_eq!(d.current.active_since, Some(9000));
    }

    #[test]
    fn titles_match_in_any_case_and_script() {
        let mut s = store();
        let etude = s.set_task("owner", "Étude", 100).unwrap().current;
        s.set_task("owner", "Laundry", 200).unwrap();
        let again = s.set_task("owner", "étude", 300).unwrap();
        assert!(again.resumed, "SQLite's lower() would not have matched É");
        assert_eq!(again.current.id, etude.id);
        assert!(s.set_task("owner", "ÉTUDE", 400).unwrap().unchanged);
    }

    #[test]
    fn finished_tasks_are_not_resumed_by_name() {
        let mut s = store();
        let a = s.set_task("owner", "Laundry", 100).unwrap().current;
        s.update_task("owner", a.id, Status::Done, 200).unwrap();
        assert!(s.active_task("owner").unwrap().is_none());
        let again = s.set_task("owner", "Laundry", 300).unwrap();
        assert!(!again.resumed);
        assert_ne!(again.current.id, a.id);
    }

    #[test]
    fn people_never_see_each_others_rows() {
        let mut s = store();
        let mine = s.set_task("owner", "Mine", 100).unwrap().current;
        let thread = s.open_thread("owner", "call the bank", 100).unwrap();
        assert!(s.task("guest", mine.id).unwrap().is_none());
        assert!(matches!(
            s.update_task("guest", mine.id, Status::Done, 200),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            s.close_thread("guest", thread.id, 200),
            Err(StoreError::NotFound(_))
        ));
        s.insert_entry("owner", 150, "working", None, None).unwrap();
        assert!(s.recent_entries("guest", 5).unwrap().is_empty());
        // The guest's own current task does not collide with the owner's.
        s.set_task("guest", "Theirs", 160).unwrap();
        assert_eq!(s.active_task("owner").unwrap().unwrap().id, mine.id);
    }

    #[test]
    fn entries_are_logged_against_the_current_task() {
        let mut s = store();
        let free = s.insert_entry("owner", 100, "coffee", None, None).unwrap();
        assert_eq!(free.task, None);
        let task = s.set_task("owner", "Inbox", 110).unwrap().current;
        let e = s
            .insert_entry(
                "owner",
                120,
                "replying to Dana",
                Some("Emails"),
                Some("tired"),
            )
            .unwrap();
        assert_eq!(e.task_id, Some(task.id));
        assert_eq!(e.task.as_deref(), Some("Inbox"));
        let recent = s.recent_entries("owner", 5).unwrap();
        assert_eq!(
            recent.iter().map(|e| e.at).collect::<Vec<_>>(),
            vec![100, 120]
        );
        assert_eq!(s.last_entry_at("owner").unwrap(), Some(120));
    }

    #[test]
    fn loose_ends_close_once() {
        let mut s = store();
        let t = s.open_thread("owner", "email Dana", 100).unwrap();
        let (closed, already) = s.close_thread("owner", t.id, 200).unwrap();
        assert!(!already);
        assert_eq!(closed.closed_at, Some(200));
        let (_, already) = s.close_thread("owner", t.id, 300).unwrap();
        assert!(already);
        assert!(s.open_threads("owner").unwrap().is_empty());
        let (opened, closed) = s.threads_between("owner", 0, 1000).unwrap();
        assert_eq!((opened.len(), closed.len()), (1, 1));
    }

    #[test]
    fn plans_round_trip_and_only_due_ones_are_listed() {
        let mut s = store();
        let mut p = Plan::new("owner", &PlanDefaults::default(), "UTC");
        p.next_due_at = Some(500);
        s.save_plan(&p).unwrap();
        assert_eq!(s.plan("owner").unwrap(), Some(p.clone()));
        assert!(s.due_plans(499).unwrap().is_empty());
        assert_eq!(s.due_plans(500).unwrap().len(), 1);
        p.enabled = false;
        p.quiet = None;
        s.save_plan(&p).unwrap();
        assert_eq!(s.plan("owner").unwrap().unwrap().quiet, None);
        assert!(s.due_plans(10_000).unwrap().is_empty());
        // A check-in asked for is due even with the chain off.
        p.oneoff_at = Some(700);
        s.save_plan(&p).unwrap();
        assert!(s.due_plans(699).unwrap().is_empty());
        assert_eq!(s.due_plans(700).unwrap()[0].oneoff_at, Some(700));
    }

    #[test]
    fn check_ins_are_counted_and_answered() {
        let mut s = store();
        let id = s.raise_checkin("owner", 100).unwrap();
        s.mark_notified(id).unwrap();
        s.raise_checkin("owner", 200).unwrap();
        assert_eq!(s.checkin_counts("owner", 0, 1000).unwrap(), (2, 0));
        assert_eq!(s.answer_checkins("owner", 250).unwrap(), 2);
        assert_eq!(s.checkin_counts("owner", 0, 1000).unwrap(), (2, 2));
        assert!(s.all_checkins("owner").unwrap()[0].notified);
    }

    #[test]
    fn a_check_in_and_its_plan_are_saved_together() {
        let mut s = store();
        let mut p = Plan::new("owner", &PlanDefaults::default(), "UTC");
        p.next_due_at = Some(500);
        s.save_plan(&p).unwrap();
        s.conn
            .execute_batch(
                "CREATE TRIGGER no_plans BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
            )
            .unwrap();
        p.unanswered = 1;
        p.next_due_at = Some(3200);
        assert!(s.raise_and_save(&p, 500).is_err());
        assert!(
            s.all_checkins("owner").unwrap().is_empty(),
            "no orphan check-in"
        );
        assert_eq!(s.plan("owner").unwrap().unwrap().unanswered, 0);
    }

    #[test]
    fn only_the_newest_unanswered_check_in_waits_for_its_notice() {
        let mut s = store();
        assert_eq!(s.unsent_checkin("owner").unwrap(), None);
        let first = s.raise_checkin("owner", 100).unwrap();
        assert_eq!(s.unsent_checkin("owner").unwrap(), Some((first, 100)));
        s.mark_notified(first).unwrap();
        assert_eq!(s.unsent_checkin("owner").unwrap(), None);
        let second = s.raise_checkin("owner", 200).unwrap();
        assert_eq!(s.unsent_checkin("owner").unwrap(), Some((second, 200)));
        s.answer_checkins("owner", 250).unwrap();
        assert_eq!(s.unsent_checkin("owner").unwrap(), None, "answered");
        assert_eq!(s.unsent_checkin("guest").unwrap(), None);
    }

    #[test]
    fn an_entry_shows_only_its_own_persons_task() {
        let mut s = store();
        s.set_task("owner", "Mine", 100).unwrap();
        s.insert_entry("owner", 110, "working", None, None).unwrap();
        // Rewire the entry to the guest's task row by hand, as a reused row
        // id after a delete could: the join still shows nothing of theirs.
        s.set_task("guest", "Theirs", 120).unwrap();
        s.conn
            .execute(
                "UPDATE entries SET task_id = (SELECT id FROM tasks WHERE user_id = 'guest')",
                [],
            )
            .unwrap();
        let entry = &s.recent_entries("owner", 1).unwrap()[0];
        assert_eq!(entry.task, None);
        assert_eq!(entry.task_id, None);
    }

    #[test]
    fn a_newer_journal_is_left_alone() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        assert!(Store::init(conn).is_err());
    }
}
