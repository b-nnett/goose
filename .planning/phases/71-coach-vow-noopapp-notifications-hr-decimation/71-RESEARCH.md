# Phase 71: Coach VOW + NoopApp Features + Notifications + HR Decimation - Research

**Researched:** 2026-06-12
**Domain:** SwiftUI feature additions — Coach nudges, More-tab views, UNUserNotificationCenter, HR sample decimation
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Coach VOW**
- Placement: horizontal card/banner at the top of CoachView's main VStack, above CoachRoutesSection
- Maximum nudges shown: 1 (the most urgent, by priority: recovery > strain > HRV)
- Thresholds: recovery < 33% → "Critical Recovery"; recovery < 66% → "Low Recovery"; strain > 18 → "High Strain"; HRV < 30ms → "Low HRV" (weekly average from healthStore)
- Data source: `healthStore` existing snapshot calls — no new bridge methods or SQLite reads

**Interval Timer (FEAT-02)**
- Entry point: `MoreRoute.intervalTimer` — new case in MoreRouteModels.swift, "Wellness" section alongside .breathe
- Functionality: user configures work duration (seconds) + rest duration (seconds); timer counts down; `model.ble.buzz(loops: 1)` fires at each interval transition (work→rest, rest→work)
- Session control: Start/Stop button; session free-running with configurable interval count or infinite

**Metric Explorer (FEAT-02)**
- Entry point: `MoreRoute.metricExplorer` — new case in MoreRouteModels.swift, new "Data" section
- Content: scrollable list of metric names + current values from `healthStore.snapshot(for:)` calls — readiness, recovery, strain, HRV, RHR, sleep, stress
- No graphs or historical views in this phase — list only

**Notifications (FEAT-03)**
- Payload format: title + body with metric values
- Sleep notification: scheduled inside `syncBandSleepHistory()` completion handler
- Workout notification: scheduled inside `PassiveActivityDetector.finished(summary, reason:)` handler
- Battery notification: scheduled in the BLE battery level callback when `batteryLevel <= 20` for the first time per connection session (track with a Bool flag reset on each BLE connect)
- All use `UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)`

**HR Decimation (DATA-04)**
- Algorithm: stride-N — keep 1 sample per N, plus the local max and min within each stride window
- Location: computed property on `HeartRateSeriesStore` — `var decimatedSamples(from:to:)` or variant returning decimated array when `samples.count > 1000`, raw array otherwise
- Target maximum: 500 samples (so stride = samples.count / 500)
- Chart views that currently read `store.samples` should switch to `store.decimatedSamples`

### Claude's Discretion
- Exact VOW card visual design (color, icon, dismissal)
- Interval Timer default work/rest durations (suggest 30s/10s)
- Whether battery notification uses `.badge` in addition to `.alert` + `.sound`
- Which chart views to update to use decimatedSamples

### Deferred Ideas (OUT OF SCOPE)
- VOW nudge history / tap to see trend
- Interval Timer: multiple preset programs, custom haptic patterns
- Metric Explorer: sparkline charts, metric comparison
- LTTB decimation algorithm
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| FEAT-01 | Coach tab shows contextual VOW nudges computed locally from existing healthStore data | VOW card inserted at top of `CoachOverviewScreen`'s LazyVStack, above CoachRoutesSection; data from `healthStore.snapshot(for:)` which already computes recovery/strain scores as numeric strings |
| FEAT-02 | Breathe UI (done), Interval Timer, Metric Explorer reachable from More tab | Interval Timer follows BreatheView Task pattern exactly; Metric Explorer is a List + ForEach over HealthRoute cases; both need new MoreRoute cases + 4-switch wiring |
| FEAT-03 | Local notifications after sleep sync, workout detection, battery low | UNUserNotificationCenter already authorised (.alert, .badge, .sound); three distinct scheduling sites identified in codebase |
| DATA-04 | HR chart loads without lag in long sessions (stride-N decimation) | All `samples()` calls go through `HealthDataStore+*` files — no SwiftUI chart views call the store directly; decimation is a pure read-path addition to `HeartRateSeriesStore` |
</phase_requirements>

---

## Summary

Phase 71 adds four independent feature clusters to the Goose iOS app. All work is pure Swift/SwiftUI with no new external dependencies. The codebase patterns from Phase 70 (BreatheView, buzz(loops:), MoreRoute wiring) are the canonical templates for FEAT-02. FEAT-01 and FEAT-03 require surgical insertions into existing coordinator methods. DATA-04 is confined entirely to `HeartRateSeriesStores.swift`.

