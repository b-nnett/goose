"""
Tests for analysis.baselines — personal-baseline module.

PURE (no DB, no network). Run offline:
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_baselines.py -q

Covers:
  - update_baseline: first night, missing night, outlier rejection, Winsorization
  - fold_history: replay of known sequences, warm-up gates, cold-start
  - deviation: z, delta, ratio, in_normal_range
  - trimmed_mean_baseline: simpler path, consistent status flags
  - EWMA math: half-life and λ relationships
"""
from __future__ import annotations

import math
import statistics
from typing import Optional

import pytest

from app.analysis.baselines import (
    HARD_OUTLIER_K,
    METRIC_CFG,
    MIN_NIGHTS_SEED,
    MIN_NIGHTS_TRUST,
    STALE_DAYS,
    WINSOR_K,
    BaselineState,
    Deviation,
    MetricCfg,
    _lambda,
    deviation,
    fold_history,
    trimmed_mean_baseline,
    update_baseline,
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

HRV_CFG = METRIC_CFG["hrv"]
RHR_CFG = METRIC_CFG["resting_hr"]
RESP_CFG = METRIC_CFG["resp"]

HRV_NIGHTS_TRUSTED = [55.0] * MIN_NIGHTS_TRUST  # exactly MIN_NIGHTS_TRUST nights
HRV_NIGHTS_SEED = [55.0] * MIN_NIGHTS_SEED       # exactly MIN_NIGHTS_SEED nights


def _trusted_state(metric_cfg: MetricCfg = HRV_CFG, mean: float = 55.0) -> BaselineState:
    """Build a trusted baseline state for one metric."""
    return fold_history([mean] * MIN_NIGHTS_TRUST, metric_cfg)


# ---------------------------------------------------------------------------
# _lambda (EWMA half-life)
# ---------------------------------------------------------------------------

class TestLambda:
    def test_14_night_halflife(self):
        lam = _lambda(14.0)
        # After 14 nights at weight (1-lambda)^k, the weight should halve.
        # (1-lambda)^14 == 0.5  →  lambda = 1 - 0.5^(1/14)
        assert abs(lam - (1.0 - 0.5 ** (1.0 / 14.0))) < 1e-12

    def test_lambda_range(self):
        lam = _lambda(14.0)
        assert 0.0 < lam < 1.0


# ---------------------------------------------------------------------------
# update_baseline: first night
# ---------------------------------------------------------------------------

class TestUpdateBaselineFirstNight:
    def test_none_value_seeds_calibrating(self):
        state = update_baseline(None, None, HRV_CFG)
        assert state.status == "calibrating"
        assert state.n_valid == 0

    def test_valid_first_night(self):
        state = update_baseline(None, 55.0, HRV_CFG)
        assert state.baseline == 55.0
        assert state.n_valid == 1
        assert state.status == "calibrating"
        assert state.spread >= HRV_CFG.floor_spread

    def test_implausible_first_night(self):
        # Below min_val → treated as missing.
        state = update_baseline(None, 1.0, HRV_CFG)  # 1 ms < 5 ms min
        assert state.n_valid == 0
        assert state.status == "calibrating"

    def test_spread_floored_at_first_night(self):
        state = update_baseline(None, 55.0, HRV_CFG)
        assert state.spread >= HRV_CFG.floor_spread


# ---------------------------------------------------------------------------
# update_baseline: missing nights (skip-and-hold)
# ---------------------------------------------------------------------------

class TestMissingNights:
    def test_missing_night_carries_forward(self):
        s0 = update_baseline(None, 55.0, HRV_CFG)
        s1 = update_baseline(s0, None, HRV_CFG)
        assert s1.baseline == s0.baseline
        assert s1.spread == s0.spread
        assert s1.n_valid == s0.n_valid
        assert s1.nights_since_update == 1

    def test_multiple_missing_increments(self):
        s = update_baseline(None, 55.0, HRV_CFG)
        for i in range(5):
            s = update_baseline(s, None, HRV_CFG)
        assert s.nights_since_update == 5

    def test_stale_after_many_missing(self):
        # Build a trusted state, then go STALE_DAYS missing nights.
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        assert s.status == "trusted"
        for _ in range(STALE_DAYS + 1):
            s = update_baseline(s, None, HRV_CFG)
        assert s.status == "stale"

    def test_valid_night_resets_nights_since_update(self):
        s = update_baseline(None, 55.0, HRV_CFG)
        s = update_baseline(s, None, HRV_CFG)
        assert s.nights_since_update == 1
        s = update_baseline(s, 56.0, HRV_CFG)
        assert s.nights_since_update == 0


# ---------------------------------------------------------------------------
# update_baseline: outlier rejection
# ---------------------------------------------------------------------------

class TestOutlierRejection:
    def test_hard_outlier_not_incorporated(self):
        """A value > HARD_OUTLIER_K * spread away should not move the baseline."""
        # Build a trusted baseline at 55 ms with known spread.
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        baseline_before = s.baseline
        spread_before = s.spread

        # Inject a massive outlier (e.g. 300 ms, >> 5 * spread from 55).
        outlier = 55.0 + HARD_OUTLIER_K * s.spread * 2
        s_after = update_baseline(s, outlier, HRV_CFG)

        # Baseline must not move; n_valid must not increase.
        assert s_after.baseline == baseline_before
        assert s_after.spread == spread_before
        assert s_after.n_valid == s.n_valid

    def test_physiological_implausible_skipped(self):
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        # Above max_val.
        s2 = update_baseline(s, 999.0, HRV_CFG)
        assert s2.n_valid == s.n_valid
        assert s2.baseline == s.baseline


# ---------------------------------------------------------------------------
# update_baseline: Winsorization
# ---------------------------------------------------------------------------

class TestWinsorization:
    def test_moderate_extreme_is_clamped_not_rejected(self):
        """A value within HARD_OUTLIER_K but beyond WINSOR_K is folded in (clamped)."""
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)

        # Pick a value just at WINSOR_K*spread away (should be incorporated).
        moderate_extreme = s.baseline + WINSOR_K * s.spread * 0.99
        s2 = update_baseline(s, moderate_extreme, HRV_CFG)
        # n_valid should increase (night was not rejected).
        assert s2.n_valid == s.n_valid + 1

    def test_baseline_moves_less_with_winsorization(self):
        """Winsorized update moves baseline less than a raw update would."""
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        # A value well beyond WINSOR_K * spread (but within HARD_OUTLIER_K * spread).
        extreme = s.baseline + (WINSOR_K + 0.5) * s.spread

        s2 = update_baseline(s, extreme, HRV_CFG)
        # The baseline should move toward the clamped value, not the raw extreme.
        # Clamped value = baseline + WINSOR_K * spread.
        lam_b = _lambda(HRV_CFG.half_life_b)
        clamped = s.baseline + WINSOR_K * s.spread
        expected_move = lam_b * (clamped - s.baseline)
        actual_move = s2.baseline - s.baseline
        assert abs(actual_move - expected_move) < 0.01


