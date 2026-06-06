"""
Tests for:
  - Profile storage (store.upsert_profile / read.query_profile)
  - GET/POST /v1/profile
  - Calorie estimation (analysis.calories.estimate_bout_calories)
  - detect_exercises with profile=None → calories None (no regression)
  - detect_exercises with profile → calories populated
  - GET /v1/workouts
  - POST /v1/backfill-workouts (smoke test)

DB tests use the timescale_dsn / clean_db fixtures from conftest.py and are
skipped automatically when Docker is unavailable (requires_docker marker).
"""
from __future__ import annotations

import importlib
import math
import psycopg
import pytest

from tests.conftest import requires_docker
from app import store
from app.analysis.calories import estimate_bout_calories, _resolve_coeffs, _COEFFS
from app.analysis.exercise import detect_exercises, ExerciseSession

T0 = 1_700_000_000.0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _hr_block(start: float, n: int, bpm: int, step_s: float = 1.0) -> list[dict]:
    return [{"ts": start + i * step_s, "bpm": bpm} for i in range(n)]


def _gravity_active(start: float, n: int, step_s: float = 1.0,
                    amp: float = 1.0) -> list[dict]:
    out = []
    for i in range(n):
        v = amp if i % 2 == 0 else -amp
        out.append({"ts": start + i * step_s, "x": v, "y": 0.0, "z": 0.0})
    return out


# ---------------------------------------------------------------------------
# Unit tests: calorie formula (pure, no DB)
# ---------------------------------------------------------------------------

def test_male_coeffs_match_keytel():
    """male workout coefficients match the published Keytel 2005 coefficients."""
    c = _COEFFS["male"]
    assert abs(c["workout_hr"] - 0.6309) < 1e-6
    assert abs(c["workout_weight"] - 0.1988) < 1e-6
    assert abs(c["workout_age"] - 0.2017) < 1e-6
    assert abs(c["workout_alpha"] - (-55.0969)) < 1e-4


def test_female_coeffs_match_keytel():
    """female workout coefficients match the published Keytel 2005 coefficients."""
    c = _COEFFS["female"]
    assert abs(c["workout_hr"] - 0.4472) < 1e-6
    assert abs(c["workout_weight"] - (-0.1263)) < 1e-6
    assert abs(c["workout_age"] - 0.0740) < 1e-6
    assert abs(c["workout_alpha"] - (-20.4022)) < 1e-4


def test_nonbinary_is_average_of_male_female():
    """nonbinary coefficients are the arithmetic mean of male + female."""
    m = _COEFFS["male"]
    f = _COEFFS["female"]
    nb = _COEFFS["nonbinary"]
    for key in ("workout_hr", "workout_weight", "workout_age", "workout_alpha",
                "resting_alpha", "resting_weight", "resting_age"):
        expected = (m[key] + f[key]) / 2.0
        assert abs(nb[key] - expected) < 1e-4, f"nonbinary {key}: {nb[key]} != avg {expected}"


def test_resolve_coeffs_unknown_falls_back_to_nonbinary():
    assert _resolve_coeffs(None) is _COEFFS["nonbinary"]
    assert _resolve_coeffs("") is _COEFFS["nonbinary"]
    assert _resolve_coeffs("unknown") is _COEFFS["nonbinary"]
    assert _resolve_coeffs("other") is _COEFFS["nonbinary"]


def test_estimate_bout_calories_deterministic():
    """
    Known HR series + known profile → expected kcal within tolerance.

    Profile: male, 75 kg, 180 cm, age 30; hrmax=185, resting_hr=55.
    Active threshold = 55 + 0.30*(185-55) = 94 bpm.
    60 samples at HR=150 (all active):
        rate = (0.6309*150 + 0.1988*75 + 0.2017*30 - 55.0969) / 251.04
             = (94.635 + 14.91 + 6.051 - 55.0969) / 251.04
             = 60.4991 / 251.04
             ≈ 0.24099 kcal/s
        total = 0.24099 * 60 ≈ 14.4597 kcal
    """
    profile = {"sex": "male", "weight_kg": 75.0, "height_cm": 180.0, "age": 30}
    hr_samples = [{"ts": float(i), "bpm": 150} for i in range(60)]
    kcal, kj = estimate_bout_calories(
        hr_samples, profile=profile, hrmax=185.0, resting_hr=55.0
    )
    expected_kcal = 14.4596  # computed by hand above
    assert abs(kcal - expected_kcal) < 0.05, f"kcal={kcal}, expected≈{expected_kcal}"
    assert abs(kj - kcal * 4.184) < 1e-6


