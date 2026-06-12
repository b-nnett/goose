# Phase 72: Screens on New Foundation + Service Layer - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

DATA-03: Three screens using Phase 69 tables:
1. **Stress/ANS view** — extend the existing `HealthRoute.stress` destination with HRV + RHR ANS tiles from bridge data (the route/screen already exists; add ANS section)
2. **Trends dashboard** — new `HealthRoute.trends` in Health tab; sparkline charts for Recovery, HRV, Strain over 7 days from the `metricSeries` table via a new bridge method `metric_series.query_range`
3. **Manual Workout Entry** — `.sheet` modal accessible from the Fitness/Activity tab; logs a workout entry to the `workout` table via `workout.upsert`

ARCH-01: Swift protocols + mocks + tests:
- `GooseBLEManaging`, `GooseRustBridging`, `HealthDataStoring` protocols in separate files
- Create minimal `GooseSwiftTests` Xcode test target
- `MockBLEClient`, `MockRustBridge`, `MockHealthStore` mock classes — minimal conformance
- 2 unit tests using the mocks: (1) WorkoutEntryTests verifying `workout.upsert` is called; (2) TrendsFetchTests verifying `metric_series.query_range` is called

</domain>

<decisions>
## Implementation Decisions

### Stress/ANS View Extension (DATA-03, screen 1)
- Extend the existing `HealthRoute.stress` destination screen (not a new screen)
- Add an "ANS Balance" section below existing stress content with HRV tile (from `HRVSeriesStore.shared.dailyEstimate()?.rmssdMS`) and RHR tile (from `HeartRateSeriesStore.shared.restingEstimate()`)
- No new route needed — extend in-place

### Trends Dashboard (DATA-03, screen 2)
- New `case trends` on `HealthRoute` enum
- Accessible from Health tab NavigationStack (same destination pattern as existing health routes)
- 3 sparklines: Recovery, HRV, Strain — 7 days fixed period
- Data source: new Rust bridge method `metric_series.query_range` → returns array of `{date, value}` for a given metric_name and date range
- Visualization: SwiftUI `Path` polyline (no Charts.framework — no external dependencies)
- Default period: 7 days fixed (no period selector UI in this phase)

### Manual Workout Entry (DATA-03, screen 3)
- `.sheet` modal presented from the fitness/activity context (e.g., from ActivitySessionModel or a dedicated "Log" button in HomeView or FitnessView)
- User selects: sport (ActivityKind enum — existing), duration (minutes, numeric input), perceived effort (Int 1–10 scale)
- On submit: calls `workout.upsert` bridge method with date, sport, duration_s, perceived_effort
- Sheet has Cancel + Log buttons; dismiss on success

### Protocols (ARCH-01)
- `GooseBLEManaging.swift` — protocol for GooseBLEClient (methods/properties used by GooseAppModel)
- `GooseRustBridging.swift` — protocol for GooseRustBridge (`request(method:args:)` signature)
- `HealthDataStoring.swift` — protocol for HealthDataStore (the main data-read methods)
- Location: `GooseSwift/` alongside the existing concrete classes

### Test Target + Mocks + Tests (ARCH-01)
- Create `GooseSwiftTests` XCTest target in GooseSwift.xcodeproj
- Mocks: `MockBLEClient: GooseBLEManaging`, `MockRustBridge: GooseRustBridging`, `MockHealthStore: HealthDataStoring` — minimal conformance (only properties/methods required by the 2 tests)
- Test 1 (WorkoutEntryTests): instantiate a view with MockRustBridge; call submit; assert that `request(method:args:)` was called with `method == "workout.upsert"`
- Test 2 (TrendsFetchTests): instantiate a Trends view with MockRustBridge; trigger data fetch; assert `request(method:args:)` called with `method == "metric_series.query_range"`
- Tests use XCTest framework; pass with Swift test runner (xcodebuild test)

### Claude's Discretion
- Exact tile layout for ANS section in Stress view
- Sparkline visual style (line color, stroke width, axis labels)
- Where exactly the "Log Workout" button appears (HomeView or FitnessView)
- Mock assertion mechanism (e.g., `var lastMethod: String?` recorded in mock)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `HealthRoute` enum in `HealthModels.swift` — add `case trends`; `case stress` already exists
- `HealthDataStore+Cardio.swift`, `HealthDataStore+Snapshots.swift` — patterns for bridge method calls
- `GooseRustBridge.request(method:args:)` — existing bridge call pattern (returns `GooseRustBridgeResult`)
- `ActivityKind` enum — existing sport types for Manual Workout Entry
- `HRVSeriesStore.shared.dailyEstimate()` — HRV source for ANS tile
- `HeartRateSeriesStore.shared.restingEstimate()` — RHR source for ANS tile
- Phase 69 bridge methods: `workout.upsert`, `metric_series.query_range` (query_range is new — needs adding to Rust bridge)
- Phase 70 `BreatheView.swift` — pattern for Health tab destination views

### Established Patterns
- Health route destinations: push navigation via `healthPath` + `navigationDestination(for: HealthRoute.self)` in AppShellView
- Bridge calls: `try rust.request(method: "...", args: [...])` pattern in HealthDataStore extensions
- SwiftUI `Path`: existing use in ChartViews or no prior use — simple polyline from normalized points

### Integration Points
- `GooseSwift/HealthModels.swift` — add `case trends` + localized title
- `GooseSwift/AppShellView.swift` — add `case .trends: TrendsDashboardView(healthStore: healthStore)` in navigationDestination
- `GooseSwift/HealthDashboardViews.swift` — add "Trends" entry point row or button
- `GooseSwift/HealthRecoveryStressViews.swift` (or CoachRouteViews.swift stress section) — add ANS tiles
- `Rust/core/src/bridge.rs` — add `metric_series.query_range` dispatch arm
- `GooseSwift.xcodeproj/project.pbxproj` — add new test target + new Swift files

</code_context>

<specifics>
## Specific Ideas

- Phase 69 added `metric_series` table with (source, metric_name, date, value) — Trends reads from this via `metric_series.query_range`
- The Rust bridge may need a new `metric_series.query_range` method; alternatively use the existing SQLite query pattern for date-range reads
- Manual Workout Entry "Log" button could be a toolbar item in the Fitness view or a dedicated row in HomeView

</specifics>

<deferred>
## Deferred Ideas

- Trends period selector (30/90 days) — defer to later phase
- Full protocol adoption across all GooseAppModel call sites — only 2 unit tests need the mocks in this phase
- Mock-based full integration test suite — minimum viable: 2 passing tests
- ANS trend history (7-day HRV chart in Stress view) — only point-in-time tiles in this phase

</deferred>
