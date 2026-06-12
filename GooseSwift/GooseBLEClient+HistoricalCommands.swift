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

    let newRunID = UUID()
    historicalManager.beginSync(runID: newRunID)
    historicalManager.historicalRangePollOnly = rangeOnly
    historicalManager.historicalDataResultAckEnabled = acknowledgeHistoricalDataResult
    historicalPacketCount = 0
    historicalManager.historicalPacketsReceivedThisSync = 0
    historicalManager.lastHistoricalPacketCountPublishedAt = Date.distantPast
    historicalManager.lastHistoricalSyncProgressCallbackAt = Date.distantPast
    historicalManager.lastHistoricalSyncProgressCallbackStatus = ""
    historicalManager.lastHistoricalSyncProgressCallbackDetail = ""
    historicalManager.coalescedHistoricalSyncProgressCallbackCount = 0
    historicalManager.historyEndAckQueued = false
    historicalManager.historyEndAckSentThisBurst = false
    historicalManager.pendingHistoryEndAckPayload = nil
    historicalManager.gen4HistoricalPageSeq = 0
    historicalManager.historyEndReceived = false
    historicalManager.historyCompleteReceived = false
    historicalManager.historyStartReceived = false
    historicalManager.historicalRangePendingResponses = 0
    historicalManager.historicalRangeRetryCount = 0
    historicalManager.historicalTransferRequestAttemptCount = 0
    historicalManager.pendingHistoricalCommand = nil
    historicalManager.pendingHistoricalFrames.removeAll()
    historicalManager.lastHandledWasHistoricalDataPacket = false
    historicalManager.historicalCommandTimeoutWorkItem?.cancel()
    historicalManager.historicalIdleWorkItem?.cancel()
    historicalManager.historicalRangeRetryWorkItem?.cancel()
    let toastDetail = rangeOnly
      ? "Polling historical range"
      : (automatic ? "Requesting missed packets" : "Requesting historical packets")
    publishSyncToast(phase: .syncing, detail: toastDetail)
    // Gen4 ignores firstCommandOverride: the strap requires the cmd 34 → 22 → 23
    // sequence regardless of caller intent. See docs/gen4-historical-sync.md.
    if activeDeviceGeneration == .gen4 {
      record(
        source: "ble.sync",
        title: "historical_sync.started",
        body: "trigger=\(trigger) first=gen4_get_data_range range_only=\(rangeOnly) override_ignored=\(firstCommandOverride?.name ?? "none")"
      )
      notifyHistoricalSyncProgress(status: "syncing", detail: "Querying Gen4 page range", terminal: false, failed: false)
      writeHistoricalCommand(.getDataRange)
      return
    }

    let firstCommand = firstCommandOverride ?? (historicalManager.requestHistoricalRangeBeforeTransfer ? .getDataRange : .sendHistoricalData)
    if firstCommand == .getDataRange {
      updateHistoricalRangeDebugStatus("started trigger=\(trigger) first=GET_DATA_RANGE")
    }
    record(
      source: "ble.sync",
      title: "historical_sync.started",
      body: "trigger=\(trigger) first=\(firstCommand.name) range_only=\(rangeOnly) ack_enabled=\(historicalManager.historicalDataResultAckEnabled)"
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

    let commandPayload: [UInt8]
    if kind == .historicalDataResult {
      commandPayload = historicalManager.pendingHistoryEndAckPayload ?? kind.payload
    } else if activeDeviceGeneration == .gen4 && (kind == .getDataRange || kind == .sendHistoricalData) {
      commandPayload = [0x00]
    } else {
      commandPayload = kind.payload
    }
    let sequence = nextHistoricalSequence()
    let frame = activeDeviceGeneration.buildCommandFrame(
      sequence: sequence,
      command: kind.commandNumber,
      data: commandPayload
    )
    if kind == .sendHistoricalData {
      historicalManager.historicalTransferRequestAttemptCount += 1
    }
    if kind == .historicalDataResult {
      historicalManager.pendingHistoricalCommand = nil
      historicalManager.historicalCommandTimeoutWorkItem?.cancel()
    } else {
      historicalManager.pendingHistoricalCommand = PendingHistoricalCommand(kind: kind, sequence: sequence)
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
      if historicalManager.historyCompleteReceived {
        completeHistoricalSync(reason: "history_result_ack_sent_after_complete")
      } else {
        scheduleHistoricalIdleCompletion(reason: "history_result_ack_sent")
      }
    }
  }

  func nextHistoricalSequence() -> UInt8 {
    let sequence = historicalManager.nextHistoricalCommandSequence
    historicalManager.nextHistoricalCommandSequence = historicalManager.nextHistoricalCommandSequence == UInt8.max ? 57 : historicalManager.nextHistoricalCommandSequence + 1
    return sequence
  }

  // Gen4 cmd 23 args: [flag=0x01][LE32 page_seq][LE32 page_count=16].
  // Format observed in the official-app PacketLogger capture; the 16-page
  // batch size matches the strap's per-burst response window.
  func gen4PageRequestPayload(seq: UInt32) -> [UInt8] {
    [0x01,
     UInt8(seq & 0xff), UInt8((seq >> 8) & 0xff),
     UInt8((seq >> 16) & 0xff), UInt8((seq >> 24) & 0xff),
     0x10, 0x00, 0x00, 0x00]
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
