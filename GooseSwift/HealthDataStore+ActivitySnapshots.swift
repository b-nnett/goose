import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func dailyRecoveryMetrics() -> [[String: Any]] {
    Self.array(packetInputReports["daily_recovery"]?["metrics"])
      .filter { Self.localHealthMetricRowIsDisplaySafe($0) }
  }

  func dailyRecoveryMetricsWithRestingHR() -> [[String: Any]] {
    dailyRecoveryMetricsWithValue("resting_hr_bpm")
  }

  func dailyRecoveryMetricsWithValue(_ valueKey: String) -> [[String: Any]] {
    dailyRecoveryMetrics()
      .filter { metric in
        metric["source_kind"] as? String == "device_sensor"
          && Self.doubleValue(metric[valueKey]) != nil
          && Self.doubleValue(metric["confidence"]) != nil
      }
  }

  func dailyRecoveryUnavailableMetrics() -> [[String: Any]] {
    dailyRecoveryMetrics()
      .filter { metric in
        metric["source_kind"] as? String == "unavailable"
          && Self.doubleValue(metric["confidence"]) != nil
      }
  }

  func preferredDailyRecoveryUnavailableMetric(
    metricID: String,
    for date: Date? = nil,
    calendar: Calendar = .current
  ) -> [String: Any]? {
    let dateKey = date.map { Self.metricDateKey(for: $0, calendar: calendar) }
    return dailyRecoveryUnavailableMetrics()
      .filter { metric in
        if let dateKey, metric["date_key"] as? String != dateKey {
          return false
        }
        return Self.dailyRecoveryUnavailableMetric(metric, matches: metricID)
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

  static func dailyRecoveryUnavailableMetric(_ metric: [String: Any], matches metricID: String) -> Bool {
    if let inputsMetricID = jsonObject(fromJSONString: metric["inputs_json"])?["metric_id"] as? String,
       inputsMetricID == metricID {
      return true
    }
    let sanitizedMetricID = Self.recoveryMetricIDToken(metricID)
    let dailyMetricID = (metric["daily_metric_id"] as? String ?? "").lowercased()
    return dailyMetricID.contains(sanitizedMetricID)
  }

  static func recoveryMetricIDToken(_ value: String) -> String {
    metricIDToken(value)
  }

  static func metricIDToken(_ value: String) -> String {
    value.lowercased().unicodeScalars.map { scalar in
      CharacterSet.alphanumerics.contains(scalar) ? String(scalar) : "-"
    }.joined()
  }

  func recoveryUnavailableSourceDetail(
    metricID: String,
    for date: Date? = nil,
    calendar: Calendar = .current
  ) -> String? {
    guard let metric = preferredDailyRecoveryUnavailableMetric(metricID: metricID, for: date, calendar: calendar) else {
      return nil
    }
    return Self.recoveryUnavailableSourceDetail(metric)
  }

  static func recoveryUnavailableSourceDetail(_ metric: [String: Any]) -> String {
    let metricID = jsonObject(fromJSONString: metric["inputs_json"])?["metric_id"] as? String
      ?? metric["daily_metric_id"] as? String
      ?? "recovery_metric"
    let blocker = firstRecoveryUnavailableBlocker(metric) ?? "metric unavailable"
    return "\(metricID) unavailable: \(blocker)"
  }

  static func firstRecoveryUnavailableBlocker(_ metric: [String: Any]) -> String? {
    if let blockers = metric["blocker_reasons"] as? [String],
       let first = blockers.first {
      return first
    }
    if let inputs = jsonObject(fromJSONString: metric["inputs_json"]),
       let blockers = inputs["blocker_reasons"] as? [String],
       let first = blockers.first {
      return first
    }
    if let provenance = jsonObject(fromJSONString: metric["provenance_json"]),
       let blockers = provenance["blocker_reasons"] as? [String],
       let first = blockers.first {
      return first
    }
    if let flags = jsonArray(fromJSONString: metric["quality_flags_json"]) as? [String],
       let blocker = flags.first(where: { !$0.contains("unavailable") && !$0.contains("source_kind") }) {
      return blocker
    }
    return nil
  }

  static func firstActivityUnavailableBlocker(_ metric: [String: Any]) -> String? {
    firstRecoveryUnavailableBlocker(metric)
  }

  static func recoveryUnavailableProvenanceSummary(_ metric: [String: Any]) -> String {
    let metricID = jsonObject(fromJSONString: metric["inputs_json"])?["metric_id"] as? String ?? "recovery_metric"
    let confidence = numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    let blocker = firstRecoveryUnavailableBlocker(metric) ?? "blocked"
    return "daily_recovery_metrics unavailable | metric=\(metricID) | confidence=\(confidence) | blocker=\(blocker)"
  }

  func preferredDailyRecoveryMetricWithRestingHR() -> [String: Any]? {
    Self.preferredDailyRecoveryMetricWithRestingHR(from: dailyRecoveryMetricsWithRestingHR())
  }

  func preferredDailyRecoveryMetricWithRestingHR(
    for date: Date,
    calendar: Calendar = .current
  ) -> [String: Any]? {
    preferredDailyRecoveryMetric(
      valueKey: "resting_hr_bpm",
      for: date,
      calendar: calendar
    )
  }

  static func preferredDailyRecoveryMetricWithRestingHR(from metrics: [[String: Any]]) -> [String: Any]? {
    preferredDailyRecoveryMetric(from: metrics, valueKey: "resting_hr_bpm")
  }

  func preferredDailyRecoveryMetric(
    valueKey: String,
    for date: Date,
    calendar: Calendar = .current
  ) -> [String: Any]? {
    let dateKey = Self.metricDateKey(for: date, calendar: calendar)
    return Self.preferredDailyRecoveryMetric(
      from: dailyRecoveryMetricsWithValue(valueKey).filter { $0["date_key"] as? String == dateKey },
      valueKey: valueKey
    )
  }

  static func preferredDailyRecoveryMetric(from metrics: [[String: Any]], valueKey: String) -> [String: Any]? {
    metrics
      .filter { localHealthMetricRowIsDisplaySafe($0) }
      .sorted { lhs, rhs in
        dailyRecoveryMetric(lhs, isBetterThan: rhs, valueKey: valueKey)
      }
      .first
  }

  static func dailyRecoveryRestingHRTrendRows(from metrics: [[String: Any]]) -> [[String: Any]] {
    dailyRecoveryTrendRows(from: metrics, valueKey: "resting_hr_bpm")
  }

  static func dailyRecoveryTrendRows(
    from metrics: [[String: Any]],
    valueKey: String
  ) -> [[String: Any]] {
    var rowsByDate: [String: [String: Any]] = [:]
    for metric in metrics {
      guard localHealthMetricRowIsDisplaySafe(metric) else {
        continue
      }
      guard let dateKey = metric["date_key"] as? String ?? metric["date"] as? String,
            let value = doubleValue(metric[valueKey]) else {
        continue
      }
      var row = metric
      row["date"] = dateKey
      row[valueKey] = value
      if let existing = rowsByDate[dateKey],
         !dailyRecoveryMetric(row, isBetterThan: existing, valueKey: valueKey) {
        continue
      }
      rowsByDate[dateKey] = row
    }
    return rowsByDate.values.sorted {
      (int64Value($0["end_time_unix_ms"]) ?? 0) < (int64Value($1["end_time_unix_ms"]) ?? 0)
    }
  }

  static func dailyRecoveryRestingHRTrendRow(_ lhs: [String: Any], isBetterThan rhs: [String: Any]) -> Bool {
    dailyRecoveryMetric(lhs, isBetterThan: rhs, valueKey: "resting_hr_bpm")
  }

  static func dailyRecoveryMetric(
    _ lhs: [String: Any],
    isBetterThan rhs: [String: Any],
    valueKey _: String
  ) -> Bool {
    let lhsConfidence = doubleValue(lhs["confidence"]) ?? 0
    let rhsConfidence = doubleValue(rhs["confidence"]) ?? 0
    if lhsConfidence != rhsConfidence {
      return lhsConfidence > rhsConfidence
    }
    let lhsEnd = int64Value(lhs["end_time_unix_ms"]) ?? 0
    let rhsEnd = int64Value(rhs["end_time_unix_ms"]) ?? 0
    if lhsEnd != rhsEnd {
      return lhsEnd > rhsEnd
    }
    let lhsUpdated = lhs["updated_at"] as? String ?? ""
    let rhsUpdated = rhs["updated_at"] as? String ?? ""
    return lhsUpdated > rhsUpdated
  }

  static func preferredStepMetric(from metrics: [[String: Any]]) -> [String: Any]? {
    metrics
      .filter { localHealthMetricRowIsDisplaySafe($0) }
      .filter { intValue($0["steps"]) != nil }
      .sorted { lhs, rhs in
        let lhsPriority = stepMetricSourcePriority(lhs["source_kind"] as? String)
        let rhsPriority = stepMetricSourcePriority(rhs["source_kind"] as? String)
        if lhsPriority != rhsPriority {
          return lhsPriority < rhsPriority
        }
        let lhsConfidence = doubleValue(lhs["confidence"]) ?? 0
        let rhsConfidence = doubleValue(rhs["confidence"]) ?? 0
        if lhsConfidence != rhsConfidence {
          return lhsConfidence > rhsConfidence
        }
        let lhsEnd = int64Value(lhs["end_time_unix_ms"]) ?? 0
        let rhsEnd = int64Value(rhs["end_time_unix_ms"]) ?? 0
        return lhsEnd > rhsEnd
      }
      .first
  }

  static func preferredDailyActivityMetric(
    from metrics: [[String: Any]],
    valueKey: String
  ) -> [String: Any]? {
    metrics
      .filter { localHealthMetricRowIsDisplaySafe($0) }
      .filter { doubleValue($0[valueKey]) != nil }
      .sorted { lhs, rhs in
        dailyActivityMetric(lhs, isBetterThan: rhs, valueKey: valueKey)
      }
      .first
  }

  static func dailyActivityTrendRows(
    from metrics: [[String: Any]],
    valueKey: String
  ) -> [[String: Any]] {
    var rowsByDate: [String: [String: Any]] = [:]
    for metric in metrics {
      guard localHealthMetricRowIsDisplaySafe(metric) else {
        continue
      }
      guard let dateKey = metric["date_key"] as? String ?? metric["date"] as? String,
            let value = doubleValue(metric[valueKey]) else {
        continue
      }
      var row = metric
      row["date"] = dateKey
      row[valueKey] = value
      if let existing = rowsByDate[dateKey],
         !dailyActivityMetric(row, isBetterThan: existing, valueKey: valueKey) {
        continue
      }
      rowsByDate[dateKey] = row
    }
    return rowsByDate.values.sorted {
      (int64Value($0["end_time_unix_ms"]) ?? 0) < (int64Value($1["end_time_unix_ms"]) ?? 0)
    }
  }

  static func dailyActivityMetric(
    _ lhs: [String: Any],
    isBetterThan rhs: [String: Any],
    valueKey: String
  ) -> Bool {
    let lhsPriority = dailyActivityMetricSourcePriority(lhs["source_kind"] as? String, valueKey: valueKey)
    let rhsPriority = dailyActivityMetricSourcePriority(rhs["source_kind"] as? String, valueKey: valueKey)
    if lhsPriority != rhsPriority {
      return lhsPriority < rhsPriority
    }
    let lhsConfidence = doubleValue(lhs["confidence"]) ?? 0
    let rhsConfidence = doubleValue(rhs["confidence"]) ?? 0
    if lhsConfidence != rhsConfidence {
      return lhsConfidence > rhsConfidence
    }
    let lhsEnd = int64Value(lhs["end_time_unix_ms"]) ?? 0
    let rhsEnd = int64Value(rhs["end_time_unix_ms"]) ?? 0
    if lhsEnd != rhsEnd {
      return lhsEnd > rhsEnd
    }
    let lhsUpdated = lhs["updated_at"] as? String ?? ""
    let rhsUpdated = rhs["updated_at"] as? String ?? ""
    return lhsUpdated > rhsUpdated
  }

  static func dailyActivityMetricSourcePriority(_ sourceKind: String?, valueKey: String) -> Int {
    if valueKey == "steps" {
      return stepMetricSourcePriority(sourceKind)
    }
    switch sourceKind {
    case "local_estimate":
      return 0
    default:
      return 10
    }
  }

  static func stepMetricSourcePriority(_ sourceKind: String?) -> Int {
    switch sourceKind {
    case "device_counter":
      return 0
    case "local_estimate":
      return 1
    default:
      return 10
    }
  }

  func strainEmptyStateSummary() -> String {
    if packetScoreStatus == "No run" {
      return "No local strain score has been computed yet."
    }
    if usesPreviewPacketData {
      return "Preview strain data is hidden."
    }
    return packetScoreStatus
  }

  func strainTrendRowsForV2() -> [HealthMetricSnapshot] {
    Self.strainTrendRows.compactMap { snapshot in
      switch snapshot.id {
      case "active-energy-trend":
        return energyRollupSnapshot(base: snapshot, valueKey: "active_kcal")
      case "total-energy-trend":
        return energyRollupSnapshot(base: snapshot, valueKey: "total_kcal")
      case "step-count-trend":
        return stepMetricSnapshot(base: snapshot)
      default:
        return nil
      }
    }
  }

  func currentStrainScore0To21() -> Double? {
    guard !usesPreviewPacketData else {
      return nil
    }
    return Self.doubleValue(Self.map(packetScoreReports["strain"], "score_result", "output")?["score_0_to_21"])
  }

  func strainSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    guard let rawScore = currentStrainScore0To21(),
          let scoreText = Self.numberText(Self.strainPercent(rawScore), fractionDigits: 0) else {
      // HK proxy: active calorie-based strain estimate
      if let score = hkStrainScore(),
         let scoreText = Self.numberText(score, fractionDigits: 0) {
        return replacingHealthMonitorSnapshot(
          snapshot,
          value: scoreText,
          unit: "%",
          status: Self.strainStatusLabel(score: score),
          freshness: "From Apple Health",
          provenance: "apple.health.active_kcal_strain_estimate",
          source: .local("apple.health"),
          trend: Self.emptyTrend(from: snapshot.trend, packetCount: 0)
        )
      }
      return zeroStrainSnapshot(
        base: snapshot,
        freshness: "No local data",
        provenance: "metrics.strain_score_from_features",
        sourceDetail: "strain requires packet-derived activity and heart-rate inputs"
      )
    }

    return replacingHealthMonitorSnapshot(
      snapshot,
      value: scoreText,
      unit: "",
      status: Self.strainStatusLabel(score: Self.strainPercent(rawScore)),
      freshness: "Latest",
      provenance: "metrics.strain_score_from_features",
      source: .bridge("goose.strain.v0"),
      trend: Self.emptyTrend(from: snapshot.trend, packetCount: packetEvidenceFrameCount())
    )
  }

  func energyRollupSnapshot(base snapshot: HealthMetricSnapshot, valueKey: String) -> HealthMetricSnapshot? {
    let energyMetrics = dailyActivityMetricsWithValue(valueKey)
    if let metric = Self.preferredDailyActivityMetric(from: energyMetrics, valueKey: valueKey),
       let value = Self.doubleValue(metric[valueKey]),
       let valueText = Self.numberText(value, fractionDigits: 0) {
      return replacingHealthMonitorSnapshot(
        snapshot,
        value: valueText,
        unit: "kcal",
        status: energyMetricStatus(metric),
        freshness: metric["date_key"] as? String ?? "Latest",
        provenance: "daily_activity_metrics | \(dailyActivityMetricProvenanceSummary(metric))",
        source: energyMetricSource(metric),
        trend: Self.energyMetricTrend(
          base: snapshot.trend,
          metrics: energyMetrics,
          valueKey: valueKey,
          latestMetric: metric
        )
      )
    }

    guard let report = packetInputReports["energy_rollup"],
          Self.boolValue(report["pass"]) == true,
          let value = Self.doubleValue(report[valueKey]),
          let valueText = Self.numberText(value, fractionDigits: 0) else {
      return nil
    }
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "kcal",
      status: "Local estimate",
      freshness: Self.rollupFreshnessText(in: report) ?? "Today",
      provenance: "metrics.energy_daily_rollup | \(energyRollupProvenanceSummary())",
      source: .localEstimate("metrics.energy_daily_rollup"),
      trend: Self.energyRollupTrend(base: snapshot.trend, report: report, valueKey: valueKey)
    )
  }

  func stepMetricSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot? {
    let stepMetrics = dailyActivityMetricsWithValue("steps")
    guard let metric = Self.preferredStepMetric(from: stepMetrics),
          let steps = Self.intValue(metric["steps"]) else {
      return nil
    }
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: Self.groupedIntegerText(steps),
      unit: "steps",
      status: stepMetricStatus(metric),
      freshness: metric["date_key"] as? String ?? "Today",
      provenance: "daily_activity_metrics | \(dailyActivityMetricProvenanceSummary(metric))",
      source: stepMetricSource(metric),
      trend: Self.stepMetricTrend(base: snapshot.trend, metrics: stepMetrics, latestMetric: metric)
    )
  }

  func stepMetricStatus(_ metric: [String: Any]) -> String {
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    let cadence = Self.numberText(metric["average_cadence_spm"], fractionDigits: 0)
      .map { " | cadence \($0) spm" } ?? ""
    switch metric["source_kind"] as? String {
    case "device_counter":
      return "WHOOP counter | confidence \(confidence)\(cadence)"
    case "local_estimate":
      return "Validated local estimate | confidence \(confidence)\(cadence)"
    default:
      return "WHOOP-derived steps | confidence \(confidence)\(cadence)"
    }
  }

  func stepMetricSource(_ metric: [String: Any]) -> HealthDataSource {
    switch metric["source_kind"] as? String {
    case "device_counter":
      return .bridgeDeviceCounter("daily_activity_metrics WHOOP step counter")
    case "local_estimate":
      return .localEstimate("daily_activity_metrics validated raw-motion steps")
    default:
      return .unavailable("unsupported step metric source")
    }
  }

  func stepMetricProvenanceSummary(_ metric: [String: Any]) -> String {
    dailyActivityMetricProvenanceSummary(metric)
  }

  func energyMetricStatus(_ metric: [String: Any]) -> String {
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    switch metric["source_kind"] as? String {
    case "local_estimate":
      return "Local estimate | confidence \(confidence)"
    default:
      return "WHOOP-derived energy | confidence \(confidence)"
    }
  }

  func energyMetricSource(_ metric: [String: Any]) -> HealthDataSource {
    switch metric["source_kind"] as? String {
    case "local_estimate":
      return .localEstimate("daily_activity_metrics local energy estimate")
    default:
      return .unavailable("unsupported energy metric source")
    }
  }

  func dailyActivityMetricProvenanceSummary(_ metric: [String: Any]) -> String {
    let sourceKind = metric["source_kind"] as? String ?? "unknown"
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    let updatedAt = metric["updated_at"] as? String
    return [
      "source_kind=\(sourceKind)",
      "confidence=\(confidence)",
      updatedAt.map { "updated=\($0)" },
    ].compactMap { $0 }.joined(separator: " | ")
  }

  func dailyRecoveryRestingHRStatus(_ metric: [String: Any]) -> String {
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    return "Packet-derived | confidence \(confidence)"
  }

  func dailyRecoveryRestingHRSource(_ metric: [String: Any]) -> HealthDataSource {
    guard metric["source_kind"] as? String == "device_sensor" else {
      return .unavailable("unsupported resting HR recovery metric source")
    }
    return .bridgeDeviceSensor("daily_recovery_metrics packet-derived resting HR")
  }

  func dailyRecoveryMetricSource(_ metric: [String: Any], metricName: String) -> HealthDataSource {
    guard metric["source_kind"] as? String == "device_sensor" else {
      return .unavailable("unsupported \(metricName) recovery metric source")
    }
    return .bridgeDeviceSensor("daily_recovery_metrics packet-derived \(metricName)")
  }

  func dailyRecoveryMetricStatus(_ metric: [String: Any]) -> String {
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    return "Packet-derived | confidence \(confidence)"
  }

  func dailyRecoveryMetricProvenanceSummary(_ metric: [String: Any]) -> String {
    dailyRecoveryRestingHRProvenanceSummary(metric)
  }

  func dailyRecoveryRestingHRProvenanceSummary(_ metric: [String: Any]) -> String {
    let sourceKind = metric["source_kind"] as? String ?? "unknown"
    let confidence = Self.numberText(metric["confidence"], fractionDigits: 2) ?? "0"
    let updatedAt = metric["updated_at"] as? String
    return [
      "source_kind=\(sourceKind)",
      "confidence=\(confidence)",
      updatedAt.map { "updated=\($0)" },
    ].compactMap { $0 }.joined(separator: " | ")
  }
}
