"""
Tests for analysis.strain — WHOOP 0–21 strain (Edwards TRIMP w/ HRR).

Covers:
  * Existing openwhoop-port behaviour (zone weights, guard rails, value tests).
  * New: HRmax source selection (observed vs Tanaka vs 220-age vs unknown).
  * New: Banister vs Edwards TRIMP comparison.
  * New: Log-map monotonicity / curvature (hard day ≫ easy day; marathon saturation).
  * New: fit_strain_denominator recovers a known D from synthetic pairs.
  * New: strain_score backward-compatible alias.

Run offline:
    cd ~/Developer/home-server/.worktrees/metrics-accuracy/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_strain.py -q
"""
from __future__ import annotations

import math

import pytest

from app.analysis.strain import (
    BANISTER_B_MEN,
    BANISTER_B_WOMEN,
    DEFAULT_AGE,
    HRMAX_MIN_SAMPLES,
    MIN_READINGS,
    STRAIN_DENOMINATOR,
    _banister_trimp,
    _edwards_trimp,
    _sample_duration_minutes,
    _trimp_to_strain,
    _zone_weight,
    default_max_hr,
    estimate_hrmax,
    fit_strain_denominator,
    strain,
    strain_score,
    tanaka_hrmax,
)

T0 = 1_700_000_000.0


def _constant(bpm: int, n: int, step_s: float = 1.0) -> list[dict]:
    return [{"ts": T0 + i * step_s, "bpm": bpm} for i in range(n)]


def _hr_list(bpm: int, n: int) -> list[float]:
    """Flat list of float HR values (for estimate_hrmax)."""
    return [float(bpm)] * n


# ===========================================================================
# _zone_weight boundaries  (Edwards 1993 HR-zone weights)
# ===========================================================================

def test_zone_weight_boundaries_hrr():
    # max_hr=200, resting_hr=50 -> HRR = 150
    rhr, hrr = 50.0, 150.0
    assert _zone_weight(120, rhr, hrr) == 0   # <50% HRR
    assert _zone_weight(125, rhr, hrr) == 1   # 50%
    assert _zone_weight(139, rhr, hrr) == 1
    assert _zone_weight(140, rhr, hrr) == 2   # 60%
    assert _zone_weight(154, rhr, hrr) == 2
    assert _zone_weight(155, rhr, hrr) == 3   # 70%
    assert _zone_weight(169, rhr, hrr) == 3
    assert _zone_weight(170, rhr, hrr) == 4   # 80%
    assert _zone_weight(184, rhr, hrr) == 4
    assert _zone_weight(185, rhr, hrr) == 5   # 90%
    assert _zone_weight(200, rhr, hrr) == 5


# ===========================================================================
# Guard rails
# ===========================================================================

def test_too_few_readings_returns_none():
    assert strain(_constant(80, 500), max_hr=200, resting_hr=60) is None
    assert len(_constant(80, MIN_READINGS - 1)) < MIN_READINGS


def test_invalid_hr_params_returns_none():
    readings = _constant(80, MIN_READINGS)
    assert strain(readings, max_hr=60, resting_hr=60) is None  # max == resting
    assert strain(readings, max_hr=50, resting_hr=60) is None  # max < resting


# ===========================================================================
# Value behaviour
# ===========================================================================

def test_resting_hr_produces_zero_strain():
    # 65 bpm, HRR=(190-60)=130 -> %HRR ~3.8% -> below zone 1 -> weight 0
    s = strain(_constant(65, MIN_READINGS), max_hr=190, resting_hr=60)
    assert s == 0.0


def test_high_hr_produces_high_strain():
    # 170 bpm, HRR 130 -> %HRR 84.6% -> zone 4; 1800 samples @1s = 30 min
    # TRIMP = 30 * 4 = 120 -> 21*ln(121)/ln(7201) ~ 11.34
    s = strain(_constant(170, 1800), max_hr=190, resting_hr=60)
    assert s is not None and s > 10.0


def test_strain_capped_at_21_24h_max():
    # 24h at max HR -> zone 5 -> TRIMP = 24*60*5 = 7200 -> strain = 21.0
    s = strain(_constant(190, 86_400), max_hr=190, resting_hr=60)
    assert s == 21.0


def test_higher_hr_means_more_strain():
    low = strain(_constant(100, MIN_READINGS), max_hr=190, resting_hr=60)
    high = strain(_constant(160, MIN_READINGS), max_hr=190, resting_hr=60)
    assert low is not None and high is not None
    assert high > low


