---
phase: 68-ble-manager-refactor-data-validator
reviewed: 2026-06-12T00:00:00Z
depth: standard
files_reviewed: 10
files_reviewed_list:
  - GooseSwift/GooseBLEHistoricalManager.swift
  - GooseSwift/GooseBLEDataValidator.swift
  - GooseSwift/GooseBLEClient.swift
  - GooseSwift/GooseBLEClient+HistoricalCommands.swift
  - GooseSwift/GooseBLEClient+HistoricalHandlers.swift
  - GooseSwift/GooseBLEClient+DebugAndSync.swift
  - GooseSwift/GooseBLEClient+Parsing.swift
  - GooseSwift/GooseBLEClient+PeripheralDelegate.swift
  - GooseSwift/GooseAppModel+NotificationPipeline.swift
  - GooseSwift/MoreDebugViews.swift
findings:
  critical: 3
  warning: 4
  info: 2
  total: 9
status: issues_found
---

# Phase 68: Code Review Report

**Reviewed:** 2026-06-12
**Depth:** standard
**Files Reviewed:** 10
**Status:** issues_found

## Summary

This phase extracts historical-sync bookkeeping into `GooseBLEHistoricalManager` and adds structural frame validation via `GooseBLEDataValidator`. The separation of concerns is clean and the validator's hex-decoding loop is correct. However, three correctness defects were found:

1. `GooseBLEHistoricalManager` exposes almost all of its state as unprotected `var` properties; the `NSLock` guards only the three mutation methods but the dozens of direct property accesses throughout `GooseBLEClient+Historical*.swift` are completely unprotected. Since the client already operates exclusively on the main thread (every CoreBluetooth delegate is bounced to main via `dispatchCoreBluetoothDelegateToMainIfNeeded`), the lock gives a false sense of safety while adding a second, redundant read inside each mutation method.

2. `dataValidator` is a value-type `struct` stored as a `var`. The `onInvalidFrame` closure is set once in `init` and the struct is never mutated afterwards — but because it is a `var`, any future code that assigns a new `GooseBLEDataValidator()` to the field will silently discard the callback wiring. The callback closure itself captures `self` weakly so there is no retain cycle, but the mutation semantics are a trap.

3. The `parseNotificationFrames` closure in `GooseAppModel+NotificationPipeline.swift` reads `ble.dataValidator` off the main thread (on `notificationParseQueue`) while `dataValidator` is a plain `var` on a non-isolated class. Under `@Observable` and Swift's current compiler, this is a data race: the main thread could mutate `dataValidator` while the background queue is reading it.

Additionally, the `acceptedFrames` count is never surfaced in any log or status string — only the raw `frames` count (from the Rust reassembler) reaches the pipeline performance logs, making it invisible when the validator is actually rejecting frames.

---

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: GooseBLEHistoricalManager properties are accessed without the lock from all call sites

**File:** `GooseSwift/GooseBLEHistoricalManager.swift:64-133` / `GooseSwift/GooseBLEClient+HistoricalCommands.swift:33-58` / `GooseSwift/GooseBLEClient+HistoricalHandlers.swift:26-706`

**Issue:** `GooseBLEHistoricalManager` declares a `private let lock = NSLock()` and uses it inside the three mutation methods (`beginSync`, `completeSync`, `failSync`). Every other property — `historicalPacketsReceivedThisSync`, `pendingHistoricalFrames`, `pendingHistoricalCommand`, `historyEndAckQueued`, `gen4HistoricalPageSeq`, and 20+ others — is accessed and mutated directly by the extension files with no locking.

The caller code in `GooseBLEClient+HistoricalCommands.swift` performs compound read-modify-write sequences such as:

```swift
// lines 33-58 of GooseBLEClient+HistoricalCommands.swift — no lock held
historicalManager.beginSync(runID: newRunID)
historicalManager.historicalRangePollOnly = rangeOnly         // unguarded
historicalManager.historicalDataResultAckEnabled = ...        // unguarded
historicalPacketCount = 0                                      // main-thread only
historicalManager.historicalPacketsReceivedThisSync = 0       // unguarded
// ... 15 more direct property mutations
```

The lock inside `beginSync` guards `isHistoricalSyncing`, `historicalSyncRunID`, and `historicalSyncStatus` atomically, then *immediately after* the locked section, the caller unguardedly mutates the other fields. If any callback (e.g., a `DispatchWorkItem` fired by a timeout on the main queue) races against another main-queue dispatch, the combined multi-property state will be inconsistent.

