# Phase 73: Smart Alarm + Wake-Window Engine - Research

**Researched:** 2026-06-12
**Domain:** SwiftUI alarm UI + existing GooseBLEClient alarm infrastructure
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Wake Alarm UI (HAP-03)**
- Location: New "Wake Alarm" section at the bottom of `CoachSleepRouteView` (`CoachRouteViews.swift`)
- Control: `DatePicker("Wake alarm", selection: $alarmTime, displayedComponents: .hourAndMinute)`
- State in GooseAppModel: `var scheduledAlarmTime: Date? = nil` and `var alarmIsArmed: Bool = false`
- Entry point: always visible; arm/cancel button disabled with "Connect WHOOP to use alarm" when `model.ble.connectionState != "ready"`

**Alarm Command (HAP-03)**
- Arm: call `model.ble.setWhoopAlarm(at: scheduledAlarmTime)`, then `model.ble.buzz(loops: 2)`; set `alarmIsArmed = true` on GooseAppModel
- Cancel: call `model.ble.disableWhoopAlarms()`, set `alarmIsArmed = false` and `scheduledAlarmTime = nil`
- Both operations are fire-and-forget (no response parsing for HAP-03)

**HAP-04 — GooseWakeWindowManager stub**
- File: `GooseSwift/GooseWakeWindowManager.swift` — `final class GooseWakeWindowManager`
- Must compile, no functional implementation
- Comment documents RE gate: "Implementation requires BTSnoop capture of STRAP_DRIVEN_ALARM_EXECUTED and Ghidra decompilation of SetAlarmInfoCommandPacketRev4 before proceeding"
- No GooseAppModel integration

### Claude's Discretion
- Exact "Wake Alarm" section layout (card vs. plain VStack)
- Whether `alarmIsArmed` is reset on BLE disconnect
- Whether to persist `scheduledAlarmTime` across app launches (UserDefaults)

### Deferred Ideas (OUT OF SCOPE)
- HAP-04 functional implementation — fully RE-gated
- Alarm persistence across app restart (UserDefaults)
- Snooze functionality — no ROADMAP requirement
- Response parsing for alarm acknowledgement from strap
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HAP-03 | Utilizador consegue agendar alarme de vibração na pulseira a hora fixa (smart alarm — single-shot BLE write; requer HAP-01) | All BLE infrastructure exists: `setWhoopAlarm(at:)`, `disableWhoopAlarms()`, `buzz(loops:)`. UI pattern confirmed from `CoachSleepRouteView`. State properties go in `GooseAppModel`. |
| HAP-04 | Pulseira vibra no momento óptimo dentro de uma janela de despertar (wake-window engine; RE-gated) | Stub-only: `GooseWakeWindowManager.swift` with documented RE gate. No functional implementation until BTSnoop + Ghidra prerequisites met. |
</phase_requirements>

---

## Summary

Phase 73 is almost entirely a UI wiring task. The BLE alarm infrastructure in `GooseBLEClient` is complete and battle-tested: `AlarmCommandKind` enum covers `set`, `get`, `run`, and `disableAll` cases; `writeAlarmCommand(_:)` in `GooseBLEClient+Commands.swift` does all framing, sequencing, timeout, and guard logic; `setWhoopAlarm(at:)` and `disableWhoopAlarms()` in `GooseBLEClient+UserActions.swift` are the two correct call sites for this phase. The `buzz(loops:)` haptic primitive shipped in Phase 70.

HAP-03 requires: (1) two new stored properties on `GooseAppModel`, (2) a "Wake Alarm" section appended to `CoachSleepRouteView`, and (3) `@Environment(GooseAppModel.self)` injection into that view. HAP-04 requires only a stub Swift file with a doc comment and registration in `project.pbxproj` at 4 locations.

