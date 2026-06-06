import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  fileprivate static let relativeFormatter: RelativeDateTimeFormatter = {
    let f = RelativeDateTimeFormatter()
    f.unitsStyle = .short
    return f
  }()

  static func stressTrendStatus(score: Double) -> String {
    if score >= 55 {
      return "Above normal"
    }
    if score < 18 {
      return "Below normal"
    }
    return "Normal range"
  }

  static func energyBankStatusLabel(percent: Double?) -> String {
    guard let percent else {
      return "No data"
    }
    if percent >= 75 {
      return "Charged"
    }
    if percent >= 50 {
      return "Balanced"
    }
    if percent >= 25 {
      return "Draining"
    }
    return "Low"
  }

  static func clamp(_ value: Double, min lowerBound: Double, max upperBound: Double) -> Double {
    min(max(value, lowerBound), upperBound)
  }

  static func cardioLoadVisibleDayCount(for range: String) -> Int {
    switch range {
    case "7D":
      return 7
    case "3M":
      return 91
    case "6M":
      return 183
    case "1Y":
      return 365
    default:
      return 30
    }
  }

  static func cardioLoadDayStarts(
    count: Int,
    endingAt today: Date,
    calendar: Calendar
  ) -> [Date] {
    (0..<count).compactMap { index in
      calendar.date(byAdding: .day, value: index - count + 1, to: today)
    }
  }

  static func cardioLoadSessionIsUsable(_ session: [String: Any]) -> Bool {
    let syncStatus = session["sync_status"] as? String ?? ""
    return ["user_confirmed", "verified", "synced"].contains(syncStatus)
  }

  static func averageHeartRate(in samples: [HeartRateSamplePoint]) -> Double? {
    guard !samples.isEmpty else {
      return nil
    }
    let total = samples.reduce(0) { $0 + $1.bpm }
    return Double(total) / Double(samples.count)
  }

  static func cardioLoadTrainingStatus(
    acute: Double,
    chronic: Double,
    activityDayCount: Int
  ) -> String {
    guard activityDayCount >= 7, chronic >= 1 else {
      return "Calibrating"
    }
    if acute < 1 {
      return "Detraining"
    }
    let ratio = acute / max(chronic, 1)
    switch ratio {
    case ..<0.72:
      return "Detraining"
    case ..<1.12:
      return "Maintaining"
    case ..<1.34:
      return "Productive"
    case ..<1.52:
      return "Peaking"
    case ..<1.82:
      return "Fatigued"
    default:
      return "Overtraining"
    }
  }

  static func isLikelySleepWindow(_ date: Date, calendar: Calendar = .current) -> Bool {
    let hour = calendar.component(.hour, from: date)
    return hour < 7 || hour >= 23
  }

  func bridgeBaseArgs(requireTrustedEvidence: Bool) -> [String: Any] {
    [
      "database_path": databasePath,
      "start": "0000",
      "end": "9999",
      "min_owned_captures": 2,
      "require_trusted_evidence": requireTrustedEvidence,
    ]
  }

  func sleepScoreReport(baseArgs: [String: Any]) throws -> [String: Any] {
    try bridge.request(
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
  }

  func recoveryScoreBridgeArgs() -> [String: Any] {
    [
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
    ]
  }

  nonisolated static func shortError(_ error: Error) -> String {
    let text = String(describing: error)
    return text.count > 96 ? "\(text.prefix(96))..." : text
  }

  func currentRecoveryRespiratoryRateRPM() -> Double? {
    let value = recoveryProvidedVitalsValue("respiratory_rate_rpm")
    guard let value, value > 0 else {
      return nil
    }
    return value
  }

  static func unixMilliseconds(_ date: Date) -> Int64 {
    Int64((date.timeIntervalSince1970 * 1000).rounded())
  }

  static func jsonString(_ value: Any) -> String {
    guard JSONSerialization.isValidJSONObject(value),
          let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
          let string = String(data: data, encoding: .utf8) else {
      return "{}"
    }
    return string
  }

  static func jsonObject(fromJSONString value: Any?) -> [String: Any]? {
    guard let string = value as? String,
          let data = string.data(using: .utf8),
          let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
      return nil
    }
    return object
  }

  static func jsonArray(fromJSONString value: Any?) -> [Any]? {
    guard let string = value as? String,
          let data = string.data(using: .utf8),
          let array = try? JSONSerialization.jsonObject(with: data) as? [Any] else {
      return nil
    }
    return array
  }

  static func localHealthMetricRowIsDisplaySafe(_ metric: [String: Any]) -> Bool {
    guard let rawSourceKind = metric["source_kind"] as? String,
          MetricSourceKind(rawValue: rawSourceKind) != nil else {
      return false
    }
    return !localHealthMetricValueContainsForbiddenSourceMarker(metric)
  }

  static func localHealthMetricValueContainsForbiddenSourceMarker(_ value: Any?) -> Bool {
    guard let value else {
      return false
    }
    if let text = value as? String {
      if localHealthMetricJSONStringContainsForbiddenSourceMarker(text) {
        return true
      }
      return localHealthMetricTextContainsForbiddenSourceMarker(text)
    }
    if let dictionary = value as? [String: Any] {
      for (key, nestedValue) in dictionary {
        if localHealthMetricTextContainsForbiddenSourceMarker(key) {
          return true
        }
        if localHealthMetricValueContainsForbiddenSourceMarker(nestedValue) {
          return true
        }
      }
      return false
    }
    if let array = value as? [Any] {
      return array.contains { localHealthMetricValueContainsForbiddenSourceMarker($0) }
    }
    return false
  }

  static func localHealthMetricJSONStringContainsForbiddenSourceMarker(_ text: String) -> Bool {
    guard let data = text.data(using: .utf8),
          let value = try? JSONSerialization.jsonObject(with: data) else {
      return false
    }
    return localHealthMetricValueContainsForbiddenSourceMarker(value)
  }

  static func localHealthMetricTextContainsForbiddenSourceMarker(_ text: String) -> Bool {
    let normalized = localHealthMetricNormalizedMarker(text)
    guard !normalized.isEmpty else {
      return false
    }

    if normalized == "not_whoop_backend"
      || normalized == "official_labels_are_labels"
      || normalized == "official_labels_policy" {
      return false
    }

    if normalized.contains("healthkit")
      || normalized.contains("health_connect")
      || normalized.contains("apple_health")
      || normalized.contains("platform_import") {
      return true
    }
    if normalized.contains("official_whoop")
      || normalized.contains("whoop_app")
      || normalized.contains("whoop_label")
      || normalized.contains("whoop_official") {
      return true
    }
    if normalized.contains("whoop_backend") {
      return !normalized.contains("not_whoop_backend")
    }
    return normalized == "official_app" || normalized.hasPrefix("official_app_")
  }

  static func localHealthMetricNormalizedMarker(_ text: String) -> String {
    text
      .lowercased()
      .unicodeScalars
      .map { CharacterSet.alphanumerics.contains($0) ? Character($0) : "_" }
      .reduce(into: "") { result, character in
        if character == "_" {
          if result.last != "_" {
            result.append(character)
          }
        } else {
          result.append(character)
        }
      }
      .trimmingCharacters(in: CharacterSet(charactersIn: "_"))
  }

  static func passStatus(_ report: [String: Any]?) -> String {
    boolValue(report?["pass"]) == true ? "pass" : "blocked"
  }

  static func referenceComparisonStatus(from report: [String: Any]) -> String {
    let status = passStatus(report)
    let deltas = array(report["deltas"]).count
    let goose = report["goose_algorithm_id"] as? String ?? "goose"
    let reference = report["reference_algorithm_id"] as? String ?? "reference"
    return "benchmark-only \(status) | \(deltas) deltas | \(goose) vs \(reference)"
  }

  static func map(_ value: Any?, _ keys: String...) -> [String: Any]? {
    var current: Any? = value
    for key in keys {
      current = (current as? [String: Any])?[key]
    }
    return current as? [String: Any]
  }

  static func array(_ value: Any?) -> [[String: Any]] {
    value as? [[String: Any]] ?? []
  }

  static func stringArray(_ value: Any?) -> [String] {
    value as? [String] ?? []
  }

  static func recoveryProvidedVitalsAreTrusted(_ vitals: [String: Any]) -> Bool {
    guard boolValue(vitals["trusted_metric_input"]) == true else {
      return false
    }
    let flags = stringArray(vitals["quality_flags"])
    return !flags.contains("provided_resp_temp_inputs_not_packet_derived")
      && !flags.contains("provided_resp_temp_provenance_untrusted")
  }

  static func recoveryProvidedVitalsSource(_ vitals: [String: Any]) -> HealthDataSource {
    let detail = vitals["source"] as? String ?? "packet-derived recovery vitals"
    return recoveryProvidedVitalsAreTrusted(vitals)
      ? .bridgeDeviceSensor(detail)
      : .unavailable(detail)
  }

  static func firstMap(in report: [String: Any]?, key: String) -> [String: Any]? {
    array(report?[key]).first
  }

  static func firstActionText(in report: [String: Any]?) -> String? {
    let action = firstMap(in: report, key: "next_actions")
    return action?["summary"] as? String
      ?? action?["action"] as? String
      ?? (report?["issues"] as? [String])?.first
  }

  static func boolValue(_ value: Any?) -> Bool? {
    if let bool = value as? Bool {
      return bool
    }
    if let number = value as? NSNumber {
      return number.boolValue
    }
    return nil
  }

  static func boolText(_ value: Any?) -> String {
    boolValue(value).map { $0 ? "true" : "false" } ?? "unknown"
  }

  static func intValue(_ value: Any?) -> Int? {
    if let int = value as? Int {
      return int
    }
    if let number = value as? NSNumber {
      return number.intValue
    }
    return nil
  }

  static func int64Value(_ value: Any?) -> Int64? {
    if let int64 = value as? Int64 {
      return int64
    }
    if let int = value as? Int {
      return Int64(int)
    }
    if let number = value as? NSNumber {
      return number.int64Value
    }
    if let string = value as? String {
      return Int64(string)
    }
    return nil
  }

  static func doubleValue(_ value: Any?) -> Double? {
    if let double = value as? Double {
      return double
    }
    if let number = value as? NSNumber {
      return number.doubleValue
    }
    return nil
  }

  static func numberText(_ value: Any?, fractionDigits: Int) -> String? {
    guard let double = doubleValue(value) else {
      return nil
    }
    return String(format: "%.\(fractionDigits)f", double)
  }

  static func signedNumberText(_ value: Any?, fractionDigits: Int) -> String? {
    guard let double = doubleValue(value),
          let text = numberText(abs(double), fractionDigits: fractionDigits) else {
      return nil
    }
    if double > 0 {
      return "+\(text)"
    }
    if double < 0 {
      return "-\(text)"
    }
    return text
  }

  static func groupedIntegerText(_ value: Int) -> String {
    let formatter = NumberFormatter()
    formatter.numberStyle = .decimal
    return formatter.string(from: NSNumber(value: value)) ?? "\(value)"
  }

  struct LiveRRDerivedHRVSample {
    let rmssdMS: Double
    let rrIntervalCount: Int
    let sampleCount: Int
    let updatedAt: Date?
    let source: String
  }

  struct LiveHRDerivedRestingHeartRateSample {
    let bpm: Double
    let sampleCount: Int
    let updatedAt: Date?
    let source: String
  }

  static func storedHRDerivedRestingHeartRateSample() -> LiveHRDerivedRestingHeartRateSample? {
    guard let estimate = HeartRateSeriesStore.shared.restingEstimate() else {
      return nil
    }
    return LiveHRDerivedRestingHeartRateSample(
      bpm: estimate.bpm,
      sampleCount: estimate.sampleCount,
      updatedAt: estimate.updatedAt,
      source: estimate.source
    )
  }

  static func liveHRDerivedRestingHeartRateSample() -> LiveHRDerivedRestingHeartRateSample? {
    let defaults = UserDefaults.standard
    if defaults.object(forKey: restingHeartRateEstimateBPMDefaultsKey) != nil,
       let sample = liveHRDerivedRestingHeartRateSample(
        bpm: defaults.double(forKey: restingHeartRateEstimateBPMDefaultsKey),
        sampleCount: defaults.integer(forKey: restingHeartRateEstimateSampleCountDefaultsKey),
        updatedAt: defaults.object(forKey: restingHeartRateEstimateUpdatedAtDefaultsKey) as? Date,
        source: defaults.string(forKey: restingHeartRateEstimateSourceDefaultsKey) ?? "ble.hr.standard.low_quartile"
       ) {
      return sample
    }

    if let estimate = HeartRateSeriesStore.shared.restingEstimate() {
      return LiveHRDerivedRestingHeartRateSample(
        bpm: estimate.bpm,
        sampleCount: estimate.sampleCount,
        updatedAt: estimate.updatedAt,
        source: estimate.source
      )
    }

    return nil
  }

  static func liveHRDerivedRestingHeartRateSample(
    bpm: Double?,
    sampleCount: Int,
    updatedAt: Date?,
    source: String
  ) -> LiveHRDerivedRestingHeartRateSample? {
    guard let bpm else {
      return nil
    }
    guard bpm.isFinite, bpm > 0, sampleCount >= 12 else {
      return nil
    }
    return LiveHRDerivedRestingHeartRateSample(
      bpm: bpm,
      sampleCount: sampleCount,
      updatedAt: updatedAt,
      source: source
    )
  }

  static func storedHRVDerivedHRVSample() -> LiveRRDerivedHRVSample? {
    guard let estimate = HRVSeriesStore.shared.dailyEstimate() else {
      return nil
    }
    return LiveRRDerivedHRVSample(
      rmssdMS: estimate.rmssdMS,
      rrIntervalCount: estimate.rrIntervalCount,
      sampleCount: estimate.sampleCount,
      updatedAt: estimate.updatedAt,
      source: estimate.source
    )
  }

  static func liveRRDerivedHRVSample() -> LiveRRDerivedHRVSample? {
    let defaults = UserDefaults.standard
    guard defaults.object(forKey: liveHRVRMSSDDefaultsKey) != nil else {
      return nil
    }
    return liveRRDerivedHRVSample(
      rmssdMS: defaults.double(forKey: liveHRVRMSSDDefaultsKey),
      rrIntervalCount: defaults.integer(forKey: liveHRVRRIntervalCountDefaultsKey),
      sampleCount: defaults.integer(forKey: liveHRVRMSSDSampleCountDefaultsKey),
      updatedAt: defaults.object(forKey: liveHRVUpdatedAtDefaultsKey) as? Date,
      source: defaults.string(forKey: liveHRVSourceDefaultsKey) ?? "ble.hr.standard.average"
    )
  }

  static func liveRRDerivedHRVSample(
    rmssdMS: Double?,
    rrIntervalCount: Int,
    sampleCount: Int,
    updatedAt: Date?,
    source: String
  ) -> LiveRRDerivedHRVSample? {
    guard let rmssd = rmssdMS else {
      return nil
    }
    guard rmssd.isFinite, rmssd >= 0, rrIntervalCount >= 2, sampleCount > 0 else {
      return nil
    }
    return LiveRRDerivedHRVSample(
      rmssdMS: rmssd,
      rrIntervalCount: rrIntervalCount,
      sampleCount: sampleCount,
      updatedAt: updatedAt,
      source: source
    )
  }

  static func percentText(_ value: Any?) -> String? {
    guard let double = doubleValue(value) else {
      return nil
    }
    return "\(Int((double * 100).rounded()))%"
  }

  nonisolated static func minutesText(_ minutes: Double) -> String {
    let rounded = Int(minutes.rounded())
    let hours = rounded / 60
    let mins = rounded % 60
    return hours > 0 ? "\(hours)h \(mins)m" : "\(mins)m"
  }

  static func bridgeDate(_ value: Any?) -> Date? {
    if let date = value as? Date {
      return date
    }
    if let number = value as? NSNumber {
      return Date(timeIntervalSince1970: number.doubleValue / 1000.0)
    }
    guard let text = value as? String else {
      return nil
    }
    if text.hasPrefix("unix_ms:"),
       let milliseconds = Double(text.dropFirst("unix_ms:".count)) {
      return Date(timeIntervalSince1970: milliseconds / 1000.0)
    }
    if let milliseconds = Double(text), milliseconds > 100_000_000_000 {
      return Date(timeIntervalSince1970: milliseconds / 1000.0)
    }
    let fractionalFormatter = ISO8601DateFormatter()
    fractionalFormatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    if let date = fractionalFormatter.date(from: text) {
      return date
    }
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime]
    return formatter.date(from: text)
  }

  static func timeLabel(_ date: Date) -> String {
    let formatter = DateFormatter()
    formatter.timeStyle = .short
    formatter.dateStyle = .none
    return formatter.string(from: date)
  }

  static func dateLabel(_ date: Date) -> String {
    let formatter = DateFormatter()
    formatter.dateFormat = "dd/MM/yyyy"
    return formatter.string(from: date)
  }

  nonisolated static func algorithmRows(from value: Any) -> [[String: Any]] {
    value as? [[String: Any]] ?? []
  }

  nonisolated static func preferenceRows(from value: Any) -> [String: String] {
    guard let rows = value as? [[String: Any]] else {
      return [:]
    }
    return Dictionary(
      uniqueKeysWithValues: rows.compactMap { row in
        guard let family = row["metric_family"] as? String,
              let algorithmID = row["algorithm_id"] as? String else {
          return nil
        }
        return (family, algorithmID)
      }
    )
  }

  static func emptyTrend(from trend: HealthTrendModel, packetCount: Int) -> HealthTrendModel {
    let hasPackets = packetCount > 0
    return HealthTrendModel(
      id: trend.id,
      title: trend.title,
      rangeLabel: hasPackets ? "\(packetCount) packets decoded" : "No packet data",
      summary: hasPackets ? "Metric unresolved" : "No packet-derived trend",
      analysis: hasPackets
        ? "Packet frames are present, but this metric has not resolved a trusted value from the decoded packet families yet."
        : "No trusted packet-derived values have been captured for this trend yet.",
      resources: trend.resources,
      points: []
    )
  }

  static func dailyTrend(
    id: String,
    title: String,
    rows: [[String: Any]],
    valueKey: String,
    unit: String,
    fractionDigits: Int,
    resources: [String]
  ) -> HealthTrendModel {
    let points = rows.enumerated().compactMap { index, row -> HealthTrendPoint? in
      guard let value = doubleValue(row[valueKey]) else {
        return nil
      }
      let date = row["date"] as? String ?? row["date_key"] as? String
      let label = date.map { String($0.suffix(5)) } ?? "D\(index + 1)"
      return HealthTrendPoint(label: label, value: value)
    }
    let values = points.map(\.value)
    let range = rangeText(values: values, unit: unit, fractionDigits: fractionDigits) ?? "No packet trend"
    return HealthTrendModel(
      id: id,
      title: title,
      rangeLabel: range,
      summary: points.isEmpty ? "No packet-derived trend" : "\(points.count)d packet trend | \(range)",
      analysis: points.isEmpty
        ? "No trusted packet-derived values have been captured for this trend yet."
        : "Packet-derived daily values from locally captured WHOOP frames.",
      resources: resources,
      points: points
    )
  }

  static func restingHeartRateDailyRecoveryTrend(
    base trend: HealthTrendModel,
    metrics: [[String: Any]]
  ) -> HealthTrendModel {
    let rows = dailyRecoveryRestingHRTrendRows(from: metrics)
    let daily = dailyTrend(
      id: trend.id,
      title: trend.title,
      rows: rows,
      valueKey: "resting_hr_bpm",
      unit: "bpm",
      fractionDigits: 0,
      resources: trend.resources
    )

    let values = rows.compactMap { doubleValue($0["resting_hr_bpm"]) }
    guard daily.hasData, !values.isEmpty else {
      return daily
    }

    let latestMetric = rows.last ?? [:]
    let latest = values.last ?? 0
    let average = values.reduce(0, +) / Double(values.count)
    let delta = latest - average
    let latestText = numberText(latest, fractionDigits: 0) ?? "--"
    let averageText = numberText(average, fractionDigits: 0) ?? "--"
    let deltaText = signedNumberText(delta, fractionDigits: 0) ?? "0"
    let confidence = numberText(latestMetric["confidence"], fractionDigits: 2) ?? "0"
    let sourceKind = latestMetric["source_kind"] as? String ?? "device_sensor"

    return HealthTrendModel(
      id: daily.id,
      title: daily.title,
      rangeLabel: daily.rangeLabel,
      summary: "\(daily.points.count)d stored recovery trend | \(daily.rangeLabel)",
      analysis: "Stored packet-derived daily resting HR from daily_recovery_metrics. Latest \(latestText) bpm | avg \(averageText) bpm | \(deltaText) bpm vs avg | source \(sourceKind) | confidence \(confidence).",
      resources: daily.resources,
      points: daily.points
    )
  }

  static func restingHeartRateRollupTrend(
    base trend: HealthTrendModel,
    report: [String: Any]
  ) -> HealthTrendModel {
    let featureReport = map(report, "feature_report")
    var rows = array(featureReport?["daily"])
    if rows.isEmpty,
       let dateKey = report["date_key"] as? String,
       let value = doubleValue(report["resting_hr_bpm"]) {
      rows = [["date": dateKey, "resting_hr_bpm": value]]
    }
    let daily = dailyTrend(
      id: trend.id,
      title: trend.title,
      rows: rows,
      valueKey: "resting_hr_bpm",
      unit: "bpm",
      fractionDigits: 0,
      resources: trend.resources
    )

    let sevenDay = numberText(report["rolling_7_day_average_bpm"], fractionDigits: 0)
    let sevenDayDelta = signedNumberText(report["selected_vs_7_day_average_bpm"], fractionDigits: 0)
    let thirtyDay = numberText(report["rolling_30_day_average_bpm"], fractionDigits: 0)
    let thirtyDayDelta = signedNumberText(report["selected_vs_30_day_average_bpm"], fractionDigits: 0)
    let sampleCount = intValue(report["sample_count"]) ?? 0
    let analysisParts = [
      sevenDay.map { average in "7d avg \(average) bpm\(sevenDayDelta.map { " (\($0) bpm)" } ?? "")" },
      thirtyDay.map { average in "30d avg \(average) bpm\(thirtyDayDelta.map { " (\($0) bpm)" } ?? "")" },
      sampleCount > 0 ? "\(sampleCount) HR samples" : nil,
    ].compactMap { $0 }
    let analysis = analysisParts.isEmpty
      ? "Packet-derived daily resting HR from locally captured WHOOP heart-rate samples."
      : "Packet-derived daily resting HR from locally captured WHOOP heart-rate samples. \(analysisParts.joined(separator: " | "))."

    return HealthTrendModel(
      id: daily.id,
      title: daily.title,
      rangeLabel: daily.rangeLabel,
      summary: daily.hasData ? "\(daily.points.count)d packet trend | \(daily.rangeLabel)" : daily.summary,
      analysis: analysis,
      resources: daily.resources,
      points: daily.points
    )
  }

  static func energyRollupTrend(
    base trend: HealthTrendModel,
    report: [String: Any],
    valueKey: String
  ) -> HealthTrendModel {
    var rows: [[String: Any]] = []
    if let dateKey = report["date_key"] as? String,
       let value = doubleValue(report[valueKey]) {
      rows = [["date": dateKey, valueKey: value]]
    }
    let daily = dailyTrend(
      id: trend.id,
      title: trend.title,
      rows: rows,
      valueKey: valueKey,
      unit: "kcal",
      fractionDigits: 0,
      resources: trend.resources
    )

    let active = numberText(report["active_kcal"], fractionDigits: 0) ?? "--"
    let resting = numberText(report["resting_kcal"], fractionDigits: 0) ?? "--"
    let total = numberText(report["total_kcal"], fractionDigits: 0) ?? "--"
    let confidence = numberText(report["confidence"], fractionDigits: 2) ?? "0"
    let covered = numberText(report["covered_minutes"], fractionDigits: 0) ?? "0"
    return HealthTrendModel(
      id: daily.id,
      title: daily.title,
      rangeLabel: daily.rangeLabel,
      summary: daily.hasData ? "\(daily.points.count)d local estimate | \(daily.rangeLabel)" : daily.summary,
      analysis: "Local WHOOP-derived calorie estimate from packet heart-rate and motion features. Active \(active) kcal | resting \(resting) kcal | total \(total) kcal | \(covered) covered minutes | confidence \(confidence).",
      resources: daily.resources,
      points: daily.points
    )
  }

  static func energyMetricTrend(
    base trend: HealthTrendModel,
    metrics: [[String: Any]],
    valueKey: String,
    latestMetric: [String: Any]
  ) -> HealthTrendModel {
    let rows = dailyActivityTrendRows(from: metrics, valueKey: valueKey)
    let daily = dailyTrend(
      id: trend.id,
      title: trend.title,
      rows: rows,
      valueKey: valueKey,
      unit: "kcal",
      fractionDigits: 0,
      resources: trend.resources
    )
    let values = rows.compactMap { doubleValue($0[valueKey]) }
    guard daily.hasData, !values.isEmpty else {
      return daily
    }

    let latest = doubleValue(latestMetric[valueKey]) ?? values.last ?? 0
    let average = values.reduce(0, +) / Double(values.count)
    let delta = latest - average
    let latestText = numberText(latest, fractionDigits: 0) ?? "--"
    let averageText = numberText(average, fractionDigits: 0) ?? "--"
    let deltaText = signedNumberText(delta, fractionDigits: 0) ?? "0"
    let confidence = numberText(latestMetric["confidence"], fractionDigits: 2) ?? "0"
    let sourceKind = latestMetric["source_kind"] as? String ?? "unknown"
    let label = energyMetricLabel(valueKey)

    return HealthTrendModel(
      id: daily.id,
      title: daily.title,
      rangeLabel: daily.rangeLabel,
      summary: "\(daily.points.count)d stored activity trend | \(daily.rangeLabel)",
      analysis: "Stored daily WHOOP-derived \(label) from daily_activity_metrics. Latest \(latestText) kcal | avg \(averageText) kcal | \(deltaText) kcal vs avg | source \(sourceKind) | confidence \(confidence).",
      resources: daily.resources,
      points: daily.points
    )
  }

  static func energyMetricLabel(_ valueKey: String) -> String {
    switch valueKey {
    case "active_kcal":
      return "active calories"
    case "resting_kcal":
      return "resting calories"
    case "total_kcal":
      return "total calories"
    default:
      return valueKey
    }
  }

  static func stepMetricTrend(
    base trend: HealthTrendModel,
    metrics: [[String: Any]],
    latestMetric metric: [String: Any]
  ) -> HealthTrendModel {
    let rows = dailyActivityTrendRows(from: metrics, valueKey: "steps")
    let daily = dailyTrend(
      id: trend.id,
      title: trend.title,
      rows: rows,
      valueKey: "steps",
      unit: "steps",
      fractionDigits: 0,
      resources: trend.resources
    )

    let steps = numberText(metric["steps"], fractionDigits: 0) ?? "--"
    let sourceKind = metric["source_kind"] as? String ?? "unknown"
    let confidence = numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    let cadence = numberText(metric["average_cadence_spm"], fractionDigits: 0)
      .map { " | cadence \($0) spm" } ?? ""
    let sourceText: String
    switch sourceKind {
    case "device_counter":
      sourceText = "decoded WHOOP step counter"
    case "local_estimate":
      sourceText = "validated raw-motion local estimate"
    default:
      sourceText = sourceKind
    }

    return HealthTrendModel(
      id: daily.id,
      title: daily.title,
      rangeLabel: daily.rangeLabel,
      summary: daily.hasData ? "\(daily.points.count)d stored \(sourceKind) trend | \(daily.rangeLabel)" : daily.summary,
      analysis: "Stored daily WHOOP-derived step metric from \(sourceText). Steps \(steps) | confidence \(confidence)\(cadence).",
      resources: daily.resources,
      points: daily.points
    )
  }

  static func latestDailyDateText(in report: [String: Any]) -> String? {
    array(report["daily"]).last?["date"] as? String
  }

  static func rollupFreshnessText(in report: [String: Any]) -> String? {
    if let dateKey = report["date_key"] as? String {
      return dateKey
    }
    return map(report, "feature_report").flatMap { latestDailyDateText(in: $0) }
  }

  static func rangeText(values: [Double], unit: String, fractionDigits: Int) -> String? {
    guard let min = values.min(), let max = values.max() else {
      return nil
    }
    let minText = numberText(min, fractionDigits: fractionDigits) ?? "\(min)"
    let maxText = numberText(max, fractionDigits: fractionDigits) ?? "\(max)"
    return "\(minText) - \(maxText) \(unit)"
  }

  static func stressTrendModel(
    base trend: HealthTrendModel,
    summary: StressAlgorithmSummary,
    points: [StressWindowPoint]? = nil,
    title: String? = nil
  ) -> HealthTrendModel {
    let windows = points ?? summary.windows
    let trendPoints = windows.map {
      HealthTrendPoint(label: $0.timeLabel, value: $0.stress)
    }
    let values = trendPoints.map(\.value)
    let range = rangeText(values: values, unit: "", fractionDigits: 0) ?? "No stress trend"
    return HealthTrendModel(
      id: trend.id,
      title: title ?? trend.title,
      rangeLabel: range,
      summary: trendPoints.isEmpty
        ? "No local stress windows"
        : "\(trendPoints.count) stress windows | avg \(numberText(summary.score, fractionDigits: 0) ?? "--")",
      analysis: trendPoints.isEmpty
        ? "No heart-rate samples have been captured for today's stress timeline yet."
        : "Stress is estimated from local heart-rate elevation over resting HR and short-window HR volatility.",
      resources: trend.resources,
      points: trendPoints
    )
  }

  static func energyBankTrendModel(
    base trend: HealthTrendModel,
    summary: EnergyBankAlgorithmSummary
  ) -> HealthTrendModel {
    let trendPoints = summary.points.map {
      HealthTrendPoint(label: $0.timeLabel, value: $0.energy)
    }
    let values = trendPoints.map(\.value)
    let range = rangeText(values: values, unit: "%", fractionDigits: 0) ?? "No energy trend"
    return HealthTrendModel(
      id: trend.id,
      title: trend.title,
      rangeLabel: range,
      summary: trendPoints.isEmpty
        ? "No Energy Bank windows"
        : "\(trendPoints.count) energy windows | current \(numberText(summary.percent, fractionDigits: 0) ?? "--")%",
      analysis: trendPoints.isEmpty
        ? "Energy Bank needs local stress windows from heart-rate samples."
        : "Energy Bank charges during likely sleep or low-stress rest and drains with stress load.",
      resources: trend.resources,
      points: trendPoints
    )
  }

  static func cardioLoadTrendModel(
    base trend: HealthTrendModel,
    summary: CardioLoadAlgorithmSummary
  ) -> HealthTrendModel {
    let trendPoints = summary.points.map {
      HealthTrendPoint(label: $0.dateLabel, value: $0.load)
    }
    let values = trendPoints.map(\.value)
    let range = rangeText(values: values, unit: "load", fractionDigits: 0) ?? "No cardio load trend"
    return HealthTrendModel(
      id: trend.id,
      title: trend.title,
      rangeLabel: range,
      summary: trendPoints.isEmpty
        ? "No Cardio Load days"
        : "\(summary.sessionCount) sessions | \(summary.activityDayCount)d with load | \(summary.status)",
      analysis: trendPoints.isEmpty
        ? "Cardio Load needs stored activity sessions and heart-rate data."
        : "Cardio Load is computed from local activity sessions, duration, and heart-rate intensity.",
      resources: trend.resources,
      points: trendPoints
    )
  }

  static func trend(_ id: String, title: String, values: [Double], range: String, summary: String) -> HealthTrendModel {
    HealthTrendModel(
      id: id,
      title: title,
      rangeLabel: range,
      summary: summary,
      analysis: values.isEmpty ? "No local data has been captured for this trend yet." : "Sample trend shows a stable baseline with one recent movement worth reviewing.",
      resources: ["The Basics", "How \(title) is calculated"],
      points: values.enumerated().map { index, value in
        HealthTrendPoint(label: "D\(index + 1)", value: value)
      }
    )
  }

  static func snapshot(
    id: String,
    route: HealthRoute,
    group: HealthMetricGroup,
    title: String,
    value: String,
    unit: String,
    status: String,
    freshness: String,
    provenance: String,
    source: HealthDataSource,
    systemImage: String,
    tint: Color,
    trendValues: [Double],
    range: String
  ) -> HealthMetricSnapshot {
    HealthMetricSnapshot(
      id: id,
      route: route,
      group: group,
      title: title,
      value: value,
      unit: unit,
      status: status,
      freshness: freshness,
      provenance: provenance,
      source: source,
      systemImage: systemImage,
      tint: tint,
      trend: trend(id, title: title, values: trendValues, range: range, summary: "\(status) | \(range)")
    )
  }

  static func relativeText(for date: Date?) -> String? {
    guard let date else {
      return nil
    }
    if abs(date.timeIntervalSinceNow) < 10 {
      return "Now"
    }
    return Self.relativeFormatter.localizedString(for: date, relativeTo: Date()).capitalized
  }
}
