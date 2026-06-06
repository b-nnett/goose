import Foundation
import Security

struct CodexSelfContainedDeviceCode: Equatable {
  let loginID: String
  let verificationURL: URL
  let userCode: String
  let interval: UInt64
}

struct CodexStoredChatGPTAuth: Codable {
  let schema: String
  let issuer: String
  let clientID: String
  let accountID: String?
  let idToken: String
  let accessToken: String
  let refreshToken: String
  let updatedAt: Date
  let expiresAt: Date?

  var needsRefresh: Bool {
    if let expiresAt {
      return expiresAt.timeIntervalSinceNow < 60
    }
    return Date().timeIntervalSince(updatedAt) > 50 * 60
  }

  var ageSummary: String {
    let seconds = max(0, Int(Date().timeIntervalSince(updatedAt).rounded()))
    if seconds < 60 {
      return "updated \(seconds)s ago"
    }
    let minutes = seconds / 60
    if minutes < 60 {
      return "updated \(minutes)m ago"
    }
    return "updated \(minutes / 60)h ago"
  }
}

private struct CodexDeviceCodeRequest: Encodable {
  let clientID: String

  enum CodingKeys: String, CodingKey {
    case clientID = "client_id"
  }
}

private struct CodexDeviceCodeResponse: Decodable {
  let deviceAuthID: String
  let userCode: String
  let interval: UInt64

  enum CodingKeys: String, CodingKey {
    case deviceAuthID = "device_auth_id"
    case userCode = "user_code"
    case usercode
    case interval
  }

  init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    deviceAuthID = try container.decode(String.self, forKey: .deviceAuthID)
    if let userCodeValue = try? container.decode(String.self, forKey: .userCode) {
      userCode = userCodeValue
    } else {
      userCode = try container.decode(String.self, forKey: .usercode)
    }
    if let stringInterval = try? container.decode(String.self, forKey: .interval) {
      interval = UInt64(stringInterval) ?? 5
    } else {
      interval = (try? container.decode(UInt64.self, forKey: .interval)) ?? 5
    }
  }
}

private struct CodexDeviceTokenPollRequest: Encodable {
  let deviceAuthID: String
  let userCode: String

  enum CodingKeys: String, CodingKey {
    case deviceAuthID = "device_auth_id"
    case userCode = "user_code"
  }
}

private struct CodexDeviceTokenPollResponse: Decodable {
  let authorizationCode: String
  let codeChallenge: String
  let codeVerifier: String

  enum CodingKeys: String, CodingKey {
    case authorizationCode = "authorization_code"
    case codeChallenge = "code_challenge"
    case codeVerifier = "code_verifier"
  }
}

private struct CodexTokenExchangeResponse: Decodable {
  let idToken: String?
  let accessToken: String
  let refreshToken: String?
  let expiresIn: TimeInterval?

  enum CodingKeys: String, CodingKey {
    case idToken = "id_token"
    case accessToken = "access_token"
    case refreshToken = "refresh_token"
    case expiresIn = "expires_in"
  }
}

enum CodexSelfContainedAuthError: Error, LocalizedError {
  case invalidURL(String)
  case httpStatus(Int, String)
  case invalidResponse(String)
  case keychainSaveFailed(OSStatus)
  case keychainDeleteFailed(OSStatus)

  var errorDescription: String? {
    switch self {
    case .invalidURL(let url):
      return "Invalid auth URL: \(url)"
    case .httpStatus(let status, let body):
      return "Auth request failed with HTTP \(status): \(body)"
    case .invalidResponse(let message):
      return message
    case .keychainSaveFailed(let status):
      return "Failed to save Codex auth to Keychain: \(status)"
    case .keychainDeleteFailed(let status):
      return "Failed to clear Codex auth from Keychain: \(status)"
    }
  }
}