The key architectural insight is that **no chart/view file directly accesses `HeartRateSeriesStore.samples`** — all sample reads are funnelled through `HealthDataStore+*` extension methods. This means decimation can be introduced as a new method on `HeartRateSeriesStore` and callers can be migrated one by one in the HealthDataStore extension files.

**Primary recommendation:** Implement in dependency order: DATA-04 first (self-contained), then FEAT-02 (template-based), then FEAT-01 (needs healthStore snapshot values), then FEAT-03 (touches most files, highest risk of merge conflict).

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| VOW nudge computation | HealthDataStore (@MainActor) | CoachView (display only) | Snapshot data lives in HealthDataStore; CoachView already reads snapshots via `CoachOverviewSnapshot.make()` |
| Interval Timer session loop | View-local (BreatheView pattern) | GooseBLEClient (buzz call) | Session state is ephemeral UI state; buzz is a side-effect on BLE |
| Metric Explorer data | HealthDataStore | — | `snapshot(for:)` already on HealthDataStore |
| Notification scheduling | GooseAppModel extensions (sleep, workout) + GooseBLEClient+Parsing (battery) | — | Each notification fires at a specific domain completion point |
| HR decimation | HeartRateSeriesStore | HealthDataStore (caller migration) | Raw sample array lives in HeartRateSeriesStore; HealthDataStore is the only consumer |

---

## Standard Stack

### Core (all already in project — no installs)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| SwiftUI | iOS 26.0 SDK | UI for all new views | Project standard |
| UserNotifications | iOS 26.0 SDK | Local notification scheduling | Already authorised in onboarding |
| Foundation | iOS 26.0 SDK | Task, DispatchQueue, UserDefaults | Universal |

### No new packages required
All capabilities use existing project dependencies. No `npm install`, `pip install`, or `cargo add` needed.

---

## Package Legitimacy Audit

Not applicable — phase introduces zero external packages.

---

## Architecture Patterns

### System Architecture Diagram

```
FEAT-01 (VOW):
healthStore.snapshot(for: .recovery/.strain)
  └─→ CoachVOWNudge struct (pure value — computes urgency)
        └─→ CoachVOWCard view (inserted at top of LazyVStack in CoachOverviewScreen)

FEAT-02 (NoopApp):
MoreRoute enum ──┬── .intervalTimer ──→ IntervalTimerView
                 │                         └─→ Task { @MainActor repeat { ... } while !Task.isCancelled }
                 │                               └─→ model.ble.buzz(loops: 1) at each transition
                 └── .metricExplorer ──→ MetricExplorerView
                                           └─→ healthStore.snapshot(for: route) per HealthRoute

FEAT-03 (Notifications):
GooseAppModel+SleepSync.syncBandSleepHistory()
  └─→ (after "Sincronizado da pulseira") → NotificationScheduler.scheduleSleepProcessed(duration:hrv:recovery:)

GooseAppModel+PacketPublishing.applyActivityDetectionEvents()
  case .finished(summary, reason):
  └─→ NotificationScheduler.scheduleWorkoutDetected(activity:duration:strain:)

GooseBLEClient+Parsing.applyBatteryLevel()
  └─→ (if percent <= 20 && !batteryLowNotificationFired) → NotificationScheduler.scheduleBatteryLow(percent:)

DATA-04 (HR Decimation):
HeartRateSeriesStore.samples(from:to:) [existing]
  └─→ (new) HeartRateSeriesStore.decimatedSamples(from:to:maxCount:) [stride-N]
        └─→ HealthDataStore+Cardio.swift caller migrated
        └─→ HealthDataStore+Snapshots.swift callers migrated
        └─→ HealthDataStore+StressEnergy.swift caller migrated
```

### Recommended Project Structure

New files to create:
```
GooseSwift/
├── GooseSwift/IntervalTimerView.swift    # FEAT-02: new More-tab destination
├── GooseSwift/MetricExplorerView.swift   # FEAT-02: new More-tab destination
└── GooseSwift/NotificationScheduler.swift # FEAT-03: actor encapsulating all UNUserNotificationCenter calls
```

Existing files to modify:
```
GooseSwift/
├── CoachView.swift                       # FEAT-01: insert CoachVOWCard in CoachOverviewScreen
├── MoreRouteModels.swift                 # FEAT-02: add .intervalTimer, .metricExplorer cases
├── MoreDataStore.swift                   # FEAT-02: add new routes to routeStatus init + refreshRouteStatus
├── MoreView.swift                        # FEAT-02: add rows to Wellness section + new Data section; add destination cases
├── AppShellView.swift                    # FEAT-02: navigationDestination cases (if MoreView uses programmatic nav)
├── GooseAppModel+SleepSync.swift         # FEAT-03: schedule sleep notification after sync success
├── GooseAppModel+PacketPublishing.swift  # FEAT-03: schedule workout notification in .finished case
├── GooseBLEClient.swift                  # FEAT-03: add batteryLowNotificationFired Bool property
├── GooseBLEClient+Parsing.swift          # FEAT-03: call NotificationScheduler in applyBatteryLevel
└── HeartRateSeriesStores.swift           # DATA-04: add decimatedSamples(from:to:maxCount:) method
```

