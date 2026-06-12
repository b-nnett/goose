# Phase 69: Data Foundation - Research

**Researched:** 2026-06-12
**Domain:** SQLite schema migration (Rust) + Swift actor for realtime strain accumulation
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Schema Design (DATA-01)
- **Timestamps**: Use `date TEXT` (ISO8601) across all 4 tables — consistent with existing codebase patterns
- **metricSeries idempotency**: `UNIQUE(source, metric_name, date)` + `INSERT OR IGNORE` pattern
- **journal**: Store behaviours as a JSON blob (`behaviors_json TEXT`) — avoids schema lock-in for evolving behaviour definitions
- **workout**: Separate `workout` table with `activity_session_id FK` to existing `activity_sessions` — no modification of existing tables

#### Migration
- **Single arm**: One v19→v20 migration arm creates all 4 tables together — simpler than 4 separate arms
- **Idempotency**: `CREATE TABLE IF NOT EXISTS` + seed `INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (20)` — running twice produces no error

#### Bridge API
- **Minimum CRUD**: Expose 4 upsert bridge methods only: `journal.upsert`, `workout.upsert`, `apple_daily.upsert`, `metric_series.upsert`
- **No query_range**: Query methods deferred — Phase 72 screens will need them but are not in scope here

#### GooseStrainAccumulator (DATA-02)
- **Type**: Swift `actor` (automatic isolation, no manual @MainActor decoration needed on internal state)
- **Throttle**: ≤1 published update per 3 seconds (accumulator tracks last-publish time internally)
- **HR source**: `WhoopDataSignalPipeline` via existing `onSignalSample` callback pattern — not a new BLE subscriber
- **Publication target**: `@Published var liveWorkoutStrain: Double` on `GooseAppModel` via `Task { @MainActor in ... }`
- **Strain formula**: Edwards Zone Load (HR zones × duration in seconds) computed entirely Swift-side — does NOT call into Rust bridge (latency concern for realtime updates)

#### Testing
- Migration idempotency test: run v19→v20 arm twice; assert no error and table count unchanged
- No round-trip CRUD tests required for this phase (deferred to Phase 72)

### Claude's Discretion
- Exact column names for journal/workout/appleDaily tables (beyond the constraints above)
- Whether GooseStrainAccumulator receives a `maxHR` from GooseAppModel or uses a default (220 - age)
- Implementation details of Edwards Zone Load (zone boundaries, coefficients)

### Deferred Ideas (OUT OF SCOPE)
- `*.query_range` bridge methods for each new table — needed by Phase 72 screens
- Historical strain backfill from existing `activity_sessions` data
- UI screens consuming the new tables (Phase 72)
- Round-trip CRUD tests per table (Phase 72 or dedicated test phase)
- GooseStrainAccumulator publishing to a dedicated Swift struct instead of a raw Double
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DATA-01 | App persiste diário de comportamentos (Y/N diários), log de treino com sport tag, dados Apple Health diários, e séries de métricas genéricas em SQLite (schema v20 — 4 tabelas com migration arm condicional) | Migration pattern verified in store.rs; idempotency via CREATE TABLE IF NOT EXISTS + INSERT OR IGNORE confirmed; `known_tables()` function location confirmed; bridge upsert pattern verified |
| DATA-02 | Ecrã de workout mostra strain acumulado em tempo real durante sessão activa (GooseStrainAccumulator Swift-side; publica via Task @MainActor) | HR sample flow verified: ble.onLiveHeartRate → GooseAppModel callback; WhoopDataSignalPipeline.ingest() entry point confirmed; activeActivityPersistence lifecycle confirmed; PassiveActivityDetectionEvent.finished is the workout-end signal |
</phase_requirements>

## Summary

Phase 69 has two fully independent deliverables joined only by the workout session concept. DATA-01 is pure Rust: bump `CURRENT_SCHEMA_VERSION` from 19 to 20 in `store.rs`, add one `execute_batch` block creating 4 tables with `CREATE TABLE IF NOT EXISTS`, seed migration row 20 with `INSERT OR IGNORE`, bump `PRAGMA user_version = 20`, and add 4 bridge dispatch arms + corresponding store methods. DATA-02 is pure Swift: a new `GooseStrainAccumulator` actor wired between `GooseBLEClient.onLiveHeartRate` (via `GooseAppModel`) and `GooseAppModel.liveWorkoutStrain: Double`.

