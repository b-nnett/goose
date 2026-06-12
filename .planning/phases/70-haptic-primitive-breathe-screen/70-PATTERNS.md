# Phase 70: Haptic Primitive + Breathe Screen - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 6 (2 new, 4 modified)
**Analogs found:** 6 / 6

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `GooseSwift/GooseBLEClient+Haptics.swift` | BLE command extension | request-response (fire-and-forget) | `GooseSwift/GooseBLEClient+HistoricalCommands.swift` | exact |
| `GooseSwift/BreatheView.swift` | view (destination) | event-driven (Task loop) | `GooseSwift/MoreRemoteServerViews.swift` | role-match |
| `GooseSwift/MoreRouteModels.swift` (edit) | model / enum | — | self (full file read) | exact |
| `GooseSwift/MoreView.swift` (edit) | view (list + router) | — | self (full file read) | exact |
| `GooseSwift/MoreDataStore.swift` (edit) | store / coordinator | — | self (full file read) | exact |
| `GooseSwift/AppShellView.swift` (no edit needed) | shell / router | — | `GooseSwift/MoreView.swift` lines 95–97 | exact |

---

## Pattern Assignments

### `GooseSwift/GooseBLEClient+Haptics.swift` (BLE command extension, fire-and-forget)

**Analog:** `GooseSwift/GooseBLEClient+HistoricalCommands.swift`

**Imports pattern** (lines 1–3 of analog):
```swift
import CoreBluetooth
import Foundation
import OSLog
```

**Extension declaration pattern** (line 6 of analog):
```swift
extension GooseBLEClient {
  // methods go here — no class declaration, no @MainActor
}
```

**Guard + writeType + writeValue core pattern** (lines 93–126 of analog):
```swift
// Three-step pattern for all BLE command writes:
// Step 1: guard activePeripheral + commandCharacteristic (shorthand binding)
guard let activePeripheral, let commandCharacteristic else {
  // For buzz: log and return silently (no failHistoricalSync equivalent needed)
  record(source: "ble.haptic", title: "buzz.blocked", body: "no active peripheral or characteristic")
  return
}
// Step 2: guard writeType — determines .withResponse vs .withoutResponse from characteristic properties
guard let writeType = writeType(for: commandCharacteristic) else {
  record(source: "ble.haptic", title: "buzz.blocked", body: "characteristic not writable")
  return
}
// Step 3: write raw bytes directly (buzz skips sequence, buildCommandFrame, pending state, timeout)
activePeripheral.writeValue(payload, for: commandCharacteristic, type: writeType)
```

**writeType(for:) helper** (lines 177–185 of analog — already exists on GooseBLEClient, do NOT redefine):
```swift
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

**writeTypeName helper** — confirmed at `GooseSwift/GooseBLEClient+Parsing.swift` line 788. Already exists; call as `writeTypeName(writeType)` in the record body.

**record pattern** (lines 15–16 and 144–145 of analog):
```swift
record(
  source: "ble.haptic",
  title: "buzz.sent",
  body: "loops=\(clamped) \(writeTypeName(writeType))"
)
```

**Complete buzz method — derived pattern** (no sequence, no frame wrapper, no pending state):
```swift
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
  record(source: "ble.haptic", title: "buzz.sent", body: "loops=\(clamped) \(writeTypeName(writeType))")
}
```

---

### `GooseSwift/BreatheView.swift` (view, event-driven Task loop)

**Analog:** `GooseSwift/MoreRemoteServerViews.swift`

**Imports pattern** (lines 1–3 of analog):
```swift
import Foundation
import SwiftUI
import UIKit
```

**@Environment model access pattern** (line 34 of analog — CRITICAL: @Observable, not ObservableObject):
```swift
struct BreatheView: View {
  @Environment(GooseAppModel.self) private var model
  // NOT @EnvironmentObject — GooseAppModel is @Observable
}
```

**Navigation title + toolbar pattern** (from MoreView.swift lines 93–94):
```swift
.navigationTitle("Breathe")
.navigationBarTitleDisplayMode(.inline)
.toolbarBackground(.hidden, for: .navigationBar)
```

**Task + sleep loop pattern** (Task.sleep modern syntax confirmed at MoreView.swift line 103):
```swift
@State private var phaseTask: Task<Void, Never>? = nil