actor CodexSelfContainedAuthClient {
  private let issuer = "https://auth.openai.com"
  private let clientID = "app_EMoamEEZ73f0CkXaXp7hrann"
  private let maxDeviceCodeWaitSeconds: UInt64 = 15 * 60
  private let session: URLSession

  init() {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.httpShouldSetCookies = false
    configuration.httpCookieAcceptPolicy = .never
    configuration.waitsForConnectivity = true
    session = URLSession(configuration: configuration)
  }

  func requestDeviceCode() async throws -> CodexSelfContainedDeviceCode {
    let response: CodexDeviceCodeResponse = try await postJSON(
      path: "/api/accounts/deviceauth/usercode",
      body: CodexDeviceCodeRequest(clientID: clientID)
    )
    guard let verificationURL = URL(string: "\(issuer)/codex/device") else {
      throw CodexSelfContainedAuthError.invalidURL("\(issuer)/codex/device")
    }
    return CodexSelfContainedDeviceCode(
      loginID: response.deviceAuthID,
      verificationURL: verificationURL,
      userCode: response.userCode,
      interval: response.interval
    )
  }

  func requestDeviceCodeWithRetry(maxAttempts: Int = 3) async throws -> CodexSelfContainedDeviceCode {
    var lastError: Error?
    for attempt in 1...max(maxAttempts, 1) {
      do {
        return try await requestDeviceCode()
      } catch let error as CancellationError {
        throw error
      } catch {
        lastError = error
        if attempt < maxAttempts {
          try await Task.sleep(for: .seconds(UInt64(attempt)))
        }
      }
    }
    throw lastError ?? CodexSelfContainedAuthError.invalidResponse("Device code request failed.")
  }

  func completeDeviceCodeLogin(_ deviceCode: CodexSelfContainedDeviceCode) async throws -> CodexStoredChatGPTAuth {
    let pollResponse = try await pollForAuthorizationCode(deviceCode)
    let tokenResponse = try await exchangeCodeForTokens(pollResponse)
    guard let idToken = tokenResponse.idToken,
          let refreshToken = tokenResponse.refreshToken else {
      throw CodexSelfContainedAuthError.invalidResponse("Auth token exchange did not include all required tokens.")
    }
    let auth = CodexStoredChatGPTAuth(
      schema: "goose.codex.chatgpt-auth.v1",
      issuer: issuer,
      clientID: clientID,
      accountID: CodexChatGPTTokenClaims.accountID(in: tokenResponse.accessToken)
        ?? CodexChatGPTTokenClaims.accountID(in: idToken),
      idToken: idToken,
      accessToken: tokenResponse.accessToken,
      refreshToken: refreshToken,
      updatedAt: Date(),
      expiresAt: expiresAt(from: tokenResponse.expiresIn)
    )
    try CodexSelfContainedAuthKeychain.save(auth)
    return auth
  }

  func storedAuth(refreshIfNeeded: Bool = true) async throws -> CodexStoredChatGPTAuth? {
    guard var auth = try CodexSelfContainedAuthKeychain.load() else {
      return nil
    }
    guard refreshIfNeeded, auth.needsRefresh else {
      return auth
    }
    auth = try await refreshStoredAuth(auth)
    try CodexSelfContainedAuthKeychain.save(auth)
    return auth
  }

  func clearStoredAuth() throws {
    try CodexSelfContainedAuthKeychain.delete()
  }

  private func pollForAuthorizationCode(_ deviceCode: CodexSelfContainedDeviceCode) async throws -> CodexDeviceTokenPollResponse {
    let startedAt = Date()
    while Date().timeIntervalSince(startedAt) < TimeInterval(maxDeviceCodeWaitSeconds) {
      try Task.checkCancellation()
      do {
        return try await postJSON(
          path: "/api/accounts/deviceauth/token",
          body: CodexDeviceTokenPollRequest(
            deviceAuthID: deviceCode.loginID,
            userCode: deviceCode.userCode
          )
        )
      } catch CodexSelfContainedAuthError.httpStatus(let status, _) where status == 403 || status == 404 {
        try await Task.sleep(for: .seconds(max(deviceCode.interval, 1)))
      }
    }
    throw CodexSelfContainedAuthError.invalidResponse("Device code login timed out after \(maxDeviceCodeWaitSeconds / 60) minutes.")
  }

  private func exchangeCodeForTokens(_ code: CodexDeviceTokenPollResponse) async throws -> CodexTokenExchangeResponse {
    guard let url = URL(string: "\(issuer)/oauth/token") else {
      throw CodexSelfContainedAuthError.invalidURL("\(issuer)/oauth/token")
    }

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    request.setValue("goose-swift", forHTTPHeaderField: "originator")
    request.httpBody = formURLEncoded([
      "grant_type": "authorization_code",
      "code": code.authorizationCode,
      "redirect_uri": "\(issuer)/deviceauth/callback",
      "client_id": clientID,
      "code_verifier": code.codeVerifier,
    ])

    let (data, response) = try await session.data(for: request)
    try validate(response: response, data: data)
    return try JSONDecoder().decode(CodexTokenExchangeResponse.self, from: data)
  }

  private func refreshStoredAuth(_ auth: CodexStoredChatGPTAuth) async throws -> CodexStoredChatGPTAuth {
    guard let url = URL(string: "\(issuer)/oauth/token") else {
      throw CodexSelfContainedAuthError.invalidURL("\(issuer)/oauth/token")
    }

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    request.setValue("goose-swift", forHTTPHeaderField: "originator")
    request.httpBody = formURLEncoded([
      "grant_type": "refresh_token",
      "refresh_token": auth.refreshToken,
      "client_id": clientID,
    ])

    let (data, response) = try await session.data(for: request)
    try validate(response: response, data: data)
    let tokenResponse = try JSONDecoder().decode(CodexTokenExchangeResponse.self, from: data)
    return CodexStoredChatGPTAuth(
      schema: auth.schema,
      issuer: auth.issuer,
      clientID: auth.clientID,
      accountID: CodexChatGPTTokenClaims.accountID(in: tokenResponse.accessToken) ?? auth.accountID,
      idToken: tokenResponse.idToken ?? auth.idToken,
      accessToken: tokenResponse.accessToken,
      refreshToken: tokenResponse.refreshToken ?? auth.refreshToken,
      updatedAt: Date(),
      expiresAt: expiresAt(from: tokenResponse.expiresIn)
    )
  }

  private func postJSON<RequestBody: Encodable, ResponseBody: Decodable>(
    path: String,
    body: RequestBody
  ) async throws -> ResponseBody {
    guard let url = URL(string: "\(issuer)\(path)") else {
      throw CodexSelfContainedAuthError.invalidURL("\(issuer)\(path)")
    }

    var request = URLRequest(url: url)
    request.httpMethod = "POST"
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    request.setValue("goose-swift", forHTTPHeaderField: "originator")
    request.httpBody = try JSONEncoder().encode(body)

    let (data, response) = try await session.data(for: request)
    try validate(response: response, data: data)
    return try JSONDecoder().decode(ResponseBody.self, from: data)
  }

  private func validate(response: URLResponse, data: Data) throws {
    guard let httpResponse = response as? HTTPURLResponse else {
      throw CodexSelfContainedAuthError.invalidResponse("Auth server returned a non-HTTP response.")
    }
    guard (200..<300).contains(httpResponse.statusCode) else {
      let body = String(data: data, encoding: .utf8) ?? ""
      throw CodexSelfContainedAuthError.httpStatus(httpResponse.statusCode, body)
    }
  }

  private func formURLEncoded(_ values: [String: String]) -> Data {
    var components = URLComponents()
    components.queryItems = values.map { URLQueryItem(name: $0.key, value: $0.value) }
    return Data((components.percentEncodedQuery ?? "").utf8)
  }

  private func expiresAt(from expiresIn: TimeInterval?) -> Date? {
    guard let expiresIn else {
      return nil
    }
    return Date().addingTimeInterval(expiresIn)
  }
}

