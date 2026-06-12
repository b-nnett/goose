---
phase: 73
phase_name: smart-alarm-wake-window-engine
depth: deep
files_reviewed: 5
files_reviewed_list:
  - GooseSwift/GooseAppModel.swift
  - GooseSwift/GooseAppModel+Lifecycle.swift
  - GooseSwift/CoachRouteViews.swift
  - GooseSwift/GooseWakeWindowManager.swift
  - GooseSwift.xcodeproj/project.pbxproj
findings:
  critical: 2
  warning: 3
  info: 1
  total: 6
status: issues_found
---

# Code Review — Phase 73: Smart Alarm + Wake-Window Engine

## Summary

The two plans in this phase (73-01 Wake Alarm UI and 73-02 GooseWakeWindowManager stub) are largely correct in structure. However two blockers require attention: `alarmIsArmed` is set optimistically in the view before the BLE write has succeeded or even been validated, leaving the UI and model permanently out of sync whenever `writeAlarmCommand` is silently blocked; and `GooseWakeWindowManager` was registered in the pbxproj as `final class` but the plan spec requires it to be an `actor`. Three warnings cover the unframed `buzz` write (which diverges from every other command in this codebase), the missing connectivity guard on the "Arm Alarm" button that is reachable while `isDisconnected` is in a transient state, and the phantom `windDownTime` parser that silently degrades to a fallback string for any locale that uses non-`HH:mm` time formatting.

---

## Critical Issues

### CR-01 — `alarmIsArmed` set optimistically before BLE command succeeds

**File:** `GooseSwift/CoachRouteViews.swift:177-181`

**Issue:** The arm-alarm button handler calls `model.ble.setWhoopAlarm(at:)` and `model.ble.buzz(loops:2)` and then immediately sets `model.alarmIsArmed = true` and `model.scheduledAlarmTime = alarmTime` in the same synchronous block — before either BLE write has been acknowledged by the strap. `setWhoopAlarm` internally calls `writeAlarmCommand`, which silently bails out and returns `void` in at least six failure cases (historical sync in progress, command already in flight, no active peripheral, connection not ready, no write type). When any of those guards fire, the write is dropped on the floor but `alarmIsArmed` flips to `true` anyway. The UI now shows "Cancelar Alarme" and disables the DatePicker while no alarm has actually been set on the strap. On reconnect, `handleBLEConnectionStateChange` clears `alarmIsArmed` only on disconnect — not when the arm was bogus from the start.

**Impact:** Silent data loss: the user believes the alarm is armed but the strap has no alarm programmed. The only recovery is a disconnect/reconnect cycle, which is non-obvious.

**Fix:** Move the model state update into the strap response handler (the callback that processes the `SET_ALARM_TIME` command response), not the button action. Until a success-response callback exists for alarm commands, the minimum safe change is to check `ble.connectionState == "ready"` and `ble.pendingAlarmCommand == nil` before updating state:

```swift
Button {
  if model.alarmIsArmed {
    model.ble.disableWhoopAlarms()
    model.alarmIsArmed = false
    model.scheduledAlarmTime = nil
  } else {
    guard model.ble.connectionState == "ready",
          model.ble.pendingAlarmCommand == nil else { return }
    model.ble.setWhoopAlarm(at: alarmTime)
    model.ble.buzz(loops: 2)
    model.alarmIsArmed = true
    model.scheduledAlarmTime = alarmTime
  }
}
```

The proper long-term fix is to drive `alarmIsArmed = true` from the BLE response callback in `GooseBLEClient` once the strap ACKs the `SET_ALARM_TIME` command, mirroring how `pendingAlarmCommand` is cleared on timeout.

---

### CR-02 — `GooseWakeWindowManager` declared as `final class`, not `actor`

**File:** `GooseSwift/GooseWakeWindowManager.swift:12`

