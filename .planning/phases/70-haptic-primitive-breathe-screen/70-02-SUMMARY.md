---
phase: 70-haptic-primitive-breathe-screen
plan: "02"
subsystem: ui
tags: [swiftui, breathe, haptics, animation, ble, more-tab]

requires:
  - phase: 70-haptic-primitive-breathe-screen
    provides: buzz(loops:) BLE haptic primitive on GooseBLEClient

provides:
  - BreatheView SwiftUI screen with 4s/4s/4s box-breathing session loop
  - BreathePhase enum (inhale/hold/exhale) with 4s duration constant
  - MoreRoute.breathe navigation case + wellnessRoutes group
  - MoreRouteStatus.breathe property (always .ready)
  - Wellness section in MoreView above Settings

affects:
  - HAP-03 (future interval timer can follow same BreatheView session loop pattern)
  - HAP-04 (wake-window alarm — same MoreRoute pattern for navigation)

tech-stack:
  added: []
  patterns:
    - "Task { @MainActor in repeat/while !Task.isCancelled } for cancellable SwiftUI session loops"
    - ".onDisappear { stopSession() } to cancel phaseTask on back-navigation (zombie task prevention)"
    - "@Environment(GooseAppModel.self) — @Observable model access in destination views"
    - "MoreRoute enum case + wellnessRoutes static group + MoreRouteStatus property for navigation wiring"

key-files:
  created:
    - GooseSwift/BreatheView.swift
  modified:
    - GooseSwift/MoreRouteModels.swift
    - GooseSwift/MoreView.swift
    - GooseSwift/MoreDataStore.swift
    - GooseSwift.xcodeproj/project.pbxproj

key-decisions:
  - "connectionState check uses != 'ready' (lowercase) confirmed from RESEARCH.md — never 'Connected'"
  - "phaseTask is @State Task<Void,Never>? — cancelled on stopSession() and .onDisappear"
  - "Disconnected banner shown only when !isRunning — hides during session to avoid distraction"
  - "BreatheView.swift registered in project.pbxproj with IDs D200000000000000000000060 / D100000000000000000000060"

requirements-completed:
  - HAP-02

duration: 25min
completed: 2026-06-12
---

# Phase 70 Plan 02: Breathe Screen Summary

**BreatheView with 4s/4s/4s box-breathing loop (circle 0.6→1.0→0.6 scaleEffect), buzz(loops:1) at each phase start, Wellness section in MoreView, full navigation wiring via MoreRoute.breathe**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-06-12T13:30:00Z
- **Completed:** 2026-06-12T13:55:29Z
- **Tasks:** 2
- **Files modified:** 5 (1 created, 4 modified + project.pbxproj)

## Accomplishments

- Created `BreatheView.swift` with complete box-breathing session state machine — inhale/hold/exhale 4s each, `Task { @MainActor in repeat/while }` pattern, `.onDisappear` cancellation, reduced motion guard
- Wired `MoreRoute.breathe` with title/subtitle/systemImage/statusKeyPath and `wellnessRoutes` group; added `var breathe: MoreStatusKind` to `MoreRouteStatus` struct
- Updated both `MoreRouteStatus` construction sites in `MoreDataStore` with `breathe: .ready`
- Added Wellness section above Settings in `MoreView` and `case .breathe: BreatheView()` in `destination(for:)` switch
- Registered `BreatheView.swift` and `GooseBLEClient+Haptics.swift` in `project.pbxproj` (PBXBuildFile + PBXFileReference + PBXGroup + PBXSourcesBuildPhase)
- BUILD SUCCEEDED with zero compiler errors

## Task Commits

1. **Task 1: Update MoreRouteModels + MoreDataStore** — `1cdc335` (feat)
2. **Task 2: Create BreatheView + Wellness section in MoreView** — `c0e5af1` (feat)

## Files Created/Modified

- `GooseSwift/BreatheView.swift` — BreatheView screen + BreathePhase enum; full box-breathing session loop
- `GooseSwift/MoreRouteModels.swift` — case breathe + wellnessRoutes + var breathe: MoreStatusKind
- `GooseSwift/MoreView.swift` — Section("Wellness") above Settings + case .breathe destination arm
- `GooseSwift/MoreDataStore.swift` — breathe: .ready at both MoreRouteStatus construction sites
- `GooseSwift.xcodeproj/project.pbxproj` — BreatheView.swift + GooseBLEClient+Haptics.swift registered

## Decisions Made

- `connectionState != "ready"` (lowercase) used for disconnected banner — confirmed from RESEARCH.md pitfall 1; "Connected" (capital C) would silently break the banner
- `Task { @MainActor in ... }` annotation ensures `withAnimation` runs on main thread — no explicit `DispatchQueue.main` needed
- Disconnected banner hidden during active session (`!isRunning && connectionState != "ready"`) — avoids distraction during breathwork
- `GooseBLEClient+Haptics.swift` was already implemented by Plan 70-01 (Wave 1 parallel); only project.pbxproj registration was needed

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added BreatheView.swift and GooseBLEClient+Haptics.swift to project.pbxproj**
- **Found during:** Task 2 (build verification)
- **Issue:** New Swift files created on disk but not registered in Xcode project — build would fail with "No such module" or files simply ignored
- **Fix:** Added PBXBuildFile, PBXFileReference, PBXGroup, and PBXSourcesBuildPhase entries for both files using next available IDs (0x60, 0x61 in the D-series sequence)
- **Files modified:** GooseSwift.xcodeproj/project.pbxproj
- **Verification:** xcodebuild BUILD SUCCEEDED after registration
- **Committed in:** c0e5af1 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 3 — blocking build issue)
**Impact on plan:** Necessary infrastructure fix; no scope creep. GooseBLEClient+Haptics.swift was already implemented by Plan 70-01 as planned.

## Issues Encountered

- xcodebuild destination `name=iPhone 16` unavailable (iOS 26 SDK simulator naming changed) — switched to `name=iPhone 17` which succeeded immediately. No impact on output.

## Known Stubs

None — BreatheView is fully wired to `model.ble.buzz(loops:)` and `model.ble.connectionState`. No placeholder data or hardcoded empty values.

## Threat Flags

No new threat surface beyond what was analyzed in the plan's `<threat_model>`. T-70-03 (zombie task) mitigated by `.onDisappear { stopSession() }`. T-70-04 (connectionState string) mitigated by confirmed lowercase "ready" usage. T-70-05 (withAnimation off main actor) mitigated by `Task { @MainActor in ... }`.

## Next Phase Readiness

- HAP-02 complete; `BreatheView` accessible via More > Wellness > Breathe
- `buzz(loops:)` (HAP-01) + `BreatheView` (HAP-02) both shipped — Phase 70 feature complete
- HAP-03 (Interval Timer) can follow identical session loop + MoreRoute navigation pattern
- HAP-04 (Wake-Window Alarm) requires RE gate before implementation

## Self-Check: PASSED

- `GooseSwift/BreatheView.swift` exists on disk
- `GooseSwift/GooseBLEClient+Haptics.swift` exists on disk
- Commits 1cdc335 and c0e5af1 verified in git log
- BUILD SUCCEEDED confirmed

---
*Phase: 70-haptic-primitive-breathe-screen*
*Completed: 2026-06-12*
