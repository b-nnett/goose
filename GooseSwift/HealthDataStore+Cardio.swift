import Darwin
import Foundation
import SwiftUI
import UIKit

extension HealthDataStore {
  func cardioLoadWeeklyPoints() -> [CardioLoadDay] {
    cardioLoadPoints(range: "7D")
  }

  func cardioLoadPoints(range: String) -> [CardioLoadDay] {
    cardioLoadAlgorithmSummary(range: range).points
  }

  func cardioLoadAlgorithmSummary(
    range: String = "30D",
    calendar: Calendar = .current
  ) -> CardioLoadAlgorithmSummary {
    _ = range
    _ = calendar
    return emptyCardioLoadSummary(
      status: previewMissingData ? "No data" : "Needs activity",
      freshness: previewMissingData ? "Missing" : "No local data",
      source: .unavailable(
        previewMissingData
          ? "preview missing cardio load data"
          : "cardio load needs local Goose activity sessions and daily activity metrics"
      )
    )
  }

  func cardioStatusRows() -> [HealthSummaryRow] {
    let points = cardioLoadWeeklyPoints()
    guard !points.isEmpty else {
      return [
        HealthSummaryRow("Calibrating", value: "No weekly HR + activity data yet", source: .unavailable("cardio inputs pending"), systemImage: "heart.circle")
      ]
    }
    let grouped = Dictionary(grouping: points, by: \.status)
    return ["Calibrating", "Detraining", "Maintaining", "Peaking", "Productive", "Fatigued", "Overtraining"].map { status in
      let days = grouped[status]?.count ?? 0
      let percent = Double(days) / Double(points.count)
      return HealthSummaryRow(
        status,
        value: days == 0 ? "0d | supported status state" : "\(days)d | \(Self.percentText(percent) ?? "0%") of visible week",
        source: .local("goose.cardio_load.local_v1 status bands"),
        systemImage: "heart.circle"
      )
    }
  }

  func emptyCardioLoadSummary(
    status: String,
    freshness: String,
    source: HealthDataSource
  ) -> CardioLoadAlgorithmSummary {
    CardioLoadAlgorithmSummary(
      points: [],
      status: status,
      freshness: freshness,
      source: source,
      sessionCount: 0,
      activityDayCount: 0,
      hasBaseline: false
    )
  }

  func cardioLoadSnapshot(base snapshot: HealthMetricSnapshot) -> HealthMetricSnapshot {
    let summary = cardioLoadAlgorithmSummary(range: "30D")
    guard let latest = summary.latestPoint else {
      return replacingHealthMonitorSnapshot(
        snapshot,
        value: "--",
        unit: "load",
        status: summary.status,
        freshness: summary.freshness,
        provenance: summary.source.detail,
        source: summary.source,
        trend: Self.cardioLoadTrendModel(base: snapshot.trend, summary: summary)
      )
    }

    return replacingHealthMonitorSnapshot(
      snapshot,
      value: Self.numberText(latest.load, fractionDigits: 0) ?? "0",
      unit: "load",
      status: summary.hasBaseline ? latest.status : "Calibrating",
      freshness: summary.freshness,
      provenance: summary.source.detail,
      source: summary.source,
      trend: Self.cardioLoadTrendModel(base: snapshot.trend, summary: summary)
    )
  }

  func cardioLoadActivitySessions(from start: Date, to end: Date) async -> [[String: Any]] {
    let path = databasePath
    let startMs = Self.unixMilliseconds(start)
    let endMs = Self.unixMilliseconds(end)
    do {
      let report = try await bridge.requestAsync(
        method: "activity.list_sessions",
        args: [
          "database_path": path,
          "start_time_unix_ms": startMs,
          "end_time_unix_ms": endMs,
        ]
      )
      return report["sessions"] as? [[String: Any]] ?? []
    } catch {
      return []
    }
  }

  func cardioLoadActivityMetricsByName(sessionID: String?) async -> [String: [String: Any]] {
    guard let sessionID else {
      return [:]
    }
    let path = databasePath
    do {
      let report = try await bridge.requestAsync(
        method: "activity.list_metrics",
        args: [
          "database_path": path,
          "activity_session_id": sessionID,
        ]
      )
      let metrics = report["metrics"] as? [[String: Any]] ?? []
      return Dictionary(
        uniqueKeysWithValues: metrics.compactMap { metric in
          guard let name = metric["metric_name"] as? String else {
            return nil
          }
          return (name, metric)
        }
      )
    } catch {
      return [:]
    }
  }

