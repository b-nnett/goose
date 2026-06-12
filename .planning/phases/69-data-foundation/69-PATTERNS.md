# Phase 69: Data Foundation - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 5 (3 Rust modifications + 1 Swift new + 1 Swift modification)
**Analogs found:** 5 / 5

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `Rust/core/src/store.rs` | model/migration | CRUD | `Rust/core/src/store.rs` lines 1109–1775 (existing arms) | self (exact) |
| `Rust/core/src/storage_check.rs` | utility/validation | CRUD | `Rust/core/src/storage_check.rs` lines 496–1012 | self (exact) |
| `Rust/core/src/bridge.rs` | service/dispatch | request-response | `Rust/core/src/bridge.rs` — `ActivitySessionUpsertArgs` + `activity_create_session_bridge` | exact |
| `GooseSwift/GooseStrainAccumulator.swift` | utility/accumulator | event-driven | `GooseSwift/GooseAppModel.swift` — `onHRSpike` + `Task { @MainActor }` pattern | role-match |
| `GooseSwift/GooseAppModel+ActivityRecording.swift` | service/coordinator | event-driven | self — `beginActivityRecording` / `finishActivityRecording` (lines 50–185) | self (exact) |

---

## Pattern Assignments

### `Rust/core/src/store.rs` — v19→v20 migration arm + `known_tables()` + 4 store methods

**Analog:** Self — existing `migrate()` execute_batch (lines 1109–1775) and `known_tables()` (lines 8730–8773)

**Migration batch pattern** (lines 1109–1113, 1745–1765):
```rust
// GooseStore::migrate() — the entire migration is one execute_batch call
self.conn.execute_batch(
    r#"
    PRAGMA foreign_keys = ON;

    CREATE TABLE IF NOT EXISTS goose_schema_migrations (
        version INTEGER PRIMARY KEY,
        applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );

    -- ... existing 38 CREATE TABLE IF NOT EXISTS blocks ...

    INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (1);
    -- ... through 19 ...
    INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (19);
    PRAGMA user_version = 19;   -- <-- replace with 20; must be last statement
    "#,
)?;
```

**v20 tables to append** (after the last existing `CREATE TABLE IF NOT EXISTS` block, before the `INSERT OR IGNORE VALUES (1)` block):
```sql
CREATE TABLE IF NOT EXISTS journal (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    date           TEXT NOT NULL,
    source         TEXT NOT NULL DEFAULT 'goose',
    behaviors_json TEXT NOT NULL DEFAULT '{}',
    notes          TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(source, date)
);

CREATE TABLE IF NOT EXISTS workout (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_session_id TEXT REFERENCES activity_sessions(session_id) ON DELETE SET NULL,
    date                TEXT NOT NULL,
    source              TEXT NOT NULL,
    sport               TEXT NOT NULL,
    start_time          TEXT NOT NULL,
    end_time            TEXT NOT NULL,
    duration_s          REAL NOT NULL,
    avg_hr_bpm          REAL,
    max_hr_bpm          REAL,
    strain              REAL,
    calories_kcal       REAL,
    distance_m          REAL,
    notes               TEXT,
    provenance_json     TEXT NOT NULL DEFAULT '{}',
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(source, start_time)
);

CREATE TABLE IF NOT EXISTS apple_daily (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    date        TEXT NOT NULL,
    source      TEXT NOT NULL DEFAULT 'healthkit',
    steps       INTEGER,
    active_kcal REAL,
    basal_kcal  REAL,
    avg_hr_bpm  REAL,
    max_hr_bpm  REAL,
    vo2max      REAL,
    weight_kg   REAL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(source, date)
);

CREATE TABLE IF NOT EXISTS metric_series (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source      TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    date        TEXT NOT NULL,
    value       REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(source, metric_name, date)
);
```

Then add migration seed and bump PRAGMA:
```sql
INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (20);
PRAGMA user_version = 20;   -- replaces the existing "PRAGMA user_version = 19;" at line 1764
```

**`CURRENT_SCHEMA_VERSION` bump** (line 14):
```rust
// Before:
pub const CURRENT_SCHEMA_VERSION: i64 = 19;
// After:
pub const CURRENT_SCHEMA_VERSION: i64 = 20;
```

**`known_tables()` addition** (lines 8730–8773 — append 4 entries before the closing `]`):
```rust
pub fn known_tables() -> &'static [&'static str] {
    &[
        // ... existing 39 entries ...
        "upload_cursors",
        // v20 additions:
        "journal",
        "workout",
        "apple_daily",
        "metric_series",
    ]
}
```

