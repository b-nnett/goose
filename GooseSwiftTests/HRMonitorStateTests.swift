import XCTest
import CoreBluetooth
@testable import GooseSwift

final class HRMonitorStateTests: XCTestCase {

  // MARK: - discoveredHRDevices default state

  func testDiscoveredHRDevicesDefaultsEmpty() {
    let client = GooseBLEClient()
    XCTAssertEqual(client.discoveredHRDevices, [], "Fresh GooseBLEClient must have empty discoveredHRDevices")
  }

  // MARK: - hrConnectionState default state

  func testHrConnectionStateDefaultsDisconnected() {
    let client = GooseBLEClient()
    XCTAssertEqual(client.hrConnectionState, "disconnected", "Fresh GooseBLEClient must have hrConnectionState == \"disconnected\"")
  }

  // MARK: - discoveredHRDevices is a settable @Published property

  func testDiscoveredHRDevicesPublishedAssignmentPropagates() {
    let client = GooseBLEClient()
    let device = GooseDiscoveredDevice(id: UUID(), name: "Polar H10", rssi: -72, generation: "hr_monitor")
    client.discoveredHRDevices = [device]
    XCTAssertEqual(client.discoveredHRDevices.count, 1, "discoveredHRDevices must accept assignment and reflect count == 1")
    XCTAssertEqual(client.discoveredHRDevices.first?.name, "Polar H10", "Assigned device name must be 'Polar H10'")
  }

  // MARK: - hrConnectionState is a settable @Published property

  func testHrConnectionStateTransitionsAreAssignable() {
    let client = GooseBLEClient()
    client.hrConnectionState = "connecting"
    XCTAssertEqual(client.hrConnectionState, "connecting", "hrConnectionState must accept 'connecting' assignment")
    client.hrConnectionState = "connected"
    XCTAssertEqual(client.hrConnectionState, "connected", "hrConnectionState must accept 'connected' assignment")
    client.hrConnectionState = "disconnected"
    XCTAssertEqual(client.hrConnectionState, "disconnected", "hrConnectionState must accept 'disconnected' assignment")
  }
}
