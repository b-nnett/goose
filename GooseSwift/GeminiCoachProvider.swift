import CryptoKit
import Foundation
import Security

// MARK: - GeminiStoredToken

struct GeminiStoredToken: Codable {
  let accessToken: String
  let refreshToken: String
  let expiresAt: Date?
  let updatedAt: Date

  var needsRefresh: Bool {
    if let expiresAt {
      return expiresAt.timeIntervalSinceNow < 60
    }
    return Date().timeIntervalSince(updatedAt) > 50 * 60
  }
}

// MARK: - GeminiKeychainError

enum GeminiKeychainError: Error {
  case saveFailed(OSStatus)
  case deleteFailed(OSStatus)
}

// MARK: - GeminiKeychainStore (internal facade for tests)

enum GeminiKeychainStore {
  static func save(_ token: GeminiStoredToken) throws {
    try GeminiKeychain.save(token)
  }

  static func load() throws -> GeminiStoredToken? {
    try GeminiKeychain.load()
  }

  static func delete() throws {
    try GeminiKeychain.delete()
  }
}

// MARK: - GeminiKeychain

private enum GeminiKeychain {
  private static let service = "com.goose.swift.gemini"
  private static let account = "oauth-token"

  static func save(_ token: GeminiStoredToken) throws {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    let data = try encoder.encode(token)
    let query = baseQuery()
    SecItemDelete(query as CFDictionary)

    var attributes = query
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw GeminiKeychainError.saveFailed(status)
    }
  }

  static func load() throws -> GeminiStoredToken? {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status != errSecItemNotFound else {
      return nil
    }
    guard status == errSecSuccess, let data = result as? Data else {
      return nil
    }

    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    return try decoder.decode(GeminiStoredToken.self, from: data)
  }

  static func delete() throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
      throw GeminiKeychainError.deleteFailed(status)
    }
  }

  private static func baseQuery() -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
  }
}

// MARK: - GeminiProviderError

enum GeminiProviderError: Error {
  case missingClientId
  case missingToken
  case tokenExchangeFailed(String)
  case invalidResponse
}

// MARK: - GeminiCoachProvider

@Observable
final class GeminiCoachProvider: CoachProvider {
  static let oauthClientIdKey = "goose.coach.gemini.oauthClientId"

  let id = "gemini"
  let displayName = "Gemini"
  let availablePresets: [CoachModelPreset] = [.gemini25Pro, .gemini25Flash]

  private(set) var isExchangingToken = false
  private(set) var isAuthenticated: Bool

  init() {
    isAuthenticated = (try? GeminiKeychain.load()) != nil
  }

  var oauthClientId: String {
    UserDefaults.standard.string(forKey: Self.oauthClientIdKey) ?? ""
  }

  // MARK: - PKCE helpers

  nonisolated static func generateCodeVerifier() -> String {
    var bytes = [UInt8](repeating: 0, count: 64)
    _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
    return Data(bytes).base64EncodedString()
      .replacingOccurrences(of: "+", with: "-")
      .replacingOccurrences(of: "/", with: "_")
      .replacingOccurrences(of: "=", with: "")
  }

  nonisolated static func codeChallenge(for verifier: String) -> String {
    let data = Data(verifier.utf8)
    let hashed = Data(SHA256.hash(data: data))
    return hashed.base64EncodedString()
      .replacingOccurrences(of: "+", with: "-")
      .replacingOccurrences(of: "/", with: "_")
      .replacingOccurrences(of: "=", with: "")
  }

  // MARK: - CoachProvider

  func signOut() {
    try? GeminiKeychain.delete()
    isAuthenticated = false
  }

