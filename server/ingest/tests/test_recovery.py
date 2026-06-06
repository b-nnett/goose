"""
Tests for analysis.recovery — resting HR during sleep + recovery_score.

PURE (no DB). Run offline:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_recovery.py -q

Recovery formula changed in Task 7 (metrics-accuracy-overhaul):
  - OLD: fixed fractional-gain anchored at 60 (ratio space)
  - NEW: z-score + logistic composite (doc 03 §1.3)

The resting_hr() function is unchanged.
"""
from __future__ import annotations

import math

import pytest

from app.analysis.recovery import (
    BAND_RED_MAX,
    BAND_YELLOW_MAX,
    LOGISTIC_K,
    LOGISTIC_Z0,
    RECOVERY_BASELINE_SCORE,  # = 58.0, kept for compat
    RECOVERY_POPULATION_MEAN,
    W_HRV,
    W_RHR,
    W_RESP,
    W_SLEEP,
    recovery_band,
    recovery_score,
    resting_hr,
)
from app.analysis.baselines import (
    METRIC_CFG,
    BaselineState,
    fold_history,
)
from app.analysis.sleep import SleepSession

T0 = 1_700_000_000.0


def _session(start: float = T0, dur_s: float = 8 * 3600.0) -> SleepSession:
    return SleepSession(start=start, end=start + dur_s, efficiency=0.9)


# ---------------------------------------------------------------------------
# resting_hr (unchanged)
# ---------------------------------------------------------------------------

def test_resting_hr_finds_low_floor():
    hr = []
    for i in range(3 * 3600):
        if 3600 <= i < 7200:
            bpm = 48
        elif i < 3600:
            bpm = 55
        else:
            bpm = 60
        hr.append({"ts": T0 + i, "bpm": bpm})
    sess = _session(dur_s=3 * 3600)
    assert resting_hr({"hr": hr}, sess) == 48


def test_resting_hr_returns_int():
    hr = [{"ts": T0 + i, "bpm": 50 + (i % 3)} for i in range(600)]
    rv = resting_hr({"hr": hr}, _session(dur_s=600))
    assert isinstance(rv, int)


def test_resting_hr_rejects_single_dip():
    hr = [{"ts": T0 + i, "bpm": 55} for i in range(1800)]
    hr[900] = {"ts": T0 + 900, "bpm": 30}
    rv = resting_hr({"hr": hr}, _session(dur_s=1800))
    assert rv is not None and rv > 50


def test_resting_hr_no_samples_returns_none():
    assert resting_hr({"hr": []}, _session()) is None
    assert resting_hr({}, _session()) is None


def test_resting_hr_only_counts_in_window():
    hr = [{"ts": T0 - 10_000 + i, "bpm": 40} for i in range(600)]
    hr += [{"ts": T0 + i, "bpm": 60} for i in range(600)]
    rv = resting_hr({"hr": hr}, _session(dur_s=600))
    assert rv == 60


# ---------------------------------------------------------------------------
# recovery_score — helpers
# ---------------------------------------------------------------------------

def _trusted_baselines(
    hrv_mean: float = 55.0,
    rhr_mean: float = 58.0,
    resp_mean: float = 14.0,
    spread_hrv: float = 5.0,
    spread_rhr: float = 2.0,
    spread_resp: float = 0.5,
) -> dict:
    """Return a baselines dict with trusted BaselineState objects for all metrics."""
    def _state(mean, spread, cfg_key):
        return BaselineState(
            baseline=mean,
            spread=spread,
            n_valid=20,
            nights_since_update=0,
            status="trusted",
        )
    return {
        "hrv": _state(hrv_mean, spread_hrv, "hrv"),
        "resting_hr": _state(rhr_mean, spread_rhr, "resting_hr"),
        "resp": _state(resp_mean, spread_resp, "resp"),
    }


# ---------------------------------------------------------------------------
# recovery_score — new z-score + logistic formula
# ---------------------------------------------------------------------------

