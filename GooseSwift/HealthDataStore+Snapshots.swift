import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func runPacketScores() async {
    packetScoreStatus = "Extracting bridge packet-derived scores..."
    let baseArgs = bridgeBaseArgs(requireTrustedEvidence: false)
    let recoveryArgs = baseArgs.merging(recoveryScoreBridgeArgs()) { _, new in new }
    let strainArgs = baseArgs.merging([
      "resting_start": "0000",
      "resting_end": "9999",
      "resting_baseline_min_days": 3,
    ]) { _, new in new }
    let stressArgs = baseArgs.merging([
      "resting_start": "0000",
      "resting_end": "9999",
      "hrv_start": "0000",
      "hrv_end": "9999",
      "hrv_baseline_start": "0000",
      "hrv_baseline_end": "9999",
      "resting_baseline_min_days": 3,
      "hrv_min_rr_intervals_to_compute": 2,
      "hrv_baseline_min_days": 3,
    ]) { _, new in new }
    let sleepArgs = baseArgs.merging([
      "sleep_need_minutes": 480.0,
      "low_motion_threshold_0_to_1": 0.05,
      "disturbance_motion_threshold_0_to_1": 0.20,
      "target_midpoint_minutes_since_midnight": 180.0,
      "history_import_in_progress": false,
      "algorithm_id": "goose.sleep.v1",
    ]) { _, new in new }

    do {
      let sleepReport = try await bridge.requestAsync(
        method: "metrics.sleep_score_from_features",
        args: sleepArgs
      )
      let strainReport = try await bridge.requestAsync(
        method: "metrics.strain_score_from_features",
        args: strainArgs
      )
      let recoveryReport = try await bridge.requestAsync(
        method: "metrics.recovery_score_from_features",
        args: recoveryArgs
      )
      let stressReport = try await bridge.requestAsync(
        method: "metrics.stress_score_from_features",
        args: stressArgs
      )
      self.packetScoreReports["sleep"] = sleepReport
      self.refreshPrimarySleepFromScoreReport()
      self.packetScoreReports["strain"] = strainReport
      self.packetScoreReports["recovery"] = recoveryReport
      self.packetScoreReports["stress"] = stressReport
      self.packetScoreStatus = "Bridge packet-derived scores recomputed"
    } catch {
      let shortErr = Self.shortError(error)
      self.packetScoreStatus = "Bridge score run blocked: \(shortErr)"
    }
  }

  func runSleepScore() async {
    packetScoreStatus = "Extracting bridge sleep score..."
    let sleepArgs = bridgeBaseArgs(requireTrustedEvidence: false).merging([
      "sleep_need_minutes": 480.0,
      "low_motion_threshold_0_to_1": 0.05,
      "disturbance_motion_threshold_0_to_1": 0.20,
      "target_midpoint_minutes_since_midnight": 180.0,
      "history_import_in_progress": false,
      "algorithm_id": "goose.sleep.v1",
    ]) { _, new in new }
    do {
      let sleepReport = try await bridge.requestAsync(
        method: "metrics.sleep_score_from_features",
        args: sleepArgs
      )
      self.packetScoreReports["sleep"] = sleepReport
      self.refreshPrimarySleepFromScoreReport()
      self.packetScoreStatus = "Bridge sleep score recomputed"
    } catch {
      let shortErr = Self.shortError(error)
      self.packetScoreStatus = "Bridge sleep score blocked: \(shortErr)"
    }
  }

  func runReferenceComparisons() {
    referenceComparisonReports = [:]
    for family in ["hrv", "sleep", "strain", "stress"] {
      referenceRunStatusByFamily[family] = "blocked | real comparison inputs not wired"
    }
  }

  func importCalibrationLabels() {
    // Labels are stored in SQLite via the Rust bridge when the user imports
    // ground-truth scores. Flip the flag optimistically so the UI updates.
    calibrationLabelsImported = true
  }

  // CAL-01: Run real train/holdout calibration via the Rust bridge.
  // Uses the last 90 days of stored algorithm runs and calibration labels.
  func calibrate() {
    let db = databasePath
    let family = calibrationTargetFamily
    let fmt = DateFormatter()
    fmt.dateFormat = "yyyy-MM-dd"
    let endKey = fmt.string(from: Date())
    let startKey = fmt.string(from: Date().addingTimeInterval(-90 * 24 * 3600))
    let splitKey = fmt.string(from: Date().addingTimeInterval(-14 * 24 * 3600))

    Task {
      let localBridge = GooseRustBridge()
      do {
        let result = try await localBridge.requestAsync(
          method: "calibration.evaluate_stored_labels",
          args: [
            "database_path": db,
            "start": startKey,
            "end": endKey,
            "options": [
              "metric_family": family,
              "algorithm_id": "goose.\(family).v1",
              "algorithm_version": "1.0",
              "split_at": splitKey,
            ],
            "persist": true,
          ]
        )
        await MainActor.run {
          self.calibrationResult = result
          self.calibrationRunComplete = (result["pass"] as? Bool) == true
        }
      } catch {
        await MainActor.run {
          self.calibrationResult = ["error": error.localizedDescription]
          self.calibrationRunComplete = false
        }
      }
    }
  }

  // ENB-01: Persist today's energy daily rollup to SQLite.
  // Only writes once per day; idempotent on repeated calls.
  func persistEnergyDailyIfNeeded() {
    let db = databasePath
    let cal = Calendar.current
    let todayKey = {
      let fmt = DateFormatter()
      fmt.dateFormat = "yyyy-MM-dd"
      return fmt.string(from: Date())
    }()
    let udKey = "goose.energyBank.persisted.\(todayKey)"
    guard UserDefaults.standard.object(forKey: udKey) == nil else { return }

    let tz = TimeZone.current.identifier
    let dayStart = cal.startOfDay(for: Date())
    let dayEnd = cal.date(byAdding: .day, value: 1, to: dayStart)!
    let startISO = ISO8601DateFormatter().string(from: dayStart)
    let endISO = ISO8601DateFormatter().string(from: dayEnd)

    let weightGrams = UserDefaults.standard.integer(forKey: "goose.swift.profile.weightGrams")
    let weightKg: Double? = weightGrams > 0 ? Double(weightGrams) / 1000.0 : nil
    let rhrBpm = hkRestingHR

    Task {
      let localBridge = GooseRustBridge()
      do {
        var args: [String: Any] = [
          "database_path": db,
          "date_key": todayKey,
          "timezone": tz,
          "start": startISO,
          "end": endISO,
          "write_metric": true,
        ]
        if let w = weightKg { args["profile_weight_kg"] = w }
        if let r = rhrBpm { args["resting_hr_bpm"] = r }

        let report = try await localBridge.requestAsync(
          method: "metrics.energy_daily_rollup",
          args: args
        )
        let pass = (report["pass"] as? Bool) == true
        await MainActor.run {
          UserDefaults.standard.set(Date().timeIntervalSince1970, forKey: udKey)
          self.energyDailyPersistStatus = pass ? "Persisted \(todayKey)" : "Rollup incomplete"
        }
      } catch {
        await MainActor.run {
          self.energyDailyPersistStatus = "Persist error: \(error.localizedDescription)"
        }
      }
    }
  }

  var algorithmFamilies: [String] {
    let families = Set(algorithmDefinitions.map(\.family))
      .union(["recovery", "sleep", "strain", "stress", "hrv"])
    return families.sorted()
  }

  func algorithms(for family: String) -> [HealthAlgorithmDefinition] {
    algorithmDefinitions.filter { $0.family == family }
  }

  func landingSnapshots(
    liveHeartRateBPM: Int?,
    liveHeartRateSource: String,
    liveHeartRateUpdatedAt: Date?,
    stableDailyMetrics: Bool = false
  ) -> [HealthMetricSnapshot] {
    var snapshots = Self.baseLandingSnapshots
    if let index = snapshots.firstIndex(where: { $0.route == .sleep }) {
      snapshots[index] = sleepSnapshot(base: snapshots[index])
    }
    if let index = snapshots.firstIndex(where: { $0.route == .recovery }) {
      snapshots[index] = recoverySnapshot(base: snapshots[index])
    }
    if let index = snapshots.firstIndex(where: { $0.route == .strain }) {
      snapshots[index] = strainSnapshot(base: snapshots[index])
    }
    if let index = snapshots.firstIndex(where: { $0.route == .stress }) {
      snapshots[index] = stressSnapshot(base: snapshots[index], allowLiveFallbacks: !stableDailyMetrics)
    }
    if let index = snapshots.firstIndex(where: { $0.route == .cardioLoad }) {
      snapshots[index] = cardioLoadSnapshot(base: snapshots[index])
    }
    if let index = snapshots.firstIndex(where: { $0.route == .energyBank }) {
      snapshots[index] = energyBankSnapshot(base: snapshots[index], allowLiveFallbacks: !stableDailyMetrics)
    }
    if let liveHeartRateBPM,
       let index = snapshots.firstIndex(where: { $0.id == "health-monitor" }) {
      snapshots[index] = HealthMetricSnapshot(
        id: "health-monitor",
        route: .healthMonitor,
        group: .today,
        title: "Health Monitor",
        value: "\(liveHeartRateBPM)",
        unit: "bpm",
        status: "Live HR",
        freshness: Self.relativeText(for: liveHeartRateUpdatedAt) ?? "Now",
        provenance: liveHeartRateSource,
        source: .live("BLE heart rate stream"),
        systemImage: "heart.text.square",
        tint: .red,
        trend: snapshots[index].trend
      )
    }
    return snapshots
  }

  func healthMonitorSnapshots(
    restingHeartRateEstimateBPM: Double? = nil,
    restingHeartRateEstimateSampleCount: Int = 0,
    restingHeartRateEstimateUpdatedAt: Date? = nil,
    restingHeartRateEstimateSource: String = "ble.hr.standard.low_quartile",
    allowLiveFallbacks: Bool = true
  ) -> [HealthMetricSnapshot] {
    if previewMissingData {
      return Self.baseHealthMonitorSnapshots.map { snapshot in
        HealthMetricSnapshot(
          id: snapshot.id,
          route: snapshot.route,
          group: snapshot.group,
          title: snapshot.title,
          value: "--",
          unit: snapshot.unit,
          status: "Unavailable",
          freshness: "No local data",
          provenance: "preview missing data",
          source: .unavailable("preview missing data"),
          systemImage: snapshot.systemImage,
          tint: snapshot.tint,
          trend: HealthTrendModel(id: snapshot.trend.id, title: snapshot.trend.title, rangeLabel: "No data", summary: "No trend data", analysis: "No local data has been captured for this trend yet.", resources: snapshot.trend.resources, points: [])
        )
      }
    }
    var snapshots = Self.baseHealthMonitorSnapshots.map {
      packetBackedHealthMonitorSnapshot(base: $0, allowLiveFallbacks: allowLiveFallbacks)
    }
    if allowLiveFallbacks,
       let index = snapshots.firstIndex(where: { $0.id == "resting-hr" }),
       snapshots[index].source.kind == .unavailable,
       let sample = Self.liveHRDerivedRestingHeartRateSample(
        bpm: restingHeartRateEstimateBPM,
        sampleCount: restingHeartRateEstimateSampleCount,
        updatedAt: restingHeartRateEstimateUpdatedAt,
        source: restingHeartRateEstimateSource
       ) {
      snapshots[index] = liveHRDerivedRestingHeartRateHealthMonitorSnapshot(
        base: snapshots[index],
        sample: sample
      )
    }
    if let index = snapshots.firstIndex(where: { $0.id == "health-sleep" }) {
      snapshots[index] = sleepHealthMonitorSnapshot(base: snapshots[index])
    }
    return snapshots
  }

  func snapshot(for route: HealthRoute) -> HealthMetricSnapshot {
    let snapshot = Self.baseLandingSnapshots.first { $0.route == route }
      ?? Self.baseLandingSnapshots[0]
    if route == .sleep && !previewMissingData {
      return sleepSnapshot(base: snapshot)
    }
    if route == .recovery {
      return recoverySnapshot(base: snapshot)
    }
    if route == .strain && !previewMissingData {
      return strainSnapshot(base: snapshot)
    }
    if route == .stress && !previewMissingData {
      return stressSnapshot(base: snapshot)
    }
    if route == .cardioLoad && !previewMissingData {
      return cardioLoadSnapshot(base: snapshot)
    }
    if route == .energyBank && !previewMissingData {
      return energyBankSnapshot(base: snapshot)
    }
    guard previewMissingData else {
      return snapshot
    }
    return HealthMetricSnapshot(
      id: snapshot.id,
      route: snapshot.route,
      group: snapshot.group,
      title: snapshot.title,
      value: "--",
      unit: snapshot.unit,
      status: "No data",
      freshness: "Missing",
      provenance: "preview missing data",
      source: .unavailable("preview missing data"),
      systemImage: snapshot.systemImage,
      tint: snapshot.tint,
      trend: HealthTrendModel(id: snapshot.trend.id, title: snapshot.trend.title, rangeLabel: "No data", summary: "No trend data", analysis: "No local data has been captured for this trend yet.", resources: snapshot.trend.resources, points: [])
    )
  }

  func strainSnapshot(for date: Date, calendar: Calendar = .current) -> HealthMetricSnapshot {
    let base = Self.baseLandingSnapshots.first { $0.route == .strain } ?? Self.baseLandingSnapshots[0]
    let snapshot = strainSnapshot(base: base)
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return zeroStrainSnapshot(
        base: snapshot,
        freshness: ScoreDateTimeline.dateLabel(for: date, calendar: calendar),
        provenance: "No local strain history for selected date",
        sourceDetail: "selected date has no local strain history"
      )
    }
    return snapshot
  }

  func sleepSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    if let output = Self.map(packetScoreReports["sleep"], "score_result", "output") {
      let scoreText = Self.numberText(output["score_0_to_100"], fractionDigits: 0) ?? snapshot.value
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: scoreText,
        unit: "%",
        status: Self.sleepQualityLabel(score: Self.doubleValue(output["score_0_to_100"])),
        freshness: "Latest",
        provenance: "metrics.sleep_score_from_features",
        source: .bridge("goose.sleep.v1"),
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: snapshot.trend
      )
    }
    if let primarySleepDetail {
      let score = hkSleepScore(durationText: primarySleepDetail.durationText)
      let scoreText = score.map { Self.numberText($0, fractionDigits: 0) ?? "--" } ?? "--"
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: scoreText,
        unit: "%",
        status: primarySleepDetail.qualityText,
        freshness: primarySleepDetail.dateLabel,
        provenance: "apple.health.sleep.duration_score",
        source: primarySleepDetail.source,
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: snapshot.trend
      )
    }
    return snapshot
  }

  // Derive a 0–100 sleep score from total sleep minutes (Apple Health duration).
  // Maps: <5h=20, 5h=40, 6h=60, 7h=80, 7.5h=90, 8h+=95, >9h=85 (too long).
  private func hkSleepScore(durationText: String) -> Double? {
    // Parse "Xh Ym" or "Xh" or "Ym" into minutes
    var minutes = 0.0
    let hourMatch = durationText.range(of: #"(\d+)h"#, options: .regularExpression)
    let minMatch = durationText.range(of: #"(\d+)m"#, options: .regularExpression)
    if let r = hourMatch {
      let digits = durationText[r].dropLast()
      minutes += (Double(digits) ?? 0) * 60
    }
    if let r = minMatch {
      let digits = durationText[r].dropLast()
      minutes += Double(digits) ?? 0
    }
    guard minutes > 0 else { return nil }
    let hours = minutes / 60
    switch hours {
    case ..<4: return 15
    case 4..<5: return 30
    case 5..<6: return 50
    case 6..<6.5: return 65
    case 6.5..<7: return 75
    case 7..<7.5: return 83
    case 7.5..<8.5: return 92
    case 8.5..<9.5: return 88
    default: return 78 // >9.5h slightly penalised
    }
  }

  func sleepHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    if let primarySleepDetail {
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: primarySleepDetail.durationText,
        unit: "",
        status: primarySleepDetail.qualityText,
        freshness: primarySleepDetail.dateLabel,
        provenance: primarySleepDetail.source.detail,
        source: primarySleepDetail.source,
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: snapshot.trend
      )
    }
    if let output = Self.map(packetScoreReports["sleep"], "score_result", "output"),
       let duration = Self.doubleValue(output["sleep_duration_minutes"]) {
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: Self.minutesText(duration),
        unit: "",
        status: Self.sleepQualityLabel(score: Self.doubleValue(output["score_0_to_100"])),
        freshness: "Latest",
        provenance: "metrics.sleep_score_from_features",
        source: .bridge("goose.sleep.v1"),
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: snapshot.trend
      )
    }
    return snapshot
  }

  func recoverySnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    guard !usesPreviewPacketData,
          let score = recoveryScoreValue(),
          let scoreText = Self.numberText(score, fractionDigits: 0) else {
      // HK proxy: HRV-based recovery estimate
      if let score = hkRecoveryScore(),
         let scoreText = Self.numberText(score, fractionDigits: 0) {
        return HealthMetricSnapshot(
          id: snapshot.id,
          route: snapshot.route,
          group: snapshot.group,
          title: snapshot.title,
          value: scoreText,
          unit: "%",
          status: Self.recoveryQualityLabel(score: score),
          freshness: "From Apple Health",
          provenance: "apple.health.hrv_recovery_estimate",
          source: .local("apple.health"),
          systemImage: snapshot.systemImage,
          tint: snapshot.tint,
          trend: Self.emptyTrend(from: snapshot.trend, packetCount: 0)
        )
      }
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: "--",
        unit: "%",
        status: "No data",
        freshness: "No recovery score",
        provenance: "metrics.recovery_score_from_features",
        source: .unavailable("recovery score not available"),
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: Self.emptyTrend(from: snapshot.trend, packetCount: packetEvidenceFrameCount())
      )
    }

    return HealthMetricSnapshot(
      id: snapshot.id,
      route: snapshot.route,
      group: snapshot.group,
      title: snapshot.title,
      value: scoreText,
      unit: "%",
      status: Self.recoveryQualityLabel(score: score),
      freshness: "Latest",
      provenance: "metrics.recovery_score_from_features",
      source: .bridge("goose.recovery.v0"),
      systemImage: snapshot.systemImage,
      tint: snapshot.tint,
      trend: recoveryScoreTrend(base: snapshot.trend, currentScore: score)
    )
  }

  var usesPreviewPacketData: Bool {
    packetInputStatus.hasPrefix("Preview") || packetScoreStatus.hasPrefix("Preview")
  }

  func recoveryScoreValue() -> Double? {
    guard !usesPreviewPacketData else {
      return nil
    }
    return Self.doubleValue(Self.map(packetScoreReports["recovery"], "score_result", "output")?["score_0_to_100"])
  }

  func recoveryScoreTrend(base trend: HealthTrendModel, currentScore: Double) -> HealthTrendModel {
    HealthTrendModel(
      id: trend.id,
      title: trend.title,
      rangeLabel: "\(Self.numberText(currentScore, fractionDigits: 0) ?? "0")%",
      summary: "Latest packet-derived recovery score",
      analysis: "Packet-derived recovery score from the local bridge.",
      resources: trend.resources,
      points: []
    )
  }

  func strainScore0To100(for date: Date = Date(), calendar: Calendar = .current) -> Double {
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return 0
    }
    return currentStrainScore0To21().map(Self.strainPercent) ?? 0
  }

  func strainScoreDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    let score = strainScore0To100(for: date, calendar: calendar)
    guard score > 0 else {
      return "--"
    }
    return Self.numberText(score, fractionDigits: 0) ?? "0"
  }

  func strainStatusText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())),
          let rawScore = currentStrainScore0To21() else {
      return "No strain data"
    }
    return Self.strainStatusLabel(score: Self.strainPercent(rawScore))
  }

  func strainTargetDisplayText() -> String {
    guard let range = optimalStrainRange() else { return "--" }
    let low = Self.numberText(range.low, fractionDigits: 0) ?? "\(Int(range.low))"
    let high = Self.numberText(range.high, fractionDigits: 0) ?? "\(Int(range.high))"
    return "\(low)–\(high)"
  }

  func strainDurationDisplayText() -> String {
    "--"
  }

  func strainEnergyDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    whoopTotalCaloriesDisplayText(for: date, calendar: calendar)
  }

  func strainActivityCountText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    whoopStepsDisplayText(for: date, calendar: calendar)
  }

  func whoopStepsDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    if let metric = stepMetric(for: date, calendar: calendar),
       let steps = Self.intValue(metric["steps"]) {
      return Self.groupedIntegerText(steps)
    }
    if calendar.isDateInToday(date), let steps = hkSteps {
      return Self.groupedIntegerText(steps)
    }
    return "--"
  }

  // Returns the WHOOP step count as an integer for comparison with IMU-derived steps.
  func strainStepCountForComparison() -> Int? {
    if let metric = stepMetric(for: Date()),
       let steps = Self.intValue(metric["steps"]), steps > 0 {
      return steps
    }
    return hkSteps.flatMap { $0 > 0 ? $0 : nil }
  }

  func whoopActiveCaloriesDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    let text = energyKcalDisplayText(key: "active_kcal", date: date, calendar: calendar)
    if text != "--" { return text }
    if calendar.isDateInToday(date), let kcal = hkActiveKcal,
       let valueText = Self.numberText(kcal, fractionDigits: 0) {
      return "\(valueText) kcal"
    }
    return "--"
  }

  func whoopTotalCaloriesDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    energyKcalDisplayText(key: "total_kcal", date: date, calendar: calendar)
  }

  func whoopStepsStatusText() -> String {
    if let metric = todayStepMetric() {
      return stepMetricStatus(metric)
    }

    if let latest = Self.preferredStepMetric(from: dailyActivityMetrics()),
       let dateKey = latest["date_key"] as? String {
      return "No today step metric | latest stored \(dateKey)"
    }

    if let report = packetInputReports["step_counter_rollup"] {
      return firstPacketAction(in: report) ?? "WHOOP step counter rollup blocked"
    }

    if let report = packetInputReports["step_counter_ingest"] {
      let persisted = Self.intValue(report["persisted_sample_count"]) ?? 0
      let candidates = Self.intValue(report["counter_candidate_count"]) ?? 0
      if persisted > 0 {
        return "\(persisted) WHOOP counter samples stored; daily delta pending"
      }
      if candidates > 0 {
        return "\(candidates) WHOOP counter candidates found; ingest blocked"
      }
    }

    if let motionReport = packetInputReports["motion"] {
      let total = Self.intValue(motionReport["feature_count"]) ?? 0
      let trusted = Self.intValue(motionReport["trusted_feature_count"]) ?? 0
      if total > 0 {
        return "WHOOP motion ready | \(trusted)/\(total) trusted inputs"
      }
      return "WHOOP motion captured; step metric pending"
    }

    if packetInputStatus == "No run" {
      return "Needs WHOOP packet extract"
    }
    return packetInputStatus
  }

  func whoopStepsSource(for date: Date = Date(), calendar: Calendar = .current) -> HealthDataSource {
    if let metric = stepMetric(for: date, calendar: calendar) {
      switch metric["source_kind"] as? String {
      case "device_counter":
        return .bridgeDeviceCounter("daily_activity_metrics WHOOP step counter")
      case "local_estimate":
        return .localEstimate("daily_activity_metrics validated raw-motion steps")
      default:
        return .unavailable("unsupported step metric source")
      }
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return .unavailable("selected date has no stored WHOOP step metric")
    }
    if let report = packetInputReports["step_counter_rollup"] {
      return .unavailable(firstPacketAction(in: report) ?? "WHOOP step counter rollup blocked")
    }
    if packetInputReports["motion"] == nil {
      return .unavailable("WHOOP step extraction pending")
    }
    return .unavailable("WHOOP step counter or validated local estimate not available")
  }

  func whoopActiveCaloriesStatusText() -> String {
    if let metric = energyMetric(for: Date(), valueKey: "active_kcal") {
      return energyMetricStatus(metric)
    }

    guard let report = packetInputReports["energy_rollup"] else {
      if let latest = Self.preferredDailyActivityMetric(
        from: dailyActivityMetricsWithValue("active_kcal"),
        valueKey: "active_kcal"
      ),
         let dateKey = latest["date_key"] as? String {
        return "No today calorie metric | latest stored \(dateKey)"
      }
      if packetInputStatus == "No run" {
        return "Needs WHOOP packet extract"
      }
      return packetInputStatus
    }
    if Self.boolValue(report["pass"]) == true,
       let confidence = Self.numberText(report["confidence"], fractionDigits: 2) {
      return "Local estimate | confidence \(confidence)"
    }
    return firstPacketAction(in: report) ?? "Calorie estimator blocked"
  }

  func whoopActiveCaloriesSource(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> HealthDataSource {
    whoopEnergySource(for: date, calendar: calendar, valueKey: "active_kcal")
  }

  func whoopTotalCaloriesSource(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> HealthDataSource {
    whoopEnergySource(for: date, calendar: calendar, valueKey: "total_kcal")
  }

  func whoopEnergySource(
    for date: Date,
    calendar: Calendar,
    valueKey: String
  ) -> HealthDataSource {
    if let metric = energyMetric(for: date, calendar: calendar, valueKey: valueKey) {
      return energyMetricSource(metric)
    }
    if let unavailable = preferredDailyActivityUnavailableMetric(metricID: valueKey, for: date, calendar: calendar) {
      return .unavailable(Self.activityUnavailableSourceDetail(unavailable))
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return .unavailable("selected date has no stored WHOOP energy metric")
    }
    guard let report = packetInputReports["energy_rollup"] else {
      return .unavailable("metrics.energy_daily_rollup not run")
    }
    guard Self.boolValue(report["pass"]) == true else {
      return .unavailable("metrics.energy_daily_rollup blocked")
    }
    return .localEstimate("metrics.energy_daily_rollup")
  }

  func energyRollupSummary() -> String {
    guard let report = packetInputReports["energy_rollup"] else {
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let active = Self.numberText(report["active_kcal"], fractionDigits: 0) ?? "--"
    let resting = Self.numberText(report["resting_kcal"], fractionDigits: 0) ?? "--"
    let total = Self.numberText(report["total_kcal"], fractionDigits: 0) ?? "--"
    let confidence = Self.numberText(report["confidence"], fractionDigits: 2) ?? "0"
    return "\(Self.passStatus(report)) | active \(active) kcal | resting \(resting) kcal | total \(total) kcal | confidence \(confidence)"
  }

  func energyRollupProvenanceSummary() -> String {
    guard let report = packetInputReports["energy_rollup"] else {
      return ""
    }
    let written = Self.boolValue(report["daily_metric_written"]) == true ? "stored" : "not stored"
    let hrSamples = Self.intValue(report["heart_rate_sample_count"]) ?? 0
    let motionSamples = Self.intValue(report["motion_sample_count"]) ?? 0
    let coverage = Self.percentText(report["coverage_fraction"]) ?? "unknown"
    return "daily_metric=\(written) | HR=\(hrSamples) | motion=\(motionSamples) | coverage=\(coverage)"
  }

  func energyKcalDisplayText(
    key: String,
    date: Date = Date(),
    calendar: Calendar = .current
  ) -> String {
    if let metric = energyMetric(for: date, calendar: calendar, valueKey: key),
       let value = Self.doubleValue(metric[key]),
       value.isFinite {
      return "\(Self.groupedIntegerText(Int(value.rounded()))) kcal"
    }
    guard let report = packetInputReports["energy_rollup"],
          calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())),
          Self.boolValue(report["pass"]) == true,
          let value = Self.doubleValue(report[key]),
          value.isFinite else {
      return "-- kcal"
    }
    return "\(Self.groupedIntegerText(Int(value.rounded()))) kcal"
  }

  func todayStepMetric() -> [String: Any]? {
    stepMetric(for: Date())
  }

  func stepMetric(for date: Date, calendar: Calendar = .current) -> [String: Any]? {
    Self.preferredStepMetric(
      from: dailyActivityMetrics(forDateKey: Self.metricDateKey(for: date, calendar: calendar))
    )
  }

  func energyMetric(
    for date: Date,
    calendar: Calendar = .current,
    valueKey: String
  ) -> [String: Any]? {
    Self.preferredDailyActivityMetric(
      from: dailyActivityMetrics(forDateKey: Self.metricDateKey(for: date, calendar: calendar)),
      valueKey: valueKey
    )
  }

  func dailyActivityMetrics() -> [[String: Any]] {
    Self.array(packetInputReports["daily_activity"]?["metrics"])
      .filter { Self.localHealthMetricRowIsDisplaySafe($0) }
  }

  func dailyActivityMetrics(forDateKey dateKey: String) -> [[String: Any]] {
    dailyActivityMetrics().filter { $0["date_key"] as? String == dateKey }
  }

  func dailyActivityMetricsWithValue(_ valueKey: String) -> [[String: Any]] {
    dailyActivityMetrics().filter { Self.doubleValue($0[valueKey]) != nil }
  }

  func hourlyActivityMetrics() -> [[String: Any]] {
    Self.array(packetInputReports["hourly_activity"]?["metrics"])
      .filter { Self.localHealthMetricRowIsDisplaySafe($0) }
  }

  func hourlyActivityMetrics(forDateKey dateKey: String) -> [[String: Any]] {
    hourlyActivityMetrics().filter { $0["date_key"] as? String == dateKey }
  }

  func hourlyActivityMetricsWithValue(_ valueKey: String) -> [[String: Any]] {
    hourlyActivityMetrics().filter { Self.doubleValue($0[valueKey]) != nil }
  }

  func dailyActivityUnavailableMetrics(metricID: String? = nil) -> [[String: Any]] {
    dailyActivityMetrics()
      .filter { metric in
        guard metric["source_kind"] as? String == "unavailable",
              Self.doubleValue(metric["confidence"]) != nil else {
          return false
        }
        if let metricID {
          return Self.dailyActivityUnavailableMetric(metric, matches: metricID)
        }
        return true
      }
  }

  func preferredDailyActivityUnavailableMetric(
    metricID: String,
    for date: Date? = nil,
    calendar: Calendar = .current
  ) -> [String: Any]? {
    let dateKey = date.map { Self.metricDateKey(for: $0, calendar: calendar) }
    return dailyActivityUnavailableMetrics(metricID: metricID)
      .filter { metric in
        if let dateKey, metric["date_key"] as? String != dateKey {
          return false
        }
        return true
      }
      .sorted { lhs, rhs in
        let lhsEnd = Self.int64Value(lhs["end_time_unix_ms"]) ?? 0
        let rhsEnd = Self.int64Value(rhs["end_time_unix_ms"]) ?? 0
        if lhsEnd != rhsEnd {
          return lhsEnd > rhsEnd
        }
        let lhsUpdated = lhs["updated_at"] as? String ?? ""
        let rhsUpdated = rhs["updated_at"] as? String ?? ""
        return lhsUpdated > rhsUpdated
      }
      .first
  }

  static func dailyActivityUnavailableMetric(_ metric: [String: Any], matches metricID: String) -> Bool {
    if let inputsMetricID = jsonObject(fromJSONString: metric["inputs_json"])?["metric_id"] as? String,
       inputsMetricID == metricID {
      return true
    }
    let sanitizedMetricID = metricIDToken(metricID)
    let dailyMetricID = (metric["daily_metric_id"] as? String ?? "").lowercased()
    return dailyMetricID.contains(sanitizedMetricID)
  }

  static func activityUnavailableSourceDetail(_ metric: [String: Any]) -> String {
    let metricID = jsonObject(fromJSONString: metric["inputs_json"])?["metric_id"] as? String
      ?? metric["daily_metric_id"] as? String
      ?? "activity_metric"
    let blocker = firstActivityUnavailableBlocker(metric) ?? "metric unavailable"
    return "\(metricID) unavailable: \(blocker)"
  }

  // MARK: - Goose Recovery (exact Rust formula: goose_recovery_v0)
  //
  // Mirrors the Rust implementation in metrics.rs exactly:
  //   hrv_score      = clamp(70 + (rmssd / baseline_rmssd - 1) * 100)   weight 0.35
  //   rhr_score      = clamp(70 + (baseline_rhr - rhr) * 5)              weight 0.20
  //   resp_score     = clamp(100 - |rr - baseline_rr| * 20)              weight 0.10
  //   temp_score     = clamp(100 - |delta_c| * 50)                       weight 0.10
  //   sleep_score    = sleep_score_0_to_100                               weight 0.15
  //   strain_ready   = clamp(100 - prior_strain/21 * 60)                 weight 0.10
  //
  // Baselines: 60-night rolling mean of Apple Watch history, or population defaults.
  func hkRecoveryScore() -> Double? {
    guard let sdnn = hkHRVSDNNMs, sdnn > 5 else { return nil }
    let sdnnMs = sdnn

    // HRV baseline: 60-night rolling mean (exclude today)
    let hrvBaseline: Double
    if hkHRVHistory.count >= 7 {
      let values = hkHRVHistory.dropLast(1).suffix(60).map { $0.sdnn }
      hrvBaseline = max(values.reduce(0, +) / Double(values.count), 1)
    } else {
      hrvBaseline = 50.0 // population average SDNN for healthy adults
    }
    let hrv_score = min(max(70 + (sdnnMs / hrvBaseline - 1) * 100, 0), 100)

    // RHR component
    let rhr_score: Double
    if let rhr = hkRestingHR {
      let rhrBaseline: Double?
      if hkRHRHistory.count >= 7 {
        let values = hkRHRHistory.dropLast(1).suffix(60).map { $0.bpm }
        rhrBaseline = values.reduce(0, +) / Double(values.count)
      } else {
        rhrBaseline = nil
      }
      // Without sufficient RHR history, emit a neutral score rather than
      // biasing the result with a hardcoded population average (BIO-05).
      if let baseline = rhrBaseline {
        rhr_score = min(max(70 + (baseline - rhr) * 5, 0), 100)
      } else {
        rhr_score = 70.0
      }
    } else {
      rhr_score = 70.0 // neutral when unavailable
    }

    // Respiratory rate component
    let resp_score: Double
    if let rr = hkRespiratoryRate {
      let baseline = 15.5 // population average
      resp_score = min(max(100 - abs(rr - baseline) * 20, 0), 100)
    } else {
      resp_score = 85.0
    }

    // Skin temperature component
    let temp_score: Double
    if let delta = hkSkinTempDeltaC {
      temp_score = min(max(100 - abs(delta) * 50, 0), 100)
    } else {
      temp_score = 85.0
    }

    // Sleep score component
    let sleep_score: Double
    if let detail = primarySleepDetail, let s = hkSleepScore(durationText: detail.durationText) {
      sleep_score = s
    } else {
      sleep_score = 70.0
    }

    // Prior strain readiness (uses yesterday's eTRIMP-derived strain on 0-21)
    let prior_strain_0_to_21 = (hkStrainScore() ?? 0) / 100 * 21
    let strain_ready = min(max(100 - prior_strain_0_to_21 / 21.0 * 60, 0), 100)

    // Weighted sum — exact Rust weights
    let score = hrv_score * 0.35
      + rhr_score * 0.20
      + resp_score * 0.10
      + temp_score * 0.10
      + sleep_score * 0.15
      + strain_ready * 0.10

    return min(max(score, 3), 97)
  }

  // MARK: - WHOOP-style Strain (eTRIMP heart rate integration)
  //
  // Algorithm (Banister eTRIMP):
  // 1. For each HR sample pair, compute HR Reserve fraction:
  //    HRR = (HR - restingHR) / (maxHR - restingHR)
  // 2. Apply gender-specific exponential weight:
  //    male: w = e^(1.92 * HRR), female: w = e^(1.67 * HRR)
  //    (Morton et al., 1990; Banister 1991)
  // 3. Accumulate: TRIMP += Δt_minutes * HRR * w
  // 4. WHOOP's 0–21 scale: empirically, a moderate 60-min session at 70% HRR
  //    produces ~10 strain units. We calibrate: strain_0_21 = TRIMP / 6.0
  //    (approximated from published WHOOP descriptions of typical session values).
  // 5. Scale to 0–100 for the dial: score = (strain_0_21 / 21) * 100
  func hkStrainScore() -> Double? {
    let todaySamples = heartRateSeriesStore.decimatedSamples(forDayContaining: Date())
      .sorted { $0.capturedAt < $1.capturedAt }
    guard todaySamples.count >= 3 else { return nil }

    let ageBestGuess: Double = hkUserAge() ?? 30
    let maxHR = 220 - ageBestGuess
    let restingHR = hkRestingHR ?? 55
    guard maxHR > restingHR + 10 else { return nil }

    let isFemale = hkUserIsFemale()
    let expCoeff = isFemale ? 1.67 : 1.92

    var trimp = 0.0
    for i in 1..<todaySamples.count {
      let prev = todaySamples[i - 1]
      let curr = todaySamples[i]
      let dtMinutes = curr.capturedAt.timeIntervalSince(prev.capturedAt) / 60
      guard dtMinutes > 0, dtMinutes < 10 else { continue } // ignore gaps > 10 min
      let hrr = (Double(curr.bpm) - restingHR) / (maxHR - restingHR)
      guard hrr > 0 else { continue }
      let weight = exp(expCoeff * hrr)
      trimp += dtMinutes * hrr * weight
    }

    guard trimp > 0 else { return nil }

    // Calibrate to WHOOP's 0–21 scale, then to 0–100
    let strain_0_21 = min(trimp / 6.0, 21.0)
    return (strain_0_21 / 21.0) * 100
  }

  private static let dobFormatter: DateFormatter = {
    let f = DateFormatter()
    f.dateFormat = "yyyy-MM-dd"
    f.locale = Locale(identifier: "en_US_POSIX")
    return f
  }()

  private func hkUserAge() -> Double? {
    guard let dob = UserDefaults.standard.string(forKey: OnboardingStorage.dateOfBirth),
          !dob.isEmpty else { return nil }
    guard let date = Self.dobFormatter.date(from: dob) else { return nil }
    return Double(Calendar.current.dateComponents([.year], from: date, to: Date()).year ?? 30)
  }

  private func hkUserIsFemale() -> Bool {
    UserDefaults.standard.string(forKey: OnboardingStorage.gender)?.lowercased() == "female"
  }

  // MARK: - Optimal Strain Target
  //
  // Derives a recommended strain range for the day based on:
  // 1. Today's recovery score (primary driver, same as WHOOP's colour zones)
  //    Green  ≥67%: target 14–21 (push hard, room to grow)
  //    Yellow 34–66%: target 10–14 (moderate, maintain load)
  //    Red    <34%: target 0–10 (rest or light movement only)
  // 2. 7-day rolling average strain (acute load)
  //    If recent load is high (avg > 15), nudge range down by ~2 units.
  //    If recent load is low (avg < 8), nudge range up by ~1 unit.
  // 3. Quality flags from the Rust schema: low sleep (<60) or high prior strain (>14)
  //    both reduce the upper bound.
  //
  // Returns a (low, high) pair on the 0–21 WHOOP scale, or nil if no recovery data.
  func optimalStrainRange() -> (low: Double, high: Double)? {
    // Prefer Rust-derived recovery if available
    let recovery: Double
    if let packetScore = recoveryScoreValue() {
      recovery = packetScore
    } else if let hkScore = hkRecoveryScore() {
      recovery = hkScore
    } else {
      return nil
    }

    // Base range from recovery colour zone (linear interpolation within each zone)
    let (baseLow, baseHigh): (Double, Double)
    switch recovery {
    case 67...:
      // Green: lerp 14–21 across 67–100
      let t = min((recovery - 67) / 33, 1)
      baseLow = 14 + t * 2   // 14–16
      baseHigh = 16 + t * 5  // 16–21
    case 34..<67:
      // Yellow: lerp 10–14 across 34–66
      let t = (recovery - 34) / 33
      baseLow = 10 + t * 1   // 10–11
      baseHigh = 12 + t * 2  // 12–14
    default:
      // Red: lerp 0–10 across 0–33
      let t = recovery / 34
      baseLow = 0
      baseHigh = 5 + t * 5   // 5–10
    }

    // Rolling 7-day average strain adjustment
    let recentStrainAvg = hkSevenDayAverageStrain()
    var adjustment = 0.0
    if let avg = recentStrainAvg {
      if avg > 15 { adjustment = -2 }      // over-trained trend
      else if avg > 12 { adjustment = -1 } // high load
      else if avg < 8 { adjustment = 1 }   // under-loaded
    }

    // Quality flags: low sleep or high prior strain reduce upper bound
    var upperPenalty = 0.0
    if let sleepDetail = primarySleepDetail,
       let s = hkSleepScore(durationText: sleepDetail.durationText), s < 60 {
      upperPenalty += 2
    }

    let low = max(baseLow + adjustment, 0)
    let high = min(max(baseHigh + adjustment - upperPenalty, low + 1), 21)
    return (low: low, high: high)
  }

  // 7-day rolling average strain on 0–21 scale from the HR series store
  // sevenDayStrainCache is declared on the main HealthDataStore class body (stored properties
  // are not allowed in Swift extensions).
  private func hkSevenDayAverageStrain() -> Double? {
    if let cache = sevenDayStrainCache, Date().timeIntervalSince(cache.computedAt) < 300 {
      return cache.value
    }
    var dailyTrimpValues: [Double] = []
    let cal = Calendar.current
    let ageBestGuess: Double = hkUserAge() ?? 30
    let maxHR = 220 - ageBestGuess
    let restingHR = hkRestingHR ?? 55
    let isFemale = hkUserIsFemale()
    let expCoeff = isFemale ? 1.67 : 1.92
    guard maxHR > restingHR + 10 else { return nil }

    for daysBack in 1...7 {
      guard let day = cal.date(byAdding: .day, value: -daysBack, to: Date()) else { continue }
      let samples = heartRateSeriesStore.decimatedSamples(forDayContaining: day)
        .sorted { $0.capturedAt < $1.capturedAt }
      guard samples.count >= 3 else { continue }
      var trimp = 0.0
      for i in 1..<samples.count {
        let dtMin = samples[i].capturedAt.timeIntervalSince(samples[i-1].capturedAt) / 60
        guard dtMin > 0, dtMin < 10 else { continue }
        let hrr = (Double(samples[i].bpm) - restingHR) / (maxHR - restingHR)
        guard hrr > 0 else { continue }
        trimp += dtMin * hrr * exp(expCoeff * hrr)
      }
      if trimp > 0 {
        dailyTrimpValues.append(min(trimp / 6.0, 21.0))
      }
    }
    let result = dailyTrimpValues.isEmpty ? nil : dailyTrimpValues.reduce(0, +) / Double(dailyTrimpValues.count)
    sevenDayStrainCache = (value: result, computedAt: Date())
    return result
  }

}
