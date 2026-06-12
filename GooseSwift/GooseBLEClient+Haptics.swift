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
    // NOTE: 0x13 haptic command is sent as a raw 2-byte payload without buildCommandFrame framing.
    // This diverges from writeAlarmCommand/writeClockCommand/writeSensorStreamCommand which all
    // go through buildCommandFrame (header + sequence + CRC). Verify via BTSnoop capture whether
    // the WHOOP haptic characteristic accepts unframed commands before assuming this is correct.
    let payload = Data([0x13, clamped])
    activePeripheral.writeValue(payload, for: commandCharacteristic, type: writeType)
    record(source: "ble.haptic", title: "buzz.sent", body: "loops=\(clamped) \(writeTypeName(writeType))")
  }
}
