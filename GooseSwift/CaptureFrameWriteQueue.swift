import Foundation
import UIKit

struct CaptureFrameWriteEnqueueResult {
  let acceptedFrameCount: Int
  let droppedFrameCount: Int
  let queuedRowCount: Int
  let maxQueuedRows: Int

  var queueFillRatio: Double {
    guard maxQueuedRows > 0 else {
      return 0
    }
    return Double(queuedRowCount) / Double(maxQueuedRows)
  }
}

struct CaptureFrameEnqueueSnapshot {
  let batchCount: Int
  let acceptedFrameCount: Int
  let droppedFrameCount: Int
  let latestCapturedAt: Date
  let queuedRowCount: Int
  let maxQueuedRows: Int
  let rowQueueDepth: Int
  let rowQueueHighWatermark: Int

  var queueFillRatio: Double {
    guard maxQueuedRows > 0 else {
      return 0
    }
    return Double(queuedRowCount) / Double(maxQueuedRows)
  }
}

struct CaptureFrameWriteResult {
  let batchCount: Int
  let frameCount: Int
  let rawInserted: Int
  let rawExisting: Int
  let inserted: Int
  let existing: Int
  let pass: Bool
  let issues: [String]
  let nextActions: [String]
  let errorDescription: String?
  let bridgeTiming: GooseRustBridgeTiming?
  let importTimingSummary: String?
}

struct CapturedFrameWriteRow {
  let evidenceID: String
  let frameID: String
  let source: String
  let capturedAt: String
  let deviceModel: String
  let frameHex: String
  let sensitivity: String
  let captureSessionID: String?
  let deviceType: String

  var bridgeObject: [String: Any] {
    [
      "evidence_id": evidenceID,
      "frame_id": frameID,
      "source": source,
      "captured_at": capturedAt,
      "device_model": deviceModel,
      "frame_hex": frameHex,
      "sensitivity": sensitivity,
      "capture_session_id": captureSessionID ?? NSNull(),
      "device_type": deviceType,
    ]
  }
}

final class CaptureFrameEnqueueAggregator {
  var onSnapshot: ((CaptureFrameEnqueueSnapshot) -> Void)?

  private let queue = DispatchQueue(label: "com.goose.swift.capture-frame-enqueue", qos: .utility)
  private let publishInterval: TimeInterval
  private var pendingSnapshot: CaptureFrameEnqueueSnapshot?
  private var publishScheduled = false
  private var lastPublishedAt = Date.distantPast

  init(publishInterval: TimeInterval) {
    self.publishInterval = publishInterval
  }

  func record(
    _ result: CaptureFrameWriteEnqueueResult,
    capturedAt: Date,
    rowQueueDepth: Int,
    rowQueueHighWatermark: Int
  ) {
    queue.async { [weak self] in
      guard let self else {
        return
      }
      self.merge(
        result,
        capturedAt: capturedAt,
        rowQueueDepth: rowQueueDepth,
        rowQueueHighWatermark: rowQueueHighWatermark
      )
      self.schedulePublish(now: Date())
    }
  }

  func flushPendingSnapshot() -> CaptureFrameEnqueueSnapshot? {
    queue.sync {
      let snapshot = pendingSnapshot
      pendingSnapshot = nil
      publishScheduled = false
      if snapshot != nil {
        lastPublishedAt = Date()
      }
      return snapshot
    }
  }

  private func merge(
    _ result: CaptureFrameWriteEnqueueResult,
    capturedAt: Date,
    rowQueueDepth: Int,
    rowQueueHighWatermark: Int
  ) {
    if let pendingSnapshot {
      self.pendingSnapshot = CaptureFrameEnqueueSnapshot(
        batchCount: pendingSnapshot.batchCount + 1,
        acceptedFrameCount: pendingSnapshot.acceptedFrameCount + result.acceptedFrameCount,
        droppedFrameCount: pendingSnapshot.droppedFrameCount + result.droppedFrameCount,
        latestCapturedAt: maxDate(pendingSnapshot.latestCapturedAt, capturedAt),
        queuedRowCount: result.queuedRowCount,
        maxQueuedRows: result.maxQueuedRows,
        rowQueueDepth: rowQueueDepth,
        rowQueueHighWatermark: max(pendingSnapshot.rowQueueHighWatermark, rowQueueHighWatermark)
      )
    } else {
      pendingSnapshot = CaptureFrameEnqueueSnapshot(
        batchCount: 1,
        acceptedFrameCount: result.acceptedFrameCount,
        droppedFrameCount: result.droppedFrameCount,
        latestCapturedAt: capturedAt,
        queuedRowCount: result.queuedRowCount,
        maxQueuedRows: result.maxQueuedRows,
        rowQueueDepth: rowQueueDepth,
        rowQueueHighWatermark: rowQueueHighWatermark
      )
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
    guard let snapshot = pendingSnapshot else {
      return
    }
    pendingSnapshot = nil
    lastPublishedAt = now
    onSnapshot?(snapshot)
  }
}

final class CaptureFrameWriteQueue: @unchecked Sendable {
  private let writeQueue = DispatchQueue(label: "com.goose.swift.capture-frame-writes", qos: .utility)
  private let stateLock = NSLock()
  private let rust = GooseRustBridge()
  private let databasePath: String
  private let maxQueuedRows: Int
  private let maxBatchRows: Int
  private let coalesceDelay: TimeInterval = 0.05
  private let completionCoalesceDelay: TimeInterval = 1
  private var pendingRows: [CapturedFrameWriteRow] = []
  private var latestCompletion: (@MainActor (CaptureFrameWriteResult) -> Void)?
  private var pendingCompletionResult: CaptureFrameWriteResult?
  private var pendingCompletion: (@MainActor (CaptureFrameWriteResult) -> Void)?
  private var completionFlushScheduled = false
  private var queuedRowCount = 0
  private var isWriting = false

