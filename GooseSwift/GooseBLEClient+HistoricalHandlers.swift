import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func handleHistoricalSyncValue(_ value: Data, characteristic: CBCharacteristic) {
    guard isHistoricalSyncing else {
      return
    }
    for frame in frames(in: value) {
      handleHistoricalSyncFrame(frame, characteristic: characteristic)
    }
  }

  func handleHistoricalSyncFrame(_ frame: Data, characteristic: CBCharacteristic) {
    guard let payload = payload(in: frame),
          let packetType = payload.first else {
      return
    }

    switch packetType {
    case V5PacketType.commandResponse, V5PacketType.puffinCommandResponse:
      handleHistoricalCommandResponse(payload)
    case V5PacketType.historicalData, V5PacketType.historicalIMUDataStream:
      historicalManager.historicalPacketsReceivedThisSync &+= 1 // SYNC-02: wrapping add; long sync wraps instead of trapping
      publishHistoricalPacketCountIfNeeded()
      scheduleHistoricalIdleCompletion(reason: "historical_data_idle")
      notifyHistoricalSyncProgress(
        status: "syncing",
        detail: "Received historical packet \(historicalManager.historicalPacketsReceivedThisSync)",
        terminal: false,
        failed: false
      )
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.packet",
        body: "\(characteristic.uuid.uuidString) count=\(historicalManager.historicalPacketsReceivedThisSync)"
      )
      // Direct write: accumulate frame hex and flush in batches, bypassing the unbounded
      // async notification pipeline that causes jetsam kills on long syncs (WHOOP pattern:
      // createWhoopStatusPacketEntityWithData → immediate CoreData write per packet).
      let hex = frame.map { String(format: "%02x", $0) }.joined()
      let capturedAtISO = ISO8601DateFormatter().string(from: Date())
      historicalManager.pendingHistoricalFrames.append((hex: hex, capturedAt: capturedAtISO))
      historicalManager.lastHandledWasHistoricalDataPacket = true
      flushPendingHistoricalFramesIfNeeded()
    case V5PacketType.metadata, V5PacketType.puffinMetadata:
      handleHistoricalMetadata(payload)
    default:
      break
    }
  }

  func publishHistoricalPacketCountIfNeeded(force: Bool = false, at date: Date = Date()) {
    guard force
      || date.timeIntervalSince(historicalManager.lastHistoricalPacketCountPublishedAt) >= Self.historicalPacketCountPublishInterval else {
      return
    }

    historicalManager.lastHistoricalPacketCountPublishedAt = date
    historicalPacketCount = historicalManager.historicalPacketsReceivedThisSync
  }

  func flushPendingHistoricalFramesIfNeeded(force: Bool = false) {
    guard force || historicalManager.pendingHistoricalFrames.count >= Self.historicalFrameFlushBatchSize else {
      return
    }
    guard !historicalManager.pendingHistoricalFrames.isEmpty, !historicalDirectWriteDatabasePath.isEmpty else {
      historicalManager.pendingHistoricalFrames.removeAll()
      return
    }
    let frames = historicalManager.pendingHistoricalFrames
    historicalManager.pendingHistoricalFrames.removeAll()
    let deviceUUID = selectedDeviceID?.uuidString
    let deviceType: String
    switch activeDeviceGeneration {
    case .gen4: deviceType = "GEN4"
    case .gen5: deviceType = "MAVERICK"
    }
    let frameObjects: [[String: Any]] = frames.map { f in
      [
        "evidence_id": UUID().uuidString,
        "source": "historical_sync",
        "captured_at": f.capturedAt,
        "device_model": rememberedDeviceName ?? deviceType,
        "frame_hex": f.hex,
        "sensitivity": "normal",
        "device_type": deviceType,
        "device_uuid": deviceUUID as Any,
        "capture_session_id": NSNull(),
      ]
    }
    let args: [String: Any] = [
      "database_path": historicalDirectWriteDatabasePath,
      "frames": frameObjects,
      "include_results": false,
      "include_timeline_rows": false,
      "compact_raw_payloads": true,
      "active_device_id": deviceUUID as Any,
    ]
    do {
      _ = try historicalDirectWriteBridge.request(method: "capture.import_frame_batch", args: args)
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.direct_write.flushed",
        body: "count=\(frames.count) total=\(historicalManager.historicalPacketsReceivedThisSync)"
      )
    } catch {
      record(level: .warn, source: "ble.sync", title: "historical_sync.direct_write.error", body: error.localizedDescription)
    }
  }

  func handleAlarmValue(_ value: Data, characteristic: CBCharacteristic) {
    guard notificationCharacteristicIDs.contains(characteristic.uuid) else {
      return
    }
    for frame in frames(in: value) {
      guard let payload = payload(in: frame),
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
    for frame in frames(in: value) {
      guard let payload = payload(in: frame),
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
    for frame in frames(in: value) {
      guard let payload = payload(in: frame),
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
          let pending = historicalManager.pendingHistoricalCommand,
          payload[2] == pending.kind.commandNumber,
          payload[3] == pending.sequence else {
      return
    }

    // Gen4 cmd 22 replies with body `<echoed_seq> 02 0b 00 00`. The 0x02 in
    // the result-code slot is a Gen4 success ack, not Gen5 PENDING — so we
    // bypass the Gen5 result-code logic and immediately advance to cmd 23.
    // historicalManager.gen4HistoricalPageSeq was set by the preceding cmd 34 response.
    if activeDeviceGeneration == .gen4 && pending.kind == .sendHistoricalData {
      historicalManager.historicalCommandTimeoutWorkItem?.cancel()
      historicalManager.pendingHistoricalCommand = nil
      record(
        source: "ble.sync",
        title: "historical_sync.gen4.transfer_ack",
        body: "seq=\(pending.sequence) payload=\(Data(payload).hexString)"
      )
      record(
        source: "ble.sync",
        title: "historical_sync.gen4.transfer_armed",
        body: "next_seq=\(historicalManager.gen4HistoricalPageSeq)"
      )
      historicalManager.pendingHistoryEndAckPayload = gen4PageRequestPayload(seq: historicalManager.gen4HistoricalPageSeq)
      writeHistoricalCommand(.historicalDataResult)
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

    historicalManager.historicalCommandTimeoutWorkItem?.cancel()
    historicalManager.pendingHistoricalCommand = nil
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
      if activeDeviceGeneration == .gen4 {
        guard payload.count >= 14 else {
          failHistoricalSync("Gen4 cmd 34 response too short: \(payload.count) bytes payload=\(Data(payload).hexString)")
          return
        }
        let lastSynced = UInt32(payload[10])
          | (UInt32(payload[11]) << 8)
          | (UInt32(payload[12]) << 16)
          | (UInt32(payload[13]) << 24)
        historicalManager.gen4HistoricalPageSeq = lastSynced &+ 1
        record(
          source: "ble.sync",
          title: "historical_sync.gen4.range",
          body: "last_synced=\(lastSynced) next_seq=\(historicalManager.gen4HistoricalPageSeq)"
        )
        if historicalManager.historicalRangePollOnly {
          completeHistoricalSync(reason: "gen4_range_poll_complete")
          return
        }
        writeHistoricalCommand(.sendHistoricalData)
        return
      }
      if historicalManager.historicalRangePollOnly {
        completeHistoricalSync(reason: "historical_range_poll_complete")
        return
      }
      writeHistoricalCommand(.sendHistoricalData)
    case .sendHistoricalData:
      scheduleHistoricalIdleCompletion(reason: "historical_transfer_idle")
    case .historicalDataResult:
      historicalManager.pendingHistoryEndAckPayload = nil
      if historicalManager.historyCompleteReceived {
        completeHistoricalSync(reason: "history_complete")
      } else {
        scheduleHistoricalIdleCompletion(reason: "history_end_ack_idle")
      }
    }
  }

  func handleHistoricalCommandPending(_ pending: PendingHistoricalCommand) {
    if pending.kind == .getDataRange {
      historicalManager.historicalRangePendingResponses &+= 1 // SYNC-02: wrapping add; long sync wraps instead of trapping
      updateHistoricalRangeDebugStatus("pending seq=\(pending.sequence) count=\(historicalManager.historicalRangePendingResponses)")
      scheduleHistoricalCommandTimeout(
        kind: pending.kind,
        sequence: pending.sequence,
        timeout: historicalManager.historicalPendingResponseGrace
      )
    }
    historicalManager.setStatus("waiting")
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
      body: "\(pending.kind.name) seq=\(pending.sequence) returned PENDING (2); waiting for SUCCESS/FAILURE/UNSUPPORTED. range_pending=\(historicalManager.historicalRangePendingResponses) grace=\(Int(historicalManager.historicalPendingResponseGrace.rounded()))s"
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
      historicalManager.historyStartReceived = true
      historicalManager.historyEndAckQueued = false
      historicalManager.historyEndAckSentThisBurst = false
      historicalManager.pendingHistoryEndAckPayload = nil
    case .historyEnd:
      flushPendingHistoricalFramesIfNeeded(force: true)
      historicalManager.historyEndReceived = true
      guard !historicalManager.historyEndAckSentThisBurst else {
        record(
          level: .debug,
          source: "ble.sync",
          title: "historical_sync.result_ack.already_sent",
          body: "history_end packets=\(historicalManager.historicalPacketsReceivedThisSync) payload=\(Data(payload).hexString)"
        )
        return
      }
      let ackPayload: [UInt8]
      if activeDeviceGeneration == .gen4 {
        historicalManager.gen4HistoricalPageSeq &+= 1
        ackPayload = gen4PageRequestPayload(seq: historicalManager.gen4HistoricalPageSeq)
        record(
          level: .debug,
          source: "ble.sync",
          title: "historical_sync.gen4.page_end",
          body: "next_seq=\(historicalManager.gen4HistoricalPageSeq) packets=\(historicalManager.historicalPacketsReceivedThisSync)"
        )
      } else {
        guard let v5Payload = Self.historicalDataResultPayload(fromHistoryEndMetadataPayload: payload) else {
          historicalManager.historyEndAckQueued = false
          historicalManager.pendingHistoryEndAckPayload = nil
          record(
            level: .warn,
            source: "ble.sync",
            title: "historical_sync.result_ack.unprepared",
            body: "short_history_end payload=\(Data(payload).hexString)"
          )
          return
        }
        ackPayload = v5Payload
      }
      historicalManager.pendingHistoryEndAckPayload = ackPayload
      historicalManager.historyEndAckQueued = true
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.result_ack.prepared",
        body: "payload=\(Data(ackPayload).hexString) history_end_body=\(Data(payload.dropFirst(9)).hexString) packets=\(historicalManager.historicalPacketsReceivedThisSync) ack_enabled=\(historicalManager.historicalDataResultAckEnabled)"
      )
      if historicalManager.pendingHistoricalCommand == nil {
        _ = processQueuedHistoricalDataResultAck(reason: "history_end")
      }
    case .historyComplete:
      historicalManager.historyCompleteReceived = true
      guard !historicalManager.historyEndAckSentThisBurst else {
        return
      }
      guard historicalManager.pendingHistoryEndAckPayload != nil else {
        record(
          level: .warn,
          source: "ble.sync",
          title: "historical_sync.result_ack.missing_payload",
          body: "history_complete packets=\(historicalManager.historicalPacketsReceivedThisSync) payload=\(Data(payload).hexString)"
        )
        return
      }
      historicalManager.historyEndAckQueued = true
      if historicalManager.pendingHistoricalCommand == nil {
        _ = processQueuedHistoricalDataResultAck(reason: "history_complete")
      }
    }
  }

  func completeHistoricalSync(reason: String) {
    historicalManager.historicalCommandTimeoutWorkItem?.cancel()
    historicalManager.historicalIdleWorkItem?.cancel()
    historicalManager.historicalRangeRetryWorkItem?.cancel()
    readySyncWorkItem?.cancel()
    let sawHistoricalMetadata = historicalManager.historyStartReceived || historicalManager.historyEndReceived || historicalManager.historyCompleteReceived
    historicalManager.pendingHistoricalCommand = nil
    historicalManager.historyEndAckQueued = false
    historicalManager.historyEndAckSentThisBurst = false
    historicalManager.pendingHistoryEndAckPayload = nil
    historicalManager.historyStartReceived = false
    historicalManager.historyEndReceived = false
    historicalManager.historyCompleteReceived = false
    historicalManager.historicalRangePendingResponses = 0
    historicalManager.historicalRangeRetryCount = 0
    historicalManager.historicalTransferRequestAttemptCount = 0
    historicalManager.historicalDataResultAckEnabled = true
    let completedAt = Date()
    let rangeOnly = historicalManager.historicalRangePollOnly
    flushPendingHistoricalFramesIfNeeded(force: true)
    historicalManager.completeSync(completedAt: completedAt)
    historicalManager.historicalRangePollOnly = false
    publishHistoricalPacketCountIfNeeded(force: true, at: completedAt)
    lastHistoricalSyncCompletedAt = completedAt
    lastSyncAt = completedAt
    let detail = rangeOnly
      ? "Historical range poll complete"
      : sawHistoricalMetadata && historicalManager.historicalPacketsReceivedThisSync == 0
      ? "Historical metadata captured but no packet bodies received"
      : historicalManager.historicalPacketsReceivedThisSync == 0
      ? "No missed packets found"
      : "\(historicalManager.historicalPacketsReceivedThisSync) historical \(historicalManager.historicalPacketsReceivedThisSync == 1 ? "packet" : "packets") captured"
    publishSyncToast(phase: .synced, detail: detail, clearAfter: 2.2)
    notifyHistoricalSyncProgress(status: "synced", detail: detail, terminal: true, failed: false)
    record(source: "ble.sync", title: "historical_sync.completed", body: "reason=\(reason) \(detail)")
  }

  func failHistoricalSync(_ message: String) {
    historicalManager.historicalCommandTimeoutWorkItem?.cancel()
    historicalManager.historicalIdleWorkItem?.cancel()
    historicalManager.historicalRangeRetryWorkItem?.cancel()
    readySyncWorkItem?.cancel()
    historicalManager.pendingHistoricalCommand = nil
    historicalManager.historyEndAckQueued = false
    historicalManager.historyEndAckSentThisBurst = false
    historicalManager.pendingHistoryEndAckPayload = nil
    historicalManager.historyStartReceived = false
    historicalManager.historyEndReceived = false
    historicalManager.historyCompleteReceived = false
    historicalManager.historicalRangePendingResponses = 0
    historicalManager.historicalRangeRetryCount = 0
    historicalManager.historicalTransferRequestAttemptCount = 0
    historicalManager.historicalDataResultAckEnabled = true
    flushPendingHistoricalFramesIfNeeded(force: true)
    historicalManager.failSync(status: "failed")
    historicalManager.historicalRangePollOnly = false
    publishHistoricalPacketCountIfNeeded(force: true)
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
    let statusChanged = status != historicalManager.lastHistoricalSyncProgressCallbackStatus
      || (!highVolumePacketProgress && detail != historicalManager.lastHistoricalSyncProgressCallbackDetail)
    let elapsed = capturedAt.timeIntervalSince(historicalManager.lastHistoricalSyncProgressCallbackAt)
    let shouldPublish = terminal
      || failed
      || statusChanged
      || elapsed >= Self.historicalProgressCallbackInterval
    guard shouldPublish else {
      historicalManager.coalescedHistoricalSyncProgressCallbackCount &+= 1 // SYNC-02: wrapping add; long sync wraps instead of trapping
      return
    }

    let coalescedCount = historicalManager.coalescedHistoricalSyncProgressCallbackCount
    historicalManager.coalescedHistoricalSyncProgressCallbackCount = 0
    historicalManager.lastHistoricalSyncProgressCallbackAt = capturedAt
    historicalManager.lastHistoricalSyncProgressCallbackStatus = status
    historicalManager.lastHistoricalSyncProgressCallbackDetail = detail
    if coalescedCount > 0 {
      record(
        level: .debug,
        source: "ble.sync",
        title: "historical_sync.progress.coalesced",
        body: "count=\(coalescedCount) reason=callback_interval_\(Self.historicalProgressCallbackInterval)s packets=\(historicalManager.historicalPacketsReceivedThisSync) status=\(status)"
      )
    }

    onHistoricalSyncProgress?(
      GooseHistoricalSyncProgress(
        status: status,
        detail: detail,
        packetCount: historicalManager.historicalPacketsReceivedThisSync,
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
