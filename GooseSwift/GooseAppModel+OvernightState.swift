import Foundation
import UIKit


extension GooseAppModel {
  @discardableResult
  func refreshOvernightPowerState(reason: String, record: Bool = false) -> OvernightPowerState {
    let power = Self.currentOvernightPowerState()
    applyOvernightPowerState(power)
    if record {
      ble.record(source: "overnight.guard", title: "power_state", body: "reason=\(reason) \(power.summary)")
    }
    return power
  }

  func applyOvernightPowerState(_ power: OvernightPowerState) {
    overnightGuardPowerSummary = power.summary
    var warnings: [String] = []
    if power.lowPowerMode {
      warnings.append("Turn Low Power Mode off for overnight capture.")
    }
    if power.batteryState == "unplugged", let percent = power.batteryPercent, percent < 40 {
      warnings.append("Plug the phone into power before sleeping.")
    }
    if power.thermalState == "serious" || power.thermalState == "critical" {
      warnings.append("Phone thermal state is \(power.thermalState); cooling it improves background reliability.")
    }
    overnightGuardPowerWarning = warnings.isEmpty ? nil : warnings.joined(separator: " ")
    updateOvernightGuardWarning()
  }

  func updateOvernightGuardWarning() {
    var warnings = ["Keep the official WHOOP app closed until Goose final sync/export finishes."]
    if let overnightGuardPowerWarning {
      warnings.append(overnightGuardPowerWarning)
    }
    if let overnightGuardWatchdogWarning {
      warnings.append(overnightGuardWatchdogWarning)
    }
    if let overnightGuardRawSpoolWarning {
      warnings.append(overnightGuardRawSpoolWarning)
    }
    if let overnightGuardBLELogWarning {
      warnings.append(overnightGuardBLELogWarning)
    }
    if let overnightGuardSQLiteMirrorWarning {
      warnings.append(overnightGuardSQLiteMirrorWarning)
    }
    overnightGuardWarning = warnings.joined(separator: " ")
  }

  func refreshOvernightReadiness(reason: String, record: Bool = false) {
    let evaluation = overnightGuardReadinessEvaluation()
    let statusChanged = evaluation.status != overnightGuardReadinessStatus
    overnightGuardReadinessStatus = evaluation.status
    overnightGuardReadinessSummary = evaluation.summary
    if record, statusChanged {
      ble.record(
        source: "overnight.guard",
        title: "readiness.\(evaluation.status)",
        body: "reason=\(reason) \(evaluation.summary)"
      )
    }
  }

  func applyOvernightSQLiteMirrorSnapshot(
    _ snapshot: OvernightSQLiteMirrorSnapshot,
    reason: String = "sqlite_mirror",
    writeSidecars: Bool = false,
    forceSidecarsAfterFlush: Bool = false
  ) {
    overnightGuardSQLiteMirrorSummary = snapshot.summary
    if let lastError = snapshot.lastError {
      overnightGuardSQLiteMirrorWarning = "SQLite mirror warning: \(lastError). JSONL raw spool is still primary."
    } else {
      overnightGuardSQLiteMirrorWarning = nil
    }
    updateOvernightGuardWarning()
    refreshOvernightReadiness(reason: reason)
    let committedRawMirrorCount = snapshot.rawInserted + snapshot.rawExisting
    let committedRangeMirrorCount = snapshot.historicalRangeInserted + snapshot.historicalRangeExisting
    let hasCommittedMirrorEvidence = snapshot.queuedRows == 0
      && (committedRawMirrorCount > 0 || committedRangeMirrorCount > 0)
    let shouldWriteInitialMirrorProof = !overnightGuardWroteInitialSQLiteMirrorStatus && hasCommittedMirrorEvidence
    let shouldWriteForcedMirrorProof = forceSidecarsAfterFlush && hasCommittedMirrorEvidence
    let shouldWriteMirrorWarning = snapshot.lastError != nil
    if writeSidecars
      && (overnightGuardActive || overnightGuardSession != nil)
      && (shouldWriteInitialMirrorProof || shouldWriteForcedMirrorProof || shouldWriteMirrorWarning) {
      if shouldWriteInitialMirrorProof {
        overnightGuardWroteInitialSQLiteMirrorStatus = true
      }
      writeOvernightGuardStatus(reason: reason)
    }
  }

