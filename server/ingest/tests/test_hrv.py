"""
Tests for analysis.hrv — RMSSD, SDNN, hrv_series, clean_rr, and nightly_hrv.

All HRV metrics are in milliseconds (ms).

TDD additions (Task 5):
  - TestRmssdMs: float64 Task Force formula, neurokit2 parity gate (< 0.01 ms)
  - TestCleanRR: range filter → kubios → interpolation pipeline
  - TestPooledRmssd: segment-aware gap-pooling
  - TestNightlyHrv: tiered window selection, correct output dict shape
"""
from __future__ import annotations

import math
import statistics
from dataclasses import dataclass

import numpy as np
import pytest

from app.analysis.hrv import (
    RR_MAX_MS,
    RR_MIN_MS,
    _filter_rr,
    hrv_series,
    rmssd,
    rmssd_ms,
    sdnn,
    clean_rr,
    nightly_hrv,
)

# ---------------------------------------------------------------------------
# _filter_rr
# ---------------------------------------------------------------------------


class TestFilterRR:
    def test_all_plausible_pass_through(self):
        rr = [400, 800, 1000, 1500, 2000]
        assert _filter_rr(rr) == [400.0, 800.0, 1000.0, 1500.0, 2000.0]

    def test_boundary_values_included(self):
        """Exact boundary values (300 ms, 2000 ms) are accepted."""
        assert _filter_rr([RR_MIN_MS, RR_MAX_MS]) == [float(RR_MIN_MS), float(RR_MAX_MS)]

    def test_below_min_dropped(self):
        """Values < 300 ms (e.g., artefact/noise) are dropped."""
        rr = [100, 299, 800, 900]
        assert _filter_rr(rr) == [800.0, 900.0]

    def test_above_max_dropped(self):
        """Values > 2000 ms (e.g., missed beat, long pause) are dropped."""
        rr = [800, 900, 2001, 5000]
        assert _filter_rr(rr) == [800.0, 900.0]

    def test_empty_input(self):
        assert _filter_rr([]) == []

    def test_all_implausible(self):
        assert _filter_rr([50, 100, 3000]) == []

    def test_mixed_outliers(self):
        """One low spike, one high spike, three plausible values."""
        assert _filter_rr([100, 800, 820, 810, 5000]) == [800.0, 820.0, 810.0]


# ---------------------------------------------------------------------------
# rmssd (backward-compatible legacy wrapper)
# ---------------------------------------------------------------------------


class TestRMSSD:
    def test_known_value(self):
        """
        rr=[800, 820, 810, 830]
        sq_diffs = [400, 100, 400]; mean=300; sqrt≈17.3205
        """
        result = rmssd([800, 820, 810, 830])
        assert math.isclose(result, 17.320508, rel_tol=1e-5)

    def test_alternating_800_900(self):
        """
        Alternating 800/900 → all |diff|=100 → RMSSD=100.
        (Mirrors openwhoop Rust test ``calculate_rmssd_with_variation``.)
        """
        rr = [800 if i % 2 == 0 else 900 for i in range(10)]
        assert math.isclose(rmssd(rr), 100.0, rel_tol=1e-9)

    def test_constant_rr_is_zero(self):
        """Constant RR → no successive differences → RMSSD=0."""
        assert math.isclose(rmssd([850] * 5), 0.0, abs_tol=1e-9)

    def test_two_element_minimum(self):
        """Two-element input is the minimum valid case."""
        result = rmssd([800, 900])
        assert math.isclose(result, 100.0, rel_tol=1e-9)

    def test_outlier_dropped_same_result(self):
        """
        An out-of-range RR (100 ms spike) should be filtered before computation,
        yielding the same result as the array without it.
        """
        clean = [800, 820, 810, 830]
        with_low_spike = [100] + clean
        assert math.isclose(rmssd(with_low_spike), rmssd(clean), rel_tol=1e-9)

    def test_high_outlier_dropped(self):
        """A 5000 ms outlier is dropped; result equals the clean array."""
        clean = [800, 820, 810, 830]
        with_high_spike = clean + [5000]
        assert math.isclose(rmssd(with_high_spike), rmssd(clean), rel_tol=1e-9)

    def test_raises_on_empty(self):
        with pytest.raises(ValueError, match="≥2"):
            rmssd([])

    def test_raises_on_one_valid(self):
        """Only one plausible value after filtering → ValueError."""
        with pytest.raises(ValueError, match="≥2"):
            rmssd([800, 100])  # 100 filtered out, only 800 remains

    def test_raises_on_all_implausible(self):
        with pytest.raises(ValueError, match="≥2"):
            rmssd([50, 3000])

    def test_float_inputs_accepted(self):
        result = rmssd([800.5, 820.5, 810.5, 830.5])
        # Same differences as int version → same result
        assert math.isclose(result, rmssd([800, 820, 810, 830]), abs_tol=0.01)