**Primary recommendation:** Use `model.ble.setWhoopAlarm(at:)` and `model.ble.disableWhoopAlarms()` — do not call `writeAlarmCommand(_:)` directly from view code. These wrappers already handle `nextFutureAlarmDate`, `validatedAlarmID`, and logging.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Alarm time picker | SwiftUI View (`CoachSleepRouteView`) | GooseAppModel (@State bridge) | Time selection is pure UI state until user taps Arm |
| Arm/cancel BLE command | GooseBLEClient (`GooseBLEClient+UserActions.swift`) | GooseAppModel (owns `ble`) | All BLE writes go through `GooseBLEClient`; model coordinates state |
| Alarm armed state | GooseAppModel (`@MainActor @Observable`) | — | Needs to survive tab navigation; must be on main actor |
| Haptic confirmation | GooseBLEClient (`GooseBLEClient+Haptics.swift`) | — | Same peripheral/characteristic access as alarm write |
| Wake-window engine | `GooseWakeWindowManager.swift` (stub) | — | RE-gated; stub only for this phase |

---

## Standard Stack

No new packages. This phase is pure Swift/SwiftUI using existing project dependencies.

### Reused Existing Components

| Component | File | Purpose |
|-----------|------|---------|
| `setWhoopAlarm(at:alarmID:)` | `GooseBLEClient+UserActions.swift:353` | Arms alarm at next future occurrence of given time |
| `disableWhoopAlarms()` | `GooseBLEClient+UserActions.swift:374` | Sends `disableAll` command to strap |
| `buzz(loops:)` | `GooseBLEClient+Haptics.swift:7` | Tactile confirmation via cmd 0x13 |
| `canWriteAlarm` | `GooseBLEClient.swift:864` | `true` when ready + command char present + no command in flight |
| `alarmCommandStatus` | `GooseBLEClient.swift:49` | Last alarm op status string (for debug/display) |
| `CoachInfoGroup` / `CoachInfoRow` | `CoachRouteViews.swift:363–409` | Existing card layout used by all Sleep Coach sections |
| `@Environment(GooseAppModel.self)` | Pattern used across 20+ views | Access model from within `CoachSleepRouteView` |

### Package Legitimacy Audit

No external packages are installed in this phase. Section not applicable.

---

## Architecture Patterns

### Recommended Project Structure

```
GooseSwift/
├── CoachRouteViews.swift          # EDIT — add Wake Alarm section to CoachSleepRouteView
├── GooseAppModel.swift            # EDIT — add scheduledAlarmTime + alarmIsArmed
└── GooseWakeWindowManager.swift   # CREATE — HAP-04 RE-gated stub
GooseSwift.xcodeproj/project.pbxproj  # EDIT — register GooseWakeWindowManager.swift (4 locations)
```

### Pattern 1: Accessing GooseAppModel from CoachSleepRouteView

`CoachSleepRouteView` currently receives `healthStore: HealthDataStore` as a `var` parameter (not via `@Environment`). It does not currently hold a `model` reference. To access alarm state and BLE commands, add `@Environment(GooseAppModel.self) private var model` — this is the standard pattern used by all other views that need BLE interaction.

```swift
// Source: existing pattern in BreatheView.swift, ConnectionView.swift, etc.
struct CoachSleepRouteView: View {
  var healthStore: HealthDataStore
  @Environment(GooseAppModel.self) private var model
  @State private var alarmTime: Date = Calendar.current.date(
    bySettingHour: 7, minute: 0, second: 0, of: Date()) ?? Date()

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 18) {
        // ... existing sections ...

        // Wake Alarm section at bottom
        wakeAlarmSection
      }
      .padding(16)
    }
    .gooseScreenBackground()
    .navigationTitle("Sleep Coach")
    .navigationBarTitleDisplayMode(.inline)
  }
}
```

### Pattern 2: Wake Alarm Section using CoachInfoGroup

The existing `CoachInfoGroup` card component is the correct container — consistent with all other Sleep Coach sections.

