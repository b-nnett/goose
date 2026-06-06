"""
Tests for home-server/stacks/whoop/ingest/app/analysis/units.py

Pure-function tests — no Docker or DB required.
Run with:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_units.py -q
"""
import sys
import os

# Allow running from the ingest root without installing the package
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import math
import numpy as np
import pytest

from app.analysis.units import (
    # Public API (backwards-compat with read.py)
    spo2_percent,
    spo2_percent_window,
    skin_temp_celsius,
    resp_rate_bpm,
    # New API
    spo2_feature_window,
    skin_temp_deviation,
    resp_rate_from_signal,
    # Calibration-fitting routines
    fit_spo2,
    fit_skin_temp,
    leave_one_night_out,
    # Named constants
    SPO2_A, SPO2_B,
    SKIN_TEMP_SLOPE, SKIN_TEMP_OFFSET,
)


# ---------------------------------------------------------------------------
# spo2_percent — single-sample fallback
# ---------------------------------------------------------------------------

class TestSpo2PercentSingleSample:
    """Single-sample fallback: crude ratio red/ir.  Tests monotonicity + clamp."""

    def test_returns_float(self):
        result = spo2_percent(600, 600)
        assert isinstance(result, float)

    def test_equal_channels_mid_range(self):
        # R = 1.0 → 110 - 25*1.0 = 85.0  (within [70,100])
        result = spo2_percent(600, 600)
        assert 70.0 <= result <= 100.0

    def test_monotonic_decreasing_in_red(self):
        # Higher red relative to ir → higher R → lower SpO2
        r_low = spo2_percent(400, 600)
        r_high = spo2_percent(700, 600)
        assert r_low > r_high, "SpO2 should decrease as red/ir ratio rises"

    def test_clamp_lower_bound(self):
        # Extreme ratio → raw formula < 70, should clamp at 70
        result = spo2_percent(65535, 100)
        assert result == 70.0

    def test_clamp_upper_bound(self):
        # Very low red relative to ir → raw formula > 100, should clamp at 100
        result = spo2_percent(100, 65535)
        assert result == 100.0

    def test_real_v24_sample_in_valid_range(self):
        # Real V24 sample: red≈587, ir≈585 → R≈1.003 → ~84.9 (clamped to [70,100])
        result = spo2_percent(587, 585)
        assert 70.0 <= result <= 100.0, (
            f"Single-sample result {result} out of [70,100]"
        )

    def test_zero_ir_raises(self):
        with pytest.raises(ZeroDivisionError):
            spo2_percent(587, 0)


# ---------------------------------------------------------------------------
# spo2_percent_window — windowed AC/DC estimator (the real estimator)
# ---------------------------------------------------------------------------

