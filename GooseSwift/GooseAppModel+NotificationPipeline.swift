import Foundation
import UIKit


extension GooseAppModel {
  func handleNotification(_ event: GooseNotificationEvent) {
    let (queueDepth, highWatermark) = incrementNotificationIngestQueueDepth()
    let captureImportActive = activeHealthPacketCapture != nil || activeActivityPersistence != nil
    let parseContext = notificationParseContext(for: event)
    publishPipelinePerformanceStatus(
      "ingest queued notification bytes=\(event.value.count) | ingestQ \(queueDepth) hwm \(highWatermark)"
    )

    notificationIngestQueue.async { [weak self] in
      guard let self else {
        return
      }
      let result = self.notificationIngestResult(for: event)
      guard !result.frames.isEmpty else {
        DispatchQueue.main.async { [weak self] in
          self?.handleEmptyNotificationIngestResult(result)
        }
        return
      }
      guard captureImportActive else {
        DispatchQueue.main.async { [weak self] in
          self?.handleNotificationIngestResultWithoutCapture(result, parseContext: parseContext)
        }
        return
      }
      DispatchQueue.main.async { [weak self] in
        self?.handleNotificationIngestResult(result)
      }
    }
  }

  func handleNotificationIngestResult(_ result: NotificationIngestResult) {
    let (queueDepth, highWatermark) = decrementNotificationIngestQueueDepth()
    publishPipelinePerformanceStatus(
      "ingest processed \(result.frames.count) frame\(result.frames.count == 1 ? "" : "s") | ingestQ \(queueDepth) hwm \(highWatermark)"
    )

    let event = result.event
    if result.droppedBytes > 0 {
      ble.record(
        level: .warn,
        source: "rust",
        title: "notification.frame.reassembly.dropped",
        body: "\(event.characteristicUUID) dropped=\(result.droppedBytes) buffered=\(result.bufferedBytes)"
      )
    }
    if result.usedBufferedData && !result.frames.isEmpty {
      ble.record(
        source: "rust",
        title: "notification.frame.reassembled",
        body: "\(event.characteristicUUID) frames=\(result.frames.count) remaining=\(result.bufferedBytes)"
      )
    }

    let frames = result.frames
    guard !frames.isEmpty else {
      if result.bufferedBytes > 0 {
        let expected = result.expectedBytes.map(String.init) ?? "?"
        ble.record(
          level: .debug,
          source: "rust",
          title: "notification.frame.reassembly.buffered",
          body: "\(event.characteristicUUID) buffered=\(result.bufferedBytes)/\(expected) notification=\(event.value.count)"
        )
        return
      }
      if result.droppedBytes > 0 {
        return
      }
      let diagnostic = skippedNotificationDiagnostics.record(event)
      ble.record(
        level: .debug,
        source: "rust",
        title: "notification.parser.skipped",
        body: diagnostic.message
      )
      if let rollup = diagnostic.rollup {
        ble.record(source: "rust", title: "notification.parser.skipped.summary", body: rollup)
      }
      return
    }

    importCapturedFrames(frames, event: event)

    parseNotificationFrames(frames, event: event)
  }

  func handleNotificationIngestResultWithoutCapture(
    _ result: NotificationIngestResult,
    parseContext: NotificationParseContext
  ) {
    let (queueDepth, highWatermark) = decrementNotificationIngestQueueDepth()
    publishPipelinePerformanceStatus(
      "ingest processed \(result.frames.count) frame\(result.frames.count == 1 ? "" : "s") main=false | ingestQ \(queueDepth) hwm \(highWatermark)"
    )

    let event = result.event
    if result.droppedBytes > 0 {
      ble.record(
        level: .warn,
        source: "rust",
        title: "notification.frame.reassembly.dropped",
        body: "\(event.characteristicUUID) dropped=\(result.droppedBytes) buffered=\(result.bufferedBytes)"
      )
    }
    if result.usedBufferedData && !result.frames.isEmpty {
      ble.record(
        source: "rust",
        title: "notification.frame.reassembled",
        body: "\(event.characteristicUUID) frames=\(result.frames.count) remaining=\(result.bufferedBytes) main=false"
      )
    }

    let frames = result.frames
    guard !frames.isEmpty else {
      return
    }
    parseNotificationFrames(frames, event: event, context: parseContext)
  }

