import Foundation
import Security

enum RemoteServerStorage {
  static let serverURL = "goose.remote.serverURL"
  static let uploadEnabled = "goose.remote.uploadEnabled"
}

enum RemoteServerURLValidator {
  static func validate(_ raw: String) -> Bool {
    guard let components = URLComponents(string: raw),
          let scheme = components.scheme,
          (scheme == "http" || scheme == "https"),
          let host = components.host,
          !host.isEmpty else {
      return false
    }
    let isNumericIP = host.range(of: #"^[0-9.]+$"#, options: .regularExpression) != nil
    if isNumericIP {
      return isPrivateIP(host)
    }
    // .local and localhost are on the local network — HTTP is fine (NSAllowsLocalNetworking).
    // Public hostnames require HTTPS to satisfy App Transport Security.
    let isLocalHost = host == "localhost" || host.hasSuffix(".local")
    if isLocalHost { return true }
    return scheme == "https"
  }

  // Allows RFC 1918 private ranges: 10.x, 172.16-31.x, 192.168.x
  private static func isPrivateIP(_ host: String) -> Bool {
    let parts = host.split(separator: ".").compactMap { Int($0) }
    guard parts.count == 4, parts.allSatisfy({ $0 >= 0 && $0 <= 255 }) else { return false }
    switch parts[0] {
    case 10: return true
    case 172: return parts[1] >= 16 && parts[1] <= 31
    case 192: return parts[1] == 168
    case 127: return true
    default: return false
    }
  }
}

enum RemoteServerKeychainError: Error, LocalizedError {
  case saveFailed(OSStatus)
  case deleteFailed(OSStatus)

  var errorDescription: String? {
    switch self {
    case .saveFailed(let status):
      return "Failed to save token to Keychain: \(status)"
    case .deleteFailed(let status):
      return "Failed to delete token from Keychain: \(status)"
    }
  }
}

enum RemoteServerKeychain {
  private static let service = "goose.remote"
  private static let account = "apiKey"

  static func saveToken(_ token: String) throws {
    let data = Data(token.utf8)
    let query = baseQuery()
    SecItemDelete(query as CFDictionary)

    var attributes = query
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw RemoteServerKeychainError.saveFailed(status)
    }
  }

  static func loadToken() throws -> String? {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status != errSecItemNotFound else {
      return nil
    }
    guard status == errSecSuccess else {
      throw RemoteServerKeychainError.saveFailed(status)
    }
    guard let data = result as? Data else {
      return nil
    }
    return String(data: data, encoding: .utf8)
  }

  static func deleteToken() throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
      throw RemoteServerKeychainError.deleteFailed(status)
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
