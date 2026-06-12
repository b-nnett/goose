# Phase 71: Coach VOW + NoopApp Features + Notifications + HR Decimation - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 11 (8 new/modified + 3 supporting files read for context)
**Analogs found:** 11 / 11

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `GooseSwift/CoachView.swift` (modify) | component | request-response | `GooseSwift/CoachView.swift` itself — `CoachJournalCard`, `coachCardSurface` | exact |
| `GooseSwift/IntervalTimerView.swift` (new) | component | event-driven | `GooseSwift/BreatheView.swift` | exact |
| `GooseSwift/MetricExplorerView.swift` (new) | component | request-response | `GooseSwift/CoachView.swift` — `CoachMetricHighlightCard` list pattern | role-match |
| `GooseSwift/MoreRouteModels.swift` (modify) | model | — | `GooseSwift/MoreRouteModels.swift` itself — `.breathe` case (Phase 70) | exact |
| `GooseSwift/MoreDataStore.swift` (modify) | model | — | `GooseSwift/MoreDataStore.swift` itself — `breathe: .ready` init and `refreshRouteStatus` | exact |
| `GooseSwift/MoreView.swift` (modify) | component | request-response | `GooseSwift/MoreView.swift` itself — `Section("Wellness")` + `destination(for:)` | exact |
| `GooseSwift/NotificationScheduler.swift` (new) | service | event-driven | `GooseSwift/OnboardingView.swift` line 478 — `UNUserNotificationCenter.requestAuthorization` | role-match |
| `GooseSwift/GooseAppModel+SleepSync.swift` (modify) | service | CRUD | `GooseSwift/GooseAppModel+SleepSync.swift` itself — async completion at line 174 | exact |
| `GooseSwift/GooseAppModel+PacketPublishing.swift` (modify) | service | event-driven | `GooseSwift/GooseAppModel+PacketPublishing.swift` itself — `.finished(summary, reason:)` case | exact |
| `GooseSwift/GooseBLEClient.swift` (modify) | service | event-driven | `GooseSwift/GooseBLEClient.swift` itself — `batteryLevelPercent` property + `resetLiveDeviceFieldsIfNeeded` | exact |
| `GooseSwift/HeartRateSeriesStores.swift` (modify) | utility | transform | `GooseSwift/HeartRateSeriesStores.swift` itself — `samples(from:to:)` public method | exact |

---

## Pattern Assignments

### `GooseSwift/CoachView.swift` — VOW card insertion (FEAT-01)

**Analog:** `GooseSwift/CoachView.swift` — `CoachJournalCard`, `coachCardSurface`, `CoachOverviewScreen`

**Insertion point** (lines 449–466): VOW card goes between `CoachJournalCard` (line 462) and `CoachRoutesSection` (line 466) inside the `LazyVStack` of `CoachOverviewScreen.body`.

**`@State` dismissal pattern** — add to `CoachOverviewScreen` (alongside `showingJournal` at line 446):
```swift
@State private var vowDismissed = false
```

**Card visibility guard** — insert between `CoachJournalCard` and `CoachRoutesSection`:
```swift
if !vowDismissed, let nudge = CoachVOWNudge.resolve(healthStore: healthStore) {
  CoachVOWCard(nudge: nudge) { vowDismissed = true }
}
```

**`coachCardSurface` modifier** (lines 693–704) — use on the card's root `HStack`:
```swift
private extension View {
  func coachCardSurface(tint: Color, prominent: Bool = false) -> some View {
    background(
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .fill(Color(.secondarySystemGroupedBackground))
        .shadow(color: tint.opacity(prominent ? 0.16 : 0.08), radius: prominent ? 14 : 8, x: 0, y: prominent ? 7 : 3)
    )
    .overlay {
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .stroke(tint.opacity(prominent ? 0.18 : 0.10), lineWidth: 1)
    }
  }
}
```
Call as `.coachCardSurface(tint: nudge.tint)` (no `prominent:` — nudge cards are secondary).

