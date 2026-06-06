-- Whoop datastore schema. Runs on first DB init; the ingest service re-applies it
-- idempotently on startup. Decoded stream `ts` is wall-clock (TIMESTAMPTZ); raw
-- IMU/optical are NOT exploded here (archived to disk, indexed in raw_batches).
CREATE EXTENSION IF NOT EXISTS timescaledb;

CREATE TABLE IF NOT EXISTS devices (
    device_id   TEXT PRIMARY KEY,
    mac         TEXT,
    name        TEXT,
    first_seen  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS raw_batches (
    batch_id          TEXT PRIMARY KEY,  -- opaque idempotency key (UUID from live/Mac; "hist-<device>-<trim>" from backfill)
    device_id         TEXT NOT NULL REFERENCES devices(device_id),
    received_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    device_clock_ref  BIGINT NOT NULL,
    wall_clock_ref    TIMESTAMPTZ NOT NULL,
    start_ts          TIMESTAMPTZ,
    end_ts            TIMESTAMPTZ,
    packet_count      INTEGER NOT NULL,
    file_path         TEXT NOT NULL,
    sha256            TEXT NOT NULL,
    byte_size         BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS raw_batches_device_time ON raw_batches (device_id, start_ts);

-- Decoded summary streams (Timescale hypertables; partition column `ts` is in every PK).
CREATE TABLE IF NOT EXISTS hr_samples (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    bpm       SMALLINT NOT NULL,
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('hr_samples', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS rr_intervals (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    rr_ms     INTEGER NOT NULL,
    PRIMARY KEY (device_id, ts, rr_ms)
);
SELECT create_hypertable('rr_intervals', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS events (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    kind      TEXT NOT NULL,
    payload   JSONB,
    PRIMARY KEY (device_id, ts, kind)
);
SELECT create_hypertable('events', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS battery (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    soc       REAL,
    mv        INTEGER,
    charging  BOOLEAN,   -- from the dense BATTERY_LEVEL event (nullable; command responses omit it)
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('battery', 'ts', if_not_exists => TRUE);
-- Idempotent migration for already-initialised databases (CREATE … IF NOT EXISTS is a no-op
-- when the table exists). bootstrap_schema re-applies this file on every startup.
ALTER TABLE battery ADD COLUMN IF NOT EXISTS charging BOOLEAN;

-- Type-47 V24 biometric history streams (the 14-day on-strap store). SpO2, skin-temp
-- and respiration values are RAW ADC counts as emitted by the strap; WHOOP computes the
-- human-readable units (%, °C, breaths/min) cloud-side, so we persist raw and do NOT
-- convert here. Gravity is the accel-derived gravity vector in g.
CREATE TABLE IF NOT EXISTS spo2_samples (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    red       INTEGER NOT NULL,  -- raw ADC (red LED)
    ir        INTEGER NOT NULL,  -- raw ADC (IR LED)
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('spo2_samples', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS skin_temp_samples (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    raw       INTEGER NOT NULL,  -- raw ADC; cloud-computed °C
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('skin_temp_samples', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS resp_samples (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    raw       INTEGER NOT NULL,  -- raw ADC; cloud-computed breaths/min
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('resp_samples', 'ts', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS gravity_samples (
    device_id TEXT NOT NULL,
    ts        TIMESTAMPTZ NOT NULL,
    x         REAL NOT NULL,
    y         REAL NOT NULL,
    z         REAL NOT NULL,
    PRIMARY KEY (device_id, ts)
);
SELECT create_hypertable('gravity_samples', 'ts', if_not_exists => TRUE);

-- ── WHOOP 5.0 generation tag (Phase 05, D-09 / SRV-04) ────────────────────────
-- Idempotent migration: tag every decoded-stream hypertable with the device
-- generation that produced the row. DEFAULT '5.0' classifies existing rows as 5.0
-- (this is a 5.0-only deployment); ADD COLUMN IF NOT EXISTS keeps the startup
-- re-apply (bootstrap_schema) a no-op once the column exists. Backward-compatible:
-- clients that omit device_generation get the '5.0' default at ingest time.
ALTER TABLE hr_samples        ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE rr_intervals      ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE events            ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE battery           ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE spo2_samples      ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE skin_temp_samples ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE resp_samples      ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';
ALTER TABLE gravity_samples   ADD COLUMN IF NOT EXISTS device_generation TEXT DEFAULT '5.0';

-- ── Derived daily-analysis tables (Task 2.5) ──────────────────────────────────
-- These hold the OUTPUT of the analysis pipeline (sleep/recovery/strain/exercise),
-- one row per night/workout/day. They are LOW VOLUME (a handful of rows per device
-- per day) and are looked up by exact key, so they are PLAIN tables — deliberately
-- NOT Timescale hypertables (hypertables only earn their keep on high-cardinality
-- time-series ingest, which these derived rollups are not). All upserts are
-- idempotent (recompute overwrites by PK) so re-running compute_day never dupes.

CREATE TABLE IF NOT EXISTS sleep_sessions (
    device_id   TEXT NOT NULL,
    start_ts    TIMESTAMPTZ NOT NULL,
    end_ts      TIMESTAMPTZ NOT NULL,
    efficiency  REAL,
    resting_hr  SMALLINT,
    avg_hrv     REAL,
    stages      JSONB,          -- [{start,end,stage}]
    PRIMARY KEY (device_id, start_ts)
);

CREATE TABLE IF NOT EXISTS exercise_sessions (
    device_id     TEXT NOT NULL,
    start_ts      TIMESTAMPTZ NOT NULL,
    end_ts        TIMESTAMPTZ NOT NULL,
    avg_hr        REAL,
    peak_hr       SMALLINT,
    strain        REAL,
    kind          TEXT,
    -- Per-bout intensity fields (metrics-accuracy overhaul, Task 11a). APPROXIMATE.
    duration_s    INTEGER,      -- bout duration (end − start), seconds
    zone_time_pct JSONB,        -- Edwards zone 0–5 time breakdown, {"0":pct,…,"5":pct}
    avg_hrr_pct   REAL,         -- mean Karvonen %HRR over the bout, [0,100]
    hrmax         REAL,         -- effective HRmax used for zone math (bpm)
    hrmax_source  TEXT,         -- "observed" | "tanaka" | "caller" | "unknown"
    PRIMARY KEY (device_id, start_ts)
);
-- Idempotent migration for already-initialised databases (the CREATE … IF NOT EXISTS
-- above is a no-op when the table exists). bootstrap_schema re-applies this file on
-- every startup, so the per-bout intensity columns are added in place on upgrade.
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS duration_s    INTEGER;
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS zone_time_pct JSONB;
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS avg_hrr_pct   REAL;
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS hrmax         REAL;
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS hrmax_source  TEXT;
-- Calorie estimation (WHOOP/Keytel formula). Populated when a user profile is set.
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS calories_kcal REAL;
ALTER TABLE exercise_sessions ADD COLUMN IF NOT EXISTS calories_kj   REAL;

-- User profile (height/weight/age/sex) used for calorie estimation.
-- One row per device; upserted via POST /v1/profile.
CREATE TABLE IF NOT EXISTS profile (
    device_id  TEXT PRIMARY KEY,
    height_cm  REAL,
    weight_kg  REAL,
    age        INTEGER,
    sex        TEXT,           -- "male" | "female" | "nonbinary" | NULL
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS daily_metrics (
    device_id      TEXT NOT NULL,
    day            DATE NOT NULL,
    total_sleep_min REAL,
    efficiency     REAL,
    deep_min       REAL,
    rem_min        REAL,
    light_min      REAL,
    disturbances   INTEGER,
    resting_hr     SMALLINT,
    avg_hrv        REAL,
    recovery       REAL,
    strain         REAL,
    exercise_count INTEGER,
    sleep_start    TIMESTAMPTZ,   -- in-bed start of the night (for bed/wake display)
    sleep_end      TIMESTAMPTZ,   -- in-bed end (wake) of the night
    -- Calibrated nightly biometric signals (metrics-accuracy overhaul, Task 11a).
    -- ALL APPROXIMATE / UN-CALIBRATED until units.fit_* is run against WHOOP ground truth.
    spo2_pct        REAL,         -- nightly SpO2 estimate (%), windowed ratio-of-ratios
    skin_temp_dev_c REAL,         -- nightly skin-temp deviation from trailing baseline (°C)
    resp_rate_bpm   REAL,         -- nightly respiratory rate (breaths/min), Welch-peak
    computed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_id, day)
);
-- Idempotent migration for already-initialised databases: CREATE TABLE IF NOT
-- EXISTS above is a no-op when the table already exists, so add the bed/wake +
-- calibrated-signal columns explicitly (bootstrap_schema re-applies this file on
-- every startup).
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS sleep_start     TIMESTAMPTZ;
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS sleep_end       TIMESTAMPTZ;
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS spo2_pct        REAL;
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS skin_temp_dev_c REAL;
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS resp_rate_bpm   REAL;
-- ── Phase 13 Backend Parity columns (ALG-10..13) ──────────────────────────────
-- Server-side derived daily metrics. All nullable; ALTER … IF NOT EXISTS keeps the
-- bootstrap_schema startup re-apply a no-op once present. No algorithm logic here —
-- the compute pipeline (Plans 13-02..13-04) populates these in upsert_daily_metrics.
-- Phase 13 ALG-10: Sleep Performance weighted score 0–100
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS sleep_performance   REAL;
-- Phase 13 ALG-11: Training State (RESTORATIVE | OPTIMAL | OVERREACHING | NULL)
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS training_state      TEXT;
-- Phase 13 ALG-12: Sleep Needed (minutes), baseline + strain/sleep debt, clamp [300,660]
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS sleep_needed_min    REAL;
-- Phase 13 ALG-13: Total daily calories (RMR Mifflin–St Jeor + exercise kcal)
ALTER TABLE daily_metrics ADD COLUMN IF NOT EXISTS total_calories_kcal REAL;