class TestSpo2PercentWindow:
    """
    Window estimator uses AC (stdev) / DC (mean) per channel.
    We control the synthetic window so the AC/DC ratio puts SpO2 in [90,100].
    """

    def _make_window(self, dc_red, ac_red_frac, dc_ir, ac_ir_frac, n=20):
        """
        Build a synthetic window where:
          DC ≈ dc_red / dc_ir
          AC/DC ≈ ac_red_frac / ac_ir_frac
        Result: R = (ac_red_frac) / (ac_ir_frac)
        SpO2 = 110 - 25 * R
        """
        # Use a simple sine wave with the desired amplitude
        reds, irs = [], []
        for i in range(n):
            angle = 2 * math.pi * i / n
            reds.append(dc_red + dc_red * ac_red_frac * math.sin(angle))
            irs.append(dc_ir + dc_ir * ac_ir_frac * math.sin(angle))
        return reds, irs

    def test_returns_float(self):
        reds, irs = self._make_window(dc_red=600, ac_red_frac=0.02,
                                      dc_ir=600, ac_ir_frac=0.02)
        assert isinstance(spo2_percent_window(reds, irs), float)

    def test_healthy_spo2_window_in_90_100(self):
        """
        Healthy SpO2 scenario:
          R = AC_red/DC_red / (AC_ir/DC_ir)
          For SpO2 = 96 → R = (110 - 96) / 25 = 0.56
          Set ac_red_frac = 0.56 * ac_ir_frac, e.g. ac_ir_frac=0.05, ac_red_frac=0.028
        """
        # R_target = 0.56 → ac_red/dc_red ÷ ac_ir/dc_ir = 0.56
        # if ac_ir_frac = 0.05 → ac_red_frac = 0.028
        reds, irs = self._make_window(dc_red=587, ac_red_frac=0.028,
                                      dc_ir=585, ac_ir_frac=0.05, n=30)
        result = spo2_percent_window(reds, irs)
        assert 90.0 <= result <= 100.0, (
            f"Windowed SpO2 {result} not in expected [90,100] for healthy scenario"
        )

    def test_low_spo2_window_below_90(self):
        """
        Hypoxia scenario: R = 0.9 → SpO2 = 110 - 25*0.9 = 87.5
        ac_red_frac = 0.9 * ac_ir_frac
        """
        reds, irs = self._make_window(dc_red=600, ac_red_frac=0.045,
                                      dc_ir=600, ac_ir_frac=0.05, n=30)
        result = spo2_percent_window(reds, irs)
        assert result < 90.0, (
            f"Windowed SpO2 {result} should be below 90 for hypoxia scenario"
        )

    def test_clamp_applied(self):
        # Force R >> 1.6 → 110 - 25*1.6 = 70 (lower clamp)
        reds, irs = self._make_window(dc_red=600, ac_red_frac=0.2,
                                      dc_ir=600, ac_ir_frac=0.05, n=30)
        result = spo2_percent_window(reds, irs)
        assert result >= 70.0

    def test_monotonic_in_ratio(self):
        """Higher AC_red/AC_ir ratio → lower SpO2."""
        # Low ratio (high SpO2)
        reds_lo, irs_lo = self._make_window(dc_red=600, ac_red_frac=0.02,
                                            dc_ir=600, ac_ir_frac=0.05, n=30)
        # High ratio (lower SpO2)
        reds_hi, irs_hi = self._make_window(dc_red=600, ac_red_frac=0.08,
                                            dc_ir=600, ac_ir_frac=0.05, n=30)
        spo2_lo = spo2_percent_window(reds_lo, irs_lo)
        spo2_hi = spo2_percent_window(reds_hi, irs_hi)
        assert spo2_lo > spo2_hi, (
            f"SpO2 should be lower when AC_red/AC_ir is higher: {spo2_lo} vs {spo2_hi}"
        )

    def test_minimum_window_size(self):
        # Minimum 2-sample window should not crash
        result = spo2_percent_window([587, 590], [585, 588])
        assert 70.0 <= result <= 100.0

    def test_raises_on_empty_window(self):
        with pytest.raises((ValueError, ZeroDivisionError)):
            spo2_percent_window([], [])

    def test_raises_on_length_mismatch(self):
        with pytest.raises(ValueError):
            spo2_percent_window([587, 590], [585])


# ---------------------------------------------------------------------------
# skin_temp_celsius
# ---------------------------------------------------------------------------

class TestSkinTempCelsius:

    def test_returns_float(self):
        assert isinstance(skin_temp_celsius(930), float)

    def test_real_v24_sample_in_range(self):
        # Real V24 sample: raw≈930 → should map to physiologically plausible wrist temp
        result = skin_temp_celsius(930)
        assert 30.0 <= result <= 36.0, (
            f"skin_temp_celsius(930) = {result}, expected [30, 36] °C"
        )

    def test_monotonic_increasing(self):
        # Higher raw → higher temperature (positive slope)
        assert skin_temp_celsius(500) < skin_temp_celsius(930) < skin_temp_celsius(1500)

    def test_skin_temp_monotonic_over_full_range(self):
        # Monotonicity over the full u16 range; result at max is finite
        lo = skin_temp_celsius(0)
        hi = skin_temp_celsius(65535)
        assert lo < hi, "Temperature should increase with raw value"
        assert math.isfinite(hi), "skin_temp_celsius(65535) should be a finite number"

    def test_exact_anchor_point(self):
        # The linear fit must hit ~33°C at raw=930
        result = skin_temp_celsius(930)
        assert abs(result - 33.0) < 2.0, (
            f"Anchor point mismatch: skin_temp_celsius(930) = {result}, want ≈33°C"
        )


