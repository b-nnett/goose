import Foundation
import UIKit


extension GooseAppModel {
  func refreshActivityTimeline(for date: Date = Date()) {
    let calendar = Calendar.current
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
    let queryStart = calendar.date(byAdding: .hour, value: -6, to: dayStart) ?? dayStart
    let queryEnd = calendar.date(byAdding: .hour, value: 6, to: dayEnd) ?? dayEnd
    let queryStartMs = unixMilliseconds(queryStart)
    let queryEndMs = unixMilliseconds(queryEnd)
    let databasePath = HealthDataStore.defaultDatabasePath()
    activityTimelineRefreshGeneration += 1
    let generation = activityTimelineRefreshGeneration

    activityTimelineRefreshQueue.async { [weak self] in
      let result: Result<ActivityTimelineRefreshResult, Error>
      do {
        let report = try GooseRustBridge().request(
          method: "activity.list_sessions_with_metrics",
          args: [
            "database_path": databasePath,
            "start_time_unix_ms": queryStartMs,
            "end_time_unix_ms": queryEndMs,
          ]
        )
        let sessions = report["sessions"] as? [[String: Any]] ?? []
        let rawMetricsBySession = report["metrics_by_session"] as? [String: Any] ?? [:]
        let metricsBySession = rawMetricsBySession.reduce(into: [String: [[String: Any]]]()) { output, element in
          if let metrics = element.value as? [[String: Any]] {
            output[element.key] = metrics
          }
        }
        result = .success(
          Self.activityTimelineRefreshResult(
            sessions: sessions,
            dayStart: dayStart,
            dayEnd: dayEnd,
            metricsBySession: metricsBySession
          )
        )
      } catch {
        result = .failure(error)
      }

      DispatchQueue.main.async { [weak self] in
        guard let self, self.activityTimelineRefreshGeneration == generation else {
          return
        }
        switch result {
        case .success(let refresh):
          self.homeActivityTimelineItems = refresh.items
          self.homeActivityTimelineStatus = refresh.status
          self.ble.record(
            source: "activity.timeline",
            title: "home.refresh.ok",
            body: "\(refresh.status) | \(Self.captureTimestampFormatter.string(from: dayStart))-\(Self.captureTimestampFormatter.string(from: dayEnd))"
          )
        case .failure(let error):
          self.homeActivityTimelineStatus = "Activity timeline failed"
          self.ble.record(level: .warn, source: "activity.timeline", title: "home.refresh.failed", body: String(describing: error))
        }
      }
    }
  }

  func startHealthPacketCapture(duration: TimeInterval = 30 * 60, source: String = "ui.debug") {
    startHealthPacketCapture(mode: .walk, duration: duration, source: source)
  }

  func startTemperaturePacketCapture(duration: TimeInterval = 10 * 60, source: String = "ui.debug") {
    startHealthPacketCapture(mode: .temperature, duration: duration, source: source)
  }

  func startPhysiologyPacketCapture(duration: TimeInterval = 30 * 60, source: String = "ui.debug") {
    startHealthPacketCapture(mode: .physiology, duration: duration, source: source)
  }