class TestRecoveryScoreFormula:
    """Verify the z-score + logistic math against the doc 03 §1.3 spec."""

    BASE = _trusted_baselines()

    def test_returns_float_in_range(self):
        r = recovery_score(55.0, 58.0, 14.0, self.BASE)
        assert isinstance(r, float)
        assert 0.0 <= r <= 100.0

    def test_at_baseline_is_near_population_mean(self):
        """At-baseline (all z≈0) should land at ~58% (WHOOP population average).

        With the logistic centered at Z0=-0.20 and k=1.6:
          score = 100 / (1 + exp(-1.6 * (0 - (-0.20)))) ≈ 58.0%
        But actual Z won't be exactly 0 because z_sleep is centered at 0.85 and
        we don't pass sleep_perf here, so only the 3 metric terms contribute.
        At baseline those z's = 0, so Z = 0 and score ≈ 58%.
        """
        # No sleep_perf → sleep term dropped; all metric z=0 → Z=0 → ≈58%
        r = recovery_score(55.0, 58.0, 14.0, self.BASE, sleep_perf=None)
        expected = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (0.0 - LOGISTIC_Z0)))
        assert abs(r - expected) < 0.5, f"Expected ~{expected:.1f}%, got {r:.2f}%"

    def test_logistic_Z0_centers_at_population_mean(self):
        """Verify the formula constant: 100/(1+exp(-k*(0-Z0))) ≈ 58%."""
        expected = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (0.0 - LOGISTIC_Z0)))
        assert abs(expected - RECOVERY_POPULATION_MEAN) < 0.1

    def test_high_hrv_raises_score(self):
        # HRV much higher than baseline → strongly positive z_hrv → high score
        r = recovery_score(90.0, 58.0, 14.0, self.BASE)
        assert r > 75.0

    def test_low_hrv_lowers_score(self):
        # HRV much lower than baseline → strongly negative z_hrv → low score
        r = recovery_score(30.0, 58.0, 14.0, self.BASE)
        assert r < 45.0

    def test_high_rhr_lowers_score(self):
        # Higher RHR than baseline → lower recovery
        low_rhr = recovery_score(55.0, 50.0, 14.0, self.BASE)
        high_rhr = recovery_score(55.0, 70.0, 14.0, self.BASE)
        assert low_rhr > high_rhr

    def test_high_resp_lowers_score(self):
        # Higher resp than baseline → lower recovery
        low_resp = recovery_score(55.0, 58.0, 11.0, self.BASE)
        high_resp = recovery_score(55.0, 58.0, 20.0, self.BASE)
        assert low_resp > high_resp

    def test_monotonic_in_hrv(self):
        lo = recovery_score(40.0, 58.0, 14.0, self.BASE)
        hi = recovery_score(70.0, 58.0, 14.0, self.BASE)
        assert hi > lo

    def test_monotonic_in_rhr(self):
        lo_rhr = recovery_score(55.0, 50.0, 14.0, self.BASE)
        hi_rhr = recovery_score(55.0, 70.0, 14.0, self.BASE)
        assert lo_rhr > hi_rhr

    def test_monotonic_in_resp(self):
        lo_resp = recovery_score(55.0, 58.0, 11.0, self.BASE)
        hi_resp = recovery_score(55.0, 58.0, 20.0, self.BASE)
        assert lo_resp > hi_resp

    def test_monotonic_in_sleep_perf(self):
        lo_sleep = recovery_score(55.0, 58.0, 14.0, self.BASE, sleep_perf=0.60)
        hi_sleep = recovery_score(55.0, 58.0, 14.0, self.BASE, sleep_perf=0.95)
        assert hi_sleep > lo_sleep

    def test_score_stays_0_to_100(self):
        # Extreme values should not break the logistic bounds.
        very_good = recovery_score(200.0, 30.0, 5.0, self.BASE, sleep_perf=1.0)
        very_bad = recovery_score(1.0, 150.0, 60.0, self.BASE, sleep_perf=0.0)
        assert 0.0 <= very_good <= 100.0
        assert 0.0 <= very_bad <= 100.0

    def test_sleep_perf_at_center_is_neutral(self):
        """sleep_perf = SLEEP_PERF_CENTER → z_sleep = 0 → same composite Z as no sleep term.

        When z_sleep = 0, the sleep term contributes 0*W_SLEEP to the weighted sum.
        Weight renormalization divides by (W_HRV+W_RHR+W_RESP+W_SLEEP) vs
        (W_HRV+W_RHR+W_RESP), but numerator is also 0 for both → same Z → same score.
        """
        from app.analysis.recovery import SLEEP_PERF_CENTER
        r_no_sleep = recovery_score(55.0, 58.0, 14.0, self.BASE, sleep_perf=None)
        r_neutral_sleep = recovery_score(55.0, 58.0, 14.0, self.BASE, sleep_perf=SLEEP_PERF_CENTER)
        # z_sleep = 0 at center, so both paths yield the same composite Z=0.
        assert abs(r_no_sleep - r_neutral_sleep) < 0.01


