// SQLite persistence via GRDB (plan §4). All memory lives here — the model is
// stateless per turn and only ever sees what SessionCoordinator assembles from
// these tables.

import Foundation
import GRDB

final class Store {
    let dbQueue: DatabaseQueue

    init(dbQueue: DatabaseQueue) throws {
        self.dbQueue = dbQueue
        try Store.migrator.migrate(dbQueue)
    }

    convenience init(path: String) throws {
        try self.init(dbQueue: DatabaseQueue(path: path))
    }

    /// In-memory store for tests.
    static func inMemory() throws -> Store {
        try Store(dbQueue: DatabaseQueue())
    }

    static func makeDefault() throws -> Store {
        let dir = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true)
        return try Store(path: dir.appendingPathComponent("bodydouble.sqlite").path)
    }

    // Schema is the plan's SQL, verbatim.
    private static var migrator: DatabaseMigrator {
        var m = DatabaseMigrator()
        m.registerMigration("v1") { db in
            try db.execute(sql: """
            CREATE TABLE entries (
              id INTEGER PRIMARY KEY,
              created_at TEXT NOT NULL,
              raw_text TEXT NOT NULL,
              summary TEXT,
              task_id INTEGER,
              mood TEXT,
              source TEXT NOT NULL
            );

            CREATE TABLE tasks (
              id INTEGER PRIMARY KEY,
              title TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              is_current INTEGER DEFAULT 0
            );

            CREATE TABLE threads (
              id INTEGER PRIMARY KEY,
              note TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              closed_at TEXT
            );

            CREATE TABLE checkins (
              id INTEGER PRIMARY KEY,
              scheduled_for TEXT NOT NULL,
              fired_at TEXT,
              responded INTEGER DEFAULT 0
            );

            CREATE TABLE app_state (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            """)
        }
        return m
    }
}

// MARK: - Entries

extension Store {
    @discardableResult
    func insertEntry(rawText: String, source: EntrySource,
                     taskId: Int64? = nil, at date: Date = Date()) throws -> Entry {
        var entry = Entry(id: nil, createdAt: ISO8601.string(date), rawText: rawText,
                          summary: nil, taskId: taskId, mood: nil, source: source.rawValue)
        try dbQueue.write { db in
            try entry.insert(db)
        }
        return entry
    }

    /// Model-supplied enrichment only — never touches raw_text (plan §5).
    func enrichEntry(id: Int64, summary: String?, mood: String?) throws {
        try dbQueue.write { db in
            try db.execute(
                sql: "UPDATE entries SET summary = COALESCE(?, summary), mood = COALESCE(?, mood) WHERE id = ?",
                arguments: [summary, mood, id])
        }
    }

    func entry(id: Int64) throws -> Entry? {
        try dbQueue.read { db in try Entry.fetchOne(db, key: id) }
    }

    func recentEntries(limit: Int, excluding entryId: Int64? = nil) throws -> [Entry] {
        try dbQueue.read { db in
            if let entryId {
                return try Entry.fetchAll(
                    db,
                    sql: "SELECT * FROM entries WHERE id <> ? ORDER BY id DESC LIMIT ?",
                    arguments: [entryId, limit])
            }
            return try Entry.fetchAll(
                db, sql: "SELECT * FROM entries ORDER BY id DESC LIMIT ?", arguments: [limit])
        }
    }

    func allEntries() throws -> [Entry] {
        try dbQueue.read { db in
            try Entry.fetchAll(db, sql: "SELECT * FROM entries ORDER BY id ASC")
        }
    }

    func entries(on day: Date, calendar: Calendar = .current) throws -> [Entry] {
        let start = calendar.startOfDay(for: day)
        guard let end = calendar.date(byAdding: .day, value: 1, to: start) else { return [] }
        return try dbQueue.read { db in
            try Entry.fetchAll(
                db,
                sql: "SELECT * FROM entries WHERE created_at >= ? AND created_at < ? ORDER BY id ASC",
                arguments: [ISO8601.string(start), ISO8601.string(end)])
        }
    }
}

// MARK: - Tasks

extension Store {
    func currentTask() throws -> TaskItem? {
        try dbQueue.read { db in
            try TaskItem.fetchOne(db, sql: "SELECT * FROM tasks WHERE is_current = 1 LIMIT 1")
        }
    }

    func allTasks() throws -> [TaskItem] {
        try dbQueue.read { db in
            try TaskItem.fetchAll(db, sql: "SELECT * FROM tasks ORDER BY updated_at DESC")
        }
    }