**Data access pattern** — `CoachOverviewScreen` receives `healthStore: HealthDataStore` (line 445). Parse snapshot values as `Double`:
```swift
// HealthMetricSnapshot.value is always String ("68", "--")
guard let score = Double(healthStore.snapshot(for: .recovery).value) else { return nil }
```

**`CoachVOWNudge` enum** — define as a private enum in `CoachView.swift` (same file, private scope matches all other private structs there):
```swift
private enum CoachVOWNudge {
  case criticalRecovery(Double)
  case lowRecovery(Double)
  case highStrain(Double)
  case lowHRV(Double)

  static func resolve(healthStore: HealthDataStore) -> CoachVOWNudge? {
    let recovery = Double(healthStore.snapshot(for: .recovery).value)
    let strain   = Double(healthStore.snapshot(for: .strain).value)
    let hrv      = HRVSeriesStore.shared.dailyEstimate()?.rmssdMS
    if let r = recovery, r < 33 { return .criticalRecovery(r) }
    if let r = recovery, r < 66 { return .lowRecovery(r) }
    if let s = strain,   s > 18 { return .highStrain(s) }
    if let h = hrv,      h < 30 { return .lowHRV(h) }
    return nil
  }

  var title: String { ... }
  var body: String { ... }
  var systemImage: String { ... }
  var tint: Color { ... }
}
```

---

### `GooseSwift/IntervalTimerView.swift` (new, FEAT-02)

**Analog:** `GooseSwift/BreatheView.swift` (lines 1–140) — exact match

**Imports pattern** (lines 1–2):
```swift
import Foundation
import SwiftUI
```

**State pattern** (lines 19–23):
```swift
@State private var isRunning = false
@State private var phaseTask: Task<Void, Never>? = nil
@State private var workDuration: Double = 30
@State private var restDuration: Double = 10
```

**Environment injection** (lines 25–26):
```swift
@Environment(GooseAppModel.self) private var model
```

**Session loop pattern** (lines 102–131) — copy exactly, adapt phase names:
```swift
private func startSession() {
  isRunning = true
  phaseTask = Task { @MainActor in
    repeat {
      // Work phase
      model.ble.buzz(loops: 1)
      try? await Task.sleep(for: .seconds(workDuration))
      guard !Task.isCancelled else { break }

      // Rest phase
      model.ble.buzz(loops: 1)
      try? await Task.sleep(for: .seconds(restDuration))
    } while !Task.isCancelled
  }
}

private func stopSession() {
  phaseTask?.cancel()
  phaseTask = nil
  isRunning = false
}
```

**Lifecycle hook** (line 99) — mandatory:
```swift
.onDisappear { stopSession() }
```

**Toolbar pattern** (lines 93–96):
```swift
.navigationTitle("Interval Timer")
.navigationBarTitleDisplayMode(.inline)
.toolbar(.hidden, for: .tabBar)
.toolbarBackground(FitnessColor.background, for: .navigationBar)
.toolbarColorScheme(.dark, for: .navigationBar)
```

**Background color** (lines 30, 96):
```swift
ZStack {
  FitnessColor.background.ignoresSafeArea()
  // content
}
.background(FitnessColor.background.ignoresSafeArea())
```

**Start/Stop button pattern** (lines 74–90):
```swift
if isRunning {
  Button("Stop") { stopSession() }
    .font(.body.weight(.semibold))
    .foregroundStyle(.white)
    .frame(width: 160, height: 48)
    .background(FitnessColor.panel, in: Capsule())
} else {
  Button("Start") { startSession() }
    .font(.body.weight(.semibold))
    .foregroundStyle(FitnessColor.standCyan)
    .frame(width: 160, height: 48)
    .background(FitnessColor.standCyan.opacity(0.14), in: Capsule())
}
```

---

### `GooseSwift/MetricExplorerView.swift` (new, FEAT-02)

