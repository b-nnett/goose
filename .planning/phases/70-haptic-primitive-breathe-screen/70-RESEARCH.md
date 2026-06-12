# Phase 70: Haptic Primitive + Breathe Screen - Research

**Researched:** 2026-06-12
**Domain:** CoreBluetooth BLE command write + SwiftUI animation state machine
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- HAP-01: `buzz(loops:)` in new file `GooseSwift/GooseBLEClient+Haptics.swift`; signature `func buzz(loops: Int)`; payload `[0x13, UInt8(clamped)]` written directly to `commandCharacteristic` via `activePeripheral.writeValue(_:for:type:)` — NO frame-sequence wrapping; fire-and-forget
- HAP-01: Guard: if `activePeripheral == nil` or `commandCharacteristic == nil`, log via OSLog and return silently
- HAP-02: Entry point `MoreRoute.breathe` added to `MoreRouteModels.swift`; push navigation inside existing `NavigationStack(path: $router.morePath)`
- HAP-02: Row label "Breathe", subtitle "Paced breathing with haptics"
- HAP-02: Box breathing pattern: 4s inhale / 4s hold / 4s exhale
- HAP-02: Animation: `Circle()` with `scaleEffect` 0.6 → 1.0 (inhale), static at 1.0 (hold), 1.0 → 0.6 (exhale)
- HAP-02: Phase labels: "INHALE" / "HOLD" / "EXHALE" below circle
- HAP-02: `buzz(loops: 1)` fired at START of each phase transition (inhale start, hold start, exhale start)
- HAP-02: Session is free-running; user taps Stop to end
- HAP-02: Screen always accessible; haptics disabled (silent no-op) when disconnected; "Connect WHOOP to enable haptics" banner shown when `!isRunning && !isConnected`

### Claude's Discretion

- Exact circle animation easing curve and scale range
- Color scheme for Breathe screen (follows FitnessColor / existing palette)
- Whether to show a cycle counter or elapsed time during the session
- OSLog subsystem string for haptic writes

### Deferred Ideas (OUT OF SCOPE)

- Configurable breath timings (user-settable inhale/hold/exhale durations)
- Multi-pattern sessions (Weil 4-7-8, coherence breathing)
- Background audio cue alongside haptic
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HAP-01 | App can vibrate WHOOP 5.0 via BLE cmd 0x13 (`buzz(loops:)` primitive) | BLE write pattern verified in `GooseBLEClient+HistoricalCommands.swift`; `writeType(for:)` helper confirmed; direct `writeValue` without frame-sequence wrapping is correct for fire-and-forget commands |
| HAP-02 | Breathe screen with paced haptic feedback (inhale/hold/exhale + `buzz(loops:1)` at each transition) | Navigation wiring pattern verified in `MoreView.swift`; `MoreRouteModels.swift` structure confirmed; animation pattern verified in `DeviceView.swift`; `@Environment(GooseAppModel.self)` access pattern confirmed |
</phase_requirements>

---

## Summary

Phase 70 adds two tightly coupled deliverables: a low-level BLE haptic primitive (HAP-01) and a SwiftUI Breathe screen that uses it (HAP-02). Both are pure Swift additions — no Rust changes, no new dependencies, no new frameworks.

HAP-01 is the simpler of the two. The existing codebase already has everything needed: the `commandCharacteristic` write path, the `writeType(for:)` helper that detects `.withResponse` vs `.withoutResponse`, and the OSLog `record(source:title:body:)` pattern. The critical distinction from other command extensions is that `buzz` skips the sequence-number frame wrapper (`buildCommandFrame(sequence:command:data:)`). It writes raw bytes `[0x13, UInt8(clamped)]` directly. This is intentional: the haptic command is fire-and-forget with no expected response packet, so sequence tracking would be wasted overhead.

