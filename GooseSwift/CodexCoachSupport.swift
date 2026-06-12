import Foundation

struct CodexLoginDeviceCode: Equatable {
  let verificationURL: URL
  let userCode: String
}

@MainActor
enum CodexLocalToolContext {
  static func build(healthStore: HealthDataStore, appModel: GooseAppModel) -> [String: Any] {
    [
      "generated_at": isoString(Date()),
      "tools": [
        "load_stats": loadStats(healthStore: healthStore, appModel: appModel),
        "get_activities": getActivities(healthStore: healthStore, appModel: appModel),
        "get_capture_sessions": getCaptureSessions(appModel: appModel),
        "get_raw_session_data": getRawSessionData(appModel: appModel),
      ],
    ]
  }

  private static func loadStats(
    healthStore: HealthDataStore,
    appModel: GooseAppModel
  ) -> [String: Any] {
    let routes: [HealthRoute] = [
      .healthMonitor,
      .sleep,
      .recovery,
      .strain,
      .stress,
      .cardioLoad,
      .energyBank,
      .packetInputs,
    ]
    return [
      "metric_snapshots": routes.map { snapshotPayload(healthStore.snapshot(for: $0)) },
      "health_monitor": healthStore.healthMonitorSnapshots(
        restingHeartRateEstimateBPM: appModel.ble.restingHeartRateEstimateBPM,
        restingHeartRateEstimateSampleCount: appModel.ble.restingHeartRateEstimateSampleCount,
        restingHeartRateEstimateUpdatedAt: appModel.ble.restingHeartRateEstimateUpdatedAt,
        restingHeartRateEstimateSource: appModel.ble.restingHeartRateEstimateSource
      ).map(snapshotPayload),
      "readiness": healthStore.metricInputReadinessSummary(),
      "input_next_action": healthStore.metricInputReadinessNextActionSummary(),
      "score_next_action": healthStore.packetDerivedScoreNextActionSummary(),
      "live_heart_rate": healthStore.latestHeartRateSummary(
        bpm: appModel.ble.liveHeartRateBPM,
        source: appModel.ble.liveHeartRateSource,
        updatedAt: appModel.ble.liveHeartRateUpdatedAt
      ),
      "recovery": [
        "score": healthStore.recoveryScoreDisplayText(),
        "hrv": healthStore.recoveryHRVDisplayText(),
        "resting_heart_rate": healthStore.recoveryRestingHRDisplayText(),
        "respiratory_rate": healthStore.recoveryRespiratoryRateDisplayText(),
        "oxygen_saturation": healthStore.recoveryOxygenSaturationDisplayText(),
        "wrist_temperature": healthStore.recoveryWristTemperatureDisplayText(),
        "sensor_rollup": healthStore.recoverySensorDailyRollupSummary(),
      ],
      "packet_scores": [
        "sleep": healthStore.sleepFeatureScoreSummary(),
        "recovery": healthStore.recoveryFeatureScoreSummary(),
        "strain": healthStore.strainFeatureScoreSummary(),
        "stress": healthStore.stressFeatureScoreSummary(),
      ],
    ]
  }

  private static func getActivities(
    healthStore: HealthDataStore,
    appModel: GooseAppModel
  ) -> [String: Any] {
    let session = appModel.activitySession
    return [
      "current_activity": [
        "selected": session.selectedActivity.title,
        "status": session.statusText,
        "active": session.isActive,
        "paused": session.isPaused,
        "elapsed_seconds": Int(session.elapsed.rounded()),
        "average_heart_rate": jsonValue(session.averageHeartRate),
        "max_heart_rate": jsonValue(session.maxHeartRate),
      ],
      "activity_detection": appModel.activityDetectionStatus,
      "activity_persistence": appModel.activityPersistenceStatus,
      "movement_packets": appModel.movementPacketStatus,
      "strain": healthStore.strainFeatureScoreSummary(),
      "motion_inputs": healthStore.motionFeatureSummary(),
      "step_discovery": healthStore.stepDiscoverySummary(),
      "steps": healthStore.whoopStepsDisplayText(),
      "active_calories": healthStore.whoopActiveCaloriesDisplayText(),
      "total_calories": healthStore.whoopTotalCaloriesDisplayText(),
    ]
  }

