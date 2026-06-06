"""DB operations for the ingest pipeline. All upserts are idempotent so re-uploaded
batches (store-and-forward retries) never duplicate rows."""
import json

import psycopg


def ensure_device(conn: psycopg.Connection, device_id: str, mac: str | None = None,
                  name: str | None = None) -> None:
    conn.execute(
        """INSERT INTO devices (device_id, mac, name) VALUES (%s, %s, %s)
           ON CONFLICT (device_id) DO UPDATE SET last_seen = now()""",
        (device_id, mac, name),
    )


def batch_exists(conn: psycopg.Connection, batch_id: str) -> bool:
    row = conn.execute("SELECT 1 FROM raw_batches WHERE batch_id = %s", (batch_id,)).fetchone()
    return row is not None


def insert_raw_batch(conn: psycopg.Connection, b: dict) -> None:
    conn.execute(
        """INSERT INTO raw_batches
           (batch_id, device_id, device_clock_ref, wall_clock_ref, start_ts, end_ts,
            packet_count, file_path, sha256, byte_size)
           VALUES (%(batch_id)s, %(device_id)s, %(device_clock_ref)s,
                   to_timestamp(%(wall_clock_ref)s), to_timestamp(%(start_ts)s),
                   to_timestamp(%(end_ts)s), %(packet_count)s, %(file_path)s,
                   %(sha256)s, %(byte_size)s)
           ON CONFLICT (batch_id) DO NOTHING""",
        b,
    )


