import Foundation
import UIKit


extension GooseAppModel {
  func recordHealthPacketCaptureFamily(_ family: HealthPacketCaptureFamily, capturedAt: Date) {
    guard activeHealthPacketCapture != nil else {
      return
    }

    healthPacketCaptureFamilyAggregator.record(family, capturedAt: capturedAt)
  }

  func applyHealthPacketCaptureFamilySnapshot(_ snapshot: HealthPacketCaptureFamilySnapshot) {
    guard activeHealthPacketCapture != nil else {
      return
    }

    healthPacketCaptureFamilyRows = snapshot.rows
    healthPacketCaptureFamilyRowsByID = Dictionary(
      uniqueKeysWithValues: snapshot.rows.map { ($0.id, $0) }
    )
    if let lastPacketSummary = snapshot.lastPacketSummary {
      healthPacketCaptureLastPacketSummary = lastPacketSummary
    }
    for family in snapshot.discoveredFamilies {
      ble.record(
        source: "health.packet_capture",
        title: "family.discovered",
        body: "\(family.title) status=\(family.status.rawValue) \(family.detail)"
      )
    }
    updateHealthPacketCaptureTargetSummary(rows: snapshot.rows)
    logHealthPacketCaptureSummaryIfNeeded(now: Date())
    if snapshot.coalescedUpdateCount > 0 || snapshot.queueDepth > 0 {
      publishPipelinePerformanceStatus(
        "capture family applied rows=\(snapshot.rows.count) coalesced=\(snapshot.coalescedUpdateCount) | familyQ \(snapshot.queueDepth) hwm \(snapshot.queueHighWatermark)"
      )
    }
  }