class TestRecoveryScoreColdStart:
    """Verify cold-start behavior: None when HRV baseline not yet trusted."""

    def test_calibrating_state_returns_none(self):
        """Fewer than MIN_NIGHTS_SEED valid nights → not usable → None."""
        few_nights = [55.0, 50.0]  # only 2 nights
        state = fold_history(few_nights, METRIC_CFG["hrv"])
        assert state.status == "calibrating"
        baselines = {
            "hrv": state,
            "resting_hr": BaselineState(58.0, 2.0, 2, 0, "calibrating"),
        }
        r = recovery_score(55.0, 58.0, 14.0, baselines)
        assert r is None, f"Expected None for cold-start, got {r}"

    def test_provisional_state_returns_score(self):
        """Between MIN_NIGHTS_SEED and MIN_NIGHTS_TRUST → 'provisional' → still returns score."""
        # 6 nights → provisional (4 ≤ n < 14)
        nights = [55.0] * 6
        state = fold_history(nights, METRIC_CFG["hrv"])
        assert state.status == "provisional"
        baselines = {
            "hrv": state,
            "resting_hr": BaselineState(58.0, 2.0, 6, 0, "provisional"),
        }
        r = recovery_score(55.0, 58.0, 14.0, baselines)
        assert r is not None
        assert 0.0 <= r <= 100.0

    def test_trusted_state_returns_score(self):
        """14+ nights → 'trusted' → returns score."""
        nights = [55.0] * 20
        state = fold_history(nights, METRIC_CFG["hrv"])
        assert state.status == "trusted"
        baselines = {
            "hrv": state,
            "resting_hr": BaselineState(58.0, 2.0, 20, 0, "trusted"),
        }
        r = recovery_score(55.0, 58.0, 14.0, baselines)
        assert r is not None

    def test_no_baselines_returns_none(self):
        r = recovery_score(55.0, 58.0, 14.0, {})
        assert r is None

    def test_none_baselines_returns_none(self):
        r = recovery_score(55.0, 58.0, 14.0, None)
        assert r is None


class TestRecoveryScoreLegacyCompat:
    """Verify backward-compat with old plain-float dict baselines."""

    BASE_DICT = {"hrv": 55.0, "resting_hr": 58.0, "resp": 14.0}

    def test_plain_float_dict_returns_score(self):
        """Old-style plain-float dict should still produce a score (not None)."""
        # Plain floats are treated as legacy: no cold-start gate, uses σ_floor.
        r = recovery_score(55.0, 58.0, 14.0, self.BASE_DICT)
        assert r is not None
        assert 0.0 <= r <= 100.0

    def test_at_baseline_float_dict_is_near_58(self):
        """At-baseline (all z=0) with float dict → ~58%.

        Plain-float baselines use σ_floor as spread, so z = (val-mean)/(1.253*floor).
        When value == mean, z=0 and the logistic returns 100/(1+exp(-k*(0-Z0))) ≈ 57.9.
        """
        r = recovery_score(55.0, 58.0, 14.0, self.BASE_DICT, sleep_perf=None)
        assert isinstance(r, float)
        assert abs(r - 57.9) < 2.0, f"Expected ~57.9%, got {r:.2f}%"

    def test_missing_baseline_component_is_neutral(self):
        """Dict with no 'resp' key → resp term dropped; HRV/RHR at baseline → near 58%."""
        r = recovery_score(55.0, 58.0, 99.0, {"hrv": 55.0, "resting_hr": 58.0})
        assert r is not None
        # No resp baseline → resp term dropped; HRV and RHR at baseline (z=0) → ~57.9%.
        assert abs(r - 57.9) < 2.0, f"Expected ~57.9%, got {r:.2f}%"

    def test_object_baseline_accepted(self):
        """Object with .hrv / .resting_hr / .resp float attributes is accepted."""
        class B:
            hrv = 55.0
            resting_hr = 58.0
            resp = 14.0

        r = recovery_score(55.0, 58.0, 14.0, B())
        assert r is not None
        assert 0.0 <= r <= 100.0


