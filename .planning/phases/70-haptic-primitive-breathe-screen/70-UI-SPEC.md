---
phase: 70
name: Haptic Primitive + Breathe Screen
status: draft
date: 2026-06-12
---

# UI-SPEC — Phase 70: Haptic Primitive + Breathe Screen

## 1. Design System

**Tool:** SwiftUI (native — no shadcn, no third-party design system)
**Target:** iOS 26.0, SwiftUI
**Color palette:** `FitnessColor` (existing enum in `FitnessFormatting.swift`) — used for the Breathe screen's full-bleed dark canvas. Standard system colors (`Color.primary`, `.secondary`) used for MoreView list row.
**State management:** `@Environment(GooseAppModel.self)` — existing `@Observable` pattern.

---

## 2. Spacing Scale

8-point base scale. Exceptions noted.

| Token | Value | Usage |
|-------|-------|-------|
| xs    | 4px   | Internal label-to-sublabel gap (VStack spacing: 3 in MoreRouteRow — match exactly) |
| sm    | 8px   | MoreRouteRow horizontal padding; button icon padding |
| md    | 16px  | Screen horizontal padding; label-to-circle gap |
| lg    | 24px  | Phase label bottom clearance above Stop button |
| xl    | 32px  | Stop button bottom safe-area padding |
| circle-diameter | 220pt | Breathing circle at rest (scale 0.6 = 132pt visible, scale 1.0 = 220pt visible) |
| touch-target    | 44pt minimum | Stop button height minimum |

---

## 3. Typography

Exactly 3 sizes, 2 weights.

| Role | Size | Weight | Line-height | Usage |
|------|------|--------|-------------|-------|
| phase-label | 20pt | semibold (600) | 1.2 | "INHALE" / "HOLD" / "EXHALE" displayed below circle |
| body | 16pt | regular (400) | 1.5 | Disconnected banner subtitle; Stop button label |
| caption | 12pt | regular (400) | 1.3 | MoreRouteRow subtitle ("Paced breathing with haptics") — matches existing `.caption` style |

Phase label uses `.font(.system(size: 20, weight: .semibold))` with `.tracking(2.0)` (all-caps letter spacing).

---

## 4. Color Contract (Breathe Screen)

The Breathe screen uses a full-bleed dark canvas — same surface as `FitnessLiveWorkoutView`. MoreView list row uses standard list styling.

| Role | Color | Exact value | Reserved for |
|------|-------|-------------|--------------|
| 60% — canvas | `FitnessColor.background` | `Color.black` | Screen background (`.ignoresSafeArea()`) |
| 30% — surface | `FitnessColor.panel` | `Color(red: 0.10, green: 0.10, blue: 0.11)` | Circle fill at rest; Stop button background |
| 10% — accent | `FitnessColor.standCyan` | `Color(red: 0.39, green: 0.92, blue: 0.95)` | Circle stroke ring; phase label foreground; Stop button when active session |
| secondary text | `FitnessColor.secondaryText` | `Color(red: 0.58, green: 0.58, blue: 0.62)` | Disconnected banner text; idle state subtext |
| destructive | `FitnessColor.endRed` | `Color(red: 1.0, green: 0.25, blue: 0.27)` | Not used in this phase |

**Accent (`FitnessColor.standCyan`) is reserved for:** the breathing circle stroke, the phase label text during an active session, and the Stop button label/background tint.

**Rationale for cyan:** The existing workout screen uses `workoutYellow` for elapsed time (exercise energy) and `exerciseGreen`/`lime` for HR ring (cardio effort). Cyan (`standCyan`) is semantically unoccupied by those contexts and reads as calm / respiratory — appropriate for breathwork. It avoids conflating Breathe with an active workout.

---

## 5. Component Hierarchy