  func scheduleHealthPacketCaptureUIUpdate(now: Date = Date()) {
    let elapsed = now.timeIntervalSince(lastHealthPacketCaptureUIUpdatedAt)
    guard elapsed < Self.healthPacketCaptureUIUpdateInterval else {
      publishHealthPacketCaptureUIUpdate(now: now)
      return
    }
    guard healthPacketCaptureUIUpdateWorkItem == nil else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.publishHealthPacketCaptureUIUpdate()
      }
    }
    healthPacketCaptureUIUpdateWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + (Self.healthPacketCaptureUIUpdateInterval - elapsed), execute: workItem)
  }

  func publishHealthPacketCaptureUIUpdate(now: Date = Date()) {
    healthPacketCaptureUIUpdateWorkItem?.cancel()
    healthPacketCaptureUIUpdateWorkItem = nil
    lastHealthPacketCaptureUIUpdatedAt = now
    if let capture = activeHealthPacketCapture {
      healthPacketCaptureFrameCount = capture.importedFrameCount
    }
    if let pendingHealthPacketCaptureLastPacketSummary {
      healthPacketCaptureLastPacketSummary = pendingHealthPacketCaptureLastPacketSummary
      self.pendingHealthPacketCaptureLastPacketSummary = nil
    }
    updateHealthPacketCaptureTargetSummary(rows: healthPacketCaptureFamilyRows)
    logHealthPacketCaptureSummaryIfNeeded(now: now)
  }

  func logHealthPacketCaptureSummaryIfNeeded(now: Date) {
    guard activeHealthPacketCapture != nil else {
      return
    }
    guard now.timeIntervalSince(lastHealthPacketCaptureSummaryLoggedAt) >= Self.healthPacketCaptureSummaryLogInterval else {
      return
    }
    lastHealthPacketCaptureSummaryLoggedAt = now

    let topFamilies = healthPacketCaptureFamilyRows
      .prefix(8)
      .map { "\($0.id)=\($0.count)" }
      .joined(separator: " | ")
    let signalCounts = deviceSignalCountsByFamily
      .sorted { lhs, rhs in
        if lhs.value != rhs.value {
          return lhs.value > rhs.value
        }
        return lhs.key < rhs.key
      }
      .prefix(8)
      .map { "\($0.key)=\($0.value)" }
      .joined(separator: " | ")
    ble.record(
      source: "health.packet_capture",
      title: "summary",
      body: "\(healthPacketCaptureTargetSummary) last=\(healthPacketCaptureLastPacketSummary) families=\(topFamilies.isEmpty ? "none" : topFamilies) signals=\(signalCounts.isEmpty ? "none" : signalCounts)"
    )
    writeCaptureStatusSnapshot(topFamilies: topFamilies, signalCounts: signalCounts)
  }

  func writeCaptureStatusSnapshot(topFamilies: String, signalCounts: String) {
    guard let captureStatusSnapshotURL else {
      return
    }
    let activeCapture = activeHealthPacketCapture
    let lines = [
      "timestamp=\(Self.captureTimestampFormatter.string(from: Date()))",
      "session_id=\(activeCapture?.sessionID ?? "none")",
      "mode=\(activeCapture?.mode.rawValue ?? "none")",
      "frame_count=\(healthPacketCaptureFrameCount)",
      "status=\(healthPacketCaptureStatus)",
      "target_summary=\(healthPacketCaptureTargetSummary)",
      "last_packet=\(healthPacketCaptureLastPacketSummary)",
      "top_families=\(topFamilies.isEmpty ? "none" : topFamilies)",
      "signal_counts=\(signalCounts.isEmpty ? "none" : signalCounts)",
      "latest_data_packet=\(latestWhoopDataPacketStatus)",
      "latest_skin_temp=\(latestSkinTemperatureCandidateStatus)",
      "latest_history_temp=\(latestHistoryTemperatureCandidateStatus)",
      "latest_respiratory=\(latestRespiratoryRateCandidateStatus)",
      "latest_pulse=\(latestPulseInformationPacketStatus)",
      "latest_optical=\(latestOpticalPacketStatus)",
      "latest_raw_research=\(latestRawResearchPacketStatus)",
      "latest_realtime_status=\(latestRealtimeStatusPacketStatus)",
      "movement=\(movementPacketStatus)",
      "activity_detection=\(activityDetectionStatus)",
      "performance_pipeline=\(performancePipelineStatus)",
      "ble_physiology=\(ble.physiologyCaptureStatus)",
      "ble_last_physiology_command=\(ble.lastPhysiologyCommandSummary)",
      "ble_historical_sync=\(ble.historicalSyncStatus)",
    ]
    let snapshot = lines.joined(separator: "\n").appending("\n")
    captureStatusSnapshotWriteQueue.async {
      try? snapshot.write(
        to: captureStatusSnapshotURL,
        atomically: true,
        encoding: .utf8
      )
    }
  }

  nonisolated static func healthPacketCaptureFamily(for parsed: [String: Any], capturedAt: Date) -> HealthPacketCaptureFamily {
    let packetName = parsed["packet_type_name"] as? String ?? "unknown"
    let packetType = intString(parsed["packet_type"])
    guard let payload = parsed["parsed_payload"] as? [String: Any] else {
      return HealthPacketCaptureFamily(
        id: "packet.\(packetType).no_payload",
        title: packetName,
        detail: "packet=\(packetType) payload=none",
        count: 1,
        lastSeen: capturedAt,
        status: .unresolved
      )
    }

    let kind = payload["kind"] as? String ?? "unknown"
    if kind == "data_packet" {
      let packetK = intValue(payload["packet_k"])
      let domain = payload["domain"] as? String ?? "unknown"
      let bodySummary = payload["body_summary"] as? [String: Any]
      let bodyKind = bodySummary?["kind"] as? String ?? "raw"
      let bodyHex = payload["body_hex"] as? String ?? ""
      let status = Self.healthPacketCaptureStatus(packetK: packetK, bodyKind: bodyKind)
      let title = packetK.map { "K\($0) \(Self.healthPacketCaptureFamilyName(packetK: $0))" } ?? "Data Packet"
      return HealthPacketCaptureFamily(
        id: "data.k\(packetK.map(String.init) ?? "unknown").\(bodyKind)",
        title: title,
        detail: "domain=\(domain) body=\(bodyKind) bytes=\(bodyHex.count / 2)",
        count: 1,
        lastSeen: capturedAt,
        status: status
      )
    }

    if kind == "event" {
      let eventID = intValue(payload["event_id"])
      let eventName = payload["event_name"] as? String ?? eventID.map { "event_\($0)" } ?? "unknown"
      let dataHex = payload["data_hex"] as? String ?? ""
      return HealthPacketCaptureFamily(
        id: "event.\(eventID.map(String.init) ?? "unknown").\(eventName)",
        title: "Event \(eventName)",
        detail: "id=\(eventID.map(String.init) ?? "?") bytes=\(dataHex.count / 2) packet=\(packetName) semantics=pending",
        count: 1,
        lastSeen: capturedAt,
        status: Self.healthPacketCaptureEventStatus(eventID: eventID, eventName: eventName)
      )
    }

    return HealthPacketCaptureFamily(
      id: "payload.\(kind).\(packetName)",
      title: packetName,
      detail: "packet=\(packetType) payload=\(kind)",
      count: 1,
      lastSeen: capturedAt,
      status: .expected
    )
  }

  nonisolated static func healthPacketCaptureFamily(for compact: NotificationFrameCompactSummary, capturedAt: Date) -> HealthPacketCaptureFamily {
    let packetName = compact.packetTypeName ?? "unknown"
    let packetType = compact.packetType.map(String.init) ?? "?"
    guard let payloadKind = compact.payloadKind else {
      return HealthPacketCaptureFamily(
        id: "packet.\(packetType).no_payload",
        title: packetName,
        detail: "packet=\(packetType) payload=none",
        count: 1,
        lastSeen: capturedAt,
        status: .unresolved
      )
    }

    if payloadKind == "data_packet" {
      let bodyKind = compact.bodyKind ?? "raw"
      let status = Self.healthPacketCaptureStatus(packetK: compact.packetK, bodyKind: bodyKind)
      let title = compact.packetK.map { "K\($0) \(Self.healthPacketCaptureFamilyName(packetK: $0))" } ?? "Data Packet"
      return HealthPacketCaptureFamily(
        id: "data.k\(compact.packetK.map(String.init) ?? "unknown").\(bodyKind)",
        title: title,
        detail: "domain=\(compact.domain ?? "unknown") body=\(bodyKind) bytes=\(compact.bodyByteCount ?? 0)",
        count: 1,
        lastSeen: capturedAt,
        status: status
      )
    }

    if payloadKind == "event" {
      let eventName = compact.eventName ?? compact.eventID.map { "event_\($0)" } ?? "unknown"
      return HealthPacketCaptureFamily(
        id: "event.\(compact.eventID.map(String.init) ?? "unknown").\(eventName)",
        title: "Event \(eventName)",
        detail: "id=\(compact.eventID.map(String.init) ?? "?") bytes=\(compact.eventByteCount ?? 0) packet=\(packetName) semantics=pending",
        count: 1,
        lastSeen: capturedAt,
        status: Self.healthPacketCaptureEventStatus(eventID: compact.eventID, eventName: eventName)
      )
    }

    return HealthPacketCaptureFamily(
      id: "payload.\(payloadKind).\(packetName)",
      title: packetName,
      detail: "packet=\(packetType) payload=\(payloadKind)",
      count: 1,
      lastSeen: capturedAt,
      status: .expected
    )
  }

  nonisolated static func healthPacketCaptureStatus(packetK: Int?, bodyKind: String) -> HealthPacketCaptureFamilyStatus {
    guard let packetK else {
      return .unresolved
    }
    switch packetK {
    case 2, 20:
      return .expected
    case 10, 11:
      return .target
    case 17, 18, 21, 24, 25, 26, 47:
      return .target
    default:
      return bodyKind == "raw" ? .unresolved : .unknown
    }
  }

  nonisolated static func healthPacketCaptureEventStatus(eventID: Int?, eventName: String) -> HealthPacketCaptureFamilyStatus {
    if eventID == 17 || eventName == "TEMPERATURE_LEVEL" {
      return .target
    }
    if eventID == 49 || eventID == 56 {
      return .target
    }
    if eventID != nil {
      return .expected
    }
    return .unknown
  }

  nonisolated static func healthPacketCaptureFamilyName(packetK: Int) -> String {
    switch packetK {
    case 2:
      return "Status K2"
    case 10:
      return "Motion/HR"
    case 11:
      return "Raw Stream"
    case 17:
      return "Optical R17"
    case 18:
      return "History K18"
    case 20:
      return "Raw/Research K20"
    case 21:
      return "IMU R21"
    case 24:
      return "History K24"
    case 25:
      return "Pulse K25"
    case 26:
      return "Pulse K26"
    case 47:
      return "Historical K47"
    default:
      return "Data"
    }
  }

  func updateHealthPacketCaptureTargetSummary(rows: [HealthPacketCaptureFamily]? = nil) {
    let rows = rows ?? Array(healthPacketCaptureFamilyRowsByID.values)
    func countForIDs(_ prefixes: String...) -> Int {
      rows
        .filter { row in prefixes.contains(where: { row.id.hasPrefix($0) }) }
        .reduce(0) { $0 + $1.count }
    }
    let motion = countForIDs("data.k10.")
    let rawStream = countForIDs("data.k11.")
    let realtimeStatus = countForIDs("data.k2.")
    let rawResearch = countForIDs("data.k20.")
    let heartRate = motion
    let r21 = countForIDs("data.k21.")
    let optical = countForIDs("data.k17.")
    let pulse = countForIDs("data.k25.", "data.k26.")
    let k18History = countForIDs("data.k18.")
    let k24History = countForIDs("data.k24.")
    let k47History = countForIDs("data.k47.")
    let eventTemperature = countForIDs("event.17.", "event.unknown.TEMPERATURE_LEVEL")
    let metadataEvents = countForIDs("event.49.", "event.56.")
    let temperature = k18History + k24History + eventTemperature
    let unresolved = rows
      .filter { $0.status == .unresolved || $0.status == .unknown }
      .reduce(0) { $0 + $1.count }
    if activeHealthPacketCapture?.mode == .temperature {
      healthPacketCaptureTargetSummary = "frames \(healthPacketCaptureFrameCount) | K18 \(k18History) | K24 \(k24History) | K47 \(k47History) | event17 \(eventTemperature) | metadata \(metadataEvents) | temp \(temperature) | unknown \(unresolved)"
    } else if r21 + optical + pulse + temperature > 0 {
      healthPacketCaptureTargetSummary = "frames \(healthPacketCaptureFrameCount) | motion \(motion) | K11 \(rawStream) | K20 \(rawResearch) | K2 \(realtimeStatus) | HR \(heartRate) | R21 \(r21) | optical \(optical) | pulse \(pulse) | K47 \(k47History) | metadata \(metadataEvents) | temp \(temperature) | unknown \(unresolved)"
    } else {
      healthPacketCaptureTargetSummary = "frames \(healthPacketCaptureFrameCount) | motion \(motion) | K11 \(rawStream) | K20 \(rawResearch) | K2 \(realtimeStatus) | HR \(heartRate) | K47 \(k47History) | metadata \(metadataEvents) | activity \(activityDetectionStatus) | unknown \(unresolved)"
    }
  }

  nonisolated static func extractHeartRate(from parsed: [String: Any]) -> Int? {
    guard
      let payload = parsed["parsed_payload"] as? [String: Any],
      payload["kind"] as? String == "data_packet",
      let body = payload["body_summary"] as? [String: Any],
      body["kind"] as? String == "raw_motion_k10"
    else {
      return nil
    }
    return intValue(body["heart_rate"])
  }

  nonisolated static func extractMovementPacket(
    from parsed: [String: Any],
    compact: NotificationFrameCompactSummary?,
    capturedAt: Date,
    fallbackHeartRate: Int?
  ) -> MovementPacketSample? {
    if let compact,
       let sample = MovementPacketSample.fromCompactSummary(
        compact,
        capturedAt: capturedAt,
        fallbackHeartRate: fallbackHeartRate
       ) {
      return sample
    }
    return MovementPacketSample.fromParsedFrame(
      parsed,
      capturedAt: capturedAt,
      fallbackHeartRate: fallbackHeartRate
    )
  }

  nonisolated static func extractWhoopEvent(from parsed: [String: Any], capturedAt: Date) -> WhoopEventSample? {
    WhoopEventSample.fromParsedFrame(parsed, capturedAt: capturedAt)
  }

  nonisolated static func extractWhoopEvent(from compact: NotificationFrameCompactSummary?, capturedAt: Date) -> WhoopEventSample? {
    compact.flatMap { WhoopEventSample.fromCompactSummary($0, capturedAt: capturedAt) }
  }

  nonisolated static func extractWhoopDataSignal(from parsed: [String: Any], capturedAt: Date) -> WhoopDataSignalSample? {
    WhoopDataSignalSample.fromParsedFrame(parsed, capturedAt: capturedAt)
  }

  nonisolated static func extractWhoopDataSignal(from compact: NotificationFrameCompactSummary?, capturedAt: Date) -> WhoopDataSignalSample? {
    compact.flatMap { WhoopDataSignalSample.fromCompactSummary($0, capturedAt: capturedAt) }
  }

  func recentLiveHeartRate(around date: Date) -> Int? {
    guard
      let heartRate = ble.liveHeartRateBPM,
      let updatedAt = ble.liveHeartRateUpdatedAt,
      abs(date.timeIntervalSince(updatedAt)) <= 12
    else {
      return nil
    }
    return heartRate
  }

  func handleMovementPacket(_ sample: MovementPacketSample) {
    updateMovementPacketValidation(with: sample)
    let intensityPercent = Int((sample.motionIntensity * 100).rounded())
    let movingText = sample.isMoving ? "moving" : "quiet"
    passiveActivityPacketCount += 1
    let packetCount = passiveActivityPacketCount
    recordDeviceSignalPoint(
      family: "Motion",
      value: "\(movingText) \(intensityPercent)%",
      detail: "K\(sample.packetK.map(String.init) ?? "?") axes=\(sample.axisCount) samples=\(sample.parsedSampleCount) \(sample.heartRateBPM.map { "\($0)bpm" } ?? "hr=?")",
      capturedAt: sample.capturedAt
    )
    if shouldUpdateMovementPacketStatus(at: sample.capturedAt) {
      publishMovementPacketStatus("\(packetCount) packets | \(sample.bodySummaryKind) \(movingText) \(intensityPercent)%")
    }
    if var persistence = activeActivityPersistence {
      persistence.ingest(sample)
      activeActivityPersistence = persistence
    }
    if shouldLogMovementPacket(sample) {
      ble.record(
        level: .debug,
        source: "activity.detect",
        title: "movement.packet",
        body: sample.logSummary(packetCount: packetCount)
      )
    }
    guard activeHealthPacketCapture == nil || activeHealthPacketCapture?.mode == .walk else {
      return
    }

    passiveActivityDetectionPipeline.ingest(
      sample,
      manualActivityActive: activitySession.isActive,
      currentPaceSecondsPerKilometer: activityLocationTracker.currentPaceSecondsPerKilometer,
      distanceMeters: activityLocationTracker.distanceMeters
    )
    schedulePassiveActivityIdleCheck()
  }

  func handleWhoopEvent(_ sample: WhoopEventSample) {
    publishWhoopEventStatus(sample.statusSummary, at: sample.capturedAt)
    if shouldLogWhoopEvent(sample) {
      ble.record(level: .debug, source: "whoop.event", title: "event.received", body: sample.logSummary)
    }

    guard sample.isTemperatureLevelEvent else {
      return
    }

    publishSkinTemperatureCandidateStatus(sample.temperatureCandidateSummary)
    recordDeviceSignalPoint(
      family: "Skin Temp",
      value: sample.primaryTemperatureCandidate?.summary ?? "candidate unresolved",
      detail: "event \(sample.eventName) body=\(sample.dataByteCount) bytes",
      capturedAt: sample.capturedAt,
      minimumInterval: 1
    )
    ble.record(
      source: "whoop.event",
      title: "temperature.skin_candidate",
      body: sample.logSummary
    )
  }

  func handleWhoopDataSignal(_ sample: WhoopDataSignalSample) {
    observeRespiratoryPacketWatch(sample)
    whoopDataSignalPipeline.ingest(sample)
  }

  func observeRespiratoryPacketWatch(_ sample: WhoopDataSignalSample) {
    guard respiratoryPacketWatchActive, sample.packetK == 18 || sample.packetK == 24 else {
      return
    }

    if sample.packetK == 18 {
      respiratoryPacketWatchK18Count += 1
    } else {
      respiratoryPacketWatchK24Count += 1
    }

    let counts = "K18 \(respiratoryPacketWatchK18Count) | K24 \(respiratoryPacketWatchK24Count)"
    if sample.packetK == 24 {
      respiratoryPacketWatchStatus = "Saw K24; still waiting for K18 respiratory history | \(counts)"
      ble.record(level: .debug, source: "respiratory.packet_watch", title: "related_history_packet", body: sample.logSummary)
      return
    }

    respiratoryPacketWatchTimeoutWorkItem?.cancel()
    respiratoryPacketWatchTimeoutWorkItem = nil
    respiratoryPacketWatchActive = false
    if let respiratoryRate = sample.historyRespiratoryRate {
      let rateText = respiratoryRate.respiratoryRateRPM.map { String(format: "%.1f rpm candidate", $0) } ?? "candidate unresolved"
      respiratoryPacketWatchStatus = "Found K18 \(rateText) | \(counts)"
    } else {
      respiratoryPacketWatchStatus = "Found K18; respiratory candidate unavailable | \(counts)"
    }
    ble.record(source: "respiratory.packet_watch", title: "matched.k18", body: sample.logSummary)
  }

  func shouldUpdateMovementPacketStatus(at date: Date) -> Bool {
    guard date.timeIntervalSince(lastMovementPacketStatusUpdatedAt) >= Self.movementPacketStatusInterval else {
      return false
    }
    lastMovementPacketStatusUpdatedAt = date
    return true
  }

  func applyPacketUIStateSnapshot(_ snapshot: PacketUIStateSnapshot) {
    packetMonitor.apply(
      snapshot,
      maxRecentDeviceSignalPoints: Self.maxRecentDeviceSignalPoints,
      publishInterval: Self.packetUIStatePublishInterval
    )
    if !snapshot.deviceSignalCountsByFamily.isEmpty {
      deviceSignalCountsByFamily = snapshot.deviceSignalCountsByFamily
    }
  }

  func publishParsedFrameSummary(_ summary: String, at date: Date) {
    guard date.timeIntervalSince(lastParsedFrameSummaryUpdatedAt) >= Self.parsedFrameSummaryUpdateInterval else {
      return
    }
    lastParsedFrameSummaryUpdatedAt = date
    packetUIStateAggregator.set(.lastParsedFrameSummary, summary)
  }

  func publishWhoopEventStatus(_ status: String, at date: Date) {
    guard date.timeIntervalSince(lastWhoopEventStatusUpdatedAt) >= Self.whoopEventStatusInterval else {
      return
    }
    lastWhoopEventStatusUpdatedAt = date
    packetUIStateAggregator.set(.whoopEventStatus, status)
  }

  func publishMovementPacketStatus(_ status: String) {
    packetUIStateAggregator.set(.movementPacketStatus, status)
  }

  func publishSkinTemperatureCandidateStatus(_ status: String) {
    packetUIStateAggregator.set(.skinTemperatureCandidateStatus, status)
  }

  func publishHistoryTemperatureCandidateStatus(_ status: String) {
    packetUIStateAggregator.set(.historyTemperatureCandidateStatus, status)
  }

  func publishRespiratoryRateCandidateStatus(_ status: String) {
    packetUIStateAggregator.set(.respiratoryRateCandidateStatus, status)
  }

  func publishPulseInformationPacketStatus(_ status: String) {
    packetUIStateAggregator.set(.pulseInformationPacketStatus, status)
  }

  func publishOpticalPacketStatus(_ status: String) {
    packetUIStateAggregator.set(.opticalPacketStatus, status)
  }

  func publishRawResearchPacketStatus(_ status: String) {
    packetUIStateAggregator.set(.rawResearchPacketStatus, status)
  }

  func publishRealtimeStatusPacketStatus(_ status: String) {
    packetUIStateAggregator.set(.realtimeStatusPacketStatus, status)
  }

  func publishPipelinePerformanceStatus(_ status: String) {
    packetUIStateAggregator.set(.performancePipelineStatus, status)
  }

  func recordRustBridgeTiming(
    _ timing: GooseRustBridgeTiming,
    frameCount: Int,
    queueDepth: Int,
    queueHighWatermark: Int,
    detail: String? = nil
  ) {
    let elapsedMS = Double(timing.methodElapsedMicroseconds) / 1_000
    let boundaryMS = Double(timing.boundaryMicroseconds) / 1_000
    let encodeMS = Double(timing.requestEncodeMicroseconds) / 1_000
    let decodeMS = Double(timing.responseDecodeMicroseconds) / 1_000
    var status = String(
      format: "rust %@ %.1fms | bridge %.1fms e%.1f/d%.1f | frames %d | parseQ %d hwm %d",
      timing.method,
      elapsedMS,
      boundaryMS,
      encodeMS,
      decodeMS,
      frameCount,
      queueDepth,
      queueHighWatermark
    )
    if let detail, !detail.isEmpty {
      status += " | \(detail)"
    }
    publishPipelinePerformanceStatus(status)

    let now = Date()
    pipelinePerformanceLogLock.lock()
    let shouldLog = now.timeIntervalSince(lastPipelinePerformanceLoggedAt) >= Self.pipelinePerformanceLogInterval
    if shouldLog {
      lastPipelinePerformanceLoggedAt = now
    }
    pipelinePerformanceLogLock.unlock()
    guard shouldLog else {
      return
    }
    ble.record(
      level: elapsedMS >= 50 || queueDepth > 4 ? .warn : .debug,
      source: "performance.pipeline",
      title: "rust.bridge.timing",
      body: status
    )
  }

  func recordDeviceSignalPoint(
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
      minimumInterval: minimumInterval ?? Self.deviceSignalPointInterval
    )
  }

  func shouldLogMovementPacket(_ sample: MovementPacketSample) -> Bool {
    movementPacketLogCount += 1
    let movementChanged = lastMovementPacketLoggedMoving != sample.isMoving
    let intervalElapsed = sample.capturedAt.timeIntervalSince(lastMovementPacketLoggedAt) >= Self.movementPacketLogInterval
    guard movementPacketLogCount <= 3 || movementChanged || intervalElapsed || movementPacketValidationIsRunning else {
      return false
    }
    lastMovementPacketLoggedAt = sample.capturedAt
    lastMovementPacketLoggedMoving = sample.isMoving
    return true
  }

  func shouldLogWhoopEvent(_ sample: WhoopEventSample) -> Bool {
    if sample.isTemperatureLevelEvent {
      lastWhoopEventLoggedAt = sample.capturedAt
      return true
    }
    guard sample.capturedAt.timeIntervalSince(lastWhoopEventLoggedAt) >= Self.whoopDataSignalLogInterval else {
      return false
    }
    lastWhoopEventLoggedAt = sample.capturedAt
    return true
  }

  func updateMovementPacketValidation(with sample: MovementPacketSample) {
    guard movementPacketValidationIsRunning else {
      return
    }

    movementPacketValidation.ingest(sample)
    let newValidationStatus = movementPacketValidation.statusSummary
    if newValidationStatus != movementPacketValidationStatus {
      movementPacketValidationStatus = newValidationStatus
    }
    ble.record(
      level: .debug,
      source: "activity.detect",
      title: "movement_packet_test.packet",
      body: movementPacketValidation.logSummary
    )

    guard sample.isMoving else {
      return
    }

    movementPacketValidationIsRunning = false
    movementPacketValidationTimeoutWorkItem?.cancel()
    let intensityPercent = Int((sample.motionIntensity * 100).rounded())
    let hrText = sample.heartRateBPM.map { ", HR \($0)" } ?? ""
    movementPacketValidationStatus = "Passed: real \(sample.bodySummaryKind) moving packet, intensity \(intensityPercent)%\(hrText)"
    ble.record(
      source: "activity.detect",
      title: "movement_packet_test.pass",
      body: movementPacketValidation.logSummary
    )
  }

  func finishMovementPacketValidationTimedOut() {
    guard movementPacketValidationIsRunning else {
      return
    }
    movementPacketValidationIsRunning = false
    movementPacketValidationStatus = movementPacketValidation.timeoutSummary
    ble.record(level: .warn, source: "activity.detect", title: "movement_packet_test.timeout", body: movementPacketValidation.logSummary)
  }

  func applyActivityDetectionEvents(_ events: [PassiveActivityDetectionEvent]) {
    for event in events {
      switch event {
      case .status(let status):
        // Guard prevents @Published objectWillChange from firing when the status string
        // repeats unchanged across consecutive movement packets.
        guard status != activityDetectionStatus else { break }
        activityDetectionStatus = status
      case .primeGPS(let reason):
        activityDetectionStatus = "Movement detected; priming GPS"
        if !activitySession.isActive {
          activityLocationTracker.start(reset: true)
        }
        ble.record(source: "activity.detect", title: "gps.prime", body: reason)
      case .started(let recording):
        guard !activitySession.isActive else {
          ble.record(
            level: .debug,
            source: "activity.detect",
            title: "candidate.start.skipped_manual_active",
            body: recording.activity.title
          )
          break
        }
        activityDetectionStatus = "Candidate \(recording.activity.title) recording"
        beginActivityRecording(
          activity: recording.activity,
          startedAt: recording.startedAt,
          source: "ios.auto_activity_detection",
          detectionMethod: "heuristic_hr_motion",
          syncStatus: "candidate"
        )
        if recording.activity.usesGPS && !activitySession.isActive {
          activityLocationTracker.start(reset: activityLocationTracker.routePointCount == 0)
        }
        ble.record(
          source: "activity.detect",
          title: "candidate.start",
          body: "\(recording.activity.title) packets=\(recording.packetCount) intensity=\(String(format: "%.3f", recording.meanMotionIntensity))"
        )
      case .finished(let summary, let reason):
        guard !activitySession.isActive || activeActivityPersistence?.detectionMethod != "user_assigned" else {
          ble.record(
            level: .debug,
            source: "activity.detect",
            title: "candidate.finish.skipped_manual_active",
            body: "\(summary.activity.title) reason=\(reason)"
          )
          break
        }
        activityDetectionStatus = "Candidate \(summary.activity.title) stored"
        finishActivityRecording(
          activity: summary.activity,
          startedAt: summary.startedAt,
          endedAt: summary.endedAt,
          elapsed: summary.elapsed,
          averageHeartRate: summary.averageHeartRate,
          maxHeartRate: summary.maxHeartRate,
          zoneDurations: summary.zoneDurations,
          distanceMeters: activityLocationTracker.distanceMeters,
          elevationGainMeters: activityLocationTracker.elevationGainMeters,
          routePointCount: activityLocationTracker.routePointCount,
          source: "ios.auto_activity_detection",
          detectionMethod: "heuristic_hr_motion",
          syncStatus: "candidate",
          confidence: summary.confidence,
          extraProvenance: [
            "auto_detection_reason": reason,
            "movement_packet_count": summary.packetCount,
            "mean_motion_intensity_0_to_1": summary.meanMotionIntensity,
            "peak_motion_intensity_0_to_1": summary.peakMotionIntensity,
            "movement_source": "whoop.ble.raw_motion_k10",
          ]
        )
        if summary.activity.usesGPS && !activitySession.isActive {
          activityLocationTracker.stop()
        }
        Task {
          await NotificationScheduler.shared.scheduleWorkoutDetected(
            activity: summary.activity.title,
            durationSeconds: summary.elapsed,
            strain: nil
          )
        }
        ble.record(
          source: "activity.detect",
          title: "candidate.finish",
          body: "\(summary.activity.title) reason=\(reason) duration=\(Int(summary.elapsed.rounded()))s packets=\(summary.packetCount)"
        )
      case .stopGPS(let reason):
        if !activitySession.isActive {
          activityLocationTracker.stop()
        }
        ble.record(source: "activity.detect", title: "gps.stop", body: reason)
      }
    }
  }

  func schedulePassiveActivityIdleCheck() {
    activityDetectionIdleWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.finishAutoDetectedActivityIfIdle(now: Date())
      }
    }
    activityDetectionIdleWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 125, execute: workItem)
  }

  func finishAutoDetectedActivityIfIdle(now: Date) {
    passiveActivityDetectionPipeline.finishIfIdle(
      now: now,
      distanceMeters: activityLocationTracker.distanceMeters
    )
  }

  func finishAutoDetectedActivityIfActive(endedAt: Date, reason: String) {
    passiveActivityDetectionPipeline.forceFinish(
      now: endedAt,
      reason: reason,
      distanceMeters: activityLocationTracker.distanceMeters
    )
  }

}