func startSession() {
  isRunning = true
  phaseTask = Task { @MainActor in   // @MainActor ensures withAnimation runs on main thread
    repeat {
      // ... phase transitions with withAnimation + Task.sleep(for: .seconds(N))
    } while !Task.isCancelled
  }
}

func stopSession() {
  phaseTask?.cancel()
  phaseTask = nil
  isRunning = false
}
```

**onDisappear cancel pattern** (required — Task must be cancelled on pop):
```swift
.onDisappear { stopSession() }
```

**Disconnected state banner** — use `model.ble.connectionState != "ready"` (lowercase "ready" confirmed
across codebase; NOT "Connected"):
```swift
if model.ble.connectionState != "ready" {
  Text("Connect WHOOP to enable haptics")
    .font(.caption)
    .foregroundStyle(FitnessColor.secondaryText)
}
```

**FitnessColor palette** (from `GooseSwift/FitnessFormatting.swift` lines 6–21):
```swift
FitnessColor.background      // Color.black
FitnessColor.panel           // Color(red: 0.10, green: 0.10, blue: 0.11)
FitnessColor.secondaryText   // Color(red: 0.58, green: 0.58, blue: 0.62)
FitnessColor.workoutYellow   // Color(red: 1.0, green: 0.91, blue: 0.24)
FitnessColor.standCyan       // Color(red: 0.39, green: 0.92, blue: 0.95)
FitnessColor.endRed          // Color(red: 1.0, green: 0.25, blue: 0.27)
```

**Reduce motion accessibility pattern** (from RESEARCH.md §Code Examples):
```swift
@Environment(\.accessibilityReduceMotion) var reduceMotion

// In phase transitions:
if reduceMotion {
  circleScale = targetScale   // direct assignment, no animation
} else {
  withAnimation(.easeInOut(duration: phaseDuration)) { circleScale = targetScale }
}
```

---

### `GooseSwift/MoreRouteModels.swift` (edit — add case breathe)

**Analog:** self (full file read at lines 1–165)

**Enum declaration** (lines 3–19):
```swift
enum MoreRoute: String, CaseIterable, Identifiable, Hashable {
  case profile
  // ... existing cases ...
  case developer
  // ADD: case breathe  — insert before .privacy or in a logical wellness position
}
```

**title switch arm pattern** (lines 23–39) — add:
```swift
case .breathe: String(localized: "Breathe")
```

**subtitle switch arm pattern** (lines 43–59) — add:
```swift
case .breathe: String(localized: "Paced breathing with haptics")
```

**systemImage switch arm pattern** (lines 63–79) — add:
```swift
case .breathe: "wind"
```

**statusKeyPath switch arm pattern** (lines 83–100) — add:
```swift
case .breathe: \.breathe
```

**Route group static let pattern** (lines 102–107) — add new group:
```swift
static let wellnessRoutes: [MoreRoute] = [.breathe]
```

**MoreRouteStatus struct** (lines 110–126) — add property:
```swift
struct MoreRouteStatus: Equatable {
  // ... existing properties ...
  var breathe: MoreStatusKind   // ADD this line
}
```

---

### `GooseSwift/MoreView.swift` (edit — add Wellness section + destination arm)

**Analog:** self (full file read at lines 1–173)

**Section + routeRows pattern** (lines 45–89) — add new Section before "Settings":
```swift
Section("Wellness") {
  routeRows(MoreRoute.wellnessRoutes)
}
```

**destination(for:) switch pattern** (lines 130–165) — add arm:
```swift
case .breathe:
  BreatheView()