  func enqueueOvernightSQLiteSession(finalStatus: String, endedAt: Date? = nil, notes: String? = nil) {
    guard overnightGuardSession != nil else {
      return
    }
    overnightSQLiteMirror.enqueueSession(
      overnightSQLiteSessionRow(finalStatus: finalStatus, endedAt: endedAt, notes: notes)
    ) { [weak self] snapshot in
      self?.applyOvernightSQLiteMirrorSnapshot(snapshot)
    }
  }

  func overnightSQLiteSessionRow(finalStatus: String, endedAt: Date? = nil, notes: String? = nil) -> [String: Any] {
    guard let session = overnightGuardSession else {
      return [:]
    }
    let errorCount = [overnightGuardRawSpoolWarning, overnightGuardBLELogWarning, overnightGuardSQLiteMirrorWarning]
      .compactMap { $0 }
      .count
    return [
      "session_id": session.id,
      "started_at": Self.captureTimestampFormatter.string(from: session.startedAt),
      "ended_at": endedAt.map { Self.captureTimestampFormatter.string(from: $0) } ?? NSNull(),
      "band_identifier": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
      "app_version": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown",
      "mode": "overnight_guard",
      "final_status": finalStatus,
      "raw_frame_count": overnightGuardRawNotificationCount,
      "historical_frame_count": ble.historicalPacketCount,
      "k18_count": overnightGuardTargetCounts.k18,
      "k24_count": overnightGuardTargetCounts.k24,
      "k25_count": overnightGuardTargetCounts.k25,
      "k26_count": overnightGuardTargetCounts.k26,
      "packet47_count": overnightGuardTargetCounts.packet47,
      "event17_count": overnightGuardTargetCounts.event17,
      "event29_count": overnightGuardTargetCounts.event29,
      "metadata49_count": overnightGuardTargetCounts.metadata49,
      "metadata56_count": overnightGuardTargetCounts.metadata56,
      "range_poll_count": overnightGuardRangeTelemetryCount,
      "successful_range_poll_count": overnightGuardSuccessfulRangePollCount,
      "event_log_count": overnightGuardEventLogCount,
      "readiness_status": overnightGuardReadinessStatus,
      "readiness": overnightGuardReadinessSummary,
      "error_count": errorCount,
      "notes": notes ?? overnightGuardWarning,
    ]
  }