**Analog:** `GooseSwift/MoreView.swift` `MoreAlgorithmsView` pattern — healthStore passed as explicit parameter (line 153: `MoreAlgorithmsView(store: store, healthStore: healthStore)`). No `@EnvironmentObject`.

**Imports pattern**:
```swift
import SwiftUI
```

**Parameter pattern** — pass `healthStore` explicitly, not via environment:
```swift
struct MetricExplorerView: View {
  var healthStore: HealthDataStore
  // ...
}
```

**List pattern** — use `.insetGrouped` style matching MoreView (line 95):
```swift
var body: some View {
  List {
    ForEach(metrics) { metric in
      // row
    }
  }
  .listStyle(.insetGrouped)
  .gooseListBackground()
  .navigationTitle("Metric Explorer")
  .navigationBarTitleDisplayMode(.inline)
  .toolbar(.hidden, for: .tabBar)
}
```

**Snapshot data access** — same pattern as `CoachOverviewSnapshot.make()` in CoachView:
```swift
let snap = healthStore.snapshot(for: route)
// snap.value is String ("68" or "--")
// snap.unit is String ("%", "ms", etc.)
```

---

### `GooseSwift/MoreRouteModels.swift` (modify, FEAT-02)

**Analog:** `GooseSwift/MoreRouteModels.swift` itself — `.breathe` case added in Phase 70

**4-switch pattern** — add in all four switches:

1. **Enum case** (after line 19 `.breathe`):
```swift
case intervalTimer
case metricExplorer
```

2. **`title` switch** (after line 40 `.breathe` case):
```swift
case .intervalTimer: String(localized: "Interval Timer")
case .metricExplorer: String(localized: "Metric Explorer")
```

3. **`subtitle` switch** (after line 61):
```swift
case .intervalTimer: String(localized: "Work and rest intervals with haptic cues")
case .metricExplorer: String(localized: "Browse current metric values from your data")
```

4. **`systemImage` switch** (after line 83 `.breathe: "wind"`):
```swift
case .intervalTimer: "timer"
case .metricExplorer: "list.bullet.rectangle"
```

5. **`statusKeyPath` switch** (after line 103 `.breathe: \.breathe`):
```swift
case .intervalTimer: \.intervalTimer
case .metricExplorer: \.metricExplorer
```

6. **Static route arrays** (after line 113 `wellnessRoutes`):
```swift
static let wellnessRoutes: [MoreRoute] = [.breathe, .intervalTimer]   // add .intervalTimer
static let dataRoutes: [MoreRoute] = [.metricExplorer]                 // new array
```

7. **`MoreRouteStatus` struct** (after line 132 `var breathe: MoreStatusKind`):
```swift
var intervalTimer: MoreStatusKind
var metricExplorer: MoreStatusKind
```

---

### `GooseSwift/MoreDataStore.swift` (modify, FEAT-02)

**Analog:** `GooseSwift/MoreDataStore.swift` itself — `breathe: .ready` at lines 28 and 165

**Two update sites:**

1. **`routeStatus` init** (after line 28 `breathe: .ready`):
```swift
intervalTimer: .ready,
metricExplorer: .ready
```

2. **`refreshRouteStatus` return** (after line 165 `breathe: .ready`):
```swift
intervalTimer: .ready,
metricExplorer: .ready
```

Both sites must be updated — `MoreRouteStatus` is a plain struct with no defaults; missing fields cause a compile error.

---

### `GooseSwift/MoreView.swift` (modify, FEAT-02)

**Analog:** `GooseSwift/MoreView.swift` itself — `Section("Wellness")` at line 78 + `destination(for:)` at line 134

**Section update** (after line 80 `routeRows(MoreRoute.wellnessRoutes)` already includes new routes via static array — no view change needed if `wellnessRoutes` is updated in MoreRouteModels).

