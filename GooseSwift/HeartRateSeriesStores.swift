import Foundation
import UIKit

struct HeartRateSamplePoint: Codable, Identifiable, Equatable {
  let id: String
  let capturedAt: Date
  let bpm: Int
  let source: String

  init(bpm: Int, source: String, capturedAt: Date) {
    let milliseconds = Int64((capturedAt.timeIntervalSince1970 * 1000).rounded())
    self.id = "\(milliseconds).\(bpm).\(source)"
    self.capturedAt = capturedAt
    self.bpm = bpm
    self.source = source
  }
}

struct HeartRateHourlyRange: Identifiable, Equatable {
  let id: String
  let hourStart: Date
  let minBPM: Int
  let maxBPM: Int
  let averageBPM: Int
  let sampleCount: Int
}

struct HeartRateTimelineSnapshot: Equatable {
  let ranges: [HeartRateHourlyRange]
  let status: String
  let generatedAt: Date
}

struct HeartRateHourlyBucket {
  var minBPM = Int.max
  var maxBPM = Int.min
  var totalBPM = 0
  var sampleCount = 0

  mutating func append(_ bpm: Int) {
    minBPM = min(minBPM, bpm)
    maxBPM = max(maxBPM, bpm)
    totalBPM += bpm
    sampleCount += 1
  }
}

struct HeartRateRestingEstimate: Equatable {
  let bpm: Double
  let sampleCount: Int
  let updatedAt: Date?
  let source: String
}

struct HeartRateSeriesFile: Codable {
  let version: Int
  let samples: [HeartRateSamplePoint]
}

final class HeartRateSeriesStore: @unchecked Sendable {
  static let shared = HeartRateSeriesStore()
  static let didUpdateNotification = Notification.Name("GooseHeartRateSeriesStoreDidUpdate")

  private static let retention: TimeInterval = 7 * 24 * 60 * 60
  private static let maxSamples = 100_000
  private static let persistDelay: TimeInterval = 1.0
  private static let updateNotificationInterval: TimeInterval = 2.0

  private let url: URL
  private let stateLock = NSLock()
  private let writeQueue = DispatchQueue(label: "com.goose.swift.heart-rate-series", qos: .utility)
  private var samples: [HeartRateSamplePoint]
  private var pendingWrite: DispatchWorkItem?
  private var lastNotificationAt = Date.distantPast

  init(url: URL = HeartRateSeriesStore.defaultURL()) {
    self.url = url
    self.samples = Self.loadSamples(from: url)
    prune(relativeTo: Date())
  }

  func append(bpm: Int, source: String, capturedAt: Date) -> Bool {
    guard GooseHRSanitizer.validRange.contains(bpm) else {
      return false
    }
    stateLock.lock()
    if let last = samples.last,
       last.bpm == bpm,
       last.source == source,
       abs(capturedAt.timeIntervalSince(last.capturedAt)) < 0.15 {
      stateLock.unlock()
      return false
    }

    samples.append(HeartRateSamplePoint(bpm: bpm, source: source, capturedAt: capturedAt))
    prune(relativeTo: capturedAt)
    schedulePersist()
    let shouldPostUpdate = markUpdateNotificationIfNeeded()
    stateLock.unlock()
    if shouldPostUpdate {
      NotificationCenter.default.post(name: Self.didUpdateNotification, object: self)
    }
    return true
  }

