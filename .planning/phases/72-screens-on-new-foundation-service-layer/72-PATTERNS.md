# Phase 72: Screens on New Foundation + Service Layer - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 16
**Analogs found:** 15 / 16

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `Rust/core/src/bridge.rs` | bridge dispatcher | request-response | `bridge.rs` `metric_series.upsert` arm (line 2621) | exact |
| `Rust/core/src/store.rs` | data access | CRUD | `store.rs` `insert_metric_series` (line 7025) | exact |
| `Rust/core/tests/` (new test) | test | batch | `store.rs` `test_metric_series_unique_constraint` (line 9710) | role-match |
| `GooseSwift/HealthRecoveryStressViews.swift` | view (modify) | request-response | same file lines 352–381 (stat card + section header pattern) | exact |
| `GooseSwift/TrendsDashboardView.swift` | view | request-response | `GooseSwift/HealthDashboardViews.swift` `HealthRouteContentView` + `StressV2TimelineSection` | role-match |
| `GooseSwift/ManualWorkoutEntrySheet.swift` | view + submit logic | request-response | `GooseSwiftTests/GooseUploadServiceTests.swift` + bridge call pattern in `HealthDataStore+Snapshots.swift` | partial |
| `GooseSwift/HealthModels.swift` | model (modify) | — | same file lines 6–51 (`HealthRoute` enum) | exact |
| `GooseSwift/AppShellView.swift` | view (modify) | — | `GooseSwift/HealthDashboardViews.swift` lines 344–368 | exact |
| `GooseSwift/HealthView.swift` | view (modify) | — | same file (existing `HealthRouteShortcutSection` call) | exact |
| `GooseSwift/GooseBLEManaging.swift` | protocol | — | `GooseSwift/GooseRustBridge.swift` lines 1–8 (error enum as companion) | partial |
| `GooseSwift/GooseRustBridging.swift` | protocol | — | `GooseSwift/GooseRustBridge.swift` lines 22–97 | exact |
| `GooseSwift/HealthDataStoring.swift` | protocol | — | `GooseSwift/HealthDataStore+Snapshots.swift` method signatures | role-match |
| `GooseSwiftTests/MockBLEClient.swift` | mock | — | `GooseSwiftTests/GooseUploadServiceTests.swift` minimal class pattern | role-match |
| `GooseSwiftTests/MockRustBridge.swift` | mock | — | `GooseSwiftTests/GooseUploadServiceTests.swift` + `GooseRustBridge` signatures | role-match |
| `GooseSwiftTests/WorkoutEntryTests.swift` | test | request-response | `GooseSwiftTests/GooseUploadServiceTests.swift` lines 1–47 | exact |
| `GooseSwiftTests/TrendsFetchTests.swift` | test | request-response | `GooseSwiftTests/GooseUploadServiceTests.swift` lines 1–20 | exact |

---

## Pattern Assignments

### `Rust/core/src/bridge.rs` — add `metric_series.query_range` (bridge dispatcher, request-response)

**Analog:** `Rust/core/src/bridge.rs` — `metric_series.upsert` entry

**BRIDGE_METHODS insertion** (line 245 — `query_range` sorts before `upsert`):
```rust
// Source: Rust/core/src/bridge.rs lines 244-246
"metric_series.query_range",   // INSERT HERE — q < u alphabetically
"metric_series.upsert",
"metrics.activity_unavailable_daily_status",
```

**Args struct pattern** (copy from `MetricSeriesUpsertArgs` at lines 1602-1609):
```rust
// Source: Rust/core/src/bridge.rs lines 1602-1609
#[derive(Debug, Clone, Deserialize)]
struct MetricSeriesUpsertArgs {
    database_path: String,
    source: String,
    metric_name: String,
    date: String,
    value: f64,
}
// New struct — same derive, same database_path field, different query fields:
#[derive(Debug, Clone, Deserialize)]
struct MetricSeriesQueryRangeArgs {
    database_path: String,
    metric_name: String,
    start_date: String,
    end_date: String,
    #[serde(default)]
    source: Option<String>,
}
```