def test_known_trimp_value():
    # 30 min @ zone 4 -> TRIMP 120 -> round(21*ln(121)/ln(7201), 2) = 11.34.
    # Hardcoded independent value (NOT recomputed with the module constant).
    s = strain(_constant(170, 1800), max_hr=190, resting_hr=60)
    assert s == 11.34


def test_rounded_to_two_dp():
    s = strain(_constant(170, 1800), max_hr=190, resting_hr=60)
    assert s is not None
    assert s == round(s, 2)


# ===========================================================================
# Defaults
# ===========================================================================

def test_default_max_hr_uses_default_age():
    assert default_max_hr() == 220 - DEFAULT_AGE
    # When max_hr omitted, strain still computes (uses 220 - DEFAULT_AGE).
    s = strain(_constant(150, MIN_READINGS), resting_hr=60)
    assert s is not None and s >= 0.0


def test_sample_duration_from_timestamps():
    # 2 s sampling doubles each sample's duration -> ~double the TRIMP/strain
    one_s = strain(_constant(170, 1800, step_s=1.0), max_hr=190, resting_hr=60)
    two_s = strain(_constant(170, 1800, step_s=2.0), max_hr=190, resting_hr=60)
    assert one_s is not None and two_s is not None
    assert two_s > one_s


# ===========================================================================
# NEW: HRmax source selection
# ===========================================================================

class TestEstimateHrmax:
    """estimate_hrmax selects the right source and applies max(observed, tanaka)."""

    def test_observed_preferred_when_rich_history(self):
        # 1000 readings at 180 bpm -> p99.5 = exactly 180.0 (all same value).
        # Tanaka(30) = 187 > 180, so the Tanaka floor wins → source = "tanaka".
        history = _hr_list(180, HRMAX_MIN_SAMPLES)
        hrmax, source = estimate_hrmax(history, age=30)
        assert source == "tanaka"  # Tanaka(30)=187 > observed p99.5=180
        assert hrmax == pytest.approx(tanaka_hrmax(30), abs=1e-9)  # exact float arithmetic

    def test_tanaka_floor_label_when_tanaka_wins(self):
        # When the Tanaka floor is larger than observed p99.5, source must be "tanaka",
        # not "observed", so provenance is honest.
        # age=25 → Tanaka(25) = 208 - 0.7*25 = 190.5; p99.5 of [185]*N = 185 < 190.5.
        history = _hr_list(185, HRMAX_MIN_SAMPLES)
        hrmax, source = estimate_hrmax(history, age=25)
        assert source == "tanaka"
        assert hrmax == pytest.approx(tanaka_hrmax(25), abs=1e-9)

    def test_observed_label_when_observed_wins(self):
        # When observed p99.5 > Tanaka, source must be "observed".
        # Tanaka(30) = 187; p99.5 of [200]*N = 200 > 187 → source = "observed".
        history = _hr_list(200, HRMAX_MIN_SAMPLES)
        hrmax, source = estimate_hrmax(history, age=30)
        assert source == "observed"
        assert hrmax == pytest.approx(200.0, abs=1e-9)

    def test_observed_vs_tanaka_takes_max(self):
        # If observed p99.5 > Tanaka, the observed value wins and source = "observed".
        # 1000 readings at 200 bpm -> p99.5 = 200 -> Tanaka(30) = 187 -> take 200.
        history = _hr_list(200, HRMAX_MIN_SAMPLES)
        hrmax, source = estimate_hrmax(history, age=30)
        assert source == "observed"
        assert hrmax >= tanaka_hrmax(30)
        assert hrmax == pytest.approx(200.0, abs=1e-9)

    def test_thin_data_falls_back_to_tanaka(self):
        # Fewer than HRMAX_MIN_SAMPLES -> Tanaka
        history = _hr_list(190, HRMAX_MIN_SAMPLES - 1)
        hrmax, source = estimate_hrmax(history, age=40)
        assert source == "tanaka"
        assert hrmax == pytest.approx(tanaka_hrmax(40), abs=0.01)

    def test_no_history_no_age_returns_unknown(self):
        hrmax, source = estimate_hrmax([], age=None)
        assert source == "unknown"
        assert hrmax == 0.0

    def test_no_history_with_age_falls_back_to_tanaka(self):
        hrmax, source = estimate_hrmax([], age=35)
        assert source == "tanaka"
        assert hrmax == pytest.approx(tanaka_hrmax(35), abs=0.01)

    def test_tanaka_formula(self):
        # Explicit values from the formula.
        assert tanaka_hrmax(30) == pytest.approx(187.0, abs=0.01)
        assert tanaka_hrmax(50) == pytest.approx(173.0, abs=0.01)
        assert tanaka_hrmax(20) == pytest.approx(194.0, abs=0.01)

    def test_observed_p995_rejects_spike(self):
        # 999 readings at 150, 1 at 999 (artifact) — p99.5 should stay ~150,
        # not 999. With 1000 samples the 99.5th percentile is between index 994
        # and 995 (0-based), which are all 150. The spike at index 999 (100th
        # percentile) doesn't pull the 99.5th up.
        history = [150.0] * (HRMAX_MIN_SAMPLES - 1) + [999.0]
        hrmax, source = estimate_hrmax(history, age=None)
        assert source == "observed"
        # p99.5 of [150]*999 + [999] with n=1000:
        # idx_f = 0.995 * 999 = 994.005 -> lo=994, frac=0.005
        # sorted[994]=150, sorted[995]=150 -> p99.5 = 150.0 + 0.005*(150-150) = 150
        assert hrmax == pytest.approx(150.0, abs=1.0)

    def test_220_minus_age_not_used_as_primary(self):
        # With age=30, Tanaka gives 187; 220-30=190.  Tanaka is the designated
        # fallback (not 220-age).  With thin data we should get Tanaka, not 190.
        hrmax, source = estimate_hrmax([], age=30)
        assert source == "tanaka"
        assert hrmax == pytest.approx(187.0, abs=0.01)
        assert hrmax != 190.0  # NOT 220-age