```
BreatheView                          // new file: GooseSwift/BreatheView.swift
  ├── ZStack
  │   ├── FitnessColor.background    // full-bleed canvas, ignoresSafeArea
  │   └── VStack(spacing: 0)
  │       ├── Spacer()               // push content to vertical center
  │       ├── BreatheCircle          // animated circle component (see §6)
  │       ├── Spacer().frame(height: 16)
  │       ├── BreathePhaseLabel      // "INHALE" / "HOLD" / "EXHALE" (see §7)
  │       ├── Spacer()
  │       ├── BreatheDisconnectedBanner  // conditional — only when disconnected (see §9)
  │       └── BreatheStopButton      // always visible when session running (see §8)
  │           // START button shown when session not running (see §8)
```

**State owned by BreatheView** (all `@State`):

```swift
@State private var isRunning = false
@State private var currentPhase: BreathePhase = .inhale
@State private var circleScale: CGFloat = 0.6
@State private var phaseTask: Task<Void, Never>? = nil
```

**Enum:**

```swift
enum BreathePhase {
  case inhale, hold, exhale

  var label: String {
    switch self {
    case .inhale: "INHALE"
    case .hold:   "HOLD"
    case .exhale:  "EXHALE"
    }
  }

  static let duration: TimeInterval = 4.0
}
```

**Model access:** `@Environment(GooseAppModel.self) private var model` — used to call `model.ble.buzz(loops: 1)` and read `model.ble.connectionState`.

---

## 6. Animation Spec — BreatheCircle

```
BreatheCircle
  ZStack
    Circle()                          // base fill
      .fill(FitnessColor.panel)
      .frame(width: 220, height: 220)
      .scaleEffect(circleScale)
    Circle()                          // stroke ring
      .strokeBorder(FitnessColor.standCyan.opacity(0.72), lineWidth: 3)
      .frame(width: 220, height: 220)
      .scaleEffect(circleScale)
```

| Phase | `circleScale` target | Duration | Easing |
|-------|---------------------|----------|--------|
| inhale start → end | 0.6 → 1.0 | 4.0s | `.easeInOut` |
| hold (static) | 1.0 (no change) | 4.0s | n/a — no animation |
| exhale start → end | 1.0 → 0.6 | 4.0s | `.easeInOut` |

**Animation application:**

```swift
// on inhale:
withAnimation(.easeInOut(duration: BreathePhase.duration)) {
  circleScale = 1.0
}

// on hold: no withAnimation call — scale stays at 1.0

// on exhale:
withAnimation(.easeInOut(duration: BreathePhase.duration)) {
  circleScale = 0.6
}
```

**Scale range rationale:** 0.6 (contracted, 132pt) → 1.0 (expanded, 220pt) provides a 67% diameter increase — perceptible without being disorienting on a 390pt-wide device canvas.

---

## 7. Phase Label — BreathePhaseLabel

```
Text(currentPhase.label)
  .font(.system(size: 20, weight: .semibold))
  .tracking(2.0)
  .foregroundStyle(isRunning ? FitnessColor.standCyan : FitnessColor.secondaryText)
  .animation(.easeInOut(duration: 0.25), value: currentPhase)
  .contentTransition(.opacity)
```

- When session is not running: display "INHALE" in `FitnessColor.secondaryText` (dim, inactive).
- When session is running: display current phase label in `FitnessColor.standCyan`.
- Phase label crossfades (`.contentTransition(.opacity)`, 0.25s) on phase change. The label text changes at the same moment the new phase animation begins.

---

## 8. Start / Stop Button States

### Start State (session not running)

```
Button("Start") { startSession() }
  .font(.body.weight(.semibold))
  .foregroundStyle(FitnessColor.standCyan)
  .frame(width: 160, height: 48)
  .background(FitnessColor.standCyan.opacity(0.14), in: Capsule())
  .padding(.bottom, 32)
```

### Stop State (session running)

```
Button("Stop") { stopSession() }
  .font(.body.weight(.semibold))
  .foregroundStyle(.white)
  .frame(width: 160, height: 48)
  .background(FitnessColor.panel, in: Capsule())
  .padding(.bottom, 32)
```

**Transition between states:** use `if isRunning { StopButton } else { StartButton }` — no crossfade needed, state flip is intentional and immediate.

