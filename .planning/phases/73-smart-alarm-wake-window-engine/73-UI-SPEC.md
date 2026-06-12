---
phase: 73
name: Smart Alarm + Wake-Window Engine
status: draft
scope: HAP-03 only — Wake Alarm section in CoachSleepRouteView
date: 2026-06-12
---

# UI-SPEC: Phase 73 — Smart Alarm + Wake-Window Engine

## 1. Scope

One new UI element: a "ALARME DE DESPERTAR" section appended at the bottom of `CoachSleepRouteView`, inside the existing `ScrollView > VStack(spacing: 18)`. HAP-04 (`GooseWakeWindowManager`) is a non-UI stub — no design contract required for it.

---

## 2. Design System

**Tool:** None (no shadcn — Swift/SwiftUI project).
**Pattern source:** Existing `CoachInfoGroup` + `CoachInfoRow` primitives in `CoachRouteViews.swift`. All new UI reuses these verbatim — no new component primitives.

---

## 3. Spacing

Follows the existing 8-point scale already used in `CoachSleepRouteView`:

| Token | Value | Usage |
|-------|-------|-------|
| xs | 4px | `VStack` inner label spacing (`.title2`/`.subheadline` pair) |
| sm | 8px | `CoachInfoGroup` title bottom padding |
| md | 12px | `CoachInfoGroup` card inner padding |
| lg | 16px | `ScrollView` outer `.padding(16)` — inherited, not repeated |
| xl | 18px | Top-level `VStack(spacing: 18)` — inherited |

The Wake Alarm section is a standard `CoachInfoGroup` entry, so it inherits all of the above automatically.

---

## 4. Typography

Source: `CoachInfoGroup` and `CoachInfoRow` — match exactly.

| Role | Font | Weight | Color |
|------|------|--------|-------|
| Section title | `.system(size: 11)` | `.black` (900) | `.secondary` |
| Row label | `.subheadline` | `.regular` (400) | `.secondary` |
| Row value / time | `.subheadline` | `.semibold` (600) | `.primary` |
| Status message | `.caption` | `.regular` (400) | `.secondary` |
| Button label | `.body` | `.semibold` (600) | see §6 |

No new type sizes are introduced.

---

## 5. Color

Follows existing 60/30/10 split. Sleep Coach tint is `.indigo`.

| Role | Value | Reserved for |
|------|-------|-------------|
| Background | `.gooseScreenBackground()` | Inherited screen background |
| Card surface | `.quaternary.opacity(0.4)` | `CoachInfoGroup` rounded rect — reused |
| Accent (armed) | `.indigo` | Arm button foreground label when idle |
| Accent fill (armed) | `.indigo.opacity(0.14)` | Arm button background fill when idle |
| Accent (cancel) | `.red` | Button foreground label when alarm is armed |
| Accent fill (cancel) | `.red.opacity(0.14)` | Button background fill when armed |
| Disabled surface | `.quaternary.opacity(0.4)` | Button background when WHOOP disconnected |
| Disabled foreground | `.secondary` | Button label + status icon when disconnected |

---

## 6. Component: WakeAlarmSection

**Insertion point:** After the `if let sleep { CoachInfoGroup(title: "DÍVIDA DE SONO") { ... } }` block, before the closing of the top-level `VStack`. Always visible (not conditional on `sleep` data).

### 6a. Layout (inside CoachInfoGroup wrapping)

```
CoachInfoGroup(title: "ALARME DE DESPERTAR") {
  VStack(spacing: 12) {
    DatePicker row
    [if disconnected] status message row
    Arm / Cancel button
  }
}
```

### 6b. DatePicker Row

```
DatePicker(
  "Hora de acordar",
  selection: $alarmTime,
  displayedComponents: .hourAndMinute
)
.labelsHidden()          // label is the group title — no duplicate
.disabled(isDisconnected || alarmIsArmed)
```

