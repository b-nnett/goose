import CoreLocation
import MapKit
import SwiftUI
import UIKit

final class ActivitySessionModel: ObservableObject {
  @Published private(set) var selectedActivity: ActivityKind = .run
  @Published private(set) var isActive = false
  @Published private(set) var isPaused = false
  @Published private(set) var startedAt: Date?
  @Published private(set) var endedAt: Date?
  @Published private(set) var elapsed: TimeInterval = 0
  @Published private(set) var averageHeartRate: Int?
  @Published private(set) var maxHeartRate: Int?
  @Published private(set) var zoneDurations: [Int: TimeInterval] = [:]

  // Private backing stores updated at full 60 Hz sample rate.
  // @Published properties are only flushed at uiPublishInterval to avoid
  // driving a SwiftUI re-render on every sample tick.
  private var _elapsed: TimeInterval = 0
  private var _averageHeartRate: Int?
  private var _maxHeartRate: Int?
  private var _zoneDurations: [Int: TimeInterval] = [:]
  private var lastPublishedAt: Date = .distantPast
  private static let uiPublishInterval: TimeInterval = 1.0 / 4.0

  private var lastTick: Date?
  private var heartRateWeightedTotal: Double = 0
  private var heartRateMeasuredSeconds: TimeInterval = 0
  private var timer: Timer?
  private var heartRateProvider: (() -> Int?)?

  deinit {
    timer?.invalidate()
  }

  var statusText: String {
    if isActive && isPaused {
      return "Paused"
    }
    if isActive {
      return "Recording"
    }
    if endedAt != nil {
      return "Ended"
    }
    return "Ready"
  }

  func select(_ activity: ActivityKind) {
    guard !isActive else {
      return
    }
    selectedActivity = activity
    resetMetrics(keepingSelection: true)
  }

  func start(now: Date = Date(), heartRateProvider: @escaping () -> Int?) {
    resetMetrics(keepingSelection: true)
    self.heartRateProvider = heartRateProvider
    isActive = true
    isPaused = false
    startedAt = now
    endedAt = nil
    lastTick = now
    scheduleTimer()
  }

  func resume(now: Date = Date(), heartRateProvider: @escaping () -> Int?) {
    guard isActive, isPaused else {
      return
    }
    self.heartRateProvider = heartRateProvider
    isPaused = false
    lastTick = now
    scheduleTimer()
  }

  func pause(now: Date = Date(), heartRate: Int?) {
    guard isActive, !isPaused else {
      return
    }
    tick(now: now, heartRate: heartRate)
    flushToUI(now: now)  // ensure latest samples are visible before pausing
    isPaused = true
    lastTick = nil
    timer?.invalidate()
    timer = nil
  }

  func end(now: Date = Date(), heartRate: Int?) {
    guard isActive else {
      return
    }
    tick(now: now, heartRate: heartRate)
    flushToUI(now: now)  // flush final sample before clearing state
    isActive = false
    isPaused = false
    endedAt = now
    lastTick = nil
    timer?.invalidate()
    timer = nil
    heartRateProvider = nil
  }

  func tick(now: Date, heartRate: Int?) {
    guard isActive, !isPaused else {
      return
    }
    let previousTick = lastTick ?? now
    let delta = max(0, now.timeIntervalSince(previousTick))
    _elapsed += delta
    lastTick = now

    if delta > 0, let heartRate {
      let zoneID = HeartRateZone.zoneID(for: heartRate)
      _zoneDurations[zoneID, default: 0] += delta
      heartRateWeightedTotal += Double(heartRate) * delta
      heartRateMeasuredSeconds += delta
      _averageHeartRate = Int((heartRateWeightedTotal / max(heartRateMeasuredSeconds, 1)).rounded())
      _maxHeartRate = max(_maxHeartRate ?? heartRate, heartRate)
    }

    if now.timeIntervalSince(lastPublishedAt) >= Self.uiPublishInterval {
      flushToUI(now: now)
    }
  }

  // Copies backing-store values into the @Published properties in a single
  // synchronous block so SwiftUI coalesces them into one re-render.
  private func flushToUI(now: Date) {
    lastPublishedAt = now
    elapsed = _elapsed
    averageHeartRate = _averageHeartRate
    maxHeartRate = _maxHeartRate
    zoneDurations = _zoneDurations
  }

  private func scheduleTimer() {
    timer?.invalidate()
    let newTimer = Timer(timeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
      guard let self else {
        return
      }
      self.tick(now: Date(), heartRate: self.heartRateProvider?())
    }
    newTimer.tolerance = 0.002
    RunLoop.main.add(newTimer, forMode: .common)
    timer = newTimer
  }

  private func resetMetrics(keepingSelection: Bool) {
    timer?.invalidate()
    timer = nil
    if !keepingSelection {
      selectedActivity = .run
    }
    _elapsed = 0
    _averageHeartRate = nil
    _maxHeartRate = nil
    _zoneDurations = [:]
    heartRateWeightedTotal = 0
    heartRateMeasuredSeconds = 0
    lastTick = nil
    lastPublishedAt = .distantPast
    elapsed = 0
    averageHeartRate = nil
    maxHeartRate = nil
    zoneDurations = [:]
    startedAt = nil
    endedAt = nil
    isActive = false
    isPaused = false
    heartRateProvider = nil
  }
}