**Store insert method pattern** — copy from any existing simple INSERT method; use `INSERT OR REPLACE` for `journal`, `workout`, `apple_daily` (to allow re-writes); use `INSERT OR IGNORE` for `metric_series` (append-only):
```rust
// Pattern: INSERT OR REPLACE for updatable tables (journal / workout / apple_daily)
pub fn insert_journal(&self, args: &JournalUpsertArgs) -> GooseResult<bool> {
    let rows = self.conn.execute(
        "INSERT OR REPLACE INTO journal (date, source, behaviors_json, notes)
         VALUES (?1, ?2, ?3, ?4)",
        params![args.date, args.source, args.behaviors_json, args.notes],
    )?;
    Ok(rows > 0)
}

// Pattern: INSERT OR IGNORE for metric_series (locked by CONTEXT.md)
pub fn insert_metric_series(&self, args: &MetricSeriesUpsertArgs) -> GooseResult<bool> {
    let rows = self.conn.execute(
        "INSERT OR IGNORE INTO metric_series (source, metric_name, date, value)
         VALUES (?1, ?2, ?3, ?4)",
        params![args.source, args.metric_name, args.date, args.value],
    )?;
    Ok(rows > 0)
}
```

---

### `Rust/core/src/storage_check.rs` — `required_columns()` additions

**Analog:** Self — `required_columns()` (lines 496–1012)

**Pattern for each new table** (append before `for table in known_tables()` loop at line 1008):
```rust
// Copy the verbatim style of existing entries — one columns.insert() per table
columns.insert("journal",        vec!["id", "date", "source", "behaviors_json", "created_at"]);
columns.insert("workout",        vec!["id", "date", "source", "sport", "start_time", "end_time", "duration_s"]);
columns.insert("apple_daily",    vec!["id", "date", "source", "created_at"]);
columns.insert("metric_series",  vec!["id", "source", "metric_name", "date", "value", "created_at"]);
```

The `debug_assert!` loop at line 1008–1009 enforces that every entry in `known_tables()` has a `required_columns()` entry — the build will panic in debug mode if these are out of sync.

---

### `Rust/core/src/bridge.rs` — 4 upsert dispatch arms + args structs + bridge functions

**Analog:** `Rust/core/src/bridge.rs` — `ActivitySessionUpsertArgs` (lines 1519–1538) + `activity_create_session_bridge` (lines 7145–7173) + dispatch arm (lines 2538–2541)

**Args struct pattern** (copy `#[derive(Debug, Clone, Deserialize)]` + `database_path: String` as first field):
```rust
#[derive(Debug, Clone, Deserialize)]
struct JournalUpsertArgs {
    database_path: String,
    date: String,
    source: String,
    behaviors_json: String,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkoutUpsertArgs {
    database_path: String,
    date: String,
    source: String,
    sport: String,
    start_time: String,
    end_time: String,
    duration_s: f64,
    #[serde(default)]
    activity_session_id: Option<String>,
    #[serde(default)]
    avg_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    strain: Option<f64>,
    #[serde(default)]
    calories_kcal: Option<f64>,
    #[serde(default)]
    distance_m: Option<f64>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct AppleDailyUpsertArgs {
    database_path: String,
    date: String,
    source: String,
    #[serde(default)]
    steps: Option<i64>,
    #[serde(default)]
    active_kcal: Option<f64>,
    #[serde(default)]
    basal_kcal: Option<f64>,
    #[serde(default)]
    avg_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    vo2max: Option<f64>,
    #[serde(default)]
    weight_kg: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetricSeriesUpsertArgs {
    database_path: String,
    source: String,
    metric_name: String,
    date: String,
    value: f64,
}
```

**Dispatch arm pattern** (lines 2538–2541 — copy exactly, change method name and types):
```rust
"journal.upsert" => request_args::<JournalUpsertArgs>(&request)
    .and_then(journal_upsert_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
"workout.upsert" => request_args::<WorkoutUpsertArgs>(&request)
    .and_then(workout_upsert_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
"apple_daily.upsert" => request_args::<AppleDailyUpsertArgs>(&request)
    .and_then(apple_daily_upsert_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
"metric_series.upsert" => request_args::<MetricSeriesUpsertArgs>(&request)
    .and_then(metric_series_upsert_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
```

**Bridge function pattern** (lines 7145–7173 — simpler variant without read-back):
```rust
fn journal_upsert_bridge(args: JournalUpsertArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let inserted = store.insert_journal(&args)?;
    Ok(json!({
        "schema": "goose.journal-upsert-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
    }))
}
// Repeat for workout_upsert_bridge, apple_daily_upsert_bridge, metric_series_upsert_bridge
```

---

### `GooseSwift/GooseStrainAccumulator.swift` (new file — Swift actor)

**Analog:** `GooseSwift/GooseAppModel.swift` lines 308–320 — `onLiveHeartRate` closure + `onHRSpike` Task @MainActor pattern. No existing `actor` type in codebase — this is the first. Class structure reference: `GooseBLEBondingManager.swift` (callback manager pattern).

**File header / imports pattern** — no framework imports needed (pure Swift stdlib):
```swift
// No imports needed; Date and TimeInterval are in Swift stdlib (Foundation-free for actors)
import Foundation  // needed for Date
```

