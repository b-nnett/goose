---
phase: 72
plan: 02
subsystem: ui
tags: [swift, swiftui, health-ui, data-03, trends, manual-workout, ans]
requires: [69-01, 69-02]
provides: [TrendsDashboardView, ManualWorkoutEntrySheet, ANS-tiles, HealthRoute.trends]
affects: [HealthModels, HealthDataStore, HealthDashboardViews, HealthView, HealthRecoveryStressViews]
tech-stack:
  added: []
  patterns:
    - SwiftUI Path polyline sparkline (no Charts.framework)
    - SleepV2StatCard composition for ANS tiles
    - WorkoutEntryViewModel with requestAsync bridge call
    - fetchTrendsSeries async method on HealthDataStore
key-files:
  created:
    - GooseSwift/TrendsDashboardViews.swift
    - GooseSwift/ManualWorkoutEntryViews.swift
  modified:
    - GooseSwift/HealthModels.swift
    - GooseSwift/HealthRecoveryStressViews.swift
    - GooseSwift/HealthDashboardViews.swift
    - GooseSwift/HealthView.swift
    - GooseSwift/HealthDataStore.swift
    - GooseSwift/HealthDataStore+Snapshots.swift
    - GooseSwift/HealthDataStore+StaticSnapshots.swift
    - GooseSwift.xcodeproj/project.pbxproj
key-decisions:
  - Used SleepV2StatCard directly for ANS tiles (no new ANSMetricTile struct — UI spec confirmed composing SleepV2StatCard is sufficient)
  - ActivityKind uses .title not .displayName — code uses kind.title in ForEach
  - WorkoutEntryViewModel uses GooseRustBridge (concrete) not GooseRustBridging protocol — protocol lands in Plan 72-03; added TODO(72-03) comment
  - fetchTrendsSeries added to HealthDataStore+Snapshots.swift extension (method only, no stored properties — stored properties added to main HealthDataStore.swift class body)
  - restingEstimate() returns HeartRateRestingEstimate? with .bpm: Double — used estimate.bpm.rounded() correctly
requirements-completed: [DATA-03]
duration: 17 min
completed: 2026-06-12
---

# Phase 72 Plan 02: ANS Tiles + TrendsDashboardView + ManualWorkoutEntrySheet Summary

Three DATA-03 screens: ANS Balance tiles in Stress view using SleepV2StatCard + HRVSeriesStore/HeartRateSeriesStore, TrendsDashboardView with SwiftUI Path sparklines for Recovery/HRV/Strain, and ManualWorkoutEntrySheet calling workout.upsert via bridge.requestAsync.

**Duration:** 17 min (2026-06-12T16:33:22Z → 2026-06-12T16:50:43Z)
**Tasks:** 2/2 completed
**Files:** 2 created, 8 modified

## Tasks Completed

### Task 1: HealthRoute.trends + ANS tiles in StressV2OverviewPage

- Added `case trends` to `HealthRoute` enum in `HealthModels.swift` with title "Trends" and systemImage "chart.line.uptrend.xyaxis"
- Added `case .trends: TrendsDashboardView(store: store)` arm to `HealthRouteContentView` switch in `HealthDashboardViews.swift`
- Added `.trends` to front of the Explore Health array in `HealthView.swift`
- Added `trends` entry to `baseLandingSnapshots` in `HealthDataStore+StaticSnapshots.swift` (source: `.bridge("metric_series.query_range")`, status: "Recovery · HRV · Strain")
- Added trend cache stored properties to `HealthDataStore.swift` main class body: `recoveryTrendPoints`, `hrvTrendPoints`, `strainTrendPoints`
- Added `fetchTrendsSeries(metricName:days:)` async method to `HealthDataStore+Snapshots.swift` calling `bridge.requestAsync("metric_series.query_range")`
- Inserted ANS Balance section into `StressV2OverviewPage.body` after `StressV2BreakdownSection` and before Trends `SleepV2SectionHeader`, using two `SleepV2StatCard` tiles in an HStack with `.frame(height: 96)`

### Task 2: TrendsDashboardViews.swift + ManualWorkoutEntryViews.swift + pbxproj registration