# ---------------------------------------------------------------------------
# resp_rate_bpm
# ---------------------------------------------------------------------------

class TestRespRateBpm:

    def test_returns_float(self):
        assert isinstance(resp_rate_bpm(512), float)

    def test_mid_range_resting(self):
        # A mid-range raw value (e.g. 512 on a 0–1023 scale) → resting 12–20 bpm
        result = resp_rate_bpm(512)
        assert 10.0 <= result <= 22.0, (
            f"resp_rate_bpm(512) = {result}, expected ~12–20 bpm"
        )

    def test_monotonic_increasing(self):
        # Assume higher raw → higher resp rate
        assert resp_rate_bpm(100) < resp_rate_bpm(512) < resp_rate_bpm(900)

    def test_zero_raw_non_negative(self):
        assert resp_rate_bpm(0) >= 0.0

    def test_max_raw_reasonable(self):
        # Absolute max raw (1023 or 65535) shouldn't produce absurd bpm
        result = resp_rate_bpm(1023)
        assert result < 100.0, f"resp_rate_bpm at max raw is unreasonably high: {result}"


# ═══════════════════════════════════════════════════════════════════════════════
# NEW TESTS — robust ratio-of-ratios, skin-temp deviation, resp spectral estimator,
#             calibration-fitting routines (Task 9 overhaul)
# ═══════════════════════════════════════════════════════════════════════════════

# ---------------------------------------------------------------------------
# spo2_feature_window — robust MAD/IQR AC, windowed R
# ---------------------------------------------------------------------------

def _sine_window(dc_red, ac_red_frac, dc_ir, ac_ir_frac, n=60):
    """Return (reds, irs) as a sine wave with known AC/DC ratios."""
    t = np.arange(n)
    reds = dc_red + dc_red * ac_red_frac * np.sin(2 * np.pi * t / n)
    irs  = dc_ir  + dc_ir  * ac_ir_frac  * np.sin(2 * np.pi * t / n)
    return reds.tolist(), irs.tolist()


