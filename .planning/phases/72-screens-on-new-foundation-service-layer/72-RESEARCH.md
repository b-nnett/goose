# Phase 72: Screens on New Foundation + Service Layer - Research

**Researched:** 2026-06-12
**Domain:** SwiftUI screens (Stress/ANS, Trends, Manual Workout), Swift protocol layer, XCTest mocks, Rust bridge new method
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Stress/ANS view: extend `HealthRoute.stress` in-place; add "ANS Balance" section with HRV tile (`HRVSeriesStore.shared.dailyEstimate()?.rmssdMS`) and RHR tile (`HeartRateSeriesStore.shared.restingEstimate()`); no new route
- Trends: new `case trends` on `HealthRoute`; 3 sparklines (Recovery, HRV, Strain) over 7 days fixed; data source is new bridge method `metric_series.query_range`; visualization via SwiftUI `Path` polyline (no Charts.framework)
- Manual Workout Entry: `.sheet` modal from fitness/activity context; user picks sport (`ActivityKind`), duration (minutes), perceived effort (Int 1–10); calls `workout.upsert` on submit; Cancel + Log buttons; dismiss on success
- Protocols: `GooseBLEManaging`, `GooseRustBridging`, `HealthDataStoring` in separate files in `GooseSwift/`
- Test target: `GooseSwiftTests` XCTest target already exists — do not create a new one
- Mocks: `MockBLEClient`, `MockRustBridge`, `MockHealthStore` — minimal conformance for the 2 tests only
- Test 1 (WorkoutEntryTests): assert `request(method:args:)` called with `method == "workout.upsert"`
- Test 2 (TrendsFetchTests): assert `request(method:args:)` called with `method == "metric_series.query_range"`
- Tests pass with `xcodebuild test`
- No external dependencies — no Charts.framework, no SPM packages

### Claude's Discretion
- Exact tile layout for ANS section in Stress view
- Sparkline visual style (line color, stroke width, axis labels)
- Where exactly the "Log Workout" button appears (HomeView or FitnessView)
- Mock assertion mechanism (e.g., `var lastMethod: String?` recorded in mock)

### Deferred Ideas (OUT OF SCOPE)
- Trends period selector (30/90 days)
- Full protocol adoption across all GooseAppModel call sites
- Mock-based full integration test suite (minimum: 2 passing tests)
- ANS trend history (7-day HRV chart in Stress view) — only point-in-time tiles in this phase
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DATA-03 | Three screens using Phase 69 tables: Stress/ANS tiles, Trends dashboard (7-day sparklines), Manual Workout Entry sheet | Bridge method `workout.upsert` confirmed present; `metric_series.query_range` confirmed absent — needs Rust addition; `StressV2OverviewPage` is the correct insertion point for ANS section; `HealthRoute` enum and `HealthView.navigationDestination` pattern confirmed |
| ARCH-01 | Swift protocols + mocks + 2 unit tests in `GooseSwiftTests` target | `GooseSwiftTests` target already exists with real tests; `GooseRustBridge.request(method:args:)` signature confirmed; existing test file pattern confirmed (`@testable import GooseSwift`, XCTestCase subclass) |
</phase_requirements>

---

## Summary

Phase 72 spans two independent workstreams: (1) DATA-03 UI screens that read from Phase 69 tables, and (2) ARCH-01 Swift protocol extraction enabling unit testing. Neither workstream blocks the other — they can be planned as separate plan files and executed in any order.

The `GooseSwiftTests` XCTest target already exists in `GooseSwift.xcodeproj` with 10 test files and an `Info.plist`. No target creation is needed — new test files are added to the existing target via `project.pbxproj`. The test pattern is `@testable import GooseSwift` + `XCTestCase` subclass, consistent with `GooseBLETypesTests.swift` and `GooseUploadServiceTests.swift`.

The critical Rust work for DATA-03 is adding `metric_series.query_range` as a bridge method and a corresponding `query_metric_series_range` function in `store.rs`. The `workout.upsert` method already exists (Phase 69) with full arg struct and dispatcher arm. The `metric_series` table schema is confirmed: `(id, source, metric_name, date, value, created_at, updated_at)` with `UNIQUE(source, metric_name, date)`.