  func handleEmptyNotificationIngestResult(_ result: NotificationIngestResult) {
    let (queueDepth, highWatermark) = decrementNotificationIngestQueueDepth()
    publishPipelinePerformanceStatus(
      "ingest processed 0 frames | ingestQ \(queueDepth) hwm \(highWatermark)"
    )

    let event = result.event
    if result.droppedBytes > 0 {
      ble.record(
        level: .warn,
        source: "rust",
        title: "notification.frame.reassembly.dropped",
        body: "\(event.characteristicUUID) dropped=\(result.droppedBytes) buffered=\(result.bufferedBytes)"
      )
      if result.bufferedBytes == 0 {
        return
      }
    }

    if result.bufferedBytes > 0 {
      let expected = result.expectedBytes.map(String.init) ?? "?"
      ble.record(
        level: .debug,
        source: "rust",
        title: "notification.frame.reassembly.buffered",
        body: "\(event.characteristicUUID) buffered=\(result.bufferedBytes)/\(expected) notification=\(event.value.count)"
      )
      return
    }

    let diagnostic = skippedNotificationDiagnostics.record(event)
    ble.record(
      level: .debug,
      source: "rust",
      title: "notification.parser.skipped",
      body: diagnostic.message
    )
    if let rollup = diagnostic.rollup {
      ble.record(source: "rust", title: "notification.parser.skipped.summary", body: rollup)
    }
  }

  func importCapturedFrames(_ frames: [NotificationFrame], event: GooseNotificationEvent) {
    lastNotificationEvent = event
    guard activeHealthPacketCapture != nil || activeActivityPersistence != nil else {
      return
    }

    let framesToWrite = frames.filter { _ in shouldWriteCapturedFrame(at: event.capturedAt) }
    guard !framesToWrite.isEmpty else {
      return
    }

    let capturedAt = Self.captureTimestampFormatter.string(from: event.capturedAt)
    let captureSessionID = activeHealthPacketCapture?.sessionID ?? activeActivityPersistence?.captureSessionID
    let request = CaptureFrameRowBuildRequest(
      frames: framesToWrite,
      event: event,
      capturedAt: capturedAt,
      captureSessionID: captureSessionID,
      deviceModel: ble.activeDeviceName
    )
    let rowBuildQueue = incrementCaptureFrameRowBuildQueueDepth()
    publishPipelinePerformanceStatus(
      "db rowbuild queued \(framesToWrite.count) frame\(framesToWrite.count == 1 ? "" : "s")"
        + " | rowQ \(rowBuildQueue.depth) hwm \(rowBuildQueue.highWatermark)"
    )

    let aggregator = captureFrameEnqueueAggregator
    captureFrameRowBuildQueue.async { [weak self] in
      guard let self else {
        return
      }
      let frameRows = Self.captureFrameRows(for: request)
      let enqueueResult = self.captureFrameWriteQueue.enqueue(rows: frameRows) { [weak self] result in
        self?.handleCaptureFrameWriteResult(result)
      }
      let rowBuildQueue = self.decrementCaptureFrameRowBuildQueueDepth()
      aggregator.record(
        enqueueResult,
        capturedAt: event.capturedAt,
        rowQueueDepth: rowBuildQueue.depth,
        rowQueueHighWatermark: rowBuildQueue.highWatermark
      )
    }
  }

  func applyCaptureFrameEnqueueSnapshot(_ snapshot: CaptureFrameEnqueueSnapshot) {
    let queuePressure = "\(snapshot.queuedRowCount)/\(snapshot.maxQueuedRows)"
    let batchPrefix = snapshot.batchCount > 1 ? "\(snapshot.batchCount) batches | " : ""
    publishPipelinePerformanceStatus(
      "\(batchPrefix)db queued \(snapshot.acceptedFrameCount) frame\(snapshot.acceptedFrameCount == 1 ? "" : "s")"
        + " dropped \(snapshot.droppedFrameCount)"
        + " | dbQ \(queuePressure)"
        + " | rowQ \(snapshot.rowQueueDepth) hwm \(snapshot.rowQueueHighWatermark)"
    )
    guard snapshot.acceptedFrameCount > 0 else {
      if snapshot.droppedFrameCount > 0 {
        pendingPacketImportStatus = "Capture DB queue full; dropped \(snapshot.droppedFrameCount) | queued \(queuePressure)"
      }
      return
    }

    if var capture = activeHealthPacketCapture {
      capture.importedFrameCount += snapshot.acceptedFrameCount
      activeHealthPacketCapture = capture
      if healthPacketCaptureFrameCount == 0 {
        healthPacketCaptureFrameCount = capture.importedFrameCount
      }
      healthPacketCaptureStreamRetryWorkItem?.cancel()
      scheduleHealthPacketCaptureUIUpdate()
    }
    if var persistence = activeActivityPersistence {
      persistence.recordImportedFrames(snapshot.acceptedFrameCount, at: snapshot.latestCapturedAt)
      activeActivityPersistence = persistence
    }
    if snapshot.droppedFrameCount > 0 {
      pendingPacketImportStatus = "Queued \(snapshot.acceptedFrameCount), dropped \(snapshot.droppedFrameCount) | depth \(queuePressure)"
      ble.record(
        level: .warn,
        source: "rust",
        title: "capture.import.queue_dropped",
        body: "batches=\(snapshot.batchCount) accepted=\(snapshot.acceptedFrameCount) dropped=\(snapshot.droppedFrameCount) queued=\(queuePressure)"
      )
    } else if snapshot.queueFillRatio >= 0.8 {
      pendingPacketImportStatus = "Capture DB queue pressure | depth \(queuePressure)"
    }
  }

