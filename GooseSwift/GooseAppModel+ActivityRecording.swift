import Foundation
import UIKit


extension GooseAppModel {
  func prepareClientHello() {
    ble.record(source: "rust", title: "hello.prepare.start")
    rustStatus = "Checking Rust bridge"
    helloSummary = "Preparing client hello"

    rustStartupQueue.async { [weak self] in
      let result: Result<(coreVersion: String, summary: String), Error>
      do {
        let rust = GooseRustBridge()
        let version = try rust.request(method: "core.version")
        let coreVersion = (version["core_version"] as? String) ?? "unknown"
        let parsed = try rust.request(
          method: "protocol.parse_frame_hex",
          args: [
            "device_type": "GOOSE",
            "frame_hex": GooseHello.clientHelloFrameHex,
          ]
        )
        let sequence = parsed["sequence"] ?? "?"
        let packetType = parsed["packet_type"] ?? "?"
        result = .success((coreVersion: coreVersion, summary: "GET_HELLO seq \(sequence), packet \(packetType)"))
      } catch {
        result = .failure(error)
      }

      DispatchQueue.main.async { [weak self] in
        guard let self else {
          return
        }
        switch result {
        case .success(let output):
          self.rustStatus = "Rust core \(output.coreVersion)"
          self.helloSummary = output.summary
          self.ble.record(source: "rust", title: "core.version", body: output.coreVersion)
          self.ble.record(source: "rust", title: "hello.prepare.ok", body: output.summary)
        case .failure(let error):
          self.rustStatus = "Rust bridge unavailable"
          self.helloSummary = "Client hello frame ready; parser unavailable"
          self.ble.record(level: .error, source: "rust", title: "hello.prepare.failed", body: String(describing: error))
        }
      }
    }
  }

  func beginActivityRecording(
    activity: ActivityKind,
    startedAt: Date,
    source: String = "ios.live_activity",
    detectionMethod: String = "user_assigned",
    syncStatus: String = "user_confirmed"
  ) {
    if detectionMethod == "user_assigned" {
      finishAutoDetectedActivityIfActive(endedAt: startedAt, reason: "manual_activity_started")
      ble.startMovementHeartRateCapture()
      enterActivityHighFrequencyHistorySyncIfNeeded(activity: activity)
    }

    let activitySessionID = "ios.activity.\(UUID().uuidString)"
    let existingCaptureSessionID = activeHealthPacketCapture?.sessionID
    let captureSessionID = existingCaptureSessionID ?? "\(activitySessionID).capture"
    let ownsCaptureSession = existingCaptureSessionID == nil
    activeActivityOwnsCaptureSession = ownsCaptureSession
    activeActivityPersistence = ActiveActivityPersistence(
      activitySessionID: activitySessionID,
      captureSessionID: captureSessionID,
      startedAt: startedAt,
      source: source,
      detectionMethod: detectionMethod,
      syncStatus: syncStatus,
      importedFrameCount: 0
    )
    Task { await self.strainAccumulator.reset() }
    activityPersistenceStatus = syncStatus == "candidate" ? "Candidate \(activity.title)" : "Recording \(activity.title)"

    if ownsCaptureSession {
      do {
        _ = try rust.request(
          method: "capture.start_session",
          args: [
            "database_path": HealthDataStore.defaultDatabasePath(),
            "session_id": captureSessionID,
            "source": source,
            "started_at_unix_ms": unixMilliseconds(startedAt),
            "device_model": ble.activeDeviceName,
            "active_device_id": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
            "provenance": [
              "activity_session_id": activitySessionID,
              "activity_type": rustActivityType(for: activity),
              "activity_title": activity.title,
              "started_by": source,
              "detection_method": detectionMethod,
              "sync_status": syncStatus,
              "capture_mode": "activity",
            ],
          ]
        )
        ble.record(source: "rust", title: "activity.capture.start.ok", body: captureSessionID)
      } catch {
        ble.record(level: .error, source: "rust", title: "activity.capture.start.failed", body: String(describing: error))
      }
    } else {
      ble.record(source: "rust", title: "activity.capture.attach_existing", body: captureSessionID)
    }
  }

  func enterActivityHighFrequencyHistorySyncIfNeeded(activity: ActivityKind) {
    guard !ble.highFrequencyHistorySyncActive else {
      activityRequestedHighFrequencyHistorySync = false
      ble.record(
        source: "activity.high_frequency_sync",
        title: "enter.skipped",
        body: "already active for \(activity.title)"
      )
      return
    }

    guard ble.canWriteHighFrequencyHistorySync else {
      activityRequestedHighFrequencyHistorySync = false
      ble.record(
        level: .warn,
        source: "activity.high_frequency_sync",
        title: "enter.blocked",
        body: "\(activity.title) | \(ble.highFrequencyHistorySyncDisplaySummary)"
      )
      return
    }

    activityRequestedHighFrequencyHistorySync = true
    ble.record(
      source: "activity.high_frequency_sync",
      title: "enter.requested",
      body: activity.title
    )
    ble.enterHighFrequencyHistorySync()
  }

