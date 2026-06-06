import Foundation
import UIKit

enum HealthPacketCaptureMode: String {
  case walk
  case temperature
  case physiology
  case hrMonitor = "hr_monitor"

  var purpose: String {
    switch self {
    case .walk:
      return "walk_movement_hr_activity_detection"
    case .temperature:
      return "temperature_history_event_capture"
    case .physiology:
      return "full_physiology_signal_capture"
    case .hrMonitor:
      return "standard_gatt_hr_monitor_capture"
    }
  }

  var targetFamilies: [String] {
    switch self {
    case .walk:
      return [
        "raw_motion_k10",
        "raw_stream_k11",
        "embedded_heart_rate",
        "passive_activity_candidate",
        "gps_route_if_authorized",
      ]
    case .temperature:
      return [
        "temperature_event_17",
        "normal_history_k18",
        "normal_history_k24",
        "history_metadata",
      ]
    case .physiology:
      return [
        "realtime_status_k2",
        "raw_motion_k10",
        "raw_stream_k11",
        "embedded_heart_rate",
        "raw_or_research_k20",
        "r17_optical_or_labrador_filtered",
        "raw_motion_k21",
        "pulse_information_k25_k26",
        "temperature_candidates_if_present",
      ]
    case .hrMonitor:
      return ["embedded_heart_rate"]
    }
  }

  var initialTargetSummary: String {
    switch self {
    case .walk:
      return "frames 0 | motion 0 | K11 0 | R21 0 | optical 0 | pulse 0 | temp 0 | unknown 0"
    case .temperature:
      return "frames 0 | K18 0 | K24 0 | event17 0 | temp 0 | unknown 0"
    case .physiology:
      return "frames 0 | motion 0 | K11 0 | HR 0 | R21 0 | optical 0 | pulse 0 | temp 0 | unknown 0"
    case .hrMonitor:
      return "frames 0 | BPM 0 | RR 0"
    }
  }

  var statusPrefix: String {
    switch self {
    case .walk:
      return "Capturing walk packets"
    case .temperature:
      return "Capturing temperature history"
    case .physiology:
      return "Capturing physiology signals"
    case .hrMonitor:
      return "Capturing HR monitor"
    }
  }
}

struct DeviceSignalPoint: Identifiable, Equatable {
  let id = UUID()
  let capturedAt: Date
  let family: String
  let value: String
  let detail: String
}

struct ActiveHealthPacketCapture {
  let sessionID: String
  let startedAt: Date
  let mode: HealthPacketCaptureMode
  var importedFrameCount: Int
}

struct OvernightGuardSession {
  let id: String
  let startedAt: Date
  let directoryURL: URL
  let rawNotificationsURL: URL?
}

struct OvernightGuardRecoveredSession {
  let id: String
  let startedAt: Date
  let modifiedAt: Date
  let notificationCount: Int
  let historicalRangePollCount: Int
  let successfulHistoricalRangePollCount: Int
  let commandWriteCount: Int
  let eventLogCount: Int
  let lastNotificationAt: Date?
  let lastStatusAt: Date?
  let lastStatusReason: String?
  let crashMarkerStatus: String?
  let targetCounts: OvernightGuardTargetCounts
  let rawByteCount: Int
  let historicalRangePollByteCount: Int
  let commandWriteByteCount: Int
  let eventLogByteCount: Int
  let directoryURL: URL
  let rawNotificationsURL: URL
  let crashMarkerURL: URL?
}

struct OvernightGuardTargetCounts {
  var k18 = 0
  var k24 = 0
  var k25 = 0
  var k26 = 0
  var packet47 = 0
  var event17 = 0
  var event29 = 0
  var metadata49 = 0
  var metadata56 = 0

  static let targetFamilyList = "K18/K24/K25/K26/packet47/event17/event29/metadata49/metadata56"

  var hasPhysiologyTargets: Bool {
    k18 > 0
      || k24 > 0
      || k25 > 0
      || k26 > 0
      || packet47 > 0
      || event17 > 0
      || event29 > 0
      || metadata49 > 0
      || metadata56 > 0
  }

  var summary: String {
    "K18 \(k18) | K24 \(k24) | K25 \(k25) | K26 \(k26) | packet47 \(packet47) | event17 \(event17) | event29 \(event29) | metadata49 \(metadata49) | metadata56 \(metadata56)"
  }
}