**New "Data" section** — insert between `Section("Wellness")` and `Section("Settings")`:
```swift
Section("Data") {
  routeRows(MoreRoute.dataRoutes)
}
```

**`destination(for:)` additions** (after line 169 `case .breathe: BreatheView()`):
```swift
case .intervalTimer:
  IntervalTimerView()
case .metricExplorer:
  MetricExplorerView(healthStore: healthStore)
```

Note: `healthStore` is available as `self.healthStore` (line 13 `private var healthStore: HealthDataStore`).

---

### `GooseSwift/NotificationScheduler.swift` (new, FEAT-03)

**Analog:** `GooseSwift/OnboardingView.swift` line 478 — `UNUserNotificationCenter.current().requestAuthorization`

**Imports pattern**:
```swift
import Foundation
import UserNotifications
```

**Actor pattern** — use `actor` (not `class`) for Swift concurrency safety:
```swift
actor NotificationScheduler {
  static let shared = NotificationScheduler()

  func schedule(title: String, body: String, identifier: String) {
    let center = UNUserNotificationCenter.current()
    center.getNotificationSettings { settings in
      guard settings.authorizationStatus == .authorized else { return }
      let content = UNMutableNotificationContent()
      content.title = title
      content.body = body
      content.sound = .default
      let trigger = UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)
      let request = UNNotificationRequest(identifier: identifier, content: content, trigger: trigger)
      center.add(request)
    }
  }

  func scheduleSleepProcessed(durationMinutes: Int, hrvMS: Double?, recoveryPercent: Double?) {
    let dur = "\(durationMinutes / 60)h\(durationMinutes % 60)m"
    var parts: [String] = [dur]
    if let hrv = hrvMS { parts.append("HRV \(Int(hrv))ms") }
    if let rec = recoveryPercent { parts.append("Recovery \(Int(rec))%") }
    schedule(
      title: "Sleep synced",
      body: parts.joined(separator: " · "),
      identifier: "goose.sleep.processed.\(Int(Date().timeIntervalSince1970))"
    )
  }

  func scheduleWorkoutDetected(activity: String, durationSeconds: Double, averageHR: Int?) {
    let dur = "\(Int(durationSeconds / 60))m"
    var body = "\(activity) · \(dur)"
    if let hr = averageHR { body += " · Avg \(hr) bpm" }
    schedule(
      title: "Workout detected",
      body: body,
      identifier: "goose.workout.detected.\(Int(Date().timeIntervalSince1970))"
    )
  }

  func scheduleBatteryLow(percent: Int) {
    schedule(
      title: "WHOOP battery low",
      body: "Battery at \(percent)% — charge soon",
      identifier: "goose.battery.low"   // static ID: fires once per session via Bool gate
    )
  }
}
```

**Dispatch pattern from callers** — all call sites use:
```swift
Task {
  await NotificationScheduler.shared.scheduleXxx(...)
}
```

---

### `GooseSwift/GooseAppModel+SleepSync.swift` (modify, FEAT-03)

**Analog:** `GooseSwift/GooseAppModel+SleepSync.swift` itself — `syncBandSleepHistory()` completion at line 174

**Insertion point** — immediately after the line that sets `store?.bandSleepImportStatus = "Sincronizado da pulseira"`:
```swift
store?.bandSleepImportStatus = "Sincronizado da pulseira"
// INSERT:
Task {
  await NotificationScheduler.shared.scheduleSleepProcessed(
    durationMinutes: /* sum stageSummary values */,
    hrvMS: store?.liveHRVRMSSD,
    recoveryPercent: Double(store?.snapshot(for: .recovery).value ?? "")
  )
}
```

`syncBandSleepHistory()` is already `async` — a `Task { await ... }` detach is correct (does not block the async context).

---

### `GooseSwift/GooseAppModel+PacketPublishing.swift` (modify, FEAT-03)

**Analog:** `GooseSwift/GooseAppModel+PacketPublishing.swift` itself — `.finished(summary, reason:)` case