def test_estimate_bout_calories_resting_only():
    """HR below active threshold → only resting rate applied."""
    profile = {"sex": "male", "weight_kg": 75.0, "height_cm": 180.0, "age": 30}
    # HR=60 < threshold(94) → all resting
    hr_samples = [{"ts": float(i), "bpm": 60} for i in range(60)]
    kcal, kj = estimate_bout_calories(
        hr_samples, profile=profile, hrmax=185.0, resting_hr=55.0
    )
    # BMR male 75/180/30: 88.362 + 13.397*75 + 479.9*1.80 - 5.677*30
    # = 88.362 + 1004.775 + 863.82 - 170.31 = 1786.647 kcal/day
    # resting/s = 1786.647 / 86400 ≈ 0.020679 kcal/s * 60s ≈ 1.2407 kcal
    expected_kcal = (1786.647 / 86400.0) * 60
    assert abs(kcal - expected_kcal) < 0.01, f"kcal={kcal}, expected≈{expected_kcal}"


def test_estimate_bout_calories_female():
    """Female coefficients produce different (lower) calories than male for same inputs."""
    profile_m = {"sex": "male", "weight_kg": 65.0, "height_cm": 170.0, "age": 25}
    profile_f = {"sex": "female", "weight_kg": 65.0, "height_cm": 170.0, "age": 25}
    hr_samples = [{"ts": float(i), "bpm": 155} for i in range(120)]
    kcal_m, _ = estimate_bout_calories(hr_samples, profile=profile_m, hrmax=190.0, resting_hr=60.0)
    kcal_f, _ = estimate_bout_calories(hr_samples, profile=profile_f, hrmax=190.0, resting_hr=60.0)
    # Male Keytel coefficients produce higher calories at the same HR (known from the paper).
    assert kcal_m > kcal_f, f"Expected male > female; got m={kcal_m:.2f}, f={kcal_f:.2f}"
    # Sanity: female > 0 at 155 bpm (well above resting)
    assert kcal_f > 0


def test_estimate_bout_calories_empty_returns_zero():
    profile = {"sex": "male", "weight_kg": 70.0, "height_cm": 175.0, "age": 28}
    kcal, kj = estimate_bout_calories([], profile=profile, hrmax=185.0, resting_hr=60.0)
    assert kcal == 0.0 and kj == 0.0


# ---------------------------------------------------------------------------
# Unit tests: detect_exercises with profile=None → no regression
# ---------------------------------------------------------------------------

def test_detect_exercises_no_profile_calories_none():
    """profile=None → calories_kcal and calories_kj are None on all sessions.

    Uses bpm=140 (zone 2: pct_hrr≈64% with resting=60, max=185) so the bout
    passes the MIN_INTENSITY_Z2PLUS filter.  bpm=130 was used previously but
    sits in zone 1 (pct_hrr≈56%) and is correctly rejected by the intensity
    qualification filter introduced to suppress low-effort false positives.
    """
    n = 400  # > MIN_EXERCISE_MIN*60 = 300s
    streams = {
        "hr": _hr_block(T0, n, bpm=140),
        "gravity": _gravity_active(T0, n),
    }
    sessions = detect_exercises(streams, resting_hr=60.0, max_hr=185.0)
    assert sessions, "expected at least one session"
    for s in sessions:
        assert s.calories_kcal is None, "calories_kcal should be None without profile"
        assert s.calories_kj is None, "calories_kj should be None without profile"


def test_detect_exercises_no_profile_other_fields_unchanged():
    """Verify that avg_hr/peak_hr/duration_s/strain are not affected by the profile=None default.

    Uses bpm=140 (zone 2: pct_hrr≈64%) so the bout passes the intensity filter.
    """
    n = 400
    bpm = 140
    streams = {
        "hr": _hr_block(T0, n, bpm=bpm),
        "gravity": _gravity_active(T0, n),
    }
    # Without profile
    sessions_no_profile = detect_exercises(streams, resting_hr=60.0, max_hr=185.0)
    # With profile=None explicitly
    sessions_none = detect_exercises(streams, resting_hr=60.0, max_hr=185.0, profile=None)
    assert len(sessions_no_profile) == len(sessions_none)
    for a, b in zip(sessions_no_profile, sessions_none):
        assert abs(a.avg_hr - b.avg_hr) < 1e-9
        assert a.peak_hr == b.peak_hr
        assert abs(a.duration_s - b.duration_s) < 1e-6


