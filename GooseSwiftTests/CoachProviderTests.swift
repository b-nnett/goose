import XCTest
@testable import GooseSwift

@MainActor
final class CoachProviderTests: XCTestCase {

  // MARK: - COACH-01: CoachProvider protocol shape (compile-time proof)

  func testCoachProviderProtocolHasRequiredMembers() {
    let registry = CoachProviderRegistry()
    XCTAssertEqual(registry.allProviders.count, 4, "Registry must expose exactly four providers")

    for provider in registry.allProviders {
      XCTAssertFalse(provider.id.isEmpty, "\(provider.id): provider.id must be non-empty")
      XCTAssertFalse(
        provider.displayName.isEmpty,
        "\(provider.id): provider.displayName must be non-empty"
      )

      // isAuthenticated: Bool — accessing is a compile-time proof
      _ = provider.isAuthenticated

      // availablePresets accessible for all providers (non-empty for ChatGPT/Claude/Gemini;
      // Custom may be empty until model ID is configured — assert accessible only)
      let presets = provider.availablePresets
      if provider.id == "custom" {
        _ = presets
      } else {
        XCTAssertFalse(
          presets.isEmpty,
          "\(provider.id): availablePresets must be non-empty"
        )
      }
    }
  }

  // MARK: - COACH-01: send() returns AsyncStream<String> (compile-time type assertion)

  func testSendSignatureMatchesAsyncStream() {
    // Prove at compile time that send(messages:systemPrompt:preset:) returns
    // AsyncStream<String> without making any network call.
    let provider: any CoachProvider = ChatGPTCoachProvider()
    // Assigning send to a typed local variable is a compile-time proof of the signature.
    let _: ([CoachChatMessage], String, CoachModelPreset) async throws -> AsyncStream<String> = provider.send
    XCTAssertTrue(true, "AsyncStream<String> return type proven at compile time")
  }
}