```swift
// Source: CoachRouteViews.swift:363 — CoachInfoGroup renders a titled card with .quaternary background
@ViewBuilder
private var wakeAlarmSection: some View {
  CoachInfoGroup(title: "WAKE ALARM") {
    VStack(spacing: 12) {
      DatePicker(
        "Wake alarm",
        selection: $alarmTime,
        displayedComponents: .hourAndMinute
      )
      .datePickerStyle(.compact)
      .padding(.vertical, 4)

      if model.alarmIsArmed {
        Button("Cancel Alarm") {
          model.ble.disableWhoopAlarms()
          model.alarmIsArmed = false
          model.scheduledAlarmTime = nil
        }
        .buttonStyle(.borderedProminent)
        .tint(.red)
      } else {
        Button("Arm Alarm") {
          model.ble.setWhoopAlarm(at: alarmTime)
          model.ble.buzz(loops: 2)
          model.alarmIsArmed = true
          model.scheduledAlarmTime = alarmTime
        }
        .buttonStyle(.borderedProminent)
        .disabled(!model.ble.connectionState == "ready" || !model.ble.canWriteAlarm)
      }

      if model.ble.connectionState != "ready" {
        Text("Connect WHOOP to use alarm")
          .font(.caption)
          .foregroundStyle(.secondary)
      }
    }
  }
}
```

### Pattern 3: GooseAppModel stored properties placement

New alarm properties go after the `liveWorkoutStrain` property (line 33) — same grouping as other live session state.

```swift
// Source: GooseAppModel.swift:33 — after liveWorkoutStrain
var liveWorkoutStrain: Double = 0
var scheduledAlarmTime: Date? = nil    // HAP-03
var alarmIsArmed: Bool = false         // HAP-03
```

### Pattern 4: GooseWakeWindowManager stub (HAP-04)

```swift
// GooseSwift/GooseWakeWindowManager.swift
import Foundation

// HAP-04: Wake-Window Engine — RE-GATED
//
// Implementation requires:
// 1. BTSnoop capture of STRAP_DRIVEN_ALARM_EXECUTED packets, documented in
//    .planning/research/whoop-re/SetAlarmInfoCommandPacketRev4.md
// 2. Ghidra decompilation of SetAlarmInfoCommandPacketRev4 field layout,
//    documented in the same file.
//
// Do not add functional implementation until both prerequisites are complete.
final class GooseWakeWindowManager {
  // Stub — not yet functional. See comment above.
}
```

### Pattern 5: pbxproj registration for GooseWakeWindowManager.swift

[ASSUMED] UUID strategy based on project skill cs:s1-131. Must grep existing E1/E2 UUIDs to find the next available `NN` index before choosing values.

```bash
# Step 1: find highest existing E1/E2 UUID index
grep -oE 'E[12]0{22}[0-9A-Fa-f]{2}' GooseSwift.xcodeproj/project.pbxproj | sort -u | tail -5

# Step 2: pick next NN, then verify no collision
grep "E100000000000000000000NN" GooseSwift.xcodeproj/project.pbxproj   # must return empty
grep "E200000000000000000000NN" GooseSwift.xcodeproj/project.pbxproj   # must return empty

# Step 3: after editing, validate exactly 4 occurrences
grep -c 'GooseWakeWindowManager.swift' GooseSwift.xcodeproj/project.pbxproj  # must return 4
```

The 4 required locations in `project.pbxproj`:
1. **PBXBuildFile section** — `{E1NN} /* GooseWakeWindowManager.swift in Sources */ = {isa = PBXBuildFile; fileRef = {E2NN} /* GooseWakeWindowManager.swift */; };`
2. **PBXFileReference section** — `{E2NN} /* GooseWakeWindowManager.swift */ = {isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = GooseWakeWindowManager.swift; sourceTree = "<group>"; };`
3. **PBXGroup children list** — `{E2NN} /* GooseWakeWindowManager.swift */,` adjacent to a logically related file (e.g., next to `GooseStrainAccumulator.swift`)
4. **PBXSourcesBuildPhase files list** — `{E1NN} /* GooseWakeWindowManager.swift in Sources */,`

### Anti-Patterns to Avoid

