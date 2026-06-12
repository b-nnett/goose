import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func buzz(loops: Int) {
    guard let activePeripheral, let commandCharacteristic else {
      record(source: "ble.haptic", title: "buzz.blocked", body: "no active peripheral or characteristic")
      return
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      record(source: "ble.haptic", title: "buzz.blocked", body: "characteristic not writable")
      return
    }
    let clamped = UInt8(max(1, min(255, loops)))
    let payload = Data([0x13, clamped])
    activePeripheral.writeValue(payload, for: commandCharacteristic, type: writeType)
    record(source: "ble.haptic", title: "buzz.sent", body: "loops=\(clamped) \(writeTypeName(writeType))")
  }
}
