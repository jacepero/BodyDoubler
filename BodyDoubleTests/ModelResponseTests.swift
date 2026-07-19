// Parsing of the model's JSON output (plan §5) — grammar guarantees shape at
// generation time; these tests cover the tolerant parse layer.

import XCTest
@testable import BodyDouble

final class ModelResponseTests: XCTestCase {

    func testParsesFullResponse() throws {
        let raw = """
        {"speech": "Noted, switching to WGU. The Dana email is still open.",
         "actions": [
           {"tool": "log_entry", "args": {"summary": "Switched to WGU assignment", "mood": null}},
           {"tool": "set_current_task", "args": {"title": "WGU assignment"}}
         ]}
        """
        let response = try ModelResponse.parse(raw)
        XCTAssertEqual(response.actions.count, 2)
        XCTAssertEqual(response.actions[0].tool, "log_entry")
        XCTAssertEqual(response.actions[0].args["summary"]?.stringValue,
                       "Switched to WGU assignment")
        XCTAssertEqual(response.actions[0].args["mood"], .null)
        XCTAssertEqual(response.actions[1].args["title"]?.stringValue, "WGU assignment")
        XCTAssertTrue(response.speech.hasPrefix("Noted"))
    }

    func testStripsSurroundingText() throws {
        let raw = """
        Sure! Here's the JSON:
        {"speech": "ok", "actions": []}
        Hope that helps.
        """
        let response = try ModelResponse.parse(raw)
        XCTAssertEqual(response.speech, "ok")
        XCTAssertTrue(response.actions.isEmpty)
    }

    func testBracesInsideStringsDoNotConfuseExtraction() throws {
        let raw = #"{"speech": "use {curly} braces \" fine", "actions": []}"#
        let response = try ModelResponse.parse(raw)
        XCTAssertEqual(response.speech, "use {curly} braces \" fine")
    }

    func testUnknownToolSurvivesDecoding() throws {
        let raw = #"{"speech": "hm", "actions": [{"tool": "telepathy", "args": {"x": 1}}]}"#
        let response = try ModelResponse.parse(raw)
        XCTAssertEqual(response.actions.first?.tool, "telepathy")
    }

    func testNumericStringIdsAreAccepted() throws {
        let raw = #"{"speech": "", "actions": [{"tool": "close_thread", "args": {"thread_id": "7"}}]}"#
        let response = try ModelResponse.parse(raw)
        XCTAssertEqual(response.actions.first?.args["thread_id"]?.intValue, 7)
    }

    func testNoJSONThrows() {
        XCTAssertThrowsError(try ModelResponse.parse("I refuse to answer in JSON."))
    }

    func testMissingFieldsDefaultInsteadOfThrowing() throws {
        let response = try ModelResponse.parse(#"{"speech": "just words"}"#)
        XCTAssertEqual(response.speech, "just words")
        XCTAssertTrue(response.actions.isEmpty)
    }
}