  func flushCaptureFrameEnqueueUpdates() {
    guard let snapshot = captureFrameEnqueueAggregator.flushPendingSnapshot() else {
      return
    }
    applyCaptureFrameEnqueueSnapshot(snapshot)
  }

  func handleCaptureFrameWriteResult(_ result: CaptureFrameWriteResult) {
    if let bridgeTiming = result.bridgeTiming {
      let parseQueue = notificationParseQueueSnapshot()
      recordRustBridgeTiming(
        bridgeTiming,
        frameCount: result.frameCount,
        queueDepth: parseQueue.depth,
        queueHighWatermark: parseQueue.highWatermark
      )
    }
    if let errorDescription = result.errorDescription {
      packetImportStatus = "Packet import failed"
      ble.record(
        level: .error,
        source: "rust",
        title: "capture.import.failed",
        body: errorDescription
      )
      return
    }

    let timingSuffix = result.importTimingSummary.map { " | \($0)" } ?? ""
    let batchPrefix = result.batchCount > 1 ? "\(result.batchCount) batches | " : ""
    pendingPacketImportStatus = "\(batchPrefix)Imported raw \(result.rawInserted), decoded \(result.inserted), existing \(result.existing)\(timingSuffix)"
    schedulePacketImportRevisionPublish()
    if !result.pass || !result.issues.isEmpty {
      let issueSummary = result.issues.prefix(3).joined(separator: " | ")
      let nextActionSummary = result.nextActions.prefix(2).joined(separator: " | ")
      ble.record(
        level: .warn,
        source: "rust",
        title: "capture.import.issues",
        body: "batches=\(result.batchCount) raw=\(result.rawInserted)/\(result.rawExisting) decoded=\(result.inserted)/\(result.existing) queued=\(result.frameCount) issues=\(issueSummary) next=\(nextActionSummary)"
      )
    }
    ble.record(
      level: .debug,
      source: "rust",
      title: "capture.import.ok",
      body: "batches \(result.batchCount) | raw \(result.rawInserted) inserted, \(result.rawExisting) existing | decoded \(result.inserted) inserted, \(result.existing) existing | \(result.frameCount) queued\(timingSuffix)"
    )
    if result.pass, result.errorDescription == nil,
       let event = lastNotificationEvent {
      triggerUpload(for: result, deviceEvent: event)
    }
  }

  func shouldWriteCapturedFrame(at capturedAt: Date) -> Bool {
    let fullRateCaptureActive = activeActivityPersistence != nil || activitySession.isActive
    guard !fullRateCaptureActive, activeHealthPacketCapture?.mode == .walk else {
      return true
    }

    guard capturedAt.timeIntervalSince(lastRestingHeartRateFrameWriteAt) >= Self.restingHeartRateFrameWriteInterval else {
      return false
    }
    lastRestingHeartRateFrameWriteAt = capturedAt
    return true
  }

  nonisolated static func captureEvidenceID(for frame: NotificationFrame, event: GooseNotificationEvent, index: Int) -> String {
    let milliseconds = Int((event.capturedAt.timeIntervalSince1970 * 1000).rounded())
    let prefix = String(frame.hex.prefix(16))
    return "ios.\(event.deviceID.uuidString).\(milliseconds).\(index).\(prefix)"
  }

