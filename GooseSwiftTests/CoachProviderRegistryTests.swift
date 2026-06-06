import XCTest
@testable import GooseSwift

@MainActor
final class CoachProviderRegistryTests: XCTestCase {

  // MARK: - COACH-06: Registry persists active provider ID to UserDefaults

  func testRegistryPersistsActiveProviderID() {
    let registry = CoachProviderRegistry()
    registry.selectProvider(id: "chatgpt")
    let stored = UserDefaults.standard.string(forKey: "goose.coach.activeProviderId")
    XCTAssertEqual(stored, "chatgpt", "Selecting chatgpt must persist 'chatgpt' to goose.coach.activeProviderId")
  }

  // MARK: - COACH-01: Registry exposes all four providers

  func testRegistryExposesAllFourProviders() {
    let registry = CoachProviderRegistry()
    XCTAssertEqual(registry.allProviders.count, 4, "Registry must expose exactly four providers")
    let ids = Set(registry.allProviders.map { $0.id })
    XCTAssertEqual(ids, ["chatgpt", "claude", "gemini", "custom"], "Provider IDs must be exactly the four expected values")
  }
}