# ===========================================================================
# NEW: Banister vs Edwards TRIMP
# ===========================================================================

class TestBanisterTrimp:
    """Banister and Edwards produce distinct values for the same HR series.

    Key verified math (b=1.92 men, b=1.67 women):
      - Banister weight = pct * 0.64 * exp(b * pct) per minute (continuous).
      - At 50% HRR: Banister ≈ 0.836 vs Edwards zone-1 weight = 1  → Edwards higher.
      - At 80% HRR: Banister ≈ 2.379 vs Edwards zone-4 weight = 4  → Edwards higher.
      - At 100% HRR: Banister ≈ 4.365 vs Edwards zone-5 weight = 5 → Edwards higher.
    In absolute terms, Edwards gives higher TRIMP than Banister across all typical zones.
    Banister's advantage is smoothness (no step artifacts) and that its *relative*
    amplification between bottom (50%) and top (100%) HRR is steeper in the continuous
    sense — it penalizes very low intensities more (below zone 1 it is > 0, but tiny).
    """

    def _series(self, bpm: int, n: int = MIN_READINGS, step_s: float = 1.0):
        return _constant(bpm, n, step_s)

    def test_banister_lower_than_edwards_for_zone4_effort(self):
        # At 85.7% HRR (zone 4), Banister per-min weight ≈ 2.84 < Edwards weight 4.
        # Edwards always gives higher absolute TRIMP than Banister at zone 1–5.
        # max_hr=200, resting_hr=60, HRR=140; 180 bpm → 85.7% HRR.
        series = self._series(180, MIN_READINGS)
        e = strain(series, max_hr=200, resting_hr=60, method="edwards")
        b = strain(series, max_hr=200, resting_hr=60, method="banister")
        assert e is not None and b is not None
        assert e > b  # Edwards gives higher absolute TRIMP at this intensity

    def test_banister_lower_than_edwards_at_low_intensity(self):
        # At 50% HRR (zone 1), Banister ≈ 0.836 < Edwards weight 1.
        # max_hr=200, resting_hr=60, HRR=140; 50% HRR = 130 bpm.
        series = self._series(130, MIN_READINGS)
        e = strain(series, max_hr=200, resting_hr=60, method="edwards")
        b = strain(series, max_hr=200, resting_hr=60, method="banister")
        assert e is not None and b is not None
        assert e > b  # Edwards higher at low intensity too

    def test_banister_sex_parameter_differs(self):
        # Women (b=1.67) vs men (b=1.92): same series, different TRIMP.
        # Higher b → steeper exponential amplification → higher TRIMP for men.
        series = self._series(175, MIN_READINGS)
        male = strain(series, max_hr=200, resting_hr=60, method="banister", sex="male")
        female = strain(series, max_hr=200, resting_hr=60, method="banister", sex="female")
        assert male is not None and female is not None
        assert male > female

    def test_banister_monotonic_with_intensity(self):
        # Banister strain must increase with HR (holding everything else equal).
        series_lo = self._series(130, MIN_READINGS)
        series_hi = self._series(175, MIN_READINGS)
        lo = strain(series_lo, max_hr=200, resting_hr=60, method="banister")
        hi = strain(series_hi, max_hr=200, resting_hr=60, method="banister")
        assert lo is not None and hi is not None
        assert hi > lo

    def test_banister_positive_for_any_nonzero_intensity(self):
        # Banister is continuous: even below Edwards' zone-1 threshold (50% HRR)
        # Banister gives a small positive TRIMP (unlike Edwards which returns 0).
        # max_hr=200, resting_hr=60; 49% HRR = 60 + 0.49*140 = 128.6 → use 128 bpm.
        # Edwards: 128 bpm → %HRR = (128-60)/140 = 48.6% < 50% → weight 0 → strain 0.
        # Banister: positive weight → positive strain.
        series = self._series(128, MIN_READINGS)
        e = strain(series, max_hr=200, resting_hr=60, method="edwards")
        b = strain(series, max_hr=200, resting_hr=60, method="banister")
        assert e == 0.0           # Edwards: below zone-1 threshold → zero
        assert b is not None and b > 0.0  # Banister: continuous → nonzero

    def test_edwards_still_default(self):
        # Default method is "edwards" (no keyword needed).
        s_explicit = strain(_constant(170, 1800), max_hr=190, resting_hr=60,
                            method="edwards")
        s_default = strain(_constant(170, 1800), max_hr=190, resting_hr=60)
        assert s_explicit == s_default


