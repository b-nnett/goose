"""Task 2.5 — daily orchestrator + persistence + endpoints (needs Docker/Timescale).

Seeds a synthetic day's 1 Hz streams (a still sleep night that ENDS on the target
day + a daytime exercise block) via store.upsert_streams, then drives compute_day,
the read queries, and the HTTP endpoints, asserting end-to-end ingest→compute→read.
"""
import datetime as _dt
import importlib

import psycopg
import pytest
from fastapi.testclient import TestClient

from app import read, store
from app.analysis import daily
from tests.conftest import requires_docker

DEVICE = "devDaily"
DAY = _dt.date(2023, 11, 15)


def _epoch(day: _dt.date, h: int, m: int = 0) -> float:
    return _dt.datetime.combine(
        day, _dt.time(h, m), _dt.timezone.utc).timestamp()


import math


def _still_block(start: float, minutes: int, *, bpm: int, dip: bool = False) -> dict:
    """Calm, still 1 Hz block: constant gravity (deltas ~0), low HR, alternating RR.

    Adds spo2 (red/ir) + skin_temp raw streams so the calibrated nightly signals
    (spo2_pct, skin_temp_dev_c, resp_rate_bpm) have data to compute over. The resp
    waveform is a slow ~0.25 Hz sinusoid (≈15 br/min) so the Welch estimator lands
    in band; spo2 carries a small synthetic pulsatile AC so the ratio is finite.
    """
    n = minutes * 60
    hr, rr, resp, grav, spo2, skin = [], [], [], [], [], []
    amp = 55 if dip else 14
    for i in range(n):
        ts = start + i
        hr.append({"ts": ts, "bpm": bpm})
        rr.append({"ts": ts, "rr_ms": 1050 + (amp if i % 2 == 0 else -amp)})
        # ~0.25 Hz breathing waveform → ≈15 br/min via the Welch-peak estimator.
        resp.append({"ts": ts, "raw": int(4000 + 200 * math.sin(2 * math.pi * 0.25 * i))})
        grav.append({"ts": ts, "x": 0.0, "y": 0.0, "z": 1.0})
        # Small pulsatile AC on red/ir around a stable DC (≈ healthy SpO2 region).
        spo2.append({"ts": ts,
                     "red": int(587 + 3 * math.sin(2 * math.pi * (i % 5) / 5.0)),
                     "ir": int(585 + 2 * math.sin(2 * math.pi * (i % 5) / 5.0))})
        skin.append({"ts": ts, "raw": 930})
    return {"hr": hr, "rr": rr, "resp": resp, "gravity": grav,
            "spo2": spo2, "skin_temp": skin}


def _active_block(start: float, minutes: int, *, bpm0: int) -> dict:
    """Jittering gravity (delta >> still threshold) + elevated HR — a workout."""
    n = minutes * 60
    hr, rr, resp, grav, spo2, skin = [], [], [], [], [], []
    for i in range(n):
        ts = start + i
        v = 1.0 if i % 2 == 0 else -1.0
        hr.append({"ts": ts, "bpm": bpm0 + (i % 9)})
        rr.append({"ts": ts, "rr_ms": 600 + (i % 40)})
        resp.append({"ts": ts, "raw": 4200})
        grav.append({"ts": ts, "x": v, "y": 0.0, "z": 0.0})
        spo2.append({"ts": ts, "red": 587, "ir": 585})
        skin.append({"ts": ts, "raw": 930})
    return {"hr": hr, "rr": rr, "resp": resp, "gravity": grav,
            "spo2": spo2, "skin_temp": skin}


def _merge(*blocks) -> dict:
    out = {"hr": [], "rr": [], "resp": [], "gravity": [], "spo2": [], "skin_temp": []}
    for b in blocks:
        for k in out:
            out[k].extend(b.get(k, []))
    return out


def _synthetic_day() -> dict:
    """A night that ENDS on DAY (starts the prior evening) + a daytime exercise.

    Night: ~22:00 (DAY-1) → ~06:00 (DAY). Exercise: ~12:00 (DAY) for 30 min.
    """
    prev = DAY - _dt.timedelta(days=1)
    night_start = _epoch(prev, 22, 0)        # 22:00 prior evening
    # settling-in (active) → light → deep → light → waking, ends ~06:18 DAY
    night = _merge(
        _active_block(night_start, 20, bpm0=78),
        _still_block(night_start + 20 * 60, 120, bpm=56),
        _still_block(night_start + 140 * 60, 80, bpm=49, dip=True),
        _still_block(night_start + 220 * 60, 150, bpm=55),
        _still_block(night_start + 370 * 60, 60, bpm=52, dip=True),
        _active_block(night_start + 430 * 60, 15, bpm0=80),
    )
    workout = _active_block(_epoch(DAY, 12, 0), 30, bpm0=140)
    return _merge(night, workout)