**Primary recommendation:** Plan 72-01 = Rust bridge `metric_series.query_range` + Stress ANS tiles + Trends dashboard + Manual Workout Entry. Plan 72-02 = Swift protocols (`GooseRustBridging`, `GooseBLEManaging`, `HealthDataStoring`) + mocks + 2 tests. The Rust method must land before Trends can be wired in Swift.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Stress/ANS tile display | Frontend (SwiftUI) | — | Point-in-time data already in-process via `HRVSeriesStore.shared` / `HeartRateSeriesStore.shared`; no bridge call needed |
| Trends data fetch | API / Rust bridge | Database (SQLite) | `metric_series.query_range` reads from Phase 69 table; Swift calls bridge async |
| Manual workout submit | API / Rust bridge | Database (SQLite) | `workout.upsert` persists to workout table; Swift calls bridge on submit |
| Protocol definitions | Frontend (Swift types) | — | Protocol files live in `GooseSwift/`; no Rust involvement |
| Mock implementations | Test target | — | Mocks live in `GooseSwiftTests/`; used only in unit tests |

---

## Standard Stack

### Core (no new packages — project constraint)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| XCTest | system (Xcode 26) | Unit test framework | Already in `GooseSwiftTests` target; `BUNDLE_LOADER` = main app for `@testable import` |
| SwiftUI `Path` | system | Sparkline polylines | No external charting dependency; pattern matches `StressV2TimelineChart` in codebase |
| `rusqlite` | 0.37 (existing) | SQLite query for `metric_series.query_range` | Already in `Cargo.toml`; all store queries use it |

**Installation:** None — no new packages. All code uses system frameworks and existing Cargo dependencies.

---

## Package Legitimacy Audit

> No external packages are introduced in this phase. All code uses system frameworks (XCTest, SwiftUI) and existing Rust dependencies (`rusqlite`, `serde_json`).

**Packages removed due to SLOP verdict:** none
**Packages flagged as suspicious:** none

---

## Architecture Patterns

### System Architecture Diagram

```
HRVSeriesStore.shared ──────────────────────────────────► StressV2OverviewPage
HeartRateSeriesStore.shared ─────────────────────────────► (ANS section appended below Breakdown)

TrendsDashboardView ──► HealthDataStore.fetchMetricSeries(7d) ──► bridge.requestAsync("metric_series.query_range")
                                                                        │
                                                              Rust: query metric_series table
                                                              WHERE metric_name IN ('recovery','hrv','strain')
                                                              AND date >= today-7d
                                                              └──► [{date, value}] ──► SwiftUI Path polyline

ManualWorkoutEntrySheet ──► submit ──► bridge.requestAsync("workout.upsert") ──► Rust: insert_workout()
                                                                                        │
                                                                                   workout table (Phase 69)

GooseRustBridging (protocol) ◄── GooseRustBridge (concrete)
                              ◄── MockRustBridge (test mock — records lastMethod)
                              │
WorkoutEntryTests: MockRustBridge → assert lastMethod == "workout.upsert"
TrendsFetchTests:  MockRustBridge → assert lastMethod == "metric_series.query_range"
```

### Recommended Project Structure

```
GooseSwift/
├── GooseBLEManaging.swift       # protocol for GooseBLEClient (ARCH-01)
├── GooseRustBridging.swift      # protocol for GooseRustBridge (ARCH-01)
├── HealthDataStoring.swift      # protocol for HealthDataStore (ARCH-01)
├── TrendsDashboardView.swift    # new HealthRoute.trends destination (DATA-03)
├── ManualWorkoutEntrySheet.swift # .sheet modal for workout logging (DATA-03)
GooseSwiftTests/
├── WorkoutEntryTests.swift      # test: workout.upsert called (ARCH-01)
├── TrendsFetchTests.swift       # test: metric_series.query_range called (ARCH-01)
├── MockBLEClient.swift          # GooseBLEManaging conformance (ARCH-01)
├── MockRustBridge.swift         # GooseRustBridging conformance (ARCH-01)
├── MockHealthStore.swift        # HealthDataStoring conformance (ARCH-01)
Rust/core/src/
├── bridge.rs                    # add "metric_series.query_range" to BRIDGE_METHODS + dispatcher arm
└── store.rs                     # add query_metric_series_range() fn
```

