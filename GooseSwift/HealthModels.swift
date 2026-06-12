import Darwin
import Foundation
import SwiftUI
import UIKit

enum HealthRoute: String, CaseIterable, Identifiable, Hashable {
  case healthMonitor
  case sleep
  case recovery
  case strain
  case stress
  case trends
  case cardioLoad
  case energyBank
  case packetInputs
  case algorithms
  case referenceComparisons
  case calibration

  var id: String { rawValue }

  var title: String {
    switch self {
    case .healthMonitor: "Health Monitor"
    case .sleep: "Sleep"
    case .recovery: "Recovery"
    case .strain: "Strain"
    case .stress: "Stress"
    case .trends: "Trends"
    case .cardioLoad: "Cardio Load"
    case .energyBank: "Energy Bank"
    case .packetInputs: "Packet Inputs"
    case .algorithms: "Algorithms"
    case .referenceComparisons: "Reference Comparisons"
    case .calibration: "Calibration"
    }
  }

  var systemImage: String {
    switch self {
    case .healthMonitor: "heart.text.square"
    case .sleep: "bed.double"
    case .recovery: "battery.100percent"
    case .strain: "figure.run"
    case .stress: "waveform.path.ecg"
    case .trends: "chart.line.uptrend.xyaxis"
    case .cardioLoad: "heart.circle"
    case .energyBank: "bolt.circle"
    case .packetInputs: "square.stack.3d.up"
    case .algorithms: "function"
    case .referenceComparisons: "scalemass"
    case .calibration: "slider.horizontal.3"
    }
  }

  var deepLinkPath: String {
    "gooseswift://health/\(rawValue)"
  }
}

struct HealthMetricSnapshot: Identifiable {
  let id: String
  let route: HealthRoute
  let group: HealthMetricGroup
  let title: String
  let value: String
  let unit: String
  let status: String
  let freshness: String
  let provenance: String
  let source: HealthDataSource
  let systemImage: String
  let tint: Color
  let trend: HealthTrendModel

  var displayValue: String {
    guard !unit.isEmpty else {
      return value
    }
    if unit == "%" {
      let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmedValue.isEmpty, trimmedValue != "--" else {
        return trimmedValue
      }
      return trimmedValue.hasSuffix("%") ? trimmedValue : "\(trimmedValue)%"
    }
    return "\(value) \(unit)"
  }
}

extension HealthRoute {
  var supportsScoreDatePicker: Bool {
    switch self {
    case .sleep, .recovery, .strain:
      true
    default:
      false
    }
  }
}

struct ScoreDateMetric: Identifiable {
  let route: HealthRoute
  let score: Int
  let tint: Color

  var id: HealthRoute { route }
}

struct ScoreDateEntry: Identifiable {
  let date: Date
  let metrics: [ScoreDateMetric]
  let isFuture: Bool

  var id: Date { date }
}

enum ScoreDateTimeline {
  static func datedSnapshot(
    from snapshot: HealthMetricSnapshot,
    date: Date,
    calendar: Calendar = .current
  ) -> HealthMetricSnapshot {
    guard snapshot.route.supportsScoreDatePicker else {
      return snapshot
    }

    let today = calendar.startOfDay(for: Date())
    let selectedDay = calendar.startOfDay(for: date)
    if snapshot.route == .recovery, snapshot.source.kind == .unavailable {
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: "--",
        unit: "%",
        status: "No data",
        freshness: dateLabel(for: selectedDay, calendar: calendar),
        provenance: snapshot.provenance,
        source: snapshot.source,
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: snapshot.trend
      )
    }
    guard calendar.isDate(selectedDay, inSameDayAs: today) else {
      return HealthMetricSnapshot(
        id: snapshot.id,
        route: snapshot.route,
        group: snapshot.group,
        title: snapshot.title,
        value: "--",
        unit: snapshot.unit,
        status: "No data",
        freshness: dateLabel(for: selectedDay, calendar: calendar),
        provenance: "No stored history for selected date",
        source: .unavailable("selected date history not available"),
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        trend: HealthTrendModel(
          id: snapshot.trend.id,
          title: snapshot.trend.title,
          rangeLabel: "No data",
          summary: "No stored history",
          analysis: "No stored metric history exists for this selected date yet.",
          resources: snapshot.trend.resources,
          points: []
        )
      )
    }

    let score = baseScorePercent(for: snapshot)

    return HealthMetricSnapshot(
      id: snapshot.id,
      route: snapshot.route,
      group: snapshot.group,
      title: snapshot.title,
      value: "\(score)",
      unit: "%",
      status: status(for: snapshot.route, score: score),
      freshness: dateLabel(for: selectedDay, calendar: calendar),
      provenance: snapshot.provenance,
      source: snapshot.source,
      systemImage: snapshot.systemImage,
      tint: snapshot.tint,
      trend: snapshot.trend
    )
  }

  static func entry(
    for date: Date,
    routes: [HealthRoute],
    snapshots: [HealthMetricSnapshot],
    calendar: Calendar = .current
  ) -> ScoreDateEntry {
    let selectedDay = calendar.startOfDay(for: date)
    let today = calendar.startOfDay(for: Date())
    let metrics = routes.compactMap { route -> ScoreDateMetric? in
      guard let snapshot = snapshots.first(where: { $0.route == route }) else {
        return nil
      }
      if route == .recovery, snapshot.source.kind == .unavailable {
        return ScoreDateMetric(route: route, score: 0, tint: snapshot.tint)
      }
      let score = calendar.isDate(selectedDay, inSameDayAs: today) ? baseScorePercent(for: snapshot) : 0
      return ScoreDateMetric(route: route, score: score, tint: snapshot.tint)
    }

    return ScoreDateEntry(date: selectedDay, metrics: metrics, isFuture: selectedDay > today)
  }

  static func dateLabel(for date: Date, calendar: Calendar = .current) -> String {
    if calendar.isDateInToday(date) {
      return "Today"
    }
    if calendar.isDateInYesterday(date) {
      return "Yesterday"
    }
    return date.formatted(.dateTime.month(.abbreviated).day())
  }

  private static func baseScorePercent(for snapshot: HealthMetricSnapshot) -> Int {
    let rawValue = firstNumber(in: snapshot.displayValue) ?? firstNumber(in: snapshot.value) ?? 0
    if snapshot.route == .strain, snapshot.unit == "/21" {
      return min(max(Int((rawValue / 21 * 100).rounded()), 0), 100)
    }
    return min(max(Int(rawValue.rounded()), 0), 100)
  }

  private static func status(for route: HealthRoute, score: Int) -> String {
    switch route {
    case .sleep:
      score >= 85 ? "High sleep performance" : score >= 70 ? "Moderate sleep performance" : "Low sleep performance"
    case .recovery:
      score >= 67 ? "Recovered" : score >= 34 ? "Moderate recovery" : "Low recovery"
    case .strain:
      score >= 70 ? "High strain" : score >= 40 ? "Moderate strain" : "Low strain"
    default:
      score >= 70 ? "On track" : "Review"
    }
  }

  private static func firstNumber(in text: String) -> Double? {
    let pattern = #"[-+]?\d*\.?\d+"#
    guard let range = text.range(of: pattern, options: .regularExpression) else {
      return nil
    }
    return Double(text[range])
  }
}