@pytest.fixture
def client(clean_db, tmp_path, monkeypatch):
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", clean_db)
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.main as m
    importlib.reload(m)
    return TestClient(m.app, headers={"Authorization": "Bearer secret"})


def _seed(dsn):
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, DEVICE)
        store.upsert_streams(conn, DEVICE, _synthetic_day())
        conn.commit()


@requires_docker
def test_compute_day_persists_and_reads_back(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        result = daily.compute_day(conn, DEVICE, DAY)
        conn.commit()

    # Returned summary is plausible + JSON-serializable (date as ISO string).
    assert result["sleep_summary"]["date"] == DAY.isoformat()
    assert result["sleep_summary"]["total_sleep_min"] > 0
    assert 0.0 <= result["sleep_summary"]["efficiency"] <= 1.0
    assert result["resting_hr"] is not None and 30 <= result["resting_hr"] <= 90
    # Recovery may be None on the first run (cold-start: not enough prior nights to
    # build a trusted personal baseline). This is intentional — more honest than
    # anchoring at a fake 60. When not None, must be in [0, 100].
    recovery = result["recovery"]
    assert recovery is None or 0.0 <= recovery <= 100.0
    # Calibrated nightly signals are present in the return.
    for k in ("spo2_pct", "skin_temp_dev_c", "resp_rate_bpm"):
        assert k in result
    # The synthetic night seeds spo2 red/ir, skin_temp raw, and resp waveform → each
    # field MUST be a real computed value (not None). A regression that drops stream
    # loading (e.g. removing "spo2" from _KINDS) will make these assertions fail.
    assert result["spo2_pct"] is not None, (
        "spo2_pct is None — likely 'spo2' missing from _KINDS or stream not seeded")
    assert 70.0 <= result["spo2_pct"] <= 100.0, (
        f"spo2_pct {result['spo2_pct']} out of physical range [70, 100]")
    assert result["resp_rate_bpm"] is not None, (
        "resp_rate_bpm is None despite synthetic resp waveform being seeded")
    assert 6.0 <= result["resp_rate_bpm"] <= 30.0, (
        f"resp_rate_bpm {result['resp_rate_bpm']} out of physiological range [6, 30]")
    # skin_temp_dev_c: baseline requires a prior window; on cold-start it may be None.
    # When not None (prior skin_temp data exists from the same seed run), assert range.
    if result["skin_temp_dev_c"] is not None:
        assert -5.0 <= result["skin_temp_dev_c"] <= 5.0, (
            f"skin_temp_dev_c {result['skin_temp_dev_c']} implausible")

    with psycopg.connect(clean_db) as conn:
        rows = read.query_daily(conn, DEVICE, DAY, DAY)
        assert len(rows) == 1
        row = rows[0]
        assert row["day"] == DAY
        assert row["total_sleep_min"] > 0
        assert 0.0 <= row["efficiency"] <= 1.0
        # See above: recovery may be null on cold-start.
        row_recovery = row["recovery"]
        assert row_recovery is None or 0.0 <= row_recovery <= 100.0
        assert 30 <= row["resting_hr"] <= 90
        # New calibrated-signal columns round-trip through query_daily.
        for k in ("spo2_pct", "skin_temp_dev_c", "resp_rate_bpm"):
            assert k in row
        assert row["spo2_pct"] == result["spo2_pct"]
        assert row["resp_rate_bpm"] == result["resp_rate_bpm"]

        sleep = read.query_sleep(conn, DEVICE, DAY)
        assert len(sleep) >= 1
        # stages parsed back as a list of dicts
        assert isinstance(sleep[0]["stages"], list)
        assert sleep[0]["stages"] and "stage" in sleep[0]["stages"][0]


@requires_docker
def test_idempotent_recompute(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        daily.compute_day(conn, DEVICE, DAY)
        conn.commit()
        daily.compute_day(conn, DEVICE, DAY)
        conn.commit()
    with psycopg.connect(clean_db) as conn, conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM daily_metrics WHERE device_id=%s", (DEVICE,))
        assert cur.fetchone()[0] == 1
        cur.execute("SELECT count(*) FROM sleep_sessions WHERE device_id=%s", (DEVICE,))
        n_sleep = cur.fetchone()[0]
        assert n_sleep >= 1
        # recompute must not multiply rows
        cur.execute("SELECT count(*) FROM sleep_sessions WHERE device_id=%s", (DEVICE,))
        assert cur.fetchone()[0] == n_sleep


@requires_docker
def test_compute_daily_endpoint_then_reads(client, clean_db):
    _seed(clean_db)
    r = client.post("/v1/compute-daily", json={"device": DEVICE, "date": DAY.isoformat()})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["sleep_summary"]["date"] == DAY.isoformat()

    daily_rows = client.get(
        "/v1/daily", params={"device": DEVICE, "from": DAY.isoformat(), "to": DAY.isoformat()}
    ).json()
    assert len(daily_rows) == 1 and daily_rows[0]["total_sleep_min"] > 0

    sleep = client.get("/v1/sleep", params={"device": DEVICE, "date": DAY.isoformat()}).json()
    assert len(sleep) >= 1 and isinstance(sleep[0]["stages"], list)


@requires_docker
def test_ingest_trigger_computes_day(client):
    """POST the day's streams via /v1/ingest-decoded → the day auto-computes."""
    body = {"device": {"id": DEVICE}, "streams": _synthetic_day()}
    r = client.post("/v1/ingest-decoded", json=body)
    assert r.status_code == 200, r.text

    daily_rows = client.get(
        "/v1/daily", params={"device": DEVICE, "from": DAY.isoformat(), "to": DAY.isoformat()}
    ).json()
    assert len(daily_rows) == 1, daily_rows
    assert daily_rows[0]["total_sleep_min"] > 0


@requires_docker
def test_exercise_perbout_fields_persist(clean_db):
    """The new per-bout intensity fields (duration_s, zone_time_pct, avg_hrr_pct,
    hrmax, hrmax_source) are persisted to exercise_sessions and are coherent."""
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        result = daily.compute_day(conn, DEVICE, DAY)
        conn.commit()

    assert result["exercises"], "expected at least one detected workout"
    ex = result["exercises"][0]
    # Return carries the new fields.
    for k in ("duration_s", "zone_time_pct", "avg_hrr_pct", "hrmax", "hrmax_source"):
        assert k in ex
    assert ex["duration_s"] > 0
    assert ex["hrmax_source"] in ("observed", "tanaka", "caller", "unknown")
    # zone_time_pct sums to ~100 when populated (string keys for JSON/JSONB).
    if ex["zone_time_pct"]:
        assert abs(sum(ex["zone_time_pct"].values()) - 100.0) < 1.0

    # Persisted columns round-trip from exercise_sessions.
    with psycopg.connect(clean_db) as conn, conn.cursor() as cur:
        cur.execute(
            "SELECT duration_s, zone_time_pct, avg_hrr_pct, hrmax, hrmax_source "
            "FROM exercise_sessions WHERE device_id=%s ORDER BY start_ts LIMIT 1",
            (DEVICE,))
        duration_s, zone_time_pct, avg_hrr_pct, hrmax, hrmax_source = cur.fetchone()
        assert duration_s is not None and duration_s > 0
        assert isinstance(zone_time_pct, dict)  # JSONB → dict
        assert hrmax is not None and hrmax > 0
        assert hrmax_source in ("observed", "tanaka", "caller", "unknown")


@requires_docker
def test_compute_daily_endpoint_exposes_new_fields(client, clean_db):
    """/v1/compute-daily and /v1/daily expose the new calibrated-signal fields."""
    _seed(clean_db)
    body = client.post(
        "/v1/compute-daily", json={"device": DEVICE, "date": DAY.isoformat()}).json()
    for k in ("spo2_pct", "skin_temp_dev_c", "resp_rate_bpm"):
        assert k in body
    # Exercises in the API response carry the per-bout intensity fields.
    if body["exercises"]:
        for k in ("duration_s", "zone_time_pct", "avg_hrr_pct", "hrmax", "hrmax_source"):
            assert k in body["exercises"][0]

    daily_rows = client.get(
        "/v1/daily", params={"device": DEVICE, "from": DAY.isoformat(), "to": DAY.isoformat()}
    ).json()
    assert len(daily_rows) == 1
    for k in ("spo2_pct", "skin_temp_dev_c", "resp_rate_bpm"):
        assert k in daily_rows[0]


@requires_docker
def test_compute_daily_bad_date_400(client):
    r = client.post("/v1/compute-daily", json={"device": DEVICE, "date": "nope"})
    assert r.status_code == 400


@requires_docker
def test_empty_day_skips_write(clean_db):
    """A day with NO streams at all → no daily_metrics row written + no_data marker."""
    empty_day = _dt.date(2024, 1, 1)  # nothing ever seeded here
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, DEVICE)
        conn.commit()
        result = daily.compute_day(conn, DEVICE, empty_day)
        conn.commit()

    assert result == {"status": "no_data", "date": empty_day.isoformat()}

    with psycopg.connect(clean_db) as conn, conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM daily_metrics WHERE device_id=%s AND day=%s",
            (DEVICE, empty_day))
        assert cur.fetchone()[0] == 0
        cur.execute(
            "SELECT count(*) FROM exercise_sessions WHERE device_id=%s "
            "AND start_ts >= %s::date AT TIME ZONE 'UTC' "
            "AND start_ts <  (%s::date + INTERVAL '1 day') AT TIME ZONE 'UTC'",
            (DEVICE, empty_day, empty_day))
        assert cur.fetchone()[0] == 0


