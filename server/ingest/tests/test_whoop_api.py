"""
tests/test_whoop_api.py — TDD tests for app.whoop_api.

All tests are OFFLINE — no real network calls.  The WhoopClient is constructed
with an injected httpx.Client backed by httpx.MockTransport.

Coverage
--------
- Token refresh: access + refresh token parsed; rotating refresh persisted via callback.
- Recovery normalization: mocked JSON → correct GroundTruthDay fields + units.
- Sleep normalization: stage_summary milli → minutes helper; field mapping.
- Cycle normalization: strain, kJ, avg/max HR mapped from cycle score.
- Workout normalization: sport_name, avg/max HR, kilojoule.
- Pagination: two mocked pages joined via next_token.
- Export parser: physiological_cycles.csv + sleeps.csv → GroundTruthDay;
  workouts.csv → GroundTruthWorkout; missing-column robustness.
- Header normalization: variant header strings map to the same field.

Run offline:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_whoop_api.py -q
"""
from __future__ import annotations

import io
import json
import os
import textwrap
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import httpx
import pytest

from app.whoop_api.client import WhoopClient, TOKEN_URL, API_BASE, build_auth_url, exchange_code
from app.whoop_api.models import GroundTruthDay, GroundTruthWorkout
from app.whoop_api.export_parser import (
    _norm,
    parse_cycles_csv,
    parse_sleeps_csv,
    parse_workouts_csv,
)

# ---------------------------------------------------------------------------
# Test fixture paths
# ---------------------------------------------------------------------------

FIXTURES = Path(__file__).parent / "fixtures"
CYCLES_CSV   = FIXTURES / "physiological_cycles.csv"
SLEEPS_CSV   = FIXTURES / "sleeps.csv"
WORKOUTS_CSV = FIXTURES / "workouts.csv"


# ---------------------------------------------------------------------------
# Mock transport helpers
# ---------------------------------------------------------------------------

def _json_resp(body: Any, status_code: int = 200) -> httpx.Response:
    return httpx.Response(status_code, json=body)


def _make_client(routes: list[tuple[str, Any]]) -> httpx.Client:
    """Build an httpx.Client whose MockTransport replays a fixed response queue.

    Each element of routes is (url_substring, response_body_dict | httpx.Response).
    The transport matches the FIRST route whose url_substring appears in the request URL.
    Unmatched requests return 404.
    """
    def handler(request: httpx.Request) -> httpx.Response:
        url = str(request.url)
        for url_frag, body in routes:
            if url_frag in url:
                if isinstance(body, httpx.Response):
                    return body
                return _json_resp(body)
        return httpx.Response(404, json={"error": f"no mock for {url}"})

    transport = httpx.MockTransport(handler)
    return httpx.Client(transport=transport)


# ---------------------------------------------------------------------------
# Sample API payloads (exact field names from docs/research/05-whoop-api.md)
# ---------------------------------------------------------------------------

SAMPLE_TOKEN_RESP = {
    "access_token":  "access_abc123",
    "refresh_token": "refresh_xyz789",
    "expires_in":    3600,
    "token_type":    "bearer",
    "scope":         "read:recovery read:sleep read:cycles read:workout offline",
}

SAMPLE_CYCLE = {
    "id": 93845,
    "user_id": 10129,
    "created_at": "2026-05-20T11:00:00.000Z",
    "updated_at": "2026-05-20T14:00:00.000Z",
    "start": "2026-05-20T02:30:00.000Z",
    "end":   "2026-05-21T02:10:00.000Z",
    "timezone_offset": "-05:00",
    "score_state": "SCORED",
    "score": {
        "strain":             8.3,
        "kilojoule":          8288.297,
        "average_heart_rate": 72,
        "max_heart_rate":     165,
    },
}

SAMPLE_RECOVERY = {
    "cycle_id": 93845,
    "sleep_id": "ed5d3a98-0000-0000-0000-000000000001",
    "user_id":  10129,
    "created_at": "2026-05-20T11:25:44.774Z",
    "updated_at": "2026-05-20T14:25:44.774Z",
    "score_state": "SCORED",
    "score": {
        "user_calibrating":   False,
        "recovery_score":     72,
        "resting_heart_rate": 58,
        "hrv_rmssd_milli":    45.2,
        "spo2_percentage":    96.5,
        "skin_temp_celsius":  33.7,
    },
}