# ---------------------------------------------------------------------------
# fold_history: replay sequences
# ---------------------------------------------------------------------------

class TestFoldHistory:
    def test_empty_is_calibrating(self):
        s = fold_history([], HRV_CFG)
        assert s.status == "calibrating"
        assert s.n_valid == 0

    def test_all_none_is_calibrating(self):
        s = fold_history([None, None, None], HRV_CFG)
        assert s.status == "calibrating"
        assert s.n_valid == 0

    def test_four_nights_is_provisional(self):
        s = fold_history([55.0] * MIN_NIGHTS_SEED, HRV_CFG)
        assert s.status == "provisional"
        assert s.n_valid == MIN_NIGHTS_SEED

    def test_fourteen_nights_is_trusted(self):
        s = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        assert s.status == "trusted"
        assert s.n_valid == MIN_NIGHTS_TRUST

    def test_fewer_than_seed_is_calibrating(self):
        s = fold_history([55.0] * (MIN_NIGHTS_SEED - 1), HRV_CFG)
        assert s.status == "calibrating"

    def test_constant_series_baseline_converges(self):
        """After many identical nights, the EWMA baseline should be very close to
        the constant value."""
        s = fold_history([55.0] * 50, HRV_CFG)
        assert abs(s.baseline - 55.0) < 0.5

    def test_gap_nights_carry_forward(self):
        """None values in the middle should not change the baseline."""
        s_no_gaps = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        # Same series but with 3 gaps interspersed.
        with_gaps = [55.0] * 5 + [None, None, None] + [55.0] * (MIN_NIGHTS_TRUST - 5)
        s_gaps = fold_history(with_gaps, HRV_CFG)
        # Baseline should still converge near 55.0.
        assert abs(s_gaps.baseline - 55.0) < 1.0

    def test_rising_series_baseline_tracks(self):
        """A series of rising values should shift the baseline upward."""
        low_nights = [40.0] * 10
        high_nights = [80.0] * 20
        s_low = fold_history(low_nights, HRV_CFG)
        s_high = fold_history(high_nights, HRV_CFG)
        assert s_high.baseline > s_low.baseline

    def test_outlier_does_not_whipsaw(self):
        """A single extreme outlier should barely affect the baseline."""
        stable = [55.0] * 20
        s_stable = fold_history(stable, HRV_CFG)

        # Insert a big outlier at the end.
        with_outlier = stable + [300.0]
        s_outlier = fold_history(with_outlier, HRV_CFG)

        # Outlier should not move baseline by more than 1 ms.
        assert abs(s_outlier.baseline - s_stable.baseline) < 1.0

    def test_hrv_spread_at_floor(self):
        """With a perfectly constant series, spread should stay at floor."""
        s = fold_history([55.0] * 30, HRV_CFG)
        assert s.spread >= HRV_CFG.floor_spread

    def test_different_metrics_have_different_floors(self):
        s_hrv = fold_history([55.0] * 30, HRV_CFG)
        s_rhr = fold_history([58.0] * 30, RHR_CFG)
        assert s_hrv.spread >= HRV_CFG.floor_spread
        assert s_rhr.spread >= RHR_CFG.floor_spread

    def test_trusted_flag(self):
        s_calibrating = fold_history([55.0] * 2, HRV_CFG)
        s_trusted = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        assert not s_calibrating.trusted
        assert s_trusted.trusted

    def test_usable_flag(self):
        s_cal = fold_history([55.0] * (MIN_NIGHTS_SEED - 1), HRV_CFG)
        s_prov = fold_history([55.0] * MIN_NIGHTS_SEED, HRV_CFG)
        s_trust = fold_history([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        assert not s_cal.usable
        assert s_prov.usable
        assert s_trust.usable


# ---------------------------------------------------------------------------
# deviation
# ---------------------------------------------------------------------------

class TestDeviation:
    def test_at_baseline_z_near_zero(self):
        s = _trusted_state(HRV_CFG, mean=55.0)
        dev = deviation(s.baseline, s)
        assert abs(dev.z) < 0.1

    def test_at_baseline_ratio_near_zero(self):
        s = _trusted_state(HRV_CFG, mean=55.0)
        dev = deviation(s.baseline, s)
        assert abs(dev.ratio) < 0.01

    def test_at_baseline_delta_zero(self):
        s = _trusted_state(HRV_CFG, mean=55.0)
        dev = deviation(s.baseline, s)
        assert abs(dev.delta) < 0.01

    def test_z_positive_for_above_baseline(self):
        s = _trusted_state(HRV_CFG, mean=55.0)
        dev = deviation(70.0, s)
        assert dev.z > 0
        assert dev.delta > 0
        assert dev.ratio > 0

    def test_z_negative_for_below_baseline(self):
        s = _trusted_state(HRV_CFG, mean=55.0)
        dev = deviation(40.0, s)
        assert dev.z < 0
        assert dev.delta < 0
        assert dev.ratio < 0

    def test_in_normal_range_within_one_sigma(self):
        """A value ±1 std from baseline should be in_normal_range."""
        s = _trusted_state(HRV_CFG, mean=55.0)
        # Value within 1 spread unit of baseline.
        small_dev = deviation(s.baseline + 0.5 * 1.253 * s.spread, s)
        assert small_dev.in_normal_range

    def test_outside_normal_range_beyond_one_sigma(self):
        """A value >1 std from baseline should be outside normal range."""
        s = _trusted_state(HRV_CFG, mean=55.0)
        # Force a large deviation by using a very small spread.
        s_tight = BaselineState(
            baseline=55.0, spread=HRV_CFG.floor_spread,
            n_valid=20, nights_since_update=0, status="trusted"
        )
        large_dev = deviation(55.0 + 2.0 * 1.253 * s_tight.spread, s_tight)
        assert not large_dev.in_normal_range

    def test_z_formula(self):
        """Manual verification of z = (value - baseline) / (1.253 * spread)."""
        s = BaselineState(baseline=55.0, spread=5.0, n_valid=20, nights_since_update=0, status="trusted")
        v = 70.0
        dev = deviation(v, s)
        expected_z = (v - 55.0) / (1.253 * 5.0)
        assert abs(dev.z - expected_z) < 1e-9

    def test_ratio_formula(self):
        s = BaselineState(baseline=55.0, spread=5.0, n_valid=20, nights_since_update=0, status="trusted")
        dev = deviation(66.0, s)
        assert abs(dev.ratio - (66.0 / 55.0 - 1.0)) < 1e-9


# ---------------------------------------------------------------------------
# trimmed_mean_baseline
# ---------------------------------------------------------------------------

class TestTrimmedMeanBaseline:
    def test_empty_is_calibrating(self):
        s = trimmed_mean_baseline([], HRV_CFG)
        assert s.status == "calibrating"
        assert s.n_valid == 0

    def test_stable_series_center(self):
        s = trimmed_mean_baseline([55.0] * 30, HRV_CFG)
        assert abs(s.baseline - 55.0) < 0.5

    def test_outlier_resilient(self):
        """A 20%-trimmed mean should not be heavily influenced by a few extreme values."""
        clean = [55.0] * 20
        with_outliers = clean + [200.0, 200.0, 5.0, 5.0]
        s_clean = trimmed_mean_baseline(clean, HRV_CFG)
        s_outlier = trimmed_mean_baseline(with_outliers, HRV_CFG)
        # With 4 outliers out of 24 (~17%), trimming should handle them.
        assert abs(s_outlier.baseline - s_clean.baseline) < 5.0

    def test_status_flags_match_n_valid(self):
        s_few = trimmed_mean_baseline([55.0] * (MIN_NIGHTS_SEED - 1), HRV_CFG)
        s_seed = trimmed_mean_baseline([55.0] * MIN_NIGHTS_SEED, HRV_CFG)
        s_trust = trimmed_mean_baseline([55.0] * MIN_NIGHTS_TRUST, HRV_CFG)
        assert s_few.status == "calibrating"
        assert s_seed.status == "provisional"
        assert s_trust.status == "trusted"

    def test_none_values_dropped(self):
        values = [55.0, None, 56.0, None, 54.0]
        s = trimmed_mean_baseline(values, HRV_CFG)
        assert s.n_valid == 3  # only the 3 real values count

    def test_spread_stored_in_abs_dev_space(self):
        """trimmed_mean_baseline stores spread in abs-dev space (like EWMA path).

        deviation() multiplies by 1.253, so sigma = 1.253 * spread >= floor_spread.
        For a constant series (sigma_mad=0), spread = floor_spread / 1.253.
        """
        s = trimmed_mean_baseline([55.0] * 30, HRV_CFG)
        # The stored spread is in abs-dev space; multiplied by 1.253 it equals the floor.
        sigma_approx = 1.253 * s.spread
        assert abs(sigma_approx - HRV_CFG.floor_spread) < 1e-6


# ---------------------------------------------------------------------------
# Leading-None initialization — CRITICAL regression tests
# ---------------------------------------------------------------------------

class TestLeadingNoneInitialization:
    """Regression tests for the leading-None EWMA corruption bug.

    When history starts with None(s), the placeholder state is seeded at the
    physiological midpoint.  The first REAL value must reset the baseline to
    itself rather than Winsorizing toward the midpoint.
    """

    def test_one_leading_none_then_30_valid_converges(self):
        """[None] + [55]*30 → baseline ≈ 55, NOT ~127."""
        s = fold_history([None] + [55.0] * 30, HRV_CFG)
        assert abs(s.baseline - 55.0) < 1.0, (
            f"baseline={s.baseline:.2f} — leading-None corrupts EWMA if baseline ≈ 127"
        )
        assert s.n_valid == 30

    def test_two_leading_nones_then_30_valid_converges(self):
        """[None, None] + [55]*30 → baseline ≈ 55, NOT ~127."""
        s = fold_history([None, None] + [55.0] * 30, HRV_CFG)
        assert abs(s.baseline - 55.0) < 1.0, (
            f"baseline={s.baseline:.2f} — two leading Nones corrupts EWMA if ≈ 127"
        )
        assert s.n_valid == 30

    def test_no_leading_none_unchanged(self):
        """Control: no leading None — still converges to 55."""
        s = fold_history([55.0] * 30, HRV_CFG)
        assert abs(s.baseline - 55.0) < 1.0

    def test_leading_none_placeholder_has_n_valid_zero(self):
        """The placeholder state from a first-night None must have n_valid=0."""
        state = update_baseline(None, None, HRV_CFG)
        assert state.n_valid == 0
        assert state.status == "calibrating"

    def test_first_real_value_after_none_resets_to_real_value(self):
        """A real first value arriving on a n_valid=0 placeholder must seed at the real value."""
        placeholder = update_baseline(None, None, HRV_CFG)
        assert placeholder.n_valid == 0
        # Now fold in the first real value
        s = update_baseline(placeholder, 55.0, HRV_CFG)
        assert s.baseline == 55.0, (
            f"Expected baseline=55.0 after first real value, got {s.baseline:.2f}"
        )
        assert s.n_valid == 1

    def test_midpoint_is_not_used_as_ewma_anchor(self):
        """After 30 consistent nights of 55 ms (with a leading None), baseline
        must NOT be anchored toward the physiological midpoint (127.5 for HRV).
        """
        midpoint = (HRV_CFG.min_val + HRV_CFG.max_val) / 2.0  # 127.5
        s = fold_history([None] + [55.0] * 30, HRV_CFG)
        assert abs(s.baseline - midpoint) > 50.0, (
            f"baseline={s.baseline:.2f} suspiciously close to midpoint {midpoint}"
        )


# ---------------------------------------------------------------------------
# trimmed_mean_baseline floor normalization — IMPORTANT regression test
# ---------------------------------------------------------------------------

class TestTrimmedMeanFloorNormalization:
    """The floor must apply in σ-space so deviation() returns exactly floor for stable series."""

    def test_constant_series_z_score_uses_exact_floor(self):
        """For a constant series (sigma_mad=0), a deviation of floor_spread units
        must produce z ≈ 1.0 (one σ unit), not 1/1.253 ≈ 0.798.
        """
        s = trimmed_mean_baseline([55.0] * 30, HRV_CFG)
        # One floor_spread unit above baseline
        dev = deviation(55.0 + HRV_CFG.floor_spread, s)
        assert abs(dev.z - 1.0) < 1e-4, (
            f"z={dev.z:.6f} — floor normalization bug: expected z=1.0 "
            f"(sigma={1.253*s.spread:.4f}, floor={HRV_CFG.floor_spread})"
        )

    def test_deviation_sigma_equals_floor_for_stable_series(self):
        """deviation() must see sigma = floor_spread, not 1.253 * floor_spread."""
        s = trimmed_mean_baseline([55.0] * 30, HRV_CFG)
        sigma_approx = 1.253 * s.spread
        assert abs(sigma_approx - HRV_CFG.floor_spread) < 1e-6, (
            f"sigma_approx={sigma_approx:.6f} != floor_spread={HRV_CFG.floor_spread}"
        )

    def test_rhr_floor_normalization(self):
        """Same invariant holds for resting_hr floor (2.0 bpm)."""
        cfg = METRIC_CFG["resting_hr"]
        s = trimmed_mean_baseline([58.0] * 30, cfg)
        dev = deviation(58.0 + cfg.floor_spread, s)
        assert abs(dev.z - 1.0) < 1e-4, f"resting_hr z={dev.z:.6f}, expected 1.0"


# ---------------------------------------------------------------------------
# METRIC_CFG completeness
# ---------------------------------------------------------------------------

class TestMetricCfg:
    def test_all_required_metrics_present(self):
        for key in ("hrv", "resting_hr", "resp", "skin_temp"):
            assert key in METRIC_CFG, f"METRIC_CFG missing '{key}'"

    def test_floors_are_positive(self):
        for key, cfg in METRIC_CFG.items():
            assert cfg.floor_spread > 0, f"{key}.floor_spread must be > 0"

    def test_bounds_are_ordered(self):
        for key, cfg in METRIC_CFG.items():
            assert cfg.min_val < cfg.max_val, f"{key}: min_val >= max_val"

    def test_half_lives_positive(self):
        for key, cfg in METRIC_CFG.items():
            assert cfg.half_life_b > 0
            assert cfg.half_life_s > 0
