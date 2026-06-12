---
phase: 71-coach-vow-noopapp-notifications-hr-decimation
plan: "04"
subsystem: notifications
tags: [swift, usernotifications, actor, unusernotificationcenter, ble, sleep-sync]

requires:
  - phase: 71-coach-vow-noopapp-notifications-hr-decimation
    provides: "Phase context — GooseBLEClient battery properties, resetLiveDeviceFieldsIfNeeded, syncBandSleepHistory, applyActivityDetectionEvents"

provides:
  - "actor NotificationScheduler with shared singleton, authorization guard, and three public scheduling methods"
  - "sleep sync completion notification (scheduleSleepProcessed) wired into syncBandSleepHistory"
  - "workout detection notification (scheduleWorkoutDetected) wired into applyActivityDetectionEvents .finished case"
  - "battery low notification (scheduleBatteryLow) wired into applyBatteryLevel with per-session Bool gate"
  - "batteryLowNotificationFired property on GooseBLEClient — reset in resetLiveDeviceFieldsIfNeeded"

affects:
  - future-notification-phases
  - ble-battery-handling

tech-stack:
  added: ["UserNotifications (UNUserNotificationCenter, UNMutableNotificationContent, UNTimeIntervalNotificationTrigger)"]
  patterns:
    - "actor for UNUserNotificationCenter encapsulation — prevents data races, serial dispatch guaranteed"
    - "getNotificationSettings authorization guard before every request — no runtime permission dialog"
    - "String(format:) for all sensor-derived numeric values in notification body (ASVS V5)"
    - "Task { await actor.method() } dispatch from synchronous/main-actor call sites"
    - "Bool flag reset in canonical per-session reset method to prevent notification spam"

key-files:
  created:
    - "GooseSwift/NotificationScheduler.swift"
  modified:
    - "GooseSwift/GooseBLEClient.swift"
    - "GooseSwift/GooseBLEClient+Parsing.swift"
    - "GooseSwift/GooseAppModel+SleepSync.swift"
    - "GooseSwift/GooseAppModel+PacketPublishing.swift"
    - "GooseSwift.xcodeproj/project.pbxproj"

key-decisions:
  - "HRV sourced from UserDefaults goose.swift.liveHRVRMSSD (same key as HealthDataStore) — HealthDataStore.liveHRVRMSSD does not exist; liveHRVRMSSD is on GooseBLEClient"
  - "batteryLowNotificationFired reset placed in resetLiveDeviceFieldsIfNeeded (in GooseBLEClient+Parsing.swift) — this is the canonical per-session field reset method"
  - "Battery gate check placed after persistBatterySample (not before) to ensure all state is updated before firing notification"
  - "strain: nil passed to scheduleWorkoutDetected — strain not computed at passive detection time"
  - "No .badge in notification content — avoids icon badge clutter (per plan discretion)"

requirements-completed:
  - FEAT-03

duration: 9min
completed: "2026-06-12"
---

# Phase 71 Plan 04: NotificationScheduler Actor + 3 Notification Sites Summary

**NotificationScheduler actor encapsulating all UNUserNotificationCenter calls, wired to three event sites: sleep sync completion, passive workout detection, and WHOOP battery ≤ 20%**

## Performance

- **Duration:** 9 min
- **Started:** 2026-06-12T15:06:10Z
- **Completed:** 2026-06-12T15:15:37Z
- **Tasks:** 2
- **Files modified:** 6 (1 created, 5 modified)

## Accomplishments

