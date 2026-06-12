---
plan: 68-01
phase: 68-ble-manager-refactor-data-validator
status: complete
started: 2026-06-12
completed: 2026-06-12
requirements: [BLE5-03]
commits:
  - f41db29
  - 6c57f96
---

## What Was Built

Created `GooseBLEHistoricalManager` — a dedicated `final class` that owns all historical sync state extracted from `GooseBLEClient`. Followed the `GooseBLEBondingManager` precedent exactly: plain stored vars, `NSLock` for thread safety, callback closures that hop to main.

### Key Decisions

- **Pattern:** Callback-based final class (not @Observable, not Combine) — matches the BondingManager precedent and CONTEXT.md decision.
- **Scope:** All ~30 historical stored vars migrated atomically: state flags, counters, work items, pending frames, ack flags, timing constants.
- **Kept on GooseBLEClient:** `historicalPacketCount` and `lastHistoricalSyncCompletedAt` (both are UI-visible @Observable vars; callbacks from the manager update them on main).
- **Proxies:** Three read-only computed properties on GooseBLEClient (`isHistoricalSyncing`, `historicalSyncStatus`, `historicalSyncRunID`) forward to the manager — all external read call sites compile and behave unchanged.
- **Intent-revealing mutations:** `beginSync(runID:)`, `completeSync(completedAt:)`, `failSync(status:)`, `setStatus(_:)` encapsulate the four state transition patterns. All direct writes removed from extension files.

### Files Modified

- **GooseSwift/GooseBLEHistoricalManager.swift** (new, 133 lines) — the manager class
- **GooseSwift/GooseBLEClient.swift** — added manager ownership + 3 proxy computed vars; removed ~30 stored declarations
- **GooseSwift/GooseBLEClient+HistoricalCommands.swift** — delegated beginSync + all manager var accesses
- **GooseSwift/GooseBLEClient+HistoricalHandlers.swift** — delegated completeSync/failSync/setStatus + manager var accesses
- **GooseSwift/GooseBLEClient+DebugAndSync.swift** — delegated setStatus("waiting") + all manager var accesses
- **GooseSwift/GooseBLEClient+Parsing.swift** — delegated setStatus("idle") + manager var accesses
- **GooseSwift/GooseBLEClient+PeripheralDelegate.swift** — fixed `lastHandledWasHistoricalDataPacket` reference
- **GooseSwift.xcodeproj/project.pbxproj** — registered GooseBLEHistoricalManager.swift in GooseSwift target

### Deviations

- The perl-based batch substitution in DebugAndSync.swift missed `self.pendingHistoricalCommand` patterns inside closures (dot-lookbehind excluded them). Fixed manually with targeted substitution.
- `GooseBLEClient+PeripheralDelegate.swift` also had a bare `lastHandledWasHistoricalDataPacket` reference not covered by the initial plan scope — fixed in Task 2 to achieve a clean build.

## Self-Check: PASSED

- `final class GooseBLEHistoricalManager` exists at GooseBLEHistoricalManager.swift:5 ✓
- `let historicalManager = GooseBLEHistoricalManager()` in GooseBLEClient.swift:100 ✓
- Zero stored `isHistoricalSyncing = false`, `historicalSyncStatus = "idle"`, `historicalSyncRunID = UUID()` on GooseBLEClient ✓
- Proxy computed vars present (`historicalManager.isHistoricalSyncing` count ≥ 2 in GooseBLEClient.swift) ✓
- No @Observable or Combine on manager ✓
- `historicalPacketCount` and `lastHistoricalSyncCompletedAt` retained on GooseBLEClient ✓
- pbxproj references: `grep -c "GooseBLEHistoricalManager.swift" project.pbxproj` = 4 ✓
- All write sites delegated (4 unique manager methods) ✓
- No leftover direct writes in extension files ✓
- Non-historical read call sites (Commands, CentralDelegate) unchanged ✓
- Stale-callback guard intact (3 occurrences in DebugAndSync) ✓
- BUILD SUCCEEDED ✓

key-files:
  created:
    - GooseSwift/GooseBLEHistoricalManager.swift
  modified:
    - GooseSwift/GooseBLEClient.swift
    - GooseSwift/GooseBLEClient+HistoricalCommands.swift
    - GooseSwift/GooseBLEClient+HistoricalHandlers.swift
    - GooseSwift/GooseBLEClient+DebugAndSync.swift
    - GooseSwift/GooseBLEClient+Parsing.swift
    - GooseSwift/GooseBLEClient+PeripheralDelegate.swift
    - GooseSwift.xcodeproj/project.pbxproj