  func schedulePacketImportRevisionPublish() {
    let now = Date()
    let elapsed = now.timeIntervalSince(lastPacketImportRevisionPublishedAt)
    guard elapsed < Self.packetImportRevisionInterval else {
      publishPacketImportRevision(now: now)
      return
    }
    guard packetImportRevisionWorkItem == nil else {
      return
    }

    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.publishPacketImportRevision()
      }
    }
    packetImportRevisionWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + (Self.packetImportRevisionInterval - elapsed), execute: workItem)
  }

  func publishPacketImportRevision(now: Date = Date()) {
    packetImportRevisionWorkItem?.cancel()
    packetImportRevisionWorkItem = nil
    lastPacketImportRevisionPublishedAt = now
    if let pendingPacketImportStatus {
      packetImportStatus = pendingPacketImportStatus
      self.pendingPacketImportStatus = nil
    }
    packetImportRevision += 1
  }

  func parseNotificationFrames(_ frames: [NotificationFrame], event: GooseNotificationEvent) {
    parseNotificationFrames(
      frames,
      event: event,
      context: notificationParseContext(for: event)
    )
  }

  func parseNotificationFrames(
    _ frames: [NotificationFrame],
    event: GooseNotificationEvent,
    context: NotificationParseContext
  ) {
    let parser = notificationFrameParser
    let deviceType = context.deviceType
    let healthCaptureActive = context.healthCaptureActive
    let overnightGuardIsActive = context.overnightGuardActive
    let respiratoryPacketWatchIsActive = context.respiratoryPacketWatchActive
    let fallbackHeartRate = context.fallbackHeartRate
    let ble = context.ble
    let packetUIStateAggregator = context.packetUIStateAggregator
    let whoopDataSignalPipeline = context.whoopDataSignalPipeline
    let (queueDepth, highWatermark) = incrementNotificationParseQueueDepth()
    publishPipelinePerformanceStatus(
      "parse queued \(frames.count) frame\(frames.count == 1 ? "" : "s") | parseQ \(queueDepth) hwm \(highWatermark)"
    )
    notificationParseQueue.async {
      let frameHexes = frames.map(\.hex)
      let (parseResults, bridgeTiming, batchTiming) = parser.parseBatch(frameHexes: frameHexes, deviceType: deviceType)
      var mainResults: [ParsedNotificationFrameResult] = []
      var offMainDataSignalCount = 0
      var skippedDiagnosticFrameCount = 0
      var skippedParseErrorCount = 0
      for result in parseResults {
        let interpretation = Self.interpretNotificationFrame(
          result,
          event: event,
          healthCaptureActive: healthCaptureActive,
          fallbackHeartRate: fallbackHeartRate
        )
        let parsedResult = ParsedNotificationFrameResult(
          interpretation: interpretation,
          event: event,
          bridgeTiming: bridgeTiming
        )
        if let dataSignal = interpretation.dataSignal,
           Self.canHandleDataSignalOffMain(
            interpretation,
            overnightGuardActive: overnightGuardIsActive,
            respiratoryPacketWatchActive: respiratoryPacketWatchIsActive
           ) {
          Self.recordSkippedParsedFrameMainHandling(
            parsedResult,
            ble: ble,
            packetUIStateAggregator: packetUIStateAggregator
          )
          whoopDataSignalPipeline.ingest(dataSignal)
          offMainDataSignalCount += 1
          continue
        }
        guard Self.requiresMainParsedFrameHandling(interpretation, overnightGuardActive: overnightGuardIsActive) else {
          Self.recordSkippedParsedFrameMainHandling(
            parsedResult,
            ble: ble,
            packetUIStateAggregator: packetUIStateAggregator
          )
          if interpretation.parseErrorDescription == nil {
            skippedDiagnosticFrameCount += 1
          } else {
            skippedParseErrorCount += 1
          }
          continue
        }
        mainResults.append(parsedResult)
      }
      let dispatch = ParsedNotificationFrameDispatch(
        mainResults: mainResults,
        totalFrameCount: parseResults.count,
        offMainDataSignalCount: offMainDataSignalCount,
        skippedDiagnosticFrameCount: skippedDiagnosticFrameCount,
        skippedParseErrorCount: skippedParseErrorCount,
        bridgeTiming: bridgeTiming,
        batchTiming: batchTiming
      )
      guard !mainResults.isEmpty else {
        DispatchQueue.main.async { [weak self] in
          self?.handleParsedNotificationFramesWithoutMain(dispatch)
        }
        return
      }
      DispatchQueue.main.async { [weak self] in
        self?.handleParsedNotificationFrames(dispatch)
      }
    }
  }

  func handleParsedNotificationFrames(_ dispatch: ParsedNotificationFrameDispatch) {
    let (queueDepth, highWatermark) = decrementNotificationParseQueueDepth()
    if let timing = dispatch.bridgeTiming {
      recordRustBridgeTiming(
        timing,
        frameCount: dispatch.totalFrameCount,
        queueDepth: queueDepth,
        queueHighWatermark: highWatermark,
        detail: dispatch.batchTiming?.statusSummary
      )
    } else {
      publishPipelinePerformanceStatus(
        "parse completed \(dispatch.totalFrameCount) frame\(dispatch.totalFrameCount == 1 ? "" : "s") | parseQ \(queueDepth) hwm \(highWatermark)"
      )
    }
    if dispatch.skippedDiagnosticFrameCount > 0 || dispatch.skippedParseErrorCount > 0 {
      publishPipelinePerformanceStatus(
        "parse main skipped diagnostic=\(dispatch.skippedDiagnosticFrameCount) parse_error=\(dispatch.skippedParseErrorCount) | handled=\(dispatch.mainResults.count)/\(dispatch.totalFrameCount)"
      )
    }
    if dispatch.offMainDataSignalCount > 0 {
      publishPipelinePerformanceStatus(
        "parse off-main data_signal=\(dispatch.offMainDataSignalCount) | handled_main=\(dispatch.mainResults.count)/\(dispatch.totalFrameCount)"
      )
    }
    for result in dispatch.mainResults {
      handleParsedNotificationFrame(result.interpretation, event: result.event)
    }
  }

  func handleParsedNotificationFramesWithoutMain(_ dispatch: ParsedNotificationFrameDispatch) {
    let (queueDepth, highWatermark) = decrementNotificationParseQueueDepth()
    if let timing = dispatch.bridgeTiming {
      recordRustBridgeTiming(
        timing,
        frameCount: dispatch.totalFrameCount,
        queueDepth: queueDepth,
        queueHighWatermark: highWatermark,
        detail: dispatch.batchTiming?.statusSummary
      )
    } else {
      publishPipelinePerformanceStatus(
        "parse completed \(dispatch.totalFrameCount) frame\(dispatch.totalFrameCount == 1 ? "" : "s") main=false | parseQ \(queueDepth) hwm \(highWatermark)"
      )
    }
    if dispatch.skippedDiagnosticFrameCount > 0 || dispatch.skippedParseErrorCount > 0 {
      publishPipelinePerformanceStatus(
        "parse main skipped diagnostic=\(dispatch.skippedDiagnosticFrameCount) parse_error=\(dispatch.skippedParseErrorCount) | handled=0/\(dispatch.totalFrameCount)"
      )
    }
    if dispatch.offMainDataSignalCount > 0 {
      publishPipelinePerformanceStatus(
        "parse off-main data_signal=\(dispatch.offMainDataSignalCount) | handled_main=0/\(dispatch.totalFrameCount)"
      )
    }
  }

  func notificationParseContext(for event: GooseNotificationEvent) -> NotificationParseContext {
    NotificationParseContext(
      deviceType: event.rustDeviceType,
      healthCaptureActive: activeHealthPacketCapture != nil,
      overnightGuardActive: overnightGuardActive,
      respiratoryPacketWatchActive: respiratoryPacketWatchActive,
      fallbackHeartRate: recentLiveHeartRate(around: event.capturedAt),
      ble: ble,
      packetUIStateAggregator: packetUIStateAggregator,
      whoopDataSignalPipeline: whoopDataSignalPipeline
    )
  }

  nonisolated static func interpretNotificationFrame(
    _ result: NotificationFrameParseResult,
    event: GooseNotificationEvent,
    healthCaptureActive: Bool,
    fallbackHeartRate: Int?
  ) -> NotificationFrameInterpretation {
    guard result.parsed != nil || result.compact != nil else {
      return NotificationFrameInterpretation(
        parseErrorDescription: result.errorDescription ?? "missing parsed frame",
        summary: nil,
        packetType: nil,
        healthPacketFamily: nil,
        heartRateBPM: nil,
        movementSample: nil,
        whoopEvent: nil,
        dataSignal: nil
      )
    }

    let parsed = result.parsed
    let compact = result.compact
    return NotificationFrameInterpretation(
      parseErrorDescription: nil,
      summary: compact?.summary ?? parsed.map(frameSummary),
      packetType: compact?.packetType ?? parsed.flatMap { intValue($0["packet_type"]) },
      healthPacketFamily: healthCaptureActive
        ? compact.map { healthPacketCaptureFamily(for: $0, capturedAt: event.capturedAt) }
          ?? parsed.map { healthPacketCaptureFamily(for: $0, capturedAt: event.capturedAt) }
        : nil,
      heartRateBPM: compact?.heartRateBPM ?? parsed.flatMap(extractHeartRate),
      movementSample: extractMovementPacket(
        from: parsed ?? [:],
        compact: compact,
        capturedAt: event.capturedAt,
        fallbackHeartRate: fallbackHeartRate
      ),
      whoopEvent: extractWhoopEvent(from: compact, capturedAt: event.capturedAt)
        ?? parsed.flatMap { extractWhoopEvent(from: $0, capturedAt: event.capturedAt) },
      dataSignal: extractWhoopDataSignal(from: compact, capturedAt: event.capturedAt)
        ?? parsed.flatMap { extractWhoopDataSignal(from: $0, capturedAt: event.capturedAt) }
    )
  }

  nonisolated static func requiresMainParsedFrameHandling(
    _ interpretation: NotificationFrameInterpretation,
    overnightGuardActive: Bool
  ) -> Bool {
    if interpretation.healthPacketFamily != nil
      || interpretation.heartRateBPM != nil
      || interpretation.movementSample != nil
      || interpretation.whoopEvent != nil
      || interpretation.dataSignal != nil {
      return true
    }
    if overnightGuardActive, let packetType = interpretation.packetType {
      return packetType == 47 || packetType == 49 || packetType == 56
    }
    return false
  }

  nonisolated static func canHandleDataSignalOffMain(
    _ interpretation: NotificationFrameInterpretation,
    overnightGuardActive: Bool,
    respiratoryPacketWatchActive: Bool
  ) -> Bool {
    guard interpretation.dataSignal != nil else {
      return false
    }
    guard !overnightGuardActive, !respiratoryPacketWatchActive else {
      return false
    }
    return interpretation.healthPacketFamily == nil
      && interpretation.heartRateBPM == nil
      && interpretation.movementSample == nil
      && interpretation.whoopEvent == nil
  }

  nonisolated static func recordSkippedParsedFrameMainHandling(
    _ result: ParsedNotificationFrameResult,
    ble: GooseBLEClient,
    packetUIStateAggregator: PacketUIStateAggregator
  ) {
    let event = result.event
    let interpretation = result.interpretation
    if let parseErrorDescription = interpretation.parseErrorDescription {
      ble.record(
        level: .warn,
        source: "rust",
        title: "notification.frame.parse_failed",
        body: "\(event.characteristicUUID) \(parseErrorDescription)"
      )
      return
    }

    if let summary = interpretation.summary {
      packetUIStateAggregator.set(.lastParsedFrameSummary, summary)
      ble.record(source: "rust", title: "notification.frame.parsed", body: summary)
    }
  }

  func handleParsedNotificationFrame(_ interpretation: NotificationFrameInterpretation, event: GooseNotificationEvent) {
    guard interpretation.parseErrorDescription == nil else {
      ble.record(
        level: .warn,
        source: "rust",
        title: "notification.frame.parse_failed",
        body: "\(event.characteristicUUID) \(interpretation.parseErrorDescription ?? "unknown error")"
      )
      return
    }

    if let summary = interpretation.summary {
      publishParsedFrameSummary(summary, at: event.capturedAt)
      ble.record(source: "rust", title: "notification.frame.parsed", body: summary)
    }
    recordOvernightPacketTypeTarget(interpretation.packetType)
    if let family = interpretation.healthPacketFamily {
      recordHealthPacketCaptureFamily(family, capturedAt: event.capturedAt)
    }

    if let bpm = interpretation.heartRateBPM {
      ble.recordLiveHeartRate(bpm, source: "rust.k10", at: event.capturedAt)
      recordDeviceSignalPoint(
        family: "HR",
        value: "\(bpm) bpm",
        detail: "raw_motion_k10 embedded heart-rate byte",
        capturedAt: event.capturedAt,
        minimumInterval: 1
      )
    }
    if let sample = interpretation.movementSample {
      handleMovementPacket(sample)
    }
    if let whoopEvent = interpretation.whoopEvent {
      handleWhoopEvent(whoopEvent)
    }
    if let dataSignal = interpretation.dataSignal {
      handleWhoopDataSignal(dataSignal)
    }
  }

  struct NotificationFrame {
    let hex: String
  }

  struct CaptureFrameRowBuildRequest {
    let frames: [NotificationFrame]
    let event: GooseNotificationEvent
    let capturedAt: String
    let captureSessionID: String?
    let deviceModel: String
  }

  nonisolated static func captureFrameRows(for request: CaptureFrameRowBuildRequest) -> [CapturedFrameWriteRow] {
    request.frames.enumerated().map { index, frame in
      let evidenceID = Self.captureEvidenceID(for: frame, event: request.event, index: index)
      return CapturedFrameWriteRow(
        evidenceID: evidenceID,
        frameID: "\(evidenceID).frame.0",
        source: "ios.corebluetooth.notification",
        capturedAt: request.capturedAt,
        deviceModel: request.deviceModel,
        frameHex: frame.hex,
        sensitivity: "user-owned-capture",
        captureSessionID: request.captureSessionID,
        deviceType: request.event.rustDeviceType
      )
    }
  }

  struct NotificationIngestResult {
    let event: GooseNotificationEvent
    let frames: [NotificationFrame]
    let bufferedBytes: Int
    let expectedBytes: Int?
    let droppedBytes: Int
    let usedBufferedData: Bool
  }

  nonisolated func notificationIngestResult(for event: GooseNotificationEvent) -> NotificationIngestResult {
    // HR monitor 0x2A37 payloads are standard GATT bytes, not 0xaa-framed WHOOP frames.
    // Bypass the WHOOP reassembly path and treat the entire notification value as one frame.
    // This function is intentionally NOT @MainActor — HR notifications arrive at high frequency
    // and must stay off the main thread (review MEDIUM-3).
    if event.rustDeviceType == "HR_MONITOR" {
      let frameHex = event.value.hexString
      guard !frameHex.isEmpty else {
        return NotificationIngestResult(
          event: event,
          frames: [],
          bufferedBytes: 0,
          expectedBytes: nil,
          droppedBytes: 0,
          usedBufferedData: false
        )
      }
      return NotificationIngestResult(
        event: event,
        frames: [NotificationFrame(hex: frameHex)],
        bufferedBytes: 0,
        expectedBytes: nil,
        droppedBytes: 0,
        usedBufferedData: false
      )
    }
    let reassembly = gooseFrames(in: event.value, event: event)
    return NotificationIngestResult(
      event: event,
      frames: reassembly.frames.map { NotificationFrame(hex: $0.hexString) },
      bufferedBytes: reassembly.bufferedBytes,
      expectedBytes: reassembly.expectedBytes,
      droppedBytes: reassembly.droppedBytes,
      usedBufferedData: reassembly.usedBufferedData
    )
  }

  func incrementNotificationIngestQueueDepth() -> (depth: Int, highWatermark: Int) {
    notificationIngestStateLock.lock()
    notificationIngestQueueDepth += 1
    notificationIngestQueueHighWatermark = max(notificationIngestQueueHighWatermark, notificationIngestQueueDepth)
    let snapshot = (notificationIngestQueueDepth, notificationIngestQueueHighWatermark)
    notificationIngestStateLock.unlock()
    return snapshot
  }

  func decrementNotificationIngestQueueDepth() -> (depth: Int, highWatermark: Int) {
    notificationIngestStateLock.lock()
    notificationIngestQueueDepth = max(0, notificationIngestQueueDepth - 1)
    let snapshot = (notificationIngestQueueDepth, notificationIngestQueueHighWatermark)
    notificationIngestStateLock.unlock()
    return snapshot
  }

  func incrementNotificationParseQueueDepth() -> (depth: Int, highWatermark: Int) {
    notificationParseStateLock.lock()
    notificationParseQueueDepth += 1
    notificationParseQueueHighWatermark = max(notificationParseQueueHighWatermark, notificationParseQueueDepth)
    let snapshot = (notificationParseQueueDepth, notificationParseQueueHighWatermark)
    notificationParseStateLock.unlock()
    return snapshot
  }

  func decrementNotificationParseQueueDepth() -> (depth: Int, highWatermark: Int) {
    notificationParseStateLock.lock()
    notificationParseQueueDepth = max(0, notificationParseQueueDepth - 1)
    let snapshot = (notificationParseQueueDepth, notificationParseQueueHighWatermark)
    notificationParseStateLock.unlock()
    return snapshot
  }

  func notificationParseQueueSnapshot() -> (depth: Int, highWatermark: Int) {
    notificationParseStateLock.lock()
    let snapshot = (notificationParseQueueDepth, notificationParseQueueHighWatermark)
    notificationParseStateLock.unlock()
    return snapshot
  }

  func incrementCaptureFrameRowBuildQueueDepth() -> (depth: Int, highWatermark: Int) {
    captureFrameRowBuildStateLock.lock()
    captureFrameRowBuildQueueDepth += 1
    captureFrameRowBuildQueueHighWatermark = max(captureFrameRowBuildQueueHighWatermark, captureFrameRowBuildQueueDepth)
    let snapshot = (captureFrameRowBuildQueueDepth, captureFrameRowBuildQueueHighWatermark)
    captureFrameRowBuildStateLock.unlock()
    return snapshot
  }

  nonisolated func decrementCaptureFrameRowBuildQueueDepth() -> (depth: Int, highWatermark: Int) {
    captureFrameRowBuildStateLock.lock()
    captureFrameRowBuildQueueDepth = max(0, captureFrameRowBuildQueueDepth - 1)
    let snapshot = (captureFrameRowBuildQueueDepth, captureFrameRowBuildQueueHighWatermark)
    captureFrameRowBuildStateLock.unlock()
    return snapshot
  }


  struct FrameReassemblyResult {
    let frames: [Data]
    let bufferedBytes: Int
    let expectedBytes: Int?
    let droppedBytes: Int
    let usedBufferedData: Bool
  }

  nonisolated func gooseFrames(in data: Data, event: GooseNotificationEvent) -> FrameReassemblyResult {
    let key = frameReassemblyKey(for: event)
    let hadBufferedData = frameReassemblyBuffers[key]?.isEmpty == false
    var bytes = Array(frameReassemblyBuffers[key] ?? Data())
    bytes.append(contentsOf: data)
    var frames: [Data] = []
    var droppedBytes = 0
    var expectedBytes: Int?
    let headerLength = event.rustDeviceType == "GEN4" ? 4 : 8

    while let startIndex = bytes.firstIndex(of: 0xaa) {
      if startIndex > 0 {
        droppedBytes += startIndex
        bytes.removeFirst(startIndex)
      }
      guard bytes.count >= headerLength else {
        break
      }

      let declaredLength: Int
      if event.rustDeviceType == "GEN4" {
        declaredLength = Int(bytes[1]) | Int(bytes[2]) << 8
      } else {
        declaredLength = Int(bytes[2]) | Int(bytes[3]) << 8
      }
      guard declaredLength >= 4,
            declaredLength + headerLength <= Self.maximumBufferedFrameBytes else {
        droppedBytes += 1
        bytes.removeFirst()
        continue
      }

      let expectedLength = declaredLength + headerLength
      guard bytes.count >= expectedLength else {
        expectedBytes = expectedLength
        break
      }
      frames.append(Data(bytes[0..<expectedLength]))
      bytes.removeFirst(expectedLength)
    }

    if bytes.isEmpty {
      frameReassemblyBuffers.removeValue(forKey: key)
    } else if bytes.first == 0xaa {
      frameReassemblyBuffers[key] = Data(bytes)
    } else {
      droppedBytes += bytes.count
      frameReassemblyBuffers.removeValue(forKey: key)
    }

    return FrameReassemblyResult(
      frames: frames,
      bufferedBytes: frameReassemblyBuffers[key]?.count ?? 0,
      expectedBytes: expectedBytes,
      droppedBytes: droppedBytes,
      usedBufferedData: hadBufferedData
    )
  }

  nonisolated func frameReassemblyKey(for event: GooseNotificationEvent) -> String {
    "\(event.deviceID.uuidString)|\(event.serviceUUID)|\(event.characteristicUUID)|\(event.rustDeviceType)"
  }

  nonisolated static func frameSummary(_ parsed: [String: Any]) -> String {
    let packet = intString(parsed["packet_type"])
    let packetName = parsed["packet_type_name"] as? String ?? "unknown"
    let sequence = intString(parsed["sequence"])
    let warnings = (parsed["warnings"] as? [Any])?.count ?? 0
    guard let payload = parsed["parsed_payload"] as? [String: Any] else {
      return "packet=\(packetName)(\(packet)) seq=\(sequence) warnings=\(warnings)"
    }

    let kind = payload["kind"] as? String ?? "unknown"
    if kind == "data_packet" {
      let packetK = intString(payload["packet_k"])
      let domain = payload["domain"] as? String ?? "unknown"
      let body = (payload["body_summary"] as? [String: Any])?["kind"] as? String ?? "none"
      return "packet=\(packetName)(\(packet)) seq=\(sequence) data.k=\(packetK) domain=\(domain) body=\(body) warnings=\(warnings)"
    }

    if kind == "event" {
      let eventID = intString(payload["event_id"])
      let eventName = payload["event_name"] as? String ?? "unknown"
      let dataHex = payload["data_hex"] as? String ?? ""
      return "packet=\(packetName)(\(packet)) seq=\(sequence) event=\(eventName)(\(eventID)) bytes=\(dataHex.count / 2) warnings=\(warnings)"
    }

    return "packet=\(packetName)(\(packet)) seq=\(sequence) payload=\(kind) warnings=\(warnings)"
  }

}
