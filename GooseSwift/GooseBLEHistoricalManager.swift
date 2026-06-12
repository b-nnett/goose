import CoreBluetooth
import Foundation


final class GooseBLEHistoricalManager {
  // MARK: - Core sync state

  var isHistoricalSyncing = false
  var historicalSyncStatus = "idle"
  var historicalSyncRunID = UUID()
  var historicalRangePollOnly = false

  // MARK: - Work items (timeouts, idle detection, retry)

  var historicalCommandTimeoutWorkItem: DispatchWorkItem?
  var historicalIdleWorkItem: DispatchWorkItem?
  var historicalRangeRetryWorkItem: DispatchWorkItem?

  // MARK: - Pending command and frames

  var pendingHistoricalCommand: GooseBLEClient.PendingHistoricalCommand?
  var pendingHistoricalFrames: [(hex: String, capturedAt: String)] = []

  // MARK: - Packet tracking

  var lastHandledWasHistoricalDataPacket = false
  var nextHistoricalCommandSequence: UInt8 = 57
  var historicalPacketsReceivedThisSync = 0
  var historicalRangePendingResponses = 0
  var historicalRangeRetryCount = 0
  var historicalTransferRequestAttemptCount = 0

  // MARK: - Ack and metadata flags

  var historyEndAckQueued = false
  var historyEndAckSentThisBurst = false
  var pendingHistoryEndAckPayload: [UInt8]?
  var historyEndReceived = false
  var historyCompleteReceived = false
  var historyStartReceived = false
  var historicalDataResultAckEnabled = true

  // MARK: - Progress tracking

  var lastHistoricalPacketCountPublishedAt = Date.distantPast
  var lastHistoricalSyncProgressCallbackAt = Date.distantPast
  var lastHistoricalSyncProgressCallbackStatus = ""
  var lastHistoricalSyncProgressCallbackDetail = ""
  var coalescedHistoricalSyncProgressCallbackCount = 0

  // MARK: - Gen4 page sequence

  var gen4HistoricalPageSeq: UInt32 = 0

  // MARK: - Configuration constants

  let requestHistoricalRangeBeforeTransfer = true
  let historicalCommandResponseTimeout: TimeInterval = 7
  let historicalPendingResponseGrace: TimeInterval = 25
  let historicalRangeRetryDelay: TimeInterval = 1
  let historicalRangeMaxRetries = 2
  let historicalTransferMaxRequestAttempts = 3

  // MARK: - Internal lock

  private let lock = NSLock()

  // MARK: - Callbacks

  // Called on main thread when isHistoricalSyncing changes.
  var onSyncStateChange: ((Bool) -> Void)?
  // Called on main thread when a sync completes; provides the completion date.
  var onSyncCompleted: ((Date) -> Void)?
  // Called on main thread when the packet count changes during sync.
  var onPacketCountChange: ((Int) -> Void)?

  // MARK: - Mutation methods

  /// Begin a new historical sync: assign a new run ID, mark syncing, set status.
  func beginSync(runID: UUID) {
    lock.withLock {
      historicalSyncRunID = runID
      isHistoricalSyncing = true
      historicalSyncStatus = "syncing"
    }
    let isSyncing = lock.withLock { isHistoricalSyncing }
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.onSyncStateChange?(isSyncing)
    }
  }

  /// Mark sync complete: set status "synced", call completion callback with date.
  func completeSync(completedAt: Date) {
    lock.withLock {
      isHistoricalSyncing = false
      historicalSyncStatus = "synced"
    }
    let isSyncing = lock.withLock { isHistoricalSyncing }
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.onSyncStateChange?(isSyncing)
      self.onSyncCompleted?(completedAt)
    }
  }

  /// Mark sync failed: set isHistoricalSyncing = false and status to the given string.
  func failSync(status: String) {
    lock.withLock {
      isHistoricalSyncing = false
      historicalSyncStatus = status
    }
    let isSyncing = lock.withLock { isHistoricalSyncing }
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.onSyncStateChange?(isSyncing)
    }
  }

  /// Set a transient status string without changing isHistoricalSyncing (e.g. "waiting", "idle").
  func setStatus(_ status: String) {
    lock.withLock {
      historicalSyncStatus = status
    }
  }

  /// Publish current packet count to GooseBLEClient via callback.
  func publishPacketCount(_ count: Int) {
    DispatchQueue.main.async { [weak self] in
      self?.onPacketCountChange?(count)
    }
  }
}
