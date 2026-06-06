import Foundation
import UIKit


extension GooseAppModel {
  func recoverUncleanOvernightGuardSessionIfNeeded() {
    guard !overnightGuardActive, overnightGuardSession == nil else { return }
    // Dispatch the slow file-system scan (contentsOfDirectory + JSONL line counts) to a
    // background queue; callback to main actor to apply state mutations.
    rustStartupQueue.async { [weak self] in
      guard let recovered = Self.latestRecoverableOvernightGuardSession() else { return }
      DispatchQueue.main.async { [weak self] in
        self?.applyUncleanSessionRecovery(recovered)
      }
    }
  }

  private func applyUncleanSessionRecovery(_ recovered: OvernightGuardRecoveredSession) {
    guard !overnightGuardActive, overnightGuardSession == nil else { return }
    do {
      let snapshot = try overnightRawSpool.resume(
        sessionID: recovered.id,
        directoryURL: recovered.directoryURL,
        metadata: [
          "active_device_name": ble.activeDeviceName,
          "active_device_id": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
          "connection_state": ble.connectionState,
          "database_path": HealthDataStore.defaultDatabasePath(),
          "resumed_at": Self.captureTimestampFormatter.string(from: Date()),
          "recovered_last_status_at": recovered.lastStatusAt
            .map { Self.captureTimestampFormatter.string(from: $0) } ?? NSNull(),
          "recovered_last_status_reason": recovered.lastStatusReason ?? NSNull(),
          "recovered_crash_marker_status": recovered.crashMarkerStatus ?? NSNull(),
          "recovered_raw_notification_count": recovered.notificationCount,
          "recovered_range_poll_response_count": recovered.historicalRangePollCount,
          "recovered_successful_range_poll_response_count": recovered.successfulHistoricalRangePollCount,
          "recovered_command_write_count": recovered.commandWriteCount,
          "roadmap": "docs/56-overnight-band-sync-roadmap.md",
        ],
        notificationCount: recovered.notificationCount,
        historicalRangePollCount: recovered.historicalRangePollCount,
        commandWriteCount: recovered.commandWriteCount,
        eventLogCount: recovered.eventLogCount,
        rawByteCount: recovered.rawByteCount,
        historicalRangePollByteCount: recovered.historicalRangePollByteCount,
        commandWriteByteCount: recovered.commandWriteByteCount,
        eventLogByteCount: recovered.eventLogByteCount,
        startedAt: recovered.startedAt,
        lastNotificationAt: recovered.lastNotificationAt
      )
      overnightGuardSession = OvernightGuardSession(
        id: recovered.id,
        startedAt: recovered.startedAt,
        directoryURL: recovered.directoryURL,
        rawNotificationsURL: recovered.rawNotificationsURL
      )
      overnightGuardActive = true
        overnightGuardFinalSyncPending = false
        overnightGuardFinalSyncDrainWorkItem?.cancel()
        overnightGuardFinalSyncDrainWorkItem = nil
        overnightGuardStartedHealthCapture = false
        overnightGuardWroteInitialRawNotificationStatus = false
        overnightGuardWroteInitialSQLiteMirrorStatus = false
        overnightGuardRawSpoolWarning = nil
        overnightGuardBLELogWarning = nil
        overnightGuardTargetCounts = recovered.targetCounts
        overnightGuardRawNotificationCount = snapshot.notificationCount
      overnightGuardRangeTelemetryCount = snapshot.historicalRangePollCount
      overnightGuardSuccessfulRangePollCount = recovered.successfulHistoricalRangePollCount
      overnightGuardCommandWriteCount = snapshot.commandWriteCount
      overnightGuardEventLogCount = snapshot.eventLogCount
      overnightGuardTargetSummary = overnightGuardTargetCounts.summary
      overnightGuardSpoolPath = recovered.rawNotificationsURL.path
      overnightGuardSpoolSizeSummary = Self.overnightSpoolSizeSummary(snapshot)
      applyOvernightSQLiteMirrorSnapshot(overnightSQLiteMirror.snapshot)
      overnightGuardLastPacketSummary = recovered.lastNotificationAt
        .map { "Resumed prior session | last raw \($0.formatted(date: .omitted, time: .standard))" }
        ?? "Resumed prior session started \(Self.captureTimestampFormatter.string(from: recovered.startedAt))"
      overnightGuardStatus = "Resumed overnight guard | raw \(snapshot.notificationCount)"
      overnightGuardExportStatus = "No overnight export"
      overnightGuardExportURL = nil
      overnightGuardExportManifestURL = nil
      overnightGuardExportManifestError = nil
      overnightGuardExportInProgress = false
      overnightGuardCanExportLastSession = false
      overnightGuardWarning = "Resumed previous overnight guard. Last heartbeat \(Self.overnightRecoveredStatusSummary(recovered)). Keep the official WHOOP app closed until Goose final sync/export finishes."
      refreshOvernightPowerState(reason: "resume", record: true)
      refreshOvernightReadiness(reason: "resume", record: true)
      enqueueOvernightSQLiteSession(finalStatus: "active", notes: "resumed_unclean_session")
      writeOvernightGuardStatus(reason: "resumed_unclean_session")
      scheduleOvernightGuardHeartbeat()
      resumeOvernightGuardStreamsIfReady(reason: "startup_recovery")
      ble.record(
        level: .warn,
        source: "overnight.guard",
        title: "resumed_unclean_session",
        body: "\(recovered.id) raw=\(snapshot.notificationCount) range_success=\(recovered.successfulHistoricalRangePollCount) last_status=\(Self.overnightRecoveredStatusSummary(recovered)) path=\(recovered.rawNotificationsURL.path)"
      )
    } catch {
      overnightGuardSession = OvernightGuardSession(
        id: recovered.id,
        startedAt: recovered.startedAt,
        directoryURL: recovered.directoryURL,
        rawNotificationsURL: recovered.rawNotificationsURL
      )
      overnightGuardRawNotificationCount = recovered.notificationCount
      overnightGuardRangeTelemetryCount = recovered.historicalRangePollCount
      overnightGuardSuccessfulRangePollCount = recovered.successfulHistoricalRangePollCount
      overnightGuardCommandWriteCount = recovered.commandWriteCount
      overnightGuardEventLogCount = recovered.eventLogCount
      overnightGuardTargetCounts = recovered.targetCounts
      overnightGuardTargetSummary = recovered.targetCounts.summary
      overnightGuardSpoolPath = recovered.rawNotificationsURL.path
      overnightGuardSpoolSizeSummary = Self.overnightRecoveredSpoolSizeSummary(recovered)
      applyOvernightSQLiteMirrorSnapshot(overnightSQLiteMirror.snapshot)
      overnightGuardLastPacketSummary = "Recovered prior session | last heartbeat \(Self.overnightRecoveredStatusSummary(recovered))"
      overnightGuardStatus = "Recovered unclean overnight guard | raw \(recovered.notificationCount)"
      overnightGuardExportStatus = "Recovered session ready to export"
      overnightGuardExportManifestURL = nil
      overnightGuardExportManifestError = nil
      overnightGuardCanExportLastSession = true
      overnightGuardWarning = "Previous overnight guard did not close cleanly; last heartbeat \(Self.overnightRecoveredStatusSummary(recovered)). Export Goose before opening the official WHOOP app."
      refreshOvernightReadiness(reason: "resume_failed", record: true)
      ble.record(
        level: .error,
        source: "overnight.guard",
        title: "resume_unclean_session.failed",
        body: "\(recovered.id) \(String(describing: error)) path=\(recovered.rawNotificationsURL.path)"
      )
    }
  }

