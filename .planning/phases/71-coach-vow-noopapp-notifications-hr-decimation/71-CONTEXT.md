# Phase 71: Coach VOW + NoopApp Features + Notifications + HR Decimation - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

FEAT-01: Coach VOW — a single contextual nudge card at the top of CoachView, computed locally from existing healthStore data, requiring no server call.

FEAT-02: Three NoopApp-derived features reachable from the app:
- Breathe screen — already done in Phase 70 (MoreRoute.breathe); no new work needed
- Interval Timer — new `MoreRoute.intervalTimer` in More tab "Wellness" section; work/rest intervals with `buzz(loops:)` at each interval end
- Metric Explorer — new `MoreRoute.metricExplorer` in More tab "Data" section; scrollable list of metrics from `healthStore.snapshot`

FEAT-03: Three local notifications via `UNUserNotificationCenter` (permission already granted in onboarding):
- Sleep processed: fires at end of `syncBandSleepHistory()` with sleep duration + HRV + estimated recovery
- Workout detected: fires at `PassiveActivityDetector.finished(summary, reason:)` with duration + strain
- Battery low: fires when BLE `batteryLevel <= 20` for the first time per BLE session

DATA-04: HR chart decimation via stride-N in `HeartRateSeriesStore` — getter returns decimated array if sample count > 1000, preserving local max/min within each stride window.

</domain>

<decisions>
## Implementation Decisions

### Coach VOW
- Placement: horizontal card/banner at the top of CoachView's main VStack, above CoachRoutesSection
- Maximum nudges shown: 1 (the most urgent, by priority: recovery > strain > HRV)
- Thresholds: recovery < 33% → "Critical Recovery"; recovery < 66% → "Low Recovery"; strain > 18 → "High Strain"; HRV < 30ms → "Low HRV" (weekly average from healthStore)
- Data source: `healthStore` existing snapshot calls — no new bridge methods or SQLite reads

### Interval Timer (FEAT-02)
- Entry point: `MoreRoute.intervalTimer` — new case in MoreRouteModels.swift, "Wellness" section alongside .breathe
- Functionality: user configures work duration (seconds) + rest duration (seconds); timer counts down; `model.ble.buzz(loops: 1)` fires at each interval transition (work→rest, rest→work)
- Session control: Start/Stop button; session free-running with configurable interval count or infinite

### Metric Explorer (FEAT-02)
- Entry point: `MoreRoute.metricExplorer` — new case in MoreRouteModels.swift, new "Data" section
- Content: scrollable list of metric names + current values from `healthStore.snapshot(for:)` calls — readiness, recovery, strain, HRV, RHR, sleep, stress
- No graphs or historical views in this phase — list only

### Notifications (FEAT-03)
- Payload format: title + body with metric values (e.g., "Sleep synced · 7h23m — HRV 52ms, Recovery 68%")
- Sleep notification: scheduled inside `syncBandSleepHistory()` completion handler
- Workout notification: scheduled inside `PassiveActivityDetector.finished(summary, reason:)` handler
- Battery notification: scheduled in the BLE battery level callback when `batteryLevel <= 20` for the first time per connection session (track with a Bool flag reset on each BLE connect)
- All use `UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)` (fire immediately)

### HR Decimation (DATA-04)
- Algorithm: stride-N — keep 1 sample per N, plus the local max and min within each stride window (to preserve peaks and valleys)
- Location: computed property on `HeartRateSeriesStore` — `var decimatedSamples: [HeartRateSample]` that returns decimated array when `samples.count > 1000`, raw array otherwise
- Target maximum: 500 samples (so stride = samples.count / 500)
- Chart views that currently read `store.samples` should switch to `store.decimatedSamples`

### Claude's Discretion
- Exact VOW card visual design (color, icon, dismissal)
- Interval Timer default work/rest durations (suggest 30s/10s)
- Whether battery notification uses a `.badge` in addition to `.alert` + `.sound`
- Which chart views to update to use decimatedSamples

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `healthStore.snapshot(for:)` — existing method returning `MetricSnapshot?` per metric; drives all Coach data
- `MoreRoute` enum in `MoreRouteModels.swift` — add `.intervalTimer` and `.metricExplorer`; same pattern as `.breathe` from Phase 70
- `model.ble.buzz(loops:)` — Phase 70 artifact; Interval Timer calls this at each transition
- `UNUserNotificationCenter` — permission already granted in onboarding (`OnboardingView.swift` line 478)
- `syncBandSleepHistory()` — sleep sync completion, triggers sleep notification
- `PassiveActivityDetector.finished(summary, reason:)` — workout completion trigger
- `HeartRateSeriesStore.shared` — singleton in `HeartRateSeriesStores.swift`; `samples: [HeartRateSample]` is the raw array

### Established Patterns
- MoreRoute addition: same 4-switch pattern from Phase 70 + MoreDataStore update
- BreatheView pattern for session-based views: `Task { @MainActor in repeat { ... } while !Task.isCancelled }` + `.onDisappear { stopSession() }`
- Notification scheduling: `UNUserNotificationCenter.current().add(UNNotificationRequest(...))`
- No external dependencies: pure Swift stdlib + Foundation + UserNotifications

### Integration Points
- `GooseSwift/CoachView.swift` — add VOW card at top of main VStack
- `GooseSwift/MoreRouteModels.swift` — add `.intervalTimer`, `.metricExplorer` cases
- `GooseSwift/MoreDataStore.swift` — add both new routes to construction sites
- `GooseSwift/MoreView.swift` — add rows for both new routes (Wellness + new Data section)
- `GooseSwift/AppShellView.swift` — add navigationDestination cases
- `GooseSwift/HeartRateSeriesStores.swift` — add `decimatedSamples` computed property
- Chart views using `store.samples` — migrate to `store.decimatedSamples`

</code_context>

<specifics>
## Specific Ideas

- Breathe screen already accessible via Phase 70 — FEAT-02 success criterion 2a is already met
- VOW nudge should be dismissable (user can swipe away for current session)
- Battery low notification should only fire once per BLE connection session (not repeatedly)
- Metric Explorer rows: name (localized), current value (formatted), last updated timestamp

</specifics>

<deferred>
## Deferred Ideas

- VOW nudge history / tap to see trend — out of scope for this phase
- Interval Timer: multiple preset programs, custom haptic patterns — Phase 73+ if needed
- Metric Explorer: sparkline charts, metric comparison — Phase 72 or later
- LTTB decimation algorithm — stride-N is sufficient for now; LTTB is a nice-to-have

</deferred>