struct OvernightGuardHistoricalOrderEvidence {
  enum Verdict: String {
    case noHistoricalPackets = "no_historical_packets"
    case oldestFirst = "oldest_first"
    case newestFirst = "newest_first"
    case mixedOrOutOfOrder = "mixed_or_out_of_order"
    case singleHistoricalPacket = "single_historical_packet"
    case unknown = "unknown"
  }

  private static let sampleLimit = 8
  private static let targetPacketKs: Set<Int> = [18, 24, 25, 26]

  private(set) var totalPacket47Count = 0
  private(set) var firstSamples: [OvernightGuardHistoricalPacketSample] = []
  private(set) var lastSamples: [OvernightGuardHistoricalPacketSample] = []
  private(set) var firstSeenByFamily: [String: OvernightGuardHistoricalPacketSample] = [:]

  mutating func record(_ sample: WhoopDataSignalSample) -> Bool {
    guard sample.packetType == 47 else {
      return false
    }

    totalPacket47Count += 1
    let packetSample = OvernightGuardHistoricalPacketSample(
      sequence: totalPacket47Count,
      capturedAt: sample.capturedAt,
      packetK: sample.packetK,
      counterOrPage: sample.counterOrPage,
      deviceTimestampSeconds: sample.deviceTimestampSeconds,
      deviceTimestampSubseconds: sample.deviceTimestampSubseconds,
      bodyByteCount: sample.bodyByteCount,
      domain: sample.domain,
      bodyKind: sample.bodyKind
    )

    var changed = false
    if firstSamples.count < Self.sampleLimit {
      firstSamples.append(packetSample)
      changed = true
    }
    lastSamples.append(packetSample)
    if lastSamples.count > Self.sampleLimit {
      lastSamples.removeFirst(lastSamples.count - Self.sampleLimit)
    }

    if Self.targetPacketKs.contains(sample.packetK) {
      let family = "K\(sample.packetK)"
      if firstSeenByFamily[family] == nil {
        firstSeenByFamily[family] = packetSample
        changed = true
      }
    }
    return changed
  }

  var verdict: Verdict {
    guard totalPacket47Count > 0 else {
      return .noHistoricalPackets
    }
    let orderedSamples = (firstSamples + lastSamples)
      .reduce(into: [Int: OvernightGuardHistoricalPacketSample]()) { result, sample in
        result[sample.sequence] = sample
      }
      .values
      .sorted { $0.sequence < $1.sequence }
    guard orderedSamples.count >= 2 else {
      return .singleHistoricalPacket
    }

    let timestamps = orderedSamples.compactMap(\.deviceTimestampSeconds)
    if timestamps.count >= 2 {
      let deltas = zip(timestamps, timestamps.dropFirst()).map { $1 - $0 }
      let positive = deltas.filter { $0 > 0 }.count
      let negative = deltas.filter { $0 < 0 }.count
      if positive > 0, negative == 0 {
        return .oldestFirst
      }
      if negative > 0, positive == 0 {
        return .newestFirst
      }
      if positive > 0, negative > 0 {
        return .mixedOrOutOfOrder
      }
    }

    let counters = orderedSamples.compactMap(\.counterOrPage)
    if counters.count >= 2 {
      let deltas = zip(counters, counters.dropFirst()).map { $1 - $0 }
      let positive = deltas.filter { $0 > 0 }.count
      let negative = deltas.filter { $0 < 0 }.count
      if positive > 0, negative == 0 {
        return .oldestFirst
      }
      if negative > 0, positive == 0 {
        return .newestFirst
      }
      if positive > 0, negative > 0 {
        return .mixedOrOutOfOrder
      }
    }
    return .unknown
  }

  var summary: String {
    guard totalPacket47Count > 0 else {
      return "no packet47 bodies yet"
    }
    let first = firstSamples.first?.shortSummary ?? "first ?"
    let last = lastSamples.last?.shortSummary ?? "last ?"
    let targets = ["K18", "K24", "K25", "K26"].map { family in
      if let sample = firstSeenByFamily[family] {
        return "\(family) first #\(sample.sequence)"
      }
      return "\(family) none"
    }.joined(separator: " | ")
    return "\(verdict.rawValue) | packet47 \(totalPacket47Count) | \(first) -> \(last) | \(targets)"
  }

