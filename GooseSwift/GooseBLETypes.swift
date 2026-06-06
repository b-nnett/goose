import CoreBluetooth
import Foundation
import OSLog

enum GooseLogLevel: String {
  case debug
  case info
  case warn
  case error
}

struct GooseDiscoveredDevice: Identifiable, Equatable {
  let id: UUID
  let name: String
  let rssi: Int
}

struct GooseMessage: Identifiable {
  let id = UUID()
  let timestamp: Date
  let level: GooseLogLevel
  let source: String
  let title: String
  let body: String
}

struct GooseNotificationEvent {
  let deviceID: UUID
  let serviceUUID: String
  let characteristicUUID: String
  let value: Data
  let capturedAt: Date

  var rustDeviceType: String {
    characteristicUUID.lowercased().hasPrefix("610800") ? "GEN4" : "GOOSE"
  }
}

struct GooseBLENotificationContext {
  let activeDeviceName: String
  let connectionState: String
}

struct GooseCommandWriteEvent {
  let deviceID: UUID
  let serviceUUID: String
  let characteristicUUID: String
  let commandName: String
  let commandNumber: UInt8?
  let sequence: UInt8?
  let payload: Data
  let frame: Data
  let writeType: String
  let source: String
  let capturedAt: Date
}

enum GooseSyncToastPhase: String {
  case syncing
  case synced
  case failed
}

struct GooseSyncToast: Identifiable, Equatable {
  let id = UUID()
  let phase: GooseSyncToastPhase
  let title: String
  let detail: String
}

struct GooseHistoricalSyncProgress {
  let status: String
  let detail: String
  let packetCount: Int
  let isTerminal: Bool
  let failed: Bool
  let capturedAt: Date
}

struct GooseHistoricalRangeTelemetry {
  let capturedAt: Date
  let status: String
  let commandSequence: UInt8
  let resultCode: UInt8
  let resultName: String
  let payloadHex: String
  let bodyHex: String
  let revisionOrStatus: UInt8?
  let wordsFromOffset1: [UInt32]
  let pageCurrent: UInt32?
  let pageOldest: UInt32?
  let pageEnd: UInt32?
  let pagesBehind: Int64?
  let pendingResponseCount: Int
  let retryCount: Int
  let notes: String
}

struct GooseSyncFailure: Identifiable, Equatable {
  let id = UUID()
  let title: String
  let message: String
  let occurredAt: Date
}

struct GooseDebugCommandDefinition: Identifiable, Equatable {
  let id: String
  let title: String
  let commandNumber: UInt8
  let family: String
  let risk: String
  let detail: String
  let defaultPayloadHex: String?
  let requiresPayloadHex: Bool
  let payloadHint: String

  var canSendFromButton: Bool {
    defaultPayloadHex != nil || !requiresPayloadHex
  }

  var remoteURLExample: String {
    if requiresPayloadHex {
      return "gooseswift://debug-command/\(id)?payload=<hex>"
    }
    return "gooseswift://debug-command/\(id)"
  }
}

struct GooseDebugCommandResponse: Identifiable, Equatable {
  let id: UUID
  let commandID: String
  let title: String
  let commandNumber: UInt8
  let sequence: UInt8
  let requestedAt: Date
  let completedAt: Date?
  let status: String
  let result: String
  let requestPayloadHex: String
  let requestFrameHex: String
  let responsePayloadHex: String
  let responseBodyHex: String
  let source: String

  var summary: String {
    let time = completedAt ?? requestedAt
    let body = responseBodyHex.isEmpty ? "no body" : "body \(responseBodyHex)"
    return "\(status) | \(result) | seq \(sequence) | \(body) | \(time.formatted(date: .omitted, time: .standard))"
  }
}

// MARK: - WhoopGeneration

/// Encapsulates all generation-specific behavior for WHOOP band communication.
/// Adding support for a new generation means adding a case here and filling in
/// the three requirements below — everything else in the app picks it up automatically.
enum WhoopGeneration: CustomStringConvertible {
  case gen4
  case gen5

  // MARK: Detection

  /// Infer generation from the command characteristic that was assigned during GATT discovery.
  static func detect(from characteristic: CBCharacteristic) -> WhoopGeneration {
    characteristic.uuid.uuidString.lowercased().hasPrefix("61080002") ? .gen4 : .gen5
  }

  // MARK: Display

  var description: String {
    switch self {
    case .gen4: return "WHOOP 4.0"
    case .gen5: return "WHOOP 5.0"
    }
  }

  // MARK: Frame building

  /// The hello frame sent immediately after the command characteristic is ready.
  /// Gen5 uses a captured static frame; Gen4 sends GET_HELLO (cmd 145) in Gen4 framing.
  var helloFrame: Data {
    switch self {
    case .gen5: return GooseHello.clientHelloFrame
    case .gen4: return buildCommandFrame(sequence: 0x23, command: 145, data: [])
    }
  }

  /// Build a correctly-framed command packet for this generation.
  func buildCommandFrame(sequence: UInt8, command: UInt8, data: [UInt8]) -> Data {
    switch self {
    case .gen5:
      return GooseBLEClient.buildV5CommandFrame(sequence: sequence, command: command, data: data)
    case .gen4:
      return Self.buildGen4CommandFrame(sequence: sequence, command: command, data: data)
    }
  }

  // MARK: Gen4 internals

  /// Gen4 frame layout: [0xaa, len_lo, len_hi, crc8(len_bytes), payload..., crc32 x4]
  ///
  /// Gen4 frames are intentionally unpadded — unlike `buildV5CommandFrame`,
  /// which rounds the payload up to a 4-byte boundary. Confirmed from the
  /// PacketLogger capture of the official iOS app: it emits `cmd 120` with a
  /// 65-byte args field (not a multiple of 4), proving no padding is applied.
  /// Our unpadded frames also round-trip cleanly with the strap.
  private static func buildGen4CommandFrame(sequence: UInt8, command: UInt8, data: [UInt8]) -> Data {
    var payload: [UInt8] = [GooseBLEClient.V5PacketType.command, sequence, command]
    payload.append(contentsOf: data)
    let totalLen = payload.count + 4
    let lenBytes: [UInt8] = [UInt8(totalLen & 0xff), UInt8((totalLen >> 8) & 0xff)]
    let headerCRC = crc8(lenBytes)
    var frame: [UInt8] = [0xaa, lenBytes[0], lenBytes[1], headerCRC]
    frame.append(contentsOf: payload)
    let payloadCRC = GooseBLEClient.crc32(payload)
    frame.append(contentsOf: [
      UInt8(payloadCRC & 0xff),
      UInt8((payloadCRC >> 8) & 0xff),
      UInt8((payloadCRC >> 16) & 0xff),
      UInt8((payloadCRC >> 24) & 0xff),
    ])
    return Data(frame)
  }

  /// CRC-8, polynomial 0x07, init 0x00 — matches Rust protocol.rs implementation.
  private static func crc8(_ bytes: [UInt8]) -> UInt8 {
    var crc = UInt8(0)
    for byte in bytes {
      crc ^= byte
      for _ in 0..<8 {
        crc = crc & 0x80 != 0 ? (crc << 1) ^ 0x07 : crc << 1
      }
    }
    return crc
  }
}