  init(databasePath: String, maxQueuedRows: Int, maxBatchRows: Int) {
    self.databasePath = databasePath
    self.maxQueuedRows = maxQueuedRows
    self.maxBatchRows = max(1, maxBatchRows)
  }

  func enqueue(
    rows: [CapturedFrameWriteRow],
    completion: @escaping @MainActor (CaptureFrameWriteResult) -> Void
  ) -> CaptureFrameWriteEnqueueResult {
    guard !rows.isEmpty else {
      stateLock.lock()
      let currentQueuedRowCount = queuedRowCount
      stateLock.unlock()
      return CaptureFrameWriteEnqueueResult(
        acceptedFrameCount: 0,
        droppedFrameCount: 0,
        queuedRowCount: currentQueuedRowCount,
        maxQueuedRows: maxQueuedRows
      )
    }

    var acceptedFrameCount = 0
    var shouldStartWriter = false
    var currentQueuedRowCount = 0
    stateLock.lock()
    defer { stateLock.unlock() }

    let capacity = max(0, maxQueuedRows - queuedRowCount)
    if capacity > 0 {
      let acceptedRows = Array(rows.prefix(capacity))
      acceptedFrameCount = acceptedRows.count
      queuedRowCount += acceptedRows.count
      pendingRows.append(contentsOf: acceptedRows)
      latestCompletion = completion

      if !isWriting {
        isWriting = true
        shouldStartWriter = true
      }
    }
    currentQueuedRowCount = queuedRowCount

    if shouldStartWriter {
      writeQueue.asyncAfter(deadline: .now() + coalesceDelay) { [weak self] in
        self?.flushNext()
      }
    }

    return CaptureFrameWriteEnqueueResult(
      acceptedFrameCount: acceptedFrameCount,
      droppedFrameCount: rows.count - acceptedFrameCount,
      queuedRowCount: currentQueuedRowCount,
      maxQueuedRows: maxQueuedRows
    )
  }

  private func flushNext() {
    while true {
      let rows: [CapturedFrameWriteRow]
      let completion: (@MainActor (CaptureFrameWriteResult) -> Void)?
      stateLock.lock()
      if pendingRows.isEmpty {
        isWriting = false
        latestCompletion = nil
        stateLock.unlock()
        return
      } else {
        let rowCount = min(maxBatchRows, pendingRows.count)
        rows = Array(pendingRows.prefix(rowCount))
        pendingRows.removeFirst(rowCount)
        queuedRowCount = max(0, queuedRowCount - rows.count)
        completion = latestCompletion
        stateLock.unlock()
      }

      let result: CaptureFrameWriteResult
      do {
        let report = try rust.request(
          method: "capture.import_frame_batch",
          args: [
            "database_path": databasePath,
            "parser_version": "goose-swift/live-notification",
            "include_timeline_rows": false,
            // Compact old raw payloads on every live write so the on-device database
            // stays bounded (see DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES)
            // instead of growing without limit during a long history sync.
            "compact_raw_payloads": true,
            "include_results": false,
            "frames": rows.map(\.bridgeObject),
          ]
        )
        result = CaptureFrameWriteResult(
          batchCount: 1,
          frameCount: rows.count,
          rawInserted: Self.intValue(report["raw_inserted"]) ?? 0,
          rawExisting: Self.intValue(report["raw_existing"]) ?? 0,
          inserted: Self.intValue(report["frames_inserted"]) ?? 0,
          existing: Self.intValue(report["frames_existing"]) ?? 0,
          pass: Self.boolValue(report["pass"]) ?? true,
          issues: Self.stringArray(report["issues"]),
          nextActions: Self.nextActionSummaries(report["next_actions"]),
          errorDescription: nil,
          bridgeTiming: rust.lastTiming,
          importTimingSummary: Self.importTimingSummary(report["timing"])
        )
      } catch {
        result = CaptureFrameWriteResult(
          batchCount: 1,
          frameCount: rows.count,
          rawInserted: 0,
          rawExisting: 0,
          inserted: 0,
          existing: 0,
          pass: false,
          issues: [],
          nextActions: [],
          errorDescription: String(describing: error),
          bridgeTiming: rust.lastTiming,
          importTimingSummary: nil
        )
      }

      if let completion {
        recordCompletion(result, completion: completion)
      }
    }
  }