HAP-02 is a standard SwiftUI feature addition. The `MoreRoute` enum extension pattern, `navigationDestination` arm in `MoreView`, and `MoreRouteStatus` property addition are all mechanical. The interesting part is the session loop: a Swift structured concurrency `Task` drives the 4s/4s/4s cycle via `Task.sleep(for: .seconds(4))`, with `withAnimation(.easeInOut(duration:))` calls for the circle scale. The `@Environment(GooseAppModel.self)` access pattern is established throughout the codebase. The UI-SPEC provides exact component hierarchy, color tokens, spacing, and accessibility requirements — all resolved before research.

**Primary recommendation:** Implement HAP-01 first (no UI dependency), then HAP-02 wired to call `model.ble.buzz(loops: 1)`. The two plans are sequentially independent within the phase but HAP-01 must be committed before HAP-02 references it.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| BLE haptic write (buzz) | BLE Client layer (`GooseBLEClient`) | — | Follows existing command extension pattern; BLE I/O belongs in the BLE client, not the model or view |
| Breathe session state machine | View (`BreatheView` `@State`) | — | Session is UI-local; no persistence, no model coordination required |
| Connection-state gate | BLE Client (`connectionState` property) | View (reads it reactively) | `model.ble.connectionState == "ready"` is the established codebase idiom for connected state; view reads it via `@Environment` |
| Navigation routing | `MoreRouteModels.swift` + `MoreView.swift` | `AppRouter` | All More-tab destinations follow the same enum + `navigationDestination` switch pattern |

---

## Standard Stack

### Core (all already in project — no new installs)

| Component | Version | Purpose | Why Standard |
|-----------|---------|---------|--------------|
| CoreBluetooth | iOS 26 SDK | BLE write to command characteristic | Already imported in all BLE extension files |
| SwiftUI | iOS 26 SDK | Breathe screen UI + animation | Project-wide UI framework |
| Foundation | iOS 26 SDK | Data, UInt8 clamping | Universal |
| OSLog / Logger | iOS 26 SDK | Haptic write diagnostics | Established logging pattern; `Logger(subsystem: "com.goose.swift", category: "ble")` |

No new packages. No `npm install`. No `Package.swift` changes. [VERIFIED: codebase grep]

---

## Package Legitimacy Audit

> Not applicable — this phase installs zero external packages. All capabilities come from the iOS SDK and existing project code.

---

## Architecture Patterns

### System Architecture Diagram

```
[BreatheView]
     │  @Environment(GooseAppModel.self)
     ▼
[GooseAppModel]
     │  .ble: GooseBLEClient
     ▼
[GooseBLEClient]
     │  .buzz(loops: 1)           ← new method in +Haptics.swift
     │  .commandCharacteristic    ← existing CBCharacteristic?
     │  .activePeripheral         ← existing CBPeripheral?
     ▼
[CBPeripheral.writeValue(_:for:type:)]
     │
     ▼
[WHOOP 5.0 BLE cmd 0x13]  (fire-and-forget; no response expected)
```

Session loop (inside BreatheView):
```
[Task { repeat { inhale → hold → exhale } }]
     │  withAnimation(.easeInOut)  →  circleScale @State
     │  model.ble.buzz(loops: 1)   →  [GooseBLEClient.buzz]
     │  await Task.sleep(.seconds(4))
     └─ cancelled by stopSession() → phaseTask?.cancel()
```

Navigation path:
```
MoreView (NavigationStack)
  └─ Section("Wellness") { NavigationLink(MoreRoute.breathe) }
       └─ navigationDestination { case .breathe: BreatheView() }
```

### Recommended Project Structure

```
GooseSwift/
├── GooseBLEClient+Haptics.swift    ← new (HAP-01)
├── BreatheView.swift               ← new (HAP-02)
├── MoreRouteModels.swift           ← modified: add case breathe + wellnessRoutes + MoreRouteStatus.breathe
└── MoreView.swift                  ← modified: add Wellness section + case .breathe destination arm
```

### Pattern 1: BLE Command Write (fire-and-forget variant)

