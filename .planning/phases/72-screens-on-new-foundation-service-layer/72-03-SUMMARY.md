---
plan: 72-03
phase: 72
status: complete
completed: 2026-06-12
---

# Plan 72-03 Summary: Swift Protocols + Mocks + Unit Tests (ARCH-01)

## What Was Built

Added three Swift protocol files, retroactive conformance for GooseRustBridge, and 5 new test files to the existing GooseSwiftTests target.

**Files created/modified:**
- `GooseSwift/GooseRustBridging.swift` — protocol with `request(method:args:)` and `requestAsync(method:args:)` + retroactive `extension GooseRustBridge: GooseRustBridging`
- `GooseSwift/GooseBLEManaging.swift` — protocol with key GooseBLEClient properties/methods used by GooseAppModel
- `GooseSwift/HealthDataStoring.swift` — protocol with HealthDataStore read methods including `fetchTrendsSeries(metricName:days:)`
- `GooseSwift/ManualWorkoutEntryViews.swift` — `WorkoutEntryViewModel.bridge` type changed from `GooseRustBridge` to `any GooseRustBridging` (enables injection)
- `GooseSwiftTests/MockRustBridge.swift` — records `lastMethod` + `lastArgs`; `stubbedResult` configurable
- `GooseSwiftTests/MockBLEClient.swift` — minimal GooseBLEManaging conformance
- `GooseSwiftTests/MockHealthStore.swift` — calls `bridge.requestAsync(method: "metric_series.query_range", ...)` in `fetchTrendsSeries` (proves bridge wiring, not stubbed bypass)
- `GooseSwiftTests/WorkoutEntryTests.swift` — 2 tests: `test_submit_calls_workout_upsert` + `test_submit_disabled_when_duration_zero`
- `GooseSwiftTests/TrendsFetchTests.swift` — 2 tests: `test_fetchTrendsSeries_calls_metric_series_query_range` + `test_workout_entry_calls_workout_upsert`
- `GooseSwift.xcodeproj/project.pbxproj` — 5 new test files registered at all 4 required locations (PBXBuildFile, PBXFileReference, PBXGroup children, PBXSourcesBuildPhase)

## Key Design Decisions

- `GooseSwiftTests` target already existed — no new target created, only new source files added
- `MockHealthStore.fetchTrendsSeries` calls `bridge.requestAsync("metric_series.query_range")` rather than returning stubbed data — proves method string wiring is correct
- Retroactive conformance on `GooseRustBridge` avoids changing the class signature, enabling adoption without disrupting existing call sites

## Verification

- xcodebuild BUILD SUCCEEDED (0 errors)
- `xcodebuild test -only-testing:GooseSwiftTests/WorkoutEntryTests -only-testing:GooseSwiftTests/TrendsFetchTests` → **4/4 passed**:
  - `TrendsFetchTests/test_fetchTrendsSeries_calls_metric_series_query_range` ✅
  - `TrendsFetchTests/test_workout_entry_calls_workout_upsert` ✅
  - `WorkoutEntryTests/test_submit_calls_workout_upsert` ✅
  - `WorkoutEntryTests/test_submit_disabled_when_duration_zero` ✅