**Issue:** The plan spec for 73-02 (HAP-04 stub) explicitly requires the type to be an `actor`. The implementation uses `final class GooseWakeWindowManager` instead. As a stub this compiles fine, but it violates the architectural contract: when the implementation is added later, any developer following the class declaration will write non-actor code, losing the automatic serial-execution safety that the RE-gated implementation will require when processing STRAP_DRIVEN_ALARM_EXECUTED notifications from the BLE thread.

**Impact:** When HAP-04 is eventually implemented, shared mutable state in the manager will lack actor isolation unless the type is changed at that point — and there is no guarantee that will be noticed. Using `final class` now sets the wrong precedent and is inconsistent with the plan specification.

**Fix:**
```swift
// HAP-04: Wake-Window Engine — RE-GATED
//
// Implementation requires:
// 1. BTSnoop capture of STRAP_DRIVEN_ALARM_EXECUTED packets, documented in
//    .planning/research/whoop-re/SetAlarmInfoCommandPacketRev4.md
// 2. Ghidra decompilation of SetAlarmInfoCommandPacketRev4 field layout,
//    documented in the same file.
//
// Do not add functional implementation until both prerequisites are complete.
actor GooseWakeWindowManager {
  // Stub — not yet functional. See comment above.
}
```

---

## Warnings

### WR-01 — `buzz` sends a raw unframed payload, unlike every other command in this codebase

**File:** `GooseSwift/GooseBLEClient+Haptics.swift:17-18`

**Issue:** `buzz(loops:)` writes `Data([0x13, clamped])` directly to the characteristic via `activePeripheral.writeValue(payload, ...)`. Every other command in this codebase — `writeAlarmCommand`, `writeClockCommand`, `writeSensorStreamCommand` — goes through `activeDeviceGeneration.buildCommandFrame(sequence:command:data:)` which adds the protocol framing (header, sequence number, CRC). The buzz payload is naked: no sequence byte, no framing, no CRC. If the WHOOP protocol requires framed commands, this will be silently ignored by the strap. The existing haptics code (BreatheView, IntervalTimerView) also calls `buzz` the same way, which either means the protocol accepts unframed 2-byte haptic commands on this characteristic, or all haptic feedback has always been silently dropped.

**Impact:** Buzz confirmation after "Arm Alarm" may not reach the strap. This is a protocol-correctness concern: if `0x13` is a valid unframed haptic command the behavior is correct; if it needs framing, the buzz is dropped. This should be verified against a BTSnoop capture.

**Fix:** Verify against a BTSnoop capture whether the WHOOP haptic command requires protocol framing. If framing is required:
```swift
func buzz(loops: Int) {
  guard let activePeripheral, let commandCharacteristic else { ... }
  guard let writeType = writeType(for: commandCharacteristic) else { ... }
  let clamped = UInt8(max(1, min(255, loops)))
  let sequence = nextHapticSequence()  // add a haptic sequence counter
  let frame = activeDeviceGeneration.buildCommandFrame(
    sequence: sequence,
    command: 0x13,
    data: [clamped]
  )
  activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
}
```

---

### WR-02 — "Arm Alarm" button reachable in transient connection states despite `isDisconnected` guard

**File:** `GooseSwift/CoachRouteViews.swift:193`

**Issue:** `isDisconnected` is defined as `model.ble.connectionState != "ready"` (line 145). The button's `.disabled(isDisconnected)` modifier correctly prevents tapping while disconnected. However, the arm branch inside the button action (lines 177-181) does not re-check connection state at execution time. SwiftUI can deliver a tap action during the frame where `connectionState` transitions from `"ready"` to another state (e.g., the strap disconnects exactly as the user taps). The `.disabled` modifier updates on the next render cycle, not synchronously with the tap gesture delivery. In that window, `setWhoopAlarm` fires and its internal guard (`connectionState == "ready"`) blocks it silently — but `alarmIsArmed` is still set to `true` (compounding CR-01).

**Impact:** Low-probability race, but combined with CR-01 the consequence is the same: `alarmIsArmed = true` with no alarm on the strap.