class TestRecoveryLeadingNoneRegression:
    """Regression tests: leading-None history must not corrupt recovery score.

    A user whose history starts with a None (missing night) previously triggered
    the EWMA midpoint-seeding bug: baseline would freeze at ~127.5 ms instead of
    converging to the user's actual ~55 ms, producing recovery ≈ 0–1% indefinitely.
    """

    def test_leading_none_history_recovery_near_58(self):
        """User with [None]+[55]*30 HRV history should have recovery ≈ 58% at baseline."""
        hrv_state = fold_history([None] + [55.0] * 30, METRIC_CFG["hrv"])
        rhr_state = fold_history([None] + [58.0] * 20, METRIC_CFG["resting_hr"])
        resp_state = fold_history([None] + [14.0] * 20, METRIC_CFG["resp"])

        baselines = {
            "hrv": hrv_state,
            "resting_hr": rhr_state,
            "resp": resp_state,
        }
        r = recovery_score(55.0, 58.0, 14.0, baselines)
        assert r is not None, "Expected a score (baseline is usable), got None"
        assert abs(r - 58.0) < 5.0, (
            f"Leading-None corruption: expected recovery ≈ 58%, got {r:.2f}% "
            f"(baseline={hrv_state.baseline:.1f} ms — was ≈127 before fix)"
        )

    def test_two_leading_nones_recovery_near_58(self):
        """[None, None] + [55]*30 → same result: recovery ≈ 58% at baseline."""
        hrv_state = fold_history([None, None] + [55.0] * 30, METRIC_CFG["hrv"])
        rhr_state = fold_history([None, None] + [58.0] * 20, METRIC_CFG["resting_hr"])
        resp_state = fold_history([None, None] + [14.0] * 20, METRIC_CFG["resp"])

        baselines = {
            "hrv": hrv_state,
            "resting_hr": rhr_state,
            "resp": resp_state,
        }
        r = recovery_score(55.0, 58.0, 14.0, baselines)
        assert r is not None
        assert abs(r - 58.0) < 5.0, (
            f"Two leading Nones: expected recovery ≈ 58%, got {r:.2f}%"
        )

    def test_leading_none_does_not_produce_near_zero_recovery(self):
        """The bug produced recovery ≈ 0–1%; explicitly assert this can't happen at baseline."""
        hrv_state = fold_history([None] + [55.0] * 30, METRIC_CFG["hrv"])
        baselines = {
            "hrv": hrv_state,
            "resting_hr": BaselineState(58.0, 2.0, 20, 0, "trusted"),
        }
        r = recovery_score(55.0, 58.0, None, baselines)
        assert r is not None
        assert r > 30.0, f"Recovery {r:.2f}% — baseline corruption still active?"


class TestRecoveryBand:
    """Verify band thresholds match WHOOP color scheme."""

    def test_red_below_34(self):
        assert recovery_band(0.0) == "red"
        assert recovery_band(33.9) == "red"

    def test_yellow_34_to_66(self):
        assert recovery_band(34.0) == "yellow"
        assert recovery_band(66.9) == "yellow"

    def test_green_67_and_above(self):
        assert recovery_band(67.0) == "green"
        assert recovery_band(100.0) == "green"


class TestRecoveryMath:
    """Explicit hand-computed checks for the logistic formula."""

    def test_logistic_at_zero_z(self):
        """Z=0 → 100/(1+exp(-k*(0-Z0))) = 100/(1+exp(k*Z0)) ≈ 58%."""
        expected = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (0.0 - LOGISTIC_Z0)))
        assert abs(expected - 58.0) < 2.0

    def test_logistic_at_positive_z(self):
        """Z > 0 → score > 58%."""
        score = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (1.0 - LOGISTIC_Z0)))
        assert score > 58.0

    def test_logistic_at_negative_z(self):
        """Z < 0 → score < 58%."""
        score = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (-1.0 - LOGISTIC_Z0)))
        assert score < 58.0

    def test_weight_constants_sum_to_1(self):
        assert abs(W_HRV + W_RHR + W_RESP + W_SLEEP - 1.0) < 1e-9

    def test_hrv_dominant(self):
        assert W_HRV > W_RHR
        assert W_HRV > W_RESP
        assert W_HRV > W_SLEEP