---

## FEAT-01: Coach VOW — Detailed Findings

### Insertion Point

`CoachOverviewScreen.body` is a `ScrollView > LazyVStack`. The current top-to-bottom order is:

```
CoachRecommendationCard
CoachOverviewChatCard
CoachJournalCard
CoachRoutesSection       ← VOW card goes ABOVE this
CoachOverviewSectionTitle("Metric Highlights")
LazyVGrid (highlights)
CoachOverviewSectionTitle("Data Gaps") [conditional]
ForEach gaps
```

Insert `CoachVOWCard` between `CoachJournalCard` and `CoachRoutesSection`. `CoachOverviewScreen` receives `healthStore: HealthDataStore` — the VOW computation has direct access.

### Data Access Pattern

`HealthMetricSnapshot.value` is a plain `String` (e.g. `"68"` for recovery 68%). To extract a numeric threshold value, parse with `Double(snapshot.value)`. This is safe — `value` is always `"--"` or a numeric string (no unit suffix; unit is in `snapshot.unit`).

Recovery score: `healthStore.snapshot(for: .recovery).value` → parse as `Double` → 0–100 range (percent).

Strain score: `healthStore.snapshot(for: .strain).value` → parse as `Double` → 0–21 WHOOP scale.

HRV: `ble.liveHRVRMSSD` on `GooseBLEClient` (Double? in ms) is the most direct live value. Alternatively use `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS`. The CONTEXT.md says "HRV weekly average from healthStore" — the closest proxy is `ble.liveHRVRMSSD` which is available on `healthStore`'s environment (CoachOverviewScreen has `healthStore`). For weekly average use `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS` which computes from stored HRV samples.

### VOW Priority Logic

```swift
// [ASSUMED] — implementation pattern, not from external source
enum CoachVOWNudge {
  case criticalRecovery(Double)  // recovery < 33
  case lowRecovery(Double)       // recovery < 66
  case highStrain(Double)        // strain > 18
  case lowHRV(Double)            // hrv < 30ms

  static func resolve(healthStore: HealthDataStore) -> CoachVOWNudge? {
    let recovery = Double(healthStore.snapshot(for: .recovery).value) ?? nil
    let strain   = Double(healthStore.snapshot(for: .strain).value) ?? nil
    let hrv      = HRVSeriesStore.shared.dailyEstimate()?.rmssdMS

    if let r = recovery, r < 33 { return .criticalRecovery(r) }
    if let r = recovery, r < 66 { return .lowRecovery(r) }
    if let s = strain,   s > 18 { return .highStrain(s) }
    if let h = hrv,      h < 30 { return .lowHRV(h) }
    return nil
  }
}
```

### Design Notes (Claude's Discretion)

- Use `coachCardSurface(tint:)` modifier (already defined in CoachView.swift as a `View` extension) for visual consistency.
- Tint: recovery nudges → `.red`; strain → `.orange`; HRV → `.blue`.
- Dismissal: `@State private var vowDismissed = false` on `CoachOverviewScreen`; hide card when dismissed. No persistence needed (resets on re-open, which is fine per CONTEXT.md scope).
- Icon: `"exclamationmark.triangle.fill"` for critical; `"info.circle"` for informational nudges.

---

## FEAT-02: Interval Timer + Metric Explorer — Detailed Findings

### MoreRoute Wiring — 4-Switch Pattern

Phase 70 established the 4-switch pattern for adding a new `MoreRoute`:

1. **`MoreRouteModels.swift`**: Add case to `enum MoreRoute`, `title`, `subtitle`, `systemImage`, `statusKeyPath` switches; add `var intervalTimer: MoreStatusKind` and `var metricExplorer: MoreStatusKind` fields to `MoreRouteStatus`; add routes to the correct static array (`wellnessRoutes` for `.intervalTimer`, new `dataRoutes` for `.metricExplorer`).

2. **`MoreDataStore.swift`**: Add `intervalTimer: .ready, metricExplorer: .ready` to `MoreRouteStatus` init; add same defaults in `refreshRouteStatus`.