```

**routeRows helper** (lines 120–127 — already exists, no changes needed):
```swift
@ViewBuilder
private func routeRows(_ routes: [MoreRoute]) -> some View {
  ForEach(routes) { route in
    NavigationLink(value: route) {
      MoreRouteRow(route: route, status: store.routeStatus[keyPath: route.statusKeyPath])
    }
    .accessibilityLabel(route.title)
  }
}
```

**navigationDestination wiring** (lines 95–97 — already exists in MoreView.swift, destination(for:) switch handles routing):
```swift
.navigationDestination(for: MoreRoute.self) { route in
  destination(for: route)
}
```

---

### `GooseSwift/MoreDataStore.swift` (edit — add breathe to MoreRouteStatus initializer)

**Analog:** self (full file read at lines 1–579)

**routeStatus @Published initializer** (lines 12–28) — add `breathe: .ready`:
```swift
@Published var routeStatus = MoreRouteStatus(
  profile: .pending,
  device: .pending,
  hrMonitor: .pending,
  connectionLab: .pending,
  capture: .pending,
  localStore: .pending,
  healthSync: .pending,
  rawExport: .pending,
  algorithms: .ready,
  debug: .pending,
  privacy: .pending,
  remoteServer: .pending,
  support: .pending,
  about: .ready,
  developer: .pending,
  breathe: .ready    // ADD — always ready; screen is always accessible
)
```

**refreshRouteStatus construction** (lines 147–165) — add `breathe: .ready` to the MoreRouteStatus(...) literal:
```swift
func refreshRouteStatus(ble: GooseBLEClient, model: GooseAppModel) {
  let newStatus = MoreRouteStatus(
    // ... existing fields ...
    developer: .pending,
    breathe: .ready    // ADD — Breathe screen is always accessible regardless of connection
  )
  routeStatus = newStatus
}
```

**Critical:** Both construction sites (lines 12–28 and 147–165) must be updated. Missing either causes a compiler error because `MoreRouteStatus` is a struct with a memberwise initializer.

---

## Shared Patterns

### BLE Guard Pattern
**Source:** `GooseSwift/GooseBLEClient+HistoricalCommands.swift` lines 93–99
**Apply to:** `GooseBLEClient+Haptics.swift`
```swift
guard let activePeripheral, let commandCharacteristic else {
  record(source: "...", title: "...", body: "...")
  return
}
guard let writeType = writeType(for: commandCharacteristic) else {
  record(source: "...", title: "...", body: "...")
  return
}
```

### @Environment Model Access
**Source:** `GooseSwift/MoreView.swift` line 11; `GooseSwift/MoreRemoteServerViews.swift` line 34
**Apply to:** `BreatheView.swift`
```swift
@Environment(GooseAppModel.self) private var model
// GooseAppModel is @Observable — never use @EnvironmentObject
```

### connectionState String Value
**Source:** `GooseSwift/GooseBLEClient+HistoricalCommands.swift` line 22 (confirmed lowercase)
**Apply to:** `BreatheView.swift` disconnected banner guard
```swift
// CORRECT:
model.ble.connectionState != "ready"
// WRONG (never use):
model.ble.connectionState != "Connected"
```

### Task + @MainActor Session Loop
**Source:** `GooseSwift/MoreView.swift` line 103 (Task.sleep modern syntax)
**Apply to:** `BreatheView.swift` startSession()
```swift
phaseTask = Task { @MainActor in
  // withAnimation calls are safe here — @MainActor guarantees main thread
  try? await Task.sleep(for: .seconds(4))
}
```

### OSLog record Pattern
**Source:** `GooseSwift/GooseBLEClient+HistoricalCommands.swift` lines 15–16, 144–145
**Apply to:** `GooseBLEClient+Haptics.swift`
```swift
record(source: "ble.haptic", title: "buzz.sent", body: "loops=\(clamped) \(writeTypeName(writeType))")
// source naming pattern: "ble.haptic" consistent with "ble.sync", "ble.clock", "ble.alarm"
```

---

## No Analog Found

All files have close analogs in the codebase. No entries.

---

## Key Assumptions Resolved

| Assumption | Resolution |
|-----------|-----------|
| `writeTypeName(_:)` exists | CONFIRMED at `GooseSwift/GooseBLEClient+Parsing.swift` line 788 — call it directly |
| `MoreDataStore.refreshRouteStatus` uses named args (not positional) | CONFIRMED — lines 147–165 use named labels; add `breathe: .ready` safely |
| `connectionState` value is lowercase "ready" | CONFIRMED at `GooseBLEClient+HistoricalCommands.swift` line 22 |
| `GooseAppModel` is `@Observable` not `ObservableObject` | CONFIRMED — `MoreView.swift` line 11 uses `@Environment(GooseAppModel.self)` |

## Metadata

**Analog search scope:** `GooseSwift/` directory
**Files scanned:** 6 analog files read in full
**Pattern extraction date:** 2026-06-12