def test_detect_exercises_with_profile_calories_populated():
    """profile provided → calories_kcal/calories_kj are floats > 0."""
    n = 400
    profile = {"sex": "male", "weight_kg": 80.0, "height_cm": 182.0, "age": 32}
    streams = {
        "hr": _hr_block(T0, n, bpm=140),
        "gravity": _gravity_active(T0, n),
    }
    sessions = detect_exercises(streams, resting_hr=60.0, max_hr=185.0, profile=profile)
    assert sessions, "expected at least one session"
    for s in sessions:
        assert s.calories_kcal is not None and s.calories_kcal > 0
        assert s.calories_kj is not None and s.calories_kj > 0
        assert abs(s.calories_kj - s.calories_kcal * 4.184) < 0.001


# ---------------------------------------------------------------------------
# DB tests: profile upsert + read round-trip
# ---------------------------------------------------------------------------

@requires_docker
def test_profile_upsert_and_read(clean_db):
    """store.upsert_profile + read.query_profile round-trip."""
    from app import read as _read
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devProf")
        store.upsert_profile(conn, "devProf", height_cm=178.0, weight_kg=72.5, age=29, sex="male")
        conn.commit()
        row = _read.query_profile(conn, "devProf")
    assert row is not None
    assert abs(row["height_cm"] - 178.0) < 1e-4
    assert abs(row["weight_kg"] - 72.5) < 1e-4
    assert row["age"] == 29
    assert row["sex"] == "male"


@requires_docker
def test_profile_upsert_overwrites(clean_db):
    """Second upsert overwrites all fields."""
    from app import read as _read
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devProf2")
        store.upsert_profile(conn, "devProf2", height_cm=170.0, weight_kg=65.0, age=25, sex="female")
        conn.commit()
        store.upsert_profile(conn, "devProf2", height_cm=171.0, weight_kg=66.0, age=26, sex="nonbinary")
        conn.commit()
        row = _read.query_profile(conn, "devProf2")
    assert abs(row["height_cm"] - 171.0) < 1e-4
    assert row["age"] == 26
    assert row["sex"] == "nonbinary"


@requires_docker
def test_profile_read_missing_returns_none(clean_db):
    """query_profile returns None for a device with no profile."""
    from app import read as _read
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devNoProf")
        conn.commit()
        row = _read.query_profile(conn, "devNoProf")
    assert row is None


# ---------------------------------------------------------------------------
# HTTP tests: GET/POST /v1/profile
# ---------------------------------------------------------------------------

@pytest.fixture
def client(clean_db, tmp_path, monkeypatch):
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", clean_db)
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.main as m
    importlib.reload(m)
    from fastapi.testclient import TestClient
    return TestClient(m.app, headers={"Authorization": "Bearer secret"})


@requires_docker
def test_get_profile_missing_returns_empty(client):
    r = client.get("/v1/profile", params={"device": "devX"})
    assert r.status_code == 200
    assert r.json() == {}


@requires_docker
def test_post_and_get_profile(client, clean_db):
    body = {"device": "devP1", "height_cm": 175.0, "weight_kg": 70.0, "age": 30, "sex": "male"}
    r = client.post("/v1/profile", json=body)
    assert r.status_code == 200
    data = r.json()
    assert abs(data["height_cm"] - 175.0) < 1e-4
    assert data["sex"] == "male"
    assert data["device_id"] == "devP1"

    # GET returns same data
    r2 = client.get("/v1/profile", params={"device": "devP1"})
    assert r2.status_code == 200
    assert r2.json()["weight_kg"] - 70.0 < 1e-4


@requires_docker
def test_post_profile_invalid_sex_422(client):
    body = {"device": "devP2", "sex": "robot"}
    r = client.post("/v1/profile", json=body)
    assert r.status_code == 422


@requires_docker
def test_post_profile_null_sex_ok(client):
    body = {"device": "devP3", "height_cm": 160.0, "weight_kg": 55.0, "age": 22, "sex": None}
    r = client.post("/v1/profile", json=body)
    assert r.status_code == 200
    assert r.json()["sex"] is None


@requires_docker
def test_profile_auth_required(client):
    r = client.get("/v1/profile", params={"device": "devX"},
                   headers={"Authorization": ""})
    assert r.status_code == 401
    r2 = client.post("/v1/profile", json={"device": "devX"},
                     headers={"Authorization": ""})
    assert r2.status_code == 401