More concretely: `beginSync` fires `DispatchQueue.main.async { self.onSyncStateChange?(isSyncing) }` after releasing the lock (line 87). The callback is `historicalManager.onSyncStateChange` — wired in `GooseBLEClient.init` to a no-op — but `onSyncCompleted` is wired to:

```swift
Task { @MainActor in
  self.lastHistoricalSyncCompletedAt = completedAt
}
```

A `Task { @MainActor in }` enqueued from a `DispatchQueue.main.async` closure re-enters the main actor via the cooperative thread pool, which means `lastHistoricalSyncCompletedAt` may be written *after* `completeHistoricalSync` has already reset `historicalPacketCount = 0` for a new sync — producing a stale completion timestamp against a new sync's state.

**Fix:** Since all callers already operate on the main thread (the entire historical sync state machine runs on main), drop the `NSLock` entirely and add a `// All properties accessed exclusively on the main thread` contract comment. If genuine cross-thread access is ever needed, protect all accesses uniformly — not just the three scalar flags. The current selective locking is incorrect in both directions: it gives the impression of thread safety it does not provide, and the re-read inside `completeSync`/`failSync` (line 99, 113) acquires the lock a second time just to re-read a value that was just set under the same lock:

```swift
// GooseBLEHistoricalManager.swift lines 94-104 — double-lock pattern is wasteful and misleading
lock.withLock {
  isHistoricalSyncing = false
  historicalSyncStatus = "synced"
}
let isSyncing = lock.withLock { isHistoricalSyncing }  // re-reads the value we just wrote
DispatchQueue.main.async { [weak self] in
  guard let self else { return }
  self.onSyncStateChange?(isSyncing)                    // always false here
}
```

---

### CR-02: `dataValidator` struct mutation semantics — callback wiring silently lost on reassignment

**File:** `GooseSwift/GooseBLEClient.swift:101,1004-1007`

**Issue:** `dataValidator` is declared as a plain `var` on `GooseBLEClient`:

```swift
var dataValidator = GooseBLEDataValidator()   // line 101
```

In `init`, the callback is wired:

```swift
dataValidator.onInvalidFrame = { [weak self] in   // line 1004
  Task { @MainActor in
    self?.invalidFrameCount += 1
  }
}
```

Because `GooseBLEDataValidator` is a `struct`, this assignment writes into a *copy* of the validator embedded in the class. If any code ever writes `dataValidator = GooseBLEDataValidator()` — or, more likely, if any extension method takes `dataValidator` via `var validator = dataValidator` and mutates it (Swift copy-on-write) — the callback is silently dropped and `invalidFrameCount` stops incrementing without any error.

In `GooseAppModel+NotificationPipeline.swift` line 387, the closure captures `ble.dataValidator` by value (structs are copied):

```swift
notificationParseQueue.async {
  let deviceID = ble.activeDeviceIdentifier
  let acceptedFrames = frames.filter { ble.dataValidator.validate(frameHex: $0.hex, deviceID: deviceID) }
```

`ble.dataValidator` is accessed off the main thread. `ble` is `GooseBLEClient` — an `@Observable final class @unchecked Sendable`. `dataValidator` is a `var` property on that class, with no isolation annotation. Any write to `dataValidator` from the main thread (e.g., an external call to reconfigure the validator) races with this background read.

**Fix — option A (preferred):** Make the validator a `let` constant since it is never legitimately replaced after init:

```swift
let dataValidator = GooseBLEDataValidator()
```

Then wire the callback in `init` via a `mutating func` or directly. Alternatively:

**Fix — option B:** Convert `GooseBLEDataValidator` to a `final class` to get reference semantics, eliminating the copy problem and making the background read safe (the struct's individual fields are all `let`).

---

### CR-03: `invalidFrameCount` incremented inside `Task { @MainActor }` — wrong isolation from a background closure

**File:** `GooseSwift/GooseBLEClient.swift:1004-1007`

**Issue:** The `onInvalidFrame` callback is set as:

```swift
dataValidator.onInvalidFrame = { [weak self] in
  Task { @MainActor in
    self?.invalidFrameCount += 1
  }
}
```

The validator comment says "Called on the background queue that runs validation." The closure therefore runs on `notificationParseQueue`. Inside that closure, a `Task { @MainActor in }` is created. The `Task` is *scheduled* on the main actor's cooperative executor but is not awaited — it is fire-and-forget. This means:

1. The increment of `invalidFrameCount` is deferred to an arbitrary future point on the cooperative thread pool, not synchronously on the main queue.
2. If `self` is deallocated between the time the task is created and the time it runs (unlikely but possible in test teardown scenarios), the `self?.invalidFrameCount` expression is silently dropped with no error.
3. `invalidFrameCount` is read in `MoreDebugViews.swift` line 81 from the `@Observable` model on the main actor, so the `@MainActor` hop is correct in intent — but the async `Task` wrapper means the count visible in the UI can lag multiple frames behind the actual rejection rate during a burst of invalid frames.

More importantly, `onInvalidFrame?()` is called *inside* `validate()`, which is called from within `frames.filter { ... }` on `notificationParseQueue`. The validator's own `logger.warning(...)` calls are synchronous, but `onInvalidFrame` dispatches asynchronously. The net effect is that valid filter result and the side-effect counter are no longer causally paired — a frame could be rejected, the pipeline could complete, and `invalidFrameCount` could increment after the batch is done.

**Fix:** Use `DispatchQueue.main.async` instead of `Task { @MainActor in }` to match the rest of the codebase pattern, or increment `invalidFrameCount` via `bleUIStateAggregator` as a batched published value to avoid per-frame dispatch overhead:

```swift
dataValidator.onInvalidFrame = { [weak self] in
  DispatchQueue.main.async {
    self?.invalidFrameCount += 1
  }
}
```

---

## Warnings

### WR-01: `acceptedFrames` count is never logged — validator rejection is invisible in the pipeline

**File:** `GooseSwift/GooseAppModel+NotificationPipeline.swift:387-389`

**Issue:** The filtering step produces `acceptedFrames` from `frames`, but the count difference — the number of frames *rejected by the validator* — is never surfaced in any log statement or `publishPipelinePerformanceStatus` call. The only signal that the validator rejected something is the asynchronous `invalidFrameCount` increment (which has its own delay per CR-03). During a burst of malformed frames, there is no log entry showing how many frames were dropped here versus how many passed through.

```swift
let acceptedFrames = frames.filter { ble.dataValidator.validate(frameHex: $0.hex, deviceID: deviceID) }
let frameHexes = acceptedFrames.map(\.hex)
let (parseResults, bridgeTiming, batchTiming) = parser.parseBatch(frameHexes: frameHexes, deviceType: deviceType)
// No log: acceptedFrames.count vs frames.count
```

The `ParsedNotificationFrameDispatch` struct passes `totalFrameCount: parseResults.count` to both `handleParsedNotificationFrames` and `handleParsedNotificationFramesWithoutMain`. This `totalFrameCount` is `parseResults.count` — the count *after* validation filtering. The original `frames.count` (pre-filter) is captured in the outer scope but never reaches the dispatch struct.

**Fix:** Log the rejection count immediately after the filter:

```swift
let acceptedFrames = frames.filter { ble.dataValidator.validate(frameHex: $0.hex, deviceID: deviceID) }
let rejectedCount = frames.count - acceptedFrames.count
if rejectedCount > 0 {
  ble.record(level: .warn, source: "ble.validator",
    title: "frame.validator.rejected",
    body: "rejected=\(rejectedCount) accepted=\(acceptedFrames.count) total=\(frames.count)"
  )
}
```

And thread the original `frames.count` into `ParsedNotificationFrameDispatch.totalFrameCount` (or add a separate `validatorRejectedCount` field) so the pipeline performance status correctly accounts for validator-dropped frames.

---

### WR-02: `historicalPacketCount = 0` in `beginHistoricalSync` bypasses `historicalManager` — split state

**File:** `GooseSwift/GooseBLEClient+HistoricalCommands.swift:36`

**Issue:** `beginHistoricalSync` resets `historicalPacketCount` directly on `GooseBLEClient`:

```swift
historicalPacketCount = 0          // line 36 — GooseBLEClient property
historicalManager.historicalPacketsReceivedThisSync = 0   // line 37
```

But the canonical count is `historicalManager.historicalPacketsReceivedThisSync`. The `GooseBLEClient.historicalPacketCount` published property is supposed to be updated *from* the manager (via `publishHistoricalPacketCountIfNeeded`). Resetting both separately means there is a window between line 36 and the eventual `completeSync` → `onPacketCountChange` callback where the two counts are inconsistent — one is 0 (the published UI value) and the other is the manager's working count, which may be non-zero if a concurrent `flushPendingHistoricalFramesIfNeeded` call runs between the two resets.

This is the same category of split-state risk introduced by the refactor: the manager holds the authoritative count, but the client still owns a shadow copy that is reset independently.

**Fix:** Remove the direct reset of `historicalPacketCount = 0` from `beginHistoricalSync`. Instead, call `publishHistoricalPacketCountIfNeeded(force: true)` *after* `historicalManager.historicalPacketsReceivedThisSync = 0` to drive the published property through the single canonical path.

---

### WR-03: `historicalDataResultPayload` slice bounds — off-by-one relative to guard

**File:** `GooseSwift/GooseBLEClient+Parsing.swift:876-885`

**Issue:**

```swift
static func historicalDataResultPayload(fromHistoryEndMetadataPayload payload: [UInt8]) -> [UInt8]? {
  guard payload.count > 21 else {   // line 877 — requires count >= 22
    return nil
  }
  var result: [UInt8] = [1]
  result.append(contentsOf: payload[13..<21])   // line 882 — reads indices 13..20
  return result
}
```

The guard requires `payload.count > 21` (i.e., count ≥ 22). The slice `payload[13..<21]` reads 8 bytes at indices 13–20, which requires `payload.count >= 21`. The guard is one byte stricter than necessary. This is not a crash risk — the extra strictness makes it safe — but it is inconsistent with the comment ("HistoryEnd body bytes 4...11" implies bytes at absolute offsets 13–20, which fits a 21-byte minimum, not 22). The over-strict guard will silently return `nil` for a 21-byte payload that is actually valid per the protocol, causing the ack to go unsent and triggering the `result_ack.unprepared` warning path.

**Fix:** Change the guard to use `>=` instead of `>`:

```swift
guard payload.count >= 21 else {
  return nil
}
```

---

### WR-04: New `ISO8601DateFormatter()` allocated per historical data packet

**File:** `GooseSwift/GooseBLEClient+HistoricalHandlers.swift:45`

**Issue:**

```swift
let capturedAtISO = ISO8601DateFormatter().string(from: Date())
```

`ISO8601DateFormatter` is expensive to allocate and is instantiated fresh for every historical data packet received. During a full historical sync, this may be called hundreds or thousands of times in rapid succession on the main thread. `GooseBLEClient` already has a `static let diagnosticLogFormatter: ISO8601DateFormatter` for exactly this kind of usage.

**Fix:** Reuse the existing formatter:

```swift
let capturedAtISO = GooseBLEClient.diagnosticLogFormatterLock.withLock {
  GooseBLEClient.diagnosticLogFormatter.string(from: Date())
}
```

Or add a dedicated static formatter for the historical path if the formatter options differ.

---

## Info

### IN-01: `GooseBLEHistoricalManager` var properties have no access control — all public to the whole module

**File:** `GooseSwift/GooseBLEHistoricalManager.swift:8-53`

**Issue:** Every property on `GooseBLEHistoricalManager` is `var` with no access modifier, making them all internal by default. Because the class lives in the same module as `GooseBLEClient`, all extensions can access and mutate every property directly — which they do. This is by design (the refactor is pure extraction, not encapsulation), but it means the class provides no structural protection against accidental mutation from unrelated call sites. The project convention for internal-state classes is `private` on stored properties with mutation via explicit methods.

**Suggestion:** Mark the 30+ bookkeeping vars `fileprivate` or provide grouped mutation methods, so the extension files in `GooseBLEClient+Historical*.swift` are the only callers. This would also make the `NSLock` story more tractable (see CR-01).

---

### IN-02: `validate(payload:deviceID:)` duplicates work when called from `validate(frameHex:deviceID:)`

**File:** `GooseSwift/GooseBLEDataValidator.swift:15-29,34-58`

**Issue:** The hex overload decodes the hex string into `[UInt8]` and then calls `validate(payload:deviceID:)`, which re-checks `payload.isEmpty`. Since the hex decoder already verified `hex.count >= 2` (enforced by the even-length guard and at least one iteration of the loop), the payload it produces is guaranteed non-empty before the call. The redundant `!payload.isEmpty` check in the byte overload is therefore dead for all calls routed through the hex path.

This is not a bug — it is a minor logic redundancy. The byte-based `validate` is a public entry point in its own right and the guard is correct there.

**Suggestion:** Add a comment noting the invariant, or make the internal byte-path a private helper that skips the redundant check.

---

_Reviewed: 2026-06-12_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