  func send(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset
  ) async throws -> AsyncStream<String> {
    let token = try await validToken()
    let modelID = preset.geminiModelID ?? "gemini-2.5-flash"
    let urlString = "https://generativelanguage.googleapis.com/v1beta/models/\(modelID):streamGenerateContent?alt=sse"
    guard let url = URL(string: urlString) else {
      throw GeminiProviderError.invalidResponse
    }

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.timeoutInterval = 180

    let body: [String: Any] = [
      "systemInstruction": ["parts": [["text": systemPrompt]]],
      "contents": messages.map { msg -> [String: Any] in
        let role = msg.role == .user ? "user" : "model"
        return ["role": role, "parts": [["text": msg.text]]]
      },
    ]
    request.httpBody = try JSONSerialization.data(withJSONObject: body)

    return AsyncStream { continuation in
      Task {
        do {
          let (bytes, response) = try await URLSession.shared.bytes(for: request)
          guard let httpResponse = response as? HTTPURLResponse,
                (200..<300).contains(httpResponse.statusCode) else {
            continuation.finish()
            return
          }
          for try await line in bytes.lines {
            try Task.checkCancellation()
            if let delta = self.extractGeminiDelta(from: line) {
              continuation.yield(delta)
            }
          }
          continuation.finish()
        } catch {
          continuation.finish()
        }
      }
    }
  }

  // MARK: - Token exchange and refresh

  func handleRedirect(code: String, codeVerifier: String) async throws {
    isExchangingToken = true
    defer { isExchangingToken = false }

    let clientId = oauthClientId
    guard !clientId.isEmpty else {
      throw GeminiProviderError.missingClientId
    }

    let params: [String: String] = [
      "grant_type": "authorization_code",
      "code": code,
      "code_verifier": codeVerifier,
      "client_id": clientId,
      "redirect_uri": "gooseswift://oauth/gemini",
    ]

    let token = try await exchangeToken(params: params)
    try GeminiKeychain.save(token)
    isAuthenticated = true
  }

  // MARK: - Internal helpers

  nonisolated func extractGeminiDelta(from line: String) -> String? {
    guard line.hasPrefix("data:") else { return nil }
    let jsonString = String(line.dropFirst(5)).trimmingCharacters(in: .whitespaces)
    guard let data = jsonString.data(using: .utf8),
          let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let candidates = obj["candidates"] as? [[String: Any]],
          let first = candidates.first,
          let content = first["content"] as? [String: Any],
          let parts = content["parts"] as? [[String: Any]],
          let text = parts.first?["text"] as? String else { return nil }
    return text
  }

  private func validToken() async throws -> String {
    guard var stored = try GeminiKeychain.load() else {
      throw GeminiProviderError.missingToken
    }

    if stored.needsRefresh {
      stored = try await refreshToken(stored)
      try GeminiKeychain.save(stored)
    }

    return stored.accessToken
  }

  private func refreshToken(_ stored: GeminiStoredToken) async throws -> GeminiStoredToken {
    let clientId = oauthClientId
    guard !clientId.isEmpty else {
      throw GeminiProviderError.missingClientId
    }

    let params: [String: String] = [
      "grant_type": "refresh_token",
      "refresh_token": stored.refreshToken,
      "client_id": clientId,
    ]

    return try await exchangeToken(params: params)
  }

  private func exchangeToken(params: [String: String]) async throws -> GeminiStoredToken {
    let url = URL(string: "https://oauth2.googleapis.com/token")!
    let configuration = URLSessionConfiguration.ephemeral
    configuration.httpShouldSetCookies = false
    configuration.httpCookieAcceptPolicy = .never
    let session = URLSession(configuration: configuration)

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    request.httpBody = formURLEncoded(params)

    let (data, response) = try await session.data(for: request)
    guard let httpResponse = response as? HTTPURLResponse,
          (200..<300).contains(httpResponse.statusCode) else {
      let body = String(data: data, encoding: .utf8) ?? ""
      throw GeminiProviderError.tokenExchangeFailed(body)
    }

    guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let accessToken = obj["access_token"] as? String else {
      throw GeminiProviderError.invalidResponse
    }

    let refreshToken = obj["refresh_token"] as? String
      ?? (params["refresh_token"] ?? "")
    let expiresIn = obj["expires_in"] as? TimeInterval
    let expiresAt = expiresIn.map { Date().addingTimeInterval($0) }

    return GeminiStoredToken(
      accessToken: accessToken,
      refreshToken: refreshToken,
      expiresAt: expiresAt,
      updatedAt: Date()
    )
  }

  private func formURLEncoded(_ values: [String: String]) -> Data {
    var components = URLComponents()
    components.queryItems = values.map { URLQueryItem(name: $0.key, value: $0.value) }
    return Data((components.percentEncodedQuery ?? "").utf8)
  }
}