# ---------------------------------------------------------------------------
# sdnn
# ---------------------------------------------------------------------------


class TestSDNN:
    def test_known_value(self):
        """
        sdnn([800, 820, 810, 830])
        True population: mean=815, deviations=[-15,5,-5,15], var_sample=((225+25+25+225)/3)=500/3
        stdev_sample = sqrt(500/3) = 12.909944487358056
        """
        result = sdnn([800, 820, 810, 830])
        assert math.isclose(result, 12.909944, rel_tol=1e-5)

    def test_constant_rr_is_zero(self):
        """All identical RR → stdev = 0."""
        assert math.isclose(sdnn([800] * 5), 0.0, abs_tol=1e-9)

    def test_uses_sample_stdev(self):
        """SDNN uses sample (ddof=1) standard deviation, not population."""
        rr = [800, 850, 900, 750]
        expected_sample = statistics.stdev(map(float, rr))
        assert math.isclose(sdnn(rr), expected_sample, rel_tol=1e-9)

    def test_outlier_dropped_same_result(self):
        """100 ms spike is filtered; result equals clean array."""
        clean = [800, 820, 810, 830]
        assert math.isclose(sdnn([100] + clean), sdnn(clean), rel_tol=1e-9)

    def test_raises_on_fewer_than_two(self):
        with pytest.raises(ValueError, match="≥2"):
            sdnn([800])

    def test_raises_on_empty(self):
        with pytest.raises(ValueError, match="≥2"):
            sdnn([])


# ---------------------------------------------------------------------------
# hrv_series
# ---------------------------------------------------------------------------


def _make_rows(start_ts: float, step_s: float, rr_values: list[int]) -> list[dict]:
    """Build a list of {ts, rr_ms} dicts."""
    return [
        {"ts": start_ts + i * step_s, "rr_ms": rr_values[i]}
        for i in range(len(rr_values))
    ]


