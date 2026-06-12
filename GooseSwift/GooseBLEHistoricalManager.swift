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

  // MARK: - Callbacks
  // All mutation methods and callbacks must be called on the main thread.
  // CoreBluetooth delegates are already bounced to main before reaching this class.

  var onSyncStateChange: ((Bool) -> Void)?
  var onSyncCompleted: ((Date) -> Void)?
  var onPacketCountChange: ((Int) -> Void)?

  // MARK: - Mutation methods

  /// Begin a new historical sync: assign a new run ID, mark syncing, set status.
  func beginSync(runID: UUID) {
    historicalSyncRunID = runID
    isHistoricalSyncing = true
    historicalSyncStatus = "syncing"
    onSyncStateChange?(true)
  }

  /// Mark sync complete: set status "synced", call completion callback with date.
  func completeSync(completedAt: Date) {
    isHistoricalSyncing = false
    historicalSyncStatus = "synced"
    onSyncStateChange?(false)
    onSyncCompleted?(completedAt)
  }

  /// Mark sync failed: set isHistoricalSyncing = false and status to the given string.
  func failSync(status: String) {
    isHistoricalSyncing = false
    historicalSyncStatus = status
    onSyncStateChange?(false)
  }

  /// Set a transient status string without changing isHistoricalSyncing (e.g. "waiting", "idle").
  func setStatus(_ status: String) {
    historicalSyncStatus = status
  }

  /// Publish current packet count to GooseBLEClient via callback.
  func publishPacketCount(_ count: Int) {
    DispatchQueue.main.async { [weak self] in
      self?.onPacketCountChange?(count)
    }
  }
}