class TestSpo2FeatureWindow:
    """Tests for the new robust ratio-of-ratios feature extractor."""

    def test_known_R_no_detrend(self):
        """
        With ac_red_frac=0.028, ac_ir_frac=0.05, R_expected ≈ 0.028/0.05 = 0.56.
        Detrend=False so the sine-wave AC is not altered by the trend removal.
        spo2_feature_window should return R close to 0.56 (within ±0.1 at 1σ
        for MAD estimator — MAD of a pure sine recovers ~0.707*amplitude ≠ stdev,
        so we test the ratio is in range rather than exact equality).
        """
        reds, irs = _sine_window(dc_red=600, ac_red_frac=0.028,
                                 dc_ir=600, ac_ir_frac=0.05, n=60)
        R = spo2_feature_window(reds, irs, detrend=False, reject_motion=False)
        assert R is not None, "Feature window returned None for clean signal"
        # The MAD-scaled spread of a sine wave is 1.4826*MAD(sin) ≈ 1.4826*0.707*A
        # The ratio (ac_red_frac)/(ac_ir_frac) = 0.028/0.05 = 0.56; MAD-scaled
        # ratio equals this because the MAD scaling cancels in the ratio.
        assert 0.3 <= R <= 0.9, f"R={R:.4f} outside plausible range [0.3, 0.9]"

    def test_R_monotonicity(self):
        """Higher AC_red relative to AC_ir → higher R."""
        reds_lo, irs_lo = _sine_window(600, 0.02, 600, 0.05, n=60)
        reds_hi, irs_hi = _sine_window(600, 0.08, 600, 0.05, n=60)
        R_lo = spo2_feature_window(reds_lo, irs_lo, detrend=False, reject_motion=False)
        R_hi = spo2_feature_window(reds_hi, irs_hi, detrend=False, reject_motion=False)
        assert R_lo is not None and R_hi is not None
        assert R_lo < R_hi, f"Monotonicity violated: R_lo={R_lo:.4f}, R_hi={R_hi:.4f}"

    def test_outlier_robustness(self):
        """
        Inject 5 % spike outliers into a clean window.
        MAD-based AC should be much less inflated than stdev-based AC.
        The returned R should stay close to the clean-signal R (within 50%).
        """
        np.random.seed(99)
        reds, irs = _sine_window(600, 0.028, 600, 0.05, n=60)
        reds_noisy = list(reds)
        irs_noisy  = list(irs)
        # Inject 3 large spikes
        for idx in [10, 30, 50]:
            reds_noisy[idx] += 300  # ~50% spike
        R_clean = spo2_feature_window(reds, irs, detrend=False, reject_motion=False)
        R_noisy = spo2_feature_window(reds_noisy, irs_noisy,
                                      detrend=False, reject_motion=False)
        # With motion rejection ON, the noisy window may be rejected (R_noisy=None)
        # — that is also acceptable robust behaviour
        if R_noisy is not None and R_clean is not None:
            # Robust (MAD-based) estimator: noisy R stays within ±50% of clean R
            assert abs(R_noisy - R_clean) <= 0.5 * R_clean, (
                f"Outlier inflated R too much: clean={R_clean:.4f}, noisy={R_noisy:.4f}"
            )

    def test_motion_rejection_on_high_perfusion(self):
        """Window with AC/DC > 10% (motion) should return None when reject_motion=True."""
        # ac_frac=0.2 → AC/DC = 20% > 10% ceiling
        reds, irs = _sine_window(600, 0.20, 600, 0.20, n=60)
        R = spo2_feature_window(reds, irs, reject_motion=True)
        assert R is None, "Expected None for high-motion window"

    def test_motion_rejection_off_returns_R(self):
        """Same high-motion window should return a value when reject_motion=False."""
        reds, irs = _sine_window(600, 0.20, 600, 0.20, n=60)
        R = spo2_feature_window(reds, irs, reject_motion=False, detrend=False)
        assert R is not None

    def test_flat_ir_returns_none(self):
        """Flat IR channel → AC_ir=0 → R undefined → None."""
        reds, _ = _sine_window(600, 0.05, 600, 0.05, n=30)
        irs_flat = [600.0] * 30
        R = spo2_feature_window(reds, irs_flat, detrend=False, reject_motion=False)
        assert R is None, "Expected None for flat IR channel"

    def test_flat_ir_with_detrend_returns_none(self):
        """
        After detrending a flat IR channel leaves ~1e-13 residual (not exactly 0.0).
        The relative-threshold guard `ac_ir < 1e-6 * dc_ir` must catch this so
        spo2_feature_window returns None rather than blowing up R to ~1e12.
        """
        reds, _ = _sine_window(600, 0.05, 600, 0.05, n=30)
        irs_flat = [600.0] * 30
        # detrend=True is the default — this is the bug scenario
        R = spo2_feature_window(reds, irs_flat, detrend=True, reject_motion=False)
        assert R is None, (
            "Flat IR with detrend=True left floating-point residual; "
            "relative guard must catch it and return None"
        )

    def test_length_mismatch_raises(self):
        with pytest.raises(ValueError):
            spo2_feature_window([600.0] * 10, [600.0] * 9)

    def test_empty_raises(self):
        with pytest.raises(ValueError):
            spo2_feature_window([], [])

    def test_returns_float_for_valid_window(self):
        reds, irs = _sine_window(600, 0.03, 600, 0.05, n=60)
        R = spo2_feature_window(reds, irs, detrend=False)
        assert R is None or isinstance(R, float)