  private static func getCaptureSessions(appModel: GooseAppModel) -> [String: Any] {
    [
      "packet_import": appModel.packetImportStatus,
      "last_parsed_frame": appModel.lastParsedFrameSummary,
      "health_packet_capture": [
        "session_id": jsonValue(appModel.healthPacketCaptureSessionID),
        "status": appModel.healthPacketCaptureStatus,
        "started_at": isoString(appModel.healthPacketCaptureStartedAt),
        "frame_count": appModel.healthPacketCaptureFrameCount,
        "target": appModel.healthPacketCaptureTargetSummary,
        "last_packet": appModel.healthPacketCaptureLastPacketSummary,
        "families": appModel.healthPacketCaptureFamilyRows.map { row in
          [
            "id": row.id,
            "title": row.title,
            "detail": row.detail,
            "count": row.count,
            "last_seen": isoString(row.lastSeen),
            "status": row.status.rawValue,
          ]
        },
      ],
      "overnight_guard": [
        "active": appModel.overnightGuardActive,
        "status": appModel.overnightGuardStatus,
        "readiness": appModel.overnightGuardReadinessSummary,
        "raw_notifications": appModel.overnightGuardRawNotificationCount,
        "target": appModel.overnightGuardTargetSummary,
        "last_packet": appModel.overnightGuardLastPacketSummary,
        "spool": appModel.overnightGuardSpoolSizeSummary,
        "sqlite_mirror": appModel.overnightGuardSQLiteMirrorSummary,
        "power": appModel.overnightGuardPowerSummary,
        "watchdog": appModel.overnightGuardWatchdogSummary,
      ],
      "device": devicePayload(appModel.ble),
    ]
  }

  private static func getRawSessionData(appModel: GooseAppModel) -> [String: Any] {
    [
      "latest_whoop_data_packet": appModel.latestWhoopDataPacketStatus,
      "latest_realtime_status_packet": appModel.latestRealtimeStatusPacketStatus,
      "latest_raw_research_packet": appModel.latestRawResearchPacketStatus,
      "latest_pulse_information_packet": appModel.latestPulseInformationPacketStatus,
      "latest_optical_packet": appModel.latestOpticalPacketStatus,
      "latest_history_temperature_candidate": appModel.latestHistoryTemperatureCandidateStatus,
      "latest_respiratory_rate_candidate": appModel.latestRespiratoryRateCandidateStatus,
      "latest_skin_temperature_candidate": appModel.latestSkinTemperatureCandidateStatus,
      "latest_whoop_event": appModel.latestWhoopEventStatus,
      "live_device_data": appModel.liveDeviceDataSummary,
      "recent_device_signal_count": appModel.recentDeviceSignalPoints.count,
    ]
  }

  private static func snapshotPayload(_ snapshot: HealthMetricSnapshot) -> [String: Any] {
    [
      "id": snapshot.id,
      "title": snapshot.title,
      "value": snapshot.displayValue,
      "status": snapshot.status,
      "freshness": snapshot.freshness,
      "provenance": snapshot.provenance,
      "source": snapshot.source.label,
      "trend": [
        "range": snapshot.trend.rangeLabel,
        "summary": snapshot.trend.summary,
        "analysis": snapshot.trend.analysis,
        "point_count": snapshot.trend.points.count,
      ],
    ]
  }

  private static func devicePayload(_ ble: GooseBLEClient) -> [String: Any] {
    [
      "connection": ble.connectionState,
      "reconnect": ble.reconnectState,
      "active_device_name": ble.activeDeviceName,
      "remembered_device": ble.rememberedDeviceDescription,
      "battery": ble.batterySettingsSummary,
      "firmware_version": jsonValue(ble.firmwareVersion),
      "software_revision": jsonValue(ble.softwareRevision),
      "hardware_revision": jsonValue(ble.hardwareRevision),
      "model_number": jsonValue(ble.modelNumber),
      "device_generation": ble.deviceGeneration.rawValue,
      "supports_labrador_ecg": ble.isWhoopMG,
      "last_sync_at": isoString(ble.lastSyncAt),
      "historical_sync": [
        "active": ble.isHistoricalSyncing,
        "status": ble.historicalSyncStatus,
        "packet_count": ble.historicalPacketCount,
      ],
    ]
  }

  private static func isoString(_ date: Date?) -> Any {
    guard let date else {
      return NSNull()
    }
    return isoString(date)
  }

  private static func isoString(_ date: Date) -> String {
    ISO8601DateFormatter().string(from: date)
  }

  private static func jsonValue<T>(_ value: T?) -> Any {
    value ?? NSNull()
  }
}
