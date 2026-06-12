import Foundation


extension GooseAppModel {
  // UserDefaults key for the last successful band sleep sync date.
  // Written at the START of syncBandSleepHistory to prevent retry loops
  // on drop+reconnect (per CONTEXT.md decision).
  static let lastBandSleepSyncDateKey = "goose.swift.last_band_sleep_sync_date"

  // Deterministic sleep session ID to prevent duplicate inserts on reconnect.
  // Format: "band_ble.{deviceId}.{yyyy-MM-dd}" using overnightStartDate in local timezone.
  static func bandSleepId(deviceId: String, overnightStartDate: Date) -> String {
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd"
    formatter.timeZone = TimeZone.current
    let dateStr = formatter.string(from: overnightStartDate)
    return "band_ble.\(deviceId).\(dateStr)"
  }

  // Overnight window in Unix seconds: yesterday 20:00 local → today 12:00 local.
  // Covers up to 16 hours — sufficient for all typical overnight sleep patterns.
  static func overnightWindow() -> (Double, Double) {
    let calendar = Calendar.current
    let now = Date()

    var todayNoonComponents = calendar.dateComponents([.year, .month, .day], from: now)
    todayNoonComponents.hour = 12
    todayNoonComponents.minute = 0
    todayNoonComponents.second = 0
    let todayNoon = calendar.date(from: todayNoonComponents) ?? now

    let yesterday = calendar.date(byAdding: .day, value: -1, to: now) ?? now
    var yesterdayEveningComponents = calendar.dateComponents([.year, .month, .day], from: yesterday)
    yesterdayEveningComponents.hour = 20
    yesterdayEveningComponents.minute = 0
    yesterdayEveningComponents.second = 0
    let yesterdayEvening = calendar.date(from: yesterdayEveningComponents) ?? yesterday

    return (yesterdayEvening.timeIntervalSince1970, todayNoon.timeIntervalSince1970)
  }

  // Gate check: called from handleBLEConnectionStateChange when state == "ready".
  // Synchronous — launches Task if all guards pass.
  func maybeScheduleMorningSleepSync() {
    guard Calendar.current.component(.hour, from: Date()) >= 4 else { return }
    if let lastSync = UserDefaults.standard.object(forKey: Self.lastBandSleepSyncDateKey) as? Date,
       Calendar.current.isDateInToday(lastSync) {
      return
    }
    Task { @MainActor in await self.syncBandSleepHistory() }
  }