- Created `TrendsDashboardViews.swift` with `TrendsDashboardView`, `TrendsSparklineCard`, `TrendsSparklineShape`, `TrendsSparklineFillShape` — all using SwiftUI `Path` (no Charts.framework)
- Created `ManualWorkoutEntryViews.swift` with `WorkoutEntryViewModel`, `ManualWorkoutEntrySheet`, `EffortScaleSelector` — WorkoutEntryViewModel calls `bridge.requestAsync("workout.upsert")` with sport, duration_s, and `perceived_effort: N` in notes
- Registered both files in 4 locations each in `project.pbxproj` (PBXBuildFile, PBXFileReference, PBXGroup children, PBXSourcesBuildPhase files) using IDs `E10000000000000000000010/E20000000000000000000010` and `E10000000000000000000011/E20000000000000000000011`

## Verification Results

| Check | Result |
|-------|--------|
| `xcodebuild BUILD SUCCEEDED` | PASS |
| `case trends` in HealthModels.swift | 1 |
| `case .trends:` in HealthDashboardViews.swift | 1 |
| `ANS Balance` in HealthRecoveryStressViews.swift | 1 |
| `fetchTrendsSeries` in HealthDataStore+Snapshots.swift | 1 |
| `TrendsDashboardViews.swift` in pbxproj | 4 |
| `ManualWorkoutEntryViews.swift` in pbxproj | 4 |
| `workout.upsert` in ManualWorkoutEntryViews.swift | 1 |
| `metric_series.query_range` in HealthDataStore+Snapshots.swift | 1 |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Typo in TrendsDashboardViews.swift stroke style**
- **Found during:** Task 2 file creation
- **Issue:** Write call contained `StrokeSound` typo in the `.stroke(tint, style:)` call
- **Fix:** Corrected to `StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round)`
- **Files modified:** `GooseSwift/TrendsDashboardViews.swift`
- **Commit:** 0eb848e

**2. [Rule 1 - Adaptation] ActivityKind.displayName does not exist**
- **Found during:** Task 2 review of ActivityModels.swift
- **Issue:** Plan referenced `kind.displayName` but `ActivityKind` only has `title` property
- **Fix:** Used `kind.title` in `ManualWorkoutEntryViews.swift` ForEach
- **Files modified:** `GooseSwift/ManualWorkoutEntryViews.swift`
- **Commit:** 0eb848e

**3. [Rule 1 - Adaptation] HealthDataSource.bridge is a factory method, not a bare enum case**
- **Found during:** Task 1 baseLandingSnapshots addition
- **Issue:** Plan wrote `source: .bridge` but `HealthDataSource` uses `static func bridge(_ detail: String)` factory
- **Fix:** Used `.bridge("metric_series.query_range")` with the required detail string
- **Files modified:** `GooseSwift/HealthDataStore+StaticSnapshots.swift`
- **Commit:** 0eb848e

**Total deviations:** 3 auto-fixed (1 typo fix, 2 API adaptation). **Impact:** None — all addressed inline before commit.

## Known Stubs

- `fetchTrendsSeries` returns data from `metric_series.query_range` Rust bridge method. If the Rust bridge does not yet implement this method, the TrendsDashboardView will display "No data for the last 7 days" (graceful empty state via `try?`). The Rust implementation is Phase 69 work and may need verification.
- `WorkoutEntryViewModel` uses `GooseRustBridge` directly (concrete type). The `GooseRustBridging` protocol seam lands in Plan 72-03 — the TODO comment marks the upgrade point.

## Threat Surface Scan

No new network endpoints or auth paths introduced. `workout.upsert` args are validated at the UI layer:
- `durationMinutes` clamped to 1–600 by Stepper range (T-72-03 mitigated)
- `sport` constrained to `ActivityKind.allCases` enum (T-72-03 mitigated)
- `effortValue` constrained 1–10 by `EffortScaleSelector` (T-72-03 mitigated)
- `bridge.requestAsync` used exclusively — no synchronous `bridge.request()` from `@MainActor` (T-72-04 mitigated)

## Self-Check: PASSED

- [x] `GooseSwift/TrendsDashboardViews.swift` exists on disk
- [x] `GooseSwift/ManualWorkoutEntryViews.swift` exists on disk
- [x] Commit `0eb848e` exists: `git log --oneline | grep 0eb848e` → confirmed
- [x] BUILD SUCCEEDED (verified twice)
- [x] All 9 acceptance criteria pass

**Next:** Ready for 72-03 (ARCH-01 protocols + test target + mocks)