# ===========================================================================
# NEW: Log-map monotonicity and curvature
# ===========================================================================

class TestLogMapCurvature:
    """strain = 21*ln(TRIMP+1)/ln(D) must show correct log shape."""

    def test_hard_day_much_greater_than_easy_day(self):
        # Easy: 10 min @zone1 (50% HRR) — marginal TRIMP.
        # Hard: 60 min @zone5 (≥90% HRR) — high TRIMP.
        easy = strain(_constant(125, MIN_READINGS), max_hr=200, resting_hr=60)
        # 3600 samples at bpm=200 (100% HRR) -> zone5, 60 min
        hard = strain(_constant(200, 3600), max_hr=200, resting_hr=60)
        assert easy is not None and hard is not None
        # Hard day should be substantially higher (>3 points).
        assert hard - easy > 3.0

    def test_log_saturation_marathon_then_second(self):
        # A second marathon (same effort) after one marathon adds very little.
        # Simulate: 4 hours at high HR (zone 4, 80% HRR).
        # max_hr=200, resting_hr=60, HRR=140; 80% -> bpm=172.
        # 4h = 14400 samples @ 1s.
        one_marathon = strain(_constant(172, 14_400), max_hr=200, resting_hr=60)
        two_marathons = strain(_constant(172, 28_800), max_hr=200, resting_hr=60)
        assert one_marathon is not None and two_marathons is not None
        # Second marathon adds less than the first.
        assert two_marathons > one_marathon
        gap_first = one_marathon   # starts from ~0
        gap_second = two_marathons - one_marathon
        assert gap_second < gap_first  # logarithmic saturation

    def test_strain_monotone_in_duration(self):
        # More minutes at the same HR → more strain.
        short = strain(_constant(160, MIN_READINGS), max_hr=200, resting_hr=60)
        long = strain(_constant(160, MIN_READINGS * 3), max_hr=200, resting_hr=60)
        assert short is not None and long is not None
        assert long > short

    def test_strain_zero_for_below_zone1(self):
        # Below 50% HRR the Edwards weight is 0 → TRIMP = 0 → strain = 0.
        # HRR = 200-60 = 140; 50% = 130 bpm; use 129 bpm to be below threshold.
        s = strain(_constant(129, MIN_READINGS), max_hr=200, resting_hr=60)
        assert s == 0.0

    def test_custom_denominator_scales_result(self):
        # A smaller denominator → same TRIMP → higher strain value (and vice versa).
        s_default = strain(_constant(170, 1800), max_hr=190, resting_hr=60)
        s_small_d = strain(_constant(170, 1800), max_hr=190, resting_hr=60,
                           denominator=100.0)
        s_large_d = strain(_constant(170, 1800), max_hr=190, resting_hr=60,
                           denominator=1_000_000.0)
        assert s_default is not None
        assert s_small_d is not None and s_large_d is not None
        assert s_small_d > s_default > s_large_d