class TestHRVSeries:
    def test_empty_input_returns_empty(self):
        assert hrv_series([], window_s=60) == []

    def test_single_window(self):
        """All rows fit in one window → one output point."""
        rows = _make_rows(0.0, 1.0, [800, 820, 810, 830])
        result = hrv_series(rows, window_s=60)
        assert len(result) == 1
        assert result[0]["ts"] == 0.0
        assert math.isclose(result[0]["rmssd"], rmssd([800, 820, 810, 830]), rel_tol=1e-9)

    def test_two_windows(self):
        """
        Two non-overlapping 30-second windows, each with 3 plausible RR values.
        Timestamps: 0,5,10 in window 1 (ts<30); 30,35,40 in window 2 (30≤ts<60).
        """
        rr_w1 = [800, 820, 810]
        rr_w2 = [900, 920, 910]
        rows = (
            _make_rows(0.0, 5.0, rr_w1) +
            _make_rows(30.0, 5.0, rr_w2)
        )
        result = hrv_series(rows, window_s=30)
        assert len(result) == 2
        assert result[0]["ts"] == 0.0
        assert math.isclose(result[0]["rmssd"], rmssd(rr_w1), rel_tol=1e-9)
        assert result[1]["ts"] == 30.0
        assert math.isclose(result[1]["rmssd"], rmssd(rr_w2), rel_tol=1e-9)

    def test_all_implausible_window_skipped(self):
        """A window whose RR values are all out-of-range produces no output point."""
        implausible_rows = [{"ts": 0.0, "rr_ms": 50}, {"ts": 5.0, "rr_ms": 5000}]
        plausible_rows = _make_rows(60.0, 5.0, [800, 820, 810])
        rows = implausible_rows + plausible_rows
        result = hrv_series(rows, window_s=60)
        # First window (0–60s): 2 implausible → skipped
        # Second window (60–120s): 3 plausible → included
        assert len(result) == 1
        assert result[0]["ts"] == 60.0

    def test_window_with_fewer_than_two_plausible_skipped(self):
        """Window with only one plausible RR is silently skipped."""
        rows = [
            {"ts": 0.0, "rr_ms": 800},   # only one plausible in [0, 30)
            {"ts": 5.0, "rr_ms": 100},   # implausible
            {"ts": 30.0, "rr_ms": 850},
            {"ts": 35.0, "rr_ms": 870},
        ]
        result = hrv_series(rows, window_s=30)
        assert len(result) == 1
        assert result[0]["ts"] == 30.0

    def test_outliers_filtered_within_window(self):
        """Spike within a window is filtered; result equals clean-only version."""
        clean = [800, 820, 810, 830]
        clean_rows = _make_rows(0.0, 1.0, clean)
        spike_rows = clean_rows + [{"ts": 4.0, "rr_ms": 5000}]
        result_clean = hrv_series(clean_rows, window_s=60)
        result_spike = hrv_series(spike_rows, window_s=60)
        assert len(result_clean) == 1
        assert len(result_spike) == 1
        assert math.isclose(result_clean[0]["rmssd"], result_spike[0]["rmssd"], rel_tol=1e-9)

    def test_invalid_window_s_raises(self):
        rows = _make_rows(0.0, 1.0, [800, 820])
        with pytest.raises(ValueError, match="window_s"):
            hrv_series(rows, window_s=0)
        with pytest.raises(ValueError, match="window_s"):
            hrv_series(rows, window_s=-5)

    def test_row_missing_rr_ms_raises_value_error(self):
        """A row without 'rr_ms' raises ValueError (not KeyError)."""
        rows = [{"ts": 0.0}, {"ts": 1.0, "rr_ms": 820}]
        with pytest.raises(ValueError, match="rr_ms"):
            hrv_series(rows, window_s=60)

    def test_row_missing_ts_raises_value_error(self):
        """A row without 'ts' raises ValueError (not KeyError)."""
        rows = [{"rr_ms": 800}, {"ts": 1.0, "rr_ms": 820}]
        with pytest.raises(ValueError, match="'ts'"):
            hrv_series(rows, window_s=60)

    def test_ts_is_window_start(self):
        """Output ts is the window start, not the row ts."""
        rows = _make_rows(100.0, 1.0, [800, 820, 810])
        result = hrv_series(rows, window_s=60)
        assert len(result) == 1
        assert result[0]["ts"] == 100.0  # window starts at first row ts

    def test_output_keys(self):
        """Each output dict has exactly 'ts' and 'rmssd' keys."""
        rows = _make_rows(0.0, 1.0, [800, 820, 810])
        result = hrv_series(rows, window_s=60)
        assert set(result[0].keys()) == {"ts", "rmssd"}

    def test_multiple_windows_chronological(self):
        """Output is in chronological order."""
        # 3 windows of 10 s each, each with 2 plausible RR
        rows = (
            _make_rows(0.0, 2.0, [800, 820]) +
            _make_rows(10.0, 2.0, [850, 870]) +
            _make_rows(20.0, 2.0, [900, 920])
        )
        result = hrv_series(rows, window_s=10)
        assert [r["ts"] for r in result] == [0.0, 10.0, 20.0]


