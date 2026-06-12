# Phase 70: Haptic Primitive + Breathe Screen - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

HAP-01: Add `buzz(loops:)` to `GooseBLEClient` via a new `GooseBLEClient+Haptics.swift` extension — issues BLE cmd 0x13 over the command characteristic with a loops byte.

HAP-02: Implement a Breathe screen accessible from More > Breathe — full breath cycle (inhale/hold/exhale) animated with a pulsing circle; `buzz(loops: 1)` fires at each phase transition to pace the user via strap vibration. Session is free-running with a Stop button. Screen always visible; haptics disabled with a clear message when no WHOOP is connected.

No Swift protocols, no server calls, no new dependencies.

</domain>

<decisions>
## Implementation Decisions

### Breathe Screen Navigation
- Entry point: new `MoreRoute.breathe` case on the existing `MoreRoute` enum in `MoreRouteModels.swift`
- Presented as a push navigation destination inside the More tab's `NavigationStack(path: $router.morePath)` — consistent with all existing MoreRoute destinations
- Row in MoreView labeled "Breathe" with subtitle "Paced breathing with haptics"
- Screen is always accessible (reachable even without device); haptic calls are no-ops when disconnected

### Breath Cycle
- Pattern: box breathing — 4s inhale / 4s hold / 4s exhale
- Animation: `Circle()` with `scaleEffect` animated from 0.6→1.0 (inhale) and back (exhale), static at max during hold; phase label ("INHALE", "HOLD", "EXHALE") below the circle
- `buzz(loops: 1)` fired at the start of each phase transition (on inhale start, hold start, exhale start)
- Session is free-running (no fixed cycle count); user taps Stop to end

### BLE Haptic Layer — buzz(loops:)
- New file: `GooseSwift/GooseBLEClient+Haptics.swift`
- Signature: `func buzz(loops: Int)` — parameter is `Int` clamped internally to `max(1, min(255, loops))` before encoding as `UInt8`
- Payload: `[0x13, UInt8(clamped)]` written directly to `commandCharacteristic` using `activePeripheral.writeValue(_:for:type:)` — no frame sequence wrapping (simpler than historical commands; haptic writes are fire-and-forget)
- Guard: if `activePeripheral == nil` or `commandCharacteristic == nil`, log via OSLog and return silently — consistent with existing command pattern

### Claude's Discretion
- Exact circle animation easing curve and scale range
- Color scheme for Breathe screen (should follow FitnessColor / existing palette)
- Whether to show a cycle counter or elapsed time during the session
- OSLog subsystem for haptic writes

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `GooseBLEClient+HistoricalCommands.swift` — `writeType(for:)` helper + `activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)` pattern; buzz follows same guard pattern
- `MoreRouteModels.swift` — `enum MoreRoute: String, CaseIterable, Identifiable, Hashable`; add `case breathe`
- `AppShellView.swift` + `AppRouter.swift` — `router.morePath` NavigationStack with `navigationDestination(for: MoreRoute.self)` for push routing
- `FitnessColor`, `GooseColors` — existing color palette for fitness/health UI

### Established Patterns
- BLE command extension files: `GooseBLEClient+Commands.swift`, `GooseBLEClient+HistoricalCommands.swift` — new `+Haptics.swift` follows exact same extension structure
- More tab route: add case to `MoreRoute`, add `navigationDestination` arm in AppShellView or MoreView, add list row in MoreView
- No-connection disabled state pattern: existing views check `model.connectionState` and show disabled UI with explanatory text

### Integration Points
- `GooseBLEClient.commandCharacteristic: CBCharacteristic?` — the write target for buzz
- `GooseBLEClient.activePeripheral: CBPeripheral?` — nil guard for disconnected state
- `GooseAppModel.ble: GooseBLEClient` — how SwiftUI views reach buzz (call via `model.ble.buzz(loops:)`)
- `GooseAppModel.connectionState: String` — observable property for disconnected-state UI gate

</code_context>

<specifics>
## Specific Ideas

- Buzz is fired at **start of each phase** (not end): inhale start → buzz, hold start → buzz, exhale start → buzz
- Breathe screen shows the current phase label and an animated circle; no other metrics needed
- "Connect WHOOP to enable haptics" message shown as subtitle or overlay when `model.connectionState` indicates disconnected

</specifics>

<deferred>
## Deferred Ideas

- Configurable breath timings (user-settable inhale/hold/exhale durations) — keep as static let constants for now, phase 71+ if needed
- Multi-pattern sessions (Weil 4-7-8, coherence breathing) — out of scope for HAP-02
- Background audio cue alongside haptic — no new dependencies allowed

</deferred>
