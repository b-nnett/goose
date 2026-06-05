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
      failHistoricalSync("Historical sync currently supports the Goose V5 fd4b command characteristic. Active command characteristic: \(characteristic).")
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
    var firstCommand = firstCommandOverride ?? (requestHistoricalRangeBeforeTransfer ? .getDataRange : .sendHistoricalData)
    let isGen4 = activeCommandGeneration == .gen4
    if isGen4 && !rangeOnly {
      // WHOOP 4.0 has no GET_DATA_RANGE step; it goes hello → set_time →
      // get_name → enter_high_freq_sync → history-start (SEND_HISTORICAL_DATA).
      firstCommand = .sendHistoricalData
    }
    if firstCommand == .getDataRange {
      updateHistoricalRangeDebugStatus("started trigger=\(trigger) first=GET_DATA_RANGE")
    }
    record(
      source: "ble.sync",
      title: "historical_sync.started",
      body: "trigger=\(trigger) first=\(firstCommand.name) range_only=\(rangeOnly) ack_enabled=\(historicalDataResultAckEnabled) gen4=\(isGen4)"
    )
    notifyHistoricalSyncProgress(status: "syncing", detail: "Starting \(firstCommand.name)", terminal: false, failed: false)
    if isGen4 && !rangeOnly {
      // Fire the Gen4 preamble, then kick off the transfer once it has drained.
      sendGen4HistoryPreamble()
      let kickoff = firstCommand
      DispatchQueue.main.asyncAfter(deadline: .now() + Self.gen4HistoryKickoffDelay) { [weak self] in
        guard let self, self.isHistoricalSyncing else {
          return
        }
        self.writeHistoricalCommand(kickoff)
      }
    } else {
      writeHistoricalCommand(firstCommand)
    }
  }

  // WHOOP 4.0 history preamble per the openwhoop Gen4 flow: set_time (cmd 10,
  // unix u32 LE + 5 zero bytes), get_name (cmd 76, [0x00]), and enter
  // high-frequency sync (cmd 96, empty). Sent fire-and-forget ahead of the
  // history-start command so they do not contend with the response state machine.
  func sendGen4HistoryPreamble() {
    guard let activePeripheral,
          let commandCharacteristic,
          let writeType = writeType(for: commandCharacteristic) else {
      return
    }
    let now = UInt32(Date().timeIntervalSince1970)
    let setTime: [UInt8] = [
      UInt8(now & 0xff),
      UInt8((now >> 8) & 0xff),
      UInt8((now >> 16) & 0xff),
      UInt8((now >> 24) & 0xff),
      0, 0, 0, 0, 0,
    ]
    // NOTE: enter_high_freq_sync (cmd 96) is intentionally NOT sent here. On a real
    // WHOOP 4.0 the high-frequency path streams raw motion (REALTIME_RAW_DATA k10/k11)
    // instead of the normal_history (HISTORICAL_DATA type 47) records that carry the
    // sleep/recovery metrics. We send only set_time + get_name before history_start so
    // the band returns its normal history rather than the high-frequency motion stream.
    let steps: [(command: UInt8, data: [UInt8], name: String)] = [
      (10, setTime, "GEN4_SET_CLOCK"),
      (76, [0x00], "GEN4_GET_NAME"),
    ]
    for (index, step) in steps.enumerated() {
      let delay = Double(index) * Self.gen4HistoryPreambleStepDelay
      DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self, weak activePeripheral, weak commandCharacteristic] in
        guard let self,
              let activePeripheral,
              let commandCharacteristic,
              self.isHistoricalSyncing else {
          return
        }
        let sequence = self.nextHistoricalSequence()
        let frame = self.buildCommandFrame(sequence: sequence, command: step.command, data: step.data)
        activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
        self.record(
          source: "ble.sync",
          title: "gen4_history_preamble.sent",
          body: "\(step.name) seq=\(sequence) frame=\(frame.hexString)"
        )
      }
    }
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
      commandPayload = pendingHistoryEndAckPayload ?? kind.payload
    } else if activeCommandGeneration == .gen4 {
      // WHOOP 4.0 GET_DATA_RANGE and history-start take a single 0x00 data byte,
      // where Gen5 sends an empty body.
      commandPayload = [0x00]
    } else {
      commandPayload = kind.payload
    }
    let sequence = nextHistoricalSequence()
    let frame = buildCommandFrame(
      sequence: sequence,
      command: kind.commandNumber,
      data: commandPayload
    )
    if kind == .sendHistoricalData {
      historicalTransferRequestAttemptCount += 1
    }
    // WHOOP 4.0 does not return a COMMAND_RESPONSE for history-start — it simply
    // begins streaming HISTORICAL_DATA packets. Waiting for a response makes the
    // sync time out, so on Gen4 we treat history-start as fire-and-forget (like the
    // ACK) and rely on the data stream + idle completion instead.
    let isGen4HistoryStart = activeCommandGeneration == .gen4 && kind == .sendHistoricalData
    if kind == .historicalDataResult || isGen4HistoryStart {
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
    if isGen4HistoryStart {
      // No command response is coming; wait for the data stream. Each incoming
      // HISTORICAL_DATA frame extends this idle window, so a real transfer keeps
      // going; if nothing streams, the sync completes gracefully instead of erroring.
      scheduleHistoricalIdleCompletion(reason: "gen4_history_start_sent")
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
