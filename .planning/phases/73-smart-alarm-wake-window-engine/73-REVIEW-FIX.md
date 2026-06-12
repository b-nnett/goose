---
phase: 73
padded_phase: 73
fix_scope: critical_warning
iteration: 1
findings_in_scope: 5
fixed: 5
skipped: 0
status: all_fixed
---

# Code Review Fix Report — Phase 73

**Fixed at:** 2026-06-12T00:00:00Z
**Source review:** .planning/phases/73-smart-alarm-wake-window-engine/73-REVIEW.md
**Iteration:** 1

## Summary

All 5 in-scope findings (2 critical, 3 warning) were fixed across 3 source files in 4 atomic commits. CR-01 and WR-02 were batched into a single commit because they both touch the same button action in CoachRouteViews.swift and the fix is identical (the `guard connectionState == "ready"` statement satisfies both findings). WR-01 was resolved with an inline comment rather than a protocol change, as the reviewer explicitly instructed: mark the unverified protocol assumption and defer the framing decision to a BTSnoop verification.

## Fixes Applied

### CR-01 — `alarmIsArmed` set optimistically before BLE command succeeds

**Files modified:** `GooseSwift/CoachRouteViews.swift`
**Commit:** `d0203f8` — `fix(73): CR-01 WR-02 guard connectionState==ready and no pendingAlarmCommand before arming alarm`
**Change:** Added a two-condition guard at the top of the `else` branch in the arm-alarm button action:
```swift
guard model.ble.connectionState == "ready",
      model.ble.pendingAlarmCommand == nil else { return }
```
`pendingAlarmCommand` was confirmed to exist as a public property on `GooseBLEClient` (line 303). The guard ensures `alarmIsArmed` is only set `true` when the connection is genuinely ready and no alarm command is already in flight, preventing the optimistic state flip when `writeAlarmCommand` would have silently bailed out.

### CR-02 — `GooseWakeWindowManager` declared as `final class`, not `actor`

**Files modified:** `GooseSwift/GooseWakeWindowManager.swift`
**Commit:** `4bcb150` — `fix(73): CR-02 change GooseWakeWindowManager from final class to actor`
**Change:** Changed `final class GooseWakeWindowManager` to `actor GooseWakeWindowManager`. All RE-gate comments and the stub body are preserved unchanged. This aligns the type declaration with the plan spec (73-02 HAP-04) and ensures the architectural contract is set correctly before functional implementation is added.

### WR-01 — `buzz` sends raw unframed payload

**Files modified:** `GooseSwift/GooseBLEClient+Haptics.swift`
**Commit:** `b754c5d` — `fix(73): WR-01 document unframed buzz protocol assumption pending BTSnoop verification`
**Change:** Added a 4-line inline comment immediately before the `Data([0x13, clamped])` write, noting that the payload is sent without `buildCommandFrame` framing, that this diverges from every other command in the codebase, and that a BTSnoop capture is required before concluding the behavior is correct. No protocol change was made — the reviewer explicitly instructed to mark the assumption rather than guess at the wire format.

### WR-02 — Button arm branch not re-guarded at execution time

**Files modified:** `GooseSwift/CoachRouteViews.swift`
**Commit:** `d0203f8` — `fix(73): CR-01 WR-02 guard connectionState==ready and no pendingAlarmCommand before arming alarm`
**Change:** Resolved by the same guard applied for CR-01. The `guard model.ble.connectionState == "ready"` check is an execution-time guard inside the button closure, which is exactly what WR-02 required. The `.disabled(isDisconnected)` render-cycle guard remains in place as a UI affordance; the new guard closes the race window between render cycles.

### WR-03 — `windDownTime` parser hardcodes `"HH:mm"` without POSIX locale

**Files modified:** `GooseSwift/CoachRouteViews.swift`
**Commit:** `9dc5315` — `fix(73): WR-03 add POSIX locale to windDownTime DateFormatter`
**Change:** Added `fmt.locale = Locale(identifier: "en_US_POSIX")` immediately after the `DateFormatter()` initializer and before `fmt.dateFormat = "HH:mm"` is set. This makes the parser locale-independent so 12-hour locale devices no longer silently fall back to the raw `"30min antes de \(start)"` string.

## Skipped

None — all 5 in-scope findings were applied successfully.

---

_Fixed: 2026-06-12T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