**Actor declaration pattern** (new to codebase — use Swift `actor` keyword per CONTEXT.md decision):
```swift
actor GooseStrainAccumulator {
  // All stored properties are automatically actor-isolated
  private var accumulatedLoad: Double = 0
  private var lastSampleDate: Date?
  private var lastPublishedAt: Date = .distantPast
  private var isFrozen: Bool = false

  static let defaultMaxHR: Double = 190
  static let publishInterval: TimeInterval = 3
  static let maxSampleGap: TimeInterval = 30  // ignore samples with gap >30s

  private var maxHR: Double = GooseStrainAccumulator.defaultMaxHR

  // Hook for Phase 72 to supply actual HRmax from user profile
  func setMaxHR(_ bpm: Double) {
    maxHR = bpm
  }

  func ingest(bpm: Int, date: Date) {
    guard !isFrozen else { return }
    guard let last = lastSampleDate else {
      lastSampleDate = date
      return
    }
    let interval = date.timeIntervalSince(last)
    guard interval > 0, interval < GooseStrainAccumulator.maxSampleGap else {
      lastSampleDate = date
      return
    }
    lastSampleDate = date
    let hrPct = Double(bpm) / maxHR
    let multiplier: Double
    switch hrPct {
    case ..<0.60: multiplier = 1.0
    case 0.60..<0.70: multiplier = 2.0
    case 0.70..<0.80: multiplier = 3.0
    case 0.80..<0.90: multiplier = 4.0
    default: multiplier = 5.0
    }
    accumulatedLoad += multiplier * interval
  }

  func reset() {
    accumulatedLoad = 0
    lastSampleDate = nil
    lastPublishedAt = .distantPast
    isFrozen = false
  }

  func freeze() {
    isFrozen = true
  }

  // Returns current load only when throttle window has elapsed; nil otherwise
  func pollIfReady(now: Date) -> Double? {
    guard now.timeIntervalSince(lastPublishedAt) >= GooseStrainAccumulator.publishInterval else {
      return nil
    }
    lastPublishedAt = now
    return accumulatedLoad
  }
}
```

**onHRSpike Task @MainActor pattern** (lines 319–320) — exact model for actor call + MainActor publish:
```swift
// Existing pattern to copy:
ble.onHRSpike = { [weak self] _, _ in
  Task { @MainActor in self?.hrSpikeCount += 1 }
}

// New accumulator pattern follows same structure:
ble.onLiveHeartRate = { [weak self] bpm, source, capturedAt in
  heartRateSamplePipeline.recordHeartRateSample(bpm: bpm, source: source, capturedAt: capturedAt)
  // NEW:
  Task { [weak self] in
    guard let self else { return }
    guard self.activeActivityPersistence != nil else { return }
    await self.strainAccumulator.ingest(bpm: bpm, date: capturedAt)
    if let load = await self.strainAccumulator.pollIfReady(now: capturedAt) {
      Task { @MainActor [weak self] in self?.liveWorkoutStrain = load }
    }
  }
}
```

---

### `GooseSwift/GooseAppModel+ActivityRecording.swift` — reset/freeze hooks

**Analog:** Self — `beginActivityRecording` (line 50) and `finishActivityRecording` (line 165)

**New @Published property on GooseAppModel** — add alongside existing `@Published` properties:
```swift
// In GooseAppModel.swift, with other @Published properties:
@Published private(set) var liveWorkoutStrain: Double = 0
```

**strainAccumulator instance** — add as stored property on GooseAppModel (same pattern as `heartRateSamplePipeline`, `ble`, etc.):
```swift
private let strainAccumulator = GooseStrainAccumulator()
```

**beginActivityRecording hook** (after line 77 — after `activityPersistenceStatus` is set):
```swift
// Reset accumulator when workout begins
Task { await strainAccumulator.reset() }
```

**finishActivityRecording hook** (after line 185 — after `activeActivityPersistence = nil`):
```swift
// Freeze accumulator; zero published strain
Task { await strainAccumulator.freeze() }
Task { @MainActor in self.liveWorkoutStrain = 0 }
```

---

## Shared Patterns

### Rust bridge dispatch arm (cross-cutting — all 4 bridge methods)
**Source:** `Rust/core/src/bridge.rs` lines 2538–2541
**Apply to:** All 4 new bridge functions
```rust
"method.name" => request_args::<ArgsType>(&request)
    .and_then(bridge_fn)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
```

### Task @MainActor publication (cross-cutting — GooseAppModel)
**Source:** `GooseSwift/GooseAppModel.swift` line 320
**Apply to:** `GooseStrainAccumulator` publish path and any accumulator result delivered from a non-MainActor context
```swift
Task { @MainActor [weak self] in self?.somePublishedProp = value }
```

### `#[serde(default)]` on optional bridge args (cross-cutting — all args structs)
**Source:** `Rust/core/src/bridge.rs` lines 1527–1532
**Apply to:** All `Option<T>` fields in the 4 new args structs — required so callers can omit optional keys without deserialisation error
```rust
#[serde(default)]
notes: Option<String>,
```

---

## No Analog Found

None. All files have strong existing analogs in the codebase.

---

## Metadata

**Analog search scope:** `Rust/core/src/`, `GooseSwift/`
**Files scanned:** store.rs, storage_check.rs, bridge.rs, GooseAppModel.swift, GooseAppModel+ActivityRecording.swift
**Pattern extraction date:** 2026-06-12