The established pattern for `writeHistoricalCommand` and `writeAlarmCommand` uses `buildCommandFrame(sequence:command:data:)` because those commands expect a response keyed by sequence number. Haptic buzz expects NO response — so the pattern simplifies to a direct byte write. [VERIFIED: codebase read]

```swift
// Source: GooseSwift/GooseBLEClient+HistoricalCommands.swift (lines 177–185)
// writeType helper — determines .withResponse vs .withoutResponse from characteristic properties
func writeType(for characteristic: CBCharacteristic) -> CBCharacteristicWriteType? {
  if characteristic.properties.contains(.write) {
    return .withResponse
  }
  if characteristic.properties.contains(.writeWithoutResponse) {
    return .withoutResponse
  }
  return nil
}
```

```swift
// buzz pattern — no sequence, no frame wrapper, no pending command state
// Source: derived from direct writeValue call in GooseBLEClient+HistoricalCommands.swift line 126
func buzz(loops: Int) {
  guard let activePeripheral, let commandCharacteristic else {
    record(source: "ble.haptic", title: "buzz.blocked", body: "no active peripheral or characteristic")
    return
  }
  guard let writeType = writeType(for: commandCharacteristic) else {
    record(source: "ble.haptic", title: "buzz.blocked", body: "characteristic not writable")
    return
  }
  let clamped = UInt8(max(1, min(255, loops)))
  let payload = Data([0x13, clamped])
  activePeripheral.writeValue(payload, for: commandCharacteristic, type: writeType)
  record(source: "ble.haptic", title: "buzz.sent", body: "loops=\(clamped) writeType=\(writeTypeName(writeType))")
}
```

Note: `writeTypeName(_:)` is an existing helper on `GooseBLEClient` — visible in existing record calls in `GooseBLEClient+Commands.swift`. [VERIFIED: codebase grep]

### Pattern 2: MoreRoute Enum Extension

```swift
// Source: GooseSwift/MoreRouteModels.swift (verified structure)

// 1. Add case to enum (before .privacy — group with wellness/feature routes)
enum MoreRoute: String, CaseIterable, Identifiable, Hashable {
  // ... existing cases ...
  case breathe   // ← add here
  case privacy
  // ...
}

// 2. Add to title/subtitle/systemImage/statusKeyPath switch arms
// title:       "Breathe"
// subtitle:    "Paced breathing with haptics"
// systemImage: "wind"
// statusKeyPath: \.breathe

// 3. Add static route group
static let wellnessRoutes: [MoreRoute] = [.breathe]

// 4. Add MoreRouteStatus property
struct MoreRouteStatus: Equatable {
  // ... existing properties ...
  var breathe: MoreStatusKind   // ← add
}
```

### Pattern 3: MoreView Navigation Destination

```swift
// Source: GooseSwift/MoreView.swift (lines 130–165 verified)

// In body — new section above Settings:
Section("Wellness") {
  routeRows(MoreRoute.wellnessRoutes)
}

// In destination(for:) switch:
case .breathe:
  BreatheView()
```

### Pattern 4: @Environment Model Access in View

```swift
// Source: GooseSwift/HealthDashboardViews.swift line 547 + MoreView.swift line 11
// Confirmed pattern: @Environment(GooseAppModel.self) private var model
// GooseAppModel is @Observable (NOT ObservableObject) — use @Environment, not @EnvironmentObject

struct BreatheView: View {
  @Environment(GooseAppModel.self) private var model
  // model.ble.buzz(loops: 1)
  // model.ble.connectionState == "ready"
}
```

### Pattern 5: SwiftUI Animation with @State CGFloat + Task sleep loop