  func startHealthPacketCapture(
    mode: HealthPacketCaptureMode,
    duration: TimeInterval,
    source: String
  ) {
    ble.record(source: "health.packet_capture", title: "start.requested", body: "source=\(source)")
    guard ble.connectionState == "ready" else {
      healthPacketCaptureStatus = "Connect WHOOP first. Current state: \(ble.connectionState)"
      ble.record(level: .warn, source: "health.packet_capture", title: "start.blocked", body: healthPacketCaptureStatus)
      return
    }
    guard activeHealthPacketCapture == nil else {
      healthPacketCaptureStatus = "Capture already active: \(healthPacketCaptureSessionID?.prefix(8) ?? "?")"
      ble.record(level: .debug, source: "health.packet_capture", title: "start.skipped", body: healthPacketCaptureStatus)
      return
    }

    let sessionID = "ios.health-packet-capture.\(UUID().uuidString)"
    let startedAt = Date()
    let targetFamilies = mode.targetFamilies

    var args: [String: Any] = [
      "database_path": HealthDataStore.defaultDatabasePath(),
      "session_id": sessionID,
      "source": "ios.health_packet_capture",
      "started_at_unix_ms": unixMilliseconds(startedAt),
      "device_model": ble.activeDeviceName,
      "active_device_id": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
      "provenance": [
        "surface": "MoreDebugView",
        "capture_mode": mode.rawValue,
        "purpose": mode.purpose,
        "target_families": targetFamilies,
        "duration_seconds": Int(duration.rounded()),
        "connection_state": ble.connectionState,
        "started_by": source,
      ],
    ]

    if let modelNumber = ble.modelNumber {
      args["device_model"] = modelNumber
    }

    do {
      _ = try rust.request(method: "capture.start_session", args: args)
      healthPacketCaptureTimeoutWorkItem?.cancel()
      activeHealthPacketCapture = ActiveHealthPacketCapture(
        sessionID: sessionID,
        startedAt: startedAt,
        mode: mode,
        importedFrameCount: 0
      )
      healthPacketCaptureStreamRetryAttempt = 0
      healthPacketCaptureSessionID = sessionID
      healthPacketCaptureStartedAt = startedAt
      healthPacketCaptureFrameCount = 0
      healthPacketCaptureFamilyRowsByID.removeAll()
      healthPacketCaptureFamilyRows = []
      healthPacketCaptureFamilyAggregator.reset()
      pendingHealthPacketCaptureLastPacketSummary = nil
      lastRestingHeartRateFrameWriteAt = .distantPast
      healthPacketCaptureUIUpdateWorkItem?.cancel()
      healthPacketCaptureUIUpdateWorkItem = nil
      lastHealthPacketCaptureUIUpdatedAt = Date.distantPast
      healthPacketCaptureTargetSummary = mode.initialTargetSummary
      healthPacketCaptureLastPacketSummary = "Waiting for packets"
      healthPacketCaptureStatus = "\(mode.statusPrefix) for \(healthPacketCaptureDurationText(duration))"
      ble.record(source: "health.packet_capture", title: "start.ok", body: "\(sessionID) mode=\(mode.rawValue) duration=\(Int(duration.rounded()))s")
      requestStreamsForActiveCapture(reason: "capture_start")
      scheduleHistoricalSyncForPhysiologyCaptureIfNeeded(mode: mode)
      scheduleHealthPacketCaptureTimeout(duration: duration)
    } catch {
      healthPacketCaptureStatus = "Start failed: \(String(describing: error))"
      healthPacketCaptureSessionID = nil
      healthPacketCaptureStartedAt = nil
      ble.record(level: .error, source: "health.packet_capture", title: "start.failed", body: String(describing: error))
    }
  }