**Insertion point** — after `finishActivityRecording(...)` in the `.finished` case (confirmed at line 743):
```swift
case .finished(let summary, let reason):
  finishActivityRecording(...)
  // INSERT:
  Task {
    await NotificationScheduler.shared.scheduleWorkoutDetected(
      activity: summary.activity.title,
      durationSeconds: summary.elapsed,
      averageHR: summary.averageHeartRate.map(Int.init)
    )
  }
```

---

### `GooseSwift/GooseBLEClient.swift` (modify, FEAT-03)

**Analog:** `GooseSwift/GooseBLEClient.swift` itself — `batteryLevelPercent` property

**New property** — add alongside existing battery properties:
```swift
var batteryLowNotificationFired = false
```

**Reset site** — add inside `resetLiveDeviceFieldsIfNeeded(for:)` (confirmed at line 459) alongside existing battery field resets:
```swift
batteryLowNotificationFired = false
```

No lock needed — `GooseBLEClient` dispatches to main thread before writing BLE state; `batteryLowNotificationFired` is set from the same dispatch path as `batteryLevelPercent`.

---

### `GooseSwift/GooseBLEClient+Parsing.swift` (modify, FEAT-03)

**Analog:** `GooseSwift/GooseBLEClient+Parsing.swift` itself — `applyBatteryLevel(_:capturedAt:sourceTitle:)` + `resetLiveDeviceFieldsIfNeeded`

**Insertion point** — after setting `batteryLevelPercent` in `applyBatteryLevel`:
```swift
if normalizedLevel <= 20, !batteryLowNotificationFired {
  batteryLowNotificationFired = true
  Task {
    await NotificationScheduler.shared.scheduleBatteryLow(percent: normalizedLevel)
  }
}
```

`applyBatteryLevel` already dispatches to main thread at its entry — `Task { await ... }` fires the actor from main, which is correct.

---

### `GooseSwift/HeartRateSeriesStores.swift` (modify, DATA-04)

**Analog:** `GooseSwift/HeartRateSeriesStores.swift` itself — `samples(from:to:)` public method (lines 160–166)

**Lock safety rule:** `decimatedSamples` must call the public `samples(from:to:)` (which acquires and releases `stateLock` internally). Do NOT acquire `stateLock` in `decimatedSamples` — `NSLock` is not reentrant; double-locking on the same thread deadlocks.

**`samples(from:to:)` pattern to build on** (lines 160–166):
```swift
func samples(from start: Date, to end: Date) -> [HeartRateSamplePoint] {
  stateLock.lock()
  defer { stateLock.unlock() }
  return samples
    .filter { $0.capturedAt >= start && $0.capturedAt < end }
    .sorted { $0.capturedAt < $1.capturedAt }
}
```

**New method to add** — place immediately after `samples(from:to:)`:
```swift
func decimatedSamples(from start: Date, to end: Date, maxCount: Int = 500) -> [HeartRateSamplePoint] {
  let raw = samples(from: start, to: end)  // acquires + releases stateLock internally
  guard raw.count > maxCount else { return raw }

  let stride = max(1, raw.count / maxCount)
  var result: [HeartRateSamplePoint] = []
  result.reserveCapacity(raw.count / stride * 3)

  var i = 0
  while i < raw.count {
    let windowEnd = min(i + stride, raw.count)
    let window = raw[i..<windowEnd]
    let first = raw[i]
    result.append(first)
    if let maxSample = window.max(by: { $0.bpm < $1.bpm }), maxSample.id != first.id {
      result.append(maxSample)
    }
    if let minSample = window.min(by: { $0.bpm < $1.bpm }),
       minSample.id != first.id,
       minSample.id != result.last?.id {
      result.append(minSample)
    }
    i += stride
  }
  return result.sorted { $0.capturedAt < $1.capturedAt }
}

func decimatedSamples(forDayContaining date: Date = Date(), calendar: Calendar = .current, maxCount: Int = 500) -> [HeartRateSamplePoint] {
  let dayStart = calendar.startOfDay(for: date)
  let dayEnd = calendar.date(byAdding: .day, value: 1, to: dayStart) ?? dayStart.addingTimeInterval(24 * 60 * 60)
  return decimatedSamples(from: dayStart, to: dayEnd, maxCount: maxCount)
}
```