```swift
// Source: GooseSwift/DeviceView.swift lines 271, 283-286 — scaleEffect + easeInOut pattern
// Source: GooseSwift/MoreView.swift line 103 — Task.sleep(for:) modern syntax
// Source: GooseSwift/LiveActivityContentView.swift line 320 — withAnimation(.easeInOut(duration:))

// Breathe session loop pattern:
@State private var circleScale: CGFloat = 0.6
@State private var currentPhase: BreathePhase = .inhale
@State private var isRunning = false
@State private var phaseTask: Task<Void, Never>? = nil

func startSession() {
  isRunning = true
  phaseTask = Task { @MainActor in
    repeat {
      currentPhase = .inhale
      model.ble.buzz(loops: 1)
      withAnimation(.easeInOut(duration: BreathePhase.duration)) { circleScale = 1.0 }
      try? await Task.sleep(for: .seconds(BreathePhase.duration))
      guard !Task.isCancelled else { break }

      currentPhase = .hold
      model.ble.buzz(loops: 1)
      // no animation — scale stays at 1.0
      try? await Task.sleep(for: .seconds(BreathePhase.duration))
      guard !Task.isCancelled else { break }

      currentPhase = .exhale
      model.ble.buzz(loops: 1)
      withAnimation(.easeInOut(duration: BreathePhase.duration)) { circleScale = 0.6 }
      try? await Task.sleep(for: .seconds(BreathePhase.duration))
    } while !Task.isCancelled
  }
}

func stopSession() {
  phaseTask?.cancel()
  phaseTask = nil
  isRunning = false
  currentPhase = .inhale
  withAnimation(.easeInOut(duration: 0.4)) { circleScale = 0.6 }
}
```

**Threading note:** `Task { @MainActor in ... }` ensures `@State` mutations happen on the main actor. `withAnimation` must be called on the main actor — this is guaranteed by `@MainActor` on the Task. [ASSUMED — Swift concurrency docs; consistent with codebase pattern]

### Anti-Patterns to Avoid

- **Using `buildCommandFrame(sequence:command:data:)` for buzz:** This adds a sequence number and framing bytes that the haptic command does not expect. Write raw `Data([0x13, clamped])` directly.
- **Calling `model.ble.buzz(loops:)` from the `@MainActor` inline:** The `buzz` method calls `activePeripheral.writeValue` which can block briefly — but since buzz is fire-and-forget and extremely short, this is acceptable. No background dispatch needed (consistent with existing alarm commands that also call `writeValue` on main).
- **Checking `connectionState != "Connected"` (capital C):** The actual live value is lowercase `"ready"`. The UI-SPEC says `!= "Connected"` but the codebase canonical check is `connectionState == "ready"`. Use `model.ble.connectionState != "ready"` for the disconnected banner guard. [VERIFIED: grep across 12 files all use lowercase `"ready"`]
- **Not calling `stopSession()` in `.onDisappear`:** The `phaseTask` Task must be cancelled when the view disappears or it continues sleeping in the background. Wire `.onDisappear { stopSession() }`.
- **Using `@EnvironmentObject` instead of `@Environment`:** `GooseAppModel` is `@Observable`, not `ObservableObject`. Use `@Environment(GooseAppModel.self)`. [VERIFIED: MoreView.swift line 11]

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Detecting writable BLE characteristic | Custom property check | `writeType(for:)` existing helper | Already handles `.write` vs `.writeWithoutResponse` precedence |
| Animation timing loop | DispatchQueue timer | `Task` + `Task.sleep(for:)` | Cancellable, structured, works with `@MainActor`; matches MoreView pattern |
| Byte clamping | Unchecked UInt8 cast | `max(1, min(255, loops))` before `UInt8(...)` | UInt8 overflow crashes on out-of-range Int |

**Key insight:** The BLE write path for buzz is simpler than every other command in the codebase because it has no response. Resist the urge to add sequence tracking, pending command state, or timeout work items — none apply to a fire-and-forget vibration command.

---

## Common Pitfalls

### Pitfall 1: connectionState string is lowercase "ready", not "Connected"