### Pattern 1: Stress ANS Tile Insertion Point

**What:** `StressV2OverviewPage.body` in `HealthRecoveryStressViews.swift` — insert ANS section after the `StressV2BreakdownSection` block (line ~379), before `SleepV2SectionHeader(title: "Trends", ...)`.

**Exact insertion location (verified):**
```swift
// Source: GooseSwift/HealthRecoveryStressViews.swift lines 378-381
StressV2BreakdownSection(palette: palette, summary: summary)

// ← INSERT ANS SECTION HERE ←

SleepV2SectionHeader(title: "Trends", palette: palette)
```

**ANS tile data sources (no bridge call needed):**
```swift
// Source: GooseSwift/HeartRateSeriesStores.swift
let rmssd: Double? = HRVSeriesStore.shared.dailyEstimate()?.rmssdMS
let rhr: Double?   = HeartRateSeriesStore.shared.restingEstimate()
```

Use `SleepV2StatCard` (existing component) to render both tiles:
```swift
SleepV2SectionHeader(title: "ANS Balance", palette: palette)
HStack(spacing: 12) {
  SleepV2StatCard(
    palette: palette,
    systemImage: "waveform.path.ecg",
    label: "Resting HRV",
    value: rmssd.map { "\(Int($0.rounded())) ms" } ?? "No data"
  )
  SleepV2StatCard(
    palette: palette,
    systemImage: "heart.fill",
    label: "Resting HR",
    value: rhr.map { "\(Int($0.rounded())) bpm" } ?? "No data"
  )
}
.frame(height: 96)
```

### Pattern 2: HealthRoute.trends Navigation Wiring

**What:** `HealthRoute` enum is in `HealthModels.swift`. Navigation destination is in `HealthView.swift` (the Health tab) via `.navigationDestination(for: HealthRoute.self)`. The Home tab uses `AppShellView` → `HealthRouteDestinationView` → `HealthRouteContentView`.

**Add `case trends` to `HealthRoute`:**
```swift
// Source: GooseSwift/HealthModels.swift
enum HealthRoute: String, CaseIterable, Identifiable, Hashable {
  // existing cases...
  case trends  // ADD HERE

  var title: String {
    // ...
    case .trends: "Trends"
  }
  var systemImage: String {
    // ...
    case .trends: "chart.line.uptrend.xyaxis"
  }
}
```

**Wire in `HealthRouteContentView` (HealthDashboardViews.swift line ~350):**
```swift
case .trends:
  TrendsDashboardView(store: store)
```

**Add entry point in `HealthView.swift`** — the "Explore Health" `HealthRouteShortcutSection` is the natural home:
```swift
// Source: GooseSwift/HealthView.swift line 35-38
HealthRouteShortcutSection(
  title: "Explore Health",
  snapshots: snapshots(for: [.trends, .stress, .cardioLoad, .energyBank])  // add .trends
)
```

Note: `snapshots(for:)` compactMaps from `cachedLandingSnapshots`, so `HealthDataStore.snapshot(for: .trends)` needs to return a valid `HealthMetricSnapshot`. A simple stub snapshot is sufficient for the shortcut card.

### Pattern 3: Rust bridge.rs dispatch arm pattern

**What:** New method `metric_series.query_range` needs: (1) entry in `BRIDGE_METHODS` constant (sorted), (2) `MetricSeriesQueryRangeArgs` struct, (3) dispatch arm in `handle_bridge_request`, (4) `metric_series_query_range_bridge` function, (5) `GooseStore::query_metric_series_range` in `store.rs`.

**BRIDGE_METHODS placement** (sorted alphabetically — between `metric_series.upsert` and `metrics.activity_unavailable_daily_status`):
```rust
// Source: Rust/core/src/bridge.rs line 245
"metric_series.upsert",
"metric_series.query_range",   // INSERT HERE (alphabetically after upsert)
"metrics.activity_unavailable_daily_status",
```

