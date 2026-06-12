---
phase: 71-coach-vow-noopapp-notifications-hr-decimation
plan: "03"
subsystem: more-tab-features
tags: [feat, swiftui, interval-timer, metric-explorer, more-route, haptics]
dependency_graph:
  requires:
    - 71-01 (CoachVOWNudge — healthStore snapshot pattern confirmed)
    - 70 (buzz(loops:) in GooseBLEClient+Haptics, BreatheView Task pattern, MoreRoute.breathe)
  provides:
    - IntervalTimerView (FEAT-02 item 2)
    - MetricExplorerView (FEAT-02 item 3)
    - MoreRoute.intervalTimer and MoreRoute.metricExplorer wiring
  affects:
    - GooseSwift/MoreRouteModels.swift
    - GooseSwift/MoreDataStore.swift
    - GooseSwift/MoreView.swift
tech_stack:
  added:
    - IntervalTimerView — SwiftUI session view, Task @MainActor repeat loop, IntervalStepperRow
    - MetricExplorerView — SwiftUI List/Section with HealthDataStore snapshots + HeartRateSeriesStore RHR
  patterns:
    - BreatheView Task pattern for session loops (copy of Phase 70 pattern)
    - explicit parameter injection (healthStore: HealthDataStore) per MoreAlgorithmsView pattern
    - 4-location pbxproj registration per skill s1-118
    - 9-location MoreRoute wiring per skill s1-119
key_files:
  created:
    - GooseSwift/IntervalTimerView.swift
    - GooseSwift/MetricExplorerView.swift
  modified:
    - GooseSwift/MoreRouteModels.swift
    - GooseSwift/MoreDataStore.swift
    - GooseSwift/MoreView.swift
    - GooseSwift.xcodeproj/project.pbxproj
decisions:
  - "IntervalTimerView default durations: work=30s, rest=10s (CONTEXT.md discretion)"
  - "Countdown uses second-by-second loop (not one sleep per phase) for live display"
  - "MetricExplorerView sections: READINESS, SLEEP, STRESS, CARDIOVASCULAR, ENERGY"
  - "RHR shown as separate row from HeartRateSeriesStore.restingEstimate(), not a HealthRoute snapshot"
  - "MetricExplorerView allRows check deferred — always show List (MetricRow builds from snapshots that always return base values)"
  - "wellnessRoutes appended surgically: [.breathe, .intervalTimer] — .breathe preserved from Phase 70"
metrics:
  duration: "~25 minutes"
  completed: "2026-06-12"
  tasks_completed: 2
  tasks_total: 2
  files_changed: 6
---

# Phase 71 Plan 03: Interval Timer + Metric Explorer Summary

Implemented FEAT-02 items 2 and 3: IntervalTimerView (work/rest haptic interval trainer) and MetricExplorerView (scrollable metric value browser), both wired into the More tab via the established MoreRoute 4-switch pattern.

## What Was Built

### Task 1 — MoreRoute wiring for intervalTimer + metricExplorer

All 9 coordinated locations updated:

| Location | File | Change |
|----------|------|--------|
| enum MoreRoute | MoreRouteModels.swift | Added `case intervalTimer`, `case metricExplorer` |
| var title | MoreRouteModels.swift | "Interval Timer", "Metric Explorer" |
| var subtitle | MoreRouteModels.swift | Descriptions for both cases |
| var systemImage | MoreRouteModels.swift | "timer", "chart.bar.doc.horizontal" |
| var statusKeyPath | MoreRouteModels.swift | `\.intervalTimer`, `\.metricExplorer` |
| static wellnessRoutes | MoreRouteModels.swift | Appended `.intervalTimer` (`.breathe` preserved) |
| static dataRoutes | MoreRouteModels.swift | New: `[.metricExplorer]` |
| struct MoreRouteStatus | MoreRouteModels.swift | Added `var intervalTimer`, `var metricExplorer` fields |
| MoreDataStore init | MoreDataStore.swift | `intervalTimer: .ready, metricExplorer: .ready` |
| MoreDataStore refreshRouteStatus | MoreDataStore.swift | Same defaults in refresh return |
| Section("Data") | MoreView.swift | New section between Wellness and Settings |
| destination(for:) | MoreView.swift | `case .intervalTimer: IntervalTimerView()`, `case .metricExplorer: MetricExplorerView(healthStore: healthStore)` |