**Dispatcher arm pattern** (copy structure from lines 2621-2624):
```rust
// Source: Rust/core/src/bridge.rs lines 2621-2624
"metric_series.upsert" => request_args::<MetricSeriesUpsertArgs>(&request)
    .and_then(metric_series_upsert_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
// New arm — identical structure:
"metric_series.query_range" => request_args::<MetricSeriesQueryRangeArgs>(&request)
    .and_then(metric_series_query_range_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
```

**Bridge function pattern** (copy from `metric_series_upsert_bridge` at lines 7293-7318):
```rust
// Source: Rust/core/src/bridge.rs lines 7293-7318
fn metric_series_upsert_bridge(args: MetricSeriesUpsertArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let inserted = store.insert_metric_series(...)?;
    Ok(json!({
        "schema": "goose.metric-series-upsert-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
    }))
}
// New function — same open_bridge_store + store method call + json! return:
fn metric_series_query_range_bridge(args: MetricSeriesQueryRangeArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let rows = store.query_metric_series_range(
        &args.metric_name,
        &args.start_date,
        &args.end_date,
        args.source.as_deref(),
    )?;
    Ok(json!({
        "schema": "goose.metric-series-query-range-result.v1",
        "generated_by": "goose-bridge",
        "rows": rows,
    }))
}
```

**CRITICAL:** `BRIDGE_METHODS` is enforced by test `bridge_methods_constant_matches_dispatcher`. The constant entry and the dispatcher arm must be added in the same edit. Test: `cargo test bridge_methods_constant_matches_dispatcher`.

---

### `Rust/core/src/store.rs` — add `query_metric_series_range` (data access, CRUD)

**Analog:** `Rust/core/src/store.rs` `insert_metric_series` (lines 7025-7038)

**Pattern** (copy connection lock + prepare + params approach):
```rust
// Source: Rust/core/src/store.rs lines 7025-7038
pub fn insert_metric_series(
    &self,
    source: &str,
    metric_name: &str,
    date: &str,
    value: f64,
) -> GooseResult<bool> {
    let rows = self.conn.execute(
        "INSERT OR IGNORE INTO metric_series (source, metric_name, date, value)
         VALUES (?1, ?2, ?3, ?4)",
        params![source, metric_name, date, value],
    )?;
    Ok(rows > 0)
}
// New function — same &self receiver, same GooseResult return, parameterized query:
pub fn query_metric_series_range(
    &self,
    metric_name: &str,
    start_date: &str,
    end_date: &str,
    source: Option<&str>,
) -> GooseResult<Vec<serde_json::Value>> {
    // Use self.conn.prepare() + .query_map() + params![] — same as other between/range queries
    // Return Vec<json!({"date": row.date, "value": row.value})>
}
```

**Range query analog** (see `daily_activity_metrics_between` at store.rs line ~3446 for the prepare+query_map pattern).

---

### `Rust/core/tests/` — new test for `metric_series.query_range` (test, batch)

**Analog:** `Rust/core/src/store.rs` test at lines 9710-9730

**Test structure pattern**:
```rust
// Source: Rust/core/src/store.rs lines 9710-9729
#[test]
fn test_metric_series_unique_constraint() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.sqlite");
    let store = GooseStore::open(db.to_str().unwrap()).unwrap();
    store.conn.execute_batch("INSERT OR IGNORE INTO metric_series ...").unwrap();
    // assert via SELECT COUNT(*)
}
// New test — same tempdir + GooseStore::open + insert seed rows + call query_metric_series_range + assert Vec len
```

---

### `GooseSwift/HealthRecoveryStressViews.swift` — add ANS section (view, modify)

**Analog:** Same file, lines 352-381 (existing stat card + section header pattern in `StressV2OverviewPage.body`)

