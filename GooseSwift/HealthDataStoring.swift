import Foundation

/// Protocol abstracting HealthDataStore for dependency injection in tests.
/// Minimal surface for Phase 72 tests. Extend as needed.
protocol HealthDataStoring: AnyObject {
  var databasePath: String { get }
  func fetchTrendsSeries(metricName: String, days: Int) async throws -> [(date: String, value: Double)]
}

// TODO(future): extension HealthDataStore: HealthDataStoring {}