SAMPLE_SLEEP = {
    "id": "ed5d3a98-0000-0000-0000-000000000001",
    "cycle_id": 93845,
    "v1_id": 93845,
    "user_id": 10129,
    "created_at": "2026-05-20T11:00:00.000Z",
    "updated_at": "2026-05-20T14:00:00.000Z",
    "start": "2026-05-19T22:45:00.000Z",
    "end":   "2026-05-20T06:30:00.000Z",
    "timezone_offset": "-05:00",
    "nap": False,
    "score_state": "SCORED",
    "score": {
        "stage_summary": {
            "total_in_bed_time_milli":          28_800_000,  # 480 min
            "total_awake_time_milli":            3_600_000,  # 60 min
            "total_no_data_time_milli":                  0,
            "total_light_sleep_time_milli":     12_600_000,  # 210 min
            "total_slow_wave_sleep_time_milli":  5_400_000,  # 90 min
            "total_rem_sleep_time_milli":        7_200_000,  # 120 min
            "sleep_cycle_count":                         3,
            "disturbance_count":                        12,
        },
        "respiratory_rate":            15.2,
        "sleep_performance_percentage": 88.0,
        "sleep_consistency_percentage": 80.0,
        "sleep_efficiency_percentage":  87.5,
    },
}

SAMPLE_WORKOUT = {
    "id": "wk-uuid-0000-0000-0000-000000000001",
    "v1_id": 1043,
    "user_id": 10129,
    "created_at": "2026-05-20T11:00:00.000Z",
    "updated_at": "2026-05-20T14:00:00.000Z",
    "start": "2026-05-20T08:00:00.000Z",
    "end":   "2026-05-20T09:00:00.000Z",
    "timezone_offset": "-05:00",
    "sport_name": "running",
    "sport_id": 1,
    "score_state": "SCORED",
    "score": {
        "strain":             10.5,
        "average_heart_rate": 145,
        "max_heart_rate":     172,
        "kilojoule":          2511.0,
        "percent_recorded":   100,
        "distance_meter":     8000.0,
    },
}


# ---------------------------------------------------------------------------
# Helper: build a WhoopClient with a pre-loaded token (skips the refresh call)
# ---------------------------------------------------------------------------

def _client_with_token(routes: list[tuple[str, Any]],
                       on_refresh=None) -> WhoopClient:
    """Return a WhoopClient that already has a valid access token injected."""
    import time
    http = _make_client(routes)
    c = WhoopClient(
        client_id="test_id",
        client_secret="test_secret",
        refresh_token="rt_initial",
        on_token_refresh=on_refresh,
        http_client=http,
    )
    # Inject a non-expired access token so _refresh() is not called on first _get()
    c._access_token = "pre_loaded_access"
    c._token_expires_at = time.monotonic() + 3600
    return c


# ===========================================================================
# 1. Token refresh
# ===========================================================================

