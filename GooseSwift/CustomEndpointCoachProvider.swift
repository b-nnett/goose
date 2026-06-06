import Foundation
import Security

// MARK: - CustomEndpointKeychainError

enum CustomEndpointKeychainError: Error {
  case saveFailed(OSStatus)
  case deleteFailed(OSStatus)
}

// MARK: - CustomEndpointKeychain

private enum CustomEndpointKeychain {
  // D-02: service "com.goose.swift.custom-endpoint", account "api-key" (D-03: one account per provider)
  static let service = "com.goose.swift.custom-endpoint"
  static let account = "api-key"

  static func save(_ key: String) throws {
    let data = Data(key.utf8)
    let query = baseQuery()
    SecItemDelete(query as CFDictionary)

    var attributes = query
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw CustomEndpointKeychainError.saveFailed(status)
    }
  }

  static func load() throws -> String? {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status != errSecItemNotFound else {
      return nil
    }
    guard status == errSecSuccess else {
      return nil
    }
    guard let data = result as? Data else {
      return nil
    }
    return String(data: data, encoding: .utf8)
  }

  static func delete() throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
      throw CustomEndpointKeychainError.deleteFailed(status)
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

// MARK: - CustomEndpointCredentialStore (internal facade for tests)

enum CustomEndpointCredentialStore {
  static func save(_ key: String) throws {
    try CustomEndpointKeychain.save(key)
  }

  static func load() throws -> String? {
    try CustomEndpointKeychain.load()
  }

  static func delete() throws {
    try CustomEndpointKeychain.delete()
  }

  static func currentKey() -> String {
    (try? CustomEndpointKeychain.load()) ?? ""
  }
}

// MARK: - CustomEndpointProviderError

enum CustomEndpointProviderError: Error {
  case invalidURL
  case missingAPIKey
}

// MARK: - CustomEndpointCoachProvider

@Observable
final class CustomEndpointCoachProvider: CoachProvider {
  let id = "custom"
  let displayName = "Custom"

  // D-04: custom provider uses a single dynamic preset from the user model ID.
  // CoachModelPreset is a fixed enum; custom ignores the preset param and reads modelID directly.
  let availablePresets: [CoachModelPreset] = []

  // MARK: - UserDefaults keys (D-02, COACH-02)

  static let baseURLKey = "goose.coach.custom.baseURL"
  static let modelIDKey = "goose.coach.custom.modelID"

  var baseURL: String {
    get { UserDefaults.standard.string(forKey: Self.baseURLKey) ?? "" }
    set { UserDefaults.standard.set(newValue, forKey: Self.baseURLKey) }
  }

  var modelID: String {
    get { UserDefaults.standard.string(forKey: Self.modelIDKey) ?? "" }
    set { UserDefaults.standard.set(newValue, forKey: Self.modelIDKey) }
  }

  // MARK: - URL validation (T-18-07: https required for non-local hosts via RemoteServerURLValidator)

  static func validateBaseURL(_ raw: String) -> Bool {
    RemoteServerURLValidator.validate(raw)
  }

  // MARK: - CoachProvider

  private(set) var isAuthenticated: Bool

  init() {
    let hasKey = (try? CustomEndpointKeychain.load()) != nil
    let savedURL = UserDefaults.standard.string(forKey: Self.baseURLKey) ?? ""
    isAuthenticated = hasKey && RemoteServerURLValidator.validate(savedURL)
  }

  func saveEndpoint(apiKey: String) throws {
    try CustomEndpointKeychain.save(apiKey)
    let savedURL = UserDefaults.standard.string(forKey: Self.baseURLKey) ?? ""
    isAuthenticated = !apiKey.isEmpty && Self.validateBaseURL(savedURL)
  }

  func signOut() {
    try? CustomEndpointKeychain.delete()
    UserDefaults.standard.removeObject(forKey: Self.baseURLKey)
    UserDefaults.standard.removeObject(forKey: Self.modelIDKey)
    isAuthenticated = false
  }

  func send(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset
  ) async throws -> AsyncStream<String> {
    guard Self.validateBaseURL(baseURL) else {
      throw CustomEndpointProviderError.invalidURL
    }
    guard let apiKey = try CustomEndpointKeychain.load(), !apiKey.isEmpty else {
      throw CustomEndpointProviderError.missingAPIKey
    }

    let request = try buildRequest(
      messages: messages,
      systemPrompt: systemPrompt,
      apiKey: apiKey
    )

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
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed == "data: [DONE]" {
              break
            }
            if let delta = extractCustomDelta(from: trimmed) {
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

  // MARK: - Internal helpers

  func extractCustomDelta(from line: String) -> String? {
    guard line.hasPrefix("data:") else { return nil }
    let jsonString = String(line.dropFirst(5)).trimmingCharacters(in: .whitespaces)
    if jsonString == "[DONE]" { return nil }
    guard let data = jsonString.data(using: .utf8),
          let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let choices = obj["choices"] as? [[String: Any]],
          let delta = choices.first?["delta"] as? [String: Any],
          let content = delta["content"] as? String else { return nil }
    return content
  }

  private func buildRequest(
    messages: [CoachChatMessage],
    systemPrompt: String,
    apiKey: String
  ) throws -> URLRequest {
    // Trim trailing slash from baseURL before appending the path
    let trimmedBase = baseURL.hasSuffix("/")
      ? String(baseURL.dropLast())
      : baseURL
    guard let url = URL(string: "\(trimmedBase)/v1/chat/completions") else {
      throw CustomEndpointProviderError.invalidURL
    }

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
    request.timeoutInterval = 180

    let systemMessage: [String: Any] = ["role": "system", "content": systemPrompt]
    let chatMessages: [[String: Any]] = messages.map { msg in
      ["role": msg.role == .user ? "user" : "assistant", "content": msg.text]
    }
    let allMessages = [systemMessage] + chatMessages

    let body: [String: Any] = [
      "model": modelID,
      "stream": true,
      "messages": allMessages,
    ]
    request.httpBody = try JSONSerialization.data(withJSONObject: body)
    return request
  }
}
