---
phase: 73-smart-alarm-wake-window-engine
plan: "01"
subsystem: alarm-ui
tags: [swiftui, ble, alarm, hap-03, sleep-coach]
requires: [HAP-01, HAP-02]
provides: [HAP-03]
affects: [CoachSleepRouteView, GooseAppModel]
tech-stack:
  added: []
  patterns:
    - "@Environment(GooseAppModel.self) injection into CoachSleepRouteView"
    - "CoachInfoGroup card pattern for Wake Alarm section"
    - "isDisconnected computed property for BLE gate"
key-files:
  created: []
  modified:
    - GooseSwift/GooseAppModel.swift
    - GooseSwift/GooseAppModel+Lifecycle.swift
    - GooseSwift/CoachRouteViews.swift
key-decisions:
  - "Used setWhoopAlarm(at:) and disableWhoopAlarms() wrappers — not writeAlarmCommand directly — per RESEARCH.md pitfall 1"
  - "alarmIsArmed = false added to handleBLEConnectionStateChange non-ready branch in GooseAppModel+Lifecycle.swift"
  - "Single toggle button (Armar/Cancelar) rather than two separate buttons — reduces UI surface area"
  - "isDisconnected = connectionState != 'ready' (not canWriteAlarm) for disabled predicate — canWriteAlarm is for internal BLE guards, not for UI gate per RESEARCH.md pitfall 4 note"
requirements-completed: [HAP-03]
duration: "5 min"
completed: "2026-06-12"
---

# Phase 73 Plan 01: Wake Alarm UI Summary

HAP-03 Wake Alarm section wired into Sleep Coach: DatePicker + Armar/Cancelar button backed by setWhoopAlarm(at:) / disableWhoopAlarms() BLE wrappers, with alarmIsArmed state on GooseAppModel and BLE-disconnect reset.

**Duration:** 5 min | **Started:** 2026-06-12T18:19:04Z | **Completed:** 2026-06-12T18:24:17Z
**Tasks:** 2/2 | **Files modified:** 3

## Tasks Completed

| # | Name | Commit | Files |
|---|------|--------|-------|
| 1 | Add alarm state properties to GooseAppModel | f2a5c43 | GooseAppModel.swift, GooseAppModel+Lifecycle.swift |
| 2 | Add Wake Alarm section to CoachSleepRouteView | d4628ab | CoachRouteViews.swift |

## What Was Built

**GooseAppModel.swift** — Two new stored properties added after `liveWorkoutStrain`:
- `var scheduledAlarmTime: Date? = nil    // HAP-03`
- `var alarmIsArmed: Bool = false         // HAP-03`

**GooseAppModel+Lifecycle.swift** — `alarmIsArmed = false` added to the non-ready branch of `handleBLEConnectionStateChange(_:)` so that an armed alarm is automatically cleared when the strap disconnects (prevents showing "armed" state for an alarm that may not fire).

**CoachRouteViews.swift** — `CoachSleepRouteView` now:
- Injects `@Environment(GooseAppModel.self) private var model`
- Holds `@State private var alarmTime` initialised to 07:00 local
- Appends `wakeAlarmSection` (a `@ViewBuilder` computed property) at the bottom of the VStack body
- `wakeAlarmSection` contains a `CoachInfoGroup(title: "ALARME DE DESPERTAR")` with:
  - `DatePicker` (.hourAndMinute, labelsHidden), disabled when disconnected OR alarm already armed, opacity 0.4 when disabled
  - Status `HStack` ("Conecta o WHOOP para usar o alarme") shown only when disconnected and not armed
  - Single toggle button "Armar Alarme" / "Cancelar Alarme" — indigo fill when disarmed, red fill when armed
  - Button disabled when `isDisconnected`; `accessibilityLabel` provided for both states

## Verification Results

| Check | Command | Result |
|-------|---------|--------|
| V1: scheduledAlarmTime + alarmIsArmed in GooseAppModel | grep -n "scheduledAlarmTime\|alarmIsArmed" GooseAppModel.swift | PASS — lines 34-35 |
| V2: ALARME DE DESPERTAR section title | grep -n "ALARME DE DESPERTAR" CoachRouteViews.swift | PASS — line 149 |
| V3: setWhoopAlarm + disableWhoopAlarms in view | grep -n "setWhoopAlarm\|disableWhoopAlarms" CoachRouteViews.swift | PASS — lines 174, 178 |
| V4: xcodebuild succeeds | xcodebuild build -scheme GooseSwift ... | PASS — BUILD SUCCEEDED |

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None — all alarm wiring is functional. HAP-04 (wake-window engine) is out of scope for this plan and will be addressed when RE prerequisites are met.

## Threat Flags

None — no new network endpoints, auth paths, file access patterns, or schema changes introduced. Alarm time is device-local and transmitted only over the existing BLE connection.

## Self-Check: PASSED

- [x] GooseSwift/GooseAppModel.swift exists and contains `scheduledAlarmTime`
- [x] GooseSwift/GooseAppModel+Lifecycle.swift exists and contains `alarmIsArmed = false` in disconnect branch
- [x] GooseSwift/CoachRouteViews.swift exists and contains `ALARME DE DESPERTAR`
- [x] Commit f2a5c43 exists (Task 1)
- [x] Commit d4628ab exists (Task 2)
- [x] xcodebuild BUILD SUCCEEDED