  func overnightGuardReadinessEvaluation() -> (status: String, summary: String) {
    var inactiveWarnings: [String] = []
    if overnightGuardRawSpoolWarning != nil {
      inactiveWarnings.append("raw spool")
    }
    if overnightGuardSQLiteMirrorWarning != nil {
      inactiveWarnings.append("sqlite mirror")
    }
    if overnightGuardBLELogWarning != nil {
      inactiveWarnings.append("BLE log")
    }
    if !overnightGuardActive {
      if overnightGuardExportURL != nil {
        if overnightGuardExportStatus.localizedCaseInsensitiveContains("validation issues")
          || overnightGuardExportStatus.localizedCaseInsensitiveContains("failed")
          || overnightGuardExportStatus.localizedCaseInsensitiveContains("missing") {
          return ("stale", "Final bundle saved with validation issues | inspect before opening WHOOP")
        }
        if !inactiveWarnings.isEmpty {
          return ("stale", "Final bundle saved with proof warning | \(inactiveWarnings.joined(separator: ", ")) | inspect before opening WHOOP")
        }
        return ("ready", "Final bundle ready | AirDrop before opening WHOOP")
      }
      if overnightGuardCanExportLastSession {
        if !inactiveWarnings.isEmpty {
          return ("stale", "Guard stopped with proof warning | \(inactiveWarnings.joined(separator: ", ")) | export and inspect before opening WHOOP")
        }
        return ("pending", "Guard stopped or recovered | export before opening WHOOP")
      }
      if ble.connectionState != "ready" {
        return ("blocked", "Not sleep-ready | connect WHOOP (\(ble.connectionState)) and start Overnight Guard")
      }
      return ("pending", "Not sleep-ready | start Overnight Guard")
    }

    let evidence = "raw \(overnightGuardRawNotificationCount) | command writes \(overnightGuardCommandWriteCount) | range success \(overnightGuardSuccessfulRangePollCount) / responses \(overnightGuardRangeTelemetryCount) | events \(overnightGuardEventLogCount) | \(overnightGuardSpoolSizeSummary)"
    var blockers: [String] = []
    var waiting: [String] = []
    var warnings: [String] = []

    if ble.connectionState != "ready" {
      blockers.append("connection \(ble.connectionState)")
    }
    if overnightGuardSpoolPath == "No overnight spool" {
      blockers.append("spool missing")
    }
    if overnightGuardRawNotificationCount == 0 {
      waiting.append("raw notifications 0")
    }
    if overnightGuardCommandWriteCount == 0 {
      waiting.append("command writes 0")
    }
    if overnightGuardSuccessfulRangePollCount == 0 {
      waiting.append("GET_DATA_RANGE success 0")
    }
    if overnightGuardEventLogCount == 0 {
      waiting.append("event log 0")
    }
    if overnightGuardPowerWarning != nil {
      warnings.append("power")
    }
    if overnightGuardWatchdogWarning != nil {
      warnings.append("watchdog")
    }
    if overnightGuardRawSpoolWarning != nil {
      warnings.append("raw spool")
    }
    if overnightGuardSQLiteMirrorWarning != nil {
      warnings.append("sqlite mirror")
    }
    if overnightGuardBLELogWarning != nil {
      warnings.append("BLE log")
    }

    if !blockers.isEmpty {
      return ("blocked", "Not sleep-ready | \(blockers.joined(separator: ", ")) | \(evidence)")
    }
    if !waiting.isEmpty {
      return ("pending", "Waiting for sleep-ready evidence | \(waiting.joined(separator: ", ")) | \(evidence)")
    }
    if !warnings.isEmpty {
      return ("stale", "Capture running with warning | \(warnings.joined(separator: ", ")) | \(evidence)")
    }
    return ("ready", "Sleep-ready | \(evidence)")
  }

