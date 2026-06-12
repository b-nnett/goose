# Phase 73: Smart Alarm + Wake-Window Engine - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

HAP-03: Add a Wake Alarm section to the Sleep Coach screen. The user picks a target wakeup time with a DatePicker, arms it (which writes the alarm command to the WHOOP strap and delivers `buzz(loops: 2)` as tactile confirmation), and can cancel it (which writes a cancel command and clears state).

HAP-04 is EXPLICITLY RE-GATED and cannot be implemented until both:
- BTSnoop capture of `STRAP_DRIVEN_ALARM_EXECUTED` packets is completed and documented in `.planning/research/whoop-re/`
- Ghidra decompilation of `SetAlarmInfoCommandPacketRev4` field layout is completed and documented

HAP-04 deliverable in this phase: create `GooseWakeWindowManager.swift` as a documented stub (not functional) — it compiles, exists, and documents the RE gate. No functional implementation until RE prerequisites are met.

</domain>

<decisions>
## Implementation Decisions

### Wake Alarm UI (HAP-03)
- Location: New "Wake Alarm" section at the bottom of the Sleep Coach view (`CoachRouteViews.swift`, inside the existing `SleepCoachView`)
- Control: `DatePicker("Wake alarm", selection: $alarmTime, displayedComponents: .hourAndMinute)` — system time picker
- State in GooseAppModel: `var scheduledAlarmTime: Date? = nil` and `var alarmIsArmed: Bool = false`
- Entry point: the "Wake Alarm" section is always visible in Sleep Coach; arm/cancel button is disabled with "Connect WHOOP to use alarm" message when `model.ble.connectionState != "ready"`

### Alarm Command (HAP-03)
- Arm: call `model.ble.writeAlarmCommand(kind: .setAlarm, time: scheduledAlarmTime)` (existing `AlarmCommandKind` + `writeAlarmCommand()` in `GooseBLEClient.swift`), then `model.ble.buzz(loops: 2)` as tactile confirmation; set `alarmIsArmed = true` on GooseAppModel
- Cancel: call `model.ble.writeAlarmCommand(kind: .cancelAlarm)`, set `alarmIsArmed = false` and `scheduledAlarmTime = nil`; update UI immediately
- Both operations are fire-and-forget (no response expected from strap for HAP-03 — response would require HAP-04 RE work)

### HAP-04 — GooseWakeWindowManager stub
- File: `GooseSwift/GooseWakeWindowManager.swift` — `final class GooseWakeWindowManager` with a clear doc comment explaining the RE gate
- The class must compile but has no functional implementation
- Comment documents what needs to happen: "Implementation requires BTSnoop capture of STRAP_DRIVEN_ALARM_EXECUTED and Ghidra decompilation of SetAlarmInfoCommandPacketRev4 before proceeding"
- No GooseAppModel integration — stub only

### Claude's Discretion
- Exact "Wake Alarm" section layout within Sleep Coach (card vs. plain VStack)
- Whether `alarmIsArmed` is reset on BLE disconnect
- Whether to persist `scheduledAlarmTime` across app launches (UserDefaults)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `GooseBLEClient.swift` lines 617+: `AlarmCommandKind` enum already exists with `.setAlarm`, `.cancelAlarm` cases and `writeAlarmCommand()` method — no new BLE command implementation needed
- `GooseBLEClient+Haptics.swift`: `buzz(loops:)` — Phase 70 artifact for tactile confirmation
- `CoachRouteViews.swift`: `SleepCoachView` is the destination for `CoachRoute.sleep` — add the "Wake Alarm" section here
- `GooseAppModel.swift`: add `var scheduledAlarmTime: Date? = nil` and `var alarmIsArmed: Bool = false` as stored properties
- `connectionState == "ready"` pattern for disconnected state (confirmed from Phase 70 research)
- `@Environment(GooseAppModel.self)` access pattern in all views

### Integration Points
- `GooseSwift/CoachRouteViews.swift` — extend `SleepCoachView` with Wake Alarm section at bottom
- `GooseSwift/GooseAppModel.swift` — add alarm state properties
- New `GooseSwift/GooseWakeWindowManager.swift` — stub file with RE gate documentation
- `GooseSwift.xcodeproj/project.pbxproj` — register GooseWakeWindowManager.swift at 4 locations

</code_context>

<specifics>
## Specific Ideas

- AlarmCommandKind in GooseBLEClient.swift is already there — the hard BLE work is done
- buzz(loops: 2) on arm gives a distinctive "armed" feel vs buzz(loops: 1) used by Breathe
- The stub GooseWakeWindowManager is important to satisfy SC#3 and SC#4 structurally even though it's not functional

</specifics>

<deferred>
## Deferred Ideas

- HAP-04 (GooseWakeWindowManager functional implementation) — fully RE-gated; requires BTSnoop + Ghidra session documented in .planning/research/whoop-re/SetAlarmInfoCommandPacketRev4.md
- Alarm persistence across app restart (UserDefaults) — Claude's discretion to include or omit in the current phase
- Snooze functionality — out of scope (no ROADMAP requirement)
- Response parsing for alarm acknowledgement from strap — requires HAP-04 RE work

</deferred>
