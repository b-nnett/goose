import Foundation
import OSLog

// Structural-invariant validator for BLE frames.
// Called before bytes reach GooseRustBridge — rejects frames that are
// malformed at a structural level without inspecting packet type or semantics.
// Structural invariants only: device_id non-nil, payload non-nil, payload non-empty.
// NO packet-type whitelist (CONTEXT decision — a whitelist would silently break WHOOP 5.0 frames).
final class GooseBLEDataValidator {
  // Called on the background queue that runs validation.
  // Consumer must hop to main for any @Observable / @MainActor state mutation.
  var onInvalidFrame: (() -> Void)?

  // Primary byte-based validate. Returns true when all structural invariants pass.
  func validate(payload: [UInt8]?, deviceID: UUID?) -> Bool {
    // Invariant 1: device_id must be non-nil — frames without an identified device are unroutable.
    guard deviceID != nil else {
      logger.warning("invalid_frame: device_id nil")
      onInvalidFrame?()
      return false
    }
    // Invariants 2 + 3: payload must be non-nil and non-empty (length >= 1).
    guard let payload, !payload.isEmpty else {
      logger.warning("invalid_frame: payload nil or empty")
      onInvalidFrame?()
      return false
    }
    return true
  }

  // Hex-string convenience overload — the notification pipeline carries frames as hex strings.
  // Treats nil/empty hex as an invalid (empty) payload.
  // Treats malformed hex (odd length, non-hex chars) as an invalid frame.
  func validate(frameHex: String?, deviceID: UUID?) -> Bool {
    guard let hex = frameHex, !hex.isEmpty else {
      logger.warning("invalid_frame: hex nil or empty")
      onInvalidFrame?()
      return false
    }
    guard hex.count % 2 == 0 else {
      logger.warning("invalid_frame: hex has odd length \(hex.count)")
      onInvalidFrame?()
      return false
    }
    var bytes: [UInt8] = []
    var index = hex.startIndex
    while index < hex.endIndex {
      let nextIndex = hex.index(index, offsetBy: 2)
      let byteStr = hex[index..<nextIndex]
      guard let byte = UInt8(byteStr, radix: 16) else {
        logger.warning("invalid_frame: hex contains non-hex chars near '\(byteStr)'")
        onInvalidFrame?()
        return false
      }
      bytes.append(byte)
      index = nextIndex
    }
    return validate(payload: bytes, deviceID: deviceID)
  }

  private let logger = Logger(subsystem: "com.goose.swift", category: "ble")
}