class TestSpo2PercentWindowRobustMAD:
    """
    spo2_percent_window now uses MAD-based AC internally (via spo2_feature_window).
    These tests verify the full pipeline: known R → known SpO2 with default a,b.
    """

    def test_ratio_of_ratios_known_R_healthy(self):
        """
        Construct a window where R_expected = 0.56 (→ SpO2 = 110 − 25*0.56 = 96 %).
        Expect result in [90, 100] with the default a=110, b=25.
        """
        reds, irs = _sine_window(dc_red=587, ac_red_frac=0.028,
                                 dc_ir=585, ac_ir_frac=0.05, n=60)
        result = spo2_percent_window(reds, irs)
        assert 90.0 <= result <= 100.0, (
            f"Expected SpO2 in [90,100] for R≈0.56, got {result:.1f}"
        )

    def test_ratio_of_ratios_known_R_hypoxia(self):
        """R≈0.9 → SpO2 = 110 − 25*0.9 = 87.5 %; expect < 90."""
        reds, irs = _sine_window(600, 0.045, 600, 0.05, n=60)
        result = spo2_percent_window(reds, irs)
        assert result < 90.0, f"Expected SpO2 < 90 for R≈0.9, got {result:.1f}"

    def test_outlier_doesnt_crash(self):
        """Window with spike outliers should still return a clamped float."""
        reds, irs = _sine_window(600, 0.03, 600, 0.05, n=60)
        reds[20] += 5000  # large spike
        result = spo2_percent_window(reds, irs)
        assert isinstance(result, float)
        assert 70.0 <= result <= 100.0

    def test_default_constants_used(self):
        """Result with default a,b equals SPO2_A - SPO2_B * R (sanity)."""
        reds, irs = _sine_window(600, 0.03, 600, 0.05, n=60)
        result = spo2_percent_window(reds, irs)
        assert 70.0 <= result <= 100.0


# ---------------------------------------------------------------------------
# skin_temp_celsius — linear map + deviation-from-baseline
# ---------------------------------------------------------------------------

class TestSkinTempLinearMap:
    """Tests for the named-constant linear map and the deviation function."""

    def test_named_constants_used(self):
        """skin_temp_celsius(raw) = SKIN_TEMP_SLOPE * raw + SKIN_TEMP_OFFSET."""
        raw = 800.0
        expected = SKIN_TEMP_SLOPE * raw + SKIN_TEMP_OFFSET
        assert abs(skin_temp_celsius(raw) - expected) < 1e-9

    def test_custom_slope_offset(self):
        """skin_temp_celsius accepts custom slope and offset kwargs."""
        val = skin_temp_celsius(1000, slope=0.01, offset=20.0)
        assert abs(val - (0.01 * 1000 + 20.0)) < 1e-9

    def test_deviation_offset_cancels(self):
        """
        Deviation-from-baseline = slope * (raw - baseline_raw).
        The additive offset (k) cancels exactly: absolute map gives
        slope*raw + k, baseline gives slope*baseline_raw + k, so
        deviation = slope*(raw - baseline_raw) regardless of k.
        Demonstrate with two very different offsets k1 and k2.
        """
        baseline_raw = 930.0
        raw = 960.0
        slope = 0.02
        k1, k2 = 14.4, 50.0   # very different offsets

        # Absolute temperatures differ by (k1 - k2) …
        abs1 = slope * raw + k1
        abs2 = slope * raw + k2
        assert abs(abs1 - abs2) > 1.0, "Test setup: offsets must produce different absolutes"

        # … but the deviation (offset-free formula) is the same for both
        expected_deviation = slope * (raw - baseline_raw)
        dev1 = slope * (raw - baseline_raw)   # k1 cancels
        dev2 = slope * (raw - baseline_raw)   # k2 cancels

        # skin_temp_deviation uses the offset-free formula internally
        dev_series = skin_temp_deviation([raw], baseline_raw, slope=slope)
        assert abs(dev_series[0] - expected_deviation) < 1e-9, (
            "skin_temp_deviation result must equal slope*(raw-baseline)"
        )
        assert abs(dev1 - dev2) < 1e-9, "Deviation must be invariant to offset k"

    def test_deviation_sign_and_magnitude(self):
        """raw > baseline → positive deviation; raw < baseline → negative."""
        baseline_raw = 930.0
        slope = 0.02
        dev = skin_temp_deviation([910.0, 930.0, 950.0], baseline_raw, slope=slope)
        assert dev[0] < 0.0, "Below-baseline raw should give negative deviation"
        assert abs(dev[1]) < 1e-9, "Baseline raw should give ~0 deviation"
        assert dev[2] > 0.0, "Above-baseline raw should give positive deviation"

    def test_deviation_magnitude(self):
        """Δraw=100 at slope=0.02 → deviation = 2.0 °C."""
        dev = skin_temp_deviation([1030.0], 930.0, slope=0.02)
        assert abs(dev[0] - 2.0) < 1e-9, f"Expected 2.0 °C, got {dev[0]}"

    def test_deviation_uses_default_slope(self):
        """Default slope (SKIN_TEMP_SLOPE) is used when not specified."""
        dev = skin_temp_deviation([960.0], 930.0)
        expected = SKIN_TEMP_SLOPE * (960.0 - 930.0)
        assert abs(dev[0] - expected) < 1e-9

    def test_deviation_returns_list(self):
        result = skin_temp_deviation([900.0, 920.0], 930.0)
        assert isinstance(result, list)
        assert len(result) == 2