The migration pattern in this codebase is fully established and verified: all tables are created in a single `execute_batch` with `CREATE TABLE IF NOT EXISTS`, migration rows are seeded with `INSERT OR IGNORE`, and conditional column additions use separate `ensure_*` helper functions. The v19→v20 arm follows this exact pattern with one important addition: `PRAGMA user_version = 20` must be included at the end of the batch.

The HR sample path for DATA-02 is: `GooseBLEClient.recordLiveHeartRate` → `realtimeVitalsQueue.async` → `processLiveHeartRate` → `onLiveHeartRate?(bpm, source, date)` callback → `GooseAppModel` closure → `heartRateSamplePipeline.recordHeartRateSample`. The accumulator must hook in at the `onLiveHeartRate` callback level, not inside `WhoopDataSignalPipeline` (which only handles K-packet types, not the standard HR GATT characteristic). The `WhoopDataSignalPipeline` mentioned in the CONTEXT.md as "HR source" refers to the fact that the same `onLiveHeartRate` callback also fires for R22-sourced HR — so hooking `onLiveHeartRate` covers all BLE HR sources.

**Primary recommendation:** Wire `GooseStrainAccumulator` into the existing `ble.onLiveHeartRate` closure in `GooseAppModel.init()` alongside `heartRateSamplePipeline`; reset on `activeActivityPersistence` becoming nil (workout end detected via `PassiveActivityDetectionEvent.finished` or `finishActivityRecording`).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| SQLite schema migration | Rust core (bridge) | — | All schema state lives in `GooseStore`; Swift never touches SQLite directly |
| Bridge upsert methods | Rust core (bridge) | — | Standard dispatch pattern in bridge.rs; one method per table |
| `known_tables()` update | Rust core (bridge) | — | `storage_check.rs` reads `known_tables()` from `store.rs`; must stay in sync |
| Live HR sample ingestion | BLE layer (`GooseBLEClient`) | — | Sanitized, throttled HR callback via `onLiveHeartRate` |
| Strain accumulation | Swift actor (`GooseStrainAccumulator`) | — | Realtime Swift-side computation; Rust bridge too slow for per-sample latency |
| Strain publication to UI | `GooseAppModel` (@MainActor) | — | `@Published var liveWorkoutStrain` on the central coordinator |
| Workout session lifecycle | `GooseAppModel+ActivityRecording.swift` | `PassiveActivityDetectionPipeline` | `activeActivityPersistence` tracks session; `finished` event is the reset trigger |

## Standard Stack

### Core (No new dependencies — all work uses existing stack)

| Component | Version/Location | Purpose |
|-----------|-----------------|---------|
| `rusqlite` | 0.37 (bundled) | SQLite DDL + DML in `store.rs` |
| `serde` / `serde_json` | 1.0 | Bridge args/results serialisation |
| Swift `actor` | Swift 5.0 (iOS 26 target) | `GooseStrainAccumulator` concurrency isolation |
| `Task { @MainActor in }` | Foundation | Bridge accumulator result → `@Published` state |

No new packages. Zero new dependencies.

**Installation:** None required.

## Package Legitimacy Audit

No external packages are introduced in this phase. Section not applicable.

## Architecture Patterns

### System Architecture Diagram