Wait — `query_range` sorts before `upsert` alphabetically. Correct placement:
```
"metric_series.query_range",   // q < u
"metric_series.upsert",
```

**Args struct:**
```rust
#[derive(Debug, Clone, Deserialize)]
struct MetricSeriesQueryRangeArgs {
    database_path: String,
    metric_name: String,
    start_date: String,   // ISO-8601 date "YYYY-MM-DD"
    end_date: String,     // ISO-8601 date "YYYY-MM-DD"
    #[serde(default)]
    source: Option<String>,
}
```

**store.rs query function:**
```rust
pub fn query_metric_series_range(
    &self,
    metric_name: &str,
    start_date: &str,
    end_date: &str,
    source: Option<&str>,
) -> GooseResult<Vec<serde_json::Value>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = if let Some(src) = source {
        conn.prepare(
            "SELECT date, value FROM metric_series
             WHERE metric_name = ?1 AND source = ?2
               AND date >= ?3 AND date <= ?4
             ORDER BY date ASC"
        )?
        // bind: metric_name, src, start_date, end_date
    } else {
        conn.prepare(
            "SELECT date, value FROM metric_series
             WHERE metric_name = ?1 AND date >= ?2 AND date <= ?3
             ORDER BY date ASC"
        )?
    };
    // rows → Vec<json!({date, value})>
}
```

**CRITICAL: `BRIDGE_METHODS` constant is verified by a test** (`tests::bridge_methods_constant_matches_dispatcher`). When adding `metric_series.query_range` to the constant, the dispatcher match arm MUST be added simultaneously or the test fails.

### Pattern 4: GooseRustBridging Protocol

**What:** Protocol matching `GooseRustBridge`'s public API. Only the methods the 2 tests need must be in the protocol initially; the protocol can be expanded later.

```swift
// Source: GooseSwift/GooseRustBridge.swift lines 32-98
// Verified signature:
func request(method: String, args: [String: Any]) throws -> [String: Any]
func requestAsync(method: String, args: [String: Any]) async throws -> [String: Any]
```

**Protocol file:**
```swift
// GooseSwift/GooseRustBridging.swift
protocol GooseRustBridging: AnyObject {
  func request(method: String, args: [String: Any]) throws -> [String: Any]
  func requestAsync(method: String, args: [String: Any]) async throws -> [String: Any]
}
extension GooseRustBridge: GooseRustBridging {}
```

**MockRustBridge** (test mock — records last call for assertion):
```swift
// GooseSwiftTests/MockRustBridge.swift
final class MockRustBridge: GooseRustBridging {
  var lastMethod: String?
  var lastArgs: [String: Any] = [:]
  var stubbedResult: [String: Any] = [:]

  func request(method: String, args: [String: Any] = [:]) throws -> [String: Any] {
    lastMethod = method; lastArgs = args; return stubbedResult
  }
  func requestAsync(method: String, args: [String: Any] = [:]) async throws -> [String: Any] {
    lastMethod = method; lastArgs = args; return stubbedResult
  }
}
```

### Pattern 5: XCTest pattern (existing target)

**What:** `GooseSwiftTests` already exists — confirmed by `project.pbxproj` and directory listing. The target has `BUNDLE_LOADER = "$(TEST_HOST)"` and `TEST_HOST = "$(BUILT_PRODUCTS_DIR)/GooseSwift.app/..."` — this is a hosted test target that can use `@testable import GooseSwift`.

**Existing test pattern** (from `GooseBLETypesTests.swift`):
```swift
import XCTest
@testable import GooseSwift

final class WorkoutEntryTests: XCTestCase {
  func test_submit_calls_workout_upsert() {
    let mock = MockRustBridge()
    // instantiate view model or call submit logic with mock
    XCTAssertEqual(mock.lastMethod, "workout.upsert")
  }
}
```

**To add new test files:** Add Swift file to `GooseSwiftTests/` directory AND add a `PBXBuildFile` + `PBXFileReference` entry in `project.pbxproj` under the `GooseSwiftTests` target's `Sources` build phase.