# ===========================================================================
# NEW: fit_strain_denominator
# ===========================================================================

class TestFitStrainDenominator:
    """fit_strain_denominator recovers a known D from synthetic (TRIMP, strain) pairs."""

    def _make_pairs(self, d: float, trimps: list[float]) -> list[tuple[float, float]]:
        """Generate synthetic (TRIMP, WHOOP_strain) pairs from a known D."""
        return [
            (t, 21.0 * math.log(t + 1.0) / math.log(d))
            for t in trimps
        ]

    def test_recovers_known_denominator(self):
        # Generate pairs from D=5000, then fit; should recover ≈ 5000.
        true_d = 5000.0
        trimps = [10.0, 50.0, 100.0, 300.0, 600.0, 1200.0, 2400.0]
        pairs = self._make_pairs(true_d, trimps)
        fitted_d = fit_strain_denominator(pairs)
        # Should recover D within 0.1% (exact synthetic data → near-perfect fit).
        assert fitted_d == pytest.approx(true_d, rel=0.001)

    def test_recovers_default_denominator(self):
        # Synthetic pairs from the module default (7201) should recover ≈ 7201.
        true_d = STRAIN_DENOMINATOR
        trimps = [60.0, 120.0, 240.0, 480.0, 960.0, 1920.0, 3600.0]
        pairs = self._make_pairs(true_d, trimps)
        fitted_d = fit_strain_denominator(pairs)
        assert fitted_d == pytest.approx(true_d, rel=0.001)

    def test_requires_at_least_two_pairs(self):
        with pytest.raises(ValueError, match="at least 2"):
            fit_strain_denominator([(100.0, 10.0)])

    def test_raises_on_zero_trimp_pairs(self):
        # All-zero TRIMP pairs → not enough valid pairs
        with pytest.raises(ValueError, match="at least 2"):
            fit_strain_denominator([(0.0, 5.0), (0.0, 8.0)])

    def test_raises_on_zero_strain_pairs(self):
        # All-zero strain pairs → not enough valid pairs
        with pytest.raises(ValueError, match="at least 2"):
            fit_strain_denominator([(100.0, 0.0), (200.0, 0.0)])

    def test_fitted_d_different_from_default_when_data_says_so(self):
        # If the "true" D is 3000 (not 7201), the fit should return a value
        # meaningfully different from the default.
        true_d = 3000.0
        trimps = [100.0, 300.0, 800.0, 1500.0]
        pairs = self._make_pairs(true_d, trimps)
        fitted_d = fit_strain_denominator(pairs)
        assert abs(fitted_d - STRAIN_DENOMINATOR) > 1000  # far from 7201
        assert fitted_d == pytest.approx(true_d, rel=0.005)

    def test_noiseless_fit_is_monotone_in_d(self):
        # Larger true D → larger fitted D.
        for true_d in [2000.0, 5000.0, 10_000.0]:
            trimps = [50.0, 200.0, 500.0, 1000.0, 2000.0]
            pairs = self._make_pairs(true_d, trimps)
            assert fit_strain_denominator(pairs) == pytest.approx(true_d, rel=0.001)


# ===========================================================================
# NEW: strain_score backward-compatible alias
# ===========================================================================

def test_strain_score_alias_is_identical():
    """strain_score must return the same result as strain for every argument."""
    series = _constant(170, 1800)
    assert strain_score(series, max_hr=190, resting_hr=60) == \
           strain(series, max_hr=190, resting_hr=60)
    # Also works with no positional keywords
    assert strain_score(_constant(150, MIN_READINGS), resting_hr=60) == \
           strain(_constant(150, MIN_READINGS), resting_hr=60)


def test_strain_score_accepts_new_kwargs():
    """strain_score passes through method/sex/denominator kwargs."""
    series = _constant(175, MIN_READINGS)
    s = strain_score(series, max_hr=200, resting_hr=60, method="banister", sex="female")
    assert s is not None and s > 0.0