**Exact insertion point** (after line 379, before the existing `SleepV2SectionHeader(title: "Trends", ...)`):
```swift
// Source: GooseSwift/HealthRecoveryStressViews.swift lines 377-381
SleepV2SectionHeader(title: "Breakdown", palette: palette)

StressV2BreakdownSection(palette: palette, summary: summary)

// INSERT ANS SECTION HERE

SleepV2SectionHeader(title: "Trends", palette: palette)
```

**Stat card pattern to copy** (lines 352-367):
```swift
// Source: GooseSwift/HealthRecoveryStressViews.swift lines 352-367
VStack(alignment: .leading, spacing: 14) {
  HStack(spacing: 12) {
    SleepV2StatCard(
      palette: palette,
      systemImage: "checkmark.seal.fill",
      label: "Confidence",
      value: stressConfidenceText
    )
    SleepV2StatCard(
      palette: palette,
      systemImage: "heart.fill",
      label: "Average HR",
      value: averageHeartRateText
    )
  }
  .frame(height: 96)
```

**ANS data sources** (no bridge call — in-process stores):
```swift
// Source: GooseSwift/HeartRateSeriesStores.swift (confirmed by grep)
let rmssd: Double? = HRVSeriesStore.shared.dailyEstimate()?.rmssdMS
let rhr: Double?   = HeartRateSeriesStore.shared.restingEstimate()
```

---

### `GooseSwift/TrendsDashboardView.swift` — new Trends screen (view, request-response)

**Analog:** `GooseSwift/HealthDashboardViews.swift` (destination view pattern, lines 344-368) + sparkline from `HealthRecoveryStressViews.swift` `StressV2TimelineSection`

**Bridge call pattern** (copy from `HealthDataStore+Snapshots.swift` lines 36-62):
```swift
// Source: GooseSwift/HealthDataStore+Snapshots.swift lines 36-62
let sleepReport = try await bridge.requestAsync(
  method: "metrics.sleep_score_from_features",
  args: sleepArgs
)
// Equivalent for Trends:
let rows = try await bridge.requestAsync(
  method: "metric_series.query_range",
  args: [
    "database_path": databasePath,
    "metric_name": metricName,
    "start_date": start,
    "end_date": end,
  ]
)
```

**Sparkline Path pattern** (from RESEARCH.md, verified against StressV2TimelineChart):
```swift
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

**CRITICAL anti-pattern:** Do NOT call `bridge.request(method:)` (synchronous) from `@MainActor`. Always use `bridge.requestAsync(method:args:)` — see CLAUDE.md anti-patterns section.

**CRITICAL stored property rule:** Trends data arrays (fetched `[{date, value}]`) must be stored properties on the class body, NOT in extensions. Extensions cannot contain stored properties.

---

### `GooseSwift/ManualWorkoutEntrySheet.swift` — new sheet modal (view, request-response)

**Analog:** Bridge call pattern from `HealthDataStore+Snapshots.swift` lines 36-62; sheet dismiss pattern from standard SwiftUI

**workout.upsert bridge call** (verified args from `WorkoutUpsertArgs` struct at bridge.rs lines 1555-1579):
```swift
// Required args (source: bridge.rs lines 1555-1579, WorkoutUpsertArgs struct)
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

**Submit logic extraction requirement:** Submit logic must live in a method on a view model / standalone function accepting `GooseRustBridging` — NOT inside SwiftUI `View.body`. This is required for `WorkoutEntryTests` to inject `MockRustBridge` and assert without instantiating the full view.

---

### `GooseSwift/HealthModels.swift` — add `case trends` (model, modify)

**Analog:** Same file, lines 6-51 (`HealthRoute` enum)

