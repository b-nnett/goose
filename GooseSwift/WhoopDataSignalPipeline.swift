import Foundation

final class WhoopDataSignalPipeline: @unchecked Sendable {
  var onStatus: ((String) -> Void)?

  private let queue = DispatchQueue(label: "com.goose.swift.whoop-data-signal", qos: .utility)
  private let stateLock = NSLock()
  private let ble: GooseBLEClient
  private let packetUIStateAggregator: PacketUIStateAggregator
  private let statusInterval: TimeInterval
  private let logInterval: TimeInterval
  private let deviceSignalPointInterval: TimeInterval
  private let maxQueuedSamples: Int

  private var queueDepth = 0
  private var queueHighWatermark = 0
  private var droppedSampleCount = 0
  private var lastQueueStatusPublishedAt = Date.distantPast
  private var lastDropLoggedAt = Date.distantPast
  private var lastDataSignalStatusUpdatedAt = Date.distantPast
  private var logCounts: [String: Int] = [:]
  private var lastLoggedAt: [String: Date] = [:]

  init(
    ble: GooseBLEClient,
    packetUIStateAggregator: PacketUIStateAggregator,
    statusInterval: TimeInterval,
    logInterval: TimeInterval,
    deviceSignalPointInterval: TimeInterval,
    maxQueuedSamples: Int
  ) {
    self.ble = ble
    self.packetUIStateAggregator = packetUIStateAggregator
    self.statusInterval = statusInterval
    self.logInterval = logInterval
    self.deviceSignalPointInterval = deviceSignalPointInterval
    self.maxQueuedSamples = maxQueuedSamples
  }