Commit: `8dcc30b`

### Task 2 — IntervalTimerView.swift + MetricExplorerView.swift

**IntervalTimerView** (`GooseSwift/IntervalTimerView.swift`):
- `private enum IntervalPhase { case work, rest }`
- Config state: `workSeconds = 30`, `restSeconds = 10`
- Session state: `isRunning`, `phaseTask: Task<Void, Never>?`, `countdownSeconds`
- `IntervalStepperRow` private subview with min/max/step stepper
- `startSession()`: `Task { @MainActor in repeat { work-loop; rest-loop } while !Task.isCancelled }`
- Second-by-second countdown via `for _ in 0..<workSeconds { try? await Task.sleep(for: .seconds(1)); guard !Task.isCancelled else { break }; countdownSeconds -= 1 }`
- `model.ble.buzz(loops: 1)` at each work→rest and rest→work transition
- `stopSession()` cancels `phaseTask`, resets state
- `.onDisappear { stopSession() }` prevents Task leak (T-71-03-01 mitigated)
- BLE hint row shown when not running and not connected
- Stop button: `FitnessColor.endRed` tint; Start button: `FitnessColor.standCyan` tint
- Accessibility labels on countdown and both buttons

**MetricExplorerView** (`GooseSwift/MetricExplorerView.swift`):
- `var healthStore: HealthDataStore` — explicit parameter, NOT `@EnvironmentObject` (T-71-03-03, Pitfall 6)
- `private struct MetricRow: Identifiable` — id, name, systemImage, tint, displayValue, timestamp
- Sections: READINESS (recovery, strain), SLEEP (sleep), STRESS (stress), CARDIOVASCULAR (cardioLoad + RHR from `HeartRateSeriesStore.shared.restingEstimate()`), ENERGY (energyBank)
- RHR: special-cased from `HeartRateSeriesStore.shared.restingEstimate()` (no `.rhr` HealthRoute exists — per research open question)
- `timestampLabel(for:)` formats hours-ago from `HeartRateRestingEstimate.updatedAt`
- `ContentUnavailableView` empty state shown when `allRows.isEmpty`
- `.accessibilityElement(children: .combine)` + `.accessibilityLabel(...)` on each row
- `.gooseListBackground()`, `.listStyle(.insetGrouped)`, `.toolbar(.hidden, for: .tabBar)`

Both files registered in `project.pbxproj` at all 4 locations (PBXBuildFile, PBXFileReference, PBXGroup children, PBXSourcesBuildPhase).

Commit: `9cb1f3c`

## Deviations from Plan

None — plan executed exactly as written.

The plan noted `.breathe` must be verified before appending `.intervalTimer` to `wellnessRoutes`. Confirmed `.breathe` was already present; appended surgically. Result: `[.breathe, .intervalTimer]`.

## Known Stubs

None. All metric rows pull live data from `healthStore.snapshot(for:)` and `HeartRateSeriesStore.shared.restingEstimate()`. The "No data" timestamp for HealthRoute snapshots is an accepted limitation — snapshot structs do not expose an `updatedAt` field; only `HeartRateRestingEstimate` does.

## Threat Surface Scan

No new network endpoints, auth paths, or file access patterns introduced. Both views are read-only consumers of existing local data stores. Threat register items T-71-03-01, T-71-03-02, T-71-03-03 all mitigated as planned.

## Self-Check: PASSED

- FOUND: GooseSwift/IntervalTimerView.swift
- FOUND: GooseSwift/MetricExplorerView.swift
- FOUND: commit 8dcc30b (Task 1 — MoreRoute wiring)
- FOUND: commit 9cb1f3c (Task 2 — view files + pbxproj)
