// Applies model tool calls to the DB (plan §5 rules):
// - 0-3 actions, executed in order; anything past 3 is ignored
// - unknown tool or bad args -> dropped silently, never crash
// - log_entry only ENRICHES the already-inserted entry (the app inserts the
//   raw entry unconditionally before inference — plan §10 guarantee)
// - set_current_task clears any prior is_current flag (done inside Store)

import Foundation

struct ToolExecutor {
    static let maxActions = 3
    static let checkinMinuteBounds: ClosedRange<Int64> = 5...480

    let store: Store
    /// Injected so the executor stays testable without UNUserNotificationCenter.
    var scheduleCheckin: ((Int) -> Void)?

    func apply(_ actions: [RawAction], entryId: Int64?) {
        for action in actions.prefix(Self.maxActions) {
            // Individually guarded: one bad action must not stop the rest.
            do { try apply(action, entryId: entryId) } catch { /* drop silently */ }
        }
    }

    private func apply(_ action: RawAction, entryId: Int64?) throws {
        switch action.tool {
        case "log_entry":
            guard let entryId else { return }
            let summary = nonEmpty(action.args["summary"]?.stringValue)
            let mood = nonEmpty(action.args["mood"]?.stringValue)
            try store.enrichEntry(id: entryId, summary: summary, mood: mood)

        case "set_current_task":
            guard let title = nonEmpty(action.args["title"]?.stringValue) else { return }
            try store.setCurrentTask(title: title)

        case "update_task":
            guard let id = action.args["task_id"]?.intValue,
                  let rawStatus = action.args["status"]?.stringValue,
                  let status = TaskStatus(rawValue: rawStatus) else { return }
            try store.updateTask(id: id, status: status)

        case "open_thread":
            guard let note = nonEmpty(action.args["note"]?.stringValue) else { return }
            try store.openThread(note: note)

        case "close_thread":
            guard let id = action.args["thread_id"]?.intValue else { return }
            try store.closeThread(id: id)

        case "schedule_checkin":
            guard let minutes = action.args["minutes_from_now"]?.intValue,
                  Self.checkinMinuteBounds.contains(minutes) else { return }
            scheduleCheckin?(Int(minutes))

        case "none":
            break

        default:
            break // unknown tool -> drop silently (plan §5)
        }
    }

    private func nonEmpty(_ s: String?) -> String? {
        guard let s = s?.trimmingCharacters(in: .whitespacesAndNewlines), !s.isEmpty else {
            return nil
        }
        return s
    }
}
