import XCTest
import CoreBluetooth
@testable import GooseSwift

final class GooseBLETypesTests: XCTestCase {

  // MARK: - GooseBLEClient.generation(from:) helper tests

  func testGenerationDerivation_gen4ServiceUUID() {
    let gen4ServiceUUID = CBUUID(string: "61080001-8d6d-82b8-614a-1c8cb0f8dcc6")
    let generation = GooseBLEClient.generation(from: [gen4ServiceUUID])
    XCTAssertEqual(generation, "4.0", "61080001 service UUID should derive generation 4.0")
  }

  func testGenerationDerivation_gen5ServiceUUID() {
    let gen5ServiceUUID = CBUUID(string: "fd4b0001-cce1-4033-93ce-002d5875f58a")
    let generation = GooseBLEClient.generation(from: [gen5ServiceUUID])
    XCTAssertEqual(generation, "5.0", "fd4b0001 service UUID should derive generation 5.0")
  }

  func testGenerationDerivation_unknownServiceUUID() {
    let unknownUUID = CBUUID(string: "00001800-0000-1000-8000-00805f9b34fb")
    let generation = GooseBLEClient.generation(from: [unknownUUID])
    XCTAssertEqual(generation, "unknown", "Unknown service UUID should derive 'unknown'")
  }

  func testGenerationDerivation_emptyServiceList() {
    let generation = GooseBLEClient.generation(from: [])
    XCTAssertEqual(generation, "unknown", "Empty service list should derive 'unknown'")
  }

  func testGenerationDerivation_gen4TakesPrecedenceWhenBothPresent() {
    let gen4UUID = CBUUID(string: "61080001-8d6d-82b8-614a-1c8cb0f8dcc6")
    let gen5UUID = CBUUID(string: "fd4b0001-cce1-4033-93ce-002d5875f58a")
    // Gen4 listed first — should return "4.0"
    let generation = GooseBLEClient.generation(from: [gen4UUID, gen5UUID])
    XCTAssertEqual(generation, "4.0", "Gen4 UUID first in list should derive 4.0")
  }

  // MARK: - GooseNotificationEvent.rustDeviceType tests

  func testRustDeviceType_gen4CharacteristicPrefix() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "61080001-8d6d-82b8-614a-1c8cb0f8dcc6",
      characteristicUUID: "61080003-8d6d-82b8-614a-1c8cb0f8dcc6",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "GEN4",
      "Characteristic starting with 610800 should produce rustDeviceType GEN4")
  }

  func testRustDeviceType_gen5CharacteristicPrefix() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "fd4b0001-cce1-4033-93ce-002d5875f58a",
      characteristicUUID: "fd4b0003-cce1-4033-93ce-002d5875f58a",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "GOOSE",
      "Characteristic starting with fd4b should produce rustDeviceType GOOSE")
  }

  // MARK: - WearableDescriptor.genericHRMonitor tests (Phase 8 P02)

  func test_genericHRMonitor_serviceUUIDPrefix() {
    XCTAssertEqual(WearableDescriptor.genericHRMonitor.serviceUUIDPrefix, "180d",
      "genericHRMonitor must use lowercased 0x180D service UUID prefix")
  }

  func test_genericHRMonitor_commandCharacteristicPrefix_empty() {
    XCTAssertEqual(WearableDescriptor.genericHRMonitor.commandCharacteristicPrefix, "",
      "genericHRMonitor has no command characteristic — prefix must be empty")
  }

  func test_genericHRMonitor_isCommandUUID_returnsFalseForAnyUUID() {
    // Proves the empty-prefix guard (MEDIUM-1): hasPrefix("") would return true without it
    XCTAssertFalse(
      WearableDescriptor.genericHRMonitor.isCommandUUID(CBUUID(string: "2A37")),
      "isCommandUUID must return false for any UUID when commandCharacteristicPrefix is empty"
    )
    XCTAssertFalse(
      WearableDescriptor.genericHRMonitor.isCommandUUID(CBUUID(string: "FD4B0002-cce1-4033-93ce-002d5875f58a")),
      "isCommandUUID must return false for Gen5 command UUID when commandCharacteristicPrefix is empty"
    )
  }

  func test_whoopGen4_isCommandUUID_stillMatchesCommandPrefix() {
    // Sanity check: the empty-prefix guard must NOT break the populated Gen4 case
    XCTAssertTrue(
      WearableDescriptor.whoopGen4.isCommandUUID(CBUUID(string: "61080002-8d6d-82b8-614a-1c8cb0f8dcc6")),
      "whoopGen4.isCommandUUID must still return true for the 61080002 command UUID"
    )
  }

  // MARK: - GooseNotificationEvent.rustDeviceType HR_MONITOR tests (Phase 8 P02 / MEDIUM-2)

  func test_rustDeviceType_2A37_short_returnsHRMonitor() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "180D",
      characteristicUUID: "2A37",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "HR_MONITOR",
      "Short-form characteristic UUID 2A37 must produce rustDeviceType HR_MONITOR")
  }

  func test_rustDeviceType_2a37_lowercase_returnsHRMonitor() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "180d",
      characteristicUUID: "2a37",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "HR_MONITOR",
      "Lowercase short-form 2a37 must produce rustDeviceType HR_MONITOR")
  }

  func test_rustDeviceType_2A37_full128bit_returnsHRMonitor() {
    // Proves MEDIUM-2: full 128-bit form must match, case-insensitively
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "0000180D-0000-1000-8000-00805F9B34FB",
      characteristicUUID: "00002A37-0000-1000-8000-00805F9B34FB",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "HR_MONITOR",
      "Full 128-bit form 00002A37-... must produce rustDeviceType HR_MONITOR")
  }

  func test_rustDeviceType_610800_stillReturnsGEN4() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "61080001-8d6d-82b8-614a-1c8cb0f8dcc6",
      characteristicUUID: "61080003-8d6d-82b8-614a-1c8cb0f8dcc6",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "GEN4",
      "Gen4 610800-prefixed characteristic must still produce rustDeviceType GEN4")
  }

  func test_rustDeviceType_fd4b_stillReturnsGOOSE() {
    let event = GooseNotificationEvent(
      deviceID: UUID(),
      serviceUUID: "fd4b0001-cce1-4033-93ce-002d5875f58a",
      characteristicUUID: "fd4b0003-cce1-4033-93ce-002d5875f58a",
      value: Data(),
      capturedAt: Date()
    )
    XCTAssertEqual(event.rustDeviceType, "GOOSE",
      "Gen5 fd4b-prefixed characteristic must still produce rustDeviceType GOOSE")
  }
}