  var jsonObject: [String: Any] {
    [
      "schema": "goose.overnight.historical_transfer_order.v1",
      "verdict": verdict.rawValue,
      "packet47_count": totalPacket47Count,
      "first_samples": firstSamples.map(\.jsonObject),
      "last_samples": lastSamples.map(\.jsonObject),
      "first_seen_targets": firstSeenByFamily
        .sorted { $0.key < $1.key }
        .reduce(into: [String: Any]()) { result, entry in
          result[entry.key] = entry.value.jsonObject
        },
    ]
  }
}

struct OvernightGuardHistoricalPacketSample {
  let sequence: Int
  let capturedAt: Date
  let packetK: Int
  let counterOrPage: Int?
  let deviceTimestampSeconds: Int?
  let deviceTimestampSubseconds: Int?
  let bodyByteCount: Int
  let domain: String
  let bodyKind: String

  var shortSummary: String {
    var parts = ["#\(sequence)", "K\(packetK)"]
    if let deviceTimestampSeconds {
      parts.append("ts=\(deviceTimestampSeconds).\(deviceTimestampSubseconds ?? 0)")
    }
    if let counterOrPage {
      parts.append("counter=\(counterOrPage)")
    }
    return parts.joined(separator: " ")
  }

  var jsonObject: [String: Any] {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return [
      "sequence": sequence,
      "captured_at": formatter.string(from: capturedAt),
      "packet_type": 47,
      "packet_k": packetK,
      "counter_or_page": counterOrPage.map { $0 as Any } ?? NSNull(),
      "device_timestamp_seconds": deviceTimestampSeconds.map { $0 as Any } ?? NSNull(),
      "device_timestamp_subseconds": deviceTimestampSubseconds.map { $0 as Any } ?? NSNull(),
      "body_byte_count": bodyByteCount,
      "domain": domain,
      "body_kind": bodyKind,
    ]
  }
}

struct HealthPacketCaptureFamily: Identifiable, Equatable {
  let id: String
  let title: String
  var detail: String
  var count: Int
  var lastSeen: Date
  let status: HealthPacketCaptureFamilyStatus
}

struct HealthPacketCaptureFamilySnapshot {
  let rows: [HealthPacketCaptureFamily]
  let lastPacketSummary: String?
  let discoveredFamilies: [HealthPacketCaptureFamily]
  let queueDepth: Int
  let queueHighWatermark: Int
  let coalescedUpdateCount: Int
}

final class HealthPacketCaptureFamilyAggregator {
  var onSnapshot: ((HealthPacketCaptureFamilySnapshot) -> Void)?
  var onStatus: ((String) -> Void)?

  private let queue = DispatchQueue(label: "com.goose.swift.health-packet-family-aggregator", qos: .utility)
  private let stateLock = NSLock()
  private let publishInterval: TimeInterval
  private var rowsByID: [String: HealthPacketCaptureFamily] = [:]
  private var pendingLastPacketSummary: String?
  private var pendingDiscoveredFamilies: [HealthPacketCaptureFamily] = []
  private var coalescedUpdateCount = 0
  private var publishScheduled = false
  private var lastPublishedAt = Date.distantPast
  private var queuedOperationCount = 0
  private var queueHighWatermark = 0
  private var lastStatusEmittedAt = Date.distantPast
  private let statusInterval: TimeInterval = 5

  init(publishInterval: TimeInterval) {
    self.publishInterval = publishInterval
  }