**Minimum touch target:** 48pt height (above the 44pt minimum). Width 160pt.

---

## 9. Disconnected State

When `model.ble.connectionState` indicates no connected WHOOP device, a banner is shown between the phase label and the Start button.

**Detection:** `model.ble.connectionState != "Connected"` (matches existing pattern used throughout the codebase).

```
BreatheDisconnectedBanner          // only rendered when !isConnected
  HStack(spacing: 8)
    Image(systemName: "sensor.tag.radiowaves.forward")
      .foregroundStyle(FitnessColor.secondaryText)
    Text("Connect WHOOP to enable haptics")
      .font(.system(size: 16, weight: .regular))
      .foregroundStyle(FitnessColor.secondaryText)
  .padding(.horizontal, 16)
  .padding(.vertical, 10)
  .background(FitnessColor.panel, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
  .padding(.bottom, 16)
```

**Behaviour when disconnected:**
- Start button remains active and tappable — session can still run.
- `buzz(loops:)` is a no-op when disconnected (guarded in HAP-01 at the BLE layer).
- The banner communicates why the haptic cue is absent; it does not block usage.
- Banner is not shown during an active session (it would distract during breathing). Show only when `!isRunning && !isConnected`.

---

## 10. Navigation Integration

### MoreRoute.breathe entry

**Section:** Add `case breathe` to `MoreRoute` enum (before `.privacy` — group with wellness/feature routes, not developer tools).

**New static route group** (add to `MoreRouteModels.swift`):

```swift
static let wellnessRoutes: [MoreRoute] = [.breathe]
```

**Title:** `"Breathe"`
**Subtitle:** `"Paced breathing with haptics"`
**systemImage:** `"wind"` (SF Symbols — calm, breath-appropriate; available iOS 14+)
**statusKeyPath:** points to a new `breathe: MoreStatusKind` property on `MoreRouteStatus` (default `.ready`)

**MoreView section** (new section above "Settings"):

```swift
Section("Wellness") {
  routeRows(MoreRoute.wellnessRoutes)
}
```

**navigationDestination arm** (inside `MoreView.destination(for:)`):

```swift
case .breathe:
  BreatheView()
```

**MoreRouteRow rendering:** Uses existing `MoreRouteRow(route: .breathe, status: .ready)` — no custom row needed. Status badge hidden (default `showsStatus: false`).

---

## 11. Screen Layout — BreatheView Navigation Chrome

```swift
BreatheView()
  .navigationTitle("Breathe")
  .navigationBarTitleDisplayMode(.inline)
  .toolbar(.hidden, for: .tabBar)       // hide tab bar during session
  .background(FitnessColor.background.ignoresSafeArea())
  .toolbarBackground(FitnessColor.background, for: .navigationBar)
  .toolbarColorScheme(.dark, for: .navigationBar)
```

Navigation back button: default system back chevron, no customisation.

---

## 12. Session Loop Logic Contract

This is a UI-level contract so the executor knows what the view drives — not a full implementation spec.

```
startSession():
  isRunning = true
  phaseTask = Task {
    repeat {
      // --- INHALE ---
      currentPhase = .inhale
      buzz(loops: 1)
      animate circleScale → 1.0 over 4s (.easeInOut)
      await Task.sleep(for: .seconds(4))

      // --- HOLD ---
      currentPhase = .hold
      buzz(loops: 1)
      // no circle animation
      await Task.sleep(for: .seconds(4))

      // --- EXHALE ---
      currentPhase = .exhale
      buzz(loops: 1)
      animate circleScale → 0.6 over 4s (.easeInOut)
      await Task.sleep(for: .seconds(4))
    } while !Task.isCancelled
  }

stopSession():
  phaseTask?.cancel()
  phaseTask = nil
  isRunning = false
  currentPhase = .inhale
  withAnimation(.easeInOut(duration: 0.4)) { circleScale = 0.6 }
```

**Task lifecycle:** `phaseTask` is cancelled on `stopSession()` and on `.onDisappear { stopSession() }`.

