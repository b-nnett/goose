---
phase: 73-smart-alarm-wake-window-engine
plan: 02
subsystem: ui
tags: [swift, xcode, pbxproj, stub, re-gate, alarm]

# Dependency graph
requires:
  - phase: 73-01
    provides: HAP-03 alarm UI wiring (GooseAppModel properties + CoachSleepRouteView section)
provides:
  - GooseWakeWindowManager.swift stub — compilable HAP-04 RE-gated class with BTSnoop + Ghidra prerequisite documentation
  - project.pbxproj registration at 4 locations (PBXBuildFile, PBXFileReference, PBXGroup children, PBXSourcesBuildPhase)
affects: [73-03, future-wake-window-implementation]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "RE-gate stub pattern: compilable empty class with doc comment listing exact prerequisites before functional implementation is permitted"
    - "E1/E2 UUID scheme: highest used index was 13 (hex); GooseWakeWindowManager.swift assigned NN=14"

key-files:
  created:
    - GooseSwift/GooseWakeWindowManager.swift
  modified:
    - GooseSwift.xcodeproj/project.pbxproj

key-decisions:
  - "UUID NN=14 chosen after grepping all existing E1/E2 entries (highest used: 13 hex); verified collision-free before inserting"
  - "Stub inserted adjacent to GooseStrainAccumulator.swift at all 4 pbxproj locations for consistent ordering"
  - "Build used iPhone 17 Pro simulator (id 95142C9B) — iPhone 16 Pro not available in this environment"

patterns-established:
  - "RE-gate stub: class body contains only a single-line comment explaining the gate; no properties, methods, or protocol conformances"

requirements-completed: [HAP-04]

# Metrics
duration: 5min
completed: 2026-06-12
---

# Phase 73 Plan 02: GooseWakeWindowManager Stub Summary

**GooseWakeWindowManager.swift RE-gated stub — empty compilable class with BTSnoop/Ghidra prerequisite doc comment, registered at 4 pbxproj locations, xcodebuild succeeds**

## Performance

- **Duration:** 5 min
- **Started:** 2026-06-12T18:18:28Z
- **Completed:** 2026-06-12T18:24:14Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Created `GooseWakeWindowManager.swift` with exact RE-gate doc comment specifying BTSnoop capture of STRAP_DRIVEN_ALARM_EXECUTED and Ghidra decompilation of SetAlarmInfoCommandPacketRev4 as prerequisites
- Registered the stub at all 4 required project.pbxproj locations using UUID pair E1/E2 ...014 (collision-free, next after highest existing index 13 hex)
- Verified `grep -c 'GooseWakeWindowManager.swift' project.pbxproj` returns exactly 4
- xcodebuild BUILD SUCCEEDED — stub compiles into GooseSwift target

## Task Commits

Each task was committed atomically:

1. **Task 1 + Task 2: Create stub + register in pbxproj** - `12b84c2` (feat)

**Plan metadata:** (see final commit below)

## Files Created/Modified
- `GooseSwift/GooseWakeWindowManager.swift` — HAP-04 RE-gated stub; `final class GooseWakeWindowManager` with empty body and doc comment listing BTSnoop + Ghidra prerequisites
- `GooseSwift.xcodeproj/project.pbxproj` — 4 new entries: PBXBuildFile (E1...014), PBXFileReference (E2...014), PBXGroup children, PBXSourcesBuildPhase files

## Decisions Made
- Tasks 1 and 2 committed together since the stub file is meaningless without pbxproj registration (one atomic unit of value)
- UUID discovery found highest existing index was `13` hex (GooseStrainAccumulator=10, NotificationScheduler=11, and others up to 13); chose `14` as next free slot
- Used `-derivedDataPath /tmp/goose-73-02 CODE_SIGNING_ALLOWED=NO` for xcodebuild (Xcode may be open holding build DB lock)

## Deviations from Plan

None — plan executed exactly as written. The stub content, UUID selection protocol, and 4-location registration all match the plan specification.

## Issues Encountered
- iPhone 16 Pro simulator not available in this environment; used iPhone 17 Pro (id `95142C9B-50CA-421B-A74D-DD622C4ACF66`) instead. Build result is identical — simulator selection does not affect compilation outcome for a stub file.

## User Setup Required
None — no external service configuration required.

## Next Phase Readiness
- HAP-04 structural deliverable complete: `GooseWakeWindowManager.swift` is in the build target, compiles, and documents the RE gate clearly
- Functional implementation of HAP-04 is blocked until:
  1. BTSnoop capture of `STRAP_DRIVEN_ALARM_EXECUTED` packets is documented in `.planning/research/whoop-re/SetAlarmInfoCommandPacketRev4.md`
  2. Ghidra decompilation of `SetAlarmInfoCommandPacketRev4` field layout is documented in the same file
- Phase 73 plan 01 (HAP-03 alarm UI) is the sibling plan for this phase — check its status separately

---
*Phase: 73-smart-alarm-wake-window-engine*
*Completed: 2026-06-12*