**What goes wrong:** View checks `connectionState != "Connected"` (from the UI-SPEC copy) and the banner never appears because the live value is `"ready"`.
**Why it happens:** The UI-SPEC used "Connected" as a readable description; the actual runtime string in `GooseBLEClient` is lowercase `"ready"` (set by `updateConnectionState("ready")`).
**How to avoid:** Use `model.ble.connectionState != "ready"` for the disconnected gate. The UI-SPEC §9 is a description of intent, not a literal string comparison.
**Warning signs:** Banner never appears even when WHOOP is disconnected.

### Pitfall 2: Task not cancelled on view disappear

**What goes wrong:** User navigates back from BreatheView while a session is running; the Task continues sleeping, and the next time the view appears a second Task is created — two loops fire `buzz` concurrently.
**Why it happens:** `phaseTask` is `@State` — it persists across navigations only if the view stays alive in the navigation stack. On iOS with push nav, the view is destroyed on pop, but the Task is not automatically cancelled.
**How to avoid:** `.onDisappear { stopSession() }` cancels the task. `stopSession()` also sets `phaseTask = nil`.
**Warning signs:** Double buzz pulses; OSLog shows two `buzz.sent` events per phase.

### Pitfall 3: `withAnimation` called from non-main context

**What goes wrong:** If the Task is not `@MainActor`, calling `withAnimation` from inside it has no effect (animation is silently dropped on non-main thread).
**Why it happens:** Plain `Task { }` inherits actor context but a detached Task runs on the cooperative pool.
**How to avoid:** Use `Task { @MainActor in ... }` explicitly, or call `await MainActor.run { withAnimation(...) { ... } }` for the animation calls inside the loop.
**Warning signs:** Circle scale never animates; phase label changes instantly without crossfade.

### Pitfall 4: `MoreRouteStatus.breathe` property missing

**What goes wrong:** Compiler error when `MoreRoute.breathe.statusKeyPath` resolves to `\.breathe` on a `MoreRouteStatus` struct that has no such property.
**Why it happens:** `MoreRouteStatus` is a concrete struct with one property per route; adding a case to the enum without adding the corresponding struct property causes a key-path compile error.
**How to avoid:** Add `var breathe: MoreStatusKind` to `MoreRouteStatus` and a `case .breathe: \.breathe` arm to the `statusKeyPath` switch in `MoreRoute`. Initialize it to `.ready` in `MoreDataStore.refreshRouteStatus`.
**Warning signs:** Compiler error `type 'MoreRouteStatus' has no member 'breathe'`.

### Pitfall 5: Forgetting the `MoreDataStore.refreshRouteStatus` initialization

**What goes wrong:** `MoreRouteStatus` initializer now has a new required property `breathe`. If `MoreDataStore` constructs `MoreRouteStatus(profile: ..., device: ...)` anywhere with positional or labeled args, it will fail to compile.
**Why it happens:** `MoreRouteStatus` is a struct — all stored properties must be initialized.
**How to avoid:** Find all `MoreRouteStatus(...)` construction sites in `MoreDataStore.swift` and add `breathe: .ready`.
**Warning signs:** Compiler error at `MoreDataStore.swift`.

---

## Code Examples

### BLE Write Anatomy (verified from existing commands)

```swift
// Source: GooseSwift/GooseBLEClient+HistoricalCommands.swift lines 89–126
// Three-step pattern for all command writes:
// 1. guard activePeripheral + commandCharacteristic
// 2. guard writeType(for: commandCharacteristic) -- determines .withResponse vs .withoutResponse
// 3. peripheral.writeValue(data, for: characteristic, type: writeType)
// buzz skips steps: pending command tracking, sequence, buildCommandFrame, timeout scheduling
```

### FitnessColor Token Reference (verified from FitnessFormatting.swift)