class TestTokenRefresh:
    """Verify that _refresh() parses access+refresh and rotates correctly."""

    def test_refresh_parses_access_and_refresh_tokens(self):
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt", http_client=http)
        c._refresh()
        assert c._access_token == "access_abc123"
        assert c._refresh_token == "refresh_xyz789"

    def test_refresh_token_is_rotated(self):
        """After refresh, the stored refresh token must be the NEW one from the response."""
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt", http_client=http)
        c._refresh()
        # old "old_rt" must be gone; new "refresh_xyz789" must be stored
        assert c._refresh_token != "old_rt"
        assert c._refresh_token == "refresh_xyz789"

    def test_on_token_refresh_callback_called_with_new_token(self):
        """The on_token_refresh callback receives the NEW refresh token."""
        persisted: list[str] = []
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt",
                        on_token_refresh=lambda rt: persisted.append(rt),
                        http_client=http)
        c._refresh()
        assert persisted == ["refresh_xyz789"]

    def test_on_token_refresh_not_called_when_none(self):
        """No crash when on_token_refresh is None."""
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt", on_token_refresh=None, http_client=http)
        c._refresh()  # must not raise

    def test_expires_in_sets_token_expiry(self):
        """expires_in from the response is used to gate future _ensure_token calls."""
        import time
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt", http_client=http)
        before = time.monotonic()
        c._refresh()
        after = time.monotonic()
        # expires_in=3600, minus 30s buffer; so expiry should be ~3570 s from now
        assert c._token_expires_at > after + 3560
        assert c._token_expires_at < before + 3601

    def test_ensure_token_triggers_refresh_when_no_token(self):
        """_ensure_token() calls _refresh() when no access token is set."""
        persisted: list[str] = []
        http = _make_client([(TOKEN_URL, SAMPLE_TOKEN_RESP)])
        c = WhoopClient("id", "secret", "old_rt",
                        on_token_refresh=lambda rt: persisted.append(rt),
                        http_client=http)
        token = c._ensure_token()
        assert token == "access_abc123"
        assert persisted == ["refresh_xyz789"]

    def test_401_triggers_refresh_and_retry(self):
        """A 401 from an API call causes a token refresh and one retry."""
        persisted: list[str] = []
        call_count = {"n": 0}

        def handler(request: httpx.Request) -> httpx.Response:
            url = str(request.url)
            if TOKEN_URL in url:
                return _json_resp(SAMPLE_TOKEN_RESP)
            if "/v2/cycle" in url:
                call_count["n"] += 1
                if call_count["n"] == 1:
                    return httpx.Response(401, json={"error": "Unauthorized"})
                # Second call succeeds
                return _json_resp({"records": [SAMPLE_CYCLE], "next_token": None})
            return httpx.Response(404, json={})

        transport = httpx.MockTransport(handler)
        http = httpx.Client(transport=transport)
        c = WhoopClient("id", "secret", "rt_initial",
                        on_token_refresh=lambda rt: persisted.append(rt),
                        http_client=http)
        # Pre-load a token so _ensure_token doesn't refresh before the first call
        import time
        c._access_token = "stale_token"
        c._token_expires_at = time.monotonic() + 3600

        result = c._get("/v2/cycle")
        assert result["records"][0]["id"] == 93845
        assert call_count["n"] == 2   # first attempt 401, second succeeds
        assert persisted == ["refresh_xyz789"]


# ===========================================================================
# 2. Recovery normalization
# ===========================================================================