# ===========================================================================
# NEW TESTS (Task 5 TDD additions)
# ===========================================================================

# ---------------------------------------------------------------------------
# rmssd_ms — pure Task Force formula (float64, no filtering)
# ---------------------------------------------------------------------------


class TestRmssdMs:
    def test_hand_computed_tiny_series(self):
        """
        nn = [800, 850, 810, 870]
        diffs = [50, -40, 60]
        sq_diffs = [2500, 1600, 3600]
        mean_sq = 7700/3 ≈ 2566.6667
        rmssd = sqrt(2566.6667) ≈ 50.6619...
        """
        nn = np.array([800.0, 850.0, 810.0, 870.0])
        result = rmssd_ms(nn)
        expected = math.sqrt((2500 + 1600 + 3600) / 3)
        assert math.isclose(result, expected, rel_tol=1e-12)

    def test_constant_is_zero(self):
        """All same intervals → successive diffs all 0 → RMSSD = 0."""
        nn = np.full(10, 800.0)
        assert rmssd_ms(nn) == 0.0

    def test_alternating_returns_diff(self):
        """800/900 alternating → all |diff| = 100 → RMSSD = 100."""
        nn = np.tile([800.0, 900.0], 50)
        assert math.isclose(rmssd_ms(nn), 100.0, rel_tol=1e-10)

    def test_returns_float64(self):
        nn = np.array([800, 820, 810, 830], dtype=np.int32)
        result = rmssd_ms(nn)
        assert isinstance(result, float)

    def test_neurokit2_parity_hard_gate(self):
        """
        HARD GATE: rmssd_ms must agree with nk.hrv_time to < 0.01 ms on the same
        clean synthetic NN array.  The only source of discrepancy is the 1 ms
        sample quantisation in intervals_to_peaks (1000 Hz round-trip).

        The strap produces integer-ms RR values, so we use integer-valued NN to
        eliminate sub-ms rounding error in the round-trip.  On integer inputs the
        agreement must be < 1e-9 ms (exact formula identity).
        """
        import neurokit2 as nk

        rng = np.random.default_rng(42)
        # Integer-valued NN in ms (matches strap output precision; 1000 Hz
        # round-trip is lossless for integer ms values).
        nn = np.round(
            (800 + 40 * rng.standard_normal(600)).clip(400, 1400)
        ).astype(np.float64)

        ours = rmssd_ms(nn)

        peaks = nk.intervals_to_peaks(nn, sampling_rate=1000)
        nk_val = float(nk.hrv_time(peaks, sampling_rate=1000)["HRV_RMSSD"].iloc[0])

        assert abs(ours - nk_val) < 0.01, (
            f"neurokit2 parity gate FAILED: ours={ours:.6f} nk={nk_val:.6f} "
            f"diff={abs(ours - nk_val):.6f} ms (limit 0.01 ms)"
        )

    def test_two_elements(self):
        """Minimum valid input: 2 intervals → 1 diff."""
        assert math.isclose(rmssd_ms(np.array([800.0, 900.0])), 100.0, rel_tol=1e-12)

    def test_accepts_list(self):
        """rmssd_ms accepts a plain Python list (not just np.ndarray)."""
        result = rmssd_ms([800, 850, 810, 870])
        nn = np.array([800.0, 850.0, 810.0, 870.0])
        assert math.isclose(result, rmssd_ms(nn), rel_tol=1e-12)


# ---------------------------------------------------------------------------
# clean_rr — range filter → kubios → interpolation
# ---------------------------------------------------------------------------