### Pattern 6: Manual Workout Entry bridge call

**workout.upsert required args** (verified from `WorkoutUpsertArgs` struct in bridge.rs):

| Arg | Type | Required | Notes |
|-----|------|----------|-------|
| `database_path` | String | yes | use `databasePath` |
| `date` | String | yes | ISO-8601 "YYYY-MM-DD" |
| `source` | String | yes | "manual" |
| `sport` | String | yes | `ActivityKind.rawValue` |
| `start_time` | String | yes | ISO-8601 datetime |
| `end_time` | String | yes | ISO-8601 datetime |
| `duration_s` | f64 | yes | minutes × 60 |
| `avg_hr_bpm` | f64? | no | omit for manual entry |
| `max_hr_bpm` | f64? | no | omit for manual entry |
| `strain` | f64? | no | omit for manual entry |
| `calories_kcal` | f64? | no | omit for manual entry |
| `notes` | String? | no | perceived_effort as string or nil |
| `provenance` | JSON object | no (default `{}`) | `{"method": "manual"}` |

**perceived_effort** — the CONTEXT.md specifies Int 1–10; since there is no dedicated field in `WorkoutUpsertArgs`, encode it as `notes: "effort: 7"` or include it in `provenance`. Recommendation: `notes: "perceived_effort: \(effort)"`.

### Anti-Patterns to Avoid

- **Calling GooseRustBridge from @MainActor inline:** `request(method:args:)` is synchronous and blocks the calling thread. Always use `bridge.requestAsync(...)` (which dispatches to a `Task.detached`) from `@MainActor` context. See CLAUDE.md anti-pattern.
- **Creating a new XCTest target:** The `GooseSwiftTests` target already exists. Creating a second target causes duplicate product names and linker errors.
- **Adding `case trends` without updating `HealthRouteContentView`:** `HealthRouteContentView` has a `switch route` with no `default:` arm — the Swift compiler will catch this, but only at build time. Add the case there simultaneously.
- **Adding `metric_series.query_range` to `BRIDGE_METHODS` without a matching dispatcher arm:** A test in `bridge.rs` (`bridge_methods_constant_matches_dispatcher`) verifies that every entry in `BRIDGE_METHODS` has a corresponding `match` arm. The test will fail if the constant and dispatcher are out of sync.
- **Using Charts.framework:** Locked out by project constraint (no external dependencies). Use `SwiftUI Path` polyline as shown in `StressV2TimelineChart`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Sparkline rendering | Custom Canvas drawing system | `SwiftUI Path` polyline (pattern from `StressV2TimelineChart`) | Pattern already established in codebase; x/y normalization helpers already written |
| Stat tiles | New tile component | `SleepV2StatCard` (existing) | Used throughout Recovery/Stress views; takes palette, systemImage, label, value |
| Section headers | New header view | `SleepV2SectionHeader` (existing) | Used in all metric family views |
| Mock bridge state recording | Complex proxy | Simple `var lastMethod: String?` on mock | Sufficient for the 2 required assertions |
| SQLite date range query | Custom SQLite wrapper | `rusqlite` prepared statement in `GooseStore` (same pattern as `insert_metric_series`) | Pattern established; store already owns the connection lock |

---

## Rust bridge.rs: Current State of metric_series

**Confirmed present (Phase 69):**
- `"metric_series.upsert"` — in `BRIDGE_METHODS` (line 245), dispatcher arm (line 2621), `MetricSeriesUpsertArgs` struct, `metric_series_upsert_bridge` function (line 7293)
- `metric_series` table: `(id, source, metric_name, date TEXT, value REAL, UNIQUE(source, metric_name, date))`

**Confirmed absent — must add in Phase 72:**
- `"metric_series.query_range"` — NOT in `BRIDGE_METHODS`, no dispatcher arm, no args struct, no query function in `store.rs`

**`store.rs` `insert_metric_series` signature (for pattern reference):**
```rust
pub fn insert_metric_series(
    &self,
    source: &str,
    metric_name: &str,
    date: &str,
    value: f64,
) -> GooseResult<bool>
```