- Full-width, centered in the card.
- Disabled (`.opacity(0.4)`) when WHOOP not connected OR alarm already armed (time must not change mid-arm).
- `$alarmTime` is a view-local `@State` initialised to "current time rounded up to next hour" on appear; it is NOT persisted to UserDefaults (Claude's discretion: omit persistence — simpler, alarm is ephemeral).

### 6c. Status Message (disconnected state only)

Shown when `model.ble.connectionState != "ready"` AND alarm is not armed.  
Pattern copied verbatim from `BreatheView.swift` lines 60-72:

```
HStack(spacing: 8) {
  Image(systemName: "sensor.tag.radiowaves.forward")
    .foregroundStyle(.secondary)
  Text("Conecta o WHOOP para usar o alarme")
    .font(.caption)
    .foregroundStyle(.secondary)
}
.accessibilityElement(children: .combine)
```

No background pill — inline within the `CoachInfoGroup` card (unlike BreatheView which uses its own pill outside a card).

### 6d. Arm / Cancel Button

Single button, full-width within the card, height 44pt (minimum touch target).

| State | Condition | Label | Foreground | Background | Disabled |
|-------|-----------|-------|------------|------------|---------|
| **Idle** | connected, not armed | "Armar Alarme" | `.indigo` | `.indigo.opacity(0.14)` | false |
| **Armed** | connected, armed | "Cancelar Alarme" | `.red` | `.red.opacity(0.14)` | false |
| **Disabled** | not connected | "Armar Alarme" | `.secondary` | `.quaternary.opacity(0.4)` | true |

Shape: `RoundedRectangle(cornerRadius: 10, style: .continuous)` — matches `CoachInfoGroup` card corners.

```swift
Button(alarmIsArmed ? "Cancelar Alarme" : "Armar Alarme") {
  alarmIsArmed ? cancelAlarm() : armAlarm()
}
.font(.body.weight(.semibold))
.foregroundStyle(buttonForeground)
.frame(maxWidth: .infinity, minHeight: 44)
.background(buttonBackground, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
.disabled(isDisconnected)
.accessibilityLabel(alarmIsArmed ? "Cancelar alarme armado" : "Armar alarme de despertar")
```

`isDisconnected` = `model.ble.connectionState != "ready"` (source: CONTEXT.md decisions).

### 6e. Armed confirmation (no UI change required)

On arm: `model.ble.buzz(loops: 2)` provides tactile confirmation — no toast, no banner, no sheet. The button transitioning to "Cancelar Alarme" / red is the sole visual acknowledgement.

---

## 7. State Machine

```
          WHOOP disconnected
          ─────────────────────────────────
          DatePicker: disabled (.opacity 0.4)
          Status message: visible
          Button: "Armar Alarme" / disabled

          WHOOP connected, not armed
          ─────────────────────────────────
          DatePicker: enabled
          Status message: hidden
          Button: "Armar Alarme" / indigo

                    tap "Armar"
                        │
                        ▼
          WHOOP connected, armed
          ─────────────────────────────────
          DatePicker: disabled (.opacity 0.4)
          Status message: hidden
          Button: "Cancelar Alarme" / red

                    tap "Cancelar"
                        │
                        ▼
          Back to: WHOOP connected, not armed

          BLE disconnect while armed
          ─────────────────────────────────
          alarmIsArmed reset to false        ← Claude's discretion: YES, reset on disconnect
          alarmTime preserved (view-local @State)
          Returns to: disconnected state
```

**alarmIsArmed reset on disconnect:** Yes — a strap that disconnects mid-arm is unreliable; reset to idle is safer than showing "armed" for an alarm that may not fire.

---

## 8. Copywriting

| Element | Portuguese copy | Notes |
|---------|----------------|-------|
| Section title | "ALARME DE DESPERTAR" | All-caps, `.black` weight — matches other group titles |
| DatePicker accessibility label | (system-provided) | `.labelsHidden()` hides visible label; VoiceOver reads time value |
| Status message | "Conecta o WHOOP para usar o alarme" | Matches imperative tone of BreatheView ("Connect WHOOP...") but in pt-PT |
| Arm button | "Armar Alarme" | Verb + noun |
| Cancel button | "Cancelar Alarme" | Verb + noun |
| Arm button a11y | "Armar alarme de despertar" | Sentence case for VoiceOver |
| Cancel button a11y | "Cancelar alarme armado" | Clarifies armed state for VoiceOver |

No empty state copy needed — the section is always visible with a DatePicker. No error state copy needed — arm/cancel are fire-and-forget (no expected response from strap in HAP-03).

---

## 9. Accessibility

- Button minimum touch target: 44pt height (`.frame(maxWidth: .infinity, minHeight: 44)`).
- Status message uses `.accessibilityElement(children: .combine)` — icon + text read as one.
- `DatePicker` in disabled state: `.accessibilityHint("Conecta o WHOOP para ativar")` to explain why interaction is unavailable.
- No reduce-motion considerations (DatePicker and button are static — no animation added).

---

## 10. What Is NOT in Scope

- Snooze UI — deferred (no ROADMAP requirement).
- Alarm history / log — deferred.
- Persistence of `alarmTime` across launches — omitted (Claude's discretion).
- Any UI for `GooseWakeWindowManager` — HAP-04 is a non-functional stub with no UI.
- Response/confirmation from strap — requires HAP-04 RE work.

---

## 11. Pre-Population Sources

| Source | Decisions Used |
|--------|---------------|
| CONTEXT.md | Location (bottom of SleepCoachView), DatePicker display components, state properties, BLE command calls, disabled condition string, buzz(loops:2) on arm |
| Codebase scan | `CoachInfoGroup`/`CoachInfoRow` primitives, `.quaternary.opacity(0.4)` card background, `.system(size:11, weight:.black)` group title, BreatheView disabled-state pattern, 16px screen padding, 18px VStack spacing |
| User input (this session) | 0 — all questions answered by upstream artifacts and codebase |
| Defaults applied | alarmIsArmed reset on disconnect (safety), no UserDefaults persistence (ephemeral), no confirmation toast (button state change is sufficient) |
