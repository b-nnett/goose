import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func handleHistoricalSyncValue(_ value: Data, characteristic: CBCharacteristic) {
    guard isHistoricalSyncing else {
      return
    }
    for frame in strapFrames(in: value) {
      handleHistoricalSyncFrame(frame, characteristic: characteristic)
    }
  }

  func handleHistoricalSyncFrame(_ frame: Data, characteristic: CBCharacteristic) {
    guard let payload = strapPayload(in: frame),
          let packetType = payload.first else {
      return
    }

    switch packetType {
    case V5PacketType.commandResponse, V5PacketType.puffinCommandResponse:
      handleHistoricalCommandResponse(payload)
    case V5PacketType.historicalData, V5PacketType.historicalIMUDataStream:
      historicalPacketsReceivedThisSync += 1
      // Bound a single sync: a never-synced WHOOP can stream its entire multi-week
      // backlog (oldest-first), which never reaches HistoryComplete in one pass and
      // would balloon storage. Stop after a sane cap; the ACK'd read pointer persists,
      // so a later Sync continues from where this one left off.
      if historicalPacketsReceivedThisSync >= Self.historicalSyncPacketCap {
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.packet_cap",
          body: "reached \(Self.historicalSyncPacketCap) packets; completing this pass to bound storage"
        )
        completeHistoricalSync(reason: "historical_sync_packet_cap")
        return
      }
      publishHistoricalPacketCountIfNeeded()
      scheduleHistoricalIdleCompletion(reason: "historical_data_idle")
      notifyHistoricalSyncProgress(
        status: "syncing",
        detail: "Received historical packet \(historicalPacketsReceivedThisSync)",
        terminal: false,
        failed: false
      )
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.packet",
        body: "\(characteristic.uuid.uuidString) count=\(historicalPacketsReceivedThisSync)"
      )
    case V5PacketType.metadata, V5PacketType.puffinMetadata:
      handleHistoricalMetadata(payload)
    default:
      break
    }
  }

  func publishHistoricalPacketCountIfNeeded(force: Bool = false, at date: Date = Date()) {
    guard force
      || date.timeIntervalSince(lastHistoricalPacketCountPublishedAt) >= Self.historicalPacketCountPublishInterval else {
      return
    }

    lastHistoricalPacketCountPublishedAt = date
    historicalPacketCount = historicalPacketsReceivedThisSync
  }

  func handleAlarmValue(_ value: Data, characteristic: CBCharacteristic) {
    guard notificationCharacteristicIDs.contains(characteristic.uuid) else {
      return
    }
    for frame in strapFrames(in: value) {
      guard let payload = strapPayload(in: frame),
            let packetType = payload.first else {
        continue
      }
      switch packetType {
      case V5PacketType.commandResponse, V5PacketType.puffinCommandResponse:
        handleAlarmCommandResponse(payload)
      case V5PacketType.event:
        handleAlarmEvent(payload)
      default:
        break
      }
    }
  }

  func handleSensorStreamValue(_ value: Data, characteristic: CBCharacteristic) {
    guard notificationCharacteristicIDs.contains(characteristic.uuid) else {
      return
    }
    for frame in strapFrames(in: value) {
      guard let payload = strapPayload(in: frame),
            payload.count >= 5,
            let packetType = payload.first,
            packetType == V5PacketType.commandResponse || packetType == V5PacketType.puffinCommandResponse,
            let commandName = SensorStreamCommandKind.responseNames[payload[2]]
      else {
        continue
      }

      let result = commandResultName(payload[4])
      let responseHex = Data(payload).hexString
      if payload[2] == 96 || payload[2] == 97 {
        handleHighFrequencyHistorySyncCommandResponse(payload, commandName: commandName, result: result)
        continue
      }

      lastPhysiologyCommandSummary = "\(commandName) seq \(payload[3]) \(result)"
      physiologyCaptureStatus = lastPhysiologyCommandSummary
      record(
        source: "ble.sensor",
        title: "sensor.command.response",
        body: "\(lastPhysiologyCommandSummary) payload=\(responseHex)"
      )
    }
  }

  func handleHighFrequencyHistorySyncCommandResponse(_ payload: [UInt8], commandName: String, result: String) {
    let resultCode = payload[4]
    lastHighFrequencyHistorySyncResponse = "\(commandName) seq \(payload[3]) \(result)"
    if resultCode == 1 {
      if payload[2] == 96 {
        highFrequencyHistorySyncActive = true
        highFrequencyHistorySyncExpiresAt = highFrequencyHistorySyncRequestedExpiry
        highFrequencyHistorySyncStatus = "Active"
      } else {
        highFrequencyHistorySyncActive = false
        highFrequencyHistorySyncRequestedExpiry = nil
        highFrequencyHistorySyncExpiresAt = nil
        highFrequencyHistorySyncStatus = "Off"
      }
      record(
        source: "ble.high_frequency_sync",
        title: "command.response",
        body: "\(lastHighFrequencyHistorySyncResponse) payload=\(Data(payload).hexString)"
      )
    } else {
      highFrequencyHistorySyncStatus = "\(commandName) \(result)"
      record(
        level: .warn,
        source: "ble.high_frequency_sync",
        title: "command.response",
        body: "\(lastHighFrequencyHistorySyncResponse) payload=\(Data(payload).hexString)"
      )
    }
  }

  func handleClockValue(_ value: Data, characteristic: CBCharacteristic) {
    guard notificationCharacteristicIDs.contains(characteristic.uuid) else {
      return
    }
    for frame in strapFrames(in: value) {
      guard let payload = strapPayload(in: frame),
            payload.count >= 5,
            let packetType = payload.first,
            packetType == V5PacketType.commandResponse || packetType == V5PacketType.puffinCommandResponse,
            payload[2] == 10 || payload[2] == 11 else {
        continue
      }
      handleClockCommandResponse(payload)
    }
  }

  func handleClockCommandResponse(_ payload: [UInt8]) {
    guard payload.count >= 5 else {
      return
    }
    guard let pending = pendingClockCommand else {
      record(level: .debug, source: "ble.clock", title: "clock.response.unmatched", body: "no pending command payload=\(Data(payload).hexString)")
      return
    }
    guard payload[2] == pending.kind.commandNumber,
          payload[3] == pending.sequence else {
      record(level: .debug, source: "ble.clock", title: "clock.response.ignored", body: "pending=\(pending.kind.name) seq=\(pending.sequence) payload=\(Data(payload).hexString)")
      return
    }

    clockCommandTimeoutWorkItem?.cancel()
    pendingClockCommand = nil
    let resultCode = payload[4]
    let result = commandResultName(resultCode)
    let body = Array(payload.dropFirst(5))
    lastClockResponsePayloadHex = Data(payload).hexString

    guard resultCode == 1 else {
      failClockCommand("\(pending.kind.name) returned \(result) (\(resultCode)) for sequence \(pending.sequence).")
      return
    }

    switch pending.kind {
    case .get:
      guard let reading = Self.parseClockTimestamp(body) else {
        failClockCommand("GET_CLOCK returned an invalid clock body: \(Data(body).hexString).")
        return
      }
      let receivedAt = Date()
      let estimatedLocalAtSample = pending.sentAt.addingTimeInterval(receivedAt.timeIntervalSince(pending.sentAt) / 2)
      let offset = reading.timeIntervalSince(estimatedLocalAtSample)
      strapClockDate = reading
      strapClockOffsetSeconds = offset
      strapClockUpdatedAt = receivedAt

      if pending.syncIfNeeded && abs(offset) > Self.strapClockAutoSyncThresholdSeconds {
        strapClockStatus = "Clock out by \(Self.clockOffsetText(offset)); syncing"
        record(
          source: "ble.clock",
          title: "clock.drift.syncing",
          body: "offset=\(String(format: "%.3f", offset)) threshold=\(Self.strapClockAutoSyncThresholdSeconds)"
        )
        writeClockCommand(.set(Date()), syncIfNeeded: false)
      } else {
        strapClockStatus = "Clock in sync"
        record(
          source: "ble.clock",
          title: "clock.read",
          body: "offset=\(String(format: "%.3f", offset)) threshold=\(Self.strapClockAutoSyncThresholdSeconds)"
        )
      }
    case .set(let setDate):
      let completedAt = Date()
      strapClockDate = setDate
      strapClockOffsetSeconds = 0
      strapClockUpdatedAt = completedAt
      strapClockStatus = "Clock synced"
      record(source: "ble.clock", title: "clock.synced", body: "seq=\(pending.sequence) \(result)")
    }
  }

  func handleAlarmCommandResponse(_ payload: [UInt8]) {
    guard payload.count >= 5 else {
      return
    }
    guard [UInt8(66), 67, 68, 69].contains(payload[2]) else {
      return
    }
    guard let pending = pendingAlarmCommand else {
      record(level: .debug, source: "ble.alarm", title: "alarm.response.unmatched", body: "no pending command payload=\(Data(payload).hexString)")
      return
    }
    guard payload[2] == pending.kind.commandNumber,
          payload[3] == pending.sequence else {
      record(level: .debug, source: "ble.alarm", title: "alarm.response.ignored", body: "pending=\(pending.kind.name) seq=\(pending.sequence) payload=\(Data(payload).hexString)")
      return
    }

    alarmCommandTimeoutWorkItem?.cancel()
    pendingAlarmCommand = nil
    let resultCode = payload[4]
    let body = Array(payload.dropFirst(5))
    let result = commandResultName(resultCode)
    let detail = alarmResponseDetail(command: pending.kind, body: body)
    lastAlarmResponsePayloadHex = Data(payload).hexString
    lastAlarmResponseSummary = "\(pending.kind.name) seq \(pending.sequence) \(result)\(detail)"

    if resultCode == 1 {
      if let scheduledDate = pending.kind.scheduledDate {
        lastAlarmScheduledAt = scheduledDate
      }
      if let alarmID = pending.kind.alarmID {
        lastAlarmID = Int(alarmID)
      }
      if case .disableAll = pending.kind {
        lastAlarmScheduledAt = nil
        lastAlarmID = nil
      }
      alarmCommandStatus = "\(pending.kind.name) \(result)\(detail)"
      record(source: "ble.alarm", title: "alarm.command.response", body: "\(lastAlarmResponseSummary) payload=\(lastAlarmResponsePayloadHex)")
    } else {
      alarmCommandStatus = "\(pending.kind.name) \(result)\(detail)"
      record(level: .warn, source: "ble.alarm", title: "alarm.command.response", body: "\(lastAlarmResponseSummary) payload=\(lastAlarmResponsePayloadHex)")
    }
  }

  func handleAlarmEvent(_ payload: [UInt8]) {
    guard payload.count >= 12 else {
      return
    }
    let eventType = UInt16(payload[2]) | UInt16(payload[3]) << 8
    let eventBody = Array(payload.dropFirst(12))
    lastAlarmEventPayloadHex = Data(payload).hexString
    switch eventType {
    case 56:
      handleAlarmSetEvent(eventBody)
    case 57:
      alarmCommandStatus = "WHOOP alarm executed"
      lastAlarmEventSummary = "STRAP_DRIVEN_ALARM_EXECUTED"
      record(source: "ble.alarm", title: "alarm.event", body: "STRAP_DRIVEN_ALARM_EXECUTED")
    case 58:
      alarmCommandStatus = "WHOOP app-driven alarm executed"
      lastAlarmEventSummary = "APP_DRIVEN_ALARM_EXECUTED"
      record(source: "ble.alarm", title: "alarm.event", body: "APP_DRIVEN_ALARM_EXECUTED")
    case 59:
      lastAlarmScheduledAt = nil
      lastAlarmID = nil
      alarmCommandStatus = "WHOOP alarm disabled"
      lastAlarmEventSummary = "STRAP_DRIVEN_ALARM_DISABLED"
      record(source: "ble.alarm", title: "alarm.event", body: "STRAP_DRIVEN_ALARM_DISABLED")
    case 60:
      lastAlarmEventSummary = "HAPTICS_FIRED"
      record(source: "ble.alarm", title: "alarm.event", body: "HAPTICS_FIRED")
    case 96:
      lastHighFrequencyHistorySyncEvent = "HIGH_FREQ_SYNC_PROMPT"
      record(
        source: "ble.high_frequency_sync",
        title: "event",
        body: "\(lastHighFrequencyHistorySyncEvent) body=\(Data(eventBody).hexString) payload=\(Data(payload).hexString)"
      )
    case 97:
      highFrequencyHistorySyncActive = true
      if highFrequencyHistorySyncExpiresAt == nil {
        highFrequencyHistorySyncExpiresAt = highFrequencyHistorySyncRequestedExpiry
      }
      highFrequencyHistorySyncStatus = "Active"
      lastHighFrequencyHistorySyncEvent = "HIGH_FREQ_SYNC_ENABLED"
      record(
        source: "ble.high_frequency_sync",
        title: "event",
        body: "\(lastHighFrequencyHistorySyncEvent) body=\(Data(eventBody).hexString) payload=\(Data(payload).hexString)"
      )
    case 98:
      highFrequencyHistorySyncActive = false
      highFrequencyHistorySyncRequestedExpiry = nil
      highFrequencyHistorySyncExpiresAt = nil
      highFrequencyHistorySyncStatus = "Off"
      lastHighFrequencyHistorySyncEvent = "HIGH_FREQ_SYNC_DISABLED"
      record(
        source: "ble.high_frequency_sync",
        title: "event",
        body: "\(lastHighFrequencyHistorySyncEvent) body=\(Data(eventBody).hexString) payload=\(Data(payload).hexString)"
      )
    case 100:
      let reason = eventBody.count >= 2 ? hapticsTerminationName(eventBody[1]) : "unknown"
      alarmCommandStatus = "Haptics terminated: \(reason)"
      lastAlarmEventSummary = "HAPTICS_TERMINATED \(reason)"
      record(source: "ble.alarm", title: "alarm.event", body: "HAPTICS_TERMINATED \(reason)")
    default:
      break
    }
  }

  func handleAlarmSetEvent(_ body: [UInt8]) {
    guard body.count >= 8 else {
      alarmCommandStatus = "WHOOP alarm set event received"
      lastAlarmEventSummary = "STRAP_DRIVEN_ALARM_SET short body=\(Data(body).hexString)"
      record(source: "ble.alarm", title: "alarm.event", body: "STRAP_DRIVEN_ALARM_SET")
      return
    }
    let revision = body[0]
    let alarmID: UInt8?
    let secondsOffset: Int
    if revision >= 2, body.count >= 8 {
      alarmID = body[1]
      secondsOffset = 2
    } else {
      alarmID = nil
      secondsOffset = 1
    }
    guard body.count >= secondsOffset + 6 else {
      return
    }
    let seconds = UInt32(body[secondsOffset])
      | UInt32(body[secondsOffset + 1]) << 8
      | UInt32(body[secondsOffset + 2]) << 16
      | UInt32(body[secondsOffset + 3]) << 24
    let subseconds = UInt16(body[secondsOffset + 4]) | UInt16(body[secondsOffset + 5]) << 8
    let date = Date(timeIntervalSince1970: TimeInterval(seconds) + TimeInterval(subseconds) / 32768.0)
    lastAlarmScheduledAt = date
    if let alarmID {
      lastAlarmID = Int(alarmID)
    }
    alarmCommandStatus = "WHOOP alarm set for \(Self.alarmTimeFormatter.string(from: date))"
    lastAlarmEventSummary = "STRAP_DRIVEN_ALARM_SET slot \(alarmID.map(String.init) ?? "legacy") \(date.formatted(date: .abbreviated, time: .standard))"
    record(
      source: "ble.alarm",
      title: "alarm.event",
      body: "STRAP_DRIVEN_ALARM_SET revision=\(revision) alarmID=\(alarmID.map(String.init) ?? "legacy")"
    )
  }

  func handleHistoricalCommandResponse(_ payload: [UInt8]) {
    guard payload.count >= 5,
          let pending = pendingHistoricalCommand,
          payload[2] == pending.kind.commandNumber,
          payload[3] == pending.sequence else {
      return
    }

    let resultCode = payload[4]
    let result = commandResultName(resultCode)
    let detail = historicalResponseDetail(command: pending.kind, payload: payload)
    if pending.kind == .getDataRange {
      updateHistoricalRangeDebugStatus(
        "raw_response seq=\(pending.sequence) result=\(result)(\(resultCode)) payload=\(Data(payload).hexString)\(detail)"
      )
    }
    record(
      level: .debug,
      source: "ble.sync",
      title: "historical_sync.command.raw_response",
      body: "\(pending.kind.name) seq=\(pending.sequence) result=\(result)(\(resultCode)) payload=\(Data(payload).hexString)\(detail)"
    )
    if resultCode == 2 {
      if pending.kind == .getDataRange {
        emitHistoricalRangeTelemetry(
          status: "pending",
          pending: pending,
          resultCode: resultCode,
          resultName: result,
          payload: payload,
          notes: "GET_DATA_RANGE returned PENDING; waiting for final response"
        )
      }
      handleHistoricalCommandPending(pending)
      return
    }

    historicalCommandTimeoutWorkItem?.cancel()
    pendingHistoricalCommand = nil
    guard resultCode == 1 else {
      if pending.kind == .getDataRange {
        let reason = "rejected seq=\(pending.sequence) result=\(result)(\(resultCode))\(detail)"
        updateHistoricalRangeDebugStatus(reason)
        emitHistoricalRangeTelemetry(
          status: "rejected",
          pending: pending,
          resultCode: resultCode,
          resultName: result,
          payload: payload,
          notes: reason
        )
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.range.rejected",
          body: "GET_DATA_RANGE returned \(result) (\(resultCode)).\(detail)"
        )
        retryHistoricalRangeOrFail(reason: reason)
        return
      }
      failHistoricalSync("\(pending.kind.name) returned \(result) (\(resultCode)) for sequence \(pending.sequence).")
      return
    }

    if pending.kind == .getDataRange {
      guard isValidHistoricalRangeResponse(payload) else {
        let reason = "invalid_body seq=\(pending.sequence)\(detail)"
        updateHistoricalRangeDebugStatus(reason)
        emitHistoricalRangeTelemetry(
          status: "invalid_body",
          pending: pending,
          resultCode: resultCode,
          resultName: result,
          payload: payload,
          notes: reason
        )
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.range.invalid_body",
          body: reason
        )
        retryHistoricalRangeOrFail(reason: reason)
        return
      }
      updateHistoricalRangeDebugStatus("success seq=\(pending.sequence)\(detail)")
      emitHistoricalRangeTelemetry(
        status: "success",
        pending: pending,
        resultCode: resultCode,
        resultName: result,
        payload: payload,
        notes: "valid GET_DATA_RANGE response"
      )
    }
    record(source: "ble.sync", title: "historical_sync.command.response", body: "\(pending.kind.name) seq=\(pending.sequence) \(result)\(detail)")

    if pending.kind != .historicalDataResult,
       processQueuedHistoricalDataResultAck(reason: "after_\(pending.kind.name)") {
      return
    }

    switch pending.kind {
    case .getDataRange:
      if historicalRangePollOnly {
        completeHistoricalSync(reason: "historical_range_poll_complete")
        return
      }
      writeHistoricalCommand(.sendHistoricalData)
    case .sendHistoricalData:
      scheduleHistoricalIdleCompletion(reason: "historical_transfer_idle")
    case .historicalDataResult:
      pendingHistoryEndAckPayload = nil
      if historyCompleteReceived {
        completeHistoricalSync(reason: "history_complete")
      } else {
        scheduleHistoricalIdleCompletion(reason: "history_end_ack_idle")
      }
    }
  }

  func handleHistoricalCommandPending(_ pending: PendingHistoricalCommand) {
    if pending.kind == .getDataRange {
      historicalRangePendingResponses += 1
      updateHistoricalRangeDebugStatus("pending seq=\(pending.sequence) count=\(historicalRangePendingResponses)")
      scheduleHistoricalCommandTimeout(
        kind: pending.kind,
        sequence: pending.sequence,
        timeout: historicalPendingResponseGrace
      )
    }
    historicalSyncStatus = "waiting"
    publishSyncToast(phase: .syncing, detail: "\(pending.kind.name) pending; waiting for final response")
    notifyHistoricalSyncProgress(
      status: "waiting",
      detail: "\(pending.kind.name) pending; waiting for final response",
      terminal: false,
      failed: false
    )
    record(
      level: .debug,
      source: "ble.sync",
      title: "historical_sync.command.pending",
      body: "\(pending.kind.name) seq=\(pending.sequence) returned PENDING (2); waiting for SUCCESS/FAILURE/UNSUPPORTED. range_pending=\(historicalRangePendingResponses) grace=\(Int(historicalPendingResponseGrace.rounded()))s"
    )
  }

  func handleHistoricalMetadata(_ payload: [UInt8]) {
    let rawKind: UInt16?
    if payload.first == V5PacketType.puffinMetadata {
      rawKind = payload.count >= 4 ? UInt16(payload[2]) | UInt16(payload[3]) << 8 : nil
    } else {
      rawKind = payload.count >= 3 ? UInt16(payload[2]) : nil
    }
    guard let rawKind, let kind = HistoricalMetadataKind(rawValue: rawKind) else {
      return
    }

    record(source: "ble.sync", title: "historical_sync.metadata", body: kind.name)
    notifyHistoricalSyncProgress(status: "syncing", detail: "Metadata \(kind.name)", terminal: false, failed: false)
    scheduleHistoricalIdleCompletion(reason: "historical_metadata_idle")

    switch kind {
    case .historyStart:
      historyStartReceived = true
      historyEndAckQueued = false
      historyEndAckSentThisBurst = false
      pendingHistoryEndAckPayload = nil
    case .historyEnd:
      historyEndReceived = true
      guard !historyEndAckSentThisBurst else {
        record(
          level: .debug,
          source: "ble.sync",
          title: "historical_sync.result_ack.already_sent",
          body: "history_end packets=\(historicalPacketsReceivedThisSync) payload=\(Data(payload).hexString)"
        )
        return
      }
      guard let ackPayload = Self.historicalDataResultPayload(fromHistoryEndMetadataPayload: payload) else {
        historyEndAckQueued = false
        pendingHistoryEndAckPayload = nil
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.result_ack.unprepared",
          body: "short_history_end payload=\(Data(payload).hexString)"
        )
        return
      }
      pendingHistoryEndAckPayload = ackPayload
      historyEndAckQueued = true
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.result_ack.prepared",
        body: "payload=\(Data(ackPayload).hexString) history_end_body=\(Data(payload.dropFirst(9)).hexString) packets=\(historicalPacketsReceivedThisSync) ack_enabled=\(historicalDataResultAckEnabled)"
      )
      if pendingHistoricalCommand == nil {
        _ = processQueuedHistoricalDataResultAck(reason: "history_end")
      }
    case .historyComplete:
      historyCompleteReceived = true
      // The band signalled that the full history has been transferred. If we already
      // pulled packet bodies this run, the sync succeeded — complete it now. Otherwise
      // the post-completion idle/retry path treats the trailing empty pages as "no
      // bodies" and marks a successful 13k-packet transfer as failed.
      if historicalPacketsReceivedThisSync > 0 {
        completeHistoricalSync(reason: "history_complete_after_data")
        return
      }
      guard !historyEndAckSentThisBurst else {
        return
      }
      guard pendingHistoryEndAckPayload != nil else {
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.result_ack.missing_payload",
          body: "history_complete packets=\(historicalPacketsReceivedThisSync) payload=\(Data(payload).hexString)"
        )
        return
      }
      historyEndAckQueued = true
      if pendingHistoricalCommand == nil {
        _ = processQueuedHistoricalDataResultAck(reason: "history_complete")
      }
    }
  }

  func completeHistoricalSync(reason: String) {
    historicalCommandTimeoutWorkItem?.cancel()
    historicalIdleWorkItem?.cancel()
    historicalRangeRetryWorkItem?.cancel()
    readySyncWorkItem?.cancel()
    let sawHistoricalMetadata = historyStartReceived || historyEndReceived || historyCompleteReceived
    pendingHistoricalCommand = nil
    historyEndAckQueued = false
    historyEndAckSentThisBurst = false
    pendingHistoryEndAckPayload = nil
    historyStartReceived = false
    historyEndReceived = false
    historyCompleteReceived = false
    historicalRangePendingResponses = 0
    historicalRangeRetryCount = 0
    historicalTransferRequestAttemptCount = 0
    historicalDataResultAckEnabled = true
    let completedAt = Date()
    let rangeOnly = historicalRangePollOnly
    isHistoricalSyncing = false
    historicalRangePollOnly = false
    publishHistoricalPacketCountIfNeeded(force: true, at: completedAt)
    historicalSyncStatus = "synced"
    lastHistoricalSyncCompletedAt = completedAt
    lastSyncAt = completedAt
    let detail = rangeOnly
      ? "Historical range poll complete"
      : sawHistoricalMetadata && historicalPacketsReceivedThisSync == 0
      ? "Historical metadata captured but no packet bodies received"
      : historicalPacketsReceivedThisSync == 0
      ? "No missed packets found"
      : "\(historicalPacketsReceivedThisSync) historical \(historicalPacketsReceivedThisSync == 1 ? "packet" : "packets") captured"
    publishSyncToast(phase: .synced, detail: detail, clearAfter: 2.2)
    notifyHistoricalSyncProgress(status: "synced", detail: detail, terminal: true, failed: false)
    record(source: "ble.sync", title: "historical_sync.completed", body: "reason=\(reason) \(detail)")
  }

  func failHistoricalSync(_ message: String) {
    historicalCommandTimeoutWorkItem?.cancel()
    historicalIdleWorkItem?.cancel()
    historicalRangeRetryWorkItem?.cancel()
    readySyncWorkItem?.cancel()
    pendingHistoricalCommand = nil
    historyEndAckQueued = false
    historyEndAckSentThisBurst = false
    pendingHistoryEndAckPayload = nil
    historyStartReceived = false
    historyEndReceived = false
    historyCompleteReceived = false
    historicalRangePendingResponses = 0
    historicalRangeRetryCount = 0
    historicalTransferRequestAttemptCount = 0
    historicalDataResultAckEnabled = true
    isHistoricalSyncing = false
    historicalRangePollOnly = false
    publishHistoricalPacketCountIfNeeded(force: true)
    historicalSyncStatus = "failed"
    let failure = GooseSyncFailure(title: "Sync Failed", message: message, occurredAt: Date())
    lastSyncFailure = failure
    syncFailureSheet = failure
    publishSyncToast(phase: .failed, detail: "Tap for details", clearAfter: 4.5)
    notifyHistoricalSyncProgress(status: "failed", detail: message, terminal: true, failed: true)
    record(level: .error, source: "ble.sync", title: "historical_sync.failed", body: message)
  }

  func notifyHistoricalSyncProgress(status: String, detail: String, terminal: Bool, failed: Bool) {
    let capturedAt = Date()
    let highVolumePacketProgress = !terminal
      && !failed
      && status == "syncing"
      && detail.hasPrefix("Received historical packet ")
    let statusChanged = status != lastHistoricalSyncProgressCallbackStatus
      || (!highVolumePacketProgress && detail != lastHistoricalSyncProgressCallbackDetail)
    let elapsed = capturedAt.timeIntervalSince(lastHistoricalSyncProgressCallbackAt)
    let shouldPublish = terminal
      || failed
      || statusChanged
      || elapsed >= Self.historicalProgressCallbackInterval
    guard shouldPublish else {
      coalescedHistoricalSyncProgressCallbackCount += 1
      return
    }

    let coalescedCount = coalescedHistoricalSyncProgressCallbackCount
    coalescedHistoricalSyncProgressCallbackCount = 0
    lastHistoricalSyncProgressCallbackAt = capturedAt
    lastHistoricalSyncProgressCallbackStatus = status
    lastHistoricalSyncProgressCallbackDetail = detail
    if coalescedCount > 0 {
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.progress.coalesced",
        body: "count=\(coalescedCount) reason=callback_interval_\(Self.historicalProgressCallbackInterval)s packets=\(historicalPacketsReceivedThisSync) status=\(status)"
      )
    }

    onHistoricalSyncProgress?(
      GooseHistoricalSyncProgress(
        status: status,
        detail: detail,
        packetCount: historicalPacketsReceivedThisSync,
        isTerminal: terminal,
        failed: failed,
        capturedAt: capturedAt
      )
    )
  }

  func publishSyncToast(
    phase: GooseSyncToastPhase,
    titleOverride: String? = nil,
    detail: String,
    clearAfter: TimeInterval? = nil
  ) {
    syncClearWorkItem?.cancel()
    let title: String
    switch phase {
    case .syncing:
      title = "Syncing"
    case .synced:
      title = "Synced"
    case .failed:
      title = "Sync Failed"
    }
    syncToast = GooseSyncToast(phase: phase, title: titleOverride ?? title, detail: detail)
    guard let clearAfter else {
      return
    }
    let toastID = syncToast?.id
    let workItem = DispatchWorkItem { [weak self] in
      guard self?.syncToast?.id == toastID else {
        return
      }
      self?.syncToast = nil
    }
    syncClearWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + clearAfter, execute: workItem)
  }

}