  // Full morning sync flow. Runs in a detached Task context (bridge calls via
  // requestAsync which uses Task.detached internally). Never call from @MainActor inline.
  func syncBandSleepHistory() async {
    // Write UserDefaults BEFORE any await to prevent retry loops on drop+reconnect.
    UserDefaults.standard.set(Date(), forKey: Self.lastBandSleepSyncDateKey)

    // Own bridge instance to avoid data races with the shared GooseAppModel.rust instance
    // (GooseRustBridge is @unchecked Sendable with unguarded mutable state).
    let localRust = GooseRustBridge()
    let store = healthStore
    store?.markBandSleepSyncRequested(
      automatic: true,
      canSync: ble.canSyncHistorical,
      detail: ""
    )

    let (overnightStart, overnightEnd) = Self.overnightWindow()

    let deviceId = ble.activeDeviceIdentifier?.uuidString ?? ""
    guard !deviceId.isEmpty else {
      store?.markBandSleepSyncFailed("No active device")
      return
    }

    let dbPath = HealthDataStore.defaultDatabasePath()

    do {
      // SQLite-first check: if >= 100 gravity rows exist, skip BLE historical sync.
      let gravityResult = try await localRust.requestAsync(
        method: "store.gravity_rows_between",
        args: [
          "database_path": dbPath,
          "device_id": deviceId,
          "ts_start": overnightStart,
          "ts_end": overnightEnd,
        ]
      )
      let gravityRows = gravityResult["rows"] as? [[String: Any]] ?? []
      let gravityCount = gravityRows.count

      if gravityCount < 100 {
        // BLE historical sync needed — poll historicalSyncStatus instead of
        // setting onHistoricalSyncCompleted to avoid AppShellView callback conflict
        // (RESEARCH.md Pitfall #3: onHistoricalSyncCompleted is a single slot
        // owned by AppShellView; overwriting it breaks manual sync in AppShellView).
        guard ble.canSyncHistorical else {
          store?.markBandSleepSyncFailed(
            "BLE sync unavailable: \(ble.historicalSyncStatus)"
          )
          return
        }
        ble.syncHistoricalPackets(rangeFirst: true)
        // Poll historicalSyncStatus: 1s intervals, max 120 attempts (2 minutes).
        var attempts = 0
        while attempts < 120 {
          try await Task.sleep(nanoseconds: 1_000_000_000)
          let status = ble.historicalSyncStatus
          if status == "synced" {
            break
          }
          if status == "failed" {
            store?.markBandSleepSyncFailed("BLE historical sync failed")
            return
          }
          attempts += 1
        }
        if attempts >= 120 {
          store?.markBandSleepSyncFailed("BLE historical sync timed out")
          return
        }
      }

      // Run sleep staging on the overnight gravity data.
      let stagingResult = try await localRust.requestAsync(
        method: "metrics.sleep_staging",
        args: [
          "database_path": dbPath,
          "device_id": deviceId,
          "sleep_start_ts": overnightStart,
          "sleep_end_ts": overnightEnd,
        ]
      )

      let stagingMethod = stagingResult["staging_method"] as? String ?? "no_imu_data"
      guard stagingMethod != "no_imu_data" else {
        store?.bandSleepImportStatus = "A aguardar sincronização"
        return
      }

      // Build deterministic sleep_id to prevent duplicates on reconnect.
      let overnightStartDate = Date(timeIntervalSince1970: overnightStart)
      let sleepId = Self.bandSleepId(deviceId: deviceId, overnightStartDate: overnightStartDate)

      // Build stage_summary from staging result (BTreeMap<String, f64> serialized as dict).
      let stageSummary = stagingResult["stage_minutes"] as? [String: Double] ?? [:]
      let provenanceDict: [String: Any] = ["source": "band_ble", "auto_sync": true]

      let sessionInput: [String: Any] = [
        "sleep_id": sleepId,
        "source": "band_ble",
        "platform": "goose_ble",
        "platform_record_id": sleepId,
        "start_time_unix_ms": Int64(overnightStart * 1000),
        "end_time_unix_ms": Int64(overnightEnd * 1000),
        "confidence": 0.7,
        "stage_summary": stageSummary,
        "provenance": provenanceDict,
      ]

      // Insert the external sleep session (idempotent: UNIQUE on platform+platform_record_id).
      _ = try await localRust.requestAsync(
        method: "sleep.import_external_history",
        args: [
          "database_path": dbPath,
          "sessions": [sessionInput],
          "stages": [] as [[String: Any]],
        ]
      )

      // Refresh sleep displays and set success status.
      await store?.refreshSleepAfterBandSync(packetCount: 0)
      store?.bandSleepImportStatus = "Sincronizado da pulseira"
      let notifDurationMinutes = Int(stageSummary.values.reduce(0, +))
      let notifHRV: Double? = {
        guard UserDefaults.standard.object(forKey: "goose.swift.liveHRVRMSSD") != nil else { return nil }
        return UserDefaults.standard.double(forKey: "goose.swift.liveHRVRMSSD")
      }()
      let notifRecovery = Double(store?.snapshot(for: .recovery).value ?? "")
      Task {
        await NotificationScheduler.shared.scheduleSleepProcessed(
          durationMinutes: notifDurationMinutes,
          hrvMS: notifHRV,
          recoveryPercent: notifRecovery
        )
      }

    } catch {
      store?.markBandSleepSyncFailed("Sleep sync error: \(error)")
    }
  }
}