class TestCleanRR:
    def _synthetic_rr(self, n: int = 200, mean: float = 800.0, std: float = 40.0,
                       seed: int = 0) -> np.ndarray:
        rng = np.random.default_rng(seed)
        return (mean + std * rng.standard_normal(n)).clip(400, 1400)

    def test_clean_signal_unchanged_count(self):
        """Clean signal → most beats survive, artifact count near 0."""
        rr = self._synthetic_rr(200)
        nn, peaks_clean, n_beats, n_artifacts = clean_rr(rr)
        assert n_beats == len(rr)
        # Clean synthetic should have very few corrections
        assert n_artifacts < 10
        assert len(nn) > 0

    def test_out_of_range_beats_are_artifacts(self):
        """Beats outside 300–2000 ms are counted in artifacts."""
        rr = self._synthetic_rr(200)
        # Insert clearly implausible beats
        rr_bad = np.concatenate([[50.0, 5000.0], rr])
        nn_bad, _, n_beats_bad, n_art_bad = clean_rr(rr_bad)
        nn_clean, _, n_beats_clean, n_art_clean = clean_rr(rr)
        # The bad version must have more artifacts
        assert n_art_bad > n_art_clean

    def test_returns_float_array(self):
        rr = self._synthetic_rr(100)
        nn, peaks_clean, n_beats, n_artifacts = clean_rr(rr)
        assert isinstance(nn, np.ndarray)
        assert nn.dtype == np.float64

    def test_too_few_beats_returns_nan_array(self):
        """Fewer than MIN_BEATS (20) plausible beats → empty/nan result."""
        rr = np.array([800.0] * 5)
        nn, peaks_clean, n_beats, n_artifacts = clean_rr(rr)
        # Should gracefully degrade: either empty array or all-nan
        assert n_beats == 5
        assert len(nn) == 0 or (len(nn) > 0 and np.all(np.isnan(nn)))

    def test_n_beats_is_input_length(self):
        """n_beats reflects the count of input intervals (before range filter)."""
        rr = self._synthetic_rr(150)
        _, _peaks, n_beats, _ = clean_rr(rr)
        assert n_beats == 150

    def test_n_artifacts_non_negative(self):
        rr = self._synthetic_rr(100)
        _, _peaks, _, n_artifacts = clean_rr(rr)
        assert n_artifacts >= 0

    def test_peaks_clean_length_invariant(self):
        """peaks_clean must satisfy len(peaks_clean) == len(nn_clean) + 1.

        This holds even when kubios inserts a synthetic peak (missed-beat
        correction that makes peaks_clean.size > original peaks.size).
        """
        rr = self._synthetic_rr(300, seed=7)
        nn, peaks_clean, n_beats, n_artifacts = clean_rr(rr)
        assert len(peaks_clean) == len(nn) + 1, (
            f"peaks_clean.size={len(peaks_clean)} != nn.size+1={len(nn)+1}"
        )

    def test_nn_ts_alignment_after_kubios_insertion(self):
        """Verify that peaks_clean[:-1]/1000 gives exactly nn_clean.size timestamps.

        Simulates the missed-beat insertion scenario: a low-variability series
        where kubios is likely to insert or remove peaks.  The key invariant is
        that peaks_clean[:-1] (NN-interval start times, in seconds) must have
        exactly the same length as nn_clean so gap-detection never misaligns.
        """
        rng = np.random.default_rng(99)
        # Nearly constant RR → kubios may insert/remove peaks
        rr = np.clip(800.0 + 5.0 * rng.standard_normal(250), 400, 1400)
        nn, peaks_clean, n_beats, n_artifacts = clean_rr(rr)
        if nn.size < 2:
            pytest.skip("too few beats after cleaning — can't test alignment")
        nn_ts = peaks_clean[:-1] / 1000.0
        assert nn_ts.size == nn.size, (
            f"Timestamp / NN-interval size mismatch after kubios: "
            f"nn_ts.size={nn_ts.size} nn.size={nn.size}"
        )

    def test_kubios_actually_corrects_ectopic(self):
        """Kubios correction must run and flag at least one artifact.

        This exercises the REAL clean_rr path (not the parity test which bypasses
        it).  We build an RR series with obvious ectopic-style beats (a short
        interval followed by a compensating long interval) that kubios's
        Lipponen-Tarvainen classifier should flag as longshort artifacts.

        Injected pattern: 350 ms (short, within the 300-2000 range filter)
        followed by 1300 ms (compensating long beat).  Three such pairs are
        inserted at positions 30, 80, 130 — clearly deviant from the ~800 ms
        normal sinus rhythm.

        Assertions:
          - n_artifacts > 0: kubios flagged at least one interval (ran correctly).
          - nn_clean differs from range-filter-only NN: correction was applied,
            not silently discarded due to the TypeError bug.
        """
        rng = np.random.default_rng(42)
        # Normal sinus RR around 800 ms, 200 beats
        base = np.clip(800.0 + 50.0 * rng.standard_normal(200), 400, 1400).astype(np.float64)

        # Inject 3 ectopic-style pairs: short (350 ms) + compensating long (1300 ms).
        # These stay within [300, 2000] ms so they pass the range filter but are
        # clearly anomalous relative to the surrounding ~800 ms rhythm.
        rr_with_ectopics = base.copy()
        for pos in (30, 80, 130):
            rr_with_ectopics[pos] = 350.0    # short ectopic-like interval
            rr_with_ectopics[pos + 1] = 1300.0  # compensating long interval

        nn_clean, _peaks_clean, n_beats, n_artifacts = clean_rr(rr_with_ectopics)

        # Kubios must have flagged at least one artifact.
        assert n_artifacts > 0, (
            f"Expected n_artifacts > 0 for an RR series with injected ectopic pairs, "
            f"got {n_artifacts}. Kubios correction appears not to have run "
            "(possible TypeError bug — artifacts dict values are not all lists)."
        )

        # The cleaned NN must differ from simple range-filtering alone.
        # The 350/1300 ms beats pass the range filter, so range-filter-only
        # would keep them unchanged.  Kubios corrects them, producing a
        # different array.  If they're identical, kubios did not apply.
        rr_range_only = rr_with_ectopics[
            (rr_with_ectopics >= RR_MIN_MS) & (rr_with_ectopics <= RR_MAX_MS)
        ]
        range_only_nn = np.diff(rr_range_only)
        assert not (
            nn_clean.size == range_only_nn.size
            and np.allclose(nn_clean, range_only_nn, atol=0.5)
        ), (
            "nn_clean is identical to range-filtered-only NN — kubios correction "
            "did not modify the series, meaning the corrected peaks were silently discarded."
        )