  func refreshOvernightWatchdogState(reason: String) {
    guard overnightGuardActive, let startedAt = overnightGuardSession?.startedAt else {
      return
    }

    let now = Date()
    let elapsed = now.timeIntervalSince(startedAt)
    let snapshot = overnightRawSpool.snapshot
    var warnings: [String] = []

    if snapshot.notificationCount == 0, elapsed >= Self.overnightGuardRawStaleWarningInterval {
      let minutes = Int((elapsed / 60).rounded())
      let warning = "No raw BLE notifications after \(minutes)m; confirm WHOOP is connected and Goose is foregrounded if possible."
      warnings.append(warning)
      recordOvernightWatchdogWarningIfNeeded(
        title: "raw_notifications.none",
        body: warning,
        lastLoggedAt: &overnightGuardLastRawStaleWarningAt,
        now: now
      )
    } else if let lastNotificationAt = snapshot.lastNotificationAt {
      let gap = now.timeIntervalSince(lastNotificationAt)
      if gap >= Self.overnightGuardRawStaleWarningInterval {
        let minutes = Int((gap / 60).rounded())
        let warning = "No raw BLE notifications for \(minutes)m; watch for iOS suspension or band disconnect."
        warnings.append(warning)
        recordOvernightWatchdogWarningIfNeeded(
          title: "raw_notifications.stale",
          body: warning,
          lastLoggedAt: &overnightGuardLastRawStaleWarningAt,
          now: now
        )
      }
    }

    if elapsed >= Self.overnightGuardRangeSuccessWarningDelay, overnightGuardSuccessfulRangePollCount == 0 {
      let minutes = Int((elapsed / 60).rounded())
      let warning = "No successful GET_DATA_RANGE after \(minutes)m; Range Polls is not sleep-ready until success > 0."
      warnings.append(warning)
      recordOvernightWatchdogWarningIfNeeded(
        title: "historical_range.success_missing",
        body: warning,
        lastLoggedAt: &overnightGuardLastRangeSuccessWarningAt,
        now: now
      )
    }

    if elapsed >= Self.overnightGuardTargetMissingWarningDelay, !overnightGuardTargetCounts.hasPhysiologyTargets {
      let minutes = Int((elapsed / 60).rounded())
      let warning = "No \(OvernightGuardTargetCounts.targetFamilyList) target packets after \(minutes)m; continuing raw capture."
      warnings.append(warning)
      recordOvernightWatchdogWarningIfNeeded(
        title: "target_packets.missing",
        body: warning,
        lastLoggedAt: &overnightGuardLastTargetMissingWarningAt,
        now: now
      )
    }

    overnightGuardWatchdogSummary = warnings.isEmpty
      ? "Watchdog ok | raw \(snapshot.notificationCount) | commands \(snapshot.commandWriteCount) | range success \(overnightGuardSuccessfulRangePollCount) | targets \(overnightGuardTargetSummary)"
      : "Watchdog warning | \(warnings.joined(separator: " "))"
    overnightGuardWatchdogWarning = warnings.isEmpty ? nil : overnightGuardWatchdogSummary
    if !warnings.isEmpty, overnightGuardStatus.hasPrefix("Recording overnight guard") {
      overnightGuardStatus = "Recording overnight guard | watchdog warning"
    }
    updateOvernightGuardWarning()
    refreshOvernightReadiness(reason: reason, record: true)
  }

  func recordOvernightWatchdogWarningIfNeeded(
    title: String,
    body: String,
    lastLoggedAt: inout Date,
    now: Date
  ) {
    guard now.timeIntervalSince(lastLoggedAt) >= Self.overnightGuardWarningRepeatInterval else {
      return
    }
    lastLoggedAt = now
    ble.record(level: .warn, source: "overnight.guard", title: title, body: body)
  }

  static func overnightSpoolSizeSummary(_ snapshot: OvernightRawSpoolSnapshot) -> String {
    let total = ByteCountFormatter.string(fromByteCount: Int64(snapshot.totalByteCount), countStyle: .file)
    let raw = ByteCountFormatter.string(fromByteCount: Int64(snapshot.byteCount), countStyle: .file)
    let range = ByteCountFormatter.string(fromByteCount: Int64(snapshot.historicalRangePollByteCount), countStyle: .file)
    let commands = ByteCountFormatter.string(fromByteCount: Int64(snapshot.commandWriteByteCount), countStyle: .file)
    let events = ByteCountFormatter.string(fromByteCount: Int64(snapshot.eventLogByteCount), countStyle: .file)
    return "\(total) total | raw \(raw) | range \(range) | commands \(commands) | events \(events)"
  }