```
[BLE GATT HR char]
       |
       v
GooseBLEClient.recordLiveHeartRate(bpm, source, date)
  [GooseHRSanitizer gate — rejects outside 25-220 BPM]
       |
       v (realtimeVitalsQueue)
GooseBLEClient.processLiveHeartRate
  → bleUIStateAggregator.publishLiveHeartRate (UI update)
  → onLiveHeartRate?(bpm, source, date) CALLBACK
       |
       +-------> heartRateSamplePipeline (existing)
       |
       +-------> GooseStrainAccumulator.ingest(bpm, date) [NEW]
                      |
                      | [actor-isolated: accumulates Edwards zone load]
                      | [throttle: ≤1 publish per 3s]
                      |
                      v  Task { @MainActor in }
               GooseAppModel.liveWorkoutStrain: Double [NEW @Published]
                      |
                      v
               FitnessLiveWorkoutViews (existing strain tile)

[workout lifecycle]
GooseAppModel.beginActivityRecording() → accumulator.reset()
GooseAppModel.finishActivityRecording() → accumulator.freeze()
  (also: PassiveActivityDetectionEvent.finished → same path)

[SQLite migration — DATA-01]
GooseStore.migrate() [on app launch, GooseStore.open()]
  ├── [existing] CREATE TABLE IF NOT EXISTS ... (1–19 tables)
  ├── [NEW v20 arm] CREATE TABLE IF NOT EXISTS journal (...)
  ├── [NEW v20 arm] CREATE TABLE IF NOT EXISTS workout (...)
  ├── [NEW v20 arm] CREATE TABLE IF NOT EXISTS apple_daily (...)
  ├── [NEW v20 arm] CREATE TABLE IF NOT EXISTS metric_series (...)
  ├── INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (20)
  └── PRAGMA user_version = 20

[bridge dispatch — DATA-01]
Swift: rust.request(method: "journal.upsert", args: [...])
  → handle_bridge_request_inner() match arm
  → journal_upsert_bridge(args) → GooseStore::insert_journal(...)
  → INSERT OR IGNORE INTO journal (...)
```

### Recommended Project Structure

Rust changes — existing files only:
```
Rust/core/src/
├── store.rs          # CURRENT_SCHEMA_VERSION=20, migrate() v20 arm, 4 store methods, known_tables() update
├── bridge.rs         # 4 new dispatch arms + args structs + bridge functions
└── storage_check.rs  # required_columns() updated to include 4 new tables
```

Swift changes — one new file + edits to existing:
```
GooseSwift/
├── GooseStrainAccumulator.swift  [NEW — actor]
└── GooseAppModel.swift           [edit — @Published liveWorkoutStrain, accumulator wiring]
```

No new Swift extension files needed — accumulator wiring goes into `GooseAppModel.swift` init closure (alongside existing `onLiveHeartRate` wiring) and `GooseAppModel+ActivityRecording.swift` (reset/freeze calls in begin/finish functions).

### Pattern 1: v19→v20 Migration Arm

**What:** Add 4 `CREATE TABLE IF NOT EXISTS` blocks + migration seed + PRAGMA bump to the existing `migrate()` `execute_batch` call.

**When to use:** Any schema version bump. The entire migration runs in one `execute_batch` — SQLite treats it as a single transaction.

```rust
// Source: verified from Rust/core/src/store.rs (existing v1-v19 arm)
// Add to the END of the existing execute_batch string in migrate():

CREATE TABLE IF NOT EXISTS journal (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    date       TEXT NOT NULL,
    source     TEXT NOT NULL DEFAULT 'goose',
    behaviors_json TEXT NOT NULL DEFAULT '{}',
    notes      TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
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

INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (20);
PRAGMA user_version = 20;
```

**Critical:** `PRAGMA user_version = 20` replaces the existing `PRAGMA user_version = 19` line. Only one PRAGMA statement can set user_version — the final one wins. Replace in-place rather than appending.

### Pattern 2: Bridge Upsert Method (verified from activity.create_session pattern)

**What:** Add args struct + dispatch arm + bridge function for each new table.

```rust
// Source: verified from Rust/core/src/bridge.rs — ActivitySessionUpsertArgs pattern

#[derive(Debug, Clone, Deserialize)]
struct JournalUpsertArgs {
    database_path: String,
    date: String,
    source: String,
    behaviors_json: String,
    #[serde(default)]
    notes: Option<String>,
}

// In handle_bridge_request_inner match block:
"journal.upsert" => request_args::<JournalUpsertArgs>(&request)
    .and_then(journal_upsert_bridge),

// Bridge function:
fn journal_upsert_bridge(args: JournalUpsertArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let inserted = store.insert_journal(&args)?;
    Ok(json!({
        "schema": "goose.journal-upsert-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
    }))
}
```

