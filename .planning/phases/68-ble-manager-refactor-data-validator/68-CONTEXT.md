# Phase 68: BLE Manager Refactor + Data Validator - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Two independent but related Swift-side improvements:

**BLE5-03 — GooseBLEHistoricalManager:**
Extract all historical sync state and logic from GooseBLEClient into a dedicated `GooseBLEHistoricalManager` final class. GooseBLEClient retains a private manager instance and exposes proxy computed properties (`isHistoricalSyncing`, `historicalSyncStatus`, `historicalSyncRunID`) that forward to the manager — all 15+ existing call sites in GooseBLEClient extensions are preserved without change.

**BLE5-04 — GooseBLEDataValidator:**
Add a `GooseBLEDataValidator` struct that checks structural invariants before bytes reach the Rust bridge. Called in `NotificationFrameParsing.swift` (the notification ingest pipeline, before `GooseRustBridge.request()`). Invalid frames are logged via OSLog (warning level) and counted in an `invalidFrameCount` counter visible in More > Debug. Validator checks: minimum payload length ≥ 1 byte, device_id non-nil, payload non-empty. NO packet-type whitelist — structural invariants only.

</domain>

<decisions>
## Implementation Decisions

### GooseBLEHistoricalManager Design
- `final class GooseBLEHistoricalManager` with callback pattern (not @Observable, not Combine) — consistent with GooseBLEBondingManager precedent
- Owned as private property on GooseBLEClient
- GooseBLEClient exposes proxy computed properties: `var isHistoricalSyncing: Bool { historicalManager.isHistoricalSyncing }`, etc.
- `historicalSyncRunID: UUID` migrates to the manager
- Historical sync start/stop logic moves to manager methods `startHistoricalSync()` / `stopHistoricalSync()` — GooseBLEClient+HistoricalHandlers.swift delegates to manager
- All existing call sites in GooseBLEClient+*.swift preserved via computed var proxies (no call site changes needed)

### GooseBLEDataValidator Scope
- Called in `NotificationFrameParsing.swift` before passing bytes to the Rust bridge
- Invariants checked: payload length ≥ 1, device_id non-nil, payload non-empty
- NO packet-type whitelist — structural invariants only (per CONTEXT + pitfalls research: whitelist would silently break WHOOP 5.0 support)
- Invalid frame action: OSLog warning per frame + increment `invalidFrameCount` (accumulated across app session)
- `invalidFrameCount` exposed as `@Published var invalidFrameCount: Int = 0` on GooseBLEClient (@MainActor); GooseBLEDataValidator calls a callback to increment it
- Debug counter shown in DebugView (More > Debug) alongside existing counters

### Claude's Discretion
- File name for GooseBLEHistoricalManager.swift (separate file vs extension of GooseBLEClient)
- Whether GooseBLEHistoricalManager.swift also replaces GooseBLEClient+HistoricalHandlers.swift content or just wraps it
- Exact OSLog category/subsystem for GooseBLEDataValidator logs

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `GooseBLEBondingManager.swift` — exact pattern for manager design: final class, callback (not @Observable), UserDefaults persistence optional
- `GooseBLEClient.swift` lines 45-46: `var isHistoricalSyncing = false` and `var historicalSyncStatus = "idle"` — migrate these to GooseBLEHistoricalManager
- `GooseBLEClient.swift` line 323: `var historicalSyncRunID = UUID()` — migrates to manager
- `NotificationFrameParsing.swift` — entry point for GooseBLEDataValidator injection
- Existing `invalidFrameCount`-style counters in GooseBLEClient (grep for "Count" vars to match naming pattern)

### Established Patterns
- `private(set)` for read-only @Published state in GooseBLEClient
- OSLog structured logging: `ble.record(level: .warning, "...")` or `Logger(subsystem:, category:)`
- @MainActor for UI-visible state mutations; Task { @MainActor in ... } for background→main hops
- Callback pattern on final class (same as GooseBLEBondingManager)

### Integration Points
- `GooseBLEClient+HistoricalHandlers.swift` — primary site for historical sync logic migration
- `GooseBLEClient+DebugAndSync.swift` — secondary historical sync references (sync status, run IDs)
- `GooseBLEClient+CentralDelegate.swift` — `isHistoricalSyncing` checks on BLE events
- `GooseBLEClient+Commands.swift` — `canSendHello && !isHistoricalSyncing` guards
- DebugView or MoreView — where to expose `invalidFrameCount`

</code_context>

<specifics>
## Specific Ideas

- Proxy computed var pattern: `var isHistoricalSyncing: Bool { historicalManager.isHistoricalSyncing }` — this preserves all 15+ call sites without change. The manager owns the state; client reads through.
- BLE5-03 pitfall (from PITFALLS.md): "Partial extraction causes dual-ownership" — ALL historical state must move to the manager atomically; no split where some state remains on GooseBLEClient.
- BLE5-04 pitfall: "BLE5-04 validator scope must be defined before implementation — structural invariants only, no packet-type whitelist, or it will break WHOOP 5.0 support silently."
- 15+ call sites referencing `isHistoricalSyncing`: confirmed in GooseBLEClient+CentralDelegate (2), GooseBLEClient+DebugAndSync (10), GooseBLEClient+Commands (4), GooseBLEClient+HistoricalHandlers (5), GooseBLEClient+Parsing (1), GooseBLEClient+PeripheralDelegate (2), GooseBLEClient+UserActions (1).
- `historicalSyncRunID` is a UUID used to detect stale async callbacks; must move to manager.

</specifics>

<deferred>
## Deferred Ideas

- Making GooseBLEHistoricalManager @Observable (deferred — not needed, callback pattern sufficient)
- Protocol for GooseBLEHistoricalManager (ARCH-01 service layer covers this in Phase 72)
- Background URLSession for historical sync (out of scope per PROJECT.md)
- Persisting invalidFrameCount across app restarts (session-only counter is sufficient)

</deferred>