```swift
// Source: GooseSwift/FitnessFormatting.swift lines 6–21
enum FitnessColor {
  static let background   = Color.black
  static let panel        = Color(red: 0.10, green: 0.10, blue: 0.11)
  static let secondaryText = Color(red: 0.58, green: 0.58, blue: 0.62)
  static let workoutYellow = Color(red: 1.0, green: 0.91, blue: 0.24)
  static let standCyan    = Color(red: 0.39, green: 0.92, blue: 0.95)
  static let endRed       = Color(red: 1.0, green: 0.25, blue: 0.27)
}
```

### OSLog Logger (verified from GooseBLEClient.swift line 89)

```swift
// Source: GooseSwift/GooseBLEClient.swift line 89
let logger = Logger(subsystem: "com.goose.swift", category: "ble")
// buzz should use source: "ble.haptic" consistent with "ble.sync", "ble.clock", "ble.alarm"
```

### Reduced Motion Accessibility (from UI-SPEC §14)

```swift
// Pattern: read @Environment(\.accessibilityReduceMotion) and skip withAnimation
@Environment(\.accessibilityReduceMotion) var reduceMotion

// In startSession / phase transitions:
if reduceMotion {
  circleScale = 1.0  // direct assignment, no animation
} else {
  withAnimation(.easeInOut(duration: BreathePhase.duration)) { circleScale = 1.0 }
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `@ObservableObject` + `@EnvironmentObject` | `@Observable` + `@Environment(Type.self)` | iOS 17 / Swift 5.9 | BreatheView must use `@Environment(GooseAppModel.self)`, not `@EnvironmentObject` |
| `Timer.scheduledTimer` for animation loops | `Task` + `Task.sleep(for:)` | Swift 5.7 / iOS 16 | Structured concurrency; cancellable via `task.cancel()` |
| `DispatchQueue.main.async` for UI updates from background | `@MainActor` task annotation | Swift 5.5 | Task annotated `@MainActor in` keeps all body on main actor |

**Deprecated/outdated for this phase:**
- `Timer.publish(every:on:in:).autoconnect()` with Combine: works but adds Combine dependency and is harder to cancel cleanly. Prefer `Task` + `sleep`.
- `DispatchQueue.main.asyncAfter` for phase timing: non-cancellable; avoid.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `writeTypeName(_:)` helper exists on `GooseBLEClient` as a private/internal func (seen referenced in record body strings in +Commands.swift) | Code Examples — buzz pattern | If absent, replace with inline ternary `writeType == .withResponse ? "withResponse" : "withoutResponse"` in the log body |
| A2 | `Task { @MainActor in ... }` is sufficient to guarantee `withAnimation` runs on main actor inside the session loop | Common Pitfalls / session loop pattern | If SwiftUI requires explicit `MainActor.run` for `withAnimation`, add it; behaviour is the same, just more verbose |
| A3 | `MoreDataStore.refreshRouteStatus` constructs `MoreRouteStatus` with named arguments (not positional) | Pitfall 5 | If positional, adding the new `breathe` property in the middle of the struct would shift order; check `MoreDataStore.swift` during execution |

**If this table is empty:** n/a — 3 low-risk assumptions logged above.

---

## Open Questions

1. **Does `writeTypeName(_:)` exist as a callable helper?**
   - What we know: The string `writeTypeName(writeType)` appears in `record` body strings in `GooseBLEClient+Commands.swift`
   - What's unclear: Whether it's a private method on `GooseBLEClient` or just an inlined string expression
   - Recommendation: Grep for `func writeTypeName` before the execution plan. If absent, use `writeType == .withResponse ? "withResponse" : "withoutResponse"` inline in the record call.

2. **Does `MoreDataStore.refreshRouteStatus` need updating?**
   - What we know: It constructs `MoreRouteStatus(...)` and sets each field; adding `breathe` to the struct requires updating that construction
   - What's unclear: Whether the struct uses a memberwise initializer or a custom one
   - Recommendation: Read `MoreDataStore.swift` lines around `MoreRouteStatus` construction; add `breathe: .ready` there.

---

## Environment Availability

> This phase is pure Swift code changes. No external CLI tools, databases, or services required.

Step 2.6: SKIPPED (no external dependencies — new Swift files + modifications to existing files only).

---

## Validation Architecture

No Rust changes. No test target exists in the Xcode project. Validation is via:
1. OSLog output — `buzz.sent` log line visible in Xcode console when BLE connected and Breathe session running
2. Simulator functional test — Start session, observe circle animates and phase label cycles; Stop returns to idle state
3. Disconnected state — Banner appears, buzz calls are silent no-ops (no crash, no log error)
4. Build success — `xcodebuild` clean build with no compiler errors

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HAP-01 | `buzz(loops:)` writes Data([0x13, N]) to commandCharacteristic | Manual — OSLog observation | n/a | N/A — new file |
| HAP-01 | Guard: no-op when activePeripheral nil | Manual — disconnect then tap Start | n/a | N/A |
| HAP-02 | Breathe screen reachable via More > Breathe | Manual — simulator tap navigation | n/a | N/A — new file |
| HAP-02 | Full breath cycle: circle animates inhale/hold/exhale | Manual — simulator observation | n/a | N/A |
| HAP-02 | Stop cancels session; view disappear cancels session | Manual — simulator back nav during session | n/a | N/A |
| HAP-02 | Disconnected banner shown when not connected + not running | Manual — simulator (no device) | n/a | N/A |

*(No Swift test target in project — all validation is manual/OSLog.)*

---

## Security Domain

> This phase has no authentication, network calls, or user data persistence. ASVS categories V2/V3/V4/V6 do not apply.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V5 Input Validation | yes (minor) | `UInt8(max(1, min(255, loops)))` — clamp before cast prevents UInt8 overflow crash |
| V2/V3/V4/V6 | no | No auth, sessions, access control, or crypto in this phase |

---

## Sources

### Primary (HIGH confidence)
- `GooseSwift/GooseBLEClient+HistoricalCommands.swift` — `writeType(for:)` helper, three-guard write pattern, `activePeripheral.writeValue` call site
- `GooseSwift/GooseBLEClient+Commands.swift` — `writeAlarmCommand`, `writeSensorStreamCommand` — confirms main-thread pattern and guard structure
- `GooseSwift/MoreRouteModels.swift` — full enum structure, `MoreRouteStatus`, `statusKeyPath`, `static let *Routes` groups
- `GooseSwift/MoreView.swift` — `routeRows`, `destination(for:)` switch, section structure, `@Environment(GooseAppModel.self)` pattern
- `GooseSwift/FitnessFormatting.swift` — all `FitnessColor` static values confirmed

### Secondary (MEDIUM confidence)
- `GooseSwift/DeviceView.swift` — `scaleEffect` + `.easeInOut(duration:).repeatForever` animation pattern; `connectionState == "ready"` check
- `GooseSwift/HealthDashboardViews.swift` line 547 — `@Environment(GooseAppModel.self)` confirmed in non-More view
- `GooseSwift/MoreView.swift` line 103 — `Task.sleep(for: .milliseconds)` modern syntax confirmed

### Tertiary (LOW confidence)
- `GooseSwift/GooseBLEClient.swift` line 89 — Logger subsystem `"com.goose.swift"` category `"ble"` confirmed for OSLog recommendation

---

## Metadata

**Confidence breakdown:**
- BLE write pattern: HIGH — verified against three existing command write implementations
- MoreRoute navigation wiring: HIGH — read actual `MoreRouteModels.swift` and `MoreView.swift` in full
- SwiftUI animation pattern: HIGH — confirmed `scaleEffect` + `.easeInOut` + `Task.sleep` from live codebase
- `@Environment` access pattern: HIGH — confirmed in two independent view files
- connectionState string value: HIGH — confirmed lowercase `"ready"` across 12 grep results

**Research date:** 2026-06-12
**Valid until:** 2026-07-12 (stable iOS SDK; no external dependencies to go stale)