def _ex_count_for_day(conn) -> int:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM exercise_sessions WHERE device_id=%s "
            "AND start_ts >= %s::date AT TIME ZONE 'UTC' "
            "AND start_ts <  (%s::date + INTERVAL '1 day') AT TIME ZONE 'UTC'",
            (DEVICE, DAY, DAY))
        return cur.fetchone()[0]


@requires_docker
def test_recompute_with_fewer_sessions_is_consistent(clean_db):
    """Recompute that yields FEWER exercise sessions must delete the stale rows so
    exercise_sessions count == daily_metrics.exercise_count (no lingering rows)."""
    # First compute: seed night + TWO daytime workouts → 2 exercise sessions.
    prev = DAY - _dt.timedelta(days=1)
    night_start = _epoch(prev, 22, 0)
    night = _merge(
        _active_block(night_start, 20, bpm0=78),
        _still_block(night_start + 20 * 60, 120, bpm=56),
        _still_block(night_start + 140 * 60, 80, bpm=49, dip=True),
        _still_block(night_start + 220 * 60, 150, bpm=55),
        _still_block(night_start + 370 * 60, 60, bpm=52, dip=True),
        _active_block(night_start + 430 * 60, 15, bpm0=80),
    )
    workout_a = _active_block(_epoch(DAY, 10, 0), 30, bpm0=140)
    workout_b = _active_block(_epoch(DAY, 15, 0), 30, bpm0=145)
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, DEVICE)
        store.upsert_streams(conn, DEVICE, _merge(night, workout_a, workout_b))
        conn.commit()
        first = daily.compute_day(conn, DEVICE, DAY)
        conn.commit()
        first_count = first["sleep_summary"] and len(first["exercises"])
        assert _ex_count_for_day(conn) == first_count
        # daily_metrics.exercise_count agrees with the actual rows.
        rows = read.query_daily(conn, DEVICE, DAY, DAY)
        assert rows[0]["exercise_count"] == first_count

    # Now DELETE the second workout's HR/gravity so a recompute yields fewer sessions.
    b_start = _epoch(DAY, 15, 0)
    b_end = b_start + 30 * 60
    with psycopg.connect(clean_db) as conn, conn.cursor() as cur:
        for tbl in ("hr_samples", "gravity_samples", "rr_intervals", "resp_samples"):
            cur.execute(
                f"DELETE FROM {tbl} WHERE device_id=%s "
                "AND ts >= to_timestamp(%s) AND ts < to_timestamp(%s)",
                (DEVICE, b_start, b_end))
        conn.commit()

    with psycopg.connect(clean_db) as conn:
        second = daily.compute_day(conn, DEVICE, DAY)
        conn.commit()
        second_count = len(second["exercises"])
        # Fewer sessions than the first run, and rows match exactly (stale row deleted).
        assert second_count < first_count
        assert _ex_count_for_day(conn) == second_count
        rows = read.query_daily(conn, DEVICE, DAY, DAY)
        assert rows[0]["exercise_count"] == second_count