**Fix:** Guard inside the button action (this also resolves CR-01's race):
```swift
guard model.ble.connectionState == "ready" else { return }
```

---

### WR-03 — `windDownTime` parser uses a hardcoded `"HH:mm"` format that breaks for non-24h locales

**File:** `GooseSwift/CoachRouteViews.swift:133-137`

**Issue:** `windDownTime` parses `sleep?.startLabel` with `DateFormatter()` using `dateFormat = "HH:mm"`. The `startLabel` string is produced by the Rust bridge (or health store formatting) and the format it uses is locale-dependent. On a device with a 12-hour locale, `startLabel` may be `"11:30 PM"` and the parse will return `nil`, silently falling back to `"30min antes de 11:30 PM"` — a degraded display string. The fallback concatenation `"30min antes de \(start)"` is the only user-visible output in that case. Additionally, `fmt` is created without setting its `locale` or `timeZone`, so it inherits the device locale and timezone, making the parse non-deterministic across user devices.

**Impact:** Wind-down time displays a raw fallback string instead of a computed time for any user on a 12-hour locale.

**Fix:** Either fix the input source to always produce ISO-8601 or 24h output, or make the formatter locale-independent:
```swift
private var windDownTime: String {
  guard let start = sleep?.startLabel else { return "—" }
  let fmt = DateFormatter()
  fmt.dateFormat = "HH:mm"
  fmt.locale = Locale(identifier: "en_US_POSIX")
  fmt.timeZone = TimeZone.current
  guard let date = fmt.date(from: start) else { return "30min antes de \(start)" }
  let adjusted = date.addingTimeInterval(-30 * 60)
  return fmt.string(from: adjusted)
}
```

---

## Info

### IN-01 — `sleepDebt` is a stub that always returns the same string regardless of input

**File:** `GooseSwift/CoachRouteViews.swift:140-143`

**Issue:** `sleepDebt(actual:)` ignores its `actual` parameter and always returns `"objetivo: 8h 00m"`. The function signature implies it computes a debt value from the actual sleep duration, but the body is a no-op placeholder. The displayed "Dívida" row is therefore always `"objetivo: 8h 00m"` regardless of how much the user slept.

**Impact:** Misleading UI — the debt row always shows the goal, not the deficit. Low severity since the comment acknowledges it requires parsing, but the parameter name `actual` implies computation is expected.

**Fix:** Either implement the parsing, or rename the function and make its stub nature explicit in the UI label (e.g., display `"—"` until implemented):
```swift
private func sleepDebt(actual: String) -> String {
  // TODO: parse actual duration and subtract from 8h goal
  return "—"
}
```

---

## Clean Areas

- **pbxproj registration**: `GooseWakeWindowManager.swift` is correctly registered at all 4 required locations (PBXBuildFile, PBXFileReference, PBXGroup children, PBXSourcesBuildPhase). UUID scheme follows the project's E1/E2 convention at index 014. Count verified at 4.
- **`@MainActor` / `@Observable` usage**: `GooseAppModel` is correctly declared `@MainActor @Observable`. `CoachSleepRouteView` accesses it via `@Environment(GooseAppModel.self)` which is the correct injection pattern for `@Observable` types (not `@EnvironmentObject`).
- **No Rust bridge calls from `@MainActor`**: Neither the view nor the lifecycle extension makes synchronous Rust bridge calls on the main thread. The `runStorageCompactionIfNeeded` call in `GooseAppModel.init` is correctly dispatched to `DispatchQueue.global(qos: .utility)`.
- **Disconnect reset of `alarmIsArmed`**: `handleBLEConnectionStateChange` (GooseAppModel+Lifecycle.swift:139) correctly clears `alarmIsArmed = false` on all non-ready BLE states, preventing a stale "armed" UI after reconnect.
- **`nextFutureAlarmDate` edge case**: The implementation correctly handles the "alarm time already passed today" case by adding one day, and uses `Calendar.current` (injectable for tests) rather than raw `TimeInterval` arithmetic.
- **HAP-04 RE gate**: The stub correctly documents both prerequisites (BTSnoop capture + Ghidra decompile) with their specific artifact names. No functional implementation was added prematurely.

---

_Reviewed: 2026-06-12T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: deep_