---

## Common Pitfalls

### Pitfall 1: BRIDGE_METHODS test enforcement
**What goes wrong:** Adding `metric_series.query_range` to the `BRIDGE_METHODS` constant but not the `handle_bridge_request` match, or vice versa — causes `bridge_methods_constant_matches_dispatcher` test to fail with a clear error message listing the mismatched methods.
**Why it happens:** The constant and the match arm are in different locations in `bridge.rs` (constant at line ~183, dispatcher at line ~2613).
**How to avoid:** Add both in the same edit pass. The constant is sorted — `metric_series.query_range` sorts before `metric_series.upsert`.
**Warning signs:** `cargo test bridge_methods_constant_matches_dispatcher` fails.

### Pitfall 2: @MainActor + sync bridge call deadlock
**What goes wrong:** Calling `bridge.request(method:)` (synchronous) directly from a `@MainActor` async context blocks the main thread.
**Why it happens:** `GooseRustBridge.requestValue` is synchronous; `GooseBLEClient.commandCharacteristic` and UI updates share the main thread.
**How to avoid:** Always use `bridge.requestAsync(method:args:)` from `@MainActor` — it internally wraps in `Task.detached(priority: .userInitiated)`.
**Warning signs:** App freezes during bridge call; no crash, just unresponsive UI.

### Pitfall 3: Swift extension stored property restriction
**What goes wrong:** Adding a `@Published` or `var` stored property to a `HealthDataStore` extension — compiler error "extensions must not contain stored properties".
**Why it happens:** `HealthDataStore` is `@Observable final class`; stored properties must be on the main class body.
**How to avoid:** Trends data (the fetched `[{date, value}]` array) must be a stored property on the class itself, not an extension. The CLAUDE.md already documents this pattern (e.g., `sevenDayStrainCache`, `recoveryV1Result`).

### Pitfall 4: HealthRoute.trends missing from HealthRouteContentView switch
**What goes wrong:** Compiler will catch missing enum case in exhaustive switch in `HealthRouteContentView`; build fails.
**How to avoid:** Update the switch in `HealthDashboardViews.swift` at the same time as adding the enum case. Both are non-optional edits for the same plan task.

### Pitfall 5: WorkoutEntry test requires @testable access to view model
**What goes wrong:** If workout submit logic is inside a SwiftUI `View` body or a closure, it cannot be tested without instantiating the full view. This requires a mock dependency injection seam.
**How to avoid:** Extract submit logic into a method on a view model class or a standalone function that accepts a `GooseRustBridging` parameter. The test instantiates the class with `MockRustBridge` and calls the method directly.

---

## Code Examples

### Sparkline Path polyline (from existing codebase pattern)

```swift
// Source: GooseSwift/HealthRecoveryStressViews.swift (StressV2TimelineChart)
// Adapt: replace windows[] with [(date: String, value: Double)]
GeometryReader { proxy in
  Path { path in
    guard points.count > 1 else { return }
    path.move(to: chartPoint(index: 0, size: proxy.size))
    for i in 1..<points.count {
      path.addLine(to: chartPoint(index: i, size: proxy.size))
    }
  }
  .stroke(tintColor, style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
}

private func chartPoint(index: Int, size: CGSize) -> CGPoint {
  let minVal = points.map(\.value).min() ?? 0
  let maxVal = points.map(\.value).max() ?? 1
  let x = size.width * CGFloat(index) / CGFloat(max(points.count - 1, 1))
  let y = size.height * CGFloat(1 - (points[index].value - minVal) / max(maxVal - minVal, 1))
  return CGPoint(x: x, y: y)
}
```

### HealthDataStore bridge call pattern (verified)

```swift
// Source: GooseSwift/HealthDataStore+Snapshots.swift (runPacketScores)
// and GooseSwift/HealthDataStore+Recovery.swift (runRecoveryV1)
func fetchTrendsSeries(metricName: String, days: Int = 7) async throws -> [[String: Any]] {
  let end = ISO8601DateFormatter.localDate(Date())
  let start = ISO8601DateFormatter.localDate(Calendar.current.date(byAdding: .day, value: -days, to: Date())!)
  let result = try await bridge.requestAsync(
    method: "metric_series.query_range",
    args: [
      "database_path": databasePath,
      "metric_name": metricName,
      "start_date": start,
      "end_date": end,
    ]
  )
  return result["rows"] as? [[String: Any]] ?? []
}
```