# ---------------------------------------------------------------------------
# DB tests: exercise_sessions with calories persisted
# ---------------------------------------------------------------------------

@requires_docker
def test_upsert_exercise_sessions_with_calories(clean_db):
    """calories_kcal / calories_kj round-trip through store + DB."""
    from app import read as _read
    import datetime
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devExCal")
        store.upsert_exercise_sessions(conn, "devExCal", [{
            "start": T0,
            "end": T0 + 1800,
            "avg_hr": 145.0,
            "peak_hr": 172,
            "strain": 8.5,
            "kind": None,
            "duration_s": 1800,
            "zone_time_pct": {"0": 10.0, "1": 20.0, "2": 30.0, "3": 25.0, "4": 10.0, "5": 5.0},
            "avg_hrr_pct": 62.3,
            "hrmax": 185.0,
            "hrmax_source": "observed",
            "calories_kcal": 350.5,
            "calories_kj": 1466.5,
        }])
        conn.commit()
        import datetime as dt
        start_date = dt.date.fromtimestamp(T0)
        rows = _read.query_workouts(conn, "devExCal", start_date, start_date)
    assert len(rows) == 1
    assert abs(rows[0]["calories_kcal"] - 350.5) < 0.01
    assert abs(rows[0]["calories_kj"] - 1466.5) < 0.01


@requires_docker
def test_upsert_exercise_sessions_no_calories(clean_db):
    """calories_kcal/calories_kj are NULL in DB when not provided."""
    from app import read as _read
    import datetime as dt
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devExNoCal")
        store.upsert_exercise_sessions(conn, "devExNoCal", [{
            "start": T0,
            "end": T0 + 1800,
            "avg_hr": 140.0,
            "peak_hr": 165,
            "strain": 7.0,
            "kind": None,
            "duration_s": 1800,
            "zone_time_pct": {},
            "avg_hrr_pct": None,
            "hrmax": None,
            "hrmax_source": "unknown",
            # no calories keys → should persist as NULL
        }])
        conn.commit()
        start_date = dt.date.fromtimestamp(T0)
        rows = _read.query_workouts(conn, "devExNoCal", start_date, start_date)
    assert len(rows) == 1
    assert rows[0]["calories_kcal"] is None
    assert rows[0]["calories_kj"] is None


# ---------------------------------------------------------------------------
# HTTP tests: GET /v1/workouts
# ---------------------------------------------------------------------------

@requires_docker
def test_get_workouts_returns_seeded_sessions(client, clean_db):
    """GET /v1/workouts returns exercise_sessions including calories."""
    import datetime as dt
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devW1")
        store.upsert_exercise_sessions(conn, "devW1", [
            {
                "start": T0,
                "end": T0 + 3600,
                "avg_hr": 148.0,
                "peak_hr": 175,
                "strain": 9.2,
                "kind": None,
                "duration_s": 3600,
                "zone_time_pct": {"0": 5.0, "1": 15.0, "2": 25.0, "3": 30.0, "4": 20.0, "5": 5.0},
                "avg_hrr_pct": 68.5,
                "hrmax": 188.0,
                "hrmax_source": "observed",
                "calories_kcal": 520.0,
                "calories_kj": 2175.7,
            }
        ])
        conn.commit()

    day_str = dt.date.fromtimestamp(T0).isoformat()
    r = client.get("/v1/workouts", params={"device": "devW1", "from": day_str, "to": day_str})
    assert r.status_code == 200
    rows = r.json()
    assert len(rows) == 1
    assert abs(rows[0]["calories_kcal"] - 520.0) < 0.01
    assert abs(rows[0]["avg_hr"] - 148.0) < 0.01
    assert rows[0]["hrmax_source"] == "observed"


@requires_docker
def test_get_workouts_empty_range(client, clean_db):
    """GET /v1/workouts returns [] for a date range with no sessions."""
    r = client.get("/v1/workouts",
                   params={"device": "devW2", "from": "2020-01-01", "to": "2020-01-01"})
    assert r.status_code == 200
    assert r.json() == []


@requires_docker
def test_workouts_auth_required(client):
    import datetime as dt
    day_str = dt.date.fromtimestamp(T0).isoformat()
    r = client.get("/v1/workouts",
                   params={"device": "devW3", "from": day_str, "to": day_str},
                   headers={"Authorization": ""})
    assert r.status_code == 401
