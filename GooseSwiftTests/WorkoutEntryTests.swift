import XCTest
@testable import GooseSwift

@MainActor
final class WorkoutEntryTests: XCTestCase {

  func test_submit_calls_workout_upsert() async throws {
    let mock = MockRustBridge()
    let vm = WorkoutEntryViewModel(
      bridge: mock,
      databasePath: "/tmp/test.sqlite"
    )
    vm.selectedKind = .run
    vm.durationMinutes = 30
    vm.effortValue = 7

    await vm.submitWorkout()

    XCTAssertEqual(mock.lastMethod, "workout.upsert",
      "submitWorkout() must call bridge method 'workout.upsert'")
    let args = mock.lastArgs
    XCTAssertEqual(args["source"] as? String, "manual")
    XCTAssertEqual(args["sport"] as? String, ActivityKind.run.rawValue)
    let durationS = args["duration_s"] as? Double
    XCTAssertNotNil(durationS)
    XCTAssertEqual(durationS!, 30.0 * 60.0, accuracy: 0.001)
    let notes = args["notes"] as? String
    XCTAssertTrue(notes?.contains("perceived_effort: 7") == true,
      "notes must encode perceived_effort")
  }

  func test_submit_disabled_when_duration_zero() async {
    let mock = MockRustBridge()
    let vm = WorkoutEntryViewModel(
      bridge: mock,
      databasePath: "/tmp/test.sqlite"
    )
    vm.durationMinutes = 0

    XCTAssertFalse(vm.isFormValid, "form must be invalid when durationMinutes == 0")
    await vm.submitWorkout()
    XCTAssertNil(mock.lastMethod, "submitWorkout must be a no-op when form is invalid")
  }
}
