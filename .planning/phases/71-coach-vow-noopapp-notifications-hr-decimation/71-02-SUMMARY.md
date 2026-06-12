---
phase: 71-coach-vow-noopapp-notifications-hr-decimation
plan: "02"
subsystem: ui
tags: [swift, heartrate, decimation, stride-n, hearthrate-series-store]

requires:
  - phase: 71-01
    provides: "prior plan in same phase (no direct dependency)"

provides:
  - "decimatedSamples(from:to:maxCount:) on HeartRateSeriesStore — stride-N with local min/max per window"
  - "decimatedSamples(forDayContaining:calendar:maxCount:) convenience overload"
  - "4 HealthDataStore+* call sites migrated to decimatedSamples"

affects:
  - "chart rendering performance for long BLE sessions"
  - "HealthDataStore+Snapshots hkStrainScore and 7-day TRIMP trend"
  - "HealthDataStore+StressEnergy stressAlgorithmSummary"
  - "HealthDataStore+Cardio sessionCardioLoad"

tech-stack:
  added: []
  patterns:
    - "Stride-N decimation: passthrough when raw.count <= 1000; stride = max(1, raw.count / maxCount); preserve local max + min per window via id-based dedup"
    - "NSLock re-entrancy guard: decimatedSamples calls public samples(from:to:) which owns locking; never acquires stateLock directly"

key-files:
  created: []
  modified:
    - GooseSwift/HeartRateSeriesStores.swift
    - GooseSwift/HealthDataStore+Snapshots.swift
    - GooseSwift/HealthDataStore+StressEnergy.swift
    - GooseSwift/HealthDataStore+Cardio.swift

key-decisions:
  - "Passthrough threshold is 1000 (not maxCount=500) — DATA-04 locked decision; short sessions return full fidelity"
  - "maxCount default 500 governs stride calculation only: stride = max(1, raw.count / maxCount)"
  - "id-based deduplication within window prevents appending the same sample twice (first == max or first == min)"
  - "decimatedSamples result is re-sorted by capturedAt to restore chronological order after window traversal"

requirements-completed:
  - DATA-04

duration: 5min
completed: 2026-06-12
---

# Phase 71 Plan 02: HR Decimation Summary

**Stride-N HR decimation added to HeartRateSeriesStore (passthrough <= 1000 samples, max 500 out with local min/max preservation) and 4 HealthDataStore+* callers migrated**

## Performance

- **Duration:** 5 min
- **Started:** 2026-06-12T14:57:40Z
- **Completed:** 2026-06-12T15:03:03Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Added `decimatedSamples(from:to:maxCount:)` to `HeartRateSeriesStore` — stride-N algorithm with local max/min preservation per stride window, passthrough guard at 1000 samples, NSLock safety via public `samples(from:to:)` delegation
- Added `decimatedSamples(forDayContaining:calendar:maxCount:)` convenience overload that computes day boundaries and delegates to the range overload
- Migrated all 4 confirmed `HealthDataStore+*` call sites: `hkStrainScore()`, 7-day TRIMP trend loop (both in Snapshots), `stressAlgorithmSummary()` (StressEnergy), `sessionCardioLoad()` HR fallback (Cardio)
- Build succeeded (`BUILD SUCCEEDED` with separate derived-data path to avoid Xcode lock contention)

## Task Commits

1. **Task 1: Add decimatedSamples methods to HeartRateSeriesStore** - `abcbb3a` (feat)
2. **Task 2: Migrate 4 HealthDataStore+* call sites to decimatedSamples** - `fd0c76a` (feat)

## Files Created/Modified

- `GooseSwift/HeartRateSeriesStores.swift` - Added two `decimatedSamples` overloads immediately after `samples(from:to:)`; 31 lines added
- `GooseSwift/HealthDataStore+Snapshots.swift` - 2 call sites migrated (lines ~996, ~1129)
- `GooseSwift/HealthDataStore+StressEnergy.swift` - 1 call site migrated (line ~20)
- `GooseSwift/HealthDataStore+Cardio.swift` - 1 call site migrated (line ~172)

## Decisions Made

- Passthrough threshold is `raw.count > 1000` (not `> maxCount`) per DATA-04 locked decision — short sessions return full-fidelity arrays
- `maxCount` (default 500) is only used to compute stride: `max(1, raw.count / maxCount)`
- `reserveCapacity(raw.count / stride * 3)` pre-allocates for up to 3 samples per window (first + max + min)
- `result.last?.id` guard prevents appending `minSample` when it was already appended as `maxSample`
- Result sorted by `capturedAt` to restore chronological order after non-sequential window appends

## Deviations from Plan

None - plan executed exactly as written.

The RESEARCH sketch showed `guard raw.count > maxCount` but the PLAN.md critical constraints (#2) specified `guard raw.count > 1000` — the plan constraint was followed as the locked decision.

## Issues Encountered

Xcode was open and holding the build database lock (`database is locked`). Resolved by passing `-derivedDataPath /tmp/goose-build-71-02` to xcodebuild to use a separate derived-data directory. Build result: `BUILD SUCCEEDED`.

## Threat Surface Scan

No new network endpoints, auth paths, file access patterns, or schema changes introduced. Pure in-memory computation on existing `[HeartRateSamplePoint]` arrays. NSLock safety verified: `decimatedSamples` methods contain zero `stateLock` references.

## Known Stubs

None.

## Self-Check

- `grep -c "func decimatedSamples" GooseSwift/HeartRateSeriesStores.swift` → 2 PASS
- `stateLock` absent from new decimatedSamples bodies → 0 occurrences PASS
- Legacy `heartRateSeriesStore.samples(` in migrated files → 0 PASS
- `decimatedSamples` count: Snapshots=2, StressEnergy=1, Cardio=1 PASS
- Build: `BUILD SUCCEEDED` PASS

## Self-Check: PASSED

## Next Phase Readiness

- DATA-04 complete; HR chart rendering now capped at ~500 samples for sessions > 1000 samples, eliminating chart lag on long BLE sessions
- No blockers for remaining Phase 71 plans (71-03 and onwards)

---
*Phase: 71-coach-vow-noopapp-notifications-hr-decimation*
*Completed: 2026-06-12*
