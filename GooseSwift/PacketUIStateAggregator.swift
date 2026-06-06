import Foundation

struct PacketUIStateSnapshot {
  let lastParsedFrameSummary: String?
  let movementPacketStatus: String?
  let whoopEventStatus: String?
  let skinTemperatureCandidateStatus: String?
  let whoopDataPacketStatus: String?
  let historyTemperatureCandidateStatus: String?
  let respiratoryRateCandidateStatus: String?
  let pulseInformationPacketStatus: String?
  let opticalPacketStatus: String?
  let rawResearchPacketStatus: String?
  let realtimeStatusPacketStatus: String?
  let performancePipelineStatus: String?
  let liveDeviceDataSummary: String?
  let deviceSignalPoints: [DeviceSignalPoint]
  let deviceSignalCountsByFamily: [String: Int]
  let droppedDeviceSignalPointCount: Int
  let coalescedStatusUpdateCount: Int
  let coalescedStatusUpdateSummary: String?
}

final class PacketUIStateAggregator: @unchecked Sendable {
  enum Field {
    case lastParsedFrameSummary
    case movementPacketStatus
    case whoopEventStatus
    case skinTemperatureCandidateStatus
    case whoopDataPacketStatus
    case historyTemperatureCandidateStatus
    case respiratoryRateCandidateStatus
    case pulseInformationPacketStatus
    case opticalPacketStatus
    case rawResearchPacketStatus
    case realtimeStatusPacketStatus
    case performancePipelineStatus
    case liveDeviceDataSummary
  }

  var onSnapshot: ((PacketUIStateSnapshot) -> Void)?

  private let queue = DispatchQueue(label: "com.goose.swift.packet-ui-state", qos: .utility)
  private let publishInterval: TimeInterval
  private let maximumPendingDeviceSignalPoints: Int
  private var publishScheduled = false
  private var lastPublishedAt = Date.distantPast

  private var pendingLastParsedFrameSummary: String?
  private var pendingMovementPacketStatus: String?
  private var pendingWhoopEventStatus: String?
  private var pendingSkinTemperatureCandidateStatus: String?
  private var pendingWhoopDataPacketStatus: String?
  private var pendingHistoryTemperatureCandidateStatus: String?
  private var pendingRespiratoryRateCandidateStatus: String?
  private var pendingPulseInformationPacketStatus: String?
  private var pendingOpticalPacketStatus: String?
  private var pendingRawResearchPacketStatus: String?
  private var pendingRealtimeStatusPacketStatus: String?
  private var pendingPerformancePipelineStatus: String?
  private var pendingLiveDeviceDataSummary: String?
  private var pendingDeviceSignalPoints: [DeviceSignalPoint] = []
  private var deviceSignalCountsByFamily: [String: Int] = [:]
  private var deviceSignalLastPublishedAtByFamily: [String: Date] = [:]
  private var droppedDeviceSignalPointCount = 0
  private var coalescedFieldUpdateCounts: [String: Int] = [:]

  init(
    publishInterval: TimeInterval,
    maximumPendingDeviceSignalPoints: Int
  ) {
    self.publishInterval = publishInterval
    self.maximumPendingDeviceSignalPoints = maximumPendingDeviceSignalPoints
  }

