import Foundation

/// Protocol abstracting GooseRustBridge so callers can be tested with MockRustBridge.
/// Only the two methods used by WorkoutEntryViewModel and TrendsDashboardView are required.
/// Expand as additional tests are added in future phases.
protocol GooseRustBridging: AnyObject {
  func request(method: String, args: [String: Any]) throws -> [String: Any]
  func requestAsync(method: String, args: [String: Any]) async throws -> [String: Any]
}

extension GooseRustBridge: GooseRustBridging {}
