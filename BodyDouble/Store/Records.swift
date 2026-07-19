// Record types mirroring the SQLite schema (plan §4).
// Column names are snake_case in the DB; CodingKeys map them explicitly.

import Foundation
import GRDB

enum EntrySource: String, Codable {
    case voice
    case text
    case notificationReply = "notification_reply"
}

enum TaskStatus: String, Codable, CaseIterable {
    case active, paused, done, dropped
}

enum ThreadStatus: String, Codable {
    case open, closed
}

struct Entry: Codable, Identifiable, Equatable, FetchableRecord, MutablePersistableRecord {
    static let databaseTableName = "entries"

    var id: Int64?
    var createdAt: String          // ISO 8601
    var rawText: String
    var summary: String?
    var taskId: Int64?
    var mood: String?
    var source: String             // EntrySource.rawValue

    enum CodingKeys: String, CodingKey {
        case id
        case createdAt = "created_at"
        case rawText = "raw_text"
        case summary
        case taskId = "task_id"
        case mood
        case source
    }

    mutating func didInsert(_ inserted: InsertionSuccess) {
        id = inserted.rowID
    }

    var createdDate: Date? { ISO8601.parse(createdAt) }
}

struct TaskItem: Codable, Identifiable, Equatable, FetchableRecord, MutablePersistableRecord {
    static let databaseTableName = "tasks"

    var id: Int64?
    var title: String
    var status: String             // TaskStatus.rawValue
    var createdAt: String
    var updatedAt: String
    var isCurrent: Int             // at most one row = 1

    enum CodingKeys: String, CodingKey {
        case id, title, status
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case isCurrent = "is_current"
    }

    mutating func didInsert(_ inserted: InsertionSuccess) {
        id = inserted.rowID
    }

    var taskStatus: TaskStatus { TaskStatus(rawValue: status) ?? .active }
    var updatedDate: Date? { ISO8601.parse(updatedAt) }
}

struct ThreadItem: Codable, Identifiable, Equatable, FetchableRecord, MutablePersistableRecord {
    static let databaseTableName = "threads"

    var id: Int64?
    var note: String
    var status: String             // ThreadStatus.rawValue
    var createdAt: String
    var closedAt: String?

    enum CodingKeys: String, CodingKey {
        case id, note, status
        case createdAt = "created_at"
        case closedAt = "closed_at"
    }

    mutating func didInsert(_ inserted: InsertionSuccess) {
        id = inserted.rowID
    }
}

struct Checkin: Codable, Identifiable, Equatable, FetchableRecord, MutablePersistableRecord {
    static let databaseTableName = "checkins"

    var id: Int64?
    var scheduledFor: String
    var firedAt: String?
    var responded: Int

    enum CodingKeys: String, CodingKey {
        case id
        case scheduledFor = "scheduled_for"
        case firedAt = "fired_at"
        case responded
    }

    mutating func didInsert(_ inserted: InsertionSuccess) {
        id = inserted.rowID
    }
}

/// Shared ISO 8601 formatting; all timestamps in the DB go through here.
enum ISO8601 {
    private static let formatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    static func string(_ date: Date) -> String { formatter.string(from: date) }
    static func parse(_ s: String) -> Date? { formatter.date(from: s) }
}
