import XCTest
@testable import GooseSwift

final class TrendsFetchTests: XCTestCase {

  func test_fetchTrendsSeries_calls_metric_series_query_range() async throws {
    // MockHealthStore delegates to bridge — asserts the correct method string is used.
    let bridge = MockRustBridge()
    bridge.stubbedResult = ["rows": [[String: Any]]()]
    let store = MockHealthStore(bridge: bridge)
    _ = try await store.fetchTrendsSeries(metricName: "recovery", days: 7)
    XCTAssertEqual(bridge.lastMethod, "metric_series.query_range",
      "fetchTrendsSeries must call bridge with method 'metric_series.query_range'")
  }

  @MainActor
  func test_workout_entry_calls_workout_upsert() async throws {
    let mock = MockRustBridge()
    mock.stubbedResult = ["ok": true]
    let vm = WorkoutEntryViewModel(bridge: mock, databasePath: "")
    vm.selectedKind = .run
    vm.durationMinutes = 30
    vm.effortValue = 7
    await vm.submitWorkout()
    XCTAssertEqual(mock.lastMethod, "workout.upsert",
      "WorkoutEntryViewModel.submitWorkout() must call bridge with 'workout.upsert'")
  }
}