- **Calling `writeAlarmCommand(_:)` directly from view code:** The view should call `model.ble.setWhoopAlarm(at:)` and `model.ble.disableWhoopAlarms()` — these wrappers handle `nextFutureAlarmDate`, `validatedAlarmID`, and logging. Direct `writeAlarmCommand` calls bypass those guards.
- **Calling `model.ble.buzz(loops:)` from background threads:** `buzz(loops:)` accesses `activePeripheral` and `commandCharacteristic` directly — call only from `@MainActor` context (SwiftUI button actions are already on main).
- **Implementing HAP-04 logic in `GooseWakeWindowManager.swift`:** The stub must remain non-functional. Any functional wake-window logic added before the RE prerequisites are documented will be rejected in verify-work.
- **Adding `@StateObject` or `@ObservedObject` for GooseAppModel:** The project uses `@Observable` (not `ObservableObject`). Access via `@Environment(GooseAppModel.self)`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| "Next future alarm at HH:mm" | Custom Date math | `GooseBLEClient.nextFutureAlarmDate(from:)` | Already handles tomorrow rollover; tested |
| Alarm payload framing | Custom byte array | `AlarmCommandKind.set(alarmID:date:pattern:).payload` | Handles subsecond timestamp encoding (ms → 32768-tick subseconds) |
| Alarm haptics pattern | Custom `[UInt8]` | `AlarmHapticsPattern.whoopDefault` | Reverse-engineered from WHOOP APK; waveform effects bytes are fixed |
| Tactile confirmation | CoreHaptics / UIImpactFeedbackGenerator | `model.ble.buzz(loops: 2)` | Phase 70 contract: strap vibration, not phone haptics |
| Guard "is BLE ready?" | Custom state check | `model.ble.canWriteAlarm` | Already checks `canSendHello && !isHistoricalSyncing && supportsAlarmCommands && pendingAlarmCommand == nil` |

---

## AlarmCommandKind — Verified Enum (HAP-03 critical)

[VERIFIED: GooseBLEClient.swift:617–684] The actual enum cases are **not** `.setAlarm` / `.cancelAlarm` as the CONTEXT.md draft suggests. The real cases are:

```swift
enum AlarmCommandKind {
  case get(alarmID: UInt8)                               // command 67 — GET_ALARM_TIME
  case set(alarmID: UInt8, date: Date, pattern: AlarmHapticsPattern)  // command 66 — SET_ALARM_TIME
  case run(alarmID: UInt8)                               // command 68 — RUN_ALARM
  case disableAll                                        // command 69 — DISABLE_ALARM
}
```

The CONTEXT.md arm/cancel pattern references `.setAlarm` and `.cancelAlarm` — **these cases do not exist**. The correct call sites are the higher-level wrappers:

| CONTEXT.md intent | Correct call | File:line |
|-------------------|-------------|-----------|
| Arm (set alarm) | `model.ble.setWhoopAlarm(at: alarmTime)` | `GooseBLEClient+UserActions.swift:353` |
| Cancel (disable) | `model.ble.disableWhoopAlarms()` | `GooseBLEClient+UserActions.swift:374` |

`setWhoopAlarm(at:)` internally calls `writeAlarmCommand(.set(alarmID: alarmID, date: targetDate, pattern: .whoopDefault))`. `disableWhoopAlarms()` calls `writeAlarmCommand(.disableAll)`. Using the wrappers is both correct and safer.

---

## writeAlarmCommand signature — Verified

[VERIFIED: GooseBLEClient+Commands.swift:324]

```swift
func writeAlarmCommand(_ kind: AlarmCommandKind)
```

Single parameter — the `AlarmCommandKind` enum value. Takes a single unlabelled `kind` argument. No `time:` parameter at the `writeAlarmCommand` level — time is encoded inside the `AlarmCommandKind.set` associated value. The CONTEXT.md draft signature `writeAlarmCommand(kind: .setAlarm, time: scheduledAlarmTime)` does not match reality; use the wrapper methods instead.

