// Verifies the ToolExecutor rules from plan §5 and the §10 acceptance
// criterion that malformed/unknown actions never crash or corrupt state.

import XCTest
@testable import BodyDouble

final class ToolExecutorTests: XCTestCase {
    private var store: Store!

    override func setUpWithError() throws {
        store = try Store.inMemory()
    }

    private func executor(onCheckin: ((Int) -> Void)? = nil) -> ToolExecutor {
        ToolExecutor(store: store, scheduleCheckin: onCheckin)
    }

    private func action(_ tool: String, _ args: [String: JSONValue] = [:]) -> RawAction {
        RawAction(tool: tool, args: args)
    }

    // MARK: log_entry

    func testLogEntryEnrichesExistingEntry() throws {
        let entry = try store.insertEntry(rawText: "switching to WGU assignment", source: .text)
        executor().apply([action("log_entry", [
            "summary": .string("Switched to WGU assignment"),
            "mood": .string("focused"),
        ])], entryId: entry.id)

        let stored = try XCTUnwrap(try store.entry(id: entry.id!))
        XCTAssertEqual(stored.summary, "Switched to WGU assignment")
        XCTAssertEqual(stored.mood, "focused")
        XCTAssertEqual(stored.rawText, "switching to WGU assignment") // never touched
    }

    func testLogEntryWithNullMoodKeepsMoodNil() throws {
        let entry = try store.insertEntry(rawText: "lunch", source: .voice)
        executor().apply([action("log_entry", [
            "summary": .string("Lunch break"),
            "mood": .null,
        ])], entryId: entry.id)

        let stored = try XCTUnwrap(try store.entry(id: entry.id!))
        XCTAssertNil(stored.mood)
    }

    // MARK: set_current_task

    func testSetCurrentTaskClearsPriorFlag() throws {
        executor().apply([action("set_current_task", ["title": .string("Refactor IVR")])],
                         entryId: nil)
        executor().apply([action("set_current_task", ["title": .string("WGU assignment")])],
                         entryId: nil)

        let tasks = try store.allTasks()
        XCTAssertEqual(tasks.filter { $0.isCurrent == 1 }.count, 1)
        XCTAssertEqual(try store.currentTask()?.title, "WGU assignment")
    }

    func testSetCurrentTaskReusesExistingOpenTask() throws {
        executor().apply([action("set_current_task", ["title": .string("Refactor IVR")])],
                         entryId: nil)
        executor().apply([action("set_current_task", ["title": .string("WGU assignment")])],
                         entryId: nil)
        executor().apply([action("set_current_task", ["title": .string("refactor ivr")])],
                         entryId: nil)

        XCTAssertEqual(try store.allTasks().count, 2) // no duplicate row
        XCTAssertEqual(try store.currentTask()?.title, "Refactor IVR")
    }

    // MARK: update_task

    func testUpdateTaskDoneClearsCurrentFlag() throws {
        let task = try store.setCurrentTask(title: "Refactor IVR")
        executor().apply([action("update_task", [
            "task_id": .number(Double(task.id!)),
            "status": .string("done"),
        ])], entryId: nil)

        XCTAssertNil(try store.currentTask())
        XCTAssertEqual(try store.allTasks().first?.status, "done")
    }

    func testUpdateTaskWithBadStatusIsDropped() throws {
        let task = try store.setCurrentTask(title: "Refactor IVR")
        executor().apply([action("update_task", [
            "task_id": .number(Double(task.id!)),
            "status": .string("exploded"),
        ])], entryId: nil)

        XCTAssertEqual(try store.currentTask()?.status, "active")
    }

    // MARK: threads

    func testOpenAndCloseThread() throws {
        executor().apply([action("open_thread", ["note": .string("email Dana re: schema")])],
                         entryId: nil)
        let thread = try XCTUnwrap(try store.openThreads().first)
        XCTAssertEqual(thread.note, "email Dana re: schema")

        executor().apply([action("close_thread", ["thread_id": .number(Double(thread.id!))])],
                         entryId: nil)
        XCTAssertTrue(try store.openThreads().isEmpty)
    }

    // MARK: schedule_checkin

    func testScheduleCheckinRoutedAndClamped() throws {
        var scheduled: [Int] = []
        let ex = executor { scheduled.append($0) }

        ex.apply([action("schedule_checkin", ["minutes_from_now": .number(30)])], entryId: nil)
        ex.apply([action("schedule_checkin", ["minutes_from_now": .number(1)])], entryId: nil)   // < 5: dropped
        ex.apply([action("schedule_checkin", ["minutes_from_now": .number(9999)])], entryId: nil) // > 480: dropped

        XCTAssertEqual(scheduled, [30])
    }

    // MARK: plan §5 hard rules

    func testUnknownToolIsDroppedSilently() throws {
        executor().apply([
            action("format_disk", ["target": .string("/")]),
            action("set_current_task", ["title": .string("Real task")]),
        ], entryId: nil)

        XCTAssertEqual(try store.currentTask()?.title, "Real task")
    }

    func testMoreThanThreeActionsIgnoresExtras() throws {
        executor().apply([
            action("open_thread", ["note": .string("one")]),
            action("open_thread", ["note": .string("two")]),
            action("open_thread", ["note": .string("three")]),
            action("open_thread", ["note": .string("four")]),
        ], entryId: nil)

        XCTAssertEqual(try store.openThreads().count, 3)
    }

    func testMissingArgsNeverCrash() throws {
        executor().apply([
            action("log_entry"),
            action("set_current_task"),
            action("update_task"),
            action("open_thread"),
            action("close_thread"),
            action("schedule_checkin"),
            action("none"),
        ], entryId: nil)
        // Nothing to assert beyond "did not crash"; state must be empty.
        XCTAssertTrue(try store.allTasks().isEmpty)
        XCTAssertTrue(try store.openThreads().isEmpty)
    }
}