3. **`MoreView.swift`**: Add `Section("Data") { routeRows(MoreRoute.dataRoutes) }` between Wellness and Settings; add `case .intervalTimer: IntervalTimerView()` and `case .metricExplorer: MetricExplorerView(healthStore: healthStore)` to `destination(for:)`.

4. **`AppShellView.swift`**: MoreView already handles its own `navigationDestination(for: MoreRoute.self)` inline — no change needed there. The navigation is handled entirely within `MoreView`'s `destination(for:)` switch.

### Interval Timer — BreatheView Pattern

The exact pattern from `BreatheView.swift` (confirmed by reading the file):

```swift
// Source: GooseSwift/BreatheView.swift
@State private var phaseTask: Task<Void, Never>? = nil

private func startSession() {
  isRunning = true
  phaseTask = Task { @MainActor in
    repeat {
      // work phase
      model.ble.buzz(loops: 1)
      try? await Task.sleep(for: .seconds(workDuration))
      guard !Task.isCancelled else { break }

      // rest phase
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

`.onDisappear { stopSession() }` is mandatory — BreatheView does this (line 99).

Default durations (Claude's discretion): work = 30s, rest = 10s. Use `@State private var workDuration: Double = 30` and `@State private var restDuration: Double = 10` with `Stepper` or `Slider` for configuration.

**Key pitfall from BreatheView:** Use `try? await Task.sleep(for: .seconds(...))` (not `DispatchQueue.asyncAfter`). The `try?` discards the `CancellationError` cleanly; the `guard !Task.isCancelled else { break }` after each sleep is still needed because sleep may return without throwing on some runtimes.

### Metric Explorer

Simple list view — no Task pattern needed:

```swift
// [ASSUMED] — implementation sketch
struct MetricExplorerView: View {
  var healthStore: HealthDataStore

  private let routes: [HealthRoute] = [.recovery, .strain, .sleep, .stress, .cardioLoad, .energyBank]

  var body: some View {
    List {
      ForEach(routes) { route in
        let snap = healthStore.snapshot(for: route)
        HStack {
          Label(snap.title, systemImage: snap.systemImage)
          Spacer()
          Text(snap.displayValue.isEmpty ? "--" : snap.displayValue)
            .foregroundStyle(.secondary)
        }
      }
    }
    .navigationTitle("Metric Explorer")
    .navigationBarTitleDisplayMode(.inline)
  }
}
```

`healthStore` must be passed as a parameter (same pattern as `MoreAlgorithmsView`). Do not inject via environment — MoreView passes it explicitly.

---

## FEAT-03: Notifications — Detailed Findings

### Permission Status (CONFIRMED)

`OnboardingView.swift` line 478:
```swift
let granted = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
```
All three notification types are requested at onboarding. No runtime permission check needed before scheduling — CONTEXT.md says to use `getNotificationSettings` as a guard (check `authorizationStatus == .authorized` before scheduling).

### NotificationScheduler Actor

Encapsulate all UNUserNotificationCenter calls in one actor to prevent data races and keep scheduling logic testable:

```swift
// [ASSUMED] — implementation pattern
actor NotificationScheduler {
  static let shared = NotificationScheduler()

  func scheduleSleepProcessed(durationMinutes: Int, hrvMS: Double?, recoveryPercent: Double?) {
    let content = UNMutableNotificationContent()
    content.title = "Sleep synced"
    // format body from parameters
    let trigger = UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)
    let request = UNNotificationRequest(identifier: "goose.sleep.processed.\(Int(Date().timeIntervalSince1970))", content: content, trigger: trigger)
    UNUserNotificationCenter.current().add(request)
  }
  // similar methods for workout + battery
}
```

Using `actor` is the correct Swift 6 pattern for mutable state protection on a reference type. `UNUserNotificationCenter.current()` is itself thread-safe.

### Sleep Notification — Scheduling Site

`GooseAppModel+SleepSync.swift`, `syncBandSleepHistory()` — after line 174:
```swift
store?.bandSleepImportStatus = "Sincronizado da pulseira"
// ← INSERT: Task { await NotificationScheduler.shared.scheduleSleepProcessed(...) }
```

The staging result provides `stageSummary` (dict of stage minutes). Sleep duration = sum of all stage values in minutes. HRV and recovery come from `HealthDataStore` (passed as `store` in this method). Since `store` is `HealthDataStore?`, use `store?.snapshot(for: .recovery).value` and `store?.liveHRVRMSSD`.

**Important:** `syncBandSleepHistory()` is already an `async` function — a `Task { await ... }` detach is fine. Do not call `UNUserNotificationCenter` directly from this async function on an arbitrary executor; dispatch via the actor.

### Workout Notification — Scheduling Site

`GooseAppModel+PacketPublishing.swift`, `applyActivityDetectionEvents()`, case `.finished(let summary, let reason:)` — after `finishActivityRecording(...)` call:

```swift
case .finished(let summary, let reason):
  // existing code...
  finishActivityRecording(...)
  // ← INSERT:
  Task {
    await NotificationScheduler.shared.scheduleWorkoutDetected(
      activity: summary.activity.title,
      durationSeconds: summary.elapsed,
      strain: nil // strain not computed yet at detection time
    )
  }