Store method uses `INSERT OR IGNORE` for idempotency:
```rust
pub fn insert_journal(&self, args: &JournalUpsertArgs) -> GooseResult<bool> {
    let rows = self.conn.execute(
        "INSERT OR IGNORE INTO journal (date, source, behaviors_json, notes)
         VALUES (?1, ?2, ?3, ?4)",
        params![args.date, args.source, args.behaviors_json, args.notes],
    )?;
    Ok(rows > 0)
}
```

### Pattern 3: GooseStrainAccumulator (Swift actor)

**What:** New Swift `actor` that accumulates Edwards Zone Load in real time.

**Edwards Zone Load formula (from CONTEXT.md specifics — confirmed WHOOP-compatible):**
- Z1: HR < 60% HRmax → multiplier 1.0
- Z2: HR 60–70% HRmax → multiplier 2.0
- Z3: HR 70–80% HRmax → multiplier 3.0
- Z4: HR 80–90% HRmax → multiplier 4.0
- Z5: HR > 90% HRmax → multiplier 5.0

Accumulated strain = sum over all samples of: `multiplier × interval_seconds`

```swift
// Source: pattern derived from GooseBLEBondingManager.swift (callback pattern)
// and GooseAppModel init (Task @MainActor publication pattern)

actor GooseStrainAccumulator {
  private var accumulatedLoad: Double = 0
  private var lastSampleDate: Date?
  private var lastPublishedAt: Date = .distantPast
  private var maxHR: Double = 190  // default; overridable
  private let publishInterval: TimeInterval = 3

  // Called from GooseAppModel.onLiveHeartRate closure (background queue → actor hop)
  func ingest(bpm: Int, date: Date) {
    guard let last = lastSampleDate else {
      lastSampleDate = date
      return
    }
    let interval = date.timeIntervalSince(last)
    guard interval > 0, interval < 30 else {   // ignore stale samples
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
  }

  // Returns current load if throttle allows, else nil
  func pollIfReady(now: Date) -> Double? {
    guard now.timeIntervalSince(lastPublishedAt) >= publishInterval else { return nil }
    lastPublishedAt = now
    return accumulatedLoad
  }
}
```

Wiring in `GooseAppModel.init()`:
```swift
// Add alongside existing onLiveHeartRate closure
ble.onLiveHeartRate = { [weak self] bpm, source, capturedAt in
  heartRateSamplePipeline.recordHeartRateSample(bpm: bpm, source: source, capturedAt: capturedAt)
  // NEW: feed accumulator
  Task { [weak self] in
    await self?.strainAccumulator.ingest(bpm: bpm, date: capturedAt)
    if let load = await self?.strainAccumulator.pollIfReady(now: capturedAt) {
      Task { @MainActor [weak self] in
        self?.liveWorkoutStrain = load
      }
    }
  }
}
```

Reset in `beginActivityRecording` and `finishActivityRecording`:
```swift
Task { await strainAccumulator.reset() }
```

### Anti-Patterns to Avoid

