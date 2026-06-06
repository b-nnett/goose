import XCTest
@testable import GooseSwift

final class CoachKeychainTests: XCTestCase {

  // MARK: - COACH-02: Keychain roundtrip for Claude and Custom endpoint

  override func tearDown() {
    super.tearDown()
    // Clean up any Keychain state left by tests
    try? ClaudeCredentialStore.delete()
    try? CustomEndpointCredentialStore.delete()
  }

  func testClaudeKeychainRoundtrip() throws {
    let testKey = "test-key-\(UUID().uuidString)"

    // Save a key
    try ClaudeCredentialStore.save(testKey)

    // Load it back — must match
    let loaded = try ClaudeCredentialStore.load()
    XCTAssertEqual(loaded, testKey, "Loaded key must match the saved key")

    // Delete it
    try ClaudeCredentialStore.delete()

    // Load after delete — must be nil
    let afterDelete = try ClaudeCredentialStore.load()
    XCTAssertNil(afterDelete, "Key must be nil after deletion")
  }

  func testCustomEndpointKeychainRoundtrip() throws {
    let testKey = "custom-test-key-\(UUID().uuidString)"

    // Save a key
    try CustomEndpointCredentialStore.save(testKey)

    // Load it back — must match
    let loaded = try CustomEndpointCredentialStore.load()
    XCTAssertEqual(loaded, testKey, "Loaded custom endpoint key must match the saved key")

    // Delete it
    try CustomEndpointCredentialStore.delete()

    // Load after delete — must be nil
    let afterDelete = try CustomEndpointCredentialStore.load()
    XCTAssertNil(afterDelete, "Custom endpoint key must be nil after deletion")
  }
}