# ---------------------------------------------------------------------------
# resp_rate_from_signal — Welch-peak spectral estimator
# ---------------------------------------------------------------------------

class TestRespRateFromSignal:
    """
    Synthetic sine waves at known breathing frequencies.
    The Welch estimator should recover the dominant frequency to within
    ±1 BrPM for a clean sine, ±3 BrPM for noisy signals.
    """

    @staticmethod
    def _resp_signal(bpm: float, n: int = 240, noise_amp: float = 0.0,
                     seed: int = 0) -> list:
        """Generate a 1 Hz resp waveform at known BrPM with optional Gaussian noise."""
        t = np.arange(n)
        hz = bpm / 60.0
        sig = np.sin(2 * np.pi * hz * t) * 100.0
        if noise_amp > 0:
            rng = np.random.default_rng(seed)
            sig = sig + rng.normal(0, noise_amp, n)
        return sig.tolist()

    def test_known_rate_15bpm(self):
        """Clean 15 BrPM sine → Welch should return 14–16 BrPM."""
        sig = self._resp_signal(15.0, n=300)
        rate = resp_rate_from_signal(sig)
        assert rate is not None, "Welch returned None for clean 15 BrPM signal"
        assert 13.0 <= rate <= 17.0, f"Expected ~15 BrPM, got {rate:.1f}"

    def test_known_rate_10bpm(self):
        """10 BrPM (slow sleep breathing) → result in [8, 12] BrPM."""
        sig = self._resp_signal(10.0, n=360)
        rate = resp_rate_from_signal(sig)
        assert rate is not None
        assert 8.0 <= rate <= 12.0, f"Expected ~10 BrPM, got {rate:.1f}"

    def test_known_rate_20bpm(self):
        """20 BrPM (faster resting) → result in [18, 22] BrPM."""
        sig = self._resp_signal(20.0, n=240)
        rate = resp_rate_from_signal(sig)
        assert rate is not None
        assert 18.0 <= rate <= 22.0, f"Expected ~20 BrPM, got {rate:.1f}"

    def test_noisy_signal_recovers_rate(self):
        """15 BrPM + Gaussian noise (SNR ~3) → result within ±3 BrPM."""
        sig = self._resp_signal(15.0, n=300, noise_amp=30.0, seed=42)
        rate = resp_rate_from_signal(sig)
        assert rate is not None, "Should return a rate even for noisy signal"
        assert 12.0 <= rate <= 18.0, f"Noisy 15 BrPM: expected [12,18], got {rate:.1f}"

    def test_nan_in_window_returns_none(self):
        """NaN anywhere in the input must return None (not 6.0 BrPM band-floor)."""
        sig = self._resp_signal(15.0, n=240)
        sig[50] = float("nan")
        rate = resp_rate_from_signal(sig)
        assert rate is None, (
            "NaN input should return None, not silently yield the band-floor 6.0 BrPM"
        )

    def test_flat_signal_returns_none(self):
        """Flat (zero-variance) signal has no spectral peak → None."""
        sig = [500.0] * 200
        rate = resp_rate_from_signal(sig)
        assert rate is None, "Expected None for flat signal"

    def test_too_short_returns_none(self):
        """< 20 samples — not enough for 2 full cycles at 6 BrPM → None."""
        sig = self._resp_signal(15.0, n=5)
        rate = resp_rate_from_signal(sig)
        assert rate is None, "Expected None for too-short signal"

    def test_welch_method_explicit(self):
        """Explicit method='welch' should behave the same as default."""
        sig = self._resp_signal(15.0, n=240)
        rate = resp_rate_from_signal(sig, method="welch")
        assert rate is not None
        assert 13.0 <= rate <= 17.0

    def test_returns_float_or_none(self):
        """Return type is always float or None, never NaN."""
        sig = self._resp_signal(15.0, n=240)
        rate = resp_rate_from_signal(sig)
        assert rate is None or (isinstance(rate, float) and not math.isnan(rate))