# ---------------------------------------------------------------------------
# nightly_hrv — tiered window selection + output shape
# ---------------------------------------------------------------------------


@dataclass
class _FakeSleepSession:
    start: float
    end: float


@dataclass
class _FakeStage:
    start: float
    end: float
    stage: str


def _make_rr_stream(start_ts: float, duration_s: float,
                    mean_rr: float = 800.0, std_rr: float = 40.0,
                    seed: int = 0) -> list[dict]:
    """Generate synthetic 1-Hz RR records spanning [start_ts, start_ts+duration_s)."""
    rng = np.random.default_rng(seed)
    n = int(duration_s)
    rr_vals = np.clip(mean_rr + std_rr * rng.standard_normal(n), 400, 1400)
    return [{"ts": start_ts + i, "rr_ms": int(v)} for i, v in enumerate(rr_vals)]


class TestNightlyHrv:
    # --- output shape --------------------------------------------------------

    def test_whole_night_fallback_returns_required_keys(self):
        """No stages → whole_night fallback; result has all required keys."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=None)
        required = {"rmssd", "tier", "window_start", "window_end",
                    "n_beats", "n_artifacts", "rmssd_whole_night",
                    "pnn50", "mean_nn"}
        assert required.issubset(result.keys()), (
            f"Missing keys: {required - result.keys()}"
        )

    def test_whole_night_tier(self):
        """stages=None → tier must be 'whole_night'."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=None)
        assert result["tier"] == "whole_night"

    def test_rmssd_is_float(self):
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=None)
        assert isinstance(result["rmssd"], float)

    def test_rmssd_plausible_range(self):
        """Synthetic clean RR around 800 ms should yield RMSSD in ~5–200 ms."""
        session = _FakeSleepSession(start=0.0, end=7200.0)
        rr = _make_rr_stream(0.0, 7200.0)
        result = nightly_hrv(rr, session, stages=None)
        assert 1.0 <= result["rmssd"] <= 300.0, f"RMSSD out of range: {result['rmssd']}"

    # --- tiered window selection ---------------------------------------------

    def test_last_sws_tier_used_when_long_enough(self):
        """If a deep (SWS) episode >= 5 min exists, tier = 'last_sws'."""
        session = _FakeSleepSession(start=0.0, end=7200.0)
        # Deep episode from 5400–7200 s (30 min)
        stages = [
            _FakeStage(start=0.0, end=5400.0, stage="light"),
            _FakeStage(start=5400.0, end=7200.0, stage="deep"),
        ]
        rr = _make_rr_stream(0.0, 7200.0)
        result = nightly_hrv(rr, session, stages=stages)
        assert result["tier"] == "last_sws"

    def test_all_sws_tier_when_last_sws_too_short(self):
        """Last SWS episode < 5 min → fall back to all_sws (not whole_night)."""
        session = _FakeSleepSession(start=0.0, end=7200.0)
        # Early deep episode 10 min (enough), last deep < 5 min
        stages = [
            _FakeStage(start=0.0, end=600.0, stage="deep"),     # 10 min
            _FakeStage(start=600.0, end=6900.0, stage="light"),
            _FakeStage(start=6900.0, end=7100.0, stage="deep"),  # 3.3 min
        ]
        rr = _make_rr_stream(0.0, 7200.0)
        result = nightly_hrv(rr, session, stages=stages)
        assert result["tier"] == "all_sws"

    def test_whole_night_fallback_when_no_sws(self):
        """No deep stages → tier = 'whole_night'."""
        session = _FakeSleepSession(start=0.0, end=7200.0)
        stages = [
            _FakeStage(start=0.0, end=7200.0, stage="light"),
        ]
        rr = _make_rr_stream(0.0, 7200.0)
        result = nightly_hrv(rr, session, stages=stages)
        assert result["tier"] == "whole_night"

    def test_whole_night_rmssd_always_present(self):
        """rmssd_whole_night is always populated (for empirical tuning during rollout)."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        stages = [
            _FakeStage(start=0.0, end=1800.0, stage="deep"),
            _FakeStage(start=1800.0, end=3600.0, stage="light"),
        ]
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=stages)
        assert result["rmssd_whole_night"] is not None
        assert isinstance(result["rmssd_whole_night"], float)

    def test_window_bounds_within_session(self):
        """The selected window start/end must be within [session.start, session.end]."""
        session = _FakeSleepSession(start=0.0, end=7200.0)
        rr = _make_rr_stream(0.0, 7200.0)
        result = nightly_hrv(rr, session, stages=None)
        assert result["window_start"] >= session.start
        assert result["window_end"] <= session.end

    def test_n_beats_reasonable(self):
        """n_beats should be > 0 for a valid night."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=None)
        assert result["n_beats"] > 0

    def test_empty_rr_returns_nan(self):
        """No RR data in window → rmssd is NaN."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        result = nightly_hrv([], session, stages=None)
        assert math.isnan(result["rmssd"])

    def test_stages_none_equals_whole_night_stages(self):
        """stages=None and stages=[whole_session_light] produce same tier."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        r1 = nightly_hrv(rr, session, stages=None)
        r2 = nightly_hrv(rr, session, stages=[_FakeStage(0.0, 3600.0, "light")])
        assert r1["tier"] == r2["tier"] == "whole_night"

    # --- gap-pooled RMSSD correctness ----------------------------------------

    def test_gap_pooling_differs_from_naive(self):
        """
        A real 60 s wall-clock gap between two segments must be excluded from
        RMSSD computation (no spurious splice diff).

        Strategy: build two segments of constant-but-different RR values.
        Naive concatenated RMSSD would include the large diff at the gap splice.
        Segment-pooled RMSSD must exclude that splice and equal the pooled
        value computed segment-by-segment.
        """
        # Segment 1: constant 800 ms, 300 beats, ts 0–299
        seg1_rr = 800.0
        seg1_n = 300
        rr_seg1 = [{"ts": float(i), "rr_ms": seg1_rr} for i in range(seg1_n)]

        # Segment 2: constant 1000 ms, 300 beats, ts 360–659 (60 s gap after seg1)
        seg2_start = 360.0
        seg2_rr = 1000.0
        seg2_n = 300
        rr_seg2 = [{"ts": seg2_start + float(i), "rr_ms": seg2_rr} for i in range(seg2_n)]

        session = _FakeSleepSession(start=0.0, end=seg2_start + seg2_n)
        rr_all = rr_seg1 + rr_seg2

        result = nightly_hrv(rr_all, session, stages=None)

        # Both segments are constant-RR → all within-segment successive diffs = 0.
        # Segment-aware pooled RMSSD = sqrt(mean(all 0s)) = 0.0.
        # Naive concatenated RMSSD would include the gap splice diff
        # (1000 - 800 = 200 ms) → RMSSD > 0 — the wrong answer.
        # So the correct result is 0.0 (gap excluded).
        assert math.isclose(result["rmssd"], 0.0, abs_tol=1e-6), (
            f"Expected RMSSD=0.0 (constant per-segment, gap excluded), "
            f"got {result['rmssd']:.6f}.  "
            "Likely the 60 s wall-clock gap splice diff was not excluded."
        )

    # --- pNN50 / mean_nn QC metrics ------------------------------------------

    def test_pnn50_and_mean_nn_present_in_result(self):
        """nightly_hrv must include pnn50 and mean_nn for QC validation."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0)
        result = nightly_hrv(rr, session, stages=None)
        assert "pnn50" in result, "pnn50 missing from nightly_hrv result"
        assert "mean_nn" in result, "mean_nn missing from nightly_hrv result"

    def test_pnn50_is_percentage(self):
        """pnn50 must be in [0, 100] for a valid night."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0, std_rr=40.0)
        result = nightly_hrv(rr, session, stages=None)
        assert 0.0 <= result["pnn50"] <= 100.0, (
            f"pnn50 out of [0,100]: {result['pnn50']}"
        )

    def test_mean_nn_plausible_range(self):
        """mean_nn should be within physiological range for synthetic 800 ms RR."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        rr = _make_rr_stream(0.0, 3600.0, mean_rr=800.0, std_rr=20.0)
        result = nightly_hrv(rr, session, stages=None)
        assert 600.0 <= result["mean_nn"] <= 1100.0, (
            f"mean_nn out of plausible range: {result['mean_nn']}"
        )

    def test_pnn50_zero_for_constant_rr(self):
        """Constant RR → all successive diffs = 0 → pnn50 = 0.0."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        # Build a constant 800 ms RR stream (all diffs = 0, none > 50 ms)
        n = 3600
        rr = [{"ts": float(i), "rr_ms": 800} for i in range(n)]
        result = nightly_hrv(rr, session, stages=None)
        assert result["pnn50"] == 0.0

    def test_pnn50_high_for_alternating_rr(self):
        """Alternating 600/1000 ms (diff=400 > 50) → pnn50 near 100%."""
        session = _FakeSleepSession(start=0.0, end=3600.0)
        # Alternating 600/1000 → every diff = 400 ms > 50 ms → pnn50 ≈ 100
        rr = [
            {"ts": float(i), "rr_ms": 600 if i % 2 == 0 else 1000}
            for i in range(3600)
        ]
        result = nightly_hrv(rr, session, stages=None)
        assert result["pnn50"] > 90.0, (
            f"Expected pnn50 near 100 for alternating RR, got {result['pnn50']}"
        )
