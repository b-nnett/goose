import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func beginHistoricalSync(
    trigger: String,
    automatic: Bool,
    firstCommandOverride: HistoricalCommandKind? = nil,
    rangeOnly: Bool = false,
    acknowledgeHistoricalDataResult: Bool = true
  ) {
    guard !isHistoricalSyncing else {
      record(level: .debug, source: "ble.sync", title: "historical_sync.skipped", body: "already syncing trigger=\(trigger)")
      return
    }
    guard activePeripheral != nil, commandCharacteristic != nil else {
      failHistoricalSync("Historical sync needs an active WHOOP command characteristic. Current connection state: \(connectionState).")
      return
    }
    guard connectionState == "ready" else {
      failHistoricalSync("Historical sync can only start from the ready state. Current connection state: \(connectionState).")
      return
    }
    guard supportsHistoricalSync else {
      let characteristic = commandCharacteristic?.uuid.uuidString ?? "missing"
      failHistoricalSync("Historical sync needs command characteristic. Active command characteristic: \(characteristic).")
      return
    }

    historicalSyncRunID = UUID()
    historicalRangePollOnly = rangeOnly
    historicalDataResultAckEnabled = acknowledgeHistoricalDataResult
    isHistoricalSyncing = true
    historicalSyncStatus = "syncing"
    historicalPacketCount = 0
    historicalPacketsReceivedThisSync = 0
    lastHistoricalPacketCountPublishedAt = Date.distantPast
    lastHistoricalSyncProgressCallbackAt = Date.distantPast
    lastHistoricalSyncProgressCallbackStatus = ""
    lastHistoricalSyncProgressCallbackDetail = ""
    coalescedHistoricalSyncProgressCallbackCount = 0
    historyEndAckQueued = false
    historyEndAckSentThisBurst = false
    pendingHistoryEndAckPayload = nil
    historyEndReceived = false
    historyCompleteReceived = false
    historyStartReceived = false
    historicalRangePendingResponses = 0
    historicalRangeRetryCount = 0
    historicalTransferRequestAttemptCount = 0
    pendingHistoricalCommand = nil
    historicalCommandTimeoutWorkItem?.cancel()
    historicalIdleWorkItem?.cancel()
    historicalRangeRetryWorkItem?.cancel()
    let toastDetail = rangeOnly
      ? "Polling historical range"
      : (automatic ? "Requesting missed packets" : "Requesting historical packets")
    publishSyncToast(phase: .syncing, detail: toastDetail)
    let firstCommand = firstCommandOverride ?? (requestHistoricalRangeBeforeTransfer ? .getDataRange : .sendHistoricalData)
    if firstCommand == .getDataRange {
      updateHistoricalRangeDebugStatus("started trigger=\(trigger) first=GET_DATA_RANGE")
    }
    record(
      source: "ble.sync",
      title: "historical_sync.started",
      body: "trigger=\(trigger) first=\(firstCommand.name) range_only=\(rangeOnly) ack_enabled=\(historicalDataResultAckEnabled)"
    )
    notifyHistoricalSyncProgress(status: "syncing", detail: "Starting \(firstCommand.name)", terminal: false, failed: false)
    writeHistoricalCommand(firstCommand)
  }

  func writeHistoricalCommand(_ kind: HistoricalCommandKind) {
    guard isHistoricalSyncing else {
      return
    }
    guard let activePeripheral, let commandCharacteristic else {
      failHistoricalSync("Lost the command characteristic before writing \(kind.name).")
      return
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      failHistoricalSync("Command characteristic \(commandCharacteristic.uuid.uuidString) is not writable for \(kind.name).")
      return
    }

    let commandPayload = kind == .historicalDataResult
      ? pendingHistoryEndAckPayload ?? kind.payload
      : kind.payload
    let sequence = nextHistoricalSequence()
    let frame = Self.buildV5CommandFrame(
      sequence: sequence,
      command: kind.commandNumber,
      data: commandPayload
    )
    if kind == .sendHistoricalData {
      historicalTransferRequestAttemptCount += 1
    }
    if kind == .historicalDataResult {
      pendingHistoricalCommand = nil
      historicalCommandTimeoutWorkItem?.cancel()
    } else {
      pendingHistoricalCommand = PendingHistoricalCommand(kind: kind, sequence: sequence)
      scheduleHistoricalCommandTimeout(kind: kind, sequence: sequence)
    }
    activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
    emitCommandWrite(
      source: "ble.sync",
      commandName: kind.name,
      commandNumber: kind.commandNumber,
      sequence: sequence,
      payload: Data(commandPayload),
      frame: frame,
      peripheral: activePeripheral,
      characteristic: commandCharacteristic,
      writeType: writeType
    )
    if kind == .getDataRange {
      updateHistoricalRangeDebugStatus("sent seq=\(sequence) \(writeTypeName(writeType)) frame=\(frame.hexString)")
    }
    notifyHistoricalSyncProgress(status: "syncing", detail: "Sent \(kind.name) seq \(sequence)", terminal: false, failed: false)
    record(
      source: "ble.sync",
      title: "historical_sync.command.sent",
      body: "\(kind.name) seq=\(sequence) \(writeTypeName(writeType)) payload=\(Data(commandPayload).hexString) \(frame.hexString)"
    )
    if kind == .historicalDataResult {
      record(
        source: "ble.sync",
        title: "historical_sync.result_ack.sent",
        body: "seq=\(sequence) payload=\(Data(commandPayload).hexString) fire_and_forget=true"
      )
      if historyCompleteReceived {
        completeHistoricalSync(reason: "history_result_ack_sent_after_complete")
      } else {
        scheduleHistoricalIdleCompletion(reason: "history_result_ack_sent")
      }
    }
  }

  func nextHistoricalSequence() -> UInt8 {
    let sequence = nextHistoricalCommandSequence
    nextHistoricalCommandSequence = nextHistoricalCommandSequence == UInt8.max ? 57 : nextHistoricalCommandSequence + 1
    return sequence
  }

  func writeType(for characteristic: CBCharacteristic) -> CBCharacteristicWriteType? {
    if characteristic.properties.contains(.write) {
      return .withResponse
    }
    if characteristic.properties.contains(.writeWithoutResponse) {
      return .withoutResponse
    }
    return nil
  }

  func debugCommandPayload(
    for definition: GooseDebugCommandDefinition,
    payloadHex: String?
  ) -> [UInt8]? {
    if definition.id == "get_device_config_value" || definition.id == "get_feature_flag_value" {
      guard let data = Self.normalizedHexData(payloadHex) else {
        return nil
      }
      if data.count == 32 {
        return [1] + Array(data)
      }
      if data.count == 33 {
        return Array(data)
      }
      return nil
    }

    if definition.requiresPayloadHex {
      guard let data = Self.normalizedHexData(payloadHex), !data.isEmpty else {
        return nil
      }
      return Array(data)
    }

    let defaultHex = payloadHex ?? definition.defaultPayloadHex ?? ""
    guard let data = Self.normalizedHexData(defaultHex) else {
      return nil
    }
    return Array(data)
  }

  static func normalizedHexData(_ hex: String?) -> Data? {
    let normalized = (hex ?? "").filter { !$0.isWhitespace }
    guard normalized.count.isMultiple(of: 2) else {
      return nil
    }

    var data = Data()
    var index = normalized.startIndex
    while index < normalized.endIndex {
      let nextIndex = normalized.index(index, offsetBy: 2)
      guard let byte = UInt8(normalized[index..<nextIndex], radix: 16) else {
        return nil
      }
      data.append(byte)
      index = nextIndex
    }
    return data
  }

}