  static func overnightRecoveredSpoolSizeSummary(_ recovered: OvernightGuardRecoveredSession) -> String {
    let totalBytes = recovered.rawByteCount + recovered.historicalRangePollByteCount + recovered.commandWriteByteCount + recovered.eventLogByteCount
    let total = ByteCountFormatter.string(fromByteCount: Int64(totalBytes), countStyle: .file)
    let raw = ByteCountFormatter.string(fromByteCount: Int64(recovered.rawByteCount), countStyle: .file)
    let range = ByteCountFormatter.string(fromByteCount: Int64(recovered.historicalRangePollByteCount), countStyle: .file)
    let commands = ByteCountFormatter.string(fromByteCount: Int64(recovered.commandWriteByteCount), countStyle: .file)
    let events = ByteCountFormatter.string(fromByteCount: Int64(recovered.eventLogByteCount), countStyle: .file)
    return "\(total) total | raw \(raw) | range \(range) | commands \(commands) | events \(events)"
  }

  static func overnightRecoveredStatusSummary(_ recovered: OvernightGuardRecoveredSession) -> String {
    let heartbeat = recovered.lastStatusAt
      .map { captureTimestampFormatter.string(from: $0) }
      ?? "unknown"
    let reason = recovered.lastStatusReason ?? "unknown reason"
    let counts = "raw \(recovered.notificationCount) | commands \(recovered.commandWriteCount) | range \(recovered.historicalRangePollCount) | events \(recovered.eventLogCount)"
    let marker = recovered.crashMarkerStatus.map { " | marker \($0)" } ?? ""
    return "\(heartbeat) | \(reason) | \(counts)\(marker)"
  }

  func applyOvernightRawSpoolWarning(
    from snapshot: OvernightRawSpoolSnapshot,
    reason: String,
    warningStatus: String? = nil
  ) {
    if let lastError = snapshot.lastError {
      overnightGuardRawSpoolWarning = "Raw spool warning: \(lastError). Inspect proof sidecars before opening WHOOP."
      if let warningStatus {
        overnightGuardStatus = "\(warningStatus): \(lastError)"
      }
    } else {
      overnightGuardRawSpoolWarning = nil
    }
    updateOvernightGuardWarning()
    refreshOvernightReadiness(reason: reason)
  }

  func applyOvernightRawSpoolStatusSnapshot(_ snapshot: OvernightRawSpoolSnapshot, reason: String) {
    overnightGuardRawNotificationCount = snapshot.notificationCount
    overnightGuardRangeTelemetryCount = snapshot.historicalRangePollCount
    overnightGuardCommandWriteCount = snapshot.commandWriteCount
    overnightGuardEventLogCount = snapshot.eventLogCount
    overnightGuardSpoolSizeSummary = Self.overnightSpoolSizeSummary(snapshot)
    if let rawURL = snapshot.rawNotificationsURL {
      overnightGuardSpoolPath = rawURL.path
    }
    let warningStatus = overnightGuardActive
      ? "Recording with raw-spool sidecar warning"
      : "Overnight guard proof warning"
    applyOvernightRawSpoolWarning(
      from: snapshot,
      reason: "raw_spool_status_\(reason)",
      warningStatus: warningStatus
    )
  }

  func applyOvernightBLELogFlushIssues(_ issues: [String], reason: String) {
    if issues.isEmpty {
      overnightGuardBLELogWarning = nil
    } else {
      let issueText = issues.prefix(2).joined(separator: " | ")
      overnightGuardBLELogWarning = "BLE log warning: \(issueText). Inspect proof sidecars before opening WHOOP."
      overnightGuardStatus = "Final export BLE log warning: \(issueText)"
    }
    updateOvernightGuardWarning()
    refreshOvernightReadiness(reason: reason, record: !issues.isEmpty)
    if !issues.isEmpty {
      let snapshot = overnightRawSpool.updateFinalSummary(
        status: reason,
        summary: overnightGuardManifestSummary(reason: reason)
      )
      if snapshot.lastError != nil {
        applyOvernightRawSpoolWarning(
          from: snapshot,
          reason: "\(reason)_manifest_refresh",
          warningStatus: "Final export BLE log warning with manifest refresh warning"
        )
      }
      writeOvernightGuardStatus(reason: reason)
    }
  }

}