  func cardioLoadContribution(
    from session: [String: Any],
    metrics: [String: [String: Any]],
    observedMaxHeartRate: Double?,
    calendar: Calendar
  ) -> CardioLoadSessionContribution? {
    guard let sessionID = session["session_id"] as? String,
          let startMs = Self.int64Value(session["start_time_unix_ms"]),
          let endMs = Self.int64Value(session["end_time_unix_ms"]),
          endMs > startMs else {
      return nil
    }

    let start = Date(timeIntervalSince1970: Double(startMs) / 1000)
    let end = Date(timeIntervalSince1970: Double(endMs) / 1000)
    let storedDurationSeconds = Self.doubleValue(metrics["duration"]?["value"])
      ?? Self.doubleValue(session["duration_ms"]).map { $0 / 1000 }
      ?? end.timeIntervalSince(start)
    let durationMinutes = max(storedDurationSeconds / 60, 0)
    guard durationMinutes >= 1 else {
      return nil
    }

    let zoneLoad = (1...5).reduce(0.0) { partial, zoneID in
      let seconds = Self.doubleValue(metrics["hr_zone_\(zoneID)_duration"]?["value"]) ?? 0
      return partial + max(seconds, 0) / 60.0 * Double(zoneID)
    }
    let load: Double
    if zoneLoad > 0.25 {
      load = zoneLoad
    } else {
      let sessionSamples = heartRateSeriesStore.decimatedSamples(from: start, to: end)
      let averageHeartRate = Self.doubleValue(metrics["average_hr"]?["value"])
        ?? Self.averageHeartRate(in: sessionSamples)
      let sessionMaxHeartRate = Self.doubleValue(metrics["max_hr"]?["value"])
        ?? sessionSamples.map(\.bpm).max().map(Double.init)
      let restingHeartRate = heartRateSeriesStore.restingEstimate(forDayContaining: start, calendar: calendar)?.bpm
        ?? Self.liveHRDerivedRestingHeartRateSample()?.bpm
      guard let averageHeartRate,
            let restingHeartRate,
            let maxHeartRate = [sessionMaxHeartRate, observedMaxHeartRate].compactMap({ $0 }).max(),
            maxHeartRate >= restingHeartRate + 25 else {
        return nil
      }
      let reserveFraction = Self.clamp(
        (averageHeartRate - restingHeartRate) / max(maxHeartRate - restingHeartRate, 1),
        min: 0,
        max: 1
      )
      guard reserveFraction > 0.05 else {
        return nil
      }
      load = durationMinutes * 0.64 * exp(1.92 * reserveFraction)
    }

    guard load.isFinite, load > 0 else {
      return nil
    }
    return CardioLoadSessionContribution(
      sessionID: sessionID,
      start: start,
      end: end,
      dayStart: calendar.startOfDay(for: start),
      load: load,
      durationMinutes: durationMinutes
    )
  }

  func cardioLoadDailyComputations(
    contributions: [CardioLoadSessionContribution],
    dayStarts: [Date]
  ) -> [CardioLoadDailyComputation] {
    let grouped = Dictionary(grouping: contributions, by: \.dayStart)
    let dailyLoads = dayStarts.map { day -> Double in
      grouped[day, default: []].reduce(0) { $0 + $1.load }
    }
    let dailyDurations = dayStarts.map { day -> Double in
      grouped[day, default: []].reduce(0) { $0 + $1.durationMinutes }
    }

    return dayStarts.enumerated().map { index, day in
      let activityDaysSoFar = dailyLoads.prefix(index + 1).filter { $0 > 0 }.count
      let acuteStart = max(0, index - 6)
      let chronicStart = max(0, index - 27)
      let acuteValues = dailyLoads[acuteStart...index]
      let chronicValues = dailyLoads[chronicStart...index]
      let acute = acuteValues.reduce(0, +) / Double(max(acuteValues.count, 1))
      let chronic = chronicValues.reduce(0, +) / Double(max(chronicValues.count, 1))
      return CardioLoadDailyComputation(
        dayStart: day,
        load: dailyLoads[index],
        durationMinutes: dailyDurations[index],
        status: Self.cardioLoadTrainingStatus(
          acute: acute,
          chronic: chronic,
          activityDayCount: activityDaysSoFar
        )
      )
    }
  }

  func energyStressChartPoints() -> [EnergyStressPoint] {
    guard !previewMissingData else {
      return []
    }
    let summary = energyBankAlgorithmSummary()
    return summary.hasData ? summary.points : []
  }

  func energyStressSelectedPoint() -> EnergyStressPoint? {
    energyStressChartPoints().first { $0.id == "2130" } ?? energyStressChartPoints().last
  }

  func healthMonitorExportRows() -> [HealthSummaryRow] {
    guard localDataSupportsExport else {
      return []
    }
    return [
      HealthSummaryRow("Local health export", value: "Packet reports and reference comparisons available", source: .bridge("local cached bridge reports"), systemImage: "square.and.arrow.up")
    ]
  }

  #if DEBUG
  func applyPreviewState(_ state: HealthPreviewState) {
    attemptedCatalogLoad = true
    switch state {
    case .populated:
      previewMissingData = false
      primarySleepDetail = nil
      packetInputStatus = "No run"
      packetScoreStatus = "No run"
      externalSleepImportStatus = "External sleep imports disabled"
      packetInputReports = [:]
      packetScoreReports = [:]
      referenceComparisonReports = [:]
      referenceRunStatusByFamily = [:]
      calibrationLabelsImported = false
      calibrationRunComplete = false
    case .missing:
      previewMissingData = true
      primarySleepDetail = nil
      packetInputStatus = "No run"
      packetScoreStatus = "No run"
      externalSleepImportStatus = "External sleep imports disabled"
      packetInputReports = [:]
      packetScoreReports = [:]
      referenceComparisonReports = [:]
      referenceRunStatusByFamily = [:]
      algorithmDefinitions = []
      referenceDefinitions = []
      selectedAlgorithmByFamily = [:]
      catalogStatus = "Preview missing catalog"
      catalogSource = .unavailable("preview missing catalog")
      calibrationLabelsImported = false
      calibrationRunComplete = false
    }
  }
  #endif
}