  func stopHealthPacketCapture(reason: String = "manual_stop") {
    healthPacketCaptureTimeoutWorkItem?.cancel()
    healthPacketCaptureStreamRetryWorkItem?.cancel()
    temperatureHistorySyncWorkItem?.cancel()
    flushCaptureFrameEnqueueUpdates()
    guard let capture = activeHealthPacketCapture else {
      healthPacketCaptureStatus = "No active health packet capture"
      ble.record(level: .debug, source: "health.packet_capture", title: "stop.skipped", body: reason)
      return
    }

    if capture.mode == .walk,
       activeActivityPersistence?.captureSessionID == capture.sessionID,
       activeActivityPersistence?.detectionMethod == "user_assigned",
       reason != "activity_finished",
       reason != "activity_store_failed" {
      healthPacketCaptureStatus = "Capture timer elapsed; keeping stream open for workout"
      ble.record(
        source: "health.packet_capture",
        title: "finish.deferred_active_activity",
        body: "\(capture.sessionID) reason=\(reason)"
      )
      return
    }

    if capture.mode == .walk {
      finishAutoDetectedActivityIfActive(endedAt: Date(), reason: "health_packet_capture_\(reason)")
    } else if activeActivityPersistence?.captureSessionID == capture.sessionID {
      finishAutoDetectedActivityIfActive(endedAt: Date(), reason: "temperature_packet_capture_\(reason)")
      if activeActivityPersistence?.captureSessionID == capture.sessionID {
        activeActivityPersistence = nil
        activeActivityOwnsCaptureSession = false
        activityDetectionIdleWorkItem?.cancel()
        ble.record(
          level: .warn,
          source: "activity.detect",
          title: "candidate.detached_temperature_capture",
          body: capture.sessionID
        )
      }
    }

    do {
      _ = try rust.request(
        method: "capture.finish_session",
        args: [
          "database_path": HealthDataStore.defaultDatabasePath(),
          "session_id": capture.sessionID,
          "ended_at_unix_ms": unixMilliseconds(Date()),
          "frame_count": capture.importedFrameCount,
        ]
      )
      activeHealthPacketCapture = nil
      healthPacketCaptureStreamRetryAttempt = 0
      healthPacketCaptureSessionID = nil
      healthPacketCaptureStartedAt = nil
      healthPacketCaptureStatus = "Stopped \(capture.importedFrameCount) frames (\(reason))"
      healthPacketCaptureFrameCount = capture.importedFrameCount
      publishHealthPacketCaptureUIUpdate()
      publishPacketImportRevision()
      ble.record(source: "health.packet_capture", title: "finish.ok", body: "\(capture.sessionID) frames=\(capture.importedFrameCount) reason=\(reason)")
      if capture.mode == .walk {
        ble.stopMovementHeartRateCapture()
      } else if capture.mode == .physiology {
        ble.stopPhysiologySignalCapture()
      }
    } catch {
      healthPacketCaptureStatus = "Finish failed: \(String(describing: error))"
      ble.record(level: .error, source: "health.packet_capture", title: "finish.failed", body: String(describing: error))
    }
  }

  func startRespiratoryPacketWatch(duration: TimeInterval = 10 * 60) {
    ble.record(
      source: "respiratory.packet_watch",
      title: "start.requested",
      body: "duration=\(Int(duration.rounded()))s sync=\(ble.historicalSyncStatus) canSync=\(ble.canSyncHistorical)"
    )
    guard ble.connectionState == "ready" else {
      respiratoryPacketWatchStatus = "Connect WHOOP first. Current state: \(ble.connectionState)"
      ble.record(level: .warn, source: "respiratory.packet_watch", title: "start.blocked", body: respiratoryPacketWatchStatus)
      return
    }
    guard !respiratoryPacketWatchActive else {
      respiratoryPacketWatchStatus = "Already watching K18 respiratory history"
      ble.record(level: .debug, source: "respiratory.packet_watch", title: "start.skipped", body: respiratoryPacketWatchStatus)
      return
    }

    respiratoryPacketWatchActive = true
    respiratoryPacketWatchK18Count = 0
    respiratoryPacketWatchK24Count = 0
    respiratoryPacketWatchStartedAt = Date()
    respiratoryPacketWatchStatus = "Watching K18 respiratory history for \(healthPacketCaptureDurationText(duration))"
    scheduleRespiratoryPacketWatchTimeout(duration: duration)

    if ble.isHistoricalSyncing {
      respiratoryPacketWatchStatus = "Watching active historical sync for K18 respiratory history"
      return
    }
    guard ble.canSyncHistorical else {
      respiratoryPacketWatchStatus = "Watching passively; historical sync unavailable: \(ble.historicalSyncStatus)"
      ble.record(level: .warn, source: "respiratory.packet_watch", title: "history_sync.unavailable", body: ble.historicalSyncStatus)
      return
    }

    respiratoryPacketWatchStatus = "Requested historical sync; watching for K18 respiratory history"
    ble.syncHistoricalPackets(rangeFirst: true)
  }

  func stopRespiratoryPacketWatch(reason: String = "manual_stop") {
    respiratoryPacketWatchTimeoutWorkItem?.cancel()
    respiratoryPacketWatchTimeoutWorkItem = nil
    guard respiratoryPacketWatchActive else {
      respiratoryPacketWatchStatus = "No active K18 respiratory history watch"
      ble.record(level: .debug, source: "respiratory.packet_watch", title: "stop.skipped", body: reason)
      return
    }

    respiratoryPacketWatchActive = false
    respiratoryPacketWatchStatus = "Stopped K18 watch: K18 \(respiratoryPacketWatchK18Count) | K24 \(respiratoryPacketWatchK24Count) (\(reason))"
    ble.record(source: "respiratory.packet_watch", title: "stop.ok", body: respiratoryPacketWatchStatus)
  }

