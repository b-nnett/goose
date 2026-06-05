import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func metricInputReadinessSummary() -> String {
    guard let report = packetInputReports["readiness"] else {
      return packetInputStatus == "No run" ? "No run | bridge extract available" : packetInputStatus
    }
    let status = Self.passStatus(report)
    let ready = Self.intValue(report["ready_family_count"]) ?? 0
    let total = Self.intValue(report["family_count"]) ?? Self.array(report["families"]).count
    return "\(status) | \(ready)/\(total) score families ready"
  }

  func metricInputReadinessNextActionSummary() -> String {
    if let action = Self.firstActionText(in: packetInputReports["readiness"]) {
      return action
    }
    return packetInputStatus == "No run" ? "Run Extract to populate packet-derived inputs" : ""
  }

  func latestHeartRateSummary(bpm: Int?, source: String, updatedAt: Date?) -> String {
    guard let bpm else {
      return "No HR extraction"
    }
    let freshness = Self.relativeText(for: updatedAt) ?? "Now"
    return "\(bpm) bpm | trusted | \(freshness)"
  }

  func latestHeartRateProvenanceSummary(source: String) -> String {
    source == "waiting" ? "" : "source_signal=\(source) | trusted_metric_input=true"
  }

  func motionFeatureSummary() -> String {
    guard let report = packetInputReports["motion"] else {
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let trusted = Self.intValue(report["trusted_feature_count"]) ?? 0
    let total = Self.intValue(report["feature_count"]) ?? Self.array(report["features"]).count
    return "\(Self.passStatus(report)) | \(trusted)/\(total) trusted motion inputs"
  }

  func motionFeatureProvenanceSummary() -> String {
    guard let feature = Self.firstMap(in: packetInputReports["motion"], key: "features") else {
      return ""
    }
    let kind = feature["body_summary_kind"] as? String ?? "motion"
    return "body_summary_kind=\(kind) | trusted_metric_input=\(Self.boolText(feature["trusted_metric_input"]))"
  }

  func stepDiscoverySummary() -> String {
    guard let report = packetInputReports["step_discovery"] else {
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let inspected = Self.intValue(report["inspected_frame_count"]) ?? 0
    let candidates = Self.intValue(report["candidate_field_count"]) ?? 0
    let counter = Self.boolValue(report["explicit_step_counter_found"]) == true ? "counter found" : "no counter"
    return "\(Self.passStatus(report)) | \(inspected) inspected | \(candidates) fields | \(counter)"
  }

  func stepDiscoveryProvenanceSummary() -> String {
    guard let report = packetInputReports["step_discovery"] else {
      return ""
    }
    if let field = Self.firstMap(in: report, key: "candidate_fields") {
      let path = field["json_path"] as? String ?? "decoded field"
      let kind = field["match_kind"] as? String ?? "candidate"
      let source = field["source_kind_inference"] as? String ?? "unknown"
      return "\(kind) | \(path) | \(source)"
    }
    let families = Self.map(report, "inspected_packet_family_counts")?.keys.sorted().joined(separator: ", ") ?? "none"
    return "families=\(families) | issues=\(Self.array(report["issues"]).count)"
  }

  func hrvFeatureSummary() -> String {
    guard let report = packetInputReports["hrv"] else {
      if let diagnostic = rrDerivedHRVDiagnosticSummary() {
        return "No validated packet HRV | \(diagnostic)"
      }
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let rr = Self.intValue(report["trusted_rr_interval_count"])
      ?? Self.intValue(report["rr_interval_count"])
      ?? 0
    let baseline = Self.map(report, "baseline")
    let baselineText = Self.numberText(baseline?["hrv_baseline_rmssd_ms"], fractionDigits: 1) ?? "no baseline"
    guard Self.boolValue(report["pass"]) == true else {
      let action = firstPacketAction(in: report) ?? "beat-interval validation pending"
      let diagnostic = rrDerivedHRVDiagnosticSummary().map { " | \($0)" } ?? ""
      return "\(Self.passStatus(report)) | \(rr) RR candidates | RMSSD held | \(action)\(diagnostic)"
    }
    let output = Self.map(report, "score_result", "output")
    guard let rmssd = Self.numberText(output?["rmssd_ms"], fractionDigits: 1) else {
      return "\(Self.passStatus(report)) | \(rr) RR | no RMSSD | \(baselineText) ms base"
    }
    return "\(Self.passStatus(report)) | \(rr) RR | \(rmssd) ms RMSSD | \(baselineText) ms base"
  }

  func hrvFeatureProvenanceSummary() -> String {
    guard let report = packetInputReports["hrv"] else {
      return rrDerivedHRVDiagnosticSummary() ?? ""
    }
    let algorithm = Self.map(report, "score_result")?["algorithm_id"] as? String ?? "goose.hrv.v0"
    return "algorithm=\(algorithm) | pass=\(Self.boolValue(report["pass"]) == true) | daily=\(Self.array(report["daily"]).count) | issues=\(Self.array(report["issues"]).count)"
  }

  func rrDerivedHRVDiagnosticSummary() -> String? {
    guard let sample = Self.storedHRVDerivedHRVSample() ?? Self.liveRRDerivedHRVSample() else {
      return nil
    }
    let freshness = Self.relativeText(for: sample.updatedAt) ?? "Latest"
    return "RR candidate only | source=\(sample.source) | chunks=\(sample.sampleCount) | rr=\(sample.rrIntervalCount) | \(freshness)"
  }

  func restingHeartRateFeatureSummary() -> String {
    if let rollup = packetInputReports["resting_hr_rollup"],
       let restingText = Self.numberText(rollup["resting_hr_bpm"], fractionDigits: 0) {
      let average = Self.numberText(rollup["rolling_7_day_average_bpm"], fractionDigits: 0) ?? "no"
      let delta = Self.signedNumberText(rollup["selected_vs_7_day_average_bpm"], fractionDigits: 0) ?? "0"
      return "\(Self.passStatus(rollup)) | \(restingText) bpm rest | 7d \(average) bpm avg | \(delta) bpm vs avg"
    }
    guard let report = packetInputReports["resting_hr"] else {
      if let sample = Self.liveHRDerivedRestingHeartRateSample(),
         let bpm = Self.numberText(sample.bpm, fractionDigits: 0) {
        return "HR-derived estimate | \(sample.sampleCount) HR | \(bpm) bpm rest | no baseline"
      }
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let resting = Self.map(report, "resting")
    let baseline = Self.map(report, "baseline")
    let baselineText = Self.numberText(baseline?["resting_hr_baseline_bpm"], fractionDigits: 0) ?? "no baseline"
    guard let restingText = Self.numberText(resting?["resting_hr_bpm"], fractionDigits: 0) else {
      if let sample = Self.liveHRDerivedRestingHeartRateSample(),
         let bpm = Self.numberText(sample.bpm, fractionDigits: 0) {
        return "\(Self.passStatus(report)) | HR-derived estimate | \(sample.sampleCount) HR | \(bpm) bpm rest | \(baselineText) bpm base"
      }
      return "\(Self.passStatus(report)) | no resting HR | \(baselineText) bpm base"
    }
    return "\(Self.passStatus(report)) | \(restingText) bpm rest | \(baselineText) bpm base"
  }

  func restingHeartRateFeatureProvenanceSummary() -> String {
    if let rollup = packetInputReports["resting_hr_rollup"] {
      let samples = Self.intValue(rollup["sample_count"]) ?? 0
      let confidence = Self.numberText(rollup["confidence"], fractionDigits: 2) ?? "0"
      let written = Self.boolValue(rollup["daily_metric_written"]) == true ? "stored" : "not stored"
      return "daily_metric=\(written) | samples=\(samples) | confidence=\(confidence)"
    }
    guard let report = packetInputReports["resting_hr"] else {
      if let sample = Self.liveHRDerivedRestingHeartRateSample() {
        let freshness = Self.relativeText(for: sample.updatedAt) ?? "Latest"
        return "source=\(sample.source) | samples=\(sample.sampleCount) | \(freshness)"
      }
      return ""
    }
    return "daily=\(Self.array(report["daily"]).count) | trusted_hr=\(Self.intValue(report["trusted_heart_rate_feature_count"]) ?? 0)"
  }

  func windowFeatureSummary() -> String {
    guard let report = packetInputReports["window"] else {
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let window = Self.map(report, "window")
    let duration = Self.numberText(window?["duration_minutes"], fractionDigits: 1) ?? "no duration"
    let average = Self.numberText(window?["average_hr_bpm"], fractionDigits: 0) ?? "no HR"
    return "\(Self.passStatus(report)) | \(duration) min | \(average) bpm avg"
  }

  func windowFeatureProvenanceSummary() -> String {
    guard let report = packetInputReports["window"] else {
      return ""
    }
    return "hr_features=\(Self.intValue(report["heart_rate_feature_count"]) ?? 0) | motion_features=\(Self.intValue(report["motion_feature_count"]) ?? 0)"
  }

  func vitalEventFeatureSummary() -> String {
    guard let report = packetInputReports["vital_event"] else {
      return packetInputStatus == "No run" ? "No run" : packetInputStatus
    }
    let trusted = Self.intValue(report["trusted_feature_count"]) ?? 0
    let total = Self.intValue(report["feature_count"]) ?? Self.array(report["features"]).count
    let resolved = Self.intValue(report["resolved_metric_input_count"]) ?? 0
    let temp = Self.intValue(report["skin_temperature_input_count"]) ?? Self.array(report["skin_temperature_inputs"]).count
    let pip = Self.intValue(report["pulse_information_packet_count"]) ?? 0
    let respRpm = Self.array(report["respiratory_rate_inputs"])
      .compactMap { ($0 as? [String: Any]) }
      .first { Self.boolValue($0["resolved_metric_input"]) == true }
      .flatMap { Self.numberText($0["respiratory_rate_rpm"], fractionDigits: 1) }
    let respText = respRpm.map { "\($0) rpm resp" } ?? "resp held"
    return "\(Self.passStatus(report)) | \(respText) | \(trusted)/\(total) events | temp \(temp) | PIP \(pip)"
  }

  func vitalEventFeatureProvenanceSummary() -> String {
    if let feature = Self.firstMap(in: packetInputReports["vital_event"], key: "features") {
      let eventName = feature["event_name"] as? String ?? "vital_event"
      return "\(eventName) | semantics_verified=\(Self.boolText(feature["value_semantics_verified"]))"
    }
    if let input = latestSkinTemperatureInput() {
      let schema = input["schema_field"] as? String ?? "skin_temperature_candidate"
      return "\(schema) | semantics_verified=\(Self.boolText(input["value_semantics_verified"]))"
    }
    return ""
  }

  func recoveryUnavailableStatusSummary() -> String {
    guard let report = packetInputReports["recovery_unavailable_status"] else {
      let unavailableCount = dailyRecoveryUnavailableMetrics().count
      if unavailableCount > 0 {
        return "\(unavailableCount) stored unavailable recovery metrics"
      }
      return packetInputStatus == "No run" ? "No run" : "No unavailable recovery status"
    }
    let count = Self.intValue(report["unavailable_metric_count"]) ?? Self.array(report["statuses"]).count
    let written = Self.intValue(report["written_metric_count"]) ?? 0
    let blocker = Self.array(report["statuses"]).compactMap { Self.firstRecoveryUnavailableBlocker($0) }.first
    let blockerText = blocker.map { " | \($0)" } ?? ""
    return "\(Self.passStatus(report)) | \(count) unavailable | stored \(written)\(blockerText)"
  }

  func recoverySensorDailyRollupSummary() -> String {
    guard let report = packetInputReports["recovery_sensor_rollup"] else {
      return packetInputStatus == "No run" ? "No run" : "No recovery sensor rollup"
    }
    let promoted = Self.intValue(report["promoted_metric_count"]) ?? 0
    let promotable = Self.intValue(report["promotable_metric_count"]) ?? 0
    let written = Self.intValue(report["written_metric_count"]) ?? 0
    let total = Self.intValue(report["metric_count"]) ?? Self.array(report["statuses"]).count
    let blocker = Self.array(report["statuses"])
      .compactMap { Self.stringArray($0["blocker_reasons"]).first }
      .first
    let blockerText = blocker.map { " | \($0)" } ?? ""
    return "\(Self.passStatus(report)) | \(promoted)/\(total) promoted | promotable \(promotable) | stored \(written)\(blockerText)"
  }

  func activityUnavailableStatusSummary() -> String {
    guard let report = packetInputReports["activity_unavailable_status"] else {
      let unavailableCount = dailyActivityUnavailableMetrics(metricID: "steps").count
      if unavailableCount > 0 {
        return "\(unavailableCount) stored unavailable step metrics"
      }
      return packetInputStatus == "No run" ? "No run" : "No unavailable activity status"
    }
    let count = Self.intValue(report["unavailable_metric_count"]) ?? Self.array(report["statuses"]).count
    let written = Self.intValue(report["written_metric_count"]) ?? 0
    let blocker = Self.array(report["statuses"]).compactMap { Self.firstActivityUnavailableBlocker($0) }.first
    let blockerText = blocker.map { " | \($0)" } ?? ""
    return "\(Self.passStatus(report)) | \(count) unavailable | stored \(written)\(blockerText)"
  }

  func energyUnavailableStatusSummary() -> String {
    guard let report = packetInputReports["energy_unavailable_status"] else {
      let unavailableCount = ["active_kcal", "resting_kcal", "total_kcal"]
        .reduce(0) { count, metricID in
          count + dailyActivityUnavailableMetrics(metricID: metricID).count
        }
      if unavailableCount > 0 {
        return "\(unavailableCount) stored unavailable energy metrics"
      }
      return packetInputStatus == "No run" ? "No run" : "No unavailable energy status"
    }
    let count = Self.intValue(report["unavailable_metric_count"]) ?? Self.array(report["statuses"]).count
    let written = Self.intValue(report["written_metric_count"]) ?? 0
    let blocker = Self.array(report["statuses"]).compactMap { Self.firstActivityUnavailableBlocker($0) }.first
    let blockerText = blocker.map { " | \($0)" } ?? ""
    return "\(Self.passStatus(report)) | \(count) unavailable | stored \(written)\(blockerText)"
  }

  func packetDerivedFeatureNextActionSummary() -> String {
    if let action = Self.firstActionText(in: packetInputReports["readiness"])
      ?? Self.firstActionText(in: packetInputReports["step_discovery"])
      ?? Self.firstActionText(in: packetInputReports["activity_unavailable_status"])
      ?? Self.firstActionText(in: packetInputReports["energy_unavailable_status"])
      ?? Self.firstActionText(in: packetInputReports["recovery_sensor_rollup"])
      ?? Self.firstActionText(in: packetInputReports["recovery_unavailable_status"])
      ?? Self.firstActionText(in: packetInputReports["vital_event"]) {
      return action
    }
    return packetInputStatus == "No run" ? "Run Extract to populate packet-derived inputs" : "Capture trusted vitals packets for respiratory, SpO2, and temperature"
  }

  func sleepFeatureScoreSummary() -> String {
    guard let report = packetScoreReports["sleep"] else {
      return packetScoreStatus == "No run" ? "No run" : packetScoreStatus
    }
    let output = Self.map(report, "score_result", "output")
    let score = Self.numberText(output?["score_0_to_100"], fractionDigits: 1) ?? "no score"
    let window = Self.map(report, "sleep_window")
    let asleep = Self.numberText(window?["sleep_duration_minutes"], fractionDigits: 0) ?? "no duration"
    let state = Self.map(output, "status_report")?["report_state"] as? String
    let prefix = state.map { "\($0.capitalized) | " } ?? ""
    return "\(prefix)\(Self.passStatus(report)) | \(score) sleep | \(asleep) min"
  }

  func sleepV1ModelStatusSummary() -> String {
    guard let report = packetScoreReports["sleep"],
          let status = Self.map(report, "score_result", "output", "status_report") else {
      return packetScoreStatus == "No run" ? "" : "V1 status unavailable"
    }
    let state = status["report_state"] as? String ?? "provisional"
    let nights = Self.intValue(status["imported_platform_sleep_nights"]) ?? 0
    let trusted = Self.intValue(status["trusted_goose_sleep_nights"]) ?? 0
    return "\(state.capitalized) | \(nights) imported nights | \(trusted) trusted goose nights"
  }

  func sleepV1ConfidenceSummary() -> String {
    guard let report = packetScoreReports["sleep"] else {
      return packetScoreStatus == "No run" ? "" : "confidence unavailable"
    }
    let output = Self.map(report, "score_result", "output")
    let confidence = Self.percentText(output?["confidence_0_to_1"]) ?? "no confidence"
    let window = Self.percentText(output?["sleep_window_confidence_0_to_1"]) ?? "no window"
    let coverage = Self.percentText(output?["data_coverage_fraction"]) ?? "no coverage"
    return "\(confidence) confidence | \(window) window | \(coverage) coverage"
  }

  func sleepV1DataNotesSummary() -> String {
    guard let report = packetScoreReports["sleep"] else {
      return ""
    }
    let issues = Self.array(report["issues"]).count
    let actions = Self.array(report["next_actions"]).count
    return "\(Self.passStatus(report)) | issues \(issues) | actions \(actions)"
  }

  func sleepV1ScheduleSummary() -> String {
    guard let output = Self.map(packetScoreReports["sleep"], "score_result", "output") else {
      return ""
    }
    let bed = Self.numberText(output["bedtime_deviation_minutes"], fractionDigits: 0) ?? "0"
    let wake = Self.numberText(output["wake_time_deviation_minutes"], fractionDigits: 0) ?? "0"
    let mid = Self.numberText(output["midpoint_deviation_minutes"], fractionDigits: 0) ?? "0"
    return "bed \(bed)m | wake \(wake)m | mid \(mid)m"
  }

  func sleepV1DebtSummary() -> String {
    guard let output = Self.map(packetScoreReports["sleep"], "score_result", "output") else {
      return ""
    }
    let tonight = Self.numberText(output["sleep_debt_minutes"], fractionDigits: 0) ?? "no"
    let rolling = Self.numberText(output["rolling_sleep_debt_minutes"], fractionDigits: 0) ?? "no"
    return "\(tonight)m tonight | \(rolling)m rolling"
  }

  func sleepV1HeartRateSummary() -> String {
    guard let output = Self.map(packetScoreReports["sleep"], "score_result", "output") else {
      return ""
    }
    let dip = Self.numberText(output["heart_rate_dip_percent"], fractionDigits: 1) ?? "no"
    let average = Self.numberText(output["sleep_hr_average_bpm"], fractionDigits: 0) ?? "no"
    let min = Self.numberText(output["sleep_hr_min_bpm"], fractionDigits: 0) ?? "no"
    return "\(dip)% dip | \(average) bpm avg | \(min) bpm min"
  }

  func sleepV1StagesSummary() -> String {
    guard let detail = primarySleepDetail else {
      return ""
    }
    return detail.stages.map { "\($0.stage) \(Int($0.durationMinutes.rounded()))m" }.joined(separator: " | ")
  }

  func sleepV1ArchitectureCalibrationSummary() -> String {
    guard let output = Self.map(packetScoreReports["sleep"], "score_result", "output") else {
      return ""
    }
    let component = Self.map(output, "component_provenance", "sleep_architecture")
    let confidence = Self.percentText(component?["confidence_0_to_1"]) ?? "architecture confidence pending"
    return "\(confidence) architecture confidence | source=packet-derived"
  }

  func sleepV1WhyChangedSummary() -> String {
    guard let comparison = Self.map(packetScoreReports["sleep"], "score_result", "output", "previous_night_comparison") else {
      return ""
    }
    let duration = Self.numberText(comparison["sleep_duration_delta_minutes"], fractionDigits: 0) ?? "0"
    let debt = Self.numberText(comparison["sleep_debt_delta_minutes"], fractionDigits: 0) ?? "0"
    let hr = Self.numberText(comparison["sleep_hr_average_delta_bpm"], fractionDigits: 0) ?? "0"
    return "duration \(duration)m vs prev | debt \(debt)m vs prev | HR avg \(hr) bpm"
  }

  func sleepV1ComponentBreakdownRows() -> [HealthSummaryRow] {
    guard packetScoreStatus != "No run" else {
      return []
    }
    let components = Self.array(Self.map(packetScoreReports["sleep"], "score_result", "output")?["components"])
    if !components.isEmpty {
      return components.enumerated().map { index, component in
        let label = component["name"] as? String ?? component["component_id"] as? String ?? "Component \(index + 1)"
        let score = Self.numberText(component["score_0_to_100"], fractionDigits: 0) ?? "no score"
        let weight = Self.percentText(component["weight_0_to_1"]) ?? "no weight"
        return HealthSummaryRow(label.capitalized, value: "\(score) score | \(weight) weight", source: .bridge("sleep v1 components"), systemImage: "chart.bar")
      }
    }
    return []
  }

  func recoveryFeatureScoreSummary() -> String {
    guard let report = packetScoreReports["recovery"] else {
      return packetScoreStatus == "No run" ? "No run" : packetScoreStatus
    }
    let score = Self.numberText(Self.map(report, "score_result", "output")?["score_0_to_100"], fractionDigits: 1) ?? "no score"
    return "\(Self.passStatus(report)) | \(score) recovery"
  }

  func recoveryProvidedVitalsSummary() -> String {
    if let vitals = Self.map(packetScoreReports["recovery"], "provided_vitals") {
      let source = vitals["source"] as? String ?? "provided"
      let flags = Self.stringArray(vitals["quality_flags"])
      let trusted = Self.recoveryProvidedVitalsAreTrusted(vitals)
      guard trusted else {
        let reason = flags.first ?? "packet-derived vitals required"
        return "blocked | \(reason)"
      }
      let rr = Self.numberText(vitals["respiratory_rate_rpm"], fractionDigits: 1) ?? "--"
      let baseline = Self.numberText(vitals["respiratory_rate_baseline_rpm"], fractionDigits: 1) ?? "--"
      let temp = Self.numberText(vitals["skin_temp_delta_c"], fractionDigits: 1) ?? "--"
      return "\(source) | \(rr) rpm | \(baseline) rpm baseline | \(temp) C"
    }
    if let report = packetScoreReports["recovery"] {
      let issues = Self.stringArray(report["issues"])
      if let issue = issues.first(where: { $0.hasPrefix("provided_resp_temp") }) {
        return "blocked | \(issue)"
      }
    }
    return "unavailable | decoded WHOOP packet vitals required"
  }

  func recoveryScoreDisplayValue() -> Int {
    Int((recoveryScoreValue() ?? 0).rounded())
  }

  func recoveryScoreDisplayText() -> String {
    guard let score = recoveryScoreValue(),
          let text = Self.numberText(score, fractionDigits: 0) else {
      return "--"
    }
    return "\(text)%"
  }

  func recoveryHRVDisplayText(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> String {
    guard !usesPreviewPacketData else {
      return "--"
    }
    if let metric = preferredDailyRecoveryMetric(valueKey: "hrv_rmssd_ms", for: date, calendar: calendar),
       let text = Self.numberText(metric["hrv_rmssd_ms"], fractionDigits: 0) {
      return "\(text) ms"
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return "--"
    }
    guard let report = packetInputReports["hrv"] else {
      return "--"
    }
    guard Self.boolValue(report["pass"]) == true else {
      return "--"
    }
    let output = Self.map(report, "score_result", "output")
    let value = Self.doubleValue(output?["rmssd_ms"])
      ?? Self.array(report["daily"]).last.flatMap { Self.doubleValue($0["rmssd_ms"]) }
    guard let value,
          let text = Self.numberText(value, fractionDigits: 0) else {
      return "--"
    }
    return "\(text) ms"
  }

  func recoveryHRVSource(for date: Date = Date(), calendar: Calendar = .current) -> HealthDataSource {
    guard !usesPreviewPacketData else {
      return .unavailable("preview packet data")
    }
    if let metric = preferredDailyRecoveryMetric(valueKey: "hrv_rmssd_ms", for: date, calendar: calendar) {
      return dailyRecoveryMetricSource(metric, metricName: "HRV")
    }
    if calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())),
       recoveryHRVDisplayText(for: date, calendar: calendar) != "--" {
      return .bridgeDeviceSensor("metrics.hrv_features")
    }
    if let detail = recoveryUnavailableSourceDetail(metricID: "hrv_rmssd_ms", for: date, calendar: calendar) {
      return .unavailable(detail)
    }
    return .unavailable("selected date has no stored HRV metric")
  }

  func recoveryRestingHRDisplayText(for date: Date = Date(), calendar: Calendar = .current) -> String {
    guard !usesPreviewPacketData else {
      return "--"
    }
    if let metric = preferredDailyRecoveryMetricWithRestingHR(for: date, calendar: calendar),
       let text = Self.numberText(metric["resting_hr_bpm"], fractionDigits: 0) {
      return "\(text) bpm"
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return "--"
    }
    if let rollup = packetInputReports["resting_hr_rollup"],
       Self.boolValue(rollup["pass"]) == true,
       let text = Self.numberText(rollup["resting_hr_bpm"], fractionDigits: 0) {
      return "\(text) bpm"
    }
    guard let report = packetInputReports["resting_hr"] else {
      if let sample = Self.liveHRDerivedRestingHeartRateSample(),
         let text = Self.numberText(sample.bpm, fractionDigits: 0) {
        return "\(text) bpm"
      }
      return "--"
    }
    let resting = Self.map(report, "resting")
    let value = Self.doubleValue(resting?["resting_hr_bpm"])
      ?? Self.array(report["daily"]).last.flatMap { Self.doubleValue($0["resting_hr_bpm"]) }
    guard let value,
          let text = Self.numberText(value, fractionDigits: 0) else {
      if let sample = Self.liveHRDerivedRestingHeartRateSample(),
         let text = Self.numberText(sample.bpm, fractionDigits: 0) {
        return "\(text) bpm"
      }
      return "--"
    }
    return "\(text) bpm"
  }

  func recoveryRestingHRSource(for date: Date = Date(), calendar: Calendar = .current) -> HealthDataSource {
    guard !usesPreviewPacketData else {
      return .unavailable("preview packet data")
    }
    if let metric = preferredDailyRecoveryMetricWithRestingHR(for: date, calendar: calendar) {
      return dailyRecoveryRestingHRSource(metric)
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return .unavailable("selected date has no stored resting HR metric")
    }
    let blockedRollupAction = packetInputReports["resting_hr_rollup"].map {
      firstPacketAction(in: $0) ?? "metrics.resting_hr_daily_rollup blocked"
    }
    if let rollup = packetInputReports["resting_hr_rollup"] {
      if Self.boolValue(rollup["pass"]) == true,
         Self.doubleValue(rollup["resting_hr_bpm"]) != nil {
        return .bridgeDeviceSensor("metrics.resting_hr_daily_rollup")
      }
    }
    if let report = packetInputReports["resting_hr"] {
      let resting = Self.map(report, "resting")
      let value = Self.doubleValue(resting?["resting_hr_bpm"])
        ?? Self.array(report["daily"]).last.flatMap { Self.doubleValue($0["resting_hr_bpm"]) }
      if value != nil {
        return .bridgeDeviceSensor("metrics.resting_hr_features")
      }
      return .unavailable(firstPacketAction(in: report) ?? "resting HR packet feature unavailable")
    }
    if let sample = Self.liveHRDerivedRestingHeartRateSample() {
      if sample.source.localizedCaseInsensitiveContains("store") {
        return .localEstimate("heart_rate_sample_store.low_quartile")
      }
      return .live("BLE heart-rate estimate")
    }
    if let blockedRollupAction {
      return .unavailable(blockedRollupAction)
    }
    return .unavailable("resting HR requires packet-derived HR samples")
  }

  func recoveryRespiratoryRateDisplayText(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> String {
    guard !usesPreviewPacketData else {
      return "--"
    }
    if let metric = preferredDailyRecoveryMetric(valueKey: "respiratory_rate_rpm", for: date, calendar: calendar),
       let text = Self.numberText(metric["respiratory_rate_rpm"], fractionDigits: 1) {
      return "\(text) rpm"
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return "--"
    }
    let value = currentRecoveryRespiratoryRateRPM() ?? 0
    guard value > 0,
          let text = Self.numberText(value, fractionDigits: 1) else {
      return "--"
    }
    return "\(text) rpm"
  }

  func recoveryRespiratoryRateSource(for date: Date = Date(), calendar: Calendar = .current) -> HealthDataSource {
    guard !usesPreviewPacketData else {
      return .unavailable("preview packet data")
    }
    if let metric = preferredDailyRecoveryMetric(valueKey: "respiratory_rate_rpm", for: date, calendar: calendar) {
      return dailyRecoveryMetricSource(metric, metricName: "respiratory rate")
    }
    if calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())),
       recoveryRespiratoryRateDisplayText(for: date, calendar: calendar) != "--" {
      if let vitals = Self.map(packetScoreReports["recovery"], "provided_vitals"),
         Self.recoveryProvidedVitalsAreTrusted(vitals) {
        return Self.recoveryProvidedVitalsSource(vitals)
      }
      return .bridgeDeviceSensor("packet-derived recovery vitals")
    }
    if let detail = recoveryUnavailableSourceDetail(metricID: "respiratory_rate_rpm", for: date, calendar: calendar) {
      return .unavailable(detail)
    }
    return .unavailable("selected date has no stored respiratory-rate metric")
  }

  func recoveryWristTemperatureDisplayText(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> String {
    guard !usesPreviewPacketData else {
      return "--"
    }
    if let metric = preferredDailyRecoveryMetric(valueKey: "skin_temperature_delta_c", for: date, calendar: calendar),
       let text = Self.signedNumberText(metric["skin_temperature_delta_c"], fractionDigits: 1) {
      return "\(text) C"
    }
    guard calendar.isDate(calendar.startOfDay(for: date), inSameDayAs: calendar.startOfDay(for: Date())) else {
      return "--"
    }
    let value = recoveryProvidedVitalsValue("skin_temp_delta_c") ?? 0
    guard value != 0,
          let text = Self.signedNumberText(value, fractionDigits: 1) else {
      return "--"
    }
    return "\(text) C"
  }

  func recoveryOxygenSaturationDisplayText(
    for date: Date = Date(),
    calendar: Calendar = .current
  ) -> String {
    guard !usesPreviewPacketData else {
      return "--"
    }
    guard let metric = preferredDailyRecoveryMetric(valueKey: "oxygen_saturation_percent", for: date, calendar: calendar),
          let text = Self.numberText(metric["oxygen_saturation_percent"], fractionDigits: 0) else {
      return "--"
    }
    return "\(text)%"
  }

  func recoveryHasAnyData() -> Bool {
    recoveryScoreValue() != nil
      || recoveryHRVDisplayText() != "--"
      || recoveryRestingHRDisplayText() != "--"
      || recoveryRespiratoryRateDisplayText() != "--"
      || recoveryWristTemperatureDisplayText() != "--"
  }

  func recoveryProvidedVitalsValue(_ key: String) -> Double? {
    guard !usesPreviewPacketData,
          let vitals = Self.map(packetScoreReports["recovery"], "provided_vitals"),
          Self.recoveryProvidedVitalsAreTrusted(vitals) else {
      return nil
    }
    return Self.doubleValue(vitals[key])
  }

  func strainFeatureScoreSummary() -> String {
    guard let report = packetScoreReports["strain"] else {
      return packetScoreStatus == "No run" ? "No run" : packetScoreStatus
    }
    let rawScore = Self.doubleValue(Self.map(report, "score_result", "output")?["score_0_to_21"])
    let score = rawScore.flatMap { Self.numberText(Self.strainPercent($0), fractionDigits: 0) } ?? "no score"
    return "\(Self.passStatus(report)) | \(score) strain"
  }

  func stressFeatureScoreSummary() -> String {
    if let report = packetScoreReports["stress"] {
      let score = Self.numberText(Self.map(report, "score_result", "output")?["score_0_to_100"], fractionDigits: 1) ?? "no score"
      return "\(Self.passStatus(report)) | \(score) stress"
    }
    let summary = stressAlgorithmSummary()
    guard let score = summary.score,
          let scoreText = Self.numberText(score, fractionDigits: 1) else {
      return summary.status
    }
    let confidence = Self.numberText(summary.confidence, fractionDigits: 2) ?? "0"
    return "local estimate | \(scoreText) stress | \(summary.sampleCount) HR samples | confidence \(confidence)"
  }

  func packetScoreProvenanceSummary(_ family: String) -> String {
    guard let report = packetScoreReports[family] else {
      return packetScoreStatus == "No run" ? "" : "family=\(family) | source=packet-derived bridge run"
    }
    let result = Self.map(report, "score_result")
    let algorithm = result?["algorithm_id"] as? String ?? "packet-derived-\(family)"
    return "family=\(family) | algorithm=\(algorithm) | issues=\(Self.array(report["issues"]).count)"
  }

  func packetDerivedScoreNextActionSummary() -> String {
    if let action = ["sleep", "recovery", "strain", "stress"].compactMap({ Self.firstActionText(in: packetScoreReports[$0]) }).first {
      return action
    }
    return packetScoreStatus == "No run" ? "Run scores to populate packet-derived outputs" : "Replace blocked score inputs with trusted captured packet feature reports"
  }

  func referenceComparisonSummary(_ family: String) -> String {
    referenceRunStatusByFamily[family] ?? "No comparison"
  }

  func calibrationLabelSummary() -> String {
    calibrationLabelsImported ? "1 label | manual" : "No labels"
  }

  func calibrationSummary() -> String {
    calibrationRunComplete ? "ready | 4 train / 2 holdout | improved" : "No run"
  }

  func calibratedScoreSummary() -> String {
    calibrationRunComplete ? "71.5 raw -> 74.2 / 100" : "No run"
  }

  func calibrationIssues() -> [String] {
    if !calibrationLabelsImported {
      return ["Import labels before calibration"]
    }
    if !calibrationRunComplete {
      return ["Run calibration to generate holdout evidence"]
    }
    return []
  }

  func calibrationNextActionSummary() -> String {
    if !calibrationLabelsImported {
      return "Import labels"
    }
    if !calibrationRunComplete {
      return "Calibrate"
    }
    return "Review calibrated \(calibrationTargetFamily) score"
  }

  func packetInputSource(_ detail: String) -> HealthDataSource {
    guard let key = Self.packetInputReportKey(for: detail) else {
      return packetInputReports.isEmpty
        ? .unavailable("\(detail) not extracted")
        : .unavailable("\(detail) is a packet-input action, not a metric source")
    }
    guard let report = packetInputReports[key] else {
      return .unavailable("\(detail) not extracted")
    }

    switch key {
    case "motion", "heart_rate", "resting_hr", "window":
      return Self.boolValue(report["pass"]) == true
        ? .bridgeDeviceSensor(detail)
        : .unavailable(firstPacketAction(in: report) ?? "\(detail) blocked")
    case "step_discovery":
      return Self.boolValue(report["explicit_step_counter_found"]) == true
        ? .bridgeDeviceCounter(detail)
        : .unavailable(firstPacketAction(in: report) ?? "WHOOP step counter not decoded")
    case "hrv":
      return Self.boolValue(report["pass"]) == true
        ? .bridgeDeviceSensor(detail)
        : .unavailable(firstPacketAction(in: report) ?? "HRV packet evidence unverified")
    case "vital_event":
      return Self.boolValue(report["pass"]) == true
        ? .bridgeDeviceSensor(detail)
        : .unavailable(firstPacketAction(in: report) ?? "vital packet semantics unverified")
    case "recovery_sensor_rollup":
      return Self.boolValue(report["pass"]) == true
        ? .bridgeDeviceSensor(detail)
        : .unavailable(firstPacketAction(in: report) ?? "recovery sensor rollup blocked")
    case "recovery_unavailable_status":
      return .unavailable(firstPacketAction(in: report) ?? "recovery vitals remain unavailable")
    case "activity_unavailable_status":
      return .unavailable(firstPacketAction(in: report) ?? "activity steps remain unavailable")
    case "energy_unavailable_status":
      return .unavailable(firstPacketAction(in: report) ?? "activity calories remain unavailable")
    case "readiness":
      return .bridge(detail)
    default:
      return Self.boolValue(report["pass"]) == true
        ? .bridge(detail)
        : .unavailable(firstPacketAction(in: report) ?? "\(detail) blocked")
    }
  }

  static func packetInputReportKey(for detail: String) -> String? {
    let normalized = detail.lowercased()
    if normalized.contains("input_readiness") || normalized.contains("readiness") {
      return "readiness"
    }
    if normalized.contains("motion_features") || normalized == "motion" {
      return "motion"
    }
    if normalized.contains("heart_rate_features") || normalized.contains("heart rate feature") {
      return "heart_rate"
    }
    if normalized.contains("step_packet_discovery") || normalized.contains("step discovery") {
      return "step_discovery"
    }
    if normalized.contains("activity_unavailable")
      || normalized.contains("steps unavailable")
      || normalized.contains("unavailable steps")
      || normalized.contains("unavailable activity") {
      return "activity_unavailable_status"
    }
    if normalized.contains("energy_unavailable")
      || normalized.contains("calories_unavailable")
      || normalized.contains("calorie_unavailable")
      || normalized.contains("energy unavailable")
      || normalized.contains("unavailable energy")
      || normalized.contains("calories unavailable")
      || normalized.contains("unavailable calories") {
      return "energy_unavailable_status"
    }
    if normalized.contains("hrv_features") || normalized.contains("hrv feature") {
      return "hrv"
    }
    if normalized.contains("resting_hr_features") || normalized.contains("resting hr") {
      return "resting_hr"
    }
    if normalized.contains("window_features") || normalized == "window" {
      return "window"
    }
    if normalized.contains("vital_event_features") || normalized.contains("vitals") {
      return "vital_event"
    }
    if normalized.contains("recovery_unavailable") || normalized.contains("unavailable recovery") {
      return "recovery_unavailable_status"
    }
    if normalized.contains("recovery_sensor_daily_rollup")
      || normalized.contains("recovery sensor rollup")
      || normalized.contains("recovery-sensor-rollup") {
      return "recovery_sensor_rollup"
    }
    return nil
  }

  func packetScoreSource(_ detail: String) -> HealthDataSource {
    packetScoreReports.isEmpty ? .unavailable("\(detail) not computed") : .bridge(detail)
  }

  func referenceComparisonSource(_ family: String) -> HealthDataSource {
    referenceComparisonReports[family] == nil ? .unavailable("real \(family) reference comparison inputs not wired") : .bridge("metrics.reference_compare")
  }

  func primarySleep() -> PrimarySleepDetail? {
    primarySleepDetail
  }

  func sleepTimelineEmptyActionSummary() -> String {
    "Manual sleep entry is not part of this MVP. Sleep rows come from trusted band sleep evidence; platform sleep imports stay reference-only."
  }
}