enum CodexChatGPTTokenClaims {
  static func accountID(in jwt: String) -> String? {
    let parts = jwt.split(separator: ".")
    guard parts.count >= 2,
          let data = Data(base64URLEncoded: String(parts[1])),
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
      return nil
    }

    if let accountID = json["chatgpt_account_id"] as? String {
      return accountID
    }
    if let auth = json["https://api.openai.com/auth"] as? [String: Any] {
      return auth["chatgpt_account_id"] as? String
    }
    return nil
  }
}

private extension Data {
  init?(base64URLEncoded value: String) {
    var base64 = value
      .replacingOccurrences(of: "-", with: "+")
      .replacingOccurrences(of: "_", with: "/")
    let padding = base64.count % 4
    if padding > 0 {
      base64.append(String(repeating: "=", count: 4 - padding))
    }
    self.init(base64Encoded: base64)
  }
}

enum CodexSelfContainedAuthKeychain {
  private static let service = "com.goose.swift.codex"
  private static let account = "chatgpt-auth"

  static func save(_ auth: CodexStoredChatGPTAuth) throws {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    let data = try encoder.encode(auth)
    let query = baseQuery()
    SecItemDelete(query as CFDictionary)

    var attributes = query
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw CodexSelfContainedAuthError.keychainSaveFailed(status)
    }
  }

  static func load() throws -> CodexStoredChatGPTAuth? {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status != errSecItemNotFound else {
      return nil
    }
    guard status == errSecSuccess else {
      throw CodexSelfContainedAuthError.invalidResponse("Failed to read Codex auth from Keychain: \(status)")
    }
    guard let data = result as? Data else {
      throw CodexSelfContainedAuthError.invalidResponse("Codex auth Keychain item did not contain data.")
    }

    return try decodeStoredAuth(from: data)
  }

  static func delete() throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
      throw CodexSelfContainedAuthError.keychainDeleteFailed(status)
    }
  }

  private static func baseQuery() -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
  }

  private static func decodeStoredAuth(from data: Data) throws -> CodexStoredChatGPTAuth {
    let isoDecoder = JSONDecoder()
    isoDecoder.dateDecodingStrategy = .iso8601
    do {
      return try isoDecoder.decode(CodexStoredChatGPTAuth.self, from: data)
    } catch {
      let legacyDecoder = JSONDecoder()
      return try legacyDecoder.decode(CodexStoredChatGPTAuth.self, from: data)
    }
  }
}
