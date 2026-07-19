// Phase 4 export (plan §10: "all data readable via a single exported file").

import Foundation

enum Exporter {
    struct Dump: Codable {
        let exportedAt: String
        let entries: [Entry]
        let tasks: [TaskItem]
        let threads: [ThreadItem]
        let checkins: [Checkin]
        let settings: [String: String]
    }

    static func exportJSON(store: Store) throws -> URL {
        let dump = Dump(
            exportedAt: ISO8601.string(Date()),
            entries: try store.allEntries(),
            tasks: try store.allTasks(),
            threads: try store.allThreads(),
            checkins: try store.allCheckins(),
            settings: try store.allState())
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(dump)
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("bodydouble-export-\(SessionCoordinator.dayStamp(Date())).json")
        try data.write(to: url, options: .atomic)
        return url
    }

    static func exportMarkdown(store: Store) throws -> URL {
        var md: [String] = ["# Body Double journal export",
                            "", "Exported \(ISO8601.string(Date()))", ""]

        let entries = try store.allEntries()
        let byDay = Dictionary(grouping: entries) { entry in
            entry.createdDate.map(SessionCoordinator.dayStamp) ?? "unknown"
        }
        for day in byDay.keys.sorted() {
            md.append("## \(day)")
            md.append("")
            for e in byDay[day] ?? [] {
                let time = e.createdDate.map(SessionCoordinator.timeOnly) ?? "?"
                var line = "- **\(time)** \(e.rawText)"
                if let summary = e.summary { line += " — _\(summary)_" }
                if let mood = e.mood { line += " `[\(mood)]`" }
                md.append(line)
            }
            md.append("")
        }

        let tasks = try store.allTasks()
        if !tasks.isEmpty {
            md.append("## Tasks")
            md.append("")
            for t in tasks {
                let marker = t.isCurrent == 1 ? " ← current" : ""
                md.append("- [\(t.status)] \(t.title)\(marker)")
            }
            md.append("")
        }

        let threads = try store.allThreads()
        if !threads.isEmpty {
            md.append("## Threads")
            md.append("")
            for t in threads {
                md.append("- [\(t.status)] \(t.note)")
            }
            md.append("")
        }

        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("bodydouble-journal-\(SessionCoordinator.dayStamp(Date())).md")
        try md.joined(separator: "\n").write(to: url, atomically: true, encoding: .utf8)
        return url
    }
}