  func set(_ field: Field, _ value: String) {
    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.setPending(field, value)
      self.schedulePublish(now: Date())
    }
  }

  func recordDeviceSignalPoint(
    family: String,
    value: String,
    detail: String,
    capturedAt: Date,
    minimumInterval: TimeInterval
  ) {
    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.deviceSignalCountsByFamily[family, default: 0] += 1
      let lastPublishedAt = self.deviceSignalLastPublishedAtByFamily[family] ?? .distantPast
      guard capturedAt.timeIntervalSince(lastPublishedAt) >= minimumInterval else {
        self.recordCoalescedUpdate(fieldKey: "device_signal_throttled")
        self.schedulePublish(now: Date())
        return
      }

      self.deviceSignalLastPublishedAtByFamily[family] = capturedAt
      let point = DeviceSignalPoint(
        capturedAt: capturedAt,
        family: family,
        value: value,
        detail: detail
      )
      self.pendingDeviceSignalPoints.append(point)
      if self.pendingDeviceSignalPoints.count > self.maximumPendingDeviceSignalPoints {
        let dropCount = self.pendingDeviceSignalPoints.count - self.maximumPendingDeviceSignalPoints
        self.pendingDeviceSignalPoints.removeFirst(dropCount)
        self.droppedDeviceSignalPointCount += dropCount
      }
      if self.pendingLiveDeviceDataSummary != nil {
        self.recordCoalescedUpdate(fieldKey: Self.fieldKey(.liveDeviceDataSummary))
      }
      self.pendingLiveDeviceDataSummary = self.liveDeviceDataSummary(fallbackFamily: family, fallbackValue: value)
      self.schedulePublish(now: Date())
    }
  }

  func appendDeviceSignalPoint(_ point: DeviceSignalPoint, liveSummary: String) {
    recordDeviceSignalPoint(
      family: point.family,
      value: point.value,
      detail: point.detail,
      capturedAt: point.capturedAt,
      minimumInterval: 0
    )
  }

  private func setPending(_ field: Field, _ value: String) {
    if pendingValue(for: field) != nil {
      recordCoalescedUpdate(fieldKey: Self.fieldKey(field))
    }
    switch field {
    case .lastParsedFrameSummary:
      pendingLastParsedFrameSummary = value
    case .movementPacketStatus:
      pendingMovementPacketStatus = value
    case .whoopEventStatus:
      pendingWhoopEventStatus = value
    case .skinTemperatureCandidateStatus:
      pendingSkinTemperatureCandidateStatus = value
    case .whoopDataPacketStatus:
      pendingWhoopDataPacketStatus = value
    case .historyTemperatureCandidateStatus:
      pendingHistoryTemperatureCandidateStatus = value
    case .respiratoryRateCandidateStatus:
      pendingRespiratoryRateCandidateStatus = value
    case .pulseInformationPacketStatus:
      pendingPulseInformationPacketStatus = value
    case .opticalPacketStatus:
      pendingOpticalPacketStatus = value
    case .rawResearchPacketStatus:
      pendingRawResearchPacketStatus = value
    case .realtimeStatusPacketStatus:
      pendingRealtimeStatusPacketStatus = value
    case .performancePipelineStatus:
      pendingPerformancePipelineStatus = value
    case .liveDeviceDataSummary:
      pendingLiveDeviceDataSummary = value
    }
  }

  private func pendingValue(for field: Field) -> String? {
    switch field {
    case .lastParsedFrameSummary:
      return pendingLastParsedFrameSummary
    case .movementPacketStatus:
      return pendingMovementPacketStatus
    case .whoopEventStatus:
      return pendingWhoopEventStatus
    case .skinTemperatureCandidateStatus:
      return pendingSkinTemperatureCandidateStatus
    case .whoopDataPacketStatus:
      return pendingWhoopDataPacketStatus
    case .historyTemperatureCandidateStatus:
      return pendingHistoryTemperatureCandidateStatus
    case .respiratoryRateCandidateStatus:
      return pendingRespiratoryRateCandidateStatus
    case .pulseInformationPacketStatus:
      return pendingPulseInformationPacketStatus
    case .opticalPacketStatus:
      return pendingOpticalPacketStatus
    case .rawResearchPacketStatus:
      return pendingRawResearchPacketStatus
    case .realtimeStatusPacketStatus:
      return pendingRealtimeStatusPacketStatus
    case .performancePipelineStatus:
      return pendingPerformancePipelineStatus
    case .liveDeviceDataSummary:
      return pendingLiveDeviceDataSummary
    }
  }

  private func recordCoalescedUpdate(fieldKey: String) {
    coalescedFieldUpdateCounts[fieldKey, default: 0] += 1
  }

  private static func fieldKey(_ field: Field) -> String {
    switch field {
    case .lastParsedFrameSummary:
      return "parsed_frame"
    case .movementPacketStatus:
      return "movement"
    case .whoopEventStatus:
      return "whoop_event"
    case .skinTemperatureCandidateStatus:
      return "skin_temperature"
    case .whoopDataPacketStatus:
      return "data_packet"
    case .historyTemperatureCandidateStatus:
      return "history_temperature"
    case .respiratoryRateCandidateStatus:
      return "respiratory_rate"
    case .pulseInformationPacketStatus:
      return "pulse_information"
    case .opticalPacketStatus:
      return "optical"
    case .rawResearchPacketStatus:
      return "raw_research"
    case .realtimeStatusPacketStatus:
      return "realtime_status"
    case .performancePipelineStatus:
      return "performance_pipeline"
    case .liveDeviceDataSummary:
      return "live_device_summary"
    }
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
    let coalescedStatusUpdateCount = coalescedFieldUpdateCounts.values.reduce(0, +)
    let snapshot = PacketUIStateSnapshot(
      lastParsedFrameSummary: pendingLastParsedFrameSummary,
      movementPacketStatus: pendingMovementPacketStatus,
      whoopEventStatus: pendingWhoopEventStatus,
      skinTemperatureCandidateStatus: pendingSkinTemperatureCandidateStatus,
      whoopDataPacketStatus: pendingWhoopDataPacketStatus,
      historyTemperatureCandidateStatus: pendingHistoryTemperatureCandidateStatus,
      respiratoryRateCandidateStatus: pendingRespiratoryRateCandidateStatus,
      pulseInformationPacketStatus: pendingPulseInformationPacketStatus,
      opticalPacketStatus: pendingOpticalPacketStatus,
      rawResearchPacketStatus: pendingRawResearchPacketStatus,
      realtimeStatusPacketStatus: pendingRealtimeStatusPacketStatus,
      performancePipelineStatus: pendingPerformancePipelineStatus,
      liveDeviceDataSummary: pendingLiveDeviceDataSummary,
      deviceSignalPoints: pendingDeviceSignalPoints,
      deviceSignalCountsByFamily: deviceSignalCountsByFamily,
      droppedDeviceSignalPointCount: droppedDeviceSignalPointCount,
      coalescedStatusUpdateCount: coalescedStatusUpdateCount,
      coalescedStatusUpdateSummary: Self.coalescedSummary(from: coalescedFieldUpdateCounts)
    )
    clearPending()
    guard snapshot.hasChanges else {
      return
    }
    lastPublishedAt = now
    onSnapshot?(snapshot)
  }

  private func clearPending() {
    pendingLastParsedFrameSummary = nil
    pendingMovementPacketStatus = nil
    pendingWhoopEventStatus = nil
    pendingSkinTemperatureCandidateStatus = nil
    pendingWhoopDataPacketStatus = nil
    pendingHistoryTemperatureCandidateStatus = nil
    pendingRespiratoryRateCandidateStatus = nil
    pendingPulseInformationPacketStatus = nil
    pendingOpticalPacketStatus = nil
    pendingRawResearchPacketStatus = nil
    pendingRealtimeStatusPacketStatus = nil
    pendingPerformancePipelineStatus = nil
    pendingLiveDeviceDataSummary = nil
    pendingDeviceSignalPoints.removeAll(keepingCapacity: true)
    droppedDeviceSignalPointCount = 0
    coalescedFieldUpdateCounts.removeAll(keepingCapacity: true)
  }

  private func liveDeviceDataSummary(fallbackFamily: String, fallbackValue: String) -> String {
    let topFamilies = deviceSignalCountsByFamily
      .sorted { lhs, rhs in
        if lhs.value != rhs.value {
          return lhs.value > rhs.value
        }
        return lhs.key < rhs.key
      }
      .prefix(4)
      .map { "\($0.key) \($0.value)" }
      .joined(separator: " | ")
    return topFamilies.isEmpty ? "\(fallbackFamily) \(fallbackValue)" : topFamilies
  }

  private static func coalescedSummary(from counts: [String: Int]) -> String? {
    guard !counts.isEmpty else {
      return nil
    }
    return counts
      .sorted { lhs, rhs in
        if lhs.value == rhs.value {
          return lhs.key < rhs.key
        }
        return lhs.value > rhs.value
      }
      .prefix(5)
      .map { "\($0.key)=\($0.value)" }
      .joined(separator: ", ")
  }
}

private extension PacketUIStateSnapshot {
  var hasChanges: Bool {
    lastParsedFrameSummary != nil
      || movementPacketStatus != nil
      || whoopEventStatus != nil
      || skinTemperatureCandidateStatus != nil
      || whoopDataPacketStatus != nil
      || historyTemperatureCandidateStatus != nil
      || respiratoryRateCandidateStatus != nil
      || pulseInformationPacketStatus != nil
      || opticalPacketStatus != nil
      || rawResearchPacketStatus != nil
      || realtimeStatusPacketStatus != nil
      || performancePipelineStatus != nil
      || liveDeviceDataSummary != nil
      || !deviceSignalPoints.isEmpty
      || droppedDeviceSignalPointCount > 0
      || coalescedStatusUpdateCount > 0
  }
}