  func exitActivityHighFrequencyHistorySyncIfNeeded(reason: String) {
    guard activityRequestedHighFrequencyHistorySync else {
      return
    }

    activityRequestedHighFrequencyHistorySync = false
    guard ble.canWriteHighFrequencyHistorySync else {
      ble.record(
        level: .warn,
        source: "activity.high_frequency_sync",
        title: "exit.blocked",
        body: "\(reason) | \(ble.highFrequencyHistorySyncDisplaySummary)"
      )
      return
    }

    ble.record(
      source: "activity.high_frequency_sync",
      title: "exit.requested",
      body: reason
    )
    ble.exitHighFrequencyHistorySync()
  }

  func finishActivityRecording(
    activity: ActivityKind,
    startedAt: Date?,
    endedAt: Date,
    elapsed: TimeInterval,
    averageHeartRate: Int?,
    maxHeartRate: Int?,
    zoneDurations: [Int: TimeInterval],
    distanceMeters: Double,
    elevationGainMeters: Double,
    routePointCount: Int,
    source: String = "ios.live_activity",
    detectionMethod: String = "user_assigned",
    syncStatus: String = "user_confirmed",
    confidence: Double = 1.0,
    extraProvenance: [String: Any] = [:]
  ) {
    flushCaptureFrameEnqueueUpdates()
    let persistence = activeActivityPersistence
    let ownsCaptureSession = activeActivityOwnsCaptureSession
    activeActivityPersistence = nil
    activeActivityOwnsCaptureSession = false
    Task { await self.strainAccumulator.freeze() }
    liveWorkoutStrain = 0

    let sessionID = persistence?.activitySessionID ?? "ios.activity.\(UUID().uuidString)"
    let captureSessionID = persistence?.captureSessionID
    let start = startedAt ?? persistence?.startedAt ?? endedAt.addingTimeInterval(-max(elapsed, 1))
    let end = max(endedAt, start.addingTimeInterval(1))
    let startMs = unixMilliseconds(start)
    let endMs = unixMilliseconds(end)
    let activityType = rustActivityType(for: activity)
    let sessionSource = persistence?.source ?? source
    let sessionDetectionMethod = persistence?.detectionMethod ?? detectionMethod
    let sessionSyncStatus = persistence?.syncStatus ?? syncStatus
    let boundedConfidence = min(max(confidence, 0), 1)
    let persistedElapsed = max(end.timeIntervalSince(start), 1)
    let activeTimerElapsed = max(elapsed, 0)
    let storesLocationMetrics = activity.usesGPS
    let sensorMetrics = sessionDetectionMethod == "user_assigned"
      ? persistence?.sensorMetricSnapshot(endedAt: end)
      : nil
    let metricAverageHeartRate = sensorMetrics?.averageHeartRate ?? averageHeartRate
    let metricMaxHeartRate = sensorMetrics?.maxHeartRate ?? maxHeartRate
    let metricZoneDurations = normalizedZoneDurations(
      sensorMetrics?.zoneDurations ?? zoneDurations,
      targetDuration: persistedElapsed,
      fallbackHeartRate: metricAverageHeartRate
    )
    let shouldFinishSharedHealthCapture = activeHealthPacketCapture?.sessionID == captureSessionID
      && sessionDetectionMethod == "user_assigned"
    let shouldExitActivityHighFrequencySync = sessionDetectionMethod == "user_assigned"
    let highFrequencyHistorySyncRequested = activityRequestedHighFrequencyHistorySync
    defer {
      if shouldExitActivityHighFrequencySync {
        exitActivityHighFrequencyHistorySyncIfNeeded(reason: "activity_finished")
      }
    }

    if let captureSessionID, ownsCaptureSession {
      do {
        _ = try rust.request(
          method: "capture.finish_session",
          args: [
            "database_path": HealthDataStore.defaultDatabasePath(),
            "session_id": captureSessionID,
            "ended_at_unix_ms": endMs,
            "frame_count": persistence?.importedFrameCount ?? 0,
          ]
        )
        ble.record(source: "rust", title: "activity.capture.finish.ok", body: "\(captureSessionID) frames=\(persistence?.importedFrameCount ?? 0)")
      } catch {
        ble.record(level: .error, source: "rust", title: "activity.capture.finish.failed", body: String(describing: error))
      }
    }

    var provenance: [String: Any] = [
      "capture_session_id": captureSessionID ?? NSNull(),
      "device_id": ble.activeDeviceIdentifier?.uuidString ?? NSNull(),
      "device_model": ble.activeDeviceName,
      "distance_source": storesLocationMetrics ? "ios.core_location" : "none",
      "heart_rate_source": "whoop.ble.raw_motion_k10",
      "route_point_count": storesLocationMetrics ? routePointCount : 0,
      "imported_frame_count": persistence?.importedFrameCount ?? 0,
      "source": sessionSource,
      "detection_method": sessionDetectionMethod,
      "sync_status": sessionSyncStatus,
      "activity_elapsed_seconds": persistedElapsed,
      "active_timer_elapsed_seconds": activeTimerElapsed,
      "heart_rate_metric_source": sensorMetrics?.hasHeartRate == true ? "whoop.ble.raw_motion_k10" : "ui_timer_live_hr",
      "high_frequency_history_sync_requested": highFrequencyHistorySyncRequested,
      "high_frequency_history_sync_status_at_finish": ble.highFrequencyHistorySyncStatus,
    ]
    if let sensorMetrics, sensorMetrics.movementPacketCount > 0 {
      provenance["movement_packet_count"] = sensorMetrics.movementPacketCount
      provenance["mean_motion_intensity_0_to_1"] = sensorMetrics.meanMotionIntensity
      provenance["peak_motion_intensity_0_to_1"] = sensorMetrics.peakMotionIntensity
    }
    if let lastImportedFrameAt = persistence?.lastImportedFrameAt {
      provenance["last_imported_frame_at"] = Self.captureTimestampFormatter.string(from: lastImportedFrameAt)
    }
    for (key, value) in extraProvenance {
      provenance[key] = value
    }

    do {
      _ = try rust.request(
        method: "activity.create_session",
        args: [
          "database_path": HealthDataStore.defaultDatabasePath(),
          "session_id": sessionID,
          "source": sessionSource,
          "start_time_unix_ms": startMs,
          "end_time_unix_ms": endMs,
          "activity_type": activityType,
          "external_activity_type_name": activityExternalName(for: activity),
          "custom_label": activity.title,
          "confidence": boundedConfidence,
          "detection_method": sessionDetectionMethod,
          "sync_status": sessionSyncStatus,
          "provenance": provenance,
        ]
      )

      var activityMetrics: [[String: Any]] = []
      appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "duration", value: persistedElapsed, unit: "s", startMs: startMs, endMs: endMs, source: sessionSource)
      if abs(activeTimerElapsed - persistedElapsed) > 1 {
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "active_duration", value: activeTimerElapsed, unit: "s", startMs: startMs, endMs: endMs, source: sessionSource)
      }
      if storesLocationMetrics {
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "distance", value: max(distanceMeters, 0), unit: "m", startMs: startMs, endMs: endMs, source: sessionSource)
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "route_points", value: Double(routePointCount), unit: "count", startMs: startMs, endMs: endMs, source: sessionSource)
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "elevation_gain", value: max(elevationGainMeters, 0), unit: "m", startMs: startMs, endMs: endMs, source: sessionSource)
      }
      if let metricAverageHeartRate {
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "average_hr", value: Double(metricAverageHeartRate), unit: "bpm", startMs: startMs, endMs: endMs, source: sessionSource)
      }
      if let metricMaxHeartRate {
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "max_hr", value: Double(metricMaxHeartRate), unit: "bpm", startMs: startMs, endMs: endMs, source: sessionSource)
      }
      for zoneID in 1...5 {
        let seconds = metricZoneDurations[zoneID, default: 0]
        appendActivityMetric(&activityMetrics, sessionID: sessionID, name: "hr_zone_\(zoneID)_duration", value: seconds, unit: "s", startMs: startMs, endMs: endMs, source: sessionSource)
      }
      attachActivityMetrics(activityMetrics)

      let storedPrefix = sessionSyncStatus == "candidate" ? "Stored candidate" : "Stored"
      let storedDistance = storesLocationMetrics ? " \(formatPersistedDistance(distanceMeters))" : ""
      let logDistance = storesLocationMetrics ? "\(Int(distanceMeters.rounded()))m" : "no distance"
      activityPersistenceStatus = "\(storedPrefix) \(activity.title)\(storedDistance)"
      ble.record(source: "rust", title: "activity.session.store.ok", body: "\(sessionID) \(activityType) \(logDistance)")
      refreshActivityTimeline(for: end)
      if shouldFinishSharedHealthCapture {
        stopHealthPacketCapture(reason: "activity_finished")
      } else if sessionDetectionMethod == "user_assigned", activeHealthPacketCapture == nil {
        ble.stopMovementHeartRateCapture()
      }
    } catch {
      activityPersistenceStatus = "Activity store failed"
      ble.record(level: .error, source: "rust", title: "activity.session.store.failed", body: String(describing: error))
      if shouldFinishSharedHealthCapture {
        stopHealthPacketCapture(reason: "activity_store_failed")
      } else if sessionDetectionMethod == "user_assigned", activeHealthPacketCapture == nil {
        ble.stopMovementHeartRateCapture()
      }
    }
  }

}