```

`PassiveDetectedActivitySummary` has `activity.title`, `elapsed`, `averageHeartRate`, `maxHeartRate`. Strain value is not available at detection time (it requires TRIMP computation). Schedule with duration + activity type + HR only.

### Battery Notification — Scheduling Site

`GooseBLEClient.swift` needs a new Bool property:
```swift
var batteryLowNotificationFired = false
```

Reset in `resetLiveDeviceFieldsIfNeeded(for:)` (line 459) where all battery fields are already reset.

`GooseBLEClient+Parsing.swift`, `applyBatteryLevel(_:capturedAt:sourceTitle:)` — after setting `batteryLevelPercent`:
```swift
if normalizedLevel <= 20, !batteryLowNotificationFired {
  batteryLowNotificationFired = true
  Task {
    await NotificationScheduler.shared.scheduleBatteryLow(percent: normalizedLevel)
  }
}
```

`applyBatteryLevel` already dispatches to main thread at the top. The `Task { ... }` fires the actor from main.

### Notification Identifiers (avoid duplicates)

Use time-stamped identifiers to avoid replacing previous notifications with the same ID:
- `"goose.sleep.processed.\(Int(Date().timeIntervalSince1970))"`
- `"goose.workout.detected.\(Int(Date().timeIntervalSince1970))"`
- `"goose.battery.low"` — static ID (only fires once per session due to Bool gate)

---

## DATA-04: HR Decimation — Detailed Findings

### HeartRateSeriesStore — Confirmed Structure

```swift
// Source: GooseSwift/HeartRateSeriesStores.swift
final class HeartRateSeriesStore: @unchecked Sendable {
  private var samples: [HeartRateSamplePoint]  // private — protected by stateLock (NSLock)

