// Store-level invariants (plan §4).

import XCTest
@testable import BodyDouble

final class StoreTests: XCTestCase {
    private var store: Store!

    override func setUpWithError() throws {
        store = try Store.inMemory()
    }

    func testInsertEntryAssignsIdAndRoundTrips() throws {
        let entry = try store.insertEntry(rawText: "started refactor after standup",
                                          source: .voice, taskId: nil)
        let id = try XCTUnwrap(entry.id)
        let fetched = try XCTUnwrap(try store.entry(id: id))
        XCTAssertEqual(fetched.rawText, "started refactor after standup")
        XCTAssertEqual(fetched.source, "voice")
        XCTAssertNotNil(fetched.createdDate)
    }

    func testAtMostOneCurrentTask() throws {
        try store.setCurrentTask(title: "A")
        try store.setCurrentTask(title: "B")
        try store.setCurrentTask(title: "C")
        let currents = try store.allTasks().filter { $0.isCurrent == 1 }
        XCTAssertEqual(currents.count, 1)
        XCTAssertEqual(currents.first?.title, "C")
    }

    func testRecentEntriesExcludesGivenId() throws {
        let a = try store.insertEntry(rawText: "a", source: .text)
        let b = try store.insertEntry(rawText: "b", source: .text)
        let recents = try store.recentEntries(limit: 10, excluding: b.id)
        XCTAssertEqual(recents.map(\.rawText), ["a"])
        XCTAssertNotNil(a.id)
    }

    func testEnrichNeverOverwritesWithNil() throws {
        let entry = try store.insertEntry(rawText: "x", source: .text)
        try store.enrichEntry(id: entry.id!, summary: "first", mood: "tired")
        try store.enrichEntry(id: entry.id!, summary: nil, mood: nil)
        let fetched = try XCTUnwrap(try store.entry(id: entry.id!))
        XCTAssertEqual(fetched.summary, "first")
        XCTAssertEqual(fetched.mood, "tired")
    }

    func testStateRoundTripAndDelete() throws {
        store.setStateValue("checkin_interval_min", "45")
        XCTAssertEqual(store.stateValue("checkin_interval_min"), "45")
        store.setStateValue("checkin_interval_min", "60")
        XCTAssertEqual(store.stateValue("checkin_interval_min"), "60")
        store.setStateValue("checkin_interval_min", nil)
        XCTAssertNil(store.stateValue("checkin_interval_min"))
    }
}
