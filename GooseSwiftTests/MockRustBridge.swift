import Foundation
@testable import GooseSwift

/// Test double for GooseRustBridging. Records the last method called.
final class MockRustBridge: GooseRustBridging {
  var lastMethod: String?
  var lastArgs: [String: Any] = [:]
  var stubbedResult: [String: Any] = [:]
  var shouldThrow = false

  func request(method: String, args: [String: Any]) throws -> [String: Any] {
    guard !shouldThrow else { throw MockError.forced }
    lastMethod = method
    lastArgs = args
    return stubbedResult
  }

  func requestAsync(method: String, args: [String: Any]) async throws -> [String: Any] {
    guard !shouldThrow else { throw MockError.forced }
    lastMethod = method
    lastArgs = args
    return stubbedResult
  }

  enum MockError: Error { case forced }
}