### workout.upsert call pattern

```swift
// Minimum required args for manual entry
let now = Date()
let startTime = Calendar.current.date(byAdding: .minute, value: -durationMinutes, to: now)!
_ = try await bridge.requestAsync(
  method: "workout.upsert",
  args: [
    "database_path": databasePath,
    "date": ISO8601DateFormatter.localDateString(now),
    "source": "manual",
    "sport": sport.rawValue,
    "start_time": ISO8601DateFormatter.string(from: startTime),
    "end_time": ISO8601DateFormatter.string(from: now),
    "duration_s": Double(durationMinutes) * 60.0,
    "notes": "perceived_effort: \(perceivedEffort)",
  ]
)
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual Path drawing boilerplate | Extract `chartPoint(index:size:)` helper from existing `StressV2TimelineChart` | Phase 72 (new) | Consistent normalization pattern across all sparklines |
| `GooseRustBridge` used directly everywhere | `GooseRustBridging` protocol + mock injection | Phase 72 (new) | Enables unit tests without Rust FFI |
| Test target missing | `GooseSwiftTests` target exists (committed to project.pbxproj) | Earlier phase | New tests only need source file + pbxproj `PBXBuildFile` entry |

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Xcode 26 / XCTest | ARCH-01 tests | confirmed | Xcode 26.5 / Swift 6.3.2 | — |
| cargo test | Rust bridge method | confirmed | Rust MSRV 1.94 (existing) | — |
| SQLite (bundled rusqlite) | metric_series query | confirmed | rusqlite 0.37 bundled | — |

**No missing dependencies.**

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | XCTest (system) |
| Config file | `GooseSwift.xcodeproj` (existing target `GooseSwiftTests`) |
| Quick run command | `xcodebuild test -project GooseSwift.xcodeproj -scheme GooseSwiftTests -destination 'platform=iOS Simulator,name=iPhone 16'` |
| Rust tests | `cargo test --manifest-path Rust/core/Cargo.toml` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DATA-03 | `workout.upsert` is called on submit | unit | xcodebuild test (WorkoutEntryTests) | No — Wave 0 |
| DATA-03 | `metric_series.query_range` is called on fetch | unit | xcodebuild test (TrendsFetchTests) | No — Wave 0 |
| ARCH-01 | `GooseRustBridging` protocol compiles | build | xcodebuild build | No — Wave 0 |
| ARCH-01 | `bridge_methods_constant_matches_dispatcher` | Rust unit | `cargo test bridge_methods_constant_matches_dispatcher` | Yes (existing) |

### Wave 0 Gaps

- [ ] `GooseSwiftTests/WorkoutEntryTests.swift` — covers DATA-03 workout.upsert
- [ ] `GooseSwiftTests/TrendsFetchTests.swift` — covers DATA-03 metric_series.query_range
- [ ] `GooseSwiftTests/MockRustBridge.swift` — shared mock for both tests
- [ ] `GooseSwift/GooseRustBridging.swift` — protocol enabling mock injection
- [ ] `GooseSwift/GooseBLEManaging.swift` — protocol (ARCH-01 completeness)
- [ ] `GooseSwift/HealthDataStoring.swift` — protocol (ARCH-01 completeness)
- [ ] `Rust/core/src/bridge.rs` + `store.rs` — `metric_series.query_range` method

*(Existing infrastructure: `GooseSwiftTests` target + all existing test files cover prior phases)*

---

## Security Domain

> No new authentication, input validation, or cryptographic concerns in this phase. All data flows through the existing `GooseRustBridge` FFI which validates args via Rust's `serde`. The `metric_series.query_range` Rust function uses parameterized `rusqlite` prepared statements — no string interpolation in SQL. `workout.upsert` is existing and already parameterized.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V5 Input Validation | yes (sport string, date strings) | Rust serde Deserialize + existing `metric_name` character validation in `metric_series_upsert_bridge` |
| V2 Authentication | no | No auth in local bridge calls |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `HRVSeriesStore.shared.dailyEstimate()` returns `?.rmssdMS` as a `Double?` | ANS tiles | If field name differs, ANS tile shows "No data" — low risk, easy to fix in code |
| A2 | `HeartRateSeriesStore.shared.restingEstimate()` returns `Double?` | ANS tiles | Same as A1 |
| A3 | perceived_effort encoded as `notes` field is acceptable since no dedicated field exists | Manual Workout | If a dedicated field is added to `WorkoutUpsertArgs` later, notes encoding becomes obsolete — no current risk |
| A4 | New pbxproj entries for test files use the sequential `T*` UUID pattern already in use | XCTest target | If UUID format is wrong, Xcode will reject the project file — verify by opening in Xcode after edit |

---

## Open Questions (RESOLVED)

1. **Where exactly does the "Log Workout" button appear?**
   - RESOLVED: "Log Workout" appears as a dedicated row in `HealthView`'s "Explore Health" section (same pattern as other HealthRoute entries). Tapping presents `ManualWorkoutEntrySheet` as a `.sheet` modal. This is implemented in Plan 72-02 Task 1.

2. **TrendsDashboardView — what does `HealthDataStore.snapshot(for: .trends)` return?**
   - RESOLVED: A minimal stub snapshot with `title: "Trends"`, `value: "7 days"`, `status: "Recovery · HRV · Strain"` is added to `HealthDataStore`'s snapshot switch in Plan 72-02 Task 1. The full trends data is loaded separately by `TrendsDashboardView` via `fetchTrendsSeries`.

---

## Sources

### Primary (HIGH confidence — direct codebase reads)
- `GooseSwift/HealthRecoveryStressViews.swift` — `StressV2OverviewPage` structure confirmed; ANS insertion point at line ~379
- `GooseSwift/HealthModels.swift` — `HealthRoute` enum confirmed; `case trends` absent
- `GooseSwift/AppShellView.swift` — Navigation pattern confirmed; Health tab uses `HealthView`'s own `.navigationDestination`
- `GooseSwift/HealthView.swift` — `HealthRouteShortcutSection` and `.navigationDestination(for: HealthRoute.self)` confirmed
- `GooseSwift/HealthDashboardViews.swift` — `HealthRouteContentView` switch confirmed; `HealthRouteDestinationView` pattern
- `GooseSwift/GooseRustBridge.swift` — `request(method:args:)` and `requestAsync(method:args:)` signatures confirmed
- `Rust/core/src/bridge.rs` — `BRIDGE_METHODS` confirmed; `workout.upsert` present (line 329); `metric_series.query_range` absent; `WorkoutUpsertArgs` struct confirmed (lines 1555–1578); dispatcher pattern confirmed
- `Rust/core/src/store.rs` — `metric_series` table schema confirmed; `insert_metric_series` signature confirmed
- `GooseSwift.xcodeproj/project.pbxproj` — `GooseSwiftTests` target confirmed with `BUNDLE_LOADER`, `TEST_HOST`, `XCTest.framework`
- `GooseSwiftTests/` — 10 existing test files confirmed; `@testable import GooseSwift` pattern confirmed

### Secondary (MEDIUM confidence)
- `GooseSwift/HeartRateSeriesStores.swift` (grep) — `restingEstimate()` and `dailyEstimate()` method names confirmed; exact return types assumed `Double?` and optional struct with `.rmssdMS`

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new packages; all existing
- Architecture: HIGH — codebase fully read; insertion points confirmed
- Rust bridge method: HIGH — BRIDGE_METHODS structure and enforcement test confirmed; query_range absent confirmed
- Pitfalls: HIGH — sourced from direct codebase observation (CLAUDE.md anti-patterns, bridge.rs test enforcement comment)

**Research date:** 2026-06-12
**Valid until:** 2026-07-12 (stable codebase; no fast-moving dependencies)
