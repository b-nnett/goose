import XCTest
@testable import GooseSwift

final class GooseUploadServiceTests: XCTestCase {

  // Minimal GooseUploadService instance for payload tests.
  // buildUploadPayload is a pure function — no server, no BLE, no Rust bridge needed.
  private let service = GooseUploadService(databasePath: "/dev/null")
  private let testID = UUID()

  // MARK: - Gen4 payload

  func test_buildUploadPayload_gen4_hasGeneration4_noDeviceClass() {
    let payload = service.buildUploadPayload(deviceID: testID, deviceType: "GEN4", streams: [:])
    XCTAssertEqual(payload["device_generation"] as? String, "4.0",
      "GEN4 payload must carry device_generation 4.0")
    XCTAssertNil(payload["device_class"],
      "GEN4 payload must NOT carry device_class")
    XCTAssertNil(payload["device_type"],
      "GEN4 payload must NOT carry device_type")
  }

  // MARK: - Gen5 payload

  func test_buildUploadPayload_gen5_goose_hasGeneration5_noDeviceClass() {
    let payload = service.buildUploadPayload(deviceID: testID, deviceType: "GOOSE", streams: [:])
    XCTAssertEqual(payload["device_generation"] as? String, "5.0",
      "GOOSE payload must carry device_generation 5.0")
    XCTAssertNil(payload["device_class"],
      "GOOSE payload must NOT carry device_class")
  }

  // MARK: - HR monitor payload

  func test_buildUploadPayload_hrMonitor_hasDeviceTypeAndDeviceClass_noGeneration() {
    let payload = service.buildUploadPayload(
      deviceID: testID,
      deviceType: "Polar H10",
      streams: [:]
    )
    XCTAssertEqual(payload["device_type"] as? String, "Polar H10",
      "HR monitor payload must carry device_type with the sanitized BLE name")
    XCTAssertEqual(payload["device_class"] as? String, "HR_MONITOR",
      "HR monitor payload must carry device_class HR_MONITOR")
    XCTAssertNil(payload["device_generation"],
      "HR monitor payload must NOT carry device_generation")
  }

  // MARK: - Unknown device defaults to HR_MONITOR class

  func test_buildUploadPayload_unknownDevice_defaultsToHrMonitorClass() {
    let payload = service.buildUploadPayload(
      deviceID: testID,
      deviceType: "Garmin HRM",
      streams: [:]
    )
    XCTAssertEqual(payload["device_class"] as? String, "HR_MONITOR",
      "Unknown device type must default to device_class HR_MONITOR")
    XCTAssertEqual(payload["device_type"] as? String, "Garmin HRM",
      "Unknown device type must carry device_type with the provided name")
  }

  // MARK: - Streams are passed through untouched

  func test_buildUploadPayload_preservesStreams() {
    let sentinel: [String: Any] = ["hr": [42]]
    let payload = service.buildUploadPayload(
      deviceID: testID,
      deviceType: "GEN4",
      streams: sentinel
    )
    guard let streams = payload["streams"] as? [String: Any] else {
      XCTFail("payload must contain a streams dictionary")
      return
    }
    guard let hrValues = streams["hr"] as? [Int] else {
      XCTFail("payload streams must carry hr as [Int]")
      return
    }
    XCTAssertEqual(hrValues, [42],
      "streams passed to buildUploadPayload must be preserved exactly in the returned payload")
  }

  // MARK: - triggerManualUpload does not hardcode "GOOSE"
  //
  // GooseAppModel cannot be instantiated in a unit test (requires BLE hardware + @MainActor).
  // Instead this test uses a source-level assertion: it reads the source file and confirms there
  // is no unconditional `deviceType: "GOOSE"` literal that would bypass the prefix-based
  // derivation logic added in P03-T02. If a behavioural extraction helper is added in future,
  // replace this with a direct call to the helper.
  func test_triggerManualUpload_doesNotHardcodeGoose() throws {
    // GooseAppModel cannot be instantiated in a unit test (requires BLE hardware + @MainActor).
    // Source-level assertion: walk up from the test bundle to find GooseAppModel+Upload.swift
    // and verify there is no unconditional `deviceType: "GOOSE"` literal that would bypass the
    // prefix-based derivation logic added in P03-T02.
    // If a behavioural extraction helper is added in future, replace with a direct call to it.
    let sourceContent = try resolveUploadSourceContent()

    // The conditional `whoopType = "GOOSE"` (inside an if-let else branch for unknown descriptor)
    // is acceptable. We only protect against an unconditional literal that bypasses derivation.
    let hasUnconditionalGoose = sourceContent.contains("deviceType: \"GOOSE\"")
    XCTAssertFalse(hasUnconditionalGoose,
      "triggerManualUpload must not pass a hardcoded unconditional deviceType: \"GOOSE\" — " +
      "device type must be derived from the active descriptor's command characteristic prefix")
  }

  // Walk up from the test bundle to locate GooseAppModel+Upload.swift in the source tree.
  // Returns the file contents, or throws XCTSkip if the file cannot be resolved (sandboxed CI).
  private func resolveUploadSourceContent() throws -> String {
    var candidate = Bundle(for: type(of: self)).bundleURL
    for _ in 0..<12 {
      candidate = candidate.deletingLastPathComponent()
      let projectPath = candidate.appendingPathComponent("GooseSwift.xcodeproj")
      guard FileManager.default.fileExists(atPath: projectPath.path) else { continue }
      let uploadPath = candidate
        .appendingPathComponent("GooseSwift")
        .appendingPathComponent("GooseAppModel+Upload.swift")
      return try String(contentsOf: uploadPath, encoding: .utf8)
    }
    throw XCTSkip("GooseAppModel+Upload.swift not accessible from test bundle; skipping source assertion")
  }
}
