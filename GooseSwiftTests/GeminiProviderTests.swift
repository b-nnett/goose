import XCTest
import CryptoKit
@testable import GooseSwift

@MainActor
final class GeminiProviderTests: XCTestCase {

  // MARK: - Token model

  func testNeedsRefreshWithinWindow() throws {
    // expiresAt within 60 seconds — needs refresh
    let soon = Date().addingTimeInterval(30)
    let token = GeminiStoredToken(
      accessToken: "access",
      refreshToken: "refresh",
      expiresAt: soon,
      updatedAt: Date()
    )
    XCTAssertTrue(token.needsRefresh, "Token expiring in 30s must need refresh")

    // expiresAt more than 60 seconds away — does not need refresh
    let later = Date().addingTimeInterval(120)
    let freshToken = GeminiStoredToken(
      accessToken: "access",
      refreshToken: "refresh",
      expiresAt: later,
      updatedAt: Date()
    )
    XCTAssertFalse(freshToken.needsRefresh, "Token expiring in 120s must not need refresh")

    // nil expiresAt with updatedAt over 50 minutes ago — needs refresh
    let oldToken = GeminiStoredToken(
      accessToken: "access",
      refreshToken: "refresh",
      expiresAt: nil,
      updatedAt: Date().addingTimeInterval(-51 * 60)
    )
    XCTAssertTrue(oldToken.needsRefresh, "Token updated 51 minutes ago must need refresh")

    // nil expiresAt with recent updatedAt — does not need refresh
    let recentToken = GeminiStoredToken(
      accessToken: "access",
      refreshToken: "refresh",
      expiresAt: nil,
      updatedAt: Date().addingTimeInterval(-10 * 60)
    )
    XCTAssertFalse(recentToken.needsRefresh, "Token updated 10 minutes ago must not need refresh")
  }

  // MARK: - PKCE

  func testCodeChallengeIsBase64URL() throws {
    let verifier = GeminiCoachProvider.generateCodeVerifier()

    // Verifier must be at least 43 chars
    XCTAssertGreaterThanOrEqual(verifier.count, 43, "Code verifier must be at least 43 characters")

    let challenge = GeminiCoachProvider.codeChallenge(for: verifier)

    // Must not contain URL-unsafe base64 chars
    XCTAssertFalse(challenge.contains("+"), "Code challenge must not contain '+'")
    XCTAssertFalse(challenge.contains("/"), "Code challenge must not contain '/'")
    XCTAssertFalse(challenge.contains("="), "Code challenge must not contain '='")

    // Must equal base64url(SHA256(verifier)) computed independently
    let verifierData = Data(verifier.utf8)
    let hashData = Data(SHA256.hash(data: verifierData))
    let expected = hashData.base64EncodedString()
      .replacingOccurrences(of: "+", with: "-")
      .replacingOccurrences(of: "/", with: "_")
      .replacingOccurrences(of: "=", with: "")
    XCTAssertEqual(challenge, expected, "Code challenge must equal base64url(SHA256(verifier))")
  }

  // MARK: - Keychain roundtrip

  func testGeminiKeychainRoundtrip() throws {
    let token = GeminiStoredToken(
      accessToken: "access-token-test",
      refreshToken: "refresh-token-test",
      expiresAt: Date().addingTimeInterval(3600),
      updatedAt: Date()
    )

    // Clean up before test
    try? GeminiKeychainStore.delete()

    // Save
    try GeminiKeychainStore.save(token)

    // Load
    let loaded = try GeminiKeychainStore.load()
    XCTAssertNotNil(loaded, "Loaded token must not be nil after save")
    XCTAssertEqual(loaded?.accessToken, token.accessToken, "Loaded access token must match")
    XCTAssertEqual(loaded?.refreshToken, token.refreshToken, "Loaded refresh token must match")

    // Delete
    try GeminiKeychainStore.delete()
    let afterDelete = try GeminiKeychainStore.load()
    XCTAssertNil(afterDelete, "Loaded token must be nil after delete")
  }

  // MARK: - SSE delta extraction

  func testGeminiDeltaExtraction() throws {
    let provider = GeminiCoachProvider()

    // Valid candidates[0].content.parts[0].text — must return the text
    let validLine = #"data: {"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#
    let result = provider.extractGeminiDelta(from: validLine)
    XCTAssertEqual(result, "Hello", "extractGeminiDelta must return 'Hello' for a valid candidates line")

    // Empty candidates array — must return nil
    let emptyCandidates = #"data: {"candidates":[]}"#
    let resultEmpty = provider.extractGeminiDelta(from: emptyCandidates)
    XCTAssertNil(resultEmpty, "extractGeminiDelta must return nil for empty candidates array")

    // No data: prefix — must return nil
    let noPrefix = #"{"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#
    let resultNoPrefix = provider.extractGeminiDelta(from: noPrefix)
    XCTAssertNil(resultNoPrefix, "extractGeminiDelta must return nil for line without data: prefix")

    // data: with no text key — must return nil
    let noText = #"data: {"candidates":[{"content":{"parts":[{"image":"abc"}]}}]}"#
    let resultNoText = provider.extractGeminiDelta(from: noText)
    XCTAssertNil(resultNoText, "extractGeminiDelta must return nil when text key is absent")
  }

  // MARK: - Available presets

  func testAvailablePresets() throws {
    let provider = GeminiCoachProvider()
    let presets = provider.availablePresets

    XCTAssertEqual(presets.count, 2, "Gemini provider must have exactly 2 presets")
    XCTAssertTrue(presets.contains(.gemini25Pro), "Gemini provider must include gemini25Pro")
    XCTAssertTrue(presets.contains(.gemini25Flash), "Gemini provider must include gemini25Flash")
  }
}