---

## 13. Copywriting Contract

| Element | Copy | Notes |
|---------|------|-------|
| Navigation title | `"Breathe"` | `.inline` display mode |
| MoreRouteRow title | `"Breathe"` | Matches `MoreRoute.breathe.title` |
| MoreRouteRow subtitle | `"Paced breathing with haptics"` | Matches `MoreRoute.breathe.subtitle` |
| Start button | `"Start"` | Plain verb — no noun needed, context is clear |
| Stop button | `"Stop"` | Plain verb |
| Phase labels | `"INHALE"` / `"HOLD"` / `"EXHALE"` | All-caps, tracking 2.0 |
| Disconnected banner | `"Connect WHOOP to enable haptics"` | No period; sentence case except WHOOP |
| Disconnected icon | SF Symbol `"sensor.tag.radiowaves.forward"` | Matches existing device icon in MoreRouteModels |
| Empty/idle subtext | none | Phase label at dim shows "INHALE" — sufficient |
| Error state | none | `buzz` failures are silent (OSLog only, per HAP-01 spec) |
| Accessibility label — Start | `"Start breathing session"` | Explicit `.accessibilityLabel` |
| Accessibility label — Stop | `"Stop breathing session"` | Explicit `.accessibilityLabel` |
| Accessibility label — circle | `"Breathing circle, \(currentPhase.label.lowercased()) phase"` | `.accessibilityLabel` on `BreatheCircle` |

---

## 14. Accessibility

- Start/Stop buttons: minimum 48pt height, explicit `.accessibilityLabel` (see §13).
- Phase label: `.accessibilityAddTraits(.updatesFrequently)` — announces on each phase change.
- Disconnected banner: `.accessibilityElement(children: .combine)`.
- Reduced motion: `@Environment(\.accessibilityReduceMotion) var reduceMotion`. When `reduceMotion == true`, skip `withAnimation` — set `circleScale` directly without animation. Phase label still updates.
- Color: accent cyan passes contrast on `Color.black` background (luminance ratio approximately 7:1 — exceeds AA large text threshold).

---

## 15. New Files

| File | Purpose |
|------|---------|
| `GooseSwift/BreatheView.swift` | Breathe screen — all view structs for this feature |
| `GooseSwift/GooseBLEClient+Haptics.swift` | `buzz(loops:)` BLE command (HAP-01, not a UI file — listed for completeness) |

**Existing files modified:**

| File | Change |
|------|--------|
| `GooseSwift/MoreRouteModels.swift` | Add `case breathe` + `wellnessRoutes` group + `MoreRouteStatus.breathe` property |
| `GooseSwift/MoreView.swift` | Add "Wellness" section + `case .breathe: BreatheView()` destination arm |

---

## 16. Pre-Population Sources

| Decision | Source |
|----------|--------|
| Box breathing 4s/4s/4s | CONTEXT.md — locked |
| Circle scaleEffect 0.6 → 1.0 | CONTEXT.md — locked |
| Phase labels INHALE/HOLD/EXHALE | CONTEXT.md — locked |
| buzz(loops: 1) at phase start | CONTEXT.md — locked |
| MoreRoute.breathe push nav | CONTEXT.md — locked |
| Row subtitle copy | CONTEXT.md — locked |
| Disconnected message copy | CONTEXT.md — locked |
| FitnessColor palette tokens | Codebase scan — `FitnessFormatting.swift` |
| MoreRouteRow structure | Codebase scan — `MoreProfileViews.swift` |
| navigationDestination pattern | Codebase scan — `MoreView.swift` |
| FitnessColor.standCyan as accent | Claude's discretion — semantically unoccupied; calm/respiratory |
| Reduced-motion guard | Claude's discretion — accessibility default |
| Tab bar hidden during session | Claude's discretion — full focus during breathwork |
| "wind" SF Symbol for route | Claude's discretion — SF Symbols, breath-appropriate |
| .easeInOut easing curve | Claude's discretion — smooth, non-jarring for breathwork |