**Enum extension pattern** (copy existing case structure exactly):
```swift
// Source: GooseSwift/HealthModels.swift lines 6-51
enum HealthRoute: String, CaseIterable, Identifiable, Hashable {
  case healthMonitor
  case sleep
  // ... existing cases ...
  case calibration
  case trends        // ADD — after existing cases

  var title: String {
    switch self {
    // existing ...
    case .trends: "Trends"    // ADD
    }
  }

  var systemImage: String {
    switch self {
    // existing ...
    case .trends: "chart.line.uptrend.xyaxis"    // ADD
    }
  }
  // Also update deepLinkPath and any other exhaustive switch
}
```

---

### `GooseSwift/AppShellView.swift` — add `case .trends` navigation (view, modify)

**Analog:** `GooseSwift/HealthDashboardViews.swift` lines 344-368 (`HealthRouteContentView` switch)

**Switch arm pattern to copy**:
```swift
// Source: GooseSwift/HealthDashboardViews.swift lines 349-368
switch route {
case .healthMonitor:
  HealthMonitorView(store: store)
case .sleep, .recovery, .strain, .stress:
  HealthMetricFamilyView(route: route, store: store, externalSelectedDate: selectedDate)
case .cardioLoad:
  CardioLoadView(store: store)
// ... other cases ...
case .calibration:
  CalibrationHealthView(store: store)
// ADD:
case .trends:
  TrendsDashboardView(store: store)
}
```

Note: The switch is in `HealthRouteContentView` in `HealthDashboardViews.swift` (line 350), not in `AppShellView.swift`. Update `HealthDashboardViews.swift` — the compiler will catch any missed case (no `default:` arm).

---

### `GooseSwift/GooseRustBridging.swift` — new protocol file (protocol)

**Analog:** `GooseSwift/GooseRustBridge.swift` lines 22-97 (concrete class to extract protocol from)

**Imports pattern** (line 1):
```swift
// Source: GooseSwift/GooseRustBridge.swift line 1
import Foundation
```

**Protocol definition** (extracted from lines 32-97):
```swift
// Source: GooseSwift/GooseRustBridge.swift lines 32-97
// Confirmed signatures:
func request(method: String, args: [String: Any] = [:]) throws -> [String: Any]      // line 32
func requestAsync(method: String, args: [String: Any] = [:]) async throws -> [String: Any]  // line 96

// Protocol + retroactive conformance:
protocol GooseRustBridging: AnyObject {
  func request(method: String, args: [String: Any]) throws -> [String: Any]
  func requestAsync(method: String, args: [String: Any]) async throws -> [String: Any]
}
extension GooseRustBridge: GooseRustBridging {}
```

---

### `GooseSwift/GooseBLEManaging.swift` — new protocol file (protocol)

**Analog:** `GooseSwift/GooseBLEClient.swift` (concrete class — extract public API used by `GooseAppModel`)

**File structure pattern** (copy from `GooseRustBridging.swift` above — same 1-file-1-protocol pattern):
```swift
import Foundation
import CoreBluetooth

protocol GooseBLEManaging: AnyObject {
  // Extract only the properties/methods referenced by the 2 tests
  // Minimal conformance: expand in a later phase
}
extension GooseBLEClient: GooseBLEManaging {}
```

---

### `GooseSwift/HealthDataStoring.swift` — new protocol file (protocol)

**Analog:** `GooseSwift/HealthDataStore+Snapshots.swift` method signatures (bridge call methods)

**File structure** (same pattern as `GooseRustBridging.swift`):
```swift
import Foundation

protocol HealthDataStoring: AnyObject {
  // Extract only properties/methods referenced by TrendsFetchTests
  // Minimal conformance; expand later
}
extension HealthDataStore: HealthDataStoring {}
```

---

### `GooseSwiftTests/MockRustBridge.swift` — mock (mock)

**Analog:** `GooseSwiftTests/GooseUploadServiceTests.swift` lines 1-9 (minimal test class pattern) + `GooseRustBridge` signatures (lines 32-97)

**Import pattern** (copy from `GooseUploadServiceTests.swift` lines 1-3):
```swift
// Source: GooseSwiftTests/GooseUploadServiceTests.swift lines 1-3
import XCTest
@testable import GooseSwift
```