  func scheduleRespiratoryPacketWatchTimeout(duration: TimeInterval) {
    respiratoryPacketWatchTimeoutWorkItem?.cancel()
    guard duration > 0 else {
      return
    }
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.finishRespiratoryPacketWatchTimedOut()
      }
    }
    respiratoryPacketWatchTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + duration, execute: workItem)
  }

  func finishRespiratoryPacketWatchTimedOut() {
    guard respiratoryPacketWatchActive else {
      return
    }
    respiratoryPacketWatchTimeoutWorkItem?.cancel()
    respiratoryPacketWatchTimeoutWorkItem = nil
    respiratoryPacketWatchActive = false
    respiratoryPacketWatchStatus = "Timed out waiting for K18: K18 \(respiratoryPacketWatchK18Count) | K24 \(respiratoryPacketWatchK24Count)"
    ble.record(level: .warn, source: "respiratory.packet_watch", title: "timeout", body: respiratoryPacketWatchStatus)
  }

  func handleHistoricalSyncProgress(_ progress: GooseHistoricalSyncProgress) {
    handleOvernightHistoricalSyncProgress(progress)
    guard respiratoryPacketWatchActive else {
      return
    }

    let counts = "K18 \(respiratoryPacketWatchK18Count) | K24 \(respiratoryPacketWatchK24Count)"
    if progress.failed {
      respiratoryPacketWatchTimeoutWorkItem?.cancel()
      respiratoryPacketWatchTimeoutWorkItem = nil
      respiratoryPacketWatchActive = false
      respiratoryPacketWatchStatus = "Sync failed before K18: \(progress.detail) | \(counts)"
      ble.record(level: .warn, source: "respiratory.packet_watch", title: "sync.failed", body: respiratoryPacketWatchStatus)
      return
    }

    if progress.isTerminal {
      respiratoryPacketWatchTimeoutWorkItem?.cancel()
      respiratoryPacketWatchTimeoutWorkItem = nil
      respiratoryPacketWatchActive = false
      respiratoryPacketWatchStatus = "Sync complete; no K18 found in \(progress.packetCount) packets | \(counts)"
      ble.record(source: "respiratory.packet_watch", title: "sync.completed_without_k18", body: respiratoryPacketWatchStatus)
      return
    }

    respiratoryPacketWatchStatus = "Sync \(progress.status): \(progress.detail) | packets \(progress.packetCount) | \(counts)"
  }

  func startHRMonitorCapture(source: String = "auto.hr_monitor_connected") {
    ble.record(source: "health.packet_capture", title: "hr_monitor.start.requested", body: "source=\(source)")
    guard ble.hrConnectionState == "connected" else {
      healthPacketCaptureStatus = "HR monitor not connected. State: \(ble.hrConnectionState)"
      ble.record(level: .warn, source: "health.packet_capture", title: "hr_monitor.start.blocked", body: healthPacketCaptureStatus)
      return
    }
    guard activeHealthPacketCapture == nil else {
      ble.record(level: .debug, source: "health.packet_capture", title: "hr_monitor.start.skipped", body: "capture already active mode=\(activeHealthPacketCapture?.mode.rawValue ?? "?")")
      return
    }

    let sessionID = "ios.health-packet-capture.\(UUID().uuidString)"
    let startedAt = Date()

    var args: [String: Any] = [
      "database_path": HealthDataStore.defaultDatabasePath(),
      "session_id": sessionID,
      "source": "ios.health_packet_capture",
      "started_at_unix_ms": unixMilliseconds(startedAt),
      "device_model": ble.activeDeviceName,
      "active_device_id": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
      "provenance": [
        "surface": "HRMonitor",
        "capture_mode": HealthPacketCaptureMode.hrMonitor.rawValue,
        "purpose": HealthPacketCaptureMode.hrMonitor.purpose,
        "target_families": HealthPacketCaptureMode.hrMonitor.targetFamilies,
        "connection_state": ble.hrConnectionState,
        "started_by": source,
      ],
    ]

    if let modelNumber = ble.modelNumber {
      args["device_model"] = modelNumber
    }

    do {
      _ = try rust.request(method: "capture.start_session", args: args)
      healthPacketCaptureTimeoutWorkItem?.cancel()
      activeHealthPacketCapture = ActiveHealthPacketCapture(
        sessionID: sessionID,
        startedAt: startedAt,
        mode: .hrMonitor,
        importedFrameCount: 0
      )
      healthPacketCaptureSessionID = sessionID
      healthPacketCaptureStartedAt = startedAt
      healthPacketCaptureFrameCount = 0
      healthPacketCaptureFamilyRowsByID.removeAll()
      healthPacketCaptureFamilyRows = []
      healthPacketCaptureFamilyAggregator.reset()
      pendingHealthPacketCaptureLastPacketSummary = nil
      lastRestingHeartRateFrameWriteAt = .distantPast
      healthPacketCaptureUIUpdateWorkItem?.cancel()
      healthPacketCaptureUIUpdateWorkItem = nil
      lastHealthPacketCaptureUIUpdatedAt = Date.distantPast
      healthPacketCaptureTargetSummary = HealthPacketCaptureMode.hrMonitor.initialTargetSummary
      healthPacketCaptureLastPacketSummary = "Waiting for packets"
      healthPacketCaptureStatus = "\(HealthPacketCaptureMode.hrMonitor.statusPrefix) — \(ble.activeDeviceName)"
      ble.record(source: "health.packet_capture", title: "hr_monitor.start.ok", body: "\(sessionID) mode=hr_monitor source=\(source)")
    } catch {
      healthPacketCaptureStatus = "HR monitor start failed: \(String(describing: error))"
      healthPacketCaptureSessionID = nil
      healthPacketCaptureStartedAt = nil
      ble.record(level: .error, source: "health.packet_capture", title: "hr_monitor.start.failed", body: String(describing: error))
    }
  }

  func stopHRMonitorCapture(reason: String = "hr_monitor_disconnected") {
    healthPacketCaptureTimeoutWorkItem?.cancel()
    flushCaptureFrameEnqueueUpdates()
    guard let capture = activeHealthPacketCapture, capture.mode == .hrMonitor else {
      ble.record(level: .debug, source: "health.packet_capture", title: "hr_monitor.stop.skipped", body: reason)
      return
    }

    do {
      _ = try rust.request(
        method: "capture.finish_session",
        args: [
          "database_path": HealthDataStore.defaultDatabasePath(),
          "session_id": capture.sessionID,
          "ended_at_unix_ms": unixMilliseconds(Date()),
          "frame_count": capture.importedFrameCount,
        ]
      )
      activeHealthPacketCapture = nil
      healthPacketCaptureSessionID = nil
      healthPacketCaptureStartedAt = nil
      healthPacketCaptureFrameCount = capture.importedFrameCount
      healthPacketCaptureStatus = "Stopped \(capture.importedFrameCount) HR monitor frames (\(reason))"
      publishHealthPacketCaptureUIUpdate()
      publishPacketImportRevision()
      ble.record(source: "health.packet_capture", title: "hr_monitor.finish.ok", body: "\(capture.sessionID) frames=\(capture.importedFrameCount) reason=\(reason)")
    } catch {
      healthPacketCaptureStatus = "HR monitor finish failed: \(String(describing: error))"
      ble.record(level: .error, source: "health.packet_capture", title: "hr_monitor.finish.failed", body: String(describing: error))
    }
  }

  func requestStreamsForActiveCapture(reason: String) {
    guard let capture = activeHealthPacketCapture else {
      return
    }

    switch capture.mode {
    case .walk:
      requestMovementHeartRateStreamForActiveCapture(reason: reason)
    case .temperature:
      requestTemperatureHistoryForActiveCapture(reason: reason)
    case .physiology:
      requestPhysiologyStreamForActiveCapture(reason: reason)
    case .hrMonitor:
      break
    }
  }

  func requestMovementHeartRateStreamForActiveCapture(reason: String) {
    guard activeHealthPacketCapture?.mode == .walk else {
      return
    }

    ble.record(source: "health.packet_capture", title: "stream.requested", body: reason)
    ble.startMovementHeartRateCapture()
    scheduleMovementHeartRateStreamRetryIfNeeded()
  }

  func scheduleMovementHeartRateStreamRetryIfNeeded() {
    healthPacketCaptureStreamRetryWorkItem?.cancel()
    guard activeHealthPacketCapture?.mode == .walk,
          healthPacketCaptureFrameCount == 0,
          healthPacketCaptureStreamRetryAttempt < 12 else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.retryMovementHeartRateStreamIfNeeded()
      }
    }
    healthPacketCaptureStreamRetryWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: workItem)
  }

  func retryMovementHeartRateStreamIfNeeded() {
    guard activeHealthPacketCapture?.mode == .walk, healthPacketCaptureFrameCount == 0 else {
      return
    }
    healthPacketCaptureStreamRetryAttempt += 1
    requestMovementHeartRateStreamForActiveCapture(reason: "retry_\(healthPacketCaptureStreamRetryAttempt)")
  }

  func requestPhysiologyStreamForActiveCapture(reason: String) {
    guard activeHealthPacketCapture?.mode == .physiology else {
      return
    }

    ble.record(source: "health.packet_capture", title: "physiology.stream.requested", body: reason)
    ble.startPhysiologySignalCapture()
    schedulePhysiologyStreamRetryIfNeeded()
  }

  func scheduleHistoricalSyncForPhysiologyCaptureIfNeeded(mode: HealthPacketCaptureMode) {
    guard mode == .physiology, autoSyncHistoryDuringPhysiologyCapture else {
      return
    }
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.runHistoricalSyncForActivePhysiologyCapture()
      }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 20, execute: workItem)
  }

  func runHistoricalSyncForActivePhysiologyCapture() {
    guard activeHealthPacketCapture?.mode == .physiology else {
      return
    }
    guard ble.canSyncHistorical else {
      ble.record(level: .warn, source: "health.packet_capture", title: "physiology.history_sync.blocked", body: ble.historicalSyncStatus)
      return
    }
    ble.record(source: "health.packet_capture", title: "physiology.history_sync.requested")
    ble.syncHistoricalPackets(rangeFirst: true)
  }

  func schedulePhysiologyStreamRetryIfNeeded() {
    healthPacketCaptureStreamRetryWorkItem?.cancel()
    guard activeHealthPacketCapture?.mode == .physiology,
          healthPacketCaptureFrameCount == 0,
          healthPacketCaptureStreamRetryAttempt < 12 else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.retryPhysiologyStreamIfNeeded()
      }
    }
    healthPacketCaptureStreamRetryWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: workItem)
  }

  func retryPhysiologyStreamIfNeeded() {
    guard activeHealthPacketCapture?.mode == .physiology, healthPacketCaptureFrameCount == 0 else {
      return
    }
    healthPacketCaptureStreamRetryAttempt += 1
    requestPhysiologyStreamForActiveCapture(reason: "retry_\(healthPacketCaptureStreamRetryAttempt)")
  }

  func requestTemperatureHistoryForActiveCapture(reason: String) {
    guard activeHealthPacketCapture?.mode == .temperature else {
      return
    }

    ble.record(
      source: "health.packet_capture",
      title: "temperature.history.requested",
      body: "reason=\(reason) sync=\(ble.historicalSyncStatus) canSync=\(ble.canSyncHistorical)"
    )
    if ble.isHistoricalSyncing {
      healthPacketCaptureStatus = "Capturing temperature from active historical sync"
      return
    }

    temperatureHistorySyncWorkItem?.cancel()
    ble.record(source: "health.packet_capture", title: "temperature.live_stream.stop_requested", body: reason)
    ble.stopMovementHeartRateCapture()
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.startTemperatureHistoricalSync(reason: reason)
      }
    }
    temperatureHistorySyncWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: workItem)
  }

  func startTemperatureHistoricalSync(reason: String) {
    guard activeHealthPacketCapture?.mode == .temperature else {
      return
    }
    temperatureHistorySyncWorkItem = nil
    if ble.isHistoricalSyncing {
      healthPacketCaptureStatus = "Capturing temperature from active historical sync"
      return
    }
    guard ble.canSyncHistorical else {
      healthPacketCaptureStatus = "Temperature capture waiting for historical sync: \(ble.historicalSyncStatus)"
      ble.record(level: .warn, source: "health.packet_capture", title: "temperature.history.blocked", body: ble.historicalSyncStatus)
      return
    }
    ble.syncHistoricalPackets(rangeFirst: true)
  }

  func scheduleHealthPacketCaptureTimeout(duration: TimeInterval) {
    guard duration > 0 else {
      return
    }
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.stopHealthPacketCapture(reason: "duration_elapsed")
      }
    }
    healthPacketCaptureTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + duration, execute: workItem)
  }

  func scheduleAutoStartHealthPacketCaptureIfNeeded() {
    guard autoStartHealthPacketCaptureOnReady || autoStartTemperaturePacketCaptureOnReady || autoStartPhysiologyPacketCaptureOnReady else {
      return
    }
    autoStartHealthPacketCaptureWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.attemptAutoStartHealthPacketCapture()
      }
    }
    autoStartHealthPacketCaptureWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 1, execute: workItem)
  }

  func attemptAutoStartHealthPacketCapture() {
    guard (autoStartHealthPacketCaptureOnReady || autoStartTemperaturePacketCaptureOnReady || autoStartPhysiologyPacketCaptureOnReady),
          activeHealthPacketCapture == nil else {
      return
    }
    autoStartHealthPacketCaptureAttempt += 1
    if ble.connectionState == "ready" {
      if autoStartPhysiologyPacketCaptureOnReady {
        startPhysiologyPacketCapture(duration: autoStartPhysiologyPacketCaptureDuration, source: "launch_argument")
      } else if autoStartTemperaturePacketCaptureOnReady {
        startTemperaturePacketCapture(duration: autoStartTemperaturePacketCaptureDuration, source: "launch_argument")
      } else {
        startHealthPacketCapture(duration: autoStartHealthPacketCaptureDuration, source: "launch_argument")
      }
      return
    }
    guard autoStartHealthPacketCaptureAttempt < 120 else {
      healthPacketCaptureStatus = "Auto-start timed out waiting for WHOOP"
      ble.record(level: .warn, source: "health.packet_capture", title: "auto_start.timeout", body: ble.connectionState)
      return
    }
    scheduleAutoStartHealthPacketCaptureIfNeeded()
  }

  func scheduleAutoStartRespiratoryPacketWatchIfNeeded() {
    guard autoStartRespiratoryPacketWatchOnReady,
          !respiratoryPacketWatchActive else {
      return
    }
    autoStartRespiratoryPacketWatchWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.attemptAutoStartRespiratoryPacketWatch()
      }
    }
    autoStartRespiratoryPacketWatchWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 1, execute: workItem)
  }

  func attemptAutoStartRespiratoryPacketWatch() {
    guard autoStartRespiratoryPacketWatchOnReady,
          !respiratoryPacketWatchActive else {
      return
    }
    autoStartRespiratoryPacketWatchAttempt += 1
    if ble.connectionState == "ready" {
      ble.record(
        source: "respiratory.packet_watch",
        title: "auto_start.ready",
        body: "attempt=\(autoStartRespiratoryPacketWatchAttempt) duration=\(Int(autoStartRespiratoryPacketWatchDuration.rounded()))s"
      )
      startRespiratoryPacketWatch(duration: autoStartRespiratoryPacketWatchDuration)
      return
    }
    guard autoStartRespiratoryPacketWatchAttempt < 120 else {
      respiratoryPacketWatchStatus = "Auto-start timed out waiting for WHOOP"
      ble.record(level: .warn, source: "respiratory.packet_watch", title: "auto_start.timeout", body: ble.connectionState)
      return
    }
    scheduleAutoStartRespiratoryPacketWatchIfNeeded()
  }

  func healthPacketCaptureDurationText(_ duration: TimeInterval) -> String {
    if duration >= 60 {
      return "\(Int((duration / 60).rounded())) min"
    }
    return "\(Int(duration.rounded())) sec"
  }

}
