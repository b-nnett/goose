---
plan: 69-01
phase: 69-data-foundation
status: complete
started: 2026-06-12
completed: 2026-06-12
requirements: [DATA-01]
commits:
  - fedcad5
  - 6bbc804
---

## What Was Built

Added schema v20 migration to the Rust store, creating 4 new SQLite tables and 4 bridge upsert methods for DATA-01 (journal, workout, apple_daily, metric_series).

### Schema Migration (Task 1)

**store.rs migration arm v19→v20:**
- `journal` — (id INTEGER PK, date TEXT, source TEXT, behaviors_json TEXT, notes TEXT, created_at TEXT)
- `workout` — (id INTEGER PK, date TEXT, source TEXT, sport TEXT, start_time TEXT, end_time TEXT, duration_s REAL, activity_session_id TEXT FK, avg_hr_bpm REAL, max_hr_bpm REAL, strain REAL, calories_kcal REAL, distance_m REAL, notes TEXT, provenance_json TEXT, created_at TEXT)
- `apple_daily` — (id INTEGER PK, date TEXT, source TEXT, steps INTEGER, active_kcal REAL, basal_kcal REAL, avg_hr_bpm REAL, max_hr_bpm REAL, vo2max REAL, weight_kg REAL, created_at TEXT)
- `metric_series` — (id INTEGER PK, source TEXT, metric_name TEXT, date TEXT, value REAL, created_at TEXT, UNIQUE(source, metric_name, date))

**Mandatory triple-sync updated:**
- `migrate()` — 4 CREATE TABLE IF NOT EXISTS + INSERT OR IGNORE INTO goose_schema_migrations(20) + PRAGMA user_version = 20
- `known_tables()` — added "journal", "workout", "apple_daily", "metric_series"
- `required_columns()` in storage_check.rs — added all 4 tables with their column lists

**Idempotency test:**
- `test_v20_migration_idempotent` — runs migrate() twice, confirms version=20 and count=1 in goose_schema_migrations

### Bridge Upsert Methods (Task 2)

4 new dispatch arms in bridge.rs + corresponding bridge functions + arg structs:
- `journal.upsert` — INSERT OR REPLACE (editable daily entry)
- `workout.upsert` — INSERT OR REPLACE (editable workout record with optional FK to activity_sessions)
- `apple_daily.upsert` — INSERT OR REPLACE (editable daily Apple Health rollup)
- `metric_series.upsert` — INSERT OR IGNORE (append-only; UNIQUE constraint prevents duplicates)

All 4 added to `BRIDGE_METHODS` constant in sorted order.

### Fix Applied During Verification

Executor committed dispatch arms before BRIDGE_METHODS constant was updated → `bridge_methods_constant_matches_dispatcher` test failed. Fixed by adding 4 sorted entries to BRIDGE_METHODS and updating schema version assertions (`test_schema_version_is_19` → `test_schema_version_is_20`, `test_exercise_sessions_schema_version` 19→20).

**Note:** 14 `bridge_tests` failures are pre-existing (documented in `.planning/debug/rust-ci-linux-test-failures.md`, present before Phase 69). Not a regression.

### Files Modified

- **Rust/core/src/store.rs** — v20 migration arm, known_tables(), insert_journal/workout/apple_daily/metric_series methods
- **Rust/core/src/storage_check.rs** — required_columns() entries for 4 new tables
- **Rust/core/src/bridge.rs** — BRIDGE_METHODS constant + 4 arg structs + 4 dispatch arms + 4 bridge functions

## Self-Check: PASSED

- `PRAGMA user_version = 20` in migration ✓
- `goose_schema_migrations` seeds version 20 ✓
- `known_tables()` includes all 4 new tables ✓
- `required_columns()` includes all 4 new tables ✓
- `bridge_methods_constant_matches_dispatcher` passes ✓
- `test_schema_version_is_20` passes ✓
- `test_exercise_sessions_schema_version` passes (asserts 20) ✓
- `metric_series` INSERT OR IGNORE idempotency test passes ✓
- 4 bridge methods in BRIDGE_METHODS sorted correctly ✓

key-files:
  modified:
    - Rust/core/src/store.rs
    - Rust/core/src/storage_check.rs
    - Rust/core/src/bridge.rs