`writeAlarmCommand` guards: must be on main thread; blocks during `isHistoricalSyncing`; blocks if `pendingAlarmCommand != nil`; requires `activePeripheral`, `commandCharacteristic`, `connectionState == "ready"`, `supportsAlarmCommands`, writable characteristic. All these guards are already inside the method — callers do not need to replicate them.

---

## buzz(loops:) — Verified

[VERIFIED: GooseBLEClient+Haptics.swift:7]

```swift
func buzz(loops: Int)
```

Sends `Data([0x13, clamped_UInt8])` directly to `commandCharacteristic`. Clamps loops to 1–255. No framing via `buildCommandFrame` — raw 2-byte payload. Requires `activePeripheral` and `commandCharacteristic` to be non-nil; silently returns (logs warn) if either is missing. Call after `setWhoopAlarm(at:)` for tactile arm confirmation.

---

## CoachSleepRouteView insertion point — Verified

[VERIFIED: CoachRouteViews.swift:80–140] The view's `body` is a `ScrollView > VStack(spacing: 18)` with three `CoachInfoGroup` sections (CRONOGRAMA, QUALIDADE, DÍVIDA DE SONO). The VStack closes at line 118 (`.padding(16)`). The Wake Alarm section inserts as a 4th `CoachInfoGroup` after the optional DÍVIDA DE SONO group, still inside the same VStack, before the `.padding(16)` modifier.

The view currently has no `@Environment(GooseAppModel.self)` — it must be added as the first property after `var healthStore: HealthDataStore`. `@State private var alarmTime: Date` should be initialised to next 07:00.

---

## Common Pitfalls

### Pitfall 1: CONTEXT.md alarm case names are wrong
**What goes wrong:** Planner or executor writes `writeAlarmCommand(kind: .setAlarm, time: ...)` — this fails to compile because `.setAlarm` does not exist and `writeAlarmCommand` takes no `time:` label.
**Why it happens:** CONTEXT.md was drafted before the codebase was read; it used placeholder names.
**How to avoid:** Always call `model.ble.setWhoopAlarm(at:)` and `model.ble.disableWhoopAlarms()`. Never call `writeAlarmCommand` from view code.
**Warning signs:** Compiler error "type 'AlarmCommandKind' has no member 'setAlarm'".

### Pitfall 2: CoachSleepRouteView has no model access today
**What goes wrong:** Adding alarm UI but forgetting `@Environment(GooseAppModel.self) private var model` — results in "use of unresolved identifier 'model'" at all `model.ble.*` call sites.
**Why it happens:** The view was designed as a passive display layer receiving only `HealthDataStore`.
**How to avoid:** Add `@Environment(GooseAppModel.self) private var model` as the first stored property alongside `var healthStore`.

