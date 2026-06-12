import Foundation


actor GooseStrainAccumulator {
  private var accumulatedLoad: Double = 0
  private var lastSampleDate: Date?
  private var lastPublishedAt: Date = .distantPast
  private var isFrozen: Bool = false
  private var maxHR: Double = GooseStrainAccumulator.defaultMaxHR

  nonisolated static let defaultMaxHR: Double = 190
  nonisolated static let publishInterval: TimeInterval = 3
  nonisolated static let maxSampleGap: TimeInterval = 30

  func ingest(bpm: Int, date: Date) {
    guard !isFrozen else { return }
    guard let last = lastSampleDate else {
      lastSampleDate = date
      return
    }
    let interval = date.timeIntervalSince(last)
    guard interval > 0, interval < GooseStrainAccumulator.maxSampleGap else {
      lastSampleDate = date
      return
    }
    lastSampleDate = date
    let hrPct = Double(bpm) / maxHR
    let multiplier: Double
    switch hrPct {
    case ..<0.60: multiplier = 1.0
    case 0.60..<0.70: multiplier = 2.0
    case 0.70..<0.80: multiplier = 3.0
    case 0.80..<0.90: multiplier = 4.0
    default: multiplier = 5.0
    }
    accumulatedLoad += multiplier * interval
  }

  func pollIfReady(now: Date) -> Double? {
    guard now.timeIntervalSince(lastPublishedAt) >= GooseStrainAccumulator.publishInterval else {
      return nil
    }
    lastPublishedAt = now
    return accumulatedLoad
  }

  func reset() {
    accumulatedLoad = 0
    lastSampleDate = nil
    lastPublishedAt = .distantPast
    isFrozen = false
  }

  func freeze() {
    isFrozen = true
  }

  func setMaxHR(_ bpm: Double) {
    maxHR = bpm
  }
}
