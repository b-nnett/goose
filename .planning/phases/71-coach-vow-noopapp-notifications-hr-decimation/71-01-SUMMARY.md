---
phase: 71-coach-vow-noopapp-notifications-hr-decimation
plan: 01
subsystem: ui
tags: [swiftui, coach, vow, nudge, healthstore, hrv]

# Dependency graph
requires:
  - phase: 70-haptic-primitive-breathe-screen
    provides: "coachCardSurface modifier and Coach tab patterns already established"
provides:
  - "CoachVOWNudge private enum with four urgency cases and @MainActor resolve(healthStore:) in CoachView.swift"
  - "CoachVOWCard private struct with dismiss xmark button, swipe-down gesture, accessibility labels"
  - "Session-scoped VOW nudge insertion in CoachOverviewScreen.body between CoachJournalCard and CoachRoutesSection"
affects:
  - "71-02 and later plans that extend CoachView.swift"

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "CoachVOWNudge.resolve() annotated @MainActor to satisfy Swift 6 HealthDataStore isolation when calling snapshot(for:)"
    - "Priority-ordered nudge resolution: criticalRecovery > lowRecovery > highStrain > lowHRV; returns nil when all healthy"
    - "Nil-safe Double(snapshot.value) parse per threat model T-71-01-02 — no nudge shown on parse failure"

key-files:
  created: []
  modified:
    - GooseSwift/CoachView.swift

key-decisions:
  - "@MainActor annotation on CoachVOWNudge.resolve() is required by Swift 6 strict concurrency — HealthDataStore.snapshot(for:) is main-actor-isolated; calling from a non-isolated static func would be a compile error"
  - "vowDismissed state is session-scoped only (no UserDefaults persistence) per CONTEXT.md scope"
  - "Swipe-down threshold set to 30pt matching plan spec; dismiss propagates same callback as xmark tap"

patterns-established:
  - "VOW card pattern: private enum with resolve(healthStore:) static func + private View struct with onDismiss closure — reusable pattern for future contextual nudges"

requirements-completed:
  - FEAT-01

# Metrics
duration: 4min
completed: 2026-06-12
---

# Phase 71 Plan 01: Coach VOW Nudge Card Summary

**CoachVOWNudge priority enum (4 cases) + CoachVOWCard dismissable view inserted between CoachJournalCard and CoachRoutesSection in CoachOverviewScreen, computing urgency from existing healthStore snapshots with nil-safe Double() parse**

## Performance

- **Duration:** 4 min
- **Started:** 2026-06-12T14:57:48Z
- **Completed:** 2026-06-12T15:02:05Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Added `CoachVOWNudge` private enum with four cases (`criticalRecovery`, `lowRecovery`, `highStrain`, `lowHRV`) and `@MainActor static func resolve(healthStore: HealthDataStore) -> CoachVOWNudge?` implementing threshold priority logic
- Added `CoachVOWCard` private struct with `coachCardSurface(tint:)` modifier, xmark dismiss button, swipe-down gesture (>30pt), `.accessibilityElement(children: .combine)`, and combined accessibility label
- Wired guarded card insertion in `CoachOverviewScreen.body` between `CoachJournalCard` and `CoachRoutesSection` via `@State private var vowDismissed = false`
- Build succeeded with zero errors under Swift 6 strict concurrency

## Task Commits

Each task was committed atomically:

1. **Task 1: Add CoachVOWNudge enum and CoachVOWCard view to CoachView.swift** — `2da1e46` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified
- `GooseSwift/CoachView.swift` — Added 101 lines: `CoachVOWNudge` enum (52 lines), `CoachVOWCard` struct (33 lines), `@State private var vowDismissed = false` + guarded insertion block (8 lines)

## Decisions Made
- `@MainActor` annotation on `CoachVOWNudge.resolve()` required by Swift 6 strict concurrency — `HealthDataStore.snapshot(for:)` is main-actor-isolated; initial implementation without annotation produced compile errors.
- `vowDismissed` is session-scoped only (no UserDefaults persistence) — per CONTEXT.md scope decision; card reappears on next app launch or tab re-enter.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Added @MainActor to CoachVOWNudge.resolve() for Swift 6 concurrency**
- **Found during:** Task 1 (build verification)
- **Issue:** `resolve(healthStore:)` called `healthStore.snapshot(for:)` which is main-actor-isolated. Without `@MainActor` on the static func, Swift 6 emitted "call to main actor-isolated instance method in a synchronous nonisolated context" for both recovery and strain snapshot calls.
- **Fix:** Added `@MainActor` annotation to `static func resolve(healthStore: HealthDataStore) -> CoachVOWNudge?`. The call site is always inside `CoachOverviewScreen.body` which is implicitly `@MainActor` via SwiftUI, so this is safe.
- **Files modified:** `GooseSwift/CoachView.swift`
- **Verification:** `xcodebuild ... build CODE_SIGNING_ALLOWED=NO` returned `BUILD SUCCEEDED` with zero errors
- **Committed in:** `2da1e46` (part of task commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — Swift 6 concurrency isolation)
**Impact on plan:** Fix required for correctness; no scope change. The `@MainActor` annotation is semantically correct because `resolve()` only reads from the main-actor-isolated `HealthDataStore` and is always called from SwiftUI body context.

## Known Stubs

None — all data sources are wired to live `healthStore.snapshot(for:)` and `HRVSeriesStore.shared.dailyEstimate()`. Card renders only when a threshold is crossed; no placeholder content.

## Threat Surface Scan

No new network endpoints, auth paths, file access patterns, or schema changes. VOW computation is read-only from existing in-memory HealthDataStore state. Threat model T-71-01-02 mitigation (nil-safe `Double()` parse) is implemented: `guard let r = Double(recoveryValue)` — parse failure returns `nil` (no nudge shown) rather than crash or incorrect threshold comparison.

## Issues Encountered

None beyond the auto-fixed Swift 6 concurrency annotation.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- FEAT-01 complete; `CoachVOWNudge.resolve()` and `CoachVOWCard` are in place and building
- Ready for Plan 02 (DATA-04 HR decimation) and subsequent plans in Phase 71
- No blockers

---
*Phase: 71-coach-vow-noopapp-notifications-hr-decimation*
*Completed: 2026-06-12*