  func ingest(_ sample: WhoopDataSignalSample) {
    let now = Date()
    let reservation = reserveQueueSlot(now: now, sample: sample)
    guard reservation.accepted else {
      if let status = reservation.status {
        onStatus?(status)
      }
      if let dropLog = reservation.dropLog {
        ble.record(
          level: .warn,
          source: "performance.pipeline",
          title: "whoop_data_signal.queue_dropped",
          body: dropLog
        )
      }
      return
    }

    if let status = reservation.status {
      onStatus?(status)
    }

    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.process(sample)
      if let status = self.releaseQueueSlot(now: Date()) {
        self.onStatus?(status)
      }
    }
  }

  private func process(_ sample: WhoopDataSignalSample) {
    if sample.capturedAt.timeIntervalSince(lastDataSignalStatusUpdatedAt) >= statusInterval {
      lastDataSignalStatusUpdatedAt = sample.capturedAt
      packetUIStateAggregator.set(.whoopDataPacketStatus, sample.statusSummary)
    }

    if shouldLog(sample, reason: "data_packet.received") {
      ble.record(level: .debug, source: "whoop.data", title: "data_packet.received", body: sample.logSummary)
    }

    if let temperature = sample.historyTemperature {
      packetUIStateAggregator.set(.historyTemperatureCandidateStatus, temperature.summary)
      packetUIStateAggregator.set(.skinTemperatureCandidateStatus, temperature.summary)
      recordDeviceSignalPoint(
        family: "Skin Temp",
        value: temperature.temperatureC.map { String(format: "%.2f C", $0) } ?? "unresolved",
        detail: temperature.logSummary,
        capturedAt: sample.capturedAt,
        minimumInterval: 1
      )
      if shouldLog(sample, reason: "temperature.history_candidate") {
        ble.record(source: "whoop.data", title: "temperature.history_candidate", body: sample.logSummary)
      }
    }

    if let respiratoryRate = sample.historyRespiratoryRate {
      packetUIStateAggregator.set(.respiratoryRateCandidateStatus, respiratoryRate.summary)
      recordDeviceSignalPoint(
        family: "Resp RR",
        value: respiratoryRate.respiratoryRateRPM.map { String(format: "%.1f rpm", $0) } ?? "unresolved",
        detail: respiratoryRate.logSummary,
        capturedAt: sample.capturedAt,
        minimumInterval: 1
      )
      if shouldLog(sample, reason: "respiratory.history_candidate") {
        ble.record(source: "whoop.data", title: "respiratory.history_candidate", body: sample.logSummary)
      }
    }

    if sample.isPulseInformationPacket {
      packetUIStateAggregator.set(.pulseInformationPacketStatus, sample.pulseInformationSummary)
      recordDeviceSignalPoint(
        family: "Pulse",
        value: "K\(sample.packetK) \(sample.bodyByteCount) bytes",
        detail: sample.bodyKind,
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "pulse_information_packet.captured") {
        ble.record(source: "whoop.data", title: "pulse_information_packet.captured", body: sample.logSummary)
      }
    }

    if sample.isRawStreamCountedPacket {
      recordDeviceSignalPoint(
        family: "K11",
        value: "\(sample.bodyByteCount) bytes",
        detail: sample.bodyKind,
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "raw_stream_counted.captured") {
        ble.record(source: "whoop.data", title: "raw_stream_counted.captured", body: sample.logSummary)
      }
    }

    if sample.isRawResearchPacket {
      packetUIStateAggregator.set(.rawResearchPacketStatus, sample.rawDiagnosticSummary)
      recordDeviceSignalPoint(
        family: "K20",
        value: "\(sample.bodyByteCount) bytes",
        detail: sample.rawDiagnosticDetail,
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "raw_research_k20.captured") {
        ble.record(source: "whoop.data", title: "raw_research_k20.captured", body: sample.logSummary)
      }
    }

    if sample.isRealtimeStatusPacket {
      packetUIStateAggregator.set(.realtimeStatusPacketStatus, sample.rawDiagnosticSummary)
      recordDeviceSignalPoint(
        family: "K2",
        value: "\(sample.bodyByteCount) bytes",
        detail: sample.rawDiagnosticDetail,
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "realtime_status_k2.captured") {
        ble.record(source: "whoop.data", title: "realtime_status_k2.captured", body: sample.logSummary)
      }
    }

    if let r21MotionSummary = sample.r21MotionSummary {
      recordDeviceSignalPoint(
        family: "R21 IMU",
        value: r21MotionSummary,
        detail: sample.r21Motion?.compactLogSummary ?? sample.bodyKind,
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "imu.r21_motion.captured") {
        ble.record(
          source: "whoop.data",
          title: "imu.r21_motion.captured",
          body: "\(r21MotionSummary) \(sample.logSummary)"
        )
      }
    }

    if let opticalSummary = sample.opticalSummary {
      packetUIStateAggregator.set(.opticalPacketStatus, opticalSummary)
      recordDeviceSignalPoint(
        family: "Optical",
        value: opticalSummary,
        detail: sample.r17ChannelsOrGain.isEmpty
          ? sample.bodyKind
          : "channels=\(sample.r17ChannelsOrGain.map(String.init).joined(separator: ","))",
        capturedAt: sample.capturedAt
      )
      if shouldLog(sample, reason: "optical.packet.captured") {
        ble.record(source: "whoop.data", title: "optical.packet.captured", body: sample.logSummary)
      }
    }
  }

  private func recordDeviceSignalPoint(
    family: String,
    value: String,
    detail: String,
    capturedAt: Date,
    minimumInterval: TimeInterval? = nil
  ) {
    packetUIStateAggregator.recordDeviceSignalPoint(
      family: family,
      value: value,
      detail: detail,
      capturedAt: capturedAt,
      minimumInterval: minimumInterval ?? deviceSignalPointInterval
    )
  }

  private func shouldLog(_ sample: WhoopDataSignalSample, reason: String) -> Bool {
    let key = "\(reason).k\(sample.packetK).\(sample.bodyKind)"
    let count = (logCounts[key] ?? 0) + 1
    logCounts[key] = count
    if count <= 3 {
      lastLoggedAt[key] = sample.capturedAt
      return true
    }

    let lastLogged = lastLoggedAt[key] ?? .distantPast
    guard sample.capturedAt.timeIntervalSince(lastLogged) >= logInterval else {
      return false
    }
    lastLoggedAt[key] = sample.capturedAt
    return true
  }

  private func reserveQueueSlot(
    now: Date,
    sample: WhoopDataSignalSample
  ) -> (accepted: Bool, status: String?, dropLog: String?) {
    stateLock.lock()
    if queueDepth >= maxQueuedSamples {
      droppedSampleCount += 1
      let dropped = droppedSampleCount
      let shouldReportDrop = dropped <= 3 || now.timeIntervalSince(lastDropLoggedAt) >= 5
      if shouldReportDrop {
        lastDropLoggedAt = now
      }
      let status = shouldReportDrop
        ? "dataSignalQ full dropped=\(dropped) reason=queue_full max=\(maxQueuedSamples) K\(sample.packetK) \(sample.bodyKind)"
        : nil
      stateLock.unlock()
      return (false, status, status)
    }

    queueDepth += 1
    queueHighWatermark = max(queueHighWatermark, queueDepth)
    let status: String?
    if shouldPublishQueueStatus(now: now) {
      status = "dataSignal queued K\(sample.packetK) \(sample.bodyKind) | dataSignalQ \(queueDepth) hwm \(queueHighWatermark) dropped \(droppedSampleCount)"
    } else {
      status = nil
    }
    stateLock.unlock()
    return (true, status, nil)
  }

  private func releaseQueueSlot(now: Date) -> String? {
    stateLock.lock()
    queueDepth = max(0, queueDepth - 1)
    let status: String?
    if shouldPublishQueueStatus(now: now) {
      status = "dataSignal processed | dataSignalQ \(queueDepth) hwm \(queueHighWatermark) dropped \(droppedSampleCount)"
    } else {
      status = nil
    }
    stateLock.unlock()
    return status
  }

  private func shouldPublishQueueStatus(now: Date) -> Bool {
    guard now.timeIntervalSince(lastQueueStatusPublishedAt) >= 1 else {
      return false
    }
    lastQueueStatusPublishedAt = now
    return true
  }
}
