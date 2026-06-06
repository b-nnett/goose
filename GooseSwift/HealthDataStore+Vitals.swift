import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func packetBackedHealthMonitorSnapshot(
    base snapshot: HealthMetricSnapshot,
    allowLiveFallbacks: Bool = true
  ) -> HealthMetricSnapshot {
    switch snapshot.id {
    case "respiratory-rate":
      if let stored = dailyRecoveryMetricSnapshot(
        base: snapshot,
        valueKey: "respiratory_rate_rpm",
        unit: "rpm",
        fractionDigits: 1,
        metricName: "respiratory rate"
      ) {
        return stored
      }
      if let rate = currentRecoveryRespiratoryRateRPM(),
         let vitals = Self.map(packetScoreReports["recovery"], "provided_vitals"),
         let text = Self.numberText(rate, fractionDigits: 1) {
        let source = vitals["source"] as? String ?? "packet-derived recovery vitals"
        return replacingHealthMonitorSnapshot(
          snapshot,
          value: text,
          unit: "rpm",
          status: "Packet-derived",
          freshness: "Decoded",
          provenance: source,
          source: Self.recoveryProvidedVitalsSource(vitals),
          trend: Self.emptyTrend(from: snapshot.trend, packetCount: packetEvidenceFrameCount())
        )
      }
      if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "respiratory_rate_rpm") {
        return unavailablePacketSnapshot(
          base: snapshot,
          status: "Unavailable",
          freshness: unavailable["date_key"] as? String ?? "Stored blocker",
          provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
          sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
        )
      }
      return unresolvedPacketVitalSnapshot(
        base: snapshot,
        status: "No data",
        freshness: packetInputStatus == "No run" ? "Run pending" : "Packet field unresolved",
        provenance: "metrics.vital_event_features",
        sourceDetail: "respiratory packet field unresolved",
        useVitalCandidateStatus: false
      )
    case "resting-hr":
      return restingHeartRateHealthMonitorSnapshot(base: snapshot, allowLiveFallbacks: allowLiveFallbacks)
    case "resting-hrv":
      return hrvHealthMonitorSnapshot(base: snapshot)
    case "oxygen-saturation":
      if let stored = dailyRecoveryMetricSnapshot(
        base: snapshot,
        valueKey: "oxygen_saturation_percent",
        unit: "%",
        fractionDigits: 0,
        metricName: "oxygen saturation"
      ) {
        return stored
      }
      if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "oxygen_saturation_percent") {
        return unavailablePacketSnapshot(
          base: snapshot,
          status: "Unavailable",
          freshness: unavailable["date_key"] as? String ?? "Stored blocker",
          provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
          sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
        )
      }
      if packetInputStatus != "No run", decodedPacketFrameCount() > 0 {
        let pipCount = pulseInformationPacketCount()
        if pipCount > 0 {
          return unavailablePacketSnapshot(
            base: snapshot,
            status: "PIP candidate",
            freshness: "\(pipCount) K25/K26 packets",
            provenance: "metrics.vital_event_features",
            sourceDetail: "K25/K26 pulse-information packets present; SpO2 field unresolved"
          )
        }
      }
      return unresolvedPacketVitalSnapshot(
        base: snapshot,
        status: "Field unresolved",
        freshness: packetInputStatus == "No run" ? "Run pending" : "SpO2 field unresolved",
        provenance: "metrics.vital_event_features",
        sourceDetail: "SpO2 packet field unresolved",
        useVitalCandidateStatus: false
      )
    case "wrist-temperature":
      return wristTemperatureHealthMonitorSnapshot(base: snapshot)
    default:
      return snapshot
    }
  }

  func wristTemperatureHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    if let stored = dailyRecoveryMetricSnapshot(
      base: snapshot,
      valueKey: "skin_temperature_delta_c",
      unit: "C",
      fractionDigits: 1,
      metricName: "skin temperature delta",
      signed: true
    ) {
      return stored
    }
    if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "skin_temperature_delta_c") {
      return unavailablePacketSnapshot(
        base: snapshot,
        status: "Unavailable",
        freshness: unavailable["date_key"] as? String ?? "Stored blocker",
        provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
        sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
      )
    }
    return unresolvedPacketVitalSnapshot(
      base: snapshot,
      status: "Semantics pending",
      freshness: packetInputStatus == "No run" ? "Run pending" : "Semantics pending",
      provenance: "metrics.vital_event_features",
      sourceDetail: "temperature semantics unverified; candidate not promoted",
      useVitalCandidateStatus: true
    )
  }

  func restingHeartRateHealthMonitorSnapshot(
    base snapshot: HealthMetricSnapshot,
    allowLiveFallbacks: Bool = true
  ) -> HealthMetricSnapshot {
    let dailyRecoveryRHRMetrics = dailyRecoveryMetricsWithRestingHR()
    if let metric = Self.preferredDailyRecoveryMetricWithRestingHR(from: dailyRecoveryRHRMetrics),
       let value = Self.doubleValue(metric["resting_hr_bpm"]),
       let valueText = Self.numberText(value, fractionDigits: 0) {
      return replacingHealthMonitorSnapshot(
        snapshot,
        value: valueText,
        unit: "bpm",
        status: dailyRecoveryRestingHRStatus(metric),
        freshness: metric["date_key"] as? String ?? "Latest",
        provenance: "daily_recovery_metrics | \(dailyRecoveryRestingHRProvenanceSummary(metric))",
        source: dailyRecoveryRestingHRSource(metric),
        trend: Self.restingHeartRateDailyRecoveryTrend(
          base: snapshot.trend,
          metrics: dailyRecoveryRHRMetrics
        )
      )
    }

    if let rollup = packetInputReports["resting_hr_rollup"],
       Self.boolValue(rollup["pass"]) == true,
       let value = Self.doubleValue(rollup["resting_hr_bpm"]),
       let valueText = Self.numberText(value, fractionDigits: 0) {
      return replacingHealthMonitorSnapshot(
        snapshot,
        value: valueText,
        unit: "bpm",
        status: "Packet-derived",
        freshness: Self.rollupFreshnessText(in: rollup) ?? "Today",
        provenance: "metrics.resting_hr_daily_rollup | \(restingHeartRateFeatureProvenanceSummary())",
        source: .bridgeDeviceSensor("metrics.resting_hr_daily_rollup"),
        trend: Self.restingHeartRateRollupTrend(
          base: snapshot.trend,
          report: rollup
        )
      )
    }

    guard let report = packetInputReports["resting_hr"] else {
      if let fallback = storedHRDerivedRestingHeartRateHealthMonitorSnapshot(base: snapshot) {
        return fallback
      }
      if allowLiveFallbacks,
         let fallback = liveHRDerivedRestingHeartRateHealthMonitorSnapshot(base: snapshot) {
        return fallback
      }
      if let fallback = hkRestingHRHealthMonitorSnapshot(base: snapshot) {
        return fallback
      }
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: packetCount > 0 ? "Extractor pending" : "Run pending",
        freshness: packetCountText(packetCount) ?? "No packet run",
        provenance: "metrics.resting_hr_features",
        sourceDetail: "resting HR packet feature not run"
      )
    }

    let resting = Self.map(report, "resting")
    let daily = Self.array(report["daily"])
    let value = Self.doubleValue(resting?["resting_hr_bpm"])
      ?? daily.last.flatMap { Self.doubleValue($0["resting_hr_bpm"]) }
    guard let value, let valueText = Self.numberText(value, fractionDigits: 0) else {
      if let fallback = storedHRDerivedRestingHeartRateHealthMonitorSnapshot(base: snapshot) {
        return fallback
      }
      if allowLiveFallbacks,
         let fallback = liveHRDerivedRestingHeartRateHealthMonitorSnapshot(base: snapshot) {
        return fallback
      }
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: packetCount > 0 ? "Field unresolved" : "No packet data",
        freshness: firstPacketAction(in: report) ?? packetCountText(packetCount) ?? "No RHR",
        provenance: restingHeartRateFeatureProvenanceSummary(),
        sourceDetail: "resting HR packet feature unavailable"
      )
    }

    let trend = Self.dailyTrend(
      id: snapshot.trend.id,
      title: snapshot.trend.title,
      rows: daily,
      valueKey: "resting_hr_bpm",
      unit: "bpm",
      fractionDigits: 0,
      resources: snapshot.trend.resources
    )
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "bpm",
      status: "Packet-derived",
      freshness: Self.latestDailyDateText(in: report) ?? "Latest packet",
      provenance: "metrics.resting_hr_features | \(restingHeartRateFeatureProvenanceSummary())",
      source: .bridgeDeviceSensor("metrics.resting_hr_features"),
      trend: trend
    )
  }

  func liveHRDerivedRestingHeartRateHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot? {
    guard let sample = Self.liveHRDerivedRestingHeartRateSample() else {
      return nil
    }
    return liveHRDerivedRestingHeartRateHealthMonitorSnapshot(base: snapshot, sample: sample)
  }

  func hkRestingHRHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot? {
    guard let bpm = hkRestingHR,
          let valueText = Self.numberText(bpm, fractionDigits: 0) else { return nil }
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "bpm",
      status: "Apple Health",
      freshness: "From Health app",
      provenance: "apple.health.resting_heart_rate",
      source: .local("apple.health"),
      trend: snapshot.trend
    )
  }

  func storedHRDerivedRestingHeartRateHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot? {
    guard let sample = Self.storedHRDerivedRestingHeartRateSample() else {
      return nil
    }
    let valueText = Self.numberText(sample.bpm, fractionDigits: 0) ?? "\(Int(sample.bpm.rounded()))"
    let trend = Self.liveHeartRateHourlyTrend(
      base: snapshot.trend,
      fallbackValue: sample.bpm,
      fallbackValueText: valueText,
      sampleCount: sample.sampleCount
    )
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "bpm",
      status: "Local daily estimate",
      freshness: Self.relativeText(for: sample.updatedAt).map { "Updated \($0)" } ?? "Stored",
      provenance: "heart_rate_sample_store.low_quartile | samples=\(sample.sampleCount) | \(sample.source)",
      source: .local("Heart-rate sample store"),
      trend: trend
    )
  }

  func liveHRDerivedRestingHeartRateHealthMonitorSnapshot(
    base snapshot: HealthMetricSnapshot,
    sample: LiveHRDerivedRestingHeartRateSample
  ) -> HealthMetricSnapshot {
    let valueText = Self.numberText(sample.bpm, fractionDigits: 0) ?? "\(Int(sample.bpm.rounded()))"
    let trend = Self.liveHeartRateHourlyTrend(
      base: snapshot.trend,
      fallbackValue: sample.bpm,
      fallbackValueText: valueText,
      sampleCount: sample.sampleCount
    )
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "bpm",
      status: "HR-derived estimate",
      freshness: Self.relativeText(for: sample.updatedAt).map { "Estimated \($0)" } ?? "Estimated",
      provenance: "ble.heart_rate.low_quartile | samples=\(sample.sampleCount) | \(sample.source)",
      source: .live("BLE heart-rate estimate"),
      trend: trend
    )
  }

  static func liveHeartRateHourlyTrend(
    base trend: HealthTrendModel,
    fallbackValue: Double,
    fallbackValueText: String,
    sampleCount: Int
  ) -> HealthTrendModel {
    let snapshot = HeartRateSeriesStore.shared.timelineSnapshot()
    let ranges = snapshot.ranges
    let points = ranges.map { range in
      HealthTrendPoint(
        label: range.hourStart.formatted(.dateTime.hour(.defaultDigits(amPM: .omitted))),
        value: Double(range.averageBPM)
      )
    }
    guard !points.isEmpty else {
      return HealthTrendModel(
        id: trend.id,
        title: trend.title,
        rangeLabel: "\(fallbackValueText) bpm resting HR estimate",
        summary: "HR-derived resting estimate | \(sampleCount) HR samples",
        analysis: "Estimated locally from the lowest quartile of recent BLE heart-rate samples. Packet-derived resting HR remains the preferred source once enough heart-rate feature inputs are available.",
        resources: trend.resources,
        points: [HealthTrendPoint(label: "Estimate", value: fallbackValue)]
      )
    }

    let values = ranges.flatMap { [Double($0.minBPM), Double($0.maxBPM)] }
    let range = rangeText(values: values, unit: "bpm", fractionDigits: 0) ?? "\(fallbackValueText) bpm"
    return HealthTrendModel(
      id: trend.id,
      title: trend.title,
      rangeLabel: "\(range) today",
      summary: "\(points.count) hourly HR buckets today | \(range)",
      analysis: "Hourly 1-day view from locally stored BLE heart-rate samples. Resting HR uses the low-quartile estimate while packet-derived resting HR is still resolving.",
      resources: trend.resources,
      points: points
    )
  }

  func hrvHealthMonitorSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    if let stored = dailyRecoveryMetricSnapshot(
      base: snapshot,
      valueKey: "hrv_rmssd_ms",
      unit: "ms",
      fractionDigits: 0,
      metricName: "HRV"
    ) {
      return stored
    }
    guard let report = packetInputReports["hrv"] else {
      if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "hrv_rmssd_ms") {
        return unavailablePacketSnapshot(
          base: snapshot,
          status: "Unavailable",
          freshness: unavailable["date_key"] as? String ?? "Stored blocker",
          provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
          sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
        )
      }
      if let ms = hkHRVSDNNMs, let valueText = Self.numberText(ms, fractionDigits: 0) {
        return replacingHealthMonitorSnapshot(
          snapshot, value: valueText, unit: "ms", status: "Apple Health",
          freshness: "From Health app", provenance: "apple.health.hrv_sdnn",
          source: .local("apple.health"), trend: snapshot.trend
        )
      }
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: packetCount > 0 ? "Extractor pending" : "Run pending",
        freshness: packetCountText(packetCount) ?? "No packet run",
        provenance: "metrics.hrv_features",
        sourceDetail: "validated HRV packet feature not run"
      )
    }

    guard Self.boolValue(report["pass"]) == true else {
      if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "hrv_rmssd_ms") {
        return unavailablePacketSnapshot(
          base: snapshot,
          status: "Unavailable",
          freshness: unavailable["date_key"] as? String ?? "Stored blocker",
          provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
          sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
        )
      }
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: "Validation pending",
        freshness: firstPacketAction(in: report) ?? packetCountText(packetCount) ?? "HRV unverified",
        provenance: hrvFeatureProvenanceSummary(),
        sourceDetail: "HRV requires validated beat-interval semantics before display"
      )
    }

    let output = Self.map(report, "score_result", "output")
    let daily = Self.array(report["daily"])
    let value = Self.doubleValue(output?["rmssd_ms"])
      ?? daily.last.flatMap { Self.doubleValue($0["rmssd_ms"]) }
    guard let value, let valueText = Self.numberText(value, fractionDigits: 0) else {
      if let unavailable = preferredDailyRecoveryUnavailableMetric(metricID: "hrv_rmssd_ms") {
        return unavailablePacketSnapshot(
          base: snapshot,
          status: "Unavailable",
          freshness: unavailable["date_key"] as? String ?? "Stored blocker",
          provenance: Self.recoveryUnavailableProvenanceSummary(unavailable),
          sourceDetail: Self.recoveryUnavailableSourceDetail(unavailable)
        )
      }
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: packetCount > 0 ? "Field unresolved" : "No packet data",
        freshness: firstPacketAction(in: report) ?? packetCountText(packetCount) ?? "No HRV",
        provenance: hrvFeatureProvenanceSummary(),
        sourceDetail: "validated HRV packet feature unavailable"
      )
    }

    let trend = Self.dailyTrend(
      id: snapshot.trend.id,
      title: snapshot.trend.title,
      rows: daily,
      valueKey: "rmssd_ms",
      unit: "ms",
      fractionDigits: 0,
      resources: snapshot.trend.resources
    )
    return replacingHealthMonitorSnapshot(
      snapshot,
      value: valueText,
      unit: "ms",
      status: "Packet-derived",
      freshness: Self.latestDailyDateText(in: report) ?? "Latest packet",
      provenance: "metrics.hrv_features | \(hrvFeatureProvenanceSummary())",
      source: .bridgeDeviceSensor("metrics.hrv_features"),
      trend: trend
    )
  }

  func unresolvedPacketVitalSnapshot(
    base snapshot: HealthMetricSnapshot,
    status: String,
    freshness: String,
    provenance: String,
    sourceDetail: String,
    useVitalCandidateStatus: Bool
  ) -> HealthMetricSnapshot {
    if packetInputStatus == "No run" {
      return unavailablePacketSnapshot(
        base: snapshot,
        status: "Run pending",
        freshness: "No packet run",
        provenance: provenance,
        sourceDetail: sourceDetail
      )
    }

    guard packetInputReports["vital_event"] != nil else {
      let packetCount = packetEvidenceFrameCount()
      return unavailablePacketSnapshot(
        base: snapshot,
        status: status,
        freshness: packetCountText(packetCount) ?? packetInputStatus,
        provenance: provenance,
        sourceDetail: sourceDetail
      )
    }

    let packetCount = packetEvidenceFrameCount()
    if packetCount == 0 {
      return unavailablePacketSnapshot(
        base: snapshot,
        status: "No packets",
        freshness: "Sync pending",
        provenance: provenance,
        sourceDetail: "no decoded packet frames imported"
      )
    }

    guard useVitalCandidateStatus,
          let report = packetInputReports["vital_event"] else {
      return unavailablePacketSnapshot(
        base: snapshot,
        status: status,
        freshness: packetCountText(packetCount) ?? freshness,
        provenance: provenance,
        sourceDetail: sourceDetail
      )
    }

    let temperatureTotal = Self.intValue(report["skin_temperature_input_count"])
      ?? Self.array(report["skin_temperature_inputs"]).count
    let total = max(
      temperatureTotal,
      Self.intValue(report["feature_count"]) ?? Self.array(report["features"]).count
    )
    guard total > 0 else {
      return unavailablePacketSnapshot(
        base: snapshot,
        status: status,
        freshness: packetCountText(packetCount) ?? "No candidates",
        provenance: vitalEventFeatureProvenanceSummary(),
        sourceDetail: sourceDetail
      )
    }

    let trusted = max(
      Self.intValue(report["trusted_skin_temperature_input_count"]) ?? 0,
      Self.intValue(report["trusted_feature_count"]) ?? 0
    )
    return unavailablePacketSnapshot(
      base: snapshot,
      status: "Candidate only",
      freshness: "\(trusted)/\(total) candidates",
      provenance: vitalEventFeatureProvenanceSummary(),
      sourceDetail: sourceDetail
    )
  }

  func decodedPacketFrameCount() -> Int {
    Self.intValue(packetInputReports["vital_event"]?["decoded_frame_count"]) ?? 0
  }

  func packetEvidenceFrameCount() -> Int {
    let reportCounts = packetInputReports.values.flatMap { report in
      [
        "decoded_frame_count",
        "candidate_frame_count",
        "data_packet_frame_count",
        "feature_count",
        "heart_rate_feature_count",
        "motion_feature_count",
        "trusted_feature_count",
        "trusted_heart_rate_feature_count",
      ].compactMap { Self.intValue(report[$0]) }
    }
    return ([decodedPacketFrameCount()] + reportCounts).max() ?? 0
  }

  func packetCountText(_ count: Int) -> String? {
    count > 0 ? "\(count) packets decoded" : nil
  }

  func pulseInformationPacketCount() -> Int {
    Self.intValue(packetInputReports["vital_event"]?["pulse_information_packet_count"]) ?? 0
  }

  func latestSkinTemperatureInput() -> [String: Any]? {
    Self.array(packetInputReports["vital_event"]?["skin_temperature_inputs"]).last
  }

  func unavailablePacketSnapshot(
    base snapshot: HealthMetricSnapshot,
    status: String,
    freshness: String,
    provenance: String,
    sourceDetail: String
  ) -> HealthMetricSnapshot {
    replacingHealthMonitorSnapshot(
      snapshot,
      value: "--",
      unit: snapshot.unit,
      status: status,
      freshness: freshness,
      provenance: provenance,
      source: .unavailable(sourceDetail),
      trend: Self.emptyTrend(from: snapshot.trend, packetCount: packetEvidenceFrameCount())
    )
  }

  func firstPacketAction(in report: [String: Any]) -> String? {
    Self.firstActionText(in: report).map { text in
      text.count > 26 ? "\(text.prefix(26))..." : text
    }
  }

  func replacingHealthMonitorSnapshot(
    _ snapshot: HealthMetricSnapshot,
    value: String,
    unit: String,
    status: String,
    freshness: String,
    provenance: String,
    source: HealthDataSource,
    trend: HealthTrendModel
  ) -> HealthMetricSnapshot {
    HealthMetricSnapshot(
      id: snapshot.id,
      route: snapshot.route,
      group: snapshot.group,
      title: snapshot.title,
      value: value,
      unit: unit,
      status: status,
      freshness: freshness,
      provenance: provenance,
      source: source,
      systemImage: snapshot.systemImage,
      tint: snapshot.tint,
      trend: trend
    )
  }
}