  private func recordCompletion(
    _ result: CaptureFrameWriteResult,
    completion: @escaping @MainActor (CaptureFrameWriteResult) -> Void
  ) {
    pendingCompletion = completion
    if let existing = pendingCompletionResult {
      pendingCompletionResult = Self.mergedCompletion(existing, result)
    } else {
      pendingCompletionResult = result
    }

    if result.errorDescription != nil || !result.pass || !result.issues.isEmpty {
      flushCompletion()
      return
    }
    scheduleCompletionFlush()
  }

  private func scheduleCompletionFlush() {
    guard !completionFlushScheduled else {
      return
    }
    completionFlushScheduled = true
    writeQueue.asyncAfter(deadline: .now() + completionCoalesceDelay) { [weak self] in
      self?.flushCompletion()
    }
  }

  private func flushCompletion() {
    completionFlushScheduled = false
    guard let result = pendingCompletionResult, let completion = pendingCompletion else {
      return
    }
    pendingCompletionResult = nil
    DispatchQueue.main.async {
      completion(result)
    }
  }

  private static func mergedCompletion(
    _ lhs: CaptureFrameWriteResult,
    _ rhs: CaptureFrameWriteResult
  ) -> CaptureFrameWriteResult {
    let errorDescription: String?
    if let lhsError = lhs.errorDescription, let rhsError = rhs.errorDescription {
      errorDescription = "\(lhsError) | \(rhsError)"
    } else {
      errorDescription = lhs.errorDescription ?? rhs.errorDescription
    }

    let timingSummary: String?
    if let rhsTiming = rhs.importTimingSummary {
      timingSummary = lhs.batchCount + rhs.batchCount > 1
        ? "batches \(lhs.batchCount + rhs.batchCount) | last \(rhsTiming)"
        : rhsTiming
    } else {
      timingSummary = lhs.importTimingSummary
    }

    return CaptureFrameWriteResult(
      batchCount: lhs.batchCount + rhs.batchCount,
      frameCount: lhs.frameCount + rhs.frameCount,
      rawInserted: lhs.rawInserted + rhs.rawInserted,
      rawExisting: lhs.rawExisting + rhs.rawExisting,
      inserted: lhs.inserted + rhs.inserted,
      existing: lhs.existing + rhs.existing,
      pass: lhs.pass && rhs.pass,
      issues: Array((lhs.issues + rhs.issues).prefix(12)),
      nextActions: Array((lhs.nextActions + rhs.nextActions).prefix(8)),
      errorDescription: errorDescription,
      bridgeTiming: rhs.bridgeTiming ?? lhs.bridgeTiming,
      importTimingSummary: timingSummary
    )
  }

  private static func intValue(_ value: Any?) -> Int? {
    if let value = value as? Int {
      return value
    }
    if let value = value as? Double {
      return Int(value)
    }
    if let value = value as? String {
      return Int(value)
    }
    return nil
  }

  private static func boolValue(_ value: Any?) -> Bool? {
    if let value = value as? Bool {
      return value
    }
    if let value = value as? Int {
      return value != 0
    }
    if let value = value as? String {
      switch value.lowercased() {
      case "true", "1", "yes":
        return true
      case "false", "0", "no":
        return false
      default:
        return nil
      }
    }
    return nil
  }

  private static func stringArray(_ value: Any?) -> [String] {
    (value as? [Any])?.compactMap { element in
      if let string = element as? String {
        return string
      }
      return nil
    } ?? []
  }

  private static func nextActionSummaries(_ value: Any?) -> [String] {
    (value as? [[String: Any]])?.compactMap { action in
      let scope = action["scope"] as? String
      let reason = action["reason"] as? String
      let text = action["action"] as? String
      return [scope, reason, text]
        .compactMap { $0 }
        .joined(separator: ": ")
    } ?? []
  }

  private static func importTimingSummary(_ value: Any?) -> String? {
    guard let timing = value as? [String: Any] else {
      return nil
    }

    func milliseconds(_ key: String) -> String {
      let microseconds = intValue(timing[key]) ?? 0
      return String(format: "%.1f", Double(microseconds) / 1_000)
    }

    return "total \(milliseconds("total_us"))ms | hex \(milliseconds("hex_decode_us"))ms | rawHex \(milliseconds("raw_hex_encode_us"))ms | raw \(milliseconds("raw_insert_us"))ms | parse \(milliseconds("frame_parse_us"))ms | decoded \(milliseconds("decoded_insert_us"))ms | timeline \(milliseconds("timeline_us"))ms | compact \(milliseconds("raw_compaction_us"))ms"
  }
}
