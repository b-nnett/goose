import Foundation
import Security

// MARK: - ClaudeKeychainError

enum ClaudeKeychainError: Error {
  case saveFailed(OSStatus)
  case deleteFailed(OSStatus)
}

// MARK: - ClaudeKeychain

enum ClaudeKeychain {
  private static let service = "com.goose.swift.claude"
  private static let account = "api-key"

  static func save(_ key: String) throws {
    let data = Data(key.utf8)
    let query = baseQuery()
    SecItemDelete(query as CFDictionary)

    var attributes = query
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw ClaudeKeychainError.saveFailed(status)
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
      throw ClaudeKeychainError.deleteFailed(status)
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

// MARK: - ClaudeCredentialStore (internal facade for tests)

enum ClaudeCredentialStore {
  static func save(_ key: String) throws {
    try ClaudeKeychain.save(key)
  }

  static func load() throws -> String? {
    try ClaudeKeychain.load()
  }

  static func delete() throws {
    try ClaudeKeychain.delete()
  }
}

// MARK: - ClaudeProviderError

enum ClaudeProviderError: Error {
  case missingAPIKey
  case invalidResponse
}

// MARK: - ClaudeCoachProvider

@Observable
final class ClaudeCoachProvider: CoachProvider {
  let id = "claude"
  let displayName = "Claude"
  let availablePresets: [CoachModelPreset] = [.claudeOpus48, .claudeSonnet46, .claudeHaiku45]

  private(set) var isAuthenticated: Bool

  init() {
    isAuthenticated = (try? ClaudeKeychain.load()) != nil
  }

  func saveAPIKey(_ key: String) throws {
    try ClaudeKeychain.save(key)
    isAuthenticated = true
  }

  func signOut() {
    try? ClaudeKeychain.delete()
    isAuthenticated = false
  }

  func send(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset
  ) async throws -> AsyncStream<String> {
    guard let apiKey = try ClaudeKeychain.load(), !apiKey.isEmpty else {
      throw ClaudeProviderError.missingAPIKey
    }

    let request = try buildRequest(
      messages: messages,
      systemPrompt: systemPrompt,
      preset: preset,
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
            if let delta = extractClaudeDelta(from: line) {
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

  func extractClaudeDelta(from line: String) -> String? {
    guard line.hasPrefix("data:") else { return nil }
    let jsonString = String(line.dropFirst(5)).trimmingCharacters(in: .whitespaces)
    guard let data = jsonString.data(using: .utf8),
          let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          obj["type"] as? String == "content_block_delta",
          let delta = obj["delta"] as? [String: Any],
          delta["type"] as? String == "text_delta",
          let text = delta["text"] as? String else { return nil }
    return text
  }

  private func buildRequest(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset,
    apiKey: String
  ) throws -> URLRequest {
    let url = URL(string: "https://api.anthropic.com/v1/messages")!
    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
    request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
    request.timeoutInterval = 180

    let modelID = preset.claudeModelID ?? "claude-sonnet-4-6"
    let body: [String: Any] = [
      "model": modelID,
      "max_tokens": 4096,
      "system": systemPrompt,
      "stream": true,
      "messages": messages.map { msg in
        ["role": msg.role == .user ? "user" : "assistant", "content": msg.text]
      },
    ]
    request.httpBody = try JSONSerialization.data(withJSONObject: body)
    return request
  }
}
