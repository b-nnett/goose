import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func refreshPrimarySleepFromScoreReport() {
    guard let detail = Self.primarySleepDetail(fromSleepReport: packetScoreReports["sleep"]) else {
      return
    }
    primarySleepDetail = detail
  }

  static func primarySleepDetail(fromSleepReport report: [String: Any]?) -> PrimarySleepDetail? {
    guard let report,
          let output = map(report, "score_result", "output") else {
      return nil
    }
    let window = map(report, "sleep_window")
    let input = map(report, "sleep_v1_input") ?? map(report, "sleep_input")
    let start = bridgeDate(input?["start_time"] ?? window?["start_time"])
    let end = bridgeDate(input?["end_time"] ?? window?["end_time"])
    let duration = doubleValue(output["sleep_duration_minutes"])
      ?? doubleValue(window?["sleep_duration_minutes"])
      ?? doubleValue(input?["sleep_duration_minutes"])
      ?? 0
    let timeInBed = doubleValue(output["time_in_bed_minutes"])
      ?? doubleValue(window?["time_in_bed_minutes"])
      ?? doubleValue(input?["time_in_bed_minutes"])
      ?? duration
    let score = numberText(output["score_0_to_100"], fractionDigits: 0) ?? "--"
    let stages = sleepStageSegments(from: output)
    let idSuffix = start.map { "\(Int($0.timeIntervalSince1970))" } ?? "latest"

    return PrimarySleepDetail(
      id: "primary-sleep-\(idSuffix)",
      dateLabel: start.map(dateLabel) ?? "Latest",
      startLabel: start.map(timeLabel) ?? "--",
      endLabel: end.map(timeLabel) ?? "--",
      durationText: minutesText(duration),
      timeInBedText: minutesText(timeInBed),
      scoreText: score,
      qualityText: sleepQualityLabel(score: doubleValue(output["score_0_to_100"])),
      source: .bridge("metrics.sleep_score_from_features"),
      stages: stages
    )
  }

  static func sleepStageSegments(from output: [String: Any]) -> [HealthSleepStageSegment] {
    let stageRows = array(output["stage_segments"])
    if !stageRows.isEmpty {
      return stageRows.enumerated().compactMap { index, row in
        let stage = row["stage_kind"] as? String ?? row["stage"] as? String ?? "core"
        let duration = doubleValue(row["duration_minutes"]) ?? 0
        guard duration > 0 else {
          return nil
        }
        let start = bridgeDate(row["start_time"])
        let end = bridgeDate(row["end_time"])
        return HealthSleepStageSegment(
          id: "bridge-stage-\(index)-\(stage)",
          stage: stage,
          startLabel: start.map(timeLabel) ?? "--",
          endLabel: end.map(timeLabel) ?? "--",
          durationMinutes: duration,
          confidence: doubleValue(row["confidence_0_to_1"]),
          source: .bridge("sleep_v1 output stage_segments")
        )
      }
    }

    guard let minutesByStage = output["stage_minutes"] as? [String: Any] else {
      return []
    }
    return ["awake", "rem", "core", "deep"].compactMap { stage in
      guard let minutes = doubleValue(minutesByStage[stage]),
            minutes > 0 else {
        return nil
      }
      return HealthSleepStageSegment(
        id: "bridge-stage-total-\(stage)",
        stage: stage,
        startLabel: "--",
        endLabel: "--",
        durationMinutes: minutes,
        confidence: doubleValue(output["stage_segment_confidence_0_to_1"]),
        source: .bridge("sleep_v1 output stage_minutes")
      )
    }
  }

  static func sleepQualityLabel(score: Double?) -> String {
    guard let score else {
      return "No score"
    }
    if score >= 85 {
      return "Optimal"
    }
    if score >= 70 {
      return "Good"
    }
    if score >= 50 {
      return "Needs attention"
    }
    return "Low"
  }

  static func recoveryQualityLabel(score: Double?) -> String {
    guard let score else {
      return "No data"
    }
    if score >= 67 {
      return "Recovered"
    }
    if score >= 34 {
      return "Moderate recovery"
    }
    if score > 0 {
      return "Low recovery"
    }
    return "No data"
  }

  static func strainStatusLabel(score: Double?) -> String {
    guard let score, score > 0 else {
      return "No strain data"
    }
    if score >= 70 {
      return "High strain"
    }
    if score >= 40 {
      return "Moderate strain"
    }
    return "Low strain"
  }

  static func strainPercent(_ rawScore0To21: Double) -> Double {
    min(max(rawScore0To21 / 21.0 * 100.0, 0), 100)
  }

  static func stressStatusLabel(score: Double?) -> String {
    guard let score else {
      return "No data"
    }
    if score >= 66 {
      return "High"
    }
    if score >= 33 {
      return "Medium"
    }
    return "Low"
  }

  @MainActor
  func importSleepFromHealthKit() async {
    externalSleepImportStatus = "Importing from Apple Health…"
    let result = await HealthKitSleepImporter.importMostRecentSleep()
    switch result {
    case .success(let detail):
      if primarySleepDetail == nil {
        primarySleepDetail = detail
      }
      externalSleepImportStatus = "Imported: \(detail.durationText) on \(detail.dateLabel)"
    case .noData(let reason):
      externalSleepImportStatus = "No data: \(reason)"
    case .denied(let reason):
      externalSleepImportStatus = "Access denied: \(reason)"
    case .unavailable:
      externalSleepImportStatus = "HealthKit unavailable on this device"
    }
  }

  @MainActor
  func importAllFromHealthKit() async {
    hkImportStatus = "Importing from Apple Health…"
    let result = await HealthKitFullImporter.importAll()

    // Sleep — only fill if band hasn't provided it
    if let detail = result.sleepDetail, primarySleepDetail == nil {
      primarySleepDetail = detail
      externalSleepImportStatus = "Imported: \(detail.durationText) on \(detail.dateLabel)"
    }

    // Vitals
    if let v = result.restingHR { hkRestingHR = v }
    if let v = result.hrvRmssdMs { hkHRVRmssdMs = v }
    if !result.hrvHistory.isEmpty { hkHRVHistory = result.hrvHistory }
    if !result.rhrHistory.isEmpty { hkRHRHistory = result.rhrHistory }
    if let v = result.respiratoryRate { hkRespiratoryRate = v }
    if let v = result.spO2Percent { hkSpO2Percent = v }
    if let v = result.skinTempDeltaC { hkSkinTempDeltaC = v }
    if let v = result.steps { hkSteps = v }
    if let v = result.activeKcal { hkActiveKcal = v }

    // Heart rate samples into the series store (feeds stress + HRV timeline)
    let pipeline = HeartRateSamplePipeline()
    for sample in result.hrSamples {
      pipeline.recordHeartRateSample(bpm: sample.bpm, source: "apple.health", capturedAt: sample.date)
    }

    // Workouts
    if !result.workouts.isEmpty {
      hkWorkouts = result.workouts
    }

    let imported = buildImportSummary(result)
    hkImportStatus = imported
  }

  private func buildImportSummary(_ r: HealthKitFullImportResult) -> String {
    var parts: [String] = []
    if r.sleepDetail != nil { parts.append("sleep") }
    if r.restingHR != nil { parts.append("resting HR") }
    if r.hrvRmssdMs != nil { parts.append("HRV") }
    if r.respiratoryRate != nil { parts.append("resp rate") }
    if r.spO2Percent != nil { parts.append("SpO2") }
    if r.skinTempDeltaC != nil { parts.append("skin temp") }
    if r.steps != nil { parts.append("steps") }
    if r.activeKcal != nil { parts.append("calories") }
    if !r.hrSamples.isEmpty { parts.append("\(r.hrSamples.count) HR samples") }
    if !r.workouts.isEmpty { parts.append("\(r.workouts.count) workouts") }
    if parts.isEmpty { return "No data found" }
    return "Imported: \(parts.joined(separator: ", "))"
  }

}
