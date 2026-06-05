import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  // Run the packet-derived scores (sleep / strain / recovery / stress) off the main
  // thread, mirroring `runPacketInputs`, and publish on main. The bridge calls each take
  // seconds; running them synchronously froze the UI, which is why they used to sit
  // behind a manual button. With this they can be kicked off automatically.
  func runPacketScores(completion: (() -> Void)? = nil) {
    guard !packetScoreIsRunning else {
      packetScoreStatus = "Packet-derived score run already running..."
      completion?()
      return
    }
    let runID = UUID()
    packetScoreRunID = runID
    packetScoreIsRunning = true
    let databasePath = databasePath
    packetScoreStatus = "Computing packet-derived scores..."

    packetScoreQueue.async { [weak self] in
      let result = HealthDataStore.packetScoreBridgeReports(databasePath: databasePath)
      DispatchQueue.main.async { [weak self] in
        guard let self, self.packetScoreRunID == runID else {
          return
        }
        self.packetScoreIsRunning = false
        switch result {
        case .success(let reports):
          self.packetScoreReports = reports
          self.refreshPrimarySleepFromScoreReport()
          self.packetScoreStatus = "Bridge packet-derived scores recomputed"
        case .failure(let error):
          self.packetScoreStatus = "Bridge score run blocked: \(HealthDataStore.shortError(error))"
        }
        completion?()
      }
    }
  }

  func runPacketScoresIfNeeded() {
    guard packetScoreReports.isEmpty, packetScoreStatus == "No run", !packetScoreIsRunning else {
      return
    }
    runPacketScores()
  }

  // Off-main worker: builds its own bridge and runs every score method with the same
  // (entirely static) args the on-screen button used. Mirrors `packetInputBridgeReports`.
  nonisolated static func packetScoreBridgeReports(
    databasePath: String
  ) -> Result<[String: [String: Any]], Error> {
    let bridge = GooseRustBridge()
    let baseArgs: [String: Any] = [
      "database_path": databasePath,
      "start": "0000",
      "end": "9999",
      "min_owned_captures": 2,
      "require_trusted_evidence": false,
    ]
    do {
      var reports: [String: [String: Any]] = [:]
      reports["sleep"] = try bridge.request(
        method: "metrics.sleep_score_from_features",
        args: baseArgs.merging([
          "sleep_need_minutes": 480.0,
          "low_motion_threshold_0_to_1": 0.05,
          "disturbance_motion_threshold_0_to_1": 0.20,
          "target_midpoint_minutes_since_midnight": 180.0,
          "history_import_in_progress": false,
          "algorithm_id": "goose.sleep.v1",
        ]) { _, new in new }
      )
      reports["strain"] = try bridge.request(
        method: "metrics.strain_score_from_features",
        args: baseArgs.merging([
          "resting_start": "0000",
          "resting_end": "9999",
          "resting_baseline_min_days": 3,
        ]) { _, new in new }
      )
      reports["recovery"] = try bridge.request(
        method: "metrics.recovery_score_from_features",
        args: baseArgs.merging([
          "hrv_start": "0000",
          "hrv_end": "9999",
          "hrv_baseline_start": "0000",
          "hrv_baseline_end": "9999",
          "resting_start": "0000",
          "resting_end": "9999",
          "sleep_start": "0000",
          "sleep_end": "9999",
          "prior_strain_start": "0000",
          "prior_strain_end": "9999",
          "resting_baseline_min_days": 3,
          "hrv_min_rr_intervals_to_compute": 2,
          "hrv_baseline_min_days": 3,
          "sleep_need_minutes": 480.0,
          "low_motion_threshold_0_to_1": 0.05,
          "disturbance_motion_threshold_0_to_1": 0.20,
          "target_midpoint_minutes_since_midnight": 180.0,
          "prior_strain_resting_baseline_min_days": 3,
        ]) { _, new in new }
      )
      reports["stress"] = try bridge.request(
        method: "metrics.stress_score_from_features",
        args: baseArgs.merging([
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
      )
      return .success(reports)
    } catch {
      return .failure(error)
    }
  }

  func runSleepScore() {
    do {
      packetScoreReports["sleep"] = try sleepScoreReport(baseArgs: bridgeBaseArgs(requireTrustedEvidence: false))
      refreshPrimarySleepFromScoreReport()
      packetScoreStatus = "Bridge sleep score recomputed"
    } catch {
      packetScoreStatus = "Bridge sleep score blocked: \(Self.shortError(error))"
    }
  }

  func runReferenceComparisons() {
    referenceComparisonReports = [:]
    for family in ["hrv", "sleep", "strain", "stress"] {
      referenceRunStatusByFamily[family] = "blocked | real comparison inputs not wired"
    }
  }

  func importCalibrationLabels() {
    calibrationLabelsImported = true
  }

  func calibrate() {
    calibrationRunComplete = true
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
    return snapshot
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
    "--"
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
    guard let metric = stepMetric(for: date, calendar: calendar),
          let steps = Self.intValue(metric["steps"]) else {
      return "--"
    }
    return Self.groupedIntegerText(steps)
  }

  func whoopActiveCaloriesDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    energyKcalDisplayText(key: "active_kcal", date: date, calendar: calendar)
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

}