- **Calling bridge for per-sample strain**: The Rust bridge is synchronous and blocks the calling thread. Calling `rust.request(method: "strain.compute")` on every HR sample would block `realtimeVitalsQueue`. Compute Swift-side instead.
- **Updating `CURRENT_SCHEMA_VERSION` without updating `PRAGMA user_version`**: The schema version check in `open_existing_current()` reads `PRAGMA user_version`, not the migrations table. Both must be updated.
- **Forgetting `known_tables()` in `store.rs`**: `storage_check.rs` imports `known_tables()` and verifies every table it returns exists in the DB. Adding 4 tables to `migrate()` without adding them to `known_tables()` will cause `check_storage_database` to report them missing and fail `storage_ready`.
- **Forgetting `required_columns()` in `storage_check.rs`**: The `required_columns()` function in `storage_check.rs` defines the expected columns per table. Tables added to `known_tables()` without a corresponding entry in `required_columns()` will trigger `debug_assert!` at compile time.
- **Using a `class` instead of `actor` for GooseStrainAccumulator**: The codebase uses `final class` with `NSLock` for older types (e.g. `GooseBLEBondingManager`), but the CONTEXT.md decision is `actor` — this is the right choice for new code as it provides automatic isolation without manual locking.
- **Publishing liveWorkoutStrain when no workout is active**: Guard `ingest` or publication with `activeActivityPersistence != nil` to avoid spurious strain updates during rest periods.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead |
|---------|-------------|-------------|
| SQLite transaction safety | Custom rollback logic | `GooseStore.immediate_transaction` (already implemented) |
| Thread-safe Swift actor | NSLock + class | Swift `actor` keyword (isolation is automatic) |
| HR sanity check in accumulator | Custom BPM range check | Samples from `onLiveHeartRate` are already sanitized by `GooseHRSanitizer` (25-220 BPM) — no second gate needed |
| Migration idempotency | Version number checks in Rust | `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE` (already the project's established pattern) |

## Common Pitfalls

### Pitfall 1: PRAGMA user_version not updated

**What goes wrong:** `schema_version()` returns 19 after migration runs. `open_existing_current()` will refuse to open the DB. `check_storage_database` reports `schema_version_valid: false`.

**Why it happens:** The developer adds `CREATE TABLE IF NOT EXISTS` blocks and the migration row seed but forgets to change `PRAGMA user_version = 19` to `PRAGMA user_version = 20` at the end of the `execute_batch` string.

**How to avoid:** Search for `PRAGMA user_version` in `store.rs` — there is exactly one occurrence. Change `= 19` to `= 20`.

**Warning signs:** `cargo test` passes but `test_exercise_sessions_schema_version` (adapted to v20) fails with `expected 20, got 19`.

### Pitfall 2: known_tables() and required_columns() not updated

**What goes wrong:** `storage_check` reports tables as missing even though they exist. A `debug_assert!` panics in debug builds.

**Why it happens:** There are three synchronised locations: `migrate()` in `store.rs`, `known_tables()` in `store.rs`, and `required_columns()` in `storage_check.rs`. Missing one breaks the health check.

**How to avoid:** After adding tables to `migrate()`, grep for `known_tables` and `required_columns` and update both. The `debug_assert!` in `required_columns()` will catch mismatches at test time.

**Warning signs:** `cargo test` with `storage_check` tests fails with "missing table" or panics on `debug_assert!`.

### Pitfall 3: actor method called from @MainActor without Task

**What goes wrong:** Swift compiler error: "actor-isolated instance method 'ingest(bpm:date:)' can not be referenced from main actor".

**Why it happens:** `GooseStrainAccumulator` is an `actor` — its methods must be called from async context. The `onLiveHeartRate` closure runs on `realtimeVitalsQueue` (non-isolated), which is fine, but any @MainActor call site will fail.

**How to avoid:** Always wrap accumulator calls in `Task { await accumulator.method() }`. This is the same pattern used for `onHRSpike` in the existing codebase: `Task { @MainActor in self?.hrSpikeCount += 1 }`.

### Pitfall 4: Strain accumulates during rest (no active workout guard)

**What goes wrong:** `liveWorkoutStrain` shows a growing value even when no workout is active, confusing the user.

**Why it happens:** `ble.onLiveHeartRate` fires continuously whenever a WHOOP is connected — not only during workouts.

**How to avoid:** In the `onLiveHeartRate` closure, guard with `activeActivityPersistence != nil` before calling `strainAccumulator.ingest`. Alternatively, `GooseStrainAccumulator.ingest` can be a no-op when in a "frozen" state set by `finishActivityRecording`.

**Warning signs:** `liveWorkoutStrain` is non-zero on the home screen outside workout sessions.

### Pitfall 5: INSERT OR IGNORE silently drops updates

**What goes wrong:** Calling `journal.upsert` twice with different `behaviors_json` for the same `(source, date)` — the second call silently ignores the new data.

**Why it happens:** `INSERT OR IGNORE` with a `UNIQUE` constraint on `(source, date)` will skip any row that already exists, regardless of new column values.

**How to avoid:** For tables where updates are expected (journal, apple_daily, workout), use `INSERT OR REPLACE` or a two-step upsert: `INSERT OR IGNORE` followed by `UPDATE ... WHERE changes() = 0`. The CONTEXT.md locks `INSERT OR IGNORE` for `metricSeries` — that table is append-only by design. For the other three, the planner should decide: use `INSERT OR REPLACE` which replaces the whole row, or accept that only the first write wins per day.

**Warning signs:** Health data stops updating in the DB after the first daily write.

### Pitfall 6: migrate() PRAGMA user_version position matters

**What goes wrong:** If `PRAGMA user_version = 20` appears before the `CREATE TABLE IF NOT EXISTS` blocks, and any table creation fails, the schema version will still be bumped — leaving the DB in an inconsistent state.

**Why it happens:** SQLite executes `execute_batch` statements in order. `PRAGMA user_version` is not transactional in the same way as DML.

**How to avoid:** Keep `PRAGMA user_version = 20` as the last statement in `execute_batch`, matching the existing pattern in the codebase (verified: `PRAGMA user_version = 19` is at line 1764, after all CREATE TABLE statements at lines 1113–1763).

## Code Examples

### existing migrate() structure (verified)

```rust
// Source: Rust/core/src/store.rs, GooseStore::migrate(), lines 1108-1775
pub fn migrate(&self) -> GooseResult<()> {
    self.conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS goose_schema_migrations ( ... );
        CREATE TABLE IF NOT EXISTS raw_evidence ( ... );
        // ... 37 more tables ...
        CREATE TABLE IF NOT EXISTS upload_cursors ( ... );

        INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (1);
        // ... through ...
        INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (19);
        PRAGMA user_version = 19;
        "#,
    )?;
    self.ensure_raw_evidence_columns()?;
    // ... other ensure_* calls ...
    Ok(())
}
```

v20 arm: append 4 CREATE TABLE blocks before `INSERT OR IGNORE VALUES (1)` block, add `INSERT OR IGNORE VALUES (20)` alongside existing seeds, and change `PRAGMA user_version = 19` to `PRAGMA user_version = 20`.

### known_tables() location (verified)

```rust
// Source: Rust/core/src/store.rs, line 8730
pub fn known_tables() -> &'static [&'static str] {
    &[
        "goose_schema_migrations",
        "raw_evidence",
        // ... 37 table names ...
        "upload_cursors",
        // ADD HERE:
        // "journal",
        // "workout",
        // "apple_daily",
        // "metric_series",
    ]
}
```

### required_columns() location (verified)

```rust
// Source: Rust/core/src/storage_check.rs, line 496
fn required_columns() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut columns = BTreeMap::new();
    // ... existing 38 tables ...
    columns.insert("journal", vec!["id", "date", "source", "behaviors_json", "created_at"]);
    columns.insert("workout", vec!["id", "date", "source", "sport", "start_time", "end_time", "duration_s"]);
    columns.insert("apple_daily", vec!["id", "date", "source", "created_at"]);
    columns.insert("metric_series", vec!["id", "source", "metric_name", "date", "value", "created_at"]);
    // debug_assert verifies each known_tables() entry has a required_columns entry
    for table in known_tables() {
        debug_assert!(columns.contains_key(table));
    }
    columns
}
```

### activity_sessions schema (FK target for workout table)

```sql
-- Source: Rust/core/src/store.rs, lines 1213-1239 (verified)
CREATE TABLE IF NOT EXISTS activity_sessions (
    session_id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    start_time_unix_ms INTEGER NOT NULL,
    end_time_unix_ms INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    activity_type TEXT NOT NULL,
    -- ...
);
```

The `workout` table FK: `activity_session_id TEXT REFERENCES activity_sessions(session_id) ON DELETE SET NULL` — allows workout rows to exist without an activity_session (e.g. manually entered workouts).

### onLiveHeartRate callback wiring (verified)

```swift
// Source: GooseSwift/GooseAppModel.swift, lines 308-310
ble.onLiveHeartRate = { bpm, source, capturedAt in
  heartRateSamplePipeline.recordHeartRateSample(bpm: bpm, source: source, capturedAt: capturedAt)
}
```

The accumulator tap goes here alongside `heartRateSamplePipeline`.

### workout session end signal (verified)

```swift
// Source: GooseSwift/GooseAppModel+PacketPublishing.swift, line 743
case .finished(let summary, let reason):
  finishActivityRecording(
    activity: summary.activity,
    startedAt: summary.startedAt,
    endedAt: summary.endedAt,
    // ...
  )
```

`finishActivityRecording` in `GooseAppModel+ActivityRecording.swift` is the single function to hook for accumulator freeze/reset on workout end (both manual and auto-detected workouts converge here).

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Per-metric migrations (ALTER TABLE ADD COLUMN) | `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE` in single batch | Idempotent; safe to re-run |
| NSLock + class for concurrency | Swift `actor` keyword | No manual locking needed for new types |
| Rust bridge for realtime metrics | Swift-side accumulation for sub-second feedback | Avoids synchronous FFI latency on hot path |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Edwards Zone Load zone boundaries from CONTEXT.md specifics: Z1 <60%, Z2 60-70%, Z3 70-80%, Z4 80-90%, Z5 >90% with multipliers 1.0/2.0/3.0/4.0/5.0 | Architecture Patterns — Pattern 3 | Strain values would be wrong; low risk as these match WHOOP-published zones |
| A2 | Default maxHR of 190 is acceptable when no user age data is available | Architecture Patterns — Pattern 3 | Slightly wrong zones for very young or old users; planner should decide whether to pass maxHR from GooseAppModel |
| A3 | The `workout` table UNIQUE constraint should be `(source, start_time)` not `(source, start_time, sport)` — the seed file uses `(deviceId, startTs, sport)` but the CONTEXT.md decision is FK to activity_sessions with no deviceId column | Standard Stack / Code Examples | If two workouts start at the same second with different sports, only the first would persist; unlikely in practice |

## Open Questions

1. **INSERT OR IGNORE vs INSERT OR REPLACE for journal/workout/apple_daily**
   - What we know: `metricSeries` is locked to `INSERT OR IGNORE` by CONTEXT.md. For the other three tables, the seed file implies daily overwrites are expected (e.g. HealthKit re-imports).
   - What's unclear: Should `journal.upsert` replace the whole row (allowing behaviour edits) or silently ignore re-writes?
   - Recommendation: Use `INSERT OR REPLACE` for `journal`, `apple_daily`, and `workout` (to allow edits); keep `INSERT OR IGNORE` only for `metric_series` (append-only by design). Planner should confirm this.

2. **GooseStrainAccumulator maxHR source**
   - What we know: Claude's Discretion per CONTEXT.md. No user age is stored in the codebase. `GooseBLEClient.restingHeartRateEstimateBPM` exists but is RHR, not HRmax.
   - What's unclear: Whether to use a hardcoded default (190) or expose a property on `GooseStrainAccumulator` that GooseAppModel can set when the user configures their profile.
   - Recommendation: Default 190 for this phase with a `setMaxHR(_ bpm: Double)` actor method left as a hook for Phase 72. Document the default in a `static let` constant.

3. **liveWorkoutStrain guard during non-workout periods**
   - What we know: `onLiveHeartRate` fires continuously when WHOOP is connected.
   - What's unclear: Should we zero `liveWorkoutStrain` when no workout is active, or just freeze it at the last session value?
   - Recommendation: Set `liveWorkoutStrain = 0` in `GooseAppModel` when `activeActivityPersistence` becomes nil (on workout finish). This makes the strain tile show "0" at rest, which is correct.

## Environment Availability

This phase is pure Rust + Swift code changes. No external services, CLIs, or runtimes beyond the existing build environment are required.

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | `cargo test` | ✓ | (project standard) | — |
| Xcode + iOS SDK | Swift compilation | ✓ | 26.5 (per MEMORY.md) | — |

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` (built-in) |
| Config file | `Rust/core/Cargo.toml` |
| Quick run command | `cargo test -p goose-core --lib -- store v20` |
| Full suite command | `cargo test -p goose-core` (from `Rust/core/`) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DATA-01 | v20 schema version after migration | unit | `cargo test -p goose-core --lib -- test_schema_version_is_20` | ❌ Wave 0 |
| DATA-01 | All 4 new tables exist after migration | unit | `cargo test -p goose-core --lib -- test_v20_tables_exist` | ❌ Wave 0 |
| DATA-01 | v19→v20 migration arm is idempotent (run twice) | unit | `cargo test -p goose-core --lib -- test_v20_migration_idempotent` | ❌ Wave 0 |
| DATA-01 | metric_series INSERT OR IGNORE — concurrent writes produce no duplicates | unit | `cargo test -p goose-core --lib -- test_metric_series_no_duplicate` | ❌ Wave 0 |
| DATA-01 | storage_check passes with v20 DB | integration | `cargo test -p goose-core -- storage_check_tests` | ✅ (needs v20 update) |
| DATA-02 | GooseStrainAccumulator — manual-only (BLE device required) | manual | — | n/a |

### Sampling Rate

- **Per task commit:** `cargo test -p goose-core --lib -- store` (store unit tests only, ~5s)
- **Per wave merge:** `cargo test -p goose-core` (full suite)
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps

- [ ] `Rust/core/src/store.rs` — add `#[cfg(test)] mod v20_migration_tests { ... }` with 4 tests listed above
- [ ] `Rust/core/tests/storage_check_tests.rs` — update expected schema version from 19 to 20

## Security Domain

`security_enforcement: true` (from `.planning/config.json`). ASVS Level 1 applies.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | `validate_required()` pattern already used in `store.rs` for all string inputs; apply same for new bridge args |
| V6 Cryptography | no | — |

### Known Threat Patterns for SQLite bridge

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via table name | Tampering | Table names are hardcoded in Rust; no user-supplied table names. Not applicable to v20 arm. |
| Unchecked TEXT field length | Tampering | `behaviors_json` can be arbitrarily large. `validate_required()` only checks non-empty. Add max-length guard or accept that SQLite has no column size limit. |
| `metric_name` with special characters | Tampering | Used as a lookup key — validate format (e.g. regex `[a-z0-9._-]+`). Add to bridge arg validation. |

## Sources

### Primary (HIGH confidence — verified by direct code inspection)

- `Rust/core/src/store.rs` — Migration pattern (lines 1108-1775), `known_tables()` (line 8730), `CURRENT_SCHEMA_VERSION` (line 14), `ActivitySessionInput` struct (reference FK schema)
- `Rust/core/src/storage_check.rs` — `required_columns()` (line 496), `check_table()` pattern
- `Rust/core/src/bridge.rs` — dispatch pattern (lines 2141+), `ActivitySessionUpsertArgs` (line 1520), `activity_create_session_bridge` (line 7145)
- `GooseSwift/GooseAppModel.swift` — `onLiveHeartRate` wiring (lines 308-310), accumulator hook point
- `GooseSwift/GooseBLEClient+VitalsAndLogging.swift` — `recordLiveHeartRate` → `onLiveHeartRate?` callback path
- `GooseSwift/GooseAppModel+ActivityRecording.swift` — `beginActivityRecording`, `finishActivityRecording` lifecycle
- `GooseSwift/GooseAppModel+PacketPublishing.swift` — `applyActivityDetectionEvents` / `.finished` event handling (line 743)
- `GooseSwift/WhoopEventSamples.swift` — `WhoopDataSignalSample` struct (line 235) — confirms WhoopDataSignalPipeline does NOT carry HR BPM directly
- `.planning/seeds/journal-workout-datastore.md` — table schema proposals (informational)
- `.planning/seeds/realtime-strain-accumulation.md` — WHOOP reverse engineering basis

### Secondary (MEDIUM confidence)

- `GooseSwift/GooseBLEBondingManager.swift` — reference pattern for Swift callback-based manager type
- `.planning/phases/69-data-foundation/69-CONTEXT.md` — all locked decisions

### Tertiary (LOW confidence)

- Edwards Zone Load zone boundaries: from CONTEXT.md specifics (stated as "standard WHOOP-compatible"); not independently verified against WHOOP binary in this session [ASSUMED]

## Metadata

**Confidence breakdown:**
- Standard Stack: HIGH — zero new dependencies; all existing stack
- Architecture: HIGH — migration and bridge patterns verified by direct code inspection
- Edwards strain formula: MEDIUM — boundaries from CONTEXT.md; multipliers standard but not re-verified via Ghidra in this session
- Swift actor pattern: HIGH — pattern matches existing codebase callback + Task @MainActor convention

**Research date:** 2026-06-12
**Valid until:** 2026-07-12 (stable codebase patterns; only invalidated by concurrent schema changes)