    /// Clears any prior is_current flag (plan §5). Reuses an existing
    /// not-done/not-dropped task with the same title instead of duplicating it.
    @discardableResult
    func setCurrentTask(title: String, at date: Date = Date()) throws -> TaskItem {
        let now = ISO8601.string(date)
        return try dbQueue.write { db in
            try db.execute(sql: "UPDATE tasks SET is_current = 0 WHERE is_current = 1")
            if var existing = try TaskItem.fetchOne(
                db,
                sql: """
                SELECT * FROM tasks
                WHERE lower(title) = lower(?) AND status IN ('active','paused')
                ORDER BY updated_at DESC LIMIT 1
                """,
                arguments: [title]) {
                existing.isCurrent = 1
                existing.status = TaskStatus.active.rawValue
                existing.updatedAt = now
                try existing.update(db)
                return existing
            }
            var task = TaskItem(id: nil, title: title, status: TaskStatus.active.rawValue,
                                createdAt: now, updatedAt: now, isCurrent: 1)
            try task.insert(db)
            return task
        }
    }

    func updateTask(id: Int64, status: TaskStatus, at date: Date = Date()) throws {
        try dbQueue.write { db in
            // A task that is done/dropped is no longer the current task.
            let clearCurrent = (status == .done || status == .dropped)
            try db.execute(
                sql: """
                UPDATE tasks SET status = ?, updated_at = ?,
                  is_current = CASE WHEN ? THEN 0 ELSE is_current END
                WHERE id = ?
                """,
                arguments: [status.rawValue, ISO8601.string(date), clearCurrent, id])
        }
    }
}

// MARK: - Threads

extension Store {
    func openThreads() throws -> [ThreadItem] {
        try dbQueue.read { db in
            try ThreadItem.fetchAll(
                db, sql: "SELECT * FROM threads WHERE status = 'open' ORDER BY id ASC")
        }
    }

    func allThreads() throws -> [ThreadItem] {
        try dbQueue.read { db in
            try ThreadItem.fetchAll(db, sql: "SELECT * FROM threads ORDER BY id ASC")
        }
    }

    @discardableResult
    func openThread(note: String, at date: Date = Date()) throws -> ThreadItem {
        var thread = ThreadItem(id: nil, note: note, status: ThreadStatus.open.rawValue,
                                createdAt: ISO8601.string(date), closedAt: nil)
        try dbQueue.write { db in try thread.insert(db) }
        return thread
    }

    func closeThread(id: Int64, at date: Date = Date()) throws {
        try dbQueue.write { db in
            try db.execute(
                sql: "UPDATE threads SET status = 'closed', closed_at = ? WHERE id = ?",
                arguments: [ISO8601.string(date), id])
        }
    }
}

// MARK: - Check-ins

extension Store {
    @discardableResult
    func recordScheduledCheckin(for date: Date) throws -> Checkin {
        var checkin = Checkin(id: nil, scheduledFor: ISO8601.string(date),
                              firedAt: nil, responded: 0)
        try dbQueue.write { db in try checkin.insert(db) }
        return checkin
    }

    /// Called on reschedule: notifications that haven't fired are being replaced.
    func clearUnfiredCheckins() throws {
        try dbQueue.write { db in
            try db.execute(sql: "DELETE FROM checkins WHERE fired_at IS NULL")
        }
    }

    /// Best-effort bookkeeping: the most recent scheduled check-in at or before
    /// `date` is the one the user just answered.
    func markLatestCheckinResponded(at date: Date = Date()) throws {
        try dbQueue.write { db in
            try db.execute(
                sql: """
                UPDATE checkins SET fired_at = ?, responded = 1
                WHERE id = (SELECT id FROM checkins WHERE scheduled_for <= ?
                            ORDER BY scheduled_for DESC LIMIT 1)
                """,
                arguments: [ISO8601.string(date), ISO8601.string(date)])
        }
    }

    func allCheckins() throws -> [Checkin] {
        try dbQueue.read { db in
            try Checkin.fetchAll(db, sql: "SELECT * FROM checkins ORDER BY id ASC")
        }
    }
}

// MARK: - app_state key/value

extension Store {
    func stateValue(_ key: String) -> String? {
        try? dbQueue.read { db in
            try String.fetchOne(db, sql: "SELECT value FROM app_state WHERE key = ?",
                                arguments: [key])
        }
    }

    func setStateValue(_ key: String, _ value: String?) {
        try? dbQueue.write { db in
            if let value {
                try db.execute(
                    sql: "INSERT INTO app_state (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    arguments: [key, value])
            } else {
                try db.execute(sql: "DELETE FROM app_state WHERE key = ?", arguments: [key])
            }
        }
    }

    func allState() throws -> [String: String] {
        try dbQueue.read { db in
            let rows = try Row.fetchAll(db, sql: "SELECT key, value FROM app_state")
            return Dictionary(uniqueKeysWithValues: rows.map { ($0["key"], $0["value"]) })
        }
    }
}
