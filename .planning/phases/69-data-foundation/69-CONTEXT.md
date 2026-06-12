# Phase 69: Data Foundation - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 69 delivers two capabilities:
1. **DATA-01**: 4 new SQLite tables (journal, workout, appleDaily, metricSeries) added via a single v19→v20 migration arm in store.rs — with bridge upsert methods for each. Schema v20 must be idempotent (re-running migration produces no error).
2. **DATA-02**: GooseStrainAccumulator Swift actor that receives HR samples from WhoopDataSignalPipeline, computes live Edwards Zone Load strain, and publishes to GooseAppModel at ≤1 update/3s — visible on the workout screen strain tile.

Out of scope: query_range bridge methods, full CRUD API, UI screens (Phase 72), historical strain backfill.

</domain>

<decisions>
## Implementation Decisions

### Schema Design (DATA-01)
- **Timestamps**: Use `date TEXT` (ISO8601) across all 4 tables — consistent with existing codebase patterns
- **metricSeries idempotency**: `UNIQUE(source, metric_name, date)` + `INSERT OR IGNORE` pattern
- **journal**: Store behaviours as a JSON blob (`behaviors_json TEXT`) — avoids schema lock-in for evolving behaviour definitions
- **workout**: Separate `workout` table with `activity_session_id FK` to existing `activity_sessions` — no modification of existing tables

### Migration
- **Single arm**: One v19→v20 migration arm creates all 4 tables together — simpler than 4 separate arms
- **Idempotency**: `CREATE TABLE IF NOT EXISTS` + seed `INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (20)` — running twice produces no error

### Bridge API
- **Minimum CRUD**: Expose 4 upsert bridge methods only: `journal.upsert`, `workout.upsert`, `apple_daily.upsert`, `metric_series.upsert`
- **No query_range**: Query methods deferred — Phase 72 screens will need them but are not in scope here

### GooseStrainAccumulator (DATA-02)
- **Type**: Swift `actor` (automatic isolation, no manual @MainActor decoration needed on internal state)
- **Throttle**: ≤1 published update per 3 seconds (accumulator tracks last-publish time internally)
- **HR source**: `WhoopDataSignalPipeline` via existing `onSignalSample` callback pattern — not a new BLE subscriber
- **Publication target**: `@Published var liveWorkoutStrain: Double` on `GooseAppModel` via `Task { @MainActor in ... }`
- **Strain formula**: Edwards Zone Load (HR zones × duration in seconds) computed entirely Swift-side — does NOT call into Rust bridge (latency concern for realtime updates)

### Testing
- Migration idempotency test: run v19→v20 arm twice; assert no error and table count unchanged
- No round-trip CRUD tests required for this phase (deferred to Phase 72)

### Claude's Discretion
- Exact column names for journal/workout/appleDaily tables (beyond the constraints above)
- Whether GooseStrainAccumulator receives a `maxHR` from GooseAppModel or uses a default (220 - age)
- Implementation details of Edwards Zone Load (zone boundaries, coefficients)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Rust/core/src/store.rs` — existing migration pattern: `CREATE TABLE IF NOT EXISTS` blocks + `INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (N)` seeding
- `CURRENT_SCHEMA_VERSION` constant in store.rs — currently at v19 (19 migration rows)
- `WhoopDataSignalPipeline.swift` — existing `onSignalSample` callback delivers HR samples
- `GooseAppModel+ActivityRecording.swift` — existing workout state machine with `activeWorkoutSession` tracking
- `HealthDataStore+Snapshots.swift:strainSnapshot()` — existing strain display logic for historical data

### Established Patterns
- All Rust bridge methods: dispatch on `method` string in `bridge.rs`, parse args with `serde_json`
- Migration arm: `if !migrations.contains(&N) { conn.execute(...); }` pattern (check `goose_schema_migrations`)
- Swift actor: used for `GooseBLEHRMonitorManager` adjacent pattern (but as a final class — actor is new for this codebase)
- `@Published` state on `GooseAppModel` updated via `Task { @MainActor in self.xyz = value }`

### Integration Points
- Phase 72 will add screens that read from these tables (metricSeries, workout) — schema must be stable
- `PassiveActivityDetector.finished(summary, reason:)` fires when workout ends — GooseStrainAccumulator should reset on this signal

</code_context>

<specifics>
## Specific Ideas

- CURRENT_SCHEMA_VERSION constant must be bumped from 19 to 20 in store.rs
- The `known_tables()` function in storage_check.rs must be updated to include the 4 new table names
- GooseStrainAccumulator should reset its accumulated strain when `GooseAppModel.activeWorkoutSession` becomes nil (workout ends)
- Edwards Zone Load zones: Z1 (<60% HRmax)=1.0, Z2 (60-70%)=2.0, Z3 (70-80%)=3.0, Z4 (80-90%)=4.0, Z5 (>90%)=5.0 coefficients (standard WHOOP-compatible)

</specifics>

<deferred>
## Deferred Ideas

- `*.query_range` bridge methods for each new table — needed by Phase 72 screens
- Historical strain backfill from existing `activity_sessions` data
- UI screens consuming the new tables (Phase 72)
- Round-trip CRUD tests per table (Phase 72 or dedicated test phase)
- GooseStrainAccumulator publishing to a dedicated Swift struct instead of a raw Double

</deferred>