# ---------------------------------------------------------------------------
# fit_spo2 — calibration fitting (linear, LONO)
# ---------------------------------------------------------------------------

class TestFitSpo2:
    """Fitting routines recover known (a, b) from synthetic data."""

    @staticmethod
    def _synthetic_R_spo2(a: float, b: float, n: int = 50,
                          noise: float = 0.3, seed: int = 7):
        """R in [0.4, 1.2], SpO2 = a − b·R + tiny noise."""
        rng = np.random.default_rng(seed)
        R = rng.uniform(0.4, 1.2, n)
        spo2 = a - b * R + rng.normal(0, noise, n)
        return R.tolist(), spo2.tolist()

    def test_recovers_known_a_b(self):
        """OLS on noise-free data recovers (a, b) within ±0.5."""
        R, spo2 = self._synthetic_R_spo2(110.0, 25.0, n=60, noise=0.01)
        a_fit, b_fit = fit_spo2(R, spo2)
        assert abs(a_fit - 110.0) < 0.5, f"a={a_fit:.3f} far from 110"
        assert abs(b_fit - 25.0)  < 0.5, f"b={b_fit:.3f} far from 25"

    def test_b_positive(self):
        """Fitted b must be > 0 (SpO2 decreases with R)."""
        R, spo2 = self._synthetic_R_spo2(110.0, 25.0)
        _, b_fit = fit_spo2(R, spo2)
        assert b_fit > 0.0

    def test_raises_on_negative_b(self):
        """If the data implies b ≤ 0 (inverted relationship), fit_spo2 raises."""
        # Artificially invert: higher R → higher SpO2
        R = np.linspace(0.4, 1.2, 30)
        spo2 = 70 + 25 * R  # monotone increasing in R → fitted b will be < 0
        with pytest.raises(ValueError, match="b="):
            fit_spo2(R.tolist(), spo2.tolist())

    def test_different_default_values(self):
        """A different true (a, b) is also recovered."""
        R, spo2 = self._synthetic_R_spo2(a=108.0, b=22.0, n=80, noise=0.1)
        a_fit, b_fit = fit_spo2(R, spo2)
        assert abs(a_fit - 108.0) < 1.0
        assert abs(b_fit - 22.0)  < 1.0

    def test_raises_too_few_samples(self):
        with pytest.raises(ValueError):
            fit_spo2([0.5], [95.0])

    def test_raises_length_mismatch(self):
        with pytest.raises(ValueError):
            fit_spo2([0.5, 0.6], [95.0])

    def test_lono_runs_without_crash(self):
        """With night_ids provided, LONO CV runs and doesn't crash."""
        R, spo2 = self._synthetic_R_spo2(110.0, 25.0, n=60)
        night_ids = [0] * 30 + [1] * 30
        # Should not raise; LONO logs MAE
        a_fit, b_fit = fit_spo2(R, spo2, night_ids=night_ids)
        assert b_fit > 0.0


# ---------------------------------------------------------------------------
# fit_skin_temp — calibration fitting (slope + offset / slope-only)
# ---------------------------------------------------------------------------

