import XCTest
import CoreBluetooth
@testable import GooseSwift

final class WearableDescriptorTests: XCTestCase {

  func testGen5DescriptorAcceptsGen5CommandUUID() {
    let uuid = CBUUID(string: "fd4b0002-cce1-4033-93ce-002d5875f58a")
    XCTAssertTrue(
      WearableDescriptor.whoopGen5.isCommandUUID(uuid),
      "Gen5 descriptor should accept fd4b0002 prefix"
    )
  }

  func testGen4DescriptorAcceptsGen4CommandUUID() {
    let uuid = CBUUID(string: "61080002-8d6d-82b8-614a-1c8cb0f8dcc6")
    XCTAssertTrue(
      WearableDescriptor.whoopGen4.isCommandUUID(uuid),
      "Gen4 descriptor should accept 61080002 prefix"
    )
  }

  func testGen5DescriptorRejectsGen4CommandUUID() {
    let uuid = CBUUID(string: "61080002-8d6d-82b8-614a-1c8cb0f8dcc6")
    XCTAssertFalse(
      WearableDescriptor.whoopGen5.isCommandUUID(uuid),
      "Gen5 descriptor should reject 61080002 prefix"
    )
  }

  func testGen4DescriptorRejectsGen5CommandUUID() {
    let uuid = CBUUID(string: "fd4b0002-cce1-4033-93ce-002d5875f58a")
    XCTAssertFalse(
      WearableDescriptor.whoopGen4.isCommandUUID(uuid),
      "Gen4 descriptor should reject fd4b0002 prefix"
    )
  }

  func testGen4ServiceUUIDPrefix() {
    XCTAssertEqual(WearableDescriptor.whoopGen4.serviceUUIDPrefix, "61080001")
  }

  func testGen5ServiceUUIDPrefix() {
    XCTAssertEqual(WearableDescriptor.whoopGen5.serviceUUIDPrefix, "fd4b0001")
  }

  func testGen4CommandCharacteristicPrefix() {
    XCTAssertEqual(WearableDescriptor.whoopGen4.commandCharacteristicPrefix, "61080002")
  }

  func testGen5CommandCharacteristicPrefix() {
    XCTAssertEqual(WearableDescriptor.whoopGen5.commandCharacteristicPrefix, "fd4b0002")
  }
}