def upsert_streams(conn: psycopg.Connection, device_id: str, streams: dict) -> dict:
    counts = {"hr": 0, "rr": 0, "events": 0, "battery": 0,
              "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0}
    with conn.cursor() as cur:
        for r in streams.get("hr", []):
            cur.execute(
                """INSERT INTO hr_samples (device_id, ts, bpm)
                   VALUES (%s, to_timestamp(%s), %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET bpm = EXCLUDED.bpm""",
                (device_id, r["ts"], r["bpm"]))
            counts["hr"] += 1
        for r in streams.get("rr", []):
            cur.execute(
                """INSERT INTO rr_intervals (device_id, ts, rr_ms)
                   VALUES (%s, to_timestamp(%s), %s)
                   ON CONFLICT (device_id, ts, rr_ms) DO NOTHING""",
                (device_id, r["ts"], r["rr_ms"]))
            counts["rr"] += 1
        for r in streams.get("events", []):
            cur.execute(
                """INSERT INTO events (device_id, ts, kind, payload)
                   VALUES (%s, to_timestamp(%s), %s, %s)
                   ON CONFLICT (device_id, ts, kind) DO UPDATE SET payload = EXCLUDED.payload""",
                (device_id, r["ts"], r["kind"], json.dumps(r.get("payload"))))
            counts["events"] += 1
        for r in streams.get("battery", []):
            cur.execute(
                """INSERT INTO battery (device_id, ts, soc, mv, charging)
                   VALUES (%s, to_timestamp(%s), %s, %s, %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET
                     soc = EXCLUDED.soc, mv = EXCLUDED.mv, charging = EXCLUDED.charging""",
                (device_id, r["ts"], r.get("soc"), r.get("mv"), r.get("charging")))
            counts["battery"] += 1
        # Type-47 V24 biometric streams (raw ADC; cloud computes human units).
        for r in streams.get("spo2", []):
            cur.execute(
                """INSERT INTO spo2_samples (device_id, ts, red, ir)
                   VALUES (%s, to_timestamp(%s), %s, %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET red = EXCLUDED.red, ir = EXCLUDED.ir""",
                (device_id, r["ts"], r["red"], r["ir"]))
            counts["spo2"] += 1
        for r in streams.get("skin_temp", []):
            cur.execute(
                """INSERT INTO skin_temp_samples (device_id, ts, raw)
                   VALUES (%s, to_timestamp(%s), %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET raw = EXCLUDED.raw""",
                (device_id, r["ts"], r["raw"]))
            counts["skin_temp"] += 1
        for r in streams.get("resp", []):
            cur.execute(
                """INSERT INTO resp_samples (device_id, ts, raw)
                   VALUES (%s, to_timestamp(%s), %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET raw = EXCLUDED.raw""",
                (device_id, r["ts"], r["raw"]))
            counts["resp"] += 1
        for r in streams.get("gravity", []):
            cur.execute(
                """INSERT INTO gravity_samples (device_id, ts, x, y, z)
                   VALUES (%s, to_timestamp(%s), %s, %s, %s)
                   ON CONFLICT (device_id, ts) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y, z = EXCLUDED.z""",
                (device_id, r["ts"], r["x"], r["y"], r["z"]))
            counts["gravity"] += 1
    return counts


# ── Derived daily-analysis upserts (Task 2.5) ────────────────────────────────
# Idempotent: re-running compute_day for the same (device, day) / (device, start)
# overwrites in place via ON CONFLICT DO UPDATE — never duplicates.

def upsert_daily_metrics(conn: psycopg.Connection, device_id: str, day, metrics: dict) -> None:
    """Upsert the single daily_metrics row for (device_id, day). ``day`` is a
    datetime.date; ``metrics`` is the flat dict produced by daily.compute_day."""
    conn.execute(
        """INSERT INTO daily_metrics
           (device_id, day, total_sleep_min, efficiency, deep_min, rem_min, light_min,
            disturbances, resting_hr, avg_hrv, recovery, strain, exercise_count,
            sleep_start, sleep_end, spo2_pct, skin_temp_dev_c, resp_rate_bpm,
            sleep_performance, training_state, sleep_needed_min, total_calories_kcal,
            computed_at)
           VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s,
                   to_timestamp(%s), to_timestamp(%s), %s, %s, %s,
                   %s, %s, %s, %s, now())
           ON CONFLICT (device_id, day) DO UPDATE SET
             total_sleep_min = EXCLUDED.total_sleep_min,
             efficiency      = EXCLUDED.efficiency,
             deep_min        = EXCLUDED.deep_min,
             rem_min         = EXCLUDED.rem_min,
             light_min       = EXCLUDED.light_min,
             disturbances    = EXCLUDED.disturbances,
             resting_hr      = EXCLUDED.resting_hr,
             avg_hrv         = EXCLUDED.avg_hrv,
             recovery        = EXCLUDED.recovery,
             strain          = EXCLUDED.strain,
             exercise_count  = EXCLUDED.exercise_count,
             sleep_start     = EXCLUDED.sleep_start,
             sleep_end       = EXCLUDED.sleep_end,
             spo2_pct        = EXCLUDED.spo2_pct,
             skin_temp_dev_c = EXCLUDED.skin_temp_dev_c,
             resp_rate_bpm   = EXCLUDED.resp_rate_bpm,
             sleep_performance   = EXCLUDED.sleep_performance,
             training_state      = EXCLUDED.training_state,
             sleep_needed_min    = EXCLUDED.sleep_needed_min,
             total_calories_kcal = EXCLUDED.total_calories_kcal,
             computed_at     = now()""",
        (device_id, day, metrics.get("total_sleep_min"), metrics.get("efficiency"),
         metrics.get("deep_min"), metrics.get("rem_min"), metrics.get("light_min"),
         metrics.get("disturbances"), metrics.get("resting_hr"), metrics.get("avg_hrv"),
         metrics.get("recovery"), metrics.get("strain"), metrics.get("exercise_count"),
         metrics.get("sleep_start"), metrics.get("sleep_end"),
         metrics.get("spo2_pct"), metrics.get("skin_temp_dev_c"), metrics.get("resp_rate_bpm"),
         metrics.get("sleep_performance"), metrics.get("training_state"),
         metrics.get("sleep_needed_min"), metrics.get("total_calories_kcal")),
    )


def delete_sessions_for_day(conn: psycopg.Connection, device_id: str, day) -> None:
    """Delete the existing derived session rows attributed to (device_id, day) so a
    recompute that yields FEWER sessions doesn't leave stale rows behind (which would
    desync daily_metrics.exercise_count from the actual exercise_sessions rows).

    Attribution mirrors the reads/compute:
      * sleep_sessions  — the night whose END date == ``day`` (matches query_sleep).
      * exercise_sessions — those whose start_ts is within the calendar day
        [day 00:00, day+1 00:00) UTC.

    Call inside compute_day's transaction, immediately before re-inserting the freshly
    computed set, so delete + insert commit atomically (idempotent recompute)."""
    with conn.cursor() as cur:
        cur.execute(
            "DELETE FROM sleep_sessions "
            "WHERE device_id = %s AND (end_ts AT TIME ZONE 'UTC')::date = %s",
            (device_id, day))
        cur.execute(
            "DELETE FROM exercise_sessions "
            "WHERE device_id = %s "
            "AND start_ts >= %s::date AT TIME ZONE 'UTC' "
            "AND start_ts <  (%s::date + INTERVAL '1 day') AT TIME ZONE 'UTC'",
            (device_id, day, day))


def upsert_sleep_sessions(conn: psycopg.Connection, device_id: str, sessions) -> None:
    """Upsert sleep sessions (PK device_id, start_ts). ``sessions`` is an iterable
    of dicts with start/end (epoch sec), efficiency, resting_hr, avg_hrv, stages
    (list of {start,end,stage} dicts)."""
    with conn.cursor() as cur:
        for s in sessions:
            cur.execute(
                """INSERT INTO sleep_sessions
                   (device_id, start_ts, end_ts, efficiency, resting_hr, avg_hrv, stages)
                   VALUES (%s, to_timestamp(%s), to_timestamp(%s), %s, %s, %s, %s)
                   ON CONFLICT (device_id, start_ts) DO UPDATE SET
                     end_ts     = EXCLUDED.end_ts,
                     efficiency = EXCLUDED.efficiency,
                     resting_hr = EXCLUDED.resting_hr,
                     avg_hrv    = EXCLUDED.avg_hrv,
                     stages     = EXCLUDED.stages""",
                (device_id, s["start"], s["end"], s.get("efficiency"),
                 s.get("resting_hr"), s.get("avg_hrv"), json.dumps(s.get("stages") or [])))


def upsert_profile(conn: psycopg.Connection, device_id: str,
                   height_cm: float | None, weight_kg: float | None,
                   age: int | None, sex: str | None) -> None:
    """Upsert the user profile row for ``device_id``. All biometric fields are
    optional (None keeps the existing value via the DO UPDATE). ``sex`` must be
    one of ``"male"``, ``"female"``, ``"nonbinary"`` or ``None``."""
    conn.execute(
        """INSERT INTO profile (device_id, height_cm, weight_kg, age, sex, updated_at)
           VALUES (%s, %s, %s, %s, %s, now())
           ON CONFLICT (device_id) DO UPDATE SET
             height_cm  = EXCLUDED.height_cm,
             weight_kg  = EXCLUDED.weight_kg,
             age        = EXCLUDED.age,
             sex        = EXCLUDED.sex,
             updated_at = now()""",
        (device_id, height_cm, weight_kg, age, sex),
    )


def upsert_exercise_sessions(conn: psycopg.Connection, device_id: str, sessions) -> None:
    """Upsert exercise sessions (PK device_id, start_ts). ``sessions`` is an iterable
    of dicts with start/end (epoch sec), avg_hr, peak_hr, strain, kind, plus the
    per-bout intensity fields duration_s, zone_time_pct (dict), avg_hrr_pct, hrmax,
    hrmax_source, calories_kcal, calories_kj. APPROXIMATE intensity fields."""
    with conn.cursor() as cur:
        for s in sessions:
            zt = s.get("zone_time_pct")
            cur.execute(
                """INSERT INTO exercise_sessions
                   (device_id, start_ts, end_ts, avg_hr, peak_hr, strain, kind,
                    duration_s, zone_time_pct, avg_hrr_pct, hrmax, hrmax_source,
                    calories_kcal, calories_kj)
                   VALUES (%s, to_timestamp(%s), to_timestamp(%s), %s, %s, %s, %s,
                           %s, %s, %s, %s, %s, %s, %s)
                   ON CONFLICT (device_id, start_ts) DO UPDATE SET
                     end_ts        = EXCLUDED.end_ts,
                     avg_hr        = EXCLUDED.avg_hr,
                     peak_hr       = EXCLUDED.peak_hr,
                     strain        = EXCLUDED.strain,
                     kind          = EXCLUDED.kind,
                     duration_s    = EXCLUDED.duration_s,
                     zone_time_pct = EXCLUDED.zone_time_pct,
                     avg_hrr_pct   = EXCLUDED.avg_hrr_pct,
                     hrmax         = EXCLUDED.hrmax,
                     hrmax_source  = EXCLUDED.hrmax_source,
                     calories_kcal = EXCLUDED.calories_kcal,
                     calories_kj   = EXCLUDED.calories_kj""",
                (device_id, s["start"], s["end"], s.get("avg_hr"),
                 s.get("peak_hr"), s.get("strain"), s.get("kind"),
                 (int(round(s["duration_s"])) if s.get("duration_s") is not None else None),
                 (json.dumps(zt) if zt is not None else None),
                 s.get("avg_hrr_pct"), s.get("hrmax"), s.get("hrmax_source"),
                 s.get("calories_kcal"), s.get("calories_kj")))