### Pitfall 3: GooseWakeWindowManager.swift missing from pbxproj
**What goes wrong:** File created in `GooseSwift/` but not registered — Xcode silently excludes it from the build target. The HAP-04 stub requirement (SC#3/SC#4) is not satisfied even though the file exists on disk.
**Why it happens:** Xcode does not auto-add files created outside the IDE.
**How to avoid:** Register at exactly 4 locations per project skill cs:s1-131. Validate with `grep -c 'GooseWakeWindowManager.swift' project.pbxproj` — must return 4.

### Pitfall 4: canWriteAlarm vs connectionState check
**What goes wrong:** Button disabled only when `connectionState != "ready"` — misses cases where historical sync is running or a command is already in flight.
**How to avoid:** Use `model.ble.canWriteAlarm` as the primary disabled predicate. It checks `canSendHello && !isHistoricalSyncing && supportsAlarmCommands && pendingAlarmCommand == nil`.

---

## Runtime State Inventory

Not applicable — this is a greenfield UI addition with no rename/refactor/migration. No stored data, live service config, OS-registered state, secrets, or build artifacts are affected.

---

## Environment Availability

| Dependency | Required By | Available | Notes |
|------------|------------|-----------|-------|
| `GooseBLEClient+Haptics.swift` (`buzz`) | HAP-03 tactile confirm | Already in codebase (Phase 70) | — |
| `GooseBLEClient+UserActions.swift` (`setWhoopAlarm`, `disableWhoopAlarms`) | HAP-03 BLE writes | Already in codebase | — |
| `AlarmCommandKind` + `writeAlarmCommand` | Underlying infrastructure | Already in codebase | — |
| WHOOP 5.0 device connected | HAP-03 runtime | User owns WHOOP 5.0 | Guards in `writeAlarmCommand` handle disconnected state gracefully |

No missing dependencies. All required infrastructure is already in the codebase from Phase 70 and prior.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (Rust); no Swift test target detected |
| Config file | none |
| Quick run command | `cd /Users/francisco/Documents/goose/Rust/core && cargo test` |
| Full suite command | `cd /Users/francisco/Documents/goose/Rust/core && cargo test` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HAP-03 | Alarm UI renders without crash | Simulator smoke | XcodeBuildMCP screenshot after build | N/A — manual |
| HAP-03 | Arm button calls setWhoopAlarm + buzz | Manual on device | Visual + tactile confirmation | N/A — manual |
| HAP-03 | Cancel clears state | Manual on device | UI state check | N/A — manual |
| HAP-04 | GooseWakeWindowManager compiles | Build | `xcodebuild build` succeeds | ❌ Wave 0 — file to create |

### Wave 0 Gaps

- [ ] `GooseSwift/GooseWakeWindowManager.swift` — must be created before build can validate HAP-04

---

## Security Domain

No security-sensitive surfaces introduced. No new network calls, no credential handling, no user data exposed. Alarm time is device-local and only transmitted to the WHOOP strap over the already-established BLE connection.

ASVS V5 (Input Validation): alarm time is a `Date` from `DatePicker` — no free-text input, no validation needed. The `validatedAlarmID` guard inside `writeAlarmCommand` handles any out-of-range alarm ID.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | pbxproj UUID scheme `E1/E2 0{22} NN` has available slots for GooseWakeWindowManager | Architecture Patterns — Pattern 5 | UUID collision → build failure; resolve by grepping highest existing index first |
| A2 | `alarmIsArmed` reset on BLE disconnect is Claude's discretion; not resetting is acceptable default | User Constraints | If user expects alarm to auto-clear on disconnect, requires additional lifecycle hook in GooseAppModel |

---

## Sources

### Primary (HIGH confidence)
- `GooseSwift/GooseBLEClient.swift:617–684` — AlarmCommandKind enum, AlarmHapticsPattern struct, canWriteAlarm computed property
- `GooseSwift/GooseBLEClient+Commands.swift:186–375` — writeAlarmCommand, supportsAlarmCommands, validatedAlarmID
- `GooseSwift/GooseBLEClient+UserActions.swift:345–377` — setWhoopAlarm(at:), runWhoopAlarmNow, disableWhoopAlarms
- `GooseSwift/GooseBLEClient+Parsing.swift:834–847` — nextFutureAlarmDate static helper
- `GooseSwift/GooseBLEClient+Haptics.swift:7–21` — buzz(loops:) implementation
- `GooseSwift/CoachRouteViews.swift:80–409` — CoachSleepRouteView body structure, CoachInfoGroup/CoachInfoRow components
- `GooseSwift/GooseAppModel.swift:1–60` — stored property placement, @MainActor @Observable pattern

### Secondary (MEDIUM confidence)
- `GooseSwift.xcodeproj/project.pbxproj` — grep confirmed 4 occurrences required per cs:s1-131

---

## Metadata

**Confidence breakdown:**
- AlarmCommandKind/writeAlarmCommand API: HIGH — read directly from source files
- CoachSleepRouteView insertion point: HIGH — read directly from source
- GooseAppModel property placement: HIGH — read directly from source
- pbxproj UUID strategy: MEDIUM — relies on project skill cs:s1-131 pattern; must grep before choosing NN
- HAP-04 stub design: HIGH — fully specified in CONTEXT.md decisions

**Research date:** 2026-06-12
**Valid until:** 60 days (stable — no external APIs; all findings from codebase)