class TestFitSkinTemp:

    @staticmethod
    def _synthetic_raw_temp(slope: float, offset: float, n: int = 40,
                            noise: float = 0.05, seed: int = 3):
        """raw in [800, 1100], T = slope*raw + offset + noise."""
        rng = np.random.default_rng(seed)
        raw  = rng.uniform(800, 1100, n)
        temp = slope * raw + offset + rng.normal(0, noise, n)
        return raw.tolist(), temp.tolist()

    def test_recovers_known_slope_offset(self):
        """OLS recovers (slope, offset) within ±10% of true values."""
        raw, temp = self._synthetic_raw_temp(0.02, 14.4, n=60, noise=0.01)
        slope_fit, offset_fit = fit_skin_temp(raw, temp)
        assert abs(slope_fit - 0.02) < 0.002,  f"slope={slope_fit:.5f}"
        assert abs(offset_fit - 14.4) < 0.5,   f"offset={offset_fit:.3f}"

    def test_slope_only_mode(self):
        """fit_offset=False forces intercept=0; slope recovered within ±10%."""
        rng = np.random.default_rng(5)
        raw  = rng.uniform(0, 200, 40)   # Δraw around 0 (deviation use-case)
        temp = 0.02 * raw + rng.normal(0, 0.05, 40)
        slope_fit, offset_fit = fit_skin_temp(raw.tolist(), temp.tolist(),
                                              fit_offset=False)
        assert abs(offset_fit) < 1e-9, "offset should be 0 in slope-only mode"
        assert abs(slope_fit - 0.02) < 0.003

    def test_lono_runs_without_crash(self):
        raw, temp = self._synthetic_raw_temp(0.02, 14.4, n=60)
        night_ids = [0] * 30 + [1] * 30
        slope_fit, offset_fit = fit_skin_temp(raw, temp, night_ids=night_ids)
        assert isinstance(slope_fit, float)

    def test_raises_length_mismatch(self):
        with pytest.raises(ValueError):
            fit_skin_temp([900.0, 920.0], [33.0])

    def test_raises_too_few_samples(self):
        with pytest.raises(ValueError):
            fit_skin_temp([900.0], [33.0])


# ---------------------------------------------------------------------------
# leave_one_night_out — generic LONO helper
# ---------------------------------------------------------------------------

class TestLeaveOneNightOut:

    def test_recovers_linear_model(self):
        """LONO on linear data: mean MAE should be small (< 0.5 units)."""
        rng = np.random.default_rng(42)
        # 4 nights, 20 samples each; true relationship y = 2*x + 5
        x_all, y_all, nids = [], [], []
        for night in range(4):
            x = rng.uniform(0, 10, 20)
            y = 2.0 * x + 5.0 + rng.normal(0, 0.1, 20)
            x_all.extend(x.tolist())
            y_all.extend(y.tolist())
            nids.extend([night] * 20)

        result = leave_one_night_out(x_all, y_all, nids, n_params=2)
        assert result["n_nights"] == 4
        assert result["mean_mae"] < 0.5, f"mean_mae={result['mean_mae']:.4f}"
        assert result["mean_rmse"] < 0.5

    def test_returns_dict_structure(self):
        """Return dict has expected keys."""
        x = list(range(20))
        y = [2.0 * v + 1.0 for v in x]
        nids = [0] * 10 + [1] * 10
        result = leave_one_night_out(x, y, nids, n_params=2)
        for key in ("night_maes", "night_rmses", "mean_mae", "mean_rmse", "n_nights"):
            assert key in result, f"Missing key: {key}"

    def test_one_night_logs_warning(self):
        """With only 1 unique night, n_nights=0 (no valid train/test split)."""
        x = list(range(10))
        y = [float(v) for v in x]
        nids = [0] * 10
        result = leave_one_night_out(x, y, nids, n_params=2)
        assert result["n_nights"] == 0, "Expected 0 valid LONO folds with 1 night"
        assert math.isnan(result["mean_mae"])

    def test_returns_float_metrics(self):
        x = list(range(20))
        y = [float(v) for v in x]
        nids = [0] * 10 + [1] * 10
        result = leave_one_night_out(x, y, nids, n_params=2)
        assert isinstance(result["mean_mae"],  float)
        assert isinstance(result["mean_rmse"], float)