  func samples(forDayContaining date: Date = Date(), calendar: Calendar = .current) -> [HeartRateSamplePoint]
  func samples(from start: Date, to end: Date) -> [HeartRateSamplePoint]
  // ↑ existing public API — these are the callers to migrate
}
```

**`HeartRateSamplePoint` struct:**
```swift
struct HeartRateSamplePoint: Codable, Identifiable, Equatable {
  let id: String
  let capturedAt: Date
  let bpm: Int
  let source: String
}
```

The array is sorted by `capturedAt` in all existing return paths.

### Decimation Algorithm

Add a new method to `HeartRateSeriesStore` that wraps the existing `samples(from:to:)` method:

```swift
// [ASSUMED] — implementation sketch
func decimatedSamples(from start: Date, to end: Date, maxCount: Int = 500) -> [HeartRateSamplePoint] {
  let raw = samples(from: start, to: end)
  guard raw.count > maxCount else { return raw }

  let stride = raw.count / maxCount
  var result: [HeartRateSamplePoint] = []
  result.reserveCapacity(maxCount + stride * 2)

  var i = 0
  while i < raw.count {
    let windowEnd = min(i + stride, raw.count)
    let window = raw[i..<windowEnd]
    // Always keep the first sample in the stride
    result.append(raw[i])
    // Also keep local min and max (if different from first)
    if let maxSample = window.max(by: { $0.bpm < $1.bpm }),
       maxSample.id != raw[i].id {
      result.append(maxSample)
    }
    if let minSample = window.min(by: { $0.bpm < $1.bpm }),
       minSample.id != raw[i].id,
       minSample.id != (result.last?.id ?? "") {
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

**Note:** The `stateLock` is already acquired inside `samples(from:to:)` — the new `decimatedSamples` method calls the existing public method which handles locking. No need to touch the lock directly.

### Callers to Migrate

All callers confirmed by codebase grep — none are in SwiftUI view files:

| File | Method | Current Call | Migration |
|------|--------|-------------|-----------|
| `HealthDataStore+Snapshots.swift:996` | `hkStrainScore()` | `heartRateSeriesStore.samples(forDayContaining: Date())` | `heartRateSeriesStore.decimatedSamples(forDayContaining: Date())` |
| `HealthDataStore+Snapshots.swift:1129` | (trend computation) | `heartRateSeriesStore.samples(forDayContaining: day)` | `heartRateSeriesStore.decimatedSamples(forDayContaining: day)` |
| `HealthDataStore+StressEnergy.swift:20` | stress computation | `heartRateSeriesStore.samples(forDayContaining: date, calendar: calendar)` | `heartRateSeriesStore.decimatedSamples(forDayContaining: date, calendar: calendar)` |
| `HealthDataStore+Cardio.swift:172` | session cardio | `heartRateSeriesStore.samples(from: start, to: end)` | `heartRateSeriesStore.decimatedSamples(from: start, to: end)` |

**No SwiftUI chart views directly call `HeartRateSeriesStore`** — confirmed by grep. All sample access is mediated through `HealthDataStore` extension methods. This makes the migration surgical and low-risk.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Notification scheduling | Custom Timer-based delivery | `UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)` | System delivers even if app backgrounded; no custom timer needed |
| Concurrent notification dispatch | DispatchQueue.main.async wrappers | `actor NotificationScheduler` | Swift actor guarantees serial execution; no manual lock needed |
| HR chart performance | Custom downsampling with complex algorithms | Stride-N with local min/max | Sufficient for 500-sample target; LTTB explicitly deferred |
| MoreRoute navigation | Custom NavigationLink destinations | `navigationDestination(for: MoreRoute.self)` (already wired in MoreView) | Pattern already established; destination switch handles all routes |

---

## Common Pitfalls

### Pitfall 1: VOW Snapshot Value Is a String, Not a Double

**What goes wrong:** `healthStore.snapshot(for: .recovery).value` returns `"68"` not `68.0`. Treating it as a ready-to-compare number causes a type error.

**Why it happens:** `HealthMetricSnapshot.value` is always `String` (see `HealthModels.swift` line 63). The unit (%) is in a separate field.

**How to avoid:** Always parse: `guard let score = Double(snapshot.value) else { return nil }`.

**Warning signs:** A nudge that never fires even with low recovery — check if the parse fails.

### Pitfall 2: Task.isCancelled Must Be Checked After Every sleep()

**What goes wrong:** The Interval Timer loop continues one extra cycle after `.cancel()` is called.

**Why it happens:** `try? await Task.sleep(...)` discards the cancellation error. The loop only terminates if `Task.isCancelled` is checked explicitly after the sleep.

**How to avoid:** Copy BreatheView's exact pattern: `try? await Task.sleep(...)` immediately followed by `guard !Task.isCancelled else { break }`.

**Warning signs:** Buzz fires once after user taps Stop.

### Pitfall 3: Battery Notification Fires on Every Reconnect Without the Bool Gate

**What goes wrong:** If `batteryLowNotificationFired` is not reset on disconnect, it stays `false` across sessions and the notification never fires again. If it is not reset on *reconnect*, it fires only on the first session ever.

**Why it happens:** The reset must happen in `resetLiveDeviceFieldsIfNeeded(for:)` which is the canonical field reset path (confirmed at line 459 in GooseBLEClient+Parsing.swift). This method is called when a new device becomes active — which covers reconnect correctly.

**How to avoid:** Add `batteryLowNotificationFired = false` inside `resetLiveDeviceFieldsIfNeeded(for:)` alongside the existing battery field resets.

**Warning signs:** Battery notification fires multiple times in one session (Bool not set), or never fires after first session (Bool not reset).

### Pitfall 4: MoreRouteStatus Must Have All Fields — No Default Values

**What goes wrong:** Adding a new `MoreRoute` case without adding the corresponding field to `MoreRouteStatus` causes a compile error on the struct literal initialisation in both `MoreDataStore.init` and `refreshRouteStatus`.

**Why it happens:** `MoreRouteStatus` is a plain `struct` with no optional fields and no defaults. Both the init and `refreshRouteStatus` must enumerate all fields.

**How to avoid:** Add both `intervalTimer` and `metricExplorer` to: (1) `MoreRouteStatus` struct fields, (2) `MoreDataStore.routeStatus` init, (3) `refreshRouteStatus` return expression, (4) `MoreRoute.statusKeyPath` switch.

**Warning signs:** `error: missing argument for parameter 'intervalTimer' in call` on the struct literal.

### Pitfall 5: NSLock in HeartRateSeriesStore — Don't Lock Twice

**What goes wrong:** If `decimatedSamples` acquires `stateLock` and then calls `samples(from:to:)` which also acquires `stateLock`, a deadlock occurs (NSLock is not reentrant).

**Why it happens:** `NSLock` is not recursive. Double-locking on the same thread causes an immediate deadlock.

**How to avoid:** `decimatedSamples` must call the public `samples(from:to:)` method (which acquires and releases the lock itself) — not a `_locked` variant. The decimation math runs *after* the lock is released.

**Warning signs:** App hangs on first HR chart render after decimation is added.

### Pitfall 6: MetricExplorerView healthStore Access — Must be Passed, Not Injected

**What goes wrong:** Using `@EnvironmentObject var healthStore: HealthDataStore` in MetricExplorerView causes a crash because `HealthDataStore` is created with `@StateObject` in `AppShellView` and injected only to the coach/health tabs — not as an environment object.

**Why it happens:** `AppShellView` creates `healthStore` with `@State private var healthStore = HealthDataStore()` (line 6) but passes it explicitly to tab views, not via `.environmentObject(...)`.

**How to avoid:** Follow `MoreAlgorithmsView` pattern — pass `healthStore: HealthDataStore` as a parameter in `MoreView.destination(for:)`.

---

## Code Examples

### VOW Card (verified pattern — uses existing coachCardSurface modifier)
```swift
// Source: GooseSwift/CoachView.swift — coachCardSurface is defined at line 693
private struct CoachVOWCard: View {
  let nudge: CoachVOWNudge
  let onDismiss: () -> Void

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: nudge.systemImage)
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(nudge.tint)
        .frame(width: 36, height: 36)
        .background(nudge.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

      VStack(alignment: .leading, spacing: 3) {
        Text(nudge.title)
          .font(.headline)
        Text(nudge.body)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }

      Spacer(minLength: 8)

      Button(action: onDismiss) {
        Image(systemName: "xmark")
          .font(.caption.weight(.semibold))
          .foregroundStyle(.secondary)
      }
    }
    .padding(14)
    .coachCardSurface(tint: nudge.tint)
  }
}
```

### Notification Scheduling (no framework imports needed beyond UserNotifications)
```swift
// [ASSUMED] — implementation pattern
import UserNotifications

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
}
```

### Stride-N Decimation (self-contained, no imports)
```swift
// Source: GooseSwift/HeartRateSeriesStores.swift — to be added
func decimatedSamples(from start: Date, to end: Date, maxCount: Int = 500) -> [HeartRateSamplePoint] {
  let raw = samples(from: start, to: end)
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
       minSample.id != first.id, minSample.id != result.last?.id {
      result.append(minSample)
    }
    i += stride
  }
  return result.sorted { $0.capturedAt < $1.capturedAt }
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `repeat { ... } while true` + `DispatchQueue` for UI timers | `Task { @MainActor in repeat { ... } while !Task.isCancelled }` | Swift Concurrency (Swift 5.5+) | Cancellation is cooperative and safe; no DispatchQueue reference capture needed |
| Singleton notification manager class | `actor` for shared mutable notification state | Swift 5.5+ | Actors eliminate explicit locking for concurrent notification dispatch |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS` is the best HRV source for VOW thresholds | FEAT-01 VOW Data Access | VOW HRV nudge never fires if a different HRV source is intended; clarify with user if needed |
| A2 | Notification identifier pattern using `Int(Date().timeIntervalSince1970)` suffix prevents duplicate suppression | FEAT-03 Notification Identifiers | If the same notification arrives twice within 1 second the identifier collides; use `UUID().uuidString` instead for absolute uniqueness |
| A3 | `decimatedSamples` method calling public `samples(from:to:)` (not a locked variant) avoids NSLock deadlock | DATA-04 NSLock note | Confirmed by reading HeartRateSeriesStores.swift — `samples(from:to:)` acquires and releases lock internally; no reentry |
| A4 | No chart views call `HeartRateSeriesStore.samples()` directly | DATA-04 Callers | Confirmed by codebase grep — all callers are in `HealthDataStore+*` files |

---

## Open Questions

1. **HRV source for VOW**
   - What we know: `ble.liveHRVRMSSD` has the most recent live reading; `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS` uses the 14-day store with weight by RR count
   - What's unclear: CONTEXT.md says "weekly average from healthStore" but `healthStore` has no direct `weeklyHRVAverage` property — nearest is `HRVSeriesStore.shared.dailyEstimate()`
   - Recommendation: Use `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS` as the weekly-ish average; mention explicitly in PLAN.md

2. **MetricExplorer route list**
   - What we know: CONTEXT.md says "readiness, recovery, strain, HRV, RHR, sleep, stress" — but `HealthRoute` has no `.rhr` case
   - What's unclear: RHR is derived data, not a HealthRoute — it may come from `HeartRateSeriesStore.shared.restingEstimate()`
   - Recommendation: Show RHR as a separate row computed from `restingEstimate()` rather than from `snapshot(for:)` — call this out in PLAN.md task

---

## Environment Availability

Step 2.6 SKIPPED — phase is pure Swift/SwiftUI code additions. No CLI tools, databases, or external services required beyond Xcode (already available).

---

## Validation Architecture

Nyquist validation config not explicitly set to false. Existing test infrastructure: Rust integration tests only (`Rust/core/tests/`). No Swift test target detected in project.

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FEAT-01 | VOW nudge resolves correctly for each threshold | unit | N/A — no Swift test target | ❌ manual verify |
| FEAT-02 | IntervalTimerView session loop cancels on Stop | manual | Run in simulator; tap Stop mid-session | — |
| FEAT-02 | MetricExplorerView shows values from healthStore | manual | Run in simulator; verify rows appear | — |
| FEAT-03 | Sleep notification fires after syncBandSleepHistory | manual | Trigger morning sync; check notification | — |
| FEAT-03 | Battery notification fires once when <= 20% | manual | Simulate low battery BLE value | — |
| DATA-04 | decimatedSamples returns <= 500 points when count > 1000 | unit | N/A — no Swift test target | ❌ manual verify |

### Wave 0 Gaps
- No Swift test target exists — all validation is manual via simulator
- Consider adding a `CoachVOWNudge.resolve()` unit test if ARCH-01 (Phase 72) adds a test target

---

## Security Domain

No authentication, cryptography, or user data network transfer in this phase. All data is local (SQLite via Rust bridge, UNUserNotificationCenter local only, UserDefaults). ASVS V5 input validation applies only to the notification body string — use `String(format:)` with explicit format specifiers, not string interpolation with untrusted data.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes (notification body) | Format numeric values with `String(format: "%.0f", value)` |
| V6 Cryptography | no | — |

---

## Sources

### Primary (HIGH confidence)
- `GooseSwift/CoachView.swift` — confirmed insertion point, CoachOverviewScreen LazyVStack order, coachCardSurface modifier, healthStore parameter availability
- `GooseSwift/BreatheView.swift` — confirmed Task { @MainActor in repeat { } while !Task.isCancelled } pattern, buzz(loops:1) call, .onDisappear { stopSession() } wiring
- `GooseSwift/HeartRateSeriesStores.swift` — confirmed HeartRateSamplePoint struct, private samples array, stateLock (NSLock), samples(from:to:) public API
- `GooseSwift/MoreRouteModels.swift` — confirmed 4-switch pattern, MoreRouteStatus struct, wellnessRoutes array
- `GooseSwift/MoreView.swift` — confirmed destination(for:) switch, Section("Wellness"), healthStore parameter passing pattern
- `GooseSwift/MoreDataStore.swift` — confirmed routeStatus init and refreshRouteStatus update sites
- `GooseSwift/GooseAppModel+SleepSync.swift` — confirmed syncBandSleepHistory() completion point (line 174)
- `GooseSwift/GooseAppModel+PacketPublishing.swift` — confirmed .finished(summary, reason:) handler location (line 743)
- `GooseSwift/GooseBLEClient+Parsing.swift` — confirmed applyBatteryLevel() as the battery update entry point
- `GooseSwift/GooseBLEClient.swift` — confirmed batteryLevelPercent property, resetLiveDeviceFieldsIfNeeded() as reset site
- `GooseSwift/OnboardingView.swift` line 478 — confirmed UNUserNotificationCenter.requestAuthorization(options: [.alert, .badge, .sound])
- `GooseSwift/HealthModels.swift` — confirmed HealthMetricSnapshot.value is String, displayValue adds unit suffix
- Codebase grep — confirmed all HeartRateSamplePoint callers are in HealthDataStore+* files only

### Secondary (MEDIUM confidence)
- `.planning/phases/71-coach-vow-noopapp-notifications-hr-decimation/71-CONTEXT.md` — locked decisions and integration points

### Tertiary (LOW confidence — marked [ASSUMED])
- Implementation sketches for VOW priority logic, NotificationScheduler actor, decimation algorithm
- These are reasonable pattern extensions but have not been validated against a running build

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries are iOS SDK built-ins already in use
- Architecture: HIGH — confirmed by reading all key source files
- Code patterns: HIGH for BreatheView/MoreRoute (read from source); MEDIUM for new files (implementation sketches)
- Pitfalls: HIGH — each pitfall was derived from reading the actual code structure

**Research date:** 2026-06-12
**Valid until:** 2026-07-12 (stable Swift patterns; no fast-moving ecosystem)
