import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func nextDebugSequence() -> UInt8 {
    let sequence = nextDebugCommandSequence
    nextDebugCommandSequence = nextDebugCommandSequence == 159 ? 120 : nextDebugCommandSequence + 1
    return sequence
  }

  func scheduleDebugCommandTimeout(_ pending: PendingDebugCommand) {
    debugCommandTimeoutWorkItems[pending.sequence]?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            let current = self.pendingDebugCommands[pending.sequence],
            current.commandNumber == pending.commandNumber else {
        return
      }
      self.completeDebugCommand(
        pending,
        status: "timeout",
        result: "No command response",
        responsePayload: [],
        responseBody: []
      )
    }
    debugCommandTimeoutWorkItems[pending.sequence] = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: workItem)
  }

  func handleDebugCommandValue(_ value: Data, characteristic: CBCharacteristic) {
    guard notificationCharacteristicIDs.contains(characteristic.uuid), !pendingDebugCommands.isEmpty else {
      return
    }
    for frame in strapFrames(in: value) {
      guard let payload = strapPayload(in: frame),
            payload.count >= 5,
            let packetType = payload.first,
            packetType == V5PacketType.commandResponse || packetType == V5PacketType.puffinCommandResponse,
            let pending = pendingDebugCommands[payload[3]],
            payload[2] == pending.commandNumber else {
        continue
      }

      let resultCode = payload[4]
      let result = commandResultName(resultCode)
      let status = resultCode == 1 ? "ok" : "failed"
      let body = Array(payload.dropFirst(5))
      completeDebugCommand(
        pending,
        status: status,
        result: "\(result)(\(resultCode))",
        responsePayload: payload,
        responseBody: body
      )
    }
  }

  func completeDebugCommand(
    _ pending: PendingDebugCommand,
    status: String,
    result: String,
    responsePayload: [UInt8],
    responseBody: [UInt8]
  ) {
    debugCommandTimeoutWorkItems[pending.sequence]?.cancel()
    debugCommandTimeoutWorkItems[pending.sequence] = nil
    pendingDebugCommands[pending.sequence] = nil
    let completedAt = Date()
    let response = GooseDebugCommandResponse(
      id: UUID(),
      commandID: pending.id,
      title: pending.title,
      commandNumber: pending.commandNumber,
      sequence: pending.sequence,
      requestedAt: pending.requestedAt,
      completedAt: completedAt,
      status: status,
      result: result,
      requestPayloadHex: pending.requestPayloadHex,
      requestFrameHex: pending.requestFrameHex,
      responsePayloadHex: Data(responsePayload).hexString,
      responseBodyHex: Data(responseBody).hexString,
      source: pending.source
    )
    debugCommandResponses.insert(response, at: 0)
    if debugCommandResponses.count > 50 {
      debugCommandResponses.removeLast(debugCommandResponses.count - 50)
    }
    setDebugCommandStatus("\(pending.title) seq \(pending.sequence) \(status): \(result)")
    let level: GooseLogLevel = status == "ok" ? .info : .warn
    record(
      level: level,
      source: "ble.debug_command",
      title: "command.response",
      body: "\(pending.id) seq=\(pending.sequence) status=\(status) result=\(result) request_payload=\(pending.requestPayloadHex) response_payload=\(response.responsePayloadHex) body=\(response.responseBodyHex)"
    )
  }

  func setDebugCommandStatus(_ status: String) {
    debugCommandStatus = status
    writeDebugCommandSnapshot()
  }

  func writeDebugCommandSnapshot() {
    let payload: [String: Any] = [
      "schema": "goose.debug.bt-commands.v1",
      "generated_at": Self.diagnosticLogTimestampString(from: Date()),
      "connection": connectionState,
      "active_device": activeDeviceName,
      "status": debugCommandStatus,
      "remote_url_format": "gooseswift://debug-command/<id>?payload=<hex>",
      "commands": Self.debugResearchCommandDefinitions.map(debugCommandDefinitionPayload),
      "pending": pendingDebugCommands.values
        .sorted { $0.requestedAt > $1.requestedAt }
        .map(debugPendingCommandPayload),
      "responses": debugCommandResponses.map(debugCommandResponsePayload),
    ]
    guard JSONSerialization.isValidJSONObject(payload),
          let data = try? JSONSerialization.data(withJSONObject: payload, options: [.prettyPrinted, .sortedKeys]) else {
      return
    }
    for url in debugCommandSnapshotURLs() {
      do {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: url, options: .atomic)
        debugCommandSnapshotPath = url.path
      } catch {
        record(level: .warn, source: "ble.debug_command", title: "snapshot.write_failed", body: "\(url.path) \(error.localizedDescription)")
      }
    }
  }

  func debugCommandSnapshotURLs() -> [URL] {
    var urls: [URL] = []
    if let documentsURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
      urls.append(
        documentsURL
          .appendingPathComponent("GooseSwift", isDirectory: true)
          .appendingPathComponent("debug-bt-commands.json")
      )
    }
    if let supportURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
      urls.append(
        supportURL
          .appendingPathComponent("GooseSwift", isDirectory: true)
          .appendingPathComponent("debug-bt-commands.json")
      )
    }
    return urls
  }

  func debugCommandDefinitionPayload(_ command: GooseDebugCommandDefinition) -> [String: Any] {
    [
      "id": command.id,
      "title": command.title,
      "command_number": Int(command.commandNumber),
      "family": command.family,
      "risk": command.risk,
      "detail": command.detail,
      "payload_hint": command.payloadHint,
      "requires_payload_hex": command.requiresPayloadHex,
      "can_send_from_button": command.canSendFromButton,
      "remote_url_example": command.remoteURLExample,
    ]
  }

  func debugPendingCommandPayload(_ pending: PendingDebugCommand) -> [String: Any] {
    [
      "command_id": pending.id,
      "title": pending.title,
      "command_number": Int(pending.commandNumber),
      "sequence": Int(pending.sequence),
      "requested_at": Self.diagnosticLogTimestampString(from: pending.requestedAt),
      "request_payload_hex": pending.requestPayloadHex,
      "request_frame_hex": pending.requestFrameHex,
      "source": pending.source,
    ]
  }

  func debugCommandResponsePayload(_ response: GooseDebugCommandResponse) -> [String: Any] {
    [
      "id": response.id.uuidString,
      "command_id": response.commandID,
      "title": response.title,
      "command_number": Int(response.commandNumber),
      "sequence": Int(response.sequence),
      "requested_at": Self.diagnosticLogTimestampString(from: response.requestedAt),
      "completed_at": response.completedAt.map(Self.diagnosticLogTimestampString(from:)) ?? "",
      "status": response.status,
      "result": response.result,
      "request_payload_hex": response.requestPayloadHex,
      "request_frame_hex": response.requestFrameHex,
      "response_payload_hex": response.responsePayloadHex,
      "response_body_hex": response.responseBodyHex,
      "source": response.source,
    ]
  }

  func failAllDebugCommands(_ message: String) {
    guard !pendingDebugCommands.isEmpty else {
      return
    }
    for pending in Array(pendingDebugCommands.values) {
      completeDebugCommand(
        pending,
        status: "failed",
        result: message,
        responsePayload: [],
        responseBody: []
      )
    }
  }

  func scheduleDebugSkinTemperatureCommandIfNeeded(reason: String) {
    guard autoSendDebugSkinTemperatureCommand, !debugSkinTemperatureCommandSent else {
      return
    }
    debugSkinTemperatureCommandWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      self?.sendDebugSkinTemperatureCommandIfPossible(reason: reason)
    }
    debugSkinTemperatureCommandWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 1, execute: workItem)
  }

  func sendDebugSkinTemperatureCommandIfPossible(reason: String) {
    guard autoSendDebugSkinTemperatureCommand, !debugSkinTemperatureCommandSent else {
      return
    }
    guard connectionState == "ready" else {
      record(level: .warn, source: "ble.debug_menu", title: "debug_menu.write.blocked", body: "connection_state=\(connectionState) reason=\(reason)")
      return
    }
    guard let activePeripheral else {
      record(level: .warn, source: "ble.debug_menu", title: "debug_menu.write.blocked", body: "missing active peripheral reason=\(reason)")
      return
    }
    guard let debugMenuCharacteristic else {
      record(level: .warn, source: "ble.debug_menu", title: "debug_menu.write.blocked", body: "missing debug characteristic reason=\(reason)")
      return
    }
    let debugWriteType: CBCharacteristicWriteType
    if let characteristicWriteType = writeType(for: debugMenuCharacteristic) {
      debugWriteType = characteristicWriteType
    } else if forceDebugMenuWrite {
      debugWriteType = .withoutResponse
      record(
        level: .warn,
        source: "ble.debug_menu",
        title: "debug_menu.write.forced",
        body: "\(debugMenuCharacteristic.uuid.uuidString) properties=\(propertyNames(debugMenuCharacteristic.properties)) reason=\(reason)"
      )
    } else {
      record(
        level: .warn,
        source: "ble.debug_menu",
        title: "debug_menu.write.blocked",
        body: "\(debugMenuCharacteristic.uuid.uuidString) properties=\(propertyNames(debugMenuCharacteristic.properties)) reason=\(reason)"
      )
      return
    }

    debugSkinTemperatureCommandSent = true
    activePeripheral.writeValue(debugSkinTemperatureCommandPayload, for: debugMenuCharacteristic, type: debugWriteType)
    emitCommandWrite(
      source: "ble.debug_menu",
      commandName: "DEBUG_MENU_SKIN_TEMPERATURE",
      commandNumber: nil,
      sequence: nil,
      payload: debugSkinTemperatureCommandPayload,
      frame: debugSkinTemperatureCommandPayload,
      peripheral: activePeripheral,
      characteristic: debugMenuCharacteristic,
      writeType: debugWriteType
    )
    record(
      source: "ble.debug_menu",
      title: "debug_menu.command.sent",
      body: "\(debugMenuCharacteristic.uuid.uuidString) \(writeTypeName(debugWriteType)) payload=\(debugSkinTemperatureCommandPayload.hexString) reason=\(reason)"
    )
  }

  func scheduleHistoricalCommandTimeout(
    kind: HistoricalCommandKind,
    sequence: UInt8,
    timeout: TimeInterval? = nil
  ) {
    historicalCommandTimeoutWorkItem?.cancel()
    let timeoutSeconds = timeout ?? historicalCommandResponseTimeout
    let runID = historicalSyncRunID
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            self.historicalSyncRunID == runID,
            let pending = self.pendingHistoricalCommand,
            pending.kind.commandNumber == kind.commandNumber,
            pending.sequence == sequence else {
        return
      }
      if pending.kind == .getDataRange {
        let timeoutStatus = "timeout seq=\(sequence) pending=\(self.historicalRangePendingResponses) grace=\(Int(timeoutSeconds.rounded()))s"
        self.updateHistoricalRangeDebugStatus(timeoutStatus)
        self.record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.range.timeout",
          body: "GET_DATA_RANGE final response timed out after \(self.historicalRangePendingResponses) pending responses and \(Int(timeoutSeconds.rounded()))s grace after sequence \(sequence)."
        )
        self.retryHistoricalRangeOrFail(reason: timeoutStatus)
        return
      }
      self.failHistoricalSync("\(kind.name) timed out waiting for command response sequence \(sequence).")
    }
    historicalCommandTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + timeoutSeconds, execute: workItem)
  }

  func processQueuedHistoricalDataResultAck(reason: String) -> Bool {
    guard historyEndAckQueued else {
      return false
    }

    historyEndAckQueued = false
    guard let ackPayload = pendingHistoryEndAckPayload else {
      record(
        level: .warn,
        source: "ble.sync",
        title: "historical_sync.result_ack.missing_payload",
        body: "reason=\(reason) packets=\(historicalPacketsReceivedThisSync)"
      )
      if retryHistoricalTransferAfterIdleIfNeeded(reason: "history_result_ack_missing_payload_\(reason)") {
        return true
      }
      completeHistoricalSync(reason: "history_result_ack_missing_payload_\(reason)")
      return true
    }

    if !historicalDataResultAckEnabled && historicalPacketsReceivedThisSync > 0 {
      record(
        level: .warn,
        source: "ble.sync",
        title: "historical_sync.result_ack.suppressed",
        body: "reason=\(reason) packets=\(historicalPacketsReceivedThisSync) payload=\(Data(ackPayload).hexString)"
      )
      if retryHistoricalTransferAfterIdleIfNeeded(reason: "history_result_ack_suppressed_\(reason)") {
        return true
      }
      completeHistoricalSync(reason: "history_result_ack_suppressed_\(reason)")
      return true
    }

    if !historicalDataResultAckEnabled {
      record(
        level: .warn,
        source: "ble.sync",
        title: "historical_sync.result_ack.metadata_only",
        body: "reason=\(reason) packets=\(historicalPacketsReceivedThisSync) payload=\(Data(ackPayload).hexString)"
      )
    }
    historyEndAckSentThisBurst = true
    writeHistoricalCommand(.historicalDataResult)
    return true
  }

  func scheduleHistoricalIdleCompletion(reason: String) {
    historicalIdleWorkItem?.cancel()
    let runID = historicalSyncRunID
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            self.historicalSyncRunID == runID,
            self.isHistoricalSyncing,
            self.pendingHistoricalCommand == nil else {
        return
      }
      if self.processQueuedHistoricalDataResultAck(reason: reason) {
        return
      }
      if self.retryHistoricalTransferAfterIdleIfNeeded(reason: reason) {
        return
      }
      self.completeHistoricalSync(reason: reason)
    }
    historicalIdleWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 12, execute: workItem)
  }

  func retryHistoricalTransferAfterIdleIfNeeded(reason: String) -> Bool {
    guard !historicalRangePollOnly,
          historicalPacketsReceivedThisSync == 0 else {
      return false
    }
    guard historicalTransferRequestAttemptCount < historicalTransferMaxRequestAttempts else {
      let metadataSummary = historyStartReceived || historyEndReceived || historyCompleteReceived
        ? "transfer metadata was received but no historical packet bodies arrived"
        : "a historical transfer never started"
      failHistoricalSync(
        "GET_DATA_RANGE/SEND_HISTORICAL_DATA produced no historical packet bodies after \(historicalTransferRequestAttemptCount) attempts; \(metadataSummary). Last idle reason: \(reason)."
      )
      return true
    }

    historicalSyncStatus = "waiting"
    let nextAttempt = historicalTransferRequestAttemptCount + 1
    let metadataSummary = historyStartReceived || historyEndReceived || historyCompleteReceived
      ? "metadata-only"
      : "no-start"
    publishSyncToast(phase: .syncing, detail: "Retrying historical transfer \(nextAttempt)/\(historicalTransferMaxRequestAttempts)")
    notifyHistoricalSyncProgress(
      status: "waiting",
      detail: "Retrying GET_DATA_RANGE then SEND_HISTORICAL_DATA \(nextAttempt)/\(historicalTransferMaxRequestAttempts) after \(metadataSummary) transfer",
      terminal: false,
      failed: false
    )
    record(
      level: .warn,
      source: "ble.sync",
      title: "historical_sync.transfer.retry",
      body: "attempt=\(nextAttempt)/\(historicalTransferMaxRequestAttempts) first=GET_DATA_RANGE reason=\(reason) previous=\(metadataSummary) history_start=\(historyStartReceived) history_end=\(historyEndReceived) history_complete=\(historyCompleteReceived)"
    )
    historyStartReceived = false
    historyEndReceived = false
    historyCompleteReceived = false
    historyEndAckQueued = false
    historyEndAckSentThisBurst = false
    pendingHistoryEndAckPayload = nil
    writeHistoricalCommand(.getDataRange)
    return true
  }

  func retryHistoricalRangeOrFail(reason: String) {
    pendingHistoricalCommand = nil
    historicalCommandTimeoutWorkItem?.cancel()
    guard historicalRangeRetryCount < historicalRangeMaxRetries else {
      failHistoricalSync("GET_DATA_RANGE did not return a final range after \(historicalRangeRetryCount) retries: \(reason).")
      return
    }

    historicalRangeRetryCount += 1
    let retryNumber = historicalRangeRetryCount
    historicalSyncStatus = "waiting"
    updateHistoricalRangeDebugStatus("retry \(retryNumber)/\(historicalRangeMaxRetries) after \(reason)")
    publishSyncToast(phase: .syncing, detail: "GET_DATA_RANGE retry \(retryNumber)/\(historicalRangeMaxRetries)")
    notifyHistoricalSyncProgress(
      status: "waiting",
      detail: "Retrying GET_DATA_RANGE \(retryNumber)/\(historicalRangeMaxRetries)",
      terminal: false,
      failed: false
    )
    record(
      level: .warn,
      source: "ble.sync",
      title: "historical_sync.range.retry",
      body: "retry=\(retryNumber)/\(historicalRangeMaxRetries) reason=\(reason)"
    )

    historicalRangeRetryWorkItem?.cancel()
    let runID = historicalSyncRunID
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            self.historicalSyncRunID == runID,
            self.isHistoricalSyncing,
            self.pendingHistoricalCommand == nil else {
        return
      }
      self.writeHistoricalCommand(.getDataRange)
    }
    historicalRangeRetryWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + historicalRangeRetryDelay, execute: workItem)
  }

}