class TestRecoveryNormalization:
    """Feed a mocked recovery JSON page → assert GroundTruthDay fields."""

    def _make_day(self) -> GroundTruthDay:
        c = _client_with_token([
            ("/v2/cycle",    {"records": [SAMPLE_CYCLE],    "next_token": None}),
            ("/v2/recovery", {"records": [SAMPLE_RECOVERY], "next_token": None}),
            ("/v2/activity/sleep",   {"records": [SAMPLE_SLEEP],    "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        # cycle starts 2026-05-20 02:30 UTC shifted by -05:00 → 2026-05-19 21:30 local
        # so the local calendar date is 2026-05-19
        assert len(days) == 1
        return next(iter(days.values()))

    def test_recovery_score(self):
        assert self._make_day().recovery_score == 72

    def test_resting_hr(self):
        assert self._make_day().resting_hr == 58

    def test_hrv_rmssd_milli_direct_from_api(self):
        """hrv_rmssd_milli comes straight from the API (ms, no conversion)."""
        day = self._make_day()
        assert day.hrv_rmssd_milli == pytest.approx(45.2)

    def test_spo2_percentage(self):
        assert self._make_day().spo2_percentage == pytest.approx(96.5)

    def test_skin_temp_celsius(self):
        assert self._make_day().skin_temp_celsius == pytest.approx(33.7)

    def test_cycle_id_populated(self):
        assert self._make_day().cycle_id == 93845

    def test_user_calibrating_false_when_not_set(self):
        """user_calibrating is False for a normal (non-calibrating) recovery."""
        assert self._make_day().user_calibrating is False

    def test_user_calibrating_true_when_flag_set(self):
        """user_calibrating is captured from recovery score.user_calibrating."""
        calibrating_recovery = {
            **SAMPLE_RECOVERY,
            "score": {**SAMPLE_RECOVERY["score"], "user_calibrating": True},
        }
        c = _client_with_token([
            ("/v2/cycle",    {"records": [SAMPLE_CYCLE],              "next_token": None}),
            ("/v2/recovery", {"records": [calibrating_recovery],      "next_token": None}),
            ("/v2/activity/sleep",   {"records": [SAMPLE_SLEEP],      "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        day   = next(iter(days.values()))
        assert day.user_calibrating is True


# ===========================================================================
# 3. Sleep normalization
# ===========================================================================

class TestSleepNormalization:
    def _make_day(self) -> GroundTruthDay:
        c = _client_with_token([
            ("/v2/cycle",  {"records": [SAMPLE_CYCLE],    "next_token": None}),
            ("/v2/recovery", {"records": [SAMPLE_RECOVERY], "next_token": None}),
            ("/v2/activity/sleep", {"records": [SAMPLE_SLEEP], "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        return next(iter(days.values()))

    def test_sleep_id(self):
        assert self._make_day().sleep_id == "ed5d3a98-0000-0000-0000-000000000001"

    def test_in_bed_milli(self):
        """in_bed_milli stored in raw milliseconds (not converted)."""
        day = self._make_day()
        assert day.in_bed_milli == 28_800_000

    def test_awake_milli(self):
        assert self._make_day().awake_milli == 3_600_000

    def test_sws_deep_milli(self):
        assert self._make_day().sws_deep_milli == 5_400_000

    def test_rem_milli(self):
        assert self._make_day().rem_milli == 7_200_000

    def test_light_milli(self):
        assert self._make_day().light_milli == 12_600_000

    def test_respiratory_rate(self):
        assert self._make_day().respiratory_rate == pytest.approx(15.2)

    def test_sleep_performance_pct(self):
        assert self._make_day().sleep_performance_pct == pytest.approx(88.0)

    def test_sleep_efficiency_pct(self):
        assert self._make_day().sleep_efficiency_pct == pytest.approx(87.5)

    def test_sleep_consistency_pct(self):
        assert self._make_day().sleep_consistency_pct == pytest.approx(80.0)

    def test_no_data_milli_populated(self):
        """no_data_milli is captured from stage_summary.total_no_data_time_milli."""
        day = self._make_day()
        assert day.no_data_milli == 0  # fixture has 0

    def test_total_sleep_min_property(self):
        """total_sleep_min = (in_bed - awake - no_data) / 60_000.
        Fixture: (28800000 - 3600000 - 0) / 60000 = 420."""
        day = self._make_day()
        assert day.total_sleep_min == pytest.approx(420.0)

    def test_total_sleep_min_subtracts_no_data(self):
        """total_sleep_min subtracts no_data time for an accurate TST."""
        sleep_with_gap = {
            **SAMPLE_SLEEP,
            "score": {
                **SAMPLE_SLEEP["score"],
                "stage_summary": {
                    **SAMPLE_SLEEP["score"]["stage_summary"],
                    "total_no_data_time_milli": 1_800_000,  # 30 min gap
                },
            },
        }
        c = _client_with_token([
            ("/v2/cycle",    {"records": [SAMPLE_CYCLE],          "next_token": None}),
            ("/v2/recovery", {"records": [SAMPLE_RECOVERY],       "next_token": None}),
            ("/v2/activity/sleep", {"records": [sleep_with_gap],  "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        day   = next(iter(days.values()))
        # (28800000 - 3600000 - 1800000) / 60000 = 390 min
        assert day.no_data_milli == 1_800_000
        assert day.total_sleep_min == pytest.approx(390.0)

    def test_deep_sleep_min_property(self):
        """sws_deep_milli=5_400_000 → 90 min."""
        assert self._make_day().deep_sleep_min == pytest.approx(90.0)

    def test_rem_sleep_min_property(self):
        """rem_milli=7_200_000 → 120 min."""
        assert self._make_day().rem_sleep_min == pytest.approx(120.0)

    def test_nap_is_excluded(self):
        """Nap sleeps (nap=True) must NOT become the primary sleep for a cycle."""
        nap = dict(SAMPLE_SLEEP)
        nap["id"]    = "nap-uuid"
        nap["nap"]   = True
        nap["start"] = "2026-05-20T14:00:00.000Z"
        nap["end"]   = "2026-05-20T14:45:00.000Z"
        nap["score"]["stage_summary"] = dict(SAMPLE_SLEEP["score"]["stage_summary"])
        nap["score"]["stage_summary"]["total_in_bed_time_milli"] = 2_700_000  # 45 min

        c = _client_with_token([
            ("/v2/cycle",    {"records": [SAMPLE_CYCLE], "next_token": None}),
            ("/v2/recovery", {"records": [SAMPLE_RECOVERY], "next_token": None}),
            # Return nap first (bigger in_bed), then real sleep
            ("/v2/activity/sleep", {"records": [nap, SAMPLE_SLEEP], "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        day   = next(iter(days.values()))
        # Primary sleep must be the non-nap one
        assert day.sleep_id == "ed5d3a98-0000-0000-0000-000000000001"


# ===========================================================================
# 4. Cycle / strain normalization
# ===========================================================================

class TestCycleNormalization:
    def _make_day(self) -> GroundTruthDay:
        c = _client_with_token([
            ("/v2/cycle",    {"records": [SAMPLE_CYCLE],    "next_token": None}),
            ("/v2/recovery", {"records": [],                "next_token": None}),
            ("/v2/activity/sleep",   {"records": [],                "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        return next(iter(days.values()))

    def test_day_strain(self):
        assert self._make_day().day_strain == pytest.approx(8.3)

    def test_kilojoule(self):
        assert self._make_day().kilojoule == pytest.approx(8288.297)

    def test_avg_hr(self):
        assert self._make_day().avg_hr == 72

    def test_max_hr(self):
        assert self._make_day().max_hr == 165

    def test_unscored_cycle_fields_are_none(self):
        """score_state != SCORED → strain/kJ/HR fields are all None."""
        unscored = dict(SAMPLE_CYCLE)
        unscored["score_state"] = "PENDING_SCORE"
        del unscored["score"]
        c = _client_with_token([
            ("/v2/cycle",    {"records": [unscored], "next_token": None}),
            ("/v2/recovery", {"records": [],          "next_token": None}),
            ("/v2/activity/sleep",   {"records": [],          "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)
        day   = next(iter(days.values()))
        assert day.day_strain is None
        assert day.kilojoule is None


# ===========================================================================
# 5. Workout normalization
# ===========================================================================

class TestWorkoutNormalization:
    def _make_workouts(self) -> list[GroundTruthWorkout]:
        c = _client_with_token([
            ("/v2/activity/workout", {"records": [SAMPLE_WORKOUT], "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        return c.ground_truth_workouts(start, end)

    def test_single_workout_returned(self):
        assert len(self._make_workouts()) == 1

    def test_workout_id(self):
        assert self._make_workouts()[0].workout_id == "wk-uuid-0000-0000-0000-000000000001"

    def test_sport_name(self):
        assert self._make_workouts()[0].sport_name == "running"

    def test_avg_hr(self):
        assert self._make_workouts()[0].avg_hr == 145

    def test_max_hr(self):
        assert self._make_workouts()[0].max_hr == 172

    def test_kilojoule(self):
        assert self._make_workouts()[0].kilojoule == pytest.approx(2511.0)

    def test_distance_meter(self):
        assert self._make_workouts()[0].distance_meter == pytest.approx(8000.0)

    def test_unscored_workout_fields_none(self):
        unscored = dict(SAMPLE_WORKOUT)
        unscored["score_state"] = "PENDING_SCORE"
        c = _client_with_token([
            ("/v2/activity/workout", {"records": [unscored], "next_token": None}),
        ])
        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 21, tzinfo=timezone.utc)
        ws    = c.ground_truth_workouts(start, end)
        assert ws[0].strain is None
        assert ws[0].avg_hr is None


# ===========================================================================
# 6. Pagination
# ===========================================================================

class TestPagination:
    """Two pages linked via next_token must both be consumed."""

    def test_two_pages_consumed(self):
        cycle_page2 = dict(SAMPLE_CYCLE)
        cycle_page2 = {**SAMPLE_CYCLE, "id": 93846,
                       "start": "2026-05-21T02:30:00.000Z",
                       "end":   "2026-05-22T02:10:00.000Z"}

        call_count = {"n": 0}

        def handler(request: httpx.Request) -> httpx.Response:
            url    = str(request.url)
            params = dict(request.url.params)
            if TOKEN_URL in url:
                return _json_resp(SAMPLE_TOKEN_RESP)
            if "/v2/cycle" in url:
                call_count["n"] += 1
                if "nextToken" not in params:
                    # First page — include a next_token cursor
                    return _json_resp({"records": [SAMPLE_CYCLE], "next_token": "page2cursor"})
                else:
                    # Second page — no more cursor
                    return _json_resp({"records": [cycle_page2], "next_token": None})
            if "/v2/recovery" in url:
                return _json_resp({"records": [], "next_token": None})
            if "/v2/activity/sleep" in url:
                return _json_resp({"records": [], "next_token": None})
            return httpx.Response(404, json={})

        http = httpx.Client(transport=httpx.MockTransport(handler))
        import time
        c = WhoopClient("id", "secret", "rt", http_client=http)
        c._access_token = "pre"
        c._token_expires_at = time.monotonic() + 3600

        start = datetime(2026, 5, 20, tzinfo=timezone.utc)
        end   = datetime(2026, 5, 22, tzinfo=timezone.utc)
        days  = c.ground_truth_days(start, end)

        assert call_count["n"] == 2
        assert len(days) == 2

    def test_repeated_next_token_stops_pagination(self):
        """If the server returns the same next_token twice, pagination must stop (no infinite loop)."""
        call_count = {"n": 0}

        def handler(request: httpx.Request) -> httpx.Response:
            if TOKEN_URL in str(request.url):
                return _json_resp(SAMPLE_TOKEN_RESP)
            if "/v2/cycle" in str(request.url):
                call_count["n"] += 1
                # Always return the same next_token — misbehaving server
                return _json_resp({"records": [SAMPLE_CYCLE], "next_token": "stuck_cursor"})
            return httpx.Response(404, json={})

        import time
        http = httpx.Client(transport=httpx.MockTransport(handler))
        c = WhoopClient("id", "secret", "rt", http_client=http)
        c._access_token = "pre"
        c._token_expires_at = time.monotonic() + 3600

        records = list(c.iter_cycles(
            datetime(2026, 5, 20, tzinfo=timezone.utc),
            datetime(2026, 5, 21, tzinfo=timezone.utc),
        ))
        # First call returns a record + next_token; second call also returns
        # next_token="stuck_cursor" (already seen) → guard breaks the loop.
        assert call_count["n"] == 2
        assert len(records) == 2  # one record from each call before guard fired

    def test_next_token_threaded_as_nextToken_param(self):
        """Request param is 'nextToken' (camelCase); response field is 'next_token'."""
        received_params: list[dict] = []

        def handler(request: httpx.Request) -> httpx.Response:
            params = dict(request.url.params)
            received_params.append(params)
            if TOKEN_URL in str(request.url):
                return _json_resp(SAMPLE_TOKEN_RESP)
            if "/v2/cycle" in str(request.url):
                if "nextToken" not in params:
                    return _json_resp({"records": [SAMPLE_CYCLE], "next_token": "tok2"})
                return _json_resp({"records": [], "next_token": None})
            return httpx.Response(404, json={})

        http = httpx.Client(transport=httpx.MockTransport(handler))
        import time
        c = WhoopClient("id", "secret", "rt", http_client=http)
        c._access_token = "pre"
        c._token_expires_at = time.monotonic() + 3600

        list(c.iter_cycles(
            datetime(2026, 5, 20, tzinfo=timezone.utc),
            datetime(2026, 5, 21, tzinfo=timezone.utc),
        ))

        # Second request must carry nextToken=tok2 (not next_token)
        second_params = received_params[1]
        assert "nextToken" in second_params
        assert second_params["nextToken"] == "tok2"


# ===========================================================================
# 7. Export parser — physiological_cycles.csv
# ===========================================================================

class TestCyclesCSVParser:
    """Parse the synthetic fixture and assert correct field extraction + unit conversion."""

    def test_three_days_parsed(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert len(days) == 3

    def test_dates_are_local_calendar_dates(self):
        days = parse_cycles_csv(CYCLES_CSV)
        # CSV "Cycle start time" = "2026-05-20 02:30:00" → date 2026-05-20
        assert date(2026, 5, 20) in days

    def test_recovery_score_parsed(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].recovery_score == 70

    def test_hrv_rmssd_milli_parsed(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].hrv_rmssd_milli == pytest.approx(50.0)

    def test_skin_temp_celsius(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].skin_temp_celsius == pytest.approx(33.0)

    def test_spo2_percentage(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].spo2_percentage == pytest.approx(97.0)

    def test_energy_kcal_converted_to_kj(self):
        """CSV 'Energy burned (cal)' is kcal; 1 kcal = 4.184 kJ."""
        days = parse_cycles_csv(CYCLES_CSV)
        day  = days[date(2026, 5, 20)]
        # fixture: 2000 cal → 2000 * 4.184 = 8368.0 kJ
        assert day.kilojoule == pytest.approx(2000 * 4.184, rel=1e-4)

    def test_stage_durations_converted_to_milli(self):
        """CSV stage durations are in minutes; parser converts to milliseconds."""
        days    = parse_cycles_csv(CYCLES_CSV)
        day     = days[date(2026, 5, 20)]
        # fixture: in_bed=480 min → 28_800_000 ms
        assert day.in_bed_milli == 480 * 60_000
        # awake=60 min → 3_600_000 ms
        assert day.awake_milli == 60 * 60_000
        # light=210 min → 12_600_000 ms
        assert day.light_milli == 210 * 60_000
        # deep=90 min → 5_400_000 ms
        assert day.sws_deep_milli == 90 * 60_000
        # rem=120 min → 7_200_000 ms
        assert day.rem_milli == 120 * 60_000

    def test_total_sleep_min_property(self):
        days = parse_cycles_csv(CYCLES_CSV)
        day  = days[date(2026, 5, 20)]
        # (480 - 60) = 420 min
        assert day.total_sleep_min == pytest.approx(420.0)

    def test_day_strain(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].day_strain == pytest.approx(8.0)

    def test_sleep_performance_pct(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].sleep_performance_pct == pytest.approx(90.0)

    def test_sleep_efficiency_pct(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].sleep_efficiency_pct == pytest.approx(87.5)

    def test_respiratory_rate(self):
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].respiratory_rate == pytest.approx(15.0)

    def test_cycle_id_is_none_for_csv(self):
        """CSV doesn't expose the WHOOP cycle_id integer."""
        days = parse_cycles_csv(CYCLES_CSV)
        assert days[date(2026, 5, 20)].cycle_id is None


# ===========================================================================
# 8. Export parser — sleeps.csv (nap handling)
# ===========================================================================

class TestSleepsCSVParser:
    def test_three_rows_parsed(self):
        records = parse_sleeps_csv(SLEEPS_CSV)
        assert len(records) == 3

    def test_nap_flagged(self):
        records = parse_sleeps_csv(SLEEPS_CSV)
        # Third row in fixture has Nap = " true"
        nap_rows = [r for r in records if r["is_nap"]]
        non_nap  = [r for r in records if not r["is_nap"]]
        assert len(nap_rows) == 1
        assert len(non_nap) == 2

    def test_non_nap_stage_durations_in_milli(self):
        records = parse_sleeps_csv(SLEEPS_CSV)
        first   = records[0]
        assert first["in_bed_milli"] == 480 * 60_000

    def test_respiratory_rate_non_nap(self):
        records = parse_sleeps_csv(SLEEPS_CSV)
        assert records[0]["respiratory_rate"] == pytest.approx(15.0)


# ===========================================================================
# 9. Export parser — workouts.csv
# ===========================================================================

class TestWorkoutsCSVParser:
    def test_two_workouts_parsed(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert len(ws) == 2

    def test_csv_workout_id_format(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].workout_id == "csv-0"
        assert ws[1].workout_id == "csv-1"

    def test_sport_name(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].sport_name == "Running"
        assert ws[1].sport_name == "Cycling"

    def test_energy_kcal_converted_to_kj(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        # 600 cal → 600 * 4.184 = 2510.4 kJ
        assert ws[0].kilojoule == pytest.approx(600 * 4.184, rel=1e-4)

    def test_avg_hr(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].avg_hr == 145

    def test_max_hr(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].max_hr == 172

    def test_distance_meter(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].distance_meter == pytest.approx(8000.0)

    def test_sport_id_none_for_csv(self):
        ws = parse_workouts_csv(WORKOUTS_CSV)
        assert ws[0].sport_id is None


# ===========================================================================
# 10. Missing-column robustness
# ===========================================================================

class TestMissingColumnRobustness:
    """Parsing a CSV with missing optional columns must produce None — not a crash."""

    def _write_minimal_csv(self, tmp_path: Path) -> Path:
        """CSV with only the mandatory date column; all metric columns absent."""
        p = tmp_path / "physiological_cycles.csv"
        p.write_text(
            "Cycle start time,Cycle end time\n"
            "2026-05-25 02:00:00,2026-05-26 02:00:00\n"
        )
        return p

    def test_all_metric_fields_none_when_columns_absent(self, tmp_path):
        p   = self._write_minimal_csv(tmp_path)
        days = parse_cycles_csv(p)
        day  = days[date(2026, 5, 25)]
        assert day.recovery_score is None
        assert day.hrv_rmssd_milli is None
        assert day.day_strain is None
        assert day.in_bed_milli is None
        assert day.respiratory_rate is None

    def test_workouts_csv_missing_distance(self, tmp_path):
        p = tmp_path / "workouts.csv"
        p.write_text(
            "Workout start time,Workout end time,Activity name,Activity Strain,"
            "Energy burned (cal),Max HR (bpm),Average HR (bpm)\n"
            "2026-05-25 08:00:00,2026-05-25 09:00:00,Running,8.5,500,160,130\n"
        )
        ws = parse_workouts_csv(p)
        assert len(ws) == 1
        assert ws[0].distance_meter is None
        assert ws[0].sport_name == "Running"

    def test_empty_cell_treated_as_none(self, tmp_path):
        p = tmp_path / "physiological_cycles.csv"
        p.write_text(
            "Cycle start time,Cycle end time,Recovery score %,Day Strain\n"
            "2026-05-25 02:00:00,2026-05-26 02:00:00,,\n"   # empty metric cells
        )
        days = parse_cycles_csv(p)
        day  = days[date(2026, 5, 25)]
        assert day.recovery_score is None
        assert day.day_strain is None


# ===========================================================================
# 11. Header normalization (_norm)
# ===========================================================================

class TestHeaderNorm:
    """Variant header strings must normalize to the same key."""

    def test_recovery_score_pct(self):
        assert _norm("Recovery score %") == "recovery_score_pct"

    def test_hrv_ms(self):
        assert _norm("Heart rate variability (ms)") == "heart_rate_variability_ms"

    def test_in_bed_min(self):
        assert _norm("In bed duration (min)") == "in_bed_duration_min"

    def test_deep_sws(self):
        assert _norm("Deep (SWS) duration (min)") == "deep_sws_duration_min"

    def test_blood_oxygen_pct(self):
        assert _norm("Blood oxygen %") == "blood_oxygen_pct"

    def test_skin_temp(self):
        assert _norm("Skin temp (celsius)") == "skin_temp_celsius"

    def test_leading_trailing_spaces_stripped(self):
        assert _norm("  Recovery score %  ") == "recovery_score_pct"

    def test_multiple_spaces_collapsed(self):
        assert _norm("Max  HR  (bpm)") == "max_hr_bpm"


# ===========================================================================
# 12. build_auth_url + exchange_code helpers
# ===========================================================================

class TestAuthHelpers:
    def test_build_auth_url_contains_required_params(self):
        url = build_auth_url("my_client", "https://example.com/cb", state="abcdefgh")
        assert "response_type=code" in url
        assert "client_id=my_client" in url
        assert "state=abcdefgh" in url
        assert "read%3Arecovery" in url or "read:recovery" in url

    def test_build_auth_url_generates_state_when_none(self):
        url1 = build_auth_url("cid", "https://example.com/cb")
        url2 = build_auth_url("cid", "https://example.com/cb")
        # Extract state params — must both exist and be different (random)
        from urllib.parse import urlparse, parse_qs
        qs1 = parse_qs(urlparse(url1).query)
        qs2 = parse_qs(urlparse(url2).query)
        assert "state" in qs1
        assert qs1["state"][0] != qs2["state"][0]

    def test_exchange_code_posts_correct_grant_type(self):
        import urllib.parse as up
        captured: list[dict] = []

        def handler(request: httpx.Request) -> httpx.Response:
            form = dict(up.parse_qsl(request.content.decode()))
            captured.append(form)
            return _json_resp(SAMPLE_TOKEN_RESP)

        http = httpx.Client(transport=httpx.MockTransport(handler))
        result = exchange_code("mycode", "cid", "csecret", "https://cb", http_client=http)
        assert result["access_token"] == "access_abc123"
        form = captured[0]
        assert form.get("grant_type") == "authorization_code"
        assert form.get("code") == "mycode"
        assert form.get("client_id") == "cid"
        assert form.get("redirect_uri") == "https://cb"