**Mock recording pattern** (assertion mechanism — from RESEARCH.md Pattern 4):
```swift
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

---

### `GooseSwiftTests/WorkoutEntryTests.swift` and `TrendsFetchTests.swift` — tests (test, request-response)

**Analog:** `GooseSwiftTests/GooseUploadServiceTests.swift` lines 1-61 (exact test structure)

**Test file pattern** (copy exactly):
```swift
// Source: GooseSwiftTests/GooseUploadServiceTests.swift lines 1-10
import XCTest
@testable import GooseSwift

final class WorkoutEntryTests: XCTestCase {

  func test_submit_calls_workout_upsert() {
    // instantiate view model / submit function with MockRustBridge
    let mock = MockRustBridge()
    // call submit logic
    XCTAssertEqual(mock.lastMethod, "workout.upsert")
  }
}
```

**Test method naming convention** (from `GooseUploadServiceTests.swift`):
```swift
// Source: GooseSwiftTests/GooseUploadServiceTests.swift lines 13, 25, 35
func test_buildUploadPayload_gen4_hasGeneration4_noDeviceClass()
func test_buildUploadPayload_gen5_goose_hasGeneration5_noDeviceClass()
// Pattern: test_<unit>_<condition>_<expected>
```

**pbxproj requirement:** Each new test file needs a `PBXBuildFile` + `PBXFileReference` entry under the `GooseSwiftTests` target's Sources build phase in `GooseSwift.xcodeproj/project.pbxproj`. Use the UUID pattern already in the file.

---

## Shared Patterns

### Bridge call from Swift (async, non-blocking)
**Source:** `GooseSwift/HealthDataStore+Snapshots.swift` lines 36-62
**Apply to:** `TrendsDashboardView`, `ManualWorkoutEntrySheet` (any file calling Rust)
```swift
let result = try await bridge.requestAsync(
  method: "...",
  args: ["database_path": databasePath, ...]
)
```
Never use `bridge.request(method:)` from `@MainActor` — it is synchronous and blocks the main thread.

### Error handling in HealthDataStore extensions
**Source:** `GooseSwift/HealthDataStore+Snapshots.swift` lines 58-62
**Apply to:** Any new `HealthDataStore` extension methods
```swift
} catch {
  let shortErr = Self.shortError(error)
  self.someStatus = "Operation failed: \(shortErr)"
}
```

### SleepV2StatCard tile pair
**Source:** `GooseSwift/HealthRecoveryStressViews.swift` lines 352-367
**Apply to:** ANS Balance section in `HealthRecoveryStressViews.swift`
```swift
HStack(spacing: 12) {
  SleepV2StatCard(palette: palette, systemImage: "...", label: "...", value: ...)
  SleepV2StatCard(palette: palette, systemImage: "...", label: "...", value: ...)
}
.frame(height: 96)
```

### SleepV2SectionHeader
**Source:** `GooseSwift/HealthRecoveryStressViews.swift` line 373, 377, 381
**Apply to:** ANS section header in `HealthRecoveryStressViews.swift`
```swift
SleepV2SectionHeader(title: "ANS Balance", palette: palette)
```

### Rust store query pattern
**Source:** `Rust/core/src/store.rs` line 7025-7038
**Apply to:** `query_metric_series_range` in `store.rs`
```rust
pub fn insert_metric_series(&self, ...) -> GooseResult<bool> {
    let rows = self.conn.execute("...", params![...])?;
    Ok(rows > 0)
}
```
For SELECT queries use `self.conn.prepare(...)?.query_map(params![...], |row| { ... })` — see any `*_between` method.

---

## No Analog Found

All files have sufficient analogs from the codebase. No files require falling back to RESEARCH.md-only patterns.

---

## Metadata

**Analog search scope:** `GooseSwift/`, `GooseSwiftTests/`, `Rust/core/src/`
**Files scanned:** ~18 source files read or grepped
**Pattern extraction date:** 2026-06-12