  func hourlyRanges(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> [HeartRateHourlyRange] {
    stateLock.lock()
    defer { stateLock.unlock() }
    return hourlyRangesLocked(forDayContaining: date, calendar: calendar)
  }

  func timelineSnapshot(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> HeartRateTimelineSnapshot {
    stateLock.lock()
    defer { stateLock.unlock() }
    let ranges = hourlyRangesLocked(forDayContaining: date, calendar: calendar)
    return HeartRateTimelineSnapshot(
      ranges: ranges,
      status: Self.summary(from: ranges),
      generatedAt: Date()
    )
  }

  private func hourlyRangesLocked(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> [HeartRateHourlyRange] {
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
    let bucketCount = max(1, Int(ceil(dayEnd.timeIntervalSince(dayStart) / 3600)))
    var buckets = Array(repeating: HeartRateHourlyBucket(), count: bucketCount)

    for sample in samples where sample.capturedAt >= dayStart && sample.capturedAt < dayEnd {
      let hourOffset = Int(sample.capturedAt.timeIntervalSince(dayStart) / 3600)
      guard buckets.indices.contains(hourOffset) else {
        continue
      }
      buckets[hourOffset].append(sample.bpm)
    }

    return buckets.enumerated().compactMap { offset, bucket in
      guard bucket.sampleCount > 0 else {
        return nil
      }
      let hourStart = dayStart.addingTimeInterval(TimeInterval(offset * 3600))
      let average = Double(bucket.totalBPM) / Double(bucket.sampleCount)
      return HeartRateHourlyRange(
        id: "\(Int64((hourStart.timeIntervalSince1970 * 1000).rounded()))",
        hourStart: hourStart,
        minBPM: bucket.minBPM,
        maxBPM: bucket.maxBPM,
        averageBPM: Int(average.rounded()),
        sampleCount: bucket.sampleCount
      )
    }
  }

  func samples(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> [HeartRateSamplePoint] {
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
    return samples(from: dayStart, to: dayEnd)
  }

  func samples(from start: Date, to end: Date) -> [HeartRateSamplePoint] {
    stateLock.lock()
    defer { stateLock.unlock() }
    return samples
      .filter { $0.capturedAt >= start && $0.capturedAt < end }
      .sorted { $0.capturedAt < $1.capturedAt }
  }

  func decimatedSamples(from start: Date, to end: Date, maxCount: Int = 500) -> [HeartRateSamplePoint] {
    let raw = samples(from: start, to: end)
    guard raw.count > 1000 else { return raw }
    let stride = max(1, raw.count / maxCount)
    var result: [HeartRateSamplePoint] = []
    result.reserveCapacity(raw.count / stride * 3)
    var i = 0
    while i < raw.count {
      let windowEnd = min(i + stride, raw.count)
      let window = raw[i..<windowEnd]
      let first = raw[i]
      result.append(first)
      if let maxSample = window.max(by: { $0.bpm < $1.bpm }), maxSample.id != first.id {
        result.append(maxSample)
      }
      if let minSample = window.min(by: { $0.bpm < $1.bpm }),
         minSample.id != first.id,
         minSample.id != result.last?.id {
        result.append(minSample)
      }
      i += stride
    }
    return result.sorted { $0.capturedAt < $1.capturedAt }
  }

  func decimatedSamples(forDayContaining date: Date = Date(), calendar: Calendar = .current, maxCount: Int = 500) -> [HeartRateSamplePoint] {
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(86400)
    return decimatedSamples(from: dayStart, to: dayEnd, maxCount: maxCount)
  }

  func summary(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> String {
    let ranges = hourlyRanges(forDayContaining: date, calendar: calendar)
    return Self.summary(from: ranges)
  }

  static func summary(from ranges: [HeartRateHourlyRange]) -> String {
    let sampleCount = ranges.reduce(0) { $0 + $1.sampleCount }
    guard sampleCount > 0,
          let minBPM = ranges.map(\.minBPM).min(),
          let maxBPM = ranges.map(\.maxBPM).max()
    else {
      return "No HR samples stored today"
    }
    return "\(sampleCount) HR samples today | \(minBPM)-\(maxBPM) bpm | \(ranges.count) hourly buckets"
  }

  func restingEstimate(
    forDayContaining date: Date = Date(),
    calendar: Calendar = .current,
    minimumSamples: Int = 12
  ) -> HeartRateRestingEstimate? {
    stateLock.lock()
    defer { stateLock.unlock() }
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
    let daySamples = samples.filter { $0.capturedAt >= dayStart && $0.capturedAt < dayEnd }
    let candidateSamples: [HeartRateSamplePoint]
    if daySamples.count >= minimumSamples {
      candidateSamples = daySamples
    } else {
      let groupedByDay = Dictionary(grouping: samples) { sample in
        calendar.startOfDay(for: sample.capturedAt)
      }
      candidateSamples = groupedByDay
        .sorted { $0.key > $1.key }
        .first { $0.value.count >= minimumSamples }?
        .value ?? []
    }

    guard candidateSamples.count >= minimumSamples else {
      return nil
    }

    let values = candidateSamples.map(\.bpm).sorted()
    let lowQuartileCount = max(1, values.count / 4)
    let lowQuartileValues = values.prefix(lowQuartileCount)
    let estimate = Double(lowQuartileValues.reduce(0, +)) / Double(lowQuartileValues.count)
    guard estimate.isFinite, (GooseHRSanitizer.minValidBPM...GooseHRSanitizer.maxValidBPM).contains(Int(estimate.rounded())) else {
      return nil
    }

    return HeartRateRestingEstimate(
      bpm: estimate,
      sampleCount: candidateSamples.count,
      updatedAt: candidateSamples.last?.capturedAt,
      source: "ble.hr.sample_store.low_quartile"
    )
  }

  func latestSample() -> HeartRateSamplePoint? {
    stateLock.lock()
    defer { stateLock.unlock() }
    return samples.last
  }

  private static func defaultURL() -> URL {
    let baseDirectory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    let directory = baseDirectory.appendingPathComponent("GooseSwift", isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory
      .appendingPathComponent("heart-rate-samples.json")
  }

  private static func loadSamples(from url: URL) -> [HeartRateSamplePoint] {
    guard let data = try? Data(contentsOf: url) else {
      return []
    }
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    if let file = try? decoder.decode(HeartRateSeriesFile.self, from: data) {
      return file.samples.sorted { $0.capturedAt < $1.capturedAt }
    }
    return (try? decoder.decode([HeartRateSamplePoint].self, from: data))?
      .sorted { $0.capturedAt < $1.capturedAt } ?? []
  }

  private func prune(relativeTo date: Date) {
    let cutoff = date.addingTimeInterval(-Self.retention)
    if let firstKept = samples.firstIndex(where: { $0.capturedAt >= cutoff }), firstKept > 0 {
      samples.removeFirst(firstKept)
    } else if samples.allSatisfy({ $0.capturedAt < cutoff }) {
      samples.removeAll()
    }
    if samples.count > Self.maxSamples {
      samples.removeFirst(samples.count - Self.maxSamples)
    }
  }

  private func schedulePersist() {
    guard pendingWrite == nil else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      guard let self else {
        return
      }
      self.stateLock.lock()
      self.pendingWrite = nil
      let url = self.url
      let payload = HeartRateSeriesFile(version: 1, samples: self.samples)
      self.stateLock.unlock()
      Self.persist(payload: payload, to: url)
    }
    pendingWrite = workItem
    writeQueue.asyncAfter(deadline: .now() + Self.persistDelay, execute: workItem)
  }

  private static func persist(payload: HeartRateSeriesFile, to url: URL) {
    do {
      let directory = url.deletingLastPathComponent()
      try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
      let encoder = JSONEncoder()
      encoder.dateEncodingStrategy = .iso8601
      let data = try encoder.encode(payload)
      try data.write(to: url, options: .atomic)
    } catch {
      NSLog("GooseSwift heart-rate sample persist failed: \(String(describing: error))")
    }
  }

  private func markUpdateNotificationIfNeeded() -> Bool {
    let now = Date()
    guard now.timeIntervalSince(lastNotificationAt) >= Self.updateNotificationInterval else {
      return false
    }
    lastNotificationAt = now
    return true
  }
}

struct HRVSamplePoint: Codable, Identifiable, Equatable {
  let id: String
  let capturedAt: Date
  let rmssdMS: Double
  let rrIntervalCount: Int
  let source: String

  init(rmssdMS: Double, rrIntervalCount: Int, source: String, capturedAt: Date) {
    let milliseconds = Int64((capturedAt.timeIntervalSince1970 * 1000).rounded())
    self.id = "\(milliseconds).\(Int((rmssdMS * 10).rounded())).\(rrIntervalCount).\(source)"
    self.capturedAt = capturedAt
    self.rmssdMS = rmssdMS
    self.rrIntervalCount = rrIntervalCount
    self.source = source
  }
}

struct HRVDailyEstimate: Equatable {
  let rmssdMS: Double
  let sampleCount: Int
  let rrIntervalCount: Int
  let updatedAt: Date?
  let source: String
}

struct HRVSeriesFile: Codable {
  let version: Int
  let samples: [HRVSamplePoint]
}

final class HRVSeriesStore {
  static let shared = HRVSeriesStore()
  static let didUpdateNotification = Notification.Name("GooseHRVSeriesStoreDidUpdate")

  private static let retention: TimeInterval = 14 * 24 * 60 * 60
  private static let maxSamples = 20_000
  private static let persistDelay: TimeInterval = 1.0
  private static let updateNotificationInterval: TimeInterval = 2.0

  private let url: URL
  private let stateLock = NSLock()
  private let writeQueue = DispatchQueue(label: "com.goose.swift.hrv-series", qos: .utility)
  private var samples: [HRVSamplePoint]
  private var pendingWrite: DispatchWorkItem?
  private var lastNotificationAt = Date.distantPast

  init(url: URL = HRVSeriesStore.defaultURL()) {
    self.url = url
    self.samples = Self.loadSamples(from: url)
    if samples.isEmpty, let migratedSample = Self.loadPersistedLiveSample() {
      samples = [migratedSample]
      schedulePersist()
    }
    prune(relativeTo: Date())
  }

  func append(rmssdMS: Double, rrIntervalCount: Int, source: String, capturedAt: Date) -> Bool {
    guard rmssdMS.isFinite, (0...300).contains(rmssdMS), rrIntervalCount >= 2 else {
      return false
    }
    stateLock.lock()
    if let last = samples.last,
       abs(last.rmssdMS - rmssdMS) < 0.05,
       last.rrIntervalCount == rrIntervalCount,
       last.source == source,
       abs(capturedAt.timeIntervalSince(last.capturedAt)) < 0.5 {
      stateLock.unlock()
      return false
    }

    samples.append(HRVSamplePoint(rmssdMS: rmssdMS, rrIntervalCount: rrIntervalCount, source: source, capturedAt: capturedAt))
    prune(relativeTo: capturedAt)
    schedulePersist()
    let shouldPostUpdate = markUpdateNotificationIfNeeded()
    stateLock.unlock()
    if shouldPostUpdate {
      NotificationCenter.default.post(name: Self.didUpdateNotification, object: self)
    }
    return true
  }

  func dailyEstimate(
    forDayContaining date: Date = Date(),
    calendar: Calendar = .current,
    minimumSamples: Int = 1
  ) -> HRVDailyEstimate? {
    stateLock.lock()
    defer { stateLock.unlock() }
    let dayStart = calendar.startOfDay(for: date)
    let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
    let daySamples = samples.filter { $0.capturedAt >= dayStart && $0.capturedAt < dayEnd }
    let candidateSamples: [HRVSamplePoint]
    if daySamples.count >= minimumSamples {
      candidateSamples = daySamples
    } else {
      let groupedByDay = Dictionary(grouping: samples) { sample in
        calendar.startOfDay(for: sample.capturedAt)
      }
      candidateSamples = groupedByDay
        .sorted { $0.key > $1.key }
        .first { $0.value.count >= minimumSamples }?
        .value ?? []
    }

    guard candidateSamples.count >= minimumSamples else {
      return nil
    }

    let totalRR = candidateSamples.reduce(0) { $0 + $1.rrIntervalCount }
    guard totalRR > 0 else {
      return nil
    }
    let weightedRMSSD = candidateSamples.reduce(0.0) { $0 + ($1.rmssdMS * Double($1.rrIntervalCount)) } / Double(totalRR)
    guard weightedRMSSD.isFinite, (0...300).contains(weightedRMSSD) else {
      return nil
    }

    return HRVDailyEstimate(
      rmssdMS: weightedRMSSD,
      sampleCount: candidateSamples.count,
      rrIntervalCount: totalRR,
      updatedAt: candidateSamples.last?.capturedAt,
      source: "ble.hr.sample_store.rmssd_daily_average"
    )
  }

  private static func defaultURL() -> URL {
    let baseDirectory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    let directory = baseDirectory.appendingPathComponent("GooseSwift", isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory
      .appendingPathComponent("hrv-samples.json")
  }

  private static func loadSamples(from url: URL) -> [HRVSamplePoint] {
    guard let data = try? Data(contentsOf: url) else {
      return []
    }
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    if let file = try? decoder.decode(HRVSeriesFile.self, from: data) {
      return file.samples.sorted { $0.capturedAt < $1.capturedAt }
    }
    return (try? decoder.decode([HRVSamplePoint].self, from: data))?
      .sorted { $0.capturedAt < $1.capturedAt } ?? []
  }

  private static func loadPersistedLiveSample() -> HRVSamplePoint? {
    let defaults = UserDefaults.standard
    guard defaults.object(forKey: "goose.swift.liveHRVRMSSD") != nil else {
      return nil
    }
    let rmssd = defaults.double(forKey: "goose.swift.liveHRVRMSSD")
    let rrIntervalCount = defaults.integer(forKey: "goose.swift.liveHRVRRIntervalCount")
    let source = defaults.string(forKey: "goose.swift.liveHRVSource") ?? "ble.hr.standard.average.migrated"
    let capturedAt = defaults.object(forKey: "goose.swift.liveHRVUpdatedAt") as? Date ?? Date()
    guard rmssd.isFinite, (0...300).contains(rmssd), rrIntervalCount >= 2 else {
      return nil
    }
    return HRVSamplePoint(
      rmssdMS: rmssd,
      rrIntervalCount: rrIntervalCount,
      source: "\(source).migrated_to_store",
      capturedAt: capturedAt
    )
  }

  private func prune(relativeTo date: Date) {
    let cutoff = date.addingTimeInterval(-Self.retention)
    if let firstKept = samples.firstIndex(where: { $0.capturedAt >= cutoff }), firstKept > 0 {
      samples.removeFirst(firstKept)
    } else if samples.allSatisfy({ $0.capturedAt < cutoff }) {
      samples.removeAll()
    }
    if samples.count > Self.maxSamples {
      samples.removeFirst(samples.count - Self.maxSamples)
    }
  }

  private func schedulePersist() {
    guard pendingWrite == nil else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      guard let self else {
        return
      }
      self.stateLock.lock()
      self.pendingWrite = nil
      let url = self.url
      let payload = HRVSeriesFile(version: 1, samples: self.samples)
      self.stateLock.unlock()
      Self.persist(payload: payload, to: url)
    }
    pendingWrite = workItem
    writeQueue.asyncAfter(deadline: .now() + Self.persistDelay, execute: workItem)
  }

  private static func persist(payload: HRVSeriesFile, to url: URL) {
    do {
      let directory = url.deletingLastPathComponent()
      try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
      let encoder = JSONEncoder()
      encoder.dateEncodingStrategy = .iso8601
      let data = try encoder.encode(payload)
      try data.write(to: url, options: .atomic)
    } catch {
      NSLog("GooseSwift HRV sample persist failed: \(String(describing: error))")
    }
  }

  private func markUpdateNotificationIfNeeded() -> Bool {
    let now = Date()
    guard now.timeIntervalSince(lastNotificationAt) >= Self.updateNotificationInterval else {
      return false
    }
    lastNotificationAt = now
    return true
  }
}

final class HeartRateSamplePipeline {
  var onHeartRateTimelineSnapshot: ((HeartRateTimelineSnapshot) -> Void)?

  private let queue = DispatchQueue(label: "com.goose.swift.heart-rate-sample-pipeline", qos: .utility)
  private let heartRateStore: HeartRateSeriesStore
  private let hrvStore: HRVSeriesStore
  private let timelinePublishInterval: TimeInterval
  private var lastTimelinePublishedAt = Date.distantPast

  init(
    heartRateStore: HeartRateSeriesStore = .shared,
    hrvStore: HRVSeriesStore = .shared,
    timelinePublishInterval: TimeInterval = 1
  ) {
    self.heartRateStore = heartRateStore
    self.hrvStore = hrvStore
    self.timelinePublishInterval = timelinePublishInterval
  }

  func refreshHeartRateTimeline(for date: Date = Date()) {
    queue.async { [weak self] in
      self?.publishHeartRateTimeline(for: date, force: true)
    }
  }

  func recordHeartRateSample(bpm: Int, source: String, capturedAt: Date) {
    queue.async { [weak self] in
      guard let self,
            self.heartRateStore.append(bpm: bpm, source: source, capturedAt: capturedAt) else {
        return
      }

      let now = Date()
      guard now.timeIntervalSince(self.lastTimelinePublishedAt) >= self.timelinePublishInterval else {
        return
      }
      self.publishHeartRateTimeline(for: now, force: true)
    }
  }

  func recordHRVSample(rmssdMS: Double, rrIntervalCount: Int, source: String, capturedAt: Date) {
    queue.async { [weak self] in
      _ = self?.hrvStore.append(rmssdMS: rmssdMS, rrIntervalCount: rrIntervalCount, source: source, capturedAt: capturedAt)
    }
  }

  private func publishHeartRateTimeline(for date: Date, force: Bool) {
    let now = Date()
    if !force, now.timeIntervalSince(lastTimelinePublishedAt) < timelinePublishInterval {
      return
    }
    let snapshot = heartRateStore.timelineSnapshot(forDayContaining: date)
    lastTimelinePublishedAt = now
    onHeartRateTimelineSnapshot?(snapshot)
  }
}
