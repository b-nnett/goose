import XCTest
@testable import GooseSwift

final class CustomEndpointProviderTests: XCTestCase {

  // MARK: - COACH-04: CustomEndpointCoachProvider URL validation

  func testCustomEndpointURLValidationRejectsHTTPExternal() {
    XCTAssertFalse(
      CustomEndpointCoachProvider.validateBaseURL("http://example.com"),
      "http:// external URL must be rejected"
    )
    XCTAssertTrue(
      CustomEndpointCoachProvider.validateBaseURL("https://example.com"),
      "https:// external URL must be accepted"
    )
    XCTAssertTrue(
      CustomEndpointCoachProvider.validateBaseURL("http://localhost:11434"),
      "http://localhost must be accepted"
    )
  }

  func testCustomEndpointConfigPersistence() {
    let provider = CustomEndpointCoachProvider()
    let testURL = "https://api.custom-test-\(UUID().uuidString).com"
    let testModelID = "custom-model-\(UUID().uuidString)"

    provider.baseURL = testURL
    provider.modelID = testModelID

    XCTAssertEqual(provider.baseURL, testURL, "baseURL must persist via UserDefaults")
    XCTAssertEqual(provider.modelID, testModelID, "modelID must persist via UserDefaults")

    // Clean up
    provider.baseURL = ""
    provider.modelID = ""
  }

  // MARK: - COACH-04: SSE delta extraction

  func testCustomDeltaExtraction() {
    let provider = CustomEndpointCoachProvider()

    let contentLine = #"data: {"choices":[{"delta":{"content":"Hi"}}]}"#
    XCTAssertEqual(provider.extractCustomDelta(from: contentLine), "Hi")

    let doneLine = "data: [DONE]"
    XCTAssertNil(provider.extractCustomDelta(from: doneLine), "[DONE] must return nil")

    let roleOnlyLine = #"data: {"choices":[{"delta":{"role":"assistant"}}]}"#
    XCTAssertNil(
      provider.extractCustomDelta(from: roleOnlyLine),
      "Role-only delta must return nil"
    )
  }
}