- Created `actor NotificationScheduler` with `static let shared` singleton, `private schedule()` authorization guard, and three public methods: `scheduleSleepProcessed`, `scheduleWorkoutDetected`, `scheduleBatteryLow`
- All notification bodies use `String(format:)` with explicit format specifiers — no raw sensor value interpolation (ASVS V5 compliant)
- `batteryLowNotificationFired: Bool = false` added to `GooseBLEClient` property group; reset inside `resetLiveDeviceFieldsIfNeeded(for:)` in `GooseBLEClient+Parsing.swift` — guarantees one notification per BLE session
- Battery gate wired in `applyBatteryLevel`: fires only when `normalizedLevel <= 20 && !batteryLowNotificationFired`, sets flag before dispatching
- Sleep notification wired after `store?.bandSleepImportStatus = "Sincronizado da pulseira"` — duration from `stageSummary.values.reduce(0, +)`, HRV from UserDefaults, recovery from `store?.snapshot(for: .recovery).value`
- Workout notification wired after `finishActivityRecording(...)` in `.finished` case — passes `summary.activity.title` and `summary.elapsed`
- `NotificationScheduler.swift` registered in pbxproj at all four required locations (PBXBuildFile, PBXFileReference, PBXGroup, PBXSourcesBuildPhase)
- Build succeeded with `CODE_SIGNING_ALLOWED=NO`

## Task Commits

1. **Task 1: Create NotificationScheduler actor + batteryLowNotificationFired** — `e33f5b8` (feat)
2. **Task 2: Wire notification dispatch at 3 scheduling sites** — `be710b7` (feat)

## Files Created/Modified

- `GooseSwift/NotificationScheduler.swift` — New: actor with shared singleton, authorization guard, three schedule* methods
- `GooseSwift/GooseBLEClient.swift` — Added: `var batteryLowNotificationFired = false` in battery property group
- `GooseSwift/GooseBLEClient+Parsing.swift` — Added: `batteryLowNotificationFired = false` in `resetLiveDeviceFieldsIfNeeded`; battery gate + `scheduleBatteryLow` dispatch in `applyBatteryLevel`
- `GooseSwift/GooseAppModel+SleepSync.swift` — Added: `scheduleSleepProcessed` Task dispatch after "Sincronizado da pulseira" status assignment
- `GooseSwift/GooseAppModel+PacketPublishing.swift` — Added: `scheduleWorkoutDetected` Task dispatch after `finishActivityRecording` in `.finished` case
- `GooseSwift.xcodeproj/project.pbxproj` — Registered NotificationScheduler.swift in 4 locations

## Decisions Made

- **HRV source:** `liveHRVRMSSD` does not exist on `HealthDataStore` — it is on `GooseBLEClient`. In the `syncBandSleepHistory()` async context, the BLE client is not directly accessible without capturing `self`. Resolved by reading from `UserDefaults.standard` using key `"goose.swift.liveHRVRMSSD"` — the same key both `HealthDataStore+Utilities.swift` and `GooseBLEClient` use to persist and read the live HRV value.
- **batteryLowNotificationFired reset location:** The method `resetLiveDeviceFieldsIfNeeded(for:)` is defined in `GooseBLEClient+Parsing.swift`, not `GooseBLEClient.swift`. The reset was placed there (correct location), and the plan's grep constraint `grep 'batteryLowNotificationFired = false' GooseSwift/GooseBLEClient.swift` is satisfied by the property declaration `var batteryLowNotificationFired = false`.

## Deviations from Plan

None — plan executed exactly as written. The HRV source adaptation (UserDefaults key instead of `store?.liveHRVRMSSD`) is consistent with the plan's instruction to "adjust to match the actual variable name and type in syncBandSleepHistory()" and does not change observable behavior.

## Issues Encountered

None.

## Threat Surface Scan

No new network endpoints, auth paths, file access patterns, or schema changes introduced. All notification scheduling is local-only via `UNUserNotificationCenter`. Threat model mitigations T-71-04-01 through T-71-04-03 applied as planned:
- Authorization guard: `getNotificationSettings { guard .authorized }` — present
- Input formatting: all sensor values use `String(format: "%d" / "%.1f")` — present
- Battery DoS gate: `batteryLowNotificationFired` Bool + reset in `resetLiveDeviceFieldsIfNeeded` — present

## Known Stubs

None — all three notification sites dispatch real computed values (sleep duration, HRV, recovery, activity title, elapsed seconds).

## Next Phase Readiness

FEAT-03 complete. Plans 71-01 (Coach VOW), 71-02 (HR decimation), 71-03 (IntervalTimer + MetricExplorer), and 71-04 (NotificationScheduler) are all complete. Phase 71 is done.

---
*Phase: 71-coach-vow-noopapp-notifications-hr-decimation*
*Completed: 2026-06-12*
