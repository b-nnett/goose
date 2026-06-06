import XCTest
@testable import GooseSwift

final class ClaudeProviderTests: XCTestCase {

  // MARK: - COACH-03: ClaudeCoachProvider SSE delta extraction

  func testClaudeDeltaExtraction() throws {
    let provider = ClaudeCoachProvider()

    // Valid content_block_delta with text_delta — must return the text
    let validLine = #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#
    let result = provider.extractClaudeDelta(from: validLine)
    XCTAssertEqual(result, "Hello", "extractClaudeDelta must return 'Hello' for a valid content_block_delta/text_delta line")

    // message_start event — must return nil
    let messageStartLine = #"data: {"type":"message_start"}"#
    let resultMessageStart = provider.extractClaudeDelta(from: messageStartLine)
    XCTAssertNil(resultMessageStart, "extractClaudeDelta must return nil for a message_start event")

    // event: line (not data: prefix) — must return nil
    let eventLine = "event: content_block_delta"
    let resultEventLine = provider.extractClaudeDelta(from: eventLine)
    XCTAssertNil(resultEventLine, "extractClaudeDelta must return nil for a line without data: prefix")

    // data: line with wrong inner delta type — must return nil
    let wrongInnerType = #"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#
    let resultWrongInner = provider.extractClaudeDelta(from: wrongInnerType)
    XCTAssertNil(resultWrongInner, "extractClaudeDelta must return nil for non-text_delta delta type")

    // Empty data: line — must return nil
    let emptyData = "data: {}"
    let resultEmpty = provider.extractClaudeDelta(from: emptyData)
    XCTAssertNil(resultEmpty, "extractClaudeDelta must return nil for empty JSON object")
  }

  func testAvailablePresets() throws {
    let provider = ClaudeCoachProvider()
    let presets = provider.availablePresets

    XCTAssertEqual(presets.count, 3, "Claude provider must have exactly 3 presets")
    XCTAssertTrue(presets.contains(.claudeOpus48), "Claude provider must include claudeOpus48")
    XCTAssertTrue(presets.contains(.claudeSonnet46), "Claude provider must include claudeSonnet46")
    XCTAssertTrue(presets.contains(.claudeHaiku45), "Claude provider must include claudeHaiku45")
  }
}