**`HeartRateSamplePoint.id` deduplication** — the `id` is deterministic: `"\(milliseconds).\(bpm).\(source)"` (lines 11–13). The `maxSample.id != first.id` guard is correct for deduplication within the window.

**Caller migration** — four sites in `HealthDataStore+*` files (confirmed by grep; no SwiftUI chart files call these directly):

| File | Current call | Replace with |
|------|-------------|-------------|
| `HealthDataStore+Snapshots.swift:996` | `heartRateSeriesStore.samples(forDayContaining: Date())` | `heartRateSeriesStore.decimatedSamples(forDayContaining: Date())` |
| `HealthDataStore+Snapshots.swift:1129` | `heartRateSeriesStore.samples(forDayContaining: day)` | `heartRateSeriesStore.decimatedSamples(forDayContaining: day)` |
| `HealthDataStore+StressEnergy.swift:20` | `heartRateSeriesStore.samples(forDayContaining: date, calendar: calendar)` | `heartRateSeriesStore.decimatedSamples(forDayContaining: date, calendar: calendar)` |
| `HealthDataStore+Cardio.swift:172` | `heartRateSeriesStore.samples(from: start, to: end)` | `heartRateSeriesStore.decimatedSamples(from: start, to: end)` |

---

## Shared Patterns

### FitnessColor palette
**Source:** `GooseSwift/BreatheView.swift` lines 29–88
**Apply to:** `IntervalTimerView.swift`
```swift
FitnessColor.background   // screen background
FitnessColor.panel        // card/button backgrounds
FitnessColor.standCyan    // primary interactive color
FitnessColor.secondaryText // muted text
```

### `@Environment(GooseAppModel.self)` injection
**Source:** `GooseSwift/BreatheView.swift` line 25
**Apply to:** `IntervalTimerView.swift` (needs `model.ble.buzz(loops:)`)
```swift
@Environment(GooseAppModel.self) private var model
```

### `healthStore` as explicit parameter (not `@EnvironmentObject`)
**Source:** `GooseSwift/MoreView.swift` line 153 — `MoreAlgorithmsView(store: store, healthStore: healthStore)`
**Apply to:** `MetricExplorerView.swift` — always pass `healthStore:` from `MoreView.destination(for:)`. Never use `@EnvironmentObject var healthStore`.

### `MoreStatusKind.ready` for always-available routes
**Source:** `GooseSwift/MoreDataStore.swift` lines 22, 28 — `algorithms: .ready`, `breathe: .ready`
**Apply to:** Both `intervalTimer` and `metricExplorer` in `MoreRouteStatus` init and `refreshRouteStatus`.

### Notification dispatch via `Task { await actor.method() }`
**Source:** Swift concurrency convention; `UNUserNotificationCenter` confirmed thread-safe
**Apply to:** All three notification scheduling call sites (sleep, workout, battery).
Never call `UNUserNotificationCenter` directly inline on `@MainActor` — dispatch through the `NotificationScheduler` actor.

---

## No Analog Found

All files have close analogs. No gaps.

---

## Metadata

**Analog search scope:** `GooseSwift/` (all Swift source files)
**Key files read:** `BreatheView.swift`, `MoreRouteModels.swift`, `MoreView.swift`, `MoreDataStore.swift`, `CoachView.swift` (lines 1–80, 437–510, 685–705), `HeartRateSeriesStores.swift` (lines 1–203), `OnboardingView.swift` (grep only)
**Pattern extraction date:** 2026-06-12