  func resumeOvernightGuardStreamsIfReady(reason: String) {
    guard overnightGuardActive, ble.connectionState == "ready" else {
      refreshOvernightReadiness(reason: "resume_waiting_for_ready")
      writeOvernightGuardStatus(reason: "resume_waiting_for_ready")
      return
    }
    if !overnightGuardStartedHealthCapture {
      if activeHealthPacketCapture == nil {
        startPhysiologyPacketCapture(duration: Self.overnightGuardDuration, source: "overnight_guard_resume")
      } else {
        ble.startPhysiologySignalCapture()
      }
      overnightGuardStartedHealthCapture = true
      ble.record(source: "overnight.guard", title: "resume.streams.started", body: reason)
    }
    if overnightGuardRangePollWorkItem == nil, !overnightGuardFinalSyncPending {
      scheduleOvernightGuardRangePoll(after: 8, reason: "resume_\(reason)")
    }
    refreshOvernightReadiness(reason: "resume_streams_ready_\(reason)", record: true)
    writeOvernightGuardStatus(reason: "resume_streams_ready_\(reason)")
  }

  nonisolated static func latestRecoverableOvernightGuardSession() -> OvernightGuardRecoveredSession? {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    let rootURL = overnightGuardRootDirectoryURL()
    let fileManager = FileManager.default
    guard let sessions = try? fileManager.contentsOfDirectory(
      at: rootURL,
      includingPropertiesForKeys: [.isDirectoryKey, .contentModificationDateKey],
      options: [.skipsHiddenFiles]
    ) else {
      return nil
    }

    let recovered = sessions.compactMap { sessionURL -> OvernightGuardRecoveredSession? in
      guard (try? sessionURL.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true else {
        return nil
      }
      let manifestURL = sessionURL.appendingPathComponent("manifest.json")
      let statusURL = sessionURL.appendingPathComponent("status.txt")
      let crashMarkerURL = sessionURL.appendingPathComponent("crash-marker.json")
      let rawURL = sessionURL.appendingPathComponent("raw-notifications.jsonl")
      let rangePollsURL = sessionURL.appendingPathComponent("historical-range-polls.jsonl")
      let commandWritesURL = sessionURL.appendingPathComponent("command-writes.jsonl")
      let eventLogURL = sessionURL.appendingPathComponent("event-log.jsonl")
      guard fileManager.fileExists(atPath: manifestURL.path),
            fileManager.fileExists(atPath: statusURL.path),
            fileManager.fileExists(atPath: rawURL.path),
            let manifest = readJSONObject(at: manifestURL) else {
        return nil
      }
      guard (manifest["status"] as? String) == "active" else {
        return nil
      }

      let statusValues = readStatusValues(at: statusURL)
      let crashMarker = readJSONObject(at: crashMarkerURL)
      let notificationCount = countJSONLRecords(at: rawURL)
        ?? overnightGuardIntValue(statusValues["notification_count"])
        ?? overnightGuardIntValue(manifest["notification_count"])
        ?? overnightGuardIntValue(crashMarker?["notification_count"])
        ?? 0
      let historicalRangePollCount = countJSONLRecords(at: rangePollsURL)
        ?? overnightGuardIntValue(statusValues["historical_range_poll_count"])
        ?? overnightGuardIntValue(manifest["historical_range_poll_count"])
        ?? overnightGuardIntValue(crashMarker?["historical_range_poll_count"])
        ?? 0
      let commandWriteCount = countJSONLRecords(at: commandWritesURL)
        ?? overnightGuardIntValue(statusValues["command_write_count"])
        ?? overnightGuardIntValue(manifest["command_write_count"])
        ?? 0
      let successfulHistoricalRangePollCount = overnightGuardIntValue(statusValues["successful_historical_range_poll_count"])
        ?? overnightGuardIntValue(statusValues["successful_range_poll_responses"])
        ?? countSuccessfulHistoricalRangePolls(at: rangePollsURL)
      let targetCounts = overnightGuardTargetCounts(from: statusValues["targets"])
      let eventLogCount = countJSONLRecords(at: eventLogURL)
        ?? overnightGuardIntValue(statusValues["event_log_count"])
        ?? overnightGuardIntValue(manifest["event_log_count"])
        ?? 0
      let startedAt = (manifest["started_at"] as? String).flatMap { formatter.date(from: $0) }
        ?? (try? sessionURL.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
        ?? Date()
      let lastNotificationAt = (statusValues["last_notification_at"])
        .flatMap { $0 == "none" ? nil : formatter.date(from: $0) }
        ?? (manifest["last_notification_at"] as? String).flatMap { formatter.date(from: $0) }
      let lastStatusAt = (statusValues["heartbeat_at"] ?? statusValues["timestamp"])
        .flatMap { formatter.date(from: $0) }
        ?? (crashMarker?["last_status_at"] as? String).flatMap { formatter.date(from: $0) }
        ?? (manifest["last_status_at"] as? String).flatMap { formatter.date(from: $0) }
      let modifiedAt = (try? statusURL.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
        ?? (try? manifestURL.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
        ?? startedAt

      return OvernightGuardRecoveredSession(
        id: manifest["session_id"] as? String ?? sessionURL.lastPathComponent,
        startedAt: startedAt,
        modifiedAt: modifiedAt,
        notificationCount: notificationCount,
        historicalRangePollCount: historicalRangePollCount,
        successfulHistoricalRangePollCount: successfulHistoricalRangePollCount,
        commandWriteCount: commandWriteCount,
        eventLogCount: eventLogCount,
        lastNotificationAt: lastNotificationAt,
        lastStatusAt: lastStatusAt,
        lastStatusReason: statusValues["reason"] ?? (crashMarker?["reason"] as? String),
        crashMarkerStatus: crashMarker?["status"] as? String,
        targetCounts: targetCounts,
        rawByteCount: fileSize(at: rawURL),
        historicalRangePollByteCount: fileSize(at: rangePollsURL),
        commandWriteByteCount: fileSize(at: commandWritesURL),
        eventLogByteCount: fileSize(at: eventLogURL),
        directoryURL: sessionURL,
        rawNotificationsURL: rawURL,
        crashMarkerURL: fileManager.fileExists(atPath: crashMarkerURL.path) ? crashMarkerURL : nil
      )
    }

    return recovered.sorted { lhs, rhs in
      lhs.modifiedAt > rhs.modifiedAt
    }.first
  }

  nonisolated static func readJSONObject(at url: URL) -> [String: Any]? {
    guard let data = try? Data(contentsOf: url),
          let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
      return nil
    }
    return object
  }

  nonisolated static func fileSize(at url: URL) -> Int {
    let byteCount = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
    return max(byteCount, 0)
  }

  nonisolated static func readStatusValues(at url: URL) -> [String: String] {
    guard let text = try? String(contentsOf: url, encoding: .utf8) else {
      return [:]
    }
    var values: [String: String] = [:]
    for line in text.split(separator: "\n") {
      guard let separator = line.firstIndex(of: "=") else {
        continue
      }
      let key = String(line[..<separator])
      let value = String(line[line.index(after: separator)...])
      values[key] = value
    }
    return values
  }

  nonisolated static func countSuccessfulHistoricalRangePolls(at url: URL) -> Int {
    guard let text = try? String(contentsOf: url, encoding: .utf8) else {
      return 0
    }
    var count = 0
    for line in text.split(separator: "\n") {
      guard let data = String(line).data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            (object["status"] as? String) == "success" else {
        continue
      }
      count += 1
    }
    return count
  }

  nonisolated static func countJSONLRecords(at url: URL) -> Int? {
    guard let handle = try? FileHandle(forReadingFrom: url) else {
      return nil
    }
    defer {
      try? handle.close()
    }

    var recordCount = 0
    var hasBytesInCurrentLine = false
    while true {
      let chunk: Data
      do {
        chunk = try handle.read(upToCount: 64 * 1024) ?? Data()
      } catch {
        return nil
      }
      if chunk.isEmpty {
        break
      }
      for byte in chunk {
        if byte == 0x0a {
          if hasBytesInCurrentLine {
            recordCount += 1
            hasBytesInCurrentLine = false
          }
        } else if byte != 0x0d {
          hasBytesInCurrentLine = true
        }
      }
    }
    if hasBytesInCurrentLine {
      recordCount += 1
    }
    return recordCount
  }

  static func finalizeRecoveredOvernightGuardSessionForExport(
    sessionID: String,
    summary: [String: Any]
  ) throws {
    let directoryURL = overnightGuardDirectoryURL(sessionID: sessionID)
    let manifestURL = directoryURL.appendingPathComponent("manifest.json")
    let statusURL = directoryURL.appendingPathComponent("status.txt")
    guard var manifest = readJSONObject(at: manifestURL) else {
      throw NSError(
        domain: "GooseOvernightGuard",
        code: 1,
        userInfo: [NSLocalizedDescriptionKey: "missing or invalid recovered manifest"]
      )
    }

    let statusValues = readStatusValues(at: statusURL)
    let crashMarkerURL = directoryURL.appendingPathComponent("crash-marker.json")
    let crashMarker = readJSONObject(at: crashMarkerURL)
    let existingStatus = manifest["status"] as? String ?? "unknown"
    let proofAlreadyFinalized = existingStatus != "active"
      && manifest["summary"] != nil
      && overnightGuardBoolValue(manifest["handles_closed"]) == true
      && overnightGuardBoolValue(manifest["post_close_status_refresh"]) == true
      && overnightGuardBoolValue(statusValues["active"]) == false
      && overnightGuardBoolValue(statusValues["handles_closed"]) == true
      && overnightGuardBoolValue(statusValues["post_close_status_refresh"]) == true
      && overnightGuardBoolValue(crashMarker?["active"]) == false
      && overnightGuardBoolValue(crashMarker?["handles_closed"]) == true
      && overnightGuardBoolValue(crashMarker?["post_close_status_refresh"]) == true
    if proofAlreadyFinalized {
      return
    }

    let now = captureTimestampFormatter.string(from: Date())
    let rawByteCount = fileSize(at: directoryURL.appendingPathComponent("raw-notifications.jsonl"))
    let rangeByteCount = fileSize(at: directoryURL.appendingPathComponent("historical-range-polls.jsonl"))
    let commandWriteByteCount = fileSize(at: directoryURL.appendingPathComponent("command-writes.jsonl"))
    let eventByteCount = fileSize(at: directoryURL.appendingPathComponent("event-log.jsonl"))
    let totalByteCount = rawByteCount + rangeByteCount + commandWriteByteCount + eventByteCount
    let rawRecordCount = countJSONLRecords(at: directoryURL.appendingPathComponent("raw-notifications.jsonl"))
    let rangeRecordCount = countJSONLRecords(at: directoryURL.appendingPathComponent("historical-range-polls.jsonl"))
    let commandWriteRecordCount = countJSONLRecords(at: directoryURL.appendingPathComponent("command-writes.jsonl"))
    let eventRecordCount = countJSONLRecords(at: directoryURL.appendingPathComponent("event-log.jsonl"))
    var recoveredSummary = (manifest["summary"] as? [String: Any]) ?? [:]
    summary.forEach { recoveredSummary[$0.key] = $0.value }
    recoveredSummary["reason"] = "recovered_export"
    recoveredSummary["recovered_from_status"] = existingStatus
    recoveredSummary["recovered_at"] = now
    recoveredSummary["last_status_at_before_recovery"] = statusValues["heartbeat_at"]
      ?? statusValues["timestamp"]
      ?? crashMarker?["last_status_at"]
      ?? NSNull()
    recoveredSummary["last_status_reason_before_recovery"] = statusValues["reason"]
      ?? crashMarker?["reason"]
      ?? NSNull()
    recoveredSummary["crash_marker_status_before_recovery"] = crashMarker?["status"] ?? NSNull()
    recoveredSummary["raw_byte_count"] = rawByteCount
    recoveredSummary["historical_range_poll_byte_count"] = rangeByteCount
    recoveredSummary["command_write_byte_count"] = commandWriteByteCount
    recoveredSummary["event_log_byte_count"] = eventByteCount
    recoveredSummary["total_byte_count"] = totalByteCount
    recoveredSummary["handles_closed"] = true
    recoveredSummary["post_close_status_refresh"] = true
    recoveredSummary["post_close_status_refreshed_at"] = now
    let recoveredRawRecordCount = rawRecordCount
      ?? overnightGuardIntValue(statusValues["notification_count"])
      ?? overnightGuardIntValue(manifest["notification_count"])
      ?? 0
    let recoveredRangeRecordCount = rangeRecordCount
      ?? overnightGuardIntValue(statusValues["historical_range_poll_count"])
      ?? overnightGuardIntValue(manifest["historical_range_poll_count"])
      ?? 0
    let recoveredCommandWriteCount = commandWriteRecordCount
      ?? overnightGuardIntValue(statusValues["command_write_count"])
      ?? overnightGuardIntValue(manifest["command_write_count"])
      ?? 0
    let recoveredEventRecordCount = eventRecordCount
      ?? overnightGuardIntValue(statusValues["event_log_count"])
      ?? overnightGuardIntValue(manifest["event_log_count"])
      ?? 0
    recoveredSummary["raw_notification_count"] = recoveredRawRecordCount
    recoveredSummary["range_poll_response_count"] = recoveredRangeRecordCount
    recoveredSummary["command_write_count"] = recoveredCommandWriteCount
    recoveredSummary["event_log_count"] = recoveredEventRecordCount
    recoveredSummary["file_metrics_recomputed_at"] = now
    let recoveredSuccessfulRangeCount = overnightGuardIntValue(statusValues["successful_historical_range_poll_count"])
      ?? overnightGuardIntValue(statusValues["successful_range_poll_responses"])
      ?? countSuccessfulHistoricalRangePolls(at: directoryURL.appendingPathComponent("historical-range-polls.jsonl"))
    recoveredSummary["successful_range_poll_response_count"] = recoveredSuccessfulRangeCount
    recoveredSummary["successful_historical_range_poll_count"] = recoveredSuccessfulRangeCount
    let readinessStatusText = (recoveredSummary["readiness_status"] as? String)
      ?? statusValues["readiness_status"]
      ?? "pending"
    let readinessText = (recoveredSummary["readiness"] as? String)
      ?? statusValues["readiness"]
      ?? "Recovered unclean overnight guard | export before opening WHOOP"
    recoveredSummary["readiness_status"] = readinessStatusText
    recoveredSummary["readiness"] = readinessText
    recoveredSummary["warning"] = "Recovered unclean overnight guard. Export before opening the official WHOOP app."

    manifest["status"] = "recovered_export"
    manifest["ended_at"] = now
    manifest["recovered_at"] = now
    manifest["last_status_at"] = now
    manifest["handles_closed"] = true
    manifest["post_close_status_refresh"] = true
    manifest["post_close_status_refreshed_at"] = now
    manifest["summary"] = recoveredSummary
    let manifestData = try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys])
    try writeProtectedOvernightSidecar(manifestData, to: manifestURL)

    let recoveredRawCount = recoveredRawRecordCount
    let recoveredRangeCount = recoveredRangeRecordCount
    let recoveredCommandWriteCountText = String(recoveredCommandWriteCount)
    let recoveredEventCount = recoveredEventRecordCount
    let notificationCountText = String(recoveredRawCount)
    let rangePollCountText = String(recoveredRangeCount)
    let successfulRangePollCountText = statusValues["successful_historical_range_poll_count"]
      ?? statusValues["successful_range_poll_responses"]
      ?? String(recoveredSuccessfulRangeCount)
    let eventLogCountText = String(recoveredEventCount)
    let statusLines = [
      "timestamp=\(now)",
      "heartbeat_at=\(now)",
      "session_id=\(sessionID)",
      "status=recovered_export",
      "active=false",
      "reason=recovered_export",
      "last_status_at_before_recovery=\(statusValues["heartbeat_at"] ?? statusValues["timestamp"] ?? "none")",
      "last_status_reason_before_recovery=\(statusValues["reason"] ?? "none")",
      "readiness_status=\(readinessStatusText)",
      "readiness=\(readinessText)",
      "notification_count=\(notificationCountText)",
      "historical_range_poll_count=\(rangePollCountText)",
      "successful_historical_range_poll_count=\(successfulRangePollCountText)",
      "command_write_count=\(recoveredCommandWriteCountText)",
      "event_log_count=\(eventLogCountText)",
      "raw_byte_count=\(rawByteCount)",
      "historical_range_poll_byte_count=\(rangeByteCount)",
      "command_write_byte_count=\(commandWriteByteCount)",
      "event_log_byte_count=\(eventByteCount)",
      "total_byte_count=\(totalByteCount)",
      "file_metrics_recomputed_at=\(now)",
      "handles_closed=true",
      "post_close_status_refresh=true",
      "post_close_status_refreshed_at=\(now)",
      "raw_notifications=\(directoryURL.appendingPathComponent("raw-notifications.jsonl").path)",
      "historical_range_polls=\(directoryURL.appendingPathComponent("historical-range-polls.jsonl").path)",
      "command_writes=\(directoryURL.appendingPathComponent("command-writes.jsonl").path)",
      "event_log=\(directoryURL.appendingPathComponent("event-log.jsonl").path)",
      "crash_marker=\(crashMarkerURL.path)",
	      "raw_notification_checksum_algorithm=\(OvernightRawNotificationStorageClassifier.checksumAlgorithm)",
	      "historical_range_checksum_algorithm=sha256(raw_payload_hex/raw_body_hex)",
	      "command_write_checksum_algorithm=sha256(payload_hex/frame_hex)",
	      "last_notification_at=\(statusValues["last_notification_at"] ?? "none")",
	      "last_error=\(statusValues["last_error"] ?? "none")",
	      "raw_spool_warning=\(statusValues["raw_spool_warning"] ?? "none")",
	      "ble_log_warning=\(statusValues["ble_log_warning"] ?? "none")",
	      "export_manifest_error=\(statusValues["export_manifest_error"] ?? "none")",
	      "warning=Recovered unclean overnight guard. Export before opening the official WHOOP app.",
	    ]
    let statusData = Data(statusLines.joined(separator: "\n").appending("\n").utf8)
    try writeProtectedOvernightSidecar(statusData, to: statusURL)
    let lastStatusAtBeforeRecovery: Any
    if let recoveredStatusAt = statusValues["heartbeat_at"] ?? statusValues["timestamp"] {
      lastStatusAtBeforeRecovery = recoveredStatusAt
    } else {
      lastStatusAtBeforeRecovery = NSNull()
    }
    let lastStatusReasonBeforeRecovery: Any
    if let recoveredReason = statusValues["reason"] {
      lastStatusReasonBeforeRecovery = recoveredReason
    } else {
      lastStatusReasonBeforeRecovery = NSNull()
    }
    let recoveredMarker: [String: Any] = [
      "schema": "goose.overnight.crash_marker.v1",
      "session_id": sessionID,
      "active": false,
      "status": "recovered_export",
      "reason": "recovered_export",
      "last_status_at": now,
      "recovered_at": now,
      "last_status_at_before_recovery": lastStatusAtBeforeRecovery,
      "last_status_reason_before_recovery": lastStatusReasonBeforeRecovery,
      "notification_count": recoveredRawCount,
      "historical_range_poll_count": recoveredRangeCount,
      "command_write_count": recoveredCommandWriteCount,
      "event_log_count": recoveredEventCount,
      "raw_byte_count": rawByteCount,
      "historical_range_poll_byte_count": rangeByteCount,
      "command_write_byte_count": commandWriteByteCount,
      "event_log_byte_count": eventByteCount,
      "total_byte_count": totalByteCount,
      "file_metrics_recomputed_at": now,
      "raw_notification_checksum_algorithm": OvernightRawNotificationStorageClassifier.checksumAlgorithm,
      "historical_range_checksum_algorithm": "sha256(raw_payload_hex/raw_body_hex)",
	      "command_write_checksum_algorithm": "sha256(payload_hex/frame_hex)",
	      "last_error": statusValues["last_error"] ?? NSNull(),
	      "raw_spool_warning": statusValues["raw_spool_warning"] ?? NSNull(),
	      "ble_log_warning": statusValues["ble_log_warning"] ?? NSNull(),
	      "export_manifest_error": statusValues["export_manifest_error"] ?? NSNull(),
	      "handles_closed": true,
	      "post_close_status_refresh": true,
	      "post_close_status_refreshed_at": now,
    ]
    let markerData = try JSONSerialization.data(withJSONObject: recoveredMarker, options: [.prettyPrinted, .sortedKeys])
    try writeProtectedOvernightSidecar(markerData, to: crashMarkerURL)
  }

  static func writeProtectedOvernightSidecar(_ data: Data, to url: URL) throws {
    try data.write(to: url, options: .atomic)
    try FileManager.default.setAttributes(
      [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
      ofItemAtPath: url.path
    )

    let handle = try FileHandle(forUpdating: url)
    var fileError: Error?
    do {
      try handle.synchronize()
    } catch {
      fileError = error
    }
    do {
      try handle.close()
    } catch {
      if fileError == nil {
        fileError = error
      }
    }
    if let fileError {
      throw fileError
    }
  }

  nonisolated static func overnightGuardIntValue(_ value: Any?) -> Int? {
    if let value = value as? Int {
      return value
    }
    if let value = value as? Double {
      return Int(value)
    }
    if let value = value as? String {
      return Int(value)
    }
    return nil
  }

  static func overnightGuardBoolValue(_ value: Any?) -> Bool? {
    if let value = value as? Bool {
      return value
    }
    if let value = value as? Int {
      return value != 0
    }
    if let value = value as? String {
      switch value.lowercased() {
      case "true", "1", "yes":
        return true
      case "false", "0", "no":
        return false
      default:
        return nil
      }
    }
    return nil
  }

  nonisolated static func overnightGuardTargetCounts(from summary: String?) -> OvernightGuardTargetCounts {
    var counts = OvernightGuardTargetCounts()
    guard let summary else {
      return counts
    }
    for part in summary.split(separator: "|") {
      let pieces = part.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: " ")
      guard pieces.count >= 2, let value = Int(pieces[1]) else {
        continue
      }
      switch pieces[0] {
      case "K18":
        counts.k18 = value
      case "K24":
        counts.k24 = value
      case "K25":
        counts.k25 = value
      case "K26":
        counts.k26 = value
      case "packet47":
        counts.packet47 = value
      case "event17":
        counts.event17 = value
      case "event29":
        counts.event29 = value
      case "metadata49", "event49":
        counts.metadata49 = value
      case "metadata56", "event56":
        counts.metadata56 = value
      default:
        continue
      }
    }
    return counts
  }

  static func currentOvernightPowerState() -> OvernightPowerState {
    let device = UIDevice.current
    device.isBatteryMonitoringEnabled = true
    let rawBatteryLevel = device.batteryLevel
    let batteryPercent = rawBatteryLevel >= 0 ? Int((rawBatteryLevel * 100).rounded()) : nil
    return OvernightPowerState(
      lowPowerMode: ProcessInfo.processInfo.isLowPowerModeEnabled,
      batteryPercent: batteryPercent,
      batteryState: batteryStateDescription(device.batteryState),
      thermalState: thermalStateDescription(ProcessInfo.processInfo.thermalState)
    )
  }

  static func batteryStateDescription(_ state: UIDevice.BatteryState) -> String {
    switch state {
    case .unknown:
      return "unknown"
    case .unplugged:
      return "unplugged"
    case .charging:
      return "charging"
    case .full:
      return "full"
    @unknown default:
      return "unknown"
    }
  }

  static func thermalStateDescription(_ state: ProcessInfo.ThermalState) -> String {
    switch state {
    case .nominal:
      return "nominal"
    case .fair:
      return "fair"
    case .serious:
      return "serious"
    case .critical:
      return "critical"
    @unknown default:
      return "unknown"
    }
  }
}
