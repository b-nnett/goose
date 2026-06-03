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

  var allowsRemoteInvocation: Bool {
    risk == "read" || risk == "keyed read"
  }

  var remoteURLExample: String {
    guard allowsRemoteInvocation else {
      return "Remote invocation disabled"
    }
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