  func reset() {
    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.rowsByID.removeAll(keepingCapacity: true)
      self.pendingLastPacketSummary = nil
      self.pendingDiscoveredFamilies.removeAll(keepingCapacity: true)
      self.coalescedUpdateCount = 0
      self.publishScheduled = false
      self.lastPublishedAt = .distantPast
      self.resetQueueDepth()
    }
  }

  func record(_ family: HealthPacketCaptureFamily, capturedAt: Date) {
    let queued = incrementQueueDepth()
    emitStatusIfNeeded(label: "queued", depth: queued.depth, highWatermark: queued.highWatermark)
    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.recordOnQueue(family, capturedAt: capturedAt)
      let completed = self.decrementQueueDepth()
      self.emitStatusIfNeeded(label: "completed", depth: completed.depth, highWatermark: completed.highWatermark)
    }
  }

  private func recordOnQueue(_ family: HealthPacketCaptureFamily, capturedAt: Date) {
    if var existing = rowsByID[family.id] {
      existing.count += 1
      existing.lastSeen = capturedAt
      existing.detail = family.detail
      rowsByID[family.id] = existing
      coalescedUpdateCount += 1
    } else {
      rowsByID[family.id] = family
      pendingDiscoveredFamilies.append(family)
    }

    pendingLastPacketSummary = "\(family.title) | \(family.detail)"
    schedulePublish(now: capturedAt)
  }

  private func schedulePublish(now: Date) {
    let elapsed = now.timeIntervalSince(lastPublishedAt)
    guard elapsed < publishInterval else {
      publish(now: now)
      return
    }
    guard !publishScheduled else {
      return
    }

    publishScheduled = true
    queue.asyncAfter(deadline: .now() + (publishInterval - elapsed)) { [weak self] in
      self?.publish(now: Date())
    }
  }

  private func publish(now: Date) {
    publishScheduled = false
    let rows = rowsByID.values.sorted { lhs, rhs in
      if lhs.status.sortRank != rhs.status.sortRank {
        return lhs.status.sortRank < rhs.status.sortRank
      }
      if lhs.count != rhs.count {
        return lhs.count > rhs.count
      }
      return lhs.lastSeen > rhs.lastSeen
    }
    let queueSnapshot = queueDepthSnapshot()
    let snapshot = HealthPacketCaptureFamilySnapshot(
      rows: rows,
      lastPacketSummary: pendingLastPacketSummary,
      discoveredFamilies: pendingDiscoveredFamilies,
      queueDepth: queueSnapshot.depth,
      queueHighWatermark: queueSnapshot.highWatermark,
      coalescedUpdateCount: coalescedUpdateCount
    )
    pendingLastPacketSummary = nil
    pendingDiscoveredFamilies.removeAll(keepingCapacity: true)
    coalescedUpdateCount = 0
    guard !snapshot.rows.isEmpty || snapshot.lastPacketSummary != nil || !snapshot.discoveredFamilies.isEmpty else {
      return
    }
    lastPublishedAt = now
    onSnapshot?(snapshot)
  }

  private func incrementQueueDepth() -> (depth: Int, highWatermark: Int) {
    stateLock.lock()
    queuedOperationCount += 1
    queueHighWatermark = max(queueHighWatermark, queuedOperationCount)
    let snapshot = (queuedOperationCount, queueHighWatermark)
    stateLock.unlock()
    return snapshot
  }

  private func decrementQueueDepth() -> (depth: Int, highWatermark: Int) {
    stateLock.lock()
    queuedOperationCount = max(0, queuedOperationCount - 1)
    let snapshot = (queuedOperationCount, queueHighWatermark)
    stateLock.unlock()
    return snapshot
  }

  private func resetQueueDepth() {
    stateLock.lock()
    queuedOperationCount = 0
    queueHighWatermark = 0
    lastStatusEmittedAt = .distantPast
    stateLock.unlock()
  }

  private func queueDepthSnapshot() -> (depth: Int, highWatermark: Int) {
    stateLock.lock()
    let snapshot = (queuedOperationCount, queueHighWatermark)
    stateLock.unlock()
    return snapshot
  }

  private func emitStatusIfNeeded(label: String, depth: Int, highWatermark: Int) {
    let now = Date()
    stateLock.lock()
    let shouldEmit = depth >= 8 || now.timeIntervalSince(lastStatusEmittedAt) >= statusInterval
    if shouldEmit {
      lastStatusEmittedAt = now
    }
    stateLock.unlock()
    guard shouldEmit else {
      return
    }
    onStatus?("capture family \(label) | familyQ \(depth) hwm \(highWatermark)")
  }
}

enum HealthPacketCaptureFamilyStatus: String {
  case target
  case expected
  case unresolved
  case unknown

  var sortRank: Int {
    switch self {
    case .target:
      return 0
    case .unresolved:
      return 1
    case .unknown:
      return 2
    case .expected:
      return 3
    }
  }
}
