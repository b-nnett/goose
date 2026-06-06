"""
tests/test_validation.py — Metrics validation harness test suite.

Test structure:
  1. TestStats         — hand-computed MAE/RMSE/bias/pct_within/BA/CCC/kappa
  2. TestReport        — build_report on synthetic GT: one PASS scenario, one FAIL
  3. TestPlausibility  — plausibility bounds: in-range (PASS) + out-of-range (FAIL)
  4. TestConsistency   — stage-sum, efficiency, recovery monotonicity, strain monotonicity
  5. TestHrvNeurokit   — HRV vs neurokit2 reference-implementation gate
  6. TestTargets       — tolerance spec sanity (all required keys present)

All tests use in-memory fixtures — no DB, no Docker required.
"""
from __future__ import annotations

import math
from datetime import date

import numpy as np
import pytest

# ---------------------------------------------------------------------------
# 1. Stats — hand-computed reference values
# ---------------------------------------------------------------------------

class TestMae:
    """MAE = mean(|pred - truth|)"""

    def test_identical_arrays(self):
        from app.analysis.validation.stats import mae
        assert mae([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]) == pytest.approx(0.0)

    def test_hand_computed(self):
        # |3-1| + |2-2| + |1-3| = 2 + 0 + 2 = 4 → 4/3 ≈ 1.333
        from app.analysis.validation.stats import mae
        assert mae([1.0, 2.0, 3.0], [3.0, 2.0, 1.0]) == pytest.approx(4.0 / 3.0)

    def test_single_pair(self):
        from app.analysis.validation.stats import mae
        assert mae([10.0], [7.0]) == pytest.approx(3.0)

    def test_mismatched_shapes_raises(self):
        from app.analysis.validation.stats import mae
        with pytest.raises(ValueError):
            mae([1.0, 2.0], [1.0])

    def test_empty_raises(self):
        from app.analysis.validation.stats import mae
        with pytest.raises(ValueError):
            mae([], [])


class TestRmse:
    """RMSE = sqrt(mean((pred - truth)^2))"""

    def test_identical(self):
        from app.analysis.validation.stats import rmse
        assert rmse([5.0, 10.0, 15.0], [5.0, 10.0, 15.0]) == pytest.approx(0.0)

    def test_hand_computed(self):
        # errors: [2, 0, -2] → sq: [4, 0, 4] → mean: 8/3 → sqrt: sqrt(8/3)
        from app.analysis.validation.stats import rmse
        expected = math.sqrt(8.0 / 3.0)
        assert rmse([1.0, 2.0, 3.0], [3.0, 2.0, 1.0]) == pytest.approx(expected)

    def test_rmse_ge_mae(self):
        # RMSE is always >= MAE
        from app.analysis.validation.stats import mae, rmse
        truth = [10.0, 20.0, 30.0, 100.0]
        pred = [12.0, 18.0, 33.0, 80.0]  # large outlier at index 3
        assert rmse(truth, pred) >= mae(truth, pred)

    def test_mismatched_shapes_raises(self):
        from app.analysis.validation.stats import rmse
        with pytest.raises(ValueError):
            rmse([1.0, 2.0], [1.0])


class TestBias:
    """bias = mean(pred - truth); positive = over-prediction"""

    def test_zero_bias(self):
        from app.analysis.validation.stats import bias
        assert bias([1.0, 3.0], [3.0, 1.0]) == pytest.approx(0.0)

    def test_positive_bias(self):
        # errors: [5-1, 6-2, 7-3] = [4, 4, 4] → bias = 4
        from app.analysis.validation.stats import bias
        assert bias([1.0, 2.0, 3.0], [5.0, 6.0, 7.0]) == pytest.approx(4.0)

    def test_negative_bias(self):
        from app.analysis.validation.stats import bias
        assert bias([5.0, 6.0, 7.0], [1.0, 2.0, 3.0]) == pytest.approx(-4.0)

    def test_asymmetric(self):
        # errors: [1-2, 3-2] = [-1, 1] → bias = 0; but different from zero-MAE
        from app.analysis.validation.stats import bias, mae
        t = [2.0, 2.0]
        p = [1.0, 3.0]
        assert bias(t, p) == pytest.approx(0.0)
        assert mae(t, p) == pytest.approx(1.0)


class TestPctWithin:
    """pct_within_tolerance with abs_tol, pct_tol, and both"""

    def test_all_within_abs(self):
        from app.analysis.validation.stats import pct_within_tolerance
        # errors: [1, 0, 1] — all within abs_tol=2
        assert pct_within_tolerance([10, 20, 30], [11, 20, 29], abs_tol=2.0, pct_tol=None) == pytest.approx(1.0)

    def test_none_within_abs(self):
        from app.analysis.validation.stats import pct_within_tolerance
        # errors: [5, 5, 5] — none within abs_tol=2
        assert pct_within_tolerance([10, 20, 30], [15, 25, 35], abs_tol=2.0, pct_tol=None) == pytest.approx(0.0)

    def test_partial_within_abs(self):
        from app.analysis.validation.stats import pct_within_tolerance
        # errors: [1, 5, 1] — 2/3 within abs_tol=2
        assert pct_within_tolerance([10, 20, 30], [11, 25, 31], abs_tol=2.0, pct_tol=None) == pytest.approx(2.0 / 3.0)

    def test_pct_tol_only(self):
        from app.analysis.validation.stats import pct_within_tolerance
        # truth=100, pred=108 → 8% error; within pct_tol=10%
        # truth=100, pred=115 → 15% error; outside pct_tol=10%
        result = pct_within_tolerance([100.0, 100.0], [108.0, 115.0], abs_tol=None, pct_tol=10.0)
        assert result == pytest.approx(0.5)

    def test_or_semantics_both_tols(self):
        from app.analysis.validation.stats import pct_within_tolerance
        # error=3: fails abs_tol=2 but passes pct_tol=10% (3/50=6%) → PASS
        result = pct_within_tolerance([50.0], [53.0], abs_tol=2.0, pct_tol=10.0)
        assert result == pytest.approx(1.0)

    def test_both_none_raises(self):
        from app.analysis.validation.stats import pct_within_tolerance
        with pytest.raises(ValueError):
            pct_within_tolerance([1.0], [1.0], abs_tol=None, pct_tol=None)


class TestBlandAltman:
    """Bland-Altman: mean_diff, sd_diff, LoA = mean ± 1.96 * sd"""

    def test_hand_computed(self):
        from app.analysis.validation.stats import bland_altman
        # diffs: [1-2, 3-2, 5-4] = [-1, 1, 1]
        truth = [2.0, 2.0, 4.0]
        pred  = [1.0, 3.0, 5.0]
        ba = bland_altman(truth, pred)
        # mean_diff = (-1+1+1)/3 = 1/3 ≈ 0.333
        assert ba.mean_diff == pytest.approx(1.0 / 3.0, abs=1e-9)
        # sd_diff (ddof=1): diffs = [-1, 1, 1], mean = 1/3
        #   deviations: [-4/3, 2/3, 2/3]; SS = 16/9+4/9+4/9 = 24/9 = 8/3
        #   var = (8/3)/2 = 4/3 → sd = sqrt(4/3) ≈ 1.1547
        expected_sd = math.sqrt(4.0 / 3.0)
        assert ba.sd_diff == pytest.approx(expected_sd, rel=1e-5)
        assert ba.loa_low == pytest.approx(1.0 / 3.0 - 1.96 * expected_sd, rel=1e-5)
        assert ba.loa_high == pytest.approx(1.0 / 3.0 + 1.96 * expected_sd, rel=1e-5)
        assert ba.n == 3

    def test_requires_at_least_2(self):
        from app.analysis.validation.stats import bland_altman
        with pytest.raises(ValueError):
            bland_altman([5.0], [5.0])

    def test_zero_error(self):
        from app.analysis.validation.stats import bland_altman
        ba = bland_altman([1.0, 2.0, 3.0], [1.0, 2.0, 3.0])
        assert ba.mean_diff == pytest.approx(0.0)
        assert ba.sd_diff == pytest.approx(0.0)
        assert ba.loa_low == pytest.approx(0.0)
        assert ba.loa_high == pytest.approx(0.0)


class TestLinsCCC:
    """Lin's CCC: measures agreement with the identity line"""

    def test_perfect_agreement(self):
        from app.analysis.validation.stats import lins_ccc
        # Identical → CCC = 1.0
        vals = [10.0, 20.0, 30.0, 40.0]
        assert lins_ccc(vals, vals) == pytest.approx(1.0, abs=1e-9)

    def test_hand_computed_simple(self):
        from app.analysis.validation.stats import lins_ccc
        # truth=[0,1,2], pred=[1,2,3]: perfect correlation (r=1), but mean shift of 1
        # mu_t=1, mu_p=2; sigma_t^2=1, sigma_p^2=1; cov=1; CCC=2*1/(1+1+1)=2/3
        truth = [0.0, 1.0, 2.0]
        pred  = [1.0, 2.0, 3.0]
        expected = 2.0 / 3.0
        assert lins_ccc(truth, pred) == pytest.approx(expected, rel=1e-5)

    def test_constant_series_returns_nan(self):
        from app.analysis.validation.stats import lins_ccc
        result = lins_ccc([5.0, 5.0, 5.0], [5.0, 5.0, 5.0])
        assert math.isnan(result)

    def test_perfect_disagreement(self):
        from app.analysis.validation.stats import lins_ccc
        # Mirrored around the mean → CCC should be negative
        truth = [1.0, 2.0, 3.0, 4.0, 5.0]
        pred  = [5.0, 4.0, 3.0, 2.0, 1.0]
        # r = -1; same variance/mean → CCC = 2*(-1)*var / (var+var+0) = -1
        result = lins_ccc(truth, pred)
        assert result == pytest.approx(-1.0, abs=1e-6)

    def test_requires_at_least_2(self):
        from app.analysis.validation.stats import lins_ccc
        with pytest.raises(ValueError):
            lins_ccc([5.0], [5.0])


class TestCohensKappa:
    """Cohen's kappa for categorical sleep-stage epoch agreement"""

    def test_perfect_agreement(self):
        from app.analysis.validation.stats import cohens_kappa
        labels = ["deep", "light", "rem", "wake", "light", "deep"]
        result = cohens_kappa(labels, labels)
        assert result.kappa == pytest.approx(1.0)
        assert result.accuracy == pytest.approx(1.0)
        assert result.n == 6

    def test_chance_level(self):
        from app.analysis.validation.stats import cohens_kappa
        # Completely opposite labeling should give kappa < 0 or near 0
        truth = ["wake", "wake", "sleep", "sleep"]
        pred  = ["sleep", "sleep", "wake", "wake"]
        result = cohens_kappa(truth, pred)
        assert result.kappa < 0.0
        assert result.accuracy == pytest.approx(0.0)

    def test_partial_agreement(self):
        from app.analysis.validation.stats import cohens_kappa
        # 3/4 exact matches → accuracy = 0.75; kappa depends on marginals
        truth = ["deep", "light", "rem", "wake"]
        pred  = ["deep", "light", "rem", "light"]  # 3 matches
        result = cohens_kappa(truth, pred)
        assert result.accuracy == pytest.approx(0.75)
        # kappa > 0 since accuracy > expected-chance
        assert result.kappa > 0.0
        assert result.n == 4

    def test_mismatched_lengths_raises(self):
        from app.analysis.validation.stats import cohens_kappa
        with pytest.raises(ValueError):
            cohens_kappa(["deep", "light"], ["deep"])

    def test_empty_raises(self):
        from app.analysis.validation.stats import cohens_kappa
        with pytest.raises(ValueError):
            cohens_kappa([], [])


# ---------------------------------------------------------------------------
# 2. Report — build_report with synthetic GT (one PASS, one FAIL scenario)
# ---------------------------------------------------------------------------

class TestBuildReport:
    """Test that build_report correctly detects PASS and FAIL for each gate."""

    def _make_gt_days(self, days_data: list[dict]) -> list:
        """Build GroundTruthDay objects from a list of field dicts."""
        from app.whoop_api.models import GroundTruthDay
        result = []
        for d in days_data:
            day_date = d.pop("day")
            result.append(GroundTruthDay(day=day_date, **d))
        return result

    def test_pass_scenario_hrv(self):
        """All HRV errors within tolerance → PASS."""
        from app.analysis.validation.report import align_by_night, build_report
        from app.analysis.validation.targets import TOLERANCES

        tol = TOLERANCES["hrv"]
        # Build 5 GT days + computed values within abs_tol=5 ms
        nights = [date(2026, 5, i) for i in range(20, 25)]
        gt_vals = [62.0, 58.0, 71.0, 55.0, 65.0]
        # pred within 3 ms of truth (well inside abs_tol=5 ms)
        pred_vals = [v + 3.0 for v in gt_vals]

        gt = self._make_gt_days([
            {"day": n, "cycle_id": None, "hrv_rmssd_milli": gv}
            for n, gv in zip(nights, gt_vals)
        ])
        computed = {(n, "hrv"): pv for n, pv in zip(nights, pred_vals)}

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])

        r = report["hrv"]
        assert r.n == 5
        assert r.status == "PASS"
        assert r.passed is True
        assert r.pct_within == pytest.approx(1.0)  # all within 5 ms
        assert r.mae == pytest.approx(3.0)
        assert r.bias == pytest.approx(3.0)
        assert r.validation_kind == "whoop_ground_truth"

    def test_fail_scenario_hrv_large_error(self):
        """All HRV errors exceed tolerance → FAIL."""
        from app.analysis.validation.report import align_by_night, build_report

        nights = [date(2026, 5, i) for i in range(20, 24)]
        gt_vals = [60.0, 60.0, 60.0, 60.0]
        # pred 15 ms off — exceeds abs_tol=5 AND pct_tol=10% (15/60=25%)
        pred_vals = [v + 15.0 for v in gt_vals]

        gt = self._make_gt_days([
            {"day": n, "cycle_id": None, "hrv_rmssd_milli": gv}
            for n, gv in zip(nights, gt_vals)
        ])
        computed = {(n, "hrv"): pv for n, pv in zip(nights, pred_vals)}

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])

        r = report["hrv"]
        assert r.status == "FAIL"
        assert r.passed is False

    def test_fail_scenario_high_bias(self):
        """Errors within abs_tol but bias exceeds max_bias → FAIL."""
        from app.analysis.validation.report import align_by_night, build_report

        nights = [date(2026, 5, i) for i in range(20, 30)]
        # 10 nights, systematic +6 bpm offset (exceeds max_bias=2)
        gt_vals = [54.0] * 10
        pred_vals = [60.0] * 10  # +6 bpm systematic over-prediction

        gt = self._make_gt_days([
            {"day": n, "cycle_id": None, "resting_hr": int(gv)}
            for n, gv in zip(nights, gt_vals)
        ])
        computed = {(n, "resting_hr"): pv for n, pv in zip(nights, pred_vals)}

        pairs = align_by_night(gt, computed, metrics=["resting_hr"])
        report = build_report(pairs, metrics=["resting_hr"])

        r = report["resting_hr"]
        assert r.status == "FAIL"
        assert r.pass_bias is False
        assert r.bias == pytest.approx(6.0)

    def test_calibrating_days_excluded(self):
        """Days with user_calibrating=True must be excluded from alignment."""
        from app.whoop_api.models import GroundTruthDay
        from app.analysis.validation.report import align_by_night, build_report

        nights = [date(2026, 5, i) for i in range(20, 23)]
        gt = [
            GroundTruthDay(day=nights[0], cycle_id=None, hrv_rmssd_milli=60.0,
                           user_calibrating=True),   # EXCLUDED
            GroundTruthDay(day=nights[1], cycle_id=None, hrv_rmssd_milli=60.0),
            GroundTruthDay(day=nights[2], cycle_id=None, hrv_rmssd_milli=60.0),
        ]
        computed = {(n, "hrv"): 62.0 for n in nights}

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])

        # Only nights[1] and nights[2] are included
        assert report["hrv"].n == 2

    def test_no_data_result(self):
        """Metric with no aligned pairs → status=NO_DATA."""
        from app.analysis.validation.report import build_report

        # No pairs at all
        report = build_report([], metrics=["hrv"])
        assert report["hrv"].status == "NO_DATA"
        assert report["hrv"].passed is False
        assert report["hrv"].mae is None

    def test_render_table_runs_without_error(self):
        """render_table should produce a non-empty string."""
        from app.analysis.validation.report import (
            align_by_night, build_report, render_table,
        )
        from app.whoop_api.models import GroundTruthDay

        nights = [date(2026, 5, 20)]
        gt = [GroundTruthDay(day=nights[0], cycle_id=None, hrv_rmssd_milli=62.0)]
        computed = {(nights[0], "hrv"): 64.0}
        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])
        table = render_table(report)
        assert "hrv" in table
        assert "whoop_ground_truth" in table

    def test_ba_and_ccc_require_2_pairs(self):
        """Bland-Altman and CCC should be None when n < 2."""
        from app.analysis.validation.report import align_by_night, build_report
        from app.whoop_api.models import GroundTruthDay

        nights = [date(2026, 5, 20)]
        gt = [GroundTruthDay(day=nights[0], cycle_id=None, hrv_rmssd_milli=62.0)]
        computed = {(nights[0], "hrv"): 64.0}
        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])

        assert report["hrv"].n == 1
        assert report["hrv"].ba is None
        assert report["hrv"].ccc is None


# ---------------------------------------------------------------------------
# 3. Plausibility bounds
# ---------------------------------------------------------------------------

class TestPlausibility:
    """Plausibility checks for each metric — in-range (PASS), out-of-range (FAIL)"""

    @pytest.mark.parametrize("metric,in_range,out_of_range", [
        ("resting_hr",  60.0,    25.0),   # 25 bpm < 30 min
        ("hrv",         50.0,   300.0),   # 300 ms > 250 max
        ("spo2",        97.0,    65.0),   # 65% < 70 min
        ("skin_temp",   33.5,    45.0),   # 45°C > 42 max
        ("resp",        15.0,     4.0),   # 4 brpm < 6 min
        ("recovery",    72.0,   105.0),   # 105 > 100 max
        ("day_strain",   8.0,    22.0),   # 22 > 21 max
        ("sleep_efficiency", 85.0, -5.0), # -5 < 0 min
    ])
    def test_in_range_passes(self, metric, in_range, out_of_range):
        from app.analysis.validation.plausibility import check_plausibility
        result = check_plausibility(metric, in_range)
        assert result.passed, f"Expected PASS for {metric}={in_range}: {result.detail}"
        assert result.validation_kind == "fallback_plausibility"

    @pytest.mark.parametrize("metric,in_range,out_of_range", [
        ("resting_hr",  60.0,    25.0),
        ("hrv",         50.0,   300.0),
        ("spo2",        97.0,    65.0),
        ("skin_temp",   33.5,    45.0),
        ("resp",        15.0,     4.0),
        ("recovery",    72.0,   105.0),
        ("day_strain",   8.0,    22.0),
        ("sleep_efficiency", 85.0, -5.0),
    ])
    def test_out_of_range_fails(self, metric, in_range, out_of_range):
        from app.analysis.validation.plausibility import check_plausibility
        result = check_plausibility(metric, out_of_range)
        assert not result.passed, f"Expected FAIL for {metric}={out_of_range}: {result.detail}"

    def test_nan_value_fails(self):
        from app.analysis.validation.plausibility import check_plausibility
        result = check_plausibility("hrv", float("nan"))
        assert not result.passed
        assert "nan" in result.detail.lower() or "not a finite" in result.detail.lower()

    def test_unknown_metric_fails(self):
        from app.analysis.validation.plausibility import check_plausibility
        result = check_plausibility("unknown_metric", 42.0)
        assert not result.passed
        assert "Unknown metric" in result.detail

    def test_batch_mixed(self):
        from app.analysis.validation.plausibility import check_plausibility_batch
        results = check_plausibility_batch({
            "hrv": 55.0,         # in range
            "resting_hr": 20.0,  # out of range
        })
        assert len(results) == 2
        hrv_result = next(r for r in results if "hrv" in r.name)
        rhr_result = next(r for r in results if "resting_hr" in r.name)
        assert hrv_result.passed
        assert not rhr_result.passed


# ---------------------------------------------------------------------------
# 4. Internal consistency
# ---------------------------------------------------------------------------

class TestConsistency:
    """Internal consistency checks for sleep stages, efficiency, recovery, strain."""

    def test_stage_sum_passes_exact(self):
        from app.analysis.validation.plausibility import check_stage_sum
        # deep=90, rem=90, light=180, awake=60 → sum=420 = TIB; sleep_sum=360=TST
        results = check_stage_sum(
            deep_min=90.0, rem_min=90.0, light_min=180.0, awake_min=60.0,
            tib_min=420.0, tst_min=360.0,
        )
        assert len(results) == 2
        for r in results:
            assert r.passed, f"Expected PASS: {r.detail}"

    def test_stage_sum_fails_on_mismatch(self):
        from app.analysis.validation.plausibility import check_stage_sum
        # deep=80+rem=80+light=180+awake=60 = 400 ≠ tib_min=420 → TIB check FAILS
        # deep=80+rem=80+light=180 = 340 = tst_min=340 → TST check PASSES
        results = check_stage_sum(
            deep_min=80.0, rem_min=80.0, light_min=180.0, awake_min=60.0,
            tib_min=420.0, tst_min=340.0,
        )
        tib_check = next(r for r in results if r.name == "stage_sum_equals_tib")
        tst_check = next(r for r in results if r.name == "stage_sum_equals_tst")
        assert not tib_check.passed   # 80+80+180+60=400 ≠ 420  → FAIL
        assert tst_check.passed       # 80+80+180=340 = tst_min=340 → PASS

    def test_efficiency_derivation_passes(self):
        from app.analysis.validation.plausibility import check_efficiency_derivation
        # TST=360 min, TIB=420 min → efficiency = 360/420 = 0.857
        result = check_efficiency_derivation(
            tst_min=360.0, tib_min=420.0, reported_efficiency=0.857,
        )
        assert result.passed

    def test_efficiency_derivation_passes_100_scale(self):
        from app.analysis.validation.plausibility import check_efficiency_derivation
        # reported as 85.7 (0-100 scale) — should normalize to 0.857
        result = check_efficiency_derivation(
            tst_min=360.0, tib_min=420.0, reported_efficiency=85.7,
        )
        assert result.passed

    def test_efficiency_derivation_fails_mismatch(self):
        from app.analysis.validation.plausibility import check_efficiency_derivation
        # TST/TIB = 0.857, reported = 0.7 → fails
        result = check_efficiency_derivation(
            tst_min=360.0, tib_min=420.0, reported_efficiency=0.70,
        )
        assert not result.passed

    def test_efficiency_zero_tib_fails(self):
        from app.analysis.validation.plausibility import check_efficiency_derivation
        result = check_efficiency_derivation(tst_min=300.0, tib_min=0.0, reported_efficiency=0.8)
        assert not result.passed

    def test_recovery_monotonic_in_hrv_passes(self):
        """Recovery increases as HRV increases (fixed baseline)."""
        from app.analysis.validation.plausibility import check_recovery_monotonic_in_hrv
        result = check_recovery_monotonic_in_hrv(
            hrv_sequence=[20.0, 40.0, 60.0, 80.0, 100.0],
            baseline_hrv=55.0,
            baseline_rhr=58.0,
            baseline_resp=14.0,
        )
        assert result.passed, result.detail
        assert result.validation_kind == "fallback_plausibility"

    def test_recovery_monotonic_below_baseline(self):
        """Recovery should also be monotonic below the baseline."""
        from app.analysis.validation.plausibility import check_recovery_monotonic_in_hrv
        result = check_recovery_monotonic_in_hrv(
            hrv_sequence=[10.0, 20.0, 30.0, 40.0, 50.0],
            baseline_hrv=55.0,
            baseline_rhr=58.0,
            baseline_resp=14.0,
        )
        assert result.passed, result.detail

    def test_strain_monotonic_in_trimp(self):
        """Strain is monotonically increasing in TRIMP (log map property)."""
        from app.analysis.validation.plausibility import check_strain_monotonic_in_trimp
        result = check_strain_monotonic_in_trimp()
        assert result.passed, result.detail
        assert result.validation_kind == "fallback_plausibility"

    def test_strain_at_zero_trimp(self):
        """strain(0) should be 0 (log(0+1)/ln(D) = 0)."""
        from app.analysis.strain import STRAIN_DENOMINATOR, MAX_STRAIN
        import math
        strain_0 = MAX_STRAIN * math.log(0 + 1) / math.log(STRAIN_DENOMINATOR)
        assert strain_0 == pytest.approx(0.0)


# ---------------------------------------------------------------------------
# 5. HRV neurokit2 reference gate
# ---------------------------------------------------------------------------

class TestHrvNeurokit:
    """Our rmssd_ms must agree with neurokit2 to within 0.01 ms on clean RR."""

    def test_parity_on_canonical_fixture(self):
        """HRV parity gate with the built-in fixture."""
        from app.analysis.validation.plausibility import (
            check_hrv_neurokit_parity,
            NEUROKIT_PARITY_TOL_MS,
        )
        result = check_hrv_neurokit_parity()
        assert result.passed, (
            f"HRV neurokit2 parity FAILED: {result.detail}\n"
            f"This means our rmssd_ms implementation disagrees with the "
            f"reference by more than {NEUROKIT_PARITY_TOL_MS} ms."
        )
        assert result.validation_kind == "fallback_plausibility"
        assert "this is the strongest current HRV evidence" in result.detail

    def test_parity_on_custom_rr(self):
        """Parity gate on a custom, controlled RR sequence."""
        import numpy as np
        from app.analysis.validation.plausibility import check_hrv_neurokit_parity

        # Simple sequence: 50 intervals at 800 ms with small jitter
        rng = np.random.default_rng(0)
        rr = (800.0 + rng.normal(0, 20, 50)).tolist()
        result = check_hrv_neurokit_parity(rr_ms=rr)
        assert result.passed, result.detail

    def test_parity_detail_contains_values(self):
        """The detail string should report both our value and nk2's value."""
        from app.analysis.validation.plausibility import check_hrv_neurokit_parity
        result = check_hrv_neurokit_parity()
        assert "our=" in result.detail
        assert "nk2=" in result.detail


# ---------------------------------------------------------------------------
# 6. Targets / tolerance spec sanity
# ---------------------------------------------------------------------------

class TestTargets:
    """Smoke-test the tolerance config: all required keys present."""

    REQUIRED_CONTINUOUS_KEYS = {
        "unit", "kind", "mode", "abs_tol", "pct_tol",
        "pass_pct_within", "max_bias", "max_mae", "gt_field",
    }
    REQUIRED_CATEGORICAL_KEYS = {
        "unit", "kind", "mode", "min_kappa", "min_accuracy",
    }

    def test_all_continuous_metrics_have_required_keys(self):
        from app.analysis.validation.targets import TOLERANCES
        for metric, spec in TOLERANCES.items():
            if spec["kind"] == "continuous":
                for key in self.REQUIRED_CONTINUOUS_KEYS:
                    assert key in spec, f"Metric {metric!r} missing key {key!r}"

    def test_all_categorical_metrics_have_required_keys(self):
        from app.analysis.validation.targets import TOLERANCES
        for metric, spec in TOLERANCES.items():
            if spec["kind"] == "categorical":
                for key in self.REQUIRED_CATEGORICAL_KEYS:
                    assert key in spec, f"Metric {metric!r} missing key {key!r}"

    def test_tolerance_for_raises_on_unknown(self):
        from app.analysis.validation.targets import tolerance_for
        with pytest.raises(KeyError):
            tolerance_for("completely_unknown_metric_xyz")

    def test_tolerance_for_returns_spec(self):
        from app.analysis.validation.targets import tolerance_for
        spec = tolerance_for("hrv")
        assert spec["unit"] == "ms"
        assert spec["abs_tol"] == 5.0

    def test_pass_pct_within_in_0_to_1(self):
        """All pass_pct_within thresholds should be fractions (0-1), not percent."""
        from app.analysis.validation.targets import TOLERANCES
        for metric, spec in TOLERANCES.items():
            ppw = spec.get("pass_pct_within")
            if ppw is not None:
                assert 0.0 <= ppw <= 1.0, (
                    f"Metric {metric!r}: pass_pct_within={ppw} is out of [0, 1]"
                )

    def test_all_required_metrics_present(self):
        """Core metrics from the plan §3 table should all have entries."""
        from app.analysis.validation.targets import TOLERANCES
        required = {
            "hrv", "resting_hr", "spo2", "skin_temp", "resp",
            "sleep_duration", "sleep_efficiency", "recovery",
            "day_strain", "sleep_stages",
        }
        missing = required - set(TOLERANCES.keys())
        assert not missing, f"Missing metrics in TOLERANCES: {missing}"


# ---------------------------------------------------------------------------
# 7. run_fallback_validations integration
# ---------------------------------------------------------------------------

class TestRunFallback:
    """Integration test: run_fallback_validations with sample data."""

    def test_runs_without_error(self):
        from app.analysis.validation.plausibility import run_fallback_validations
        results = run_fallback_validations()
        assert len(results) > 0
        for r in results:
            assert r.validation_kind == "fallback_plausibility"
            assert isinstance(r.passed, bool)

    def test_with_in_range_metrics_all_plausibility_pass(self):
        from app.analysis.validation.plausibility import run_fallback_validations
        sample = {
            "hrv": 55.0,
            "resting_hr": 58.0,
            "spo2": 97.0,
            "recovery": 72.0,
            "day_strain": 8.5,
        }
        results = run_fallback_validations(sample_metrics=sample)
        plausibility_results = [r for r in results if r.name.startswith("plausibility_")]
        for r in plausibility_results:
            assert r.passed, f"Expected PASS: {r.name}: {r.detail}"

    def test_with_out_of_range_metric_plausibility_fails(self):
        from app.analysis.validation.plausibility import run_fallback_validations
        sample = {
            "hrv": 999.0,       # way out of range
            "resting_hr": 58.0,
        }
        results = run_fallback_validations(sample_metrics=sample)
        hrv_check = next(r for r in results if r.name == "plausibility_hrv")
        assert not hrv_check.passed

    def test_with_consistent_sleep_stats(self):
        from app.analysis.validation.plausibility import run_fallback_validations
        sleep = {
            "deep_min": 90.0, "rem_min": 90.0, "light_min": 180.0,
            "awake_min": 60.0, "tib_min": 420.0, "tst_min": 360.0,
            "efficiency": 360.0 / 420.0,
        }
        results = run_fallback_validations(sleep_stats=sleep)
        consistency_results = [r for r in results if
                                r.name in ("stage_sum_equals_tib",
                                           "stage_sum_equals_tst",
                                           "efficiency_derivation")]
        for r in consistency_results:
            assert r.passed, f"Expected PASS: {r.name}: {r.detail}"

    def test_render_fallback_table_runs(self):
        from app.analysis.validation.plausibility import (
            run_fallback_validations, render_fallback_table,
        )
        results = run_fallback_validations()
        table = render_fallback_table(results)
        assert "fallback_plausibility" in table
        assert "NOT validated vs WHOOP" in table or "not validated vs WHOOP" in table
        assert len(table) > 100


# ---------------------------------------------------------------------------
# 8. Align by night — edge cases
# ---------------------------------------------------------------------------

class TestAlignByNight:
    """Edge cases for the alignment function."""

    def test_empty_gt_returns_empty(self):
        from app.analysis.validation.report import align_by_night
        pairs = align_by_night([], {}, metrics=["hrv"])
        assert pairs == []

    def test_empty_computed_returns_empty(self):
        from app.whoop_api.models import GroundTruthDay
        from app.analysis.validation.report import align_by_night
        gt = [GroundTruthDay(day=date(2026, 5, 20), cycle_id=None,
                             hrv_rmssd_milli=60.0)]
        pairs = align_by_night(gt, {}, metrics=["hrv"])
        assert pairs == []

    def test_nan_pred_excluded(self):
        from app.whoop_api.models import GroundTruthDay
        from app.analysis.validation.report import align_by_night
        gt = [GroundTruthDay(day=date(2026, 5, 20), cycle_id=None,
                             hrv_rmssd_milli=60.0)]
        computed = {(date(2026, 5, 20), "hrv"): float("nan")}
        pairs = align_by_night(gt, computed, metrics=["hrv"])
        assert pairs == []

    def test_none_gt_field_excluded(self):
        """GT days with None for a metric field should not produce pairs."""
        from app.whoop_api.models import GroundTruthDay
        from app.analysis.validation.report import align_by_night
        gt = [GroundTruthDay(day=date(2026, 5, 20), cycle_id=None,
                             hrv_rmssd_milli=None)]  # None GT field
        computed = {(date(2026, 5, 20), "hrv"): 60.0}
        pairs = align_by_night(gt, computed, metrics=["hrv"])
        assert pairs == []


# ---------------------------------------------------------------------------
# 9. Sleep-stages categorical gate (Major #1)
# ---------------------------------------------------------------------------

class TestSleepStagesCategoricalGate:
    """build_report with sleep_stage_epochs → kappa + accuracy gate."""

    def test_matching_hypnograms_pass(self):
        """Perfect epoch agreement (kappa=1.0, accuracy=1.0) → PASS."""
        from app.analysis.validation.report import build_report

        # 20 epochs, perfectly matching
        stages = ["wake", "light", "deep", "rem", "light"] * 4
        epochs = [(s, s) for s in stages]

        report = build_report([], sleep_stage_epochs=epochs)
        r = report["sleep_stages"]
        assert r.status == "PASS", f"Expected PASS but got {r.status}: kappa={r.kappa}"
        assert r.passed is True
        assert r.kappa is not None
        assert r.kappa.kappa == pytest.approx(1.0)
        assert r.kappa.accuracy == pytest.approx(1.0)
        assert r.n == 20
        assert r.validation_kind == "whoop_ground_truth"

    def test_poorly_matching_hypnograms_fail(self):
        """Low epoch agreement (accuracy well below 0.70) → FAIL."""
        from app.analysis.validation.report import build_report

        # Deliberately scrambled: truth cycles through 4 stages, pred always "wake"
        # This gives accuracy = 0.25 (only the "wake" epochs match)
        truth_stages = ["wake", "light", "deep", "rem"] * 5   # 20 epochs
        pred_stages = ["wake"] * 20                            # always wake

        epochs = list(zip(truth_stages, pred_stages))
        report = build_report([], sleep_stage_epochs=epochs)
        r = report["sleep_stages"]
        assert r.status == "FAIL", f"Expected FAIL but got {r.status}"
        assert r.passed is False
        assert r.kappa is not None
        # accuracy = 5/20 = 0.25, well below min_accuracy=0.70
        assert r.kappa.accuracy == pytest.approx(0.25)

    def test_absent_epoch_data_gives_no_data(self):
        """No sleep_stage_epochs provided → status=NO_DATA, not PASS."""
        from app.analysis.validation.report import build_report

        report = build_report([], sleep_stage_epochs=None)
        r = report["sleep_stages"]
        assert r.status == "NO_DATA"
        assert r.passed is False
        assert r.kappa is None
        assert r.n == 0

    def test_empty_epoch_list_gives_no_data(self):
        """Empty list for sleep_stage_epochs → status=NO_DATA."""
        from app.analysis.validation.report import build_report

        report = build_report([], sleep_stage_epochs=[])
        r = report["sleep_stages"]
        assert r.status == "NO_DATA"
        assert r.passed is False

    def test_kappa_below_threshold_fails(self):
        """Accuracy can pass threshold while kappa is still below min_kappa."""
        from app.analysis.validation.report import build_report
        from app.analysis.validation.targets import TOLERANCES

        # min_kappa=0.40.  Construct epochs where accuracy >= 0.70 but
        # kappa < 0.40 by having heavily imbalanced class distribution:
        # 14/20 truth = "light", pred always "light" → accuracy=0.70,
        # but kappa is depressed by the high base rate of "light".
        # p_e for "light" = (14/20) * (20/20) = 0.70
        # p_o = 0.70, so kappa = (0.70 - 0.70) / (1 - 0.70) = 0.0
        truth_stages = ["light"] * 14 + ["deep"] * 3 + ["rem"] * 2 + ["wake"] * 1
        pred_stages  = ["light"] * 20

        epochs = list(zip(truth_stages, pred_stages))
        report = build_report([], sleep_stage_epochs=epochs)
        r = report["sleep_stages"]
        assert r.kappa is not None
        # accuracy check: 14/20 = 0.70 → pass_pct (accuracy gate) passes
        assert r.kappa.accuracy == pytest.approx(0.70)
        # kappa should be ~ 0.0, well below min_kappa=0.40 → overall FAIL
        assert r.kappa.kappa < TOLERANCES["sleep_stages"]["min_kappa"]
        assert r.status == "FAIL"


# ---------------------------------------------------------------------------
# 10. Coverage counts (Major #2)
# ---------------------------------------------------------------------------

class TestCoverageCounts:
    """build_report populates coverage dict correctly from gt/computed inputs."""

    def _make_gt_day(self, day_date, hrv_val=60.0, calibrating=False):
        from app.whoop_api.models import GroundTruthDay
        return GroundTruthDay(
            day=day_date,
            cycle_id=None,
            hrv_rmssd_milli=hrv_val,
            user_calibrating=calibrating,
        )

    def test_coverage_populated_when_gt_and_computed_passed(self):
        """n_gt, n_gt_calibrating, n_computed, n_aligned all correctly filled."""
        from app.analysis.validation.report import align_by_night, build_report

        # 3 GT days: 2 normal, 1 calibrating
        nights = [date(2026, 5, i) for i in range(20, 23)]
        gt = [
            self._make_gt_day(nights[0], 62.0, calibrating=True),  # calibrating
            self._make_gt_day(nights[1], 60.0),
            self._make_gt_day(nights[2], 58.0),
        ]
        # computed has entries for hrv on 2 of the 3 nights (nights[1] and [2])
        computed = {
            (nights[1], "hrv"): 61.0,
            (nights[2], "hrv"): 59.0,
            (nights[1], "resting_hr"): 55.0,  # different metric
        }

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, ground_truth=gt, computed=computed, metrics=["hrv"])

        cov = report["hrv"].coverage
        assert cov["n_gt"] == 3               # 3 total GT days
        assert cov["n_gt_calibrating"] == 1   # 1 calibrating day
        assert cov["n_computed"] == 2          # 2 (date, "hrv") entries in computed
        assert cov["n_aligned"] == 2           # 2 aligned pairs

    def test_coverage_n_gt_calibrating_positive_when_calibrating(self):
        """n_gt_calibrating is nonzero when calibrating days are present."""
        from app.analysis.validation.report import align_by_night, build_report

        nights = [date(2026, 5, i) for i in range(20, 25)]
        gt = [self._make_gt_day(n, calibrating=(i < 2)) for i, n in enumerate(nights)]
        # 2 calibrating days, 3 normal
        computed = {(n, "hrv"): 61.0 for n in nights}

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, ground_truth=gt, computed=computed, metrics=["hrv"])

        assert report["hrv"].coverage["n_gt_calibrating"] == 2

    def test_coverage_defaults_when_gt_computed_not_passed(self):
        """Without gt/computed args, n_gt=0 and n_computed=0 (not error)."""
        from app.analysis.validation.report import build_report, Pair

        pairs = [Pair(night=date(2026, 5, 20), metric="hrv", truth=60.0, pred=62.0)]
        report = build_report(pairs, metrics=["hrv"])

        cov = report["hrv"].coverage
        assert cov["n_gt"] == 0
        assert cov["n_computed"] == 0
        assert cov["n_aligned"] == 1

    def test_no_data_coverage_has_all_keys(self):
        """NO_DATA MetricResult still has all four coverage keys."""
        from app.analysis.validation.report import build_report

        report = build_report([], metrics=["hrv"])
        cov = report["hrv"].coverage
        for key in ("n_gt", "n_gt_calibrating", "n_computed", "n_aligned"):
            assert key in cov, f"Missing coverage key: {key!r}"


# ---------------------------------------------------------------------------
# 11. Isolated bias-gate regression test (Major #3)
# ---------------------------------------------------------------------------

class TestIsolatedBiasGate:
    """Bias gate fails while MAE and pct_within both pass."""

    def test_bias_alone_fails_resting_hr(self):
        """resting_hr: small consistent offset that exceeds max_bias=2 bpm but
        stays within abs_tol=2 bpm on each individual night → MAE and
        pct_within pass while bias fails.

        Construction:
          - 10 nights, each with truth=54, pred=56.5  (error = +2.5 bpm).
          - |error| = 2.5 > abs_tol=2 → each night fails the within-tolerance
            check → pct_within=0.0 (FAIL).

        Wait — that breaks pct_within too.  We need errors that are small
        in magnitude (each within abs_tol) but consistently signed.

        resting_hr abs_tol=2, max_bias=2, pass_pct_within=0.80, max_mae=3.

        To isolate only the bias gate we need:
          (a) |error_i| <= abs_tol=2  for every night  → pct_within=1.0  (PASS)
          (b) MAE <= max_mae=3                           → PASS  (guaranteed by a)
          (c) |mean(error)| > max_bias=2                 → bias FAIL

        With abs_tol=2, the largest consistently-signed error that fits
        inside abs_tol is exactly 2 bpm.  But max_bias=2 means
        |bias| <= 2 passes — we need |bias| > 2.

        Solution: use a mix of +2 and +2.5 bpm errors.
          Some nights error = +2   (within abs_tol=2 ✓)
          Some nights error = +2.5 (outside abs_tol=2 → those nights fail
          pct_within).

        Actually impossible for resting_hr to isolate bias-alone since
        max_bias == abs_tol (2).  Any bias > 2 requires at least some nights
        to have |error| > 2, which fails pct_within too.

        Instead we use HRV where max_bias=5 but abs_tol=5 (same issue) OR
        sleep_duration where abs_tol=10, max_bias=10 (same).

        The geometry: bias_alone_fail requires max_bias < abs_tol, meaning
        a systematic offset can sneak under the per-night tolerance but still
        exceed the mean-bias gate.  In our TOLERANCES every metric has
        max_bias <= abs_tol, so bias-alone-fail is unreachable for perfectly
        uniform errors.

        However with heterogeneous errors we can do it:
          6/10 nights: error = +4 ms  (within abs_tol=5 ✓)
          4/10 nights: error = +7 ms  (outside abs_tol=5 ✗, but within pct_tol=10%
                                       if truth ≈ 70 ms → 7/70 = 10% ✓)

        Then:
          pct_within: all 10 within at least one tolerance → 1.0 ✓
          MAE = (6*4 + 4*7)/10 = (24+28)/10 = 5.2 ≤ max_mae=8 ✓
          bias = (6*4 + 4*7)/10 = 5.2 > max_bias=5 ✗  → FAIL

        This is the bias-alone-fail scenario for HRV.
        """
        from app.analysis.validation.report import align_by_night, build_report
        from app.whoop_api.models import GroundTruthDay

        # HRV: abs_tol=5 ms OR pct_tol=10%, max_bias=5, max_mae=8, pass_pct=0.70
        # truth = 70 ms so 7 ms error = 10% → passes pct_tol
        nights = [date(2026, 5, i) for i in range(1, 11)]  # 10 nights
        gt_hrv = 70.0  # fixed truth for all nights

        # 6 nights at +4 ms (within abs_tol=5 ✓), 4 nights at +7 ms (>abs_tol but ≤10% ✓)
        errors = [4.0] * 6 + [7.0] * 4
        pred_vals = [gt_hrv + e for e in errors]

        gt = [
            GroundTruthDay(day=n, cycle_id=None, hrv_rmssd_milli=gt_hrv)
            for n in nights
        ]
        computed = {(n, "hrv"): pv for n, pv in zip(nights, pred_vals)}

        pairs = align_by_night(gt, computed, metrics=["hrv"])
        report = build_report(pairs, metrics=["hrv"])

        r = report["hrv"]
        # Verify our construction is correct
        expected_mae = (6 * 4.0 + 4 * 7.0) / 10  # = 5.2
        expected_bias = expected_mae               # all positive
        assert r.mae == pytest.approx(expected_mae, rel=1e-6)
        assert r.bias == pytest.approx(expected_bias, rel=1e-6)
        assert r.pct_within == pytest.approx(1.0)  # all within at least one tol

        # Gate checks: pct PASS, MAE PASS, bias FAIL
        assert r.pass_pct is True,  f"pct_within gate should PASS (got {r.pct_within:.2%})"
        assert r.pass_mae is True,  f"MAE gate should PASS (got {r.mae:.2f} <= 8)"
        assert r.pass_bias is False, f"bias gate should FAIL (|{r.bias:.2f}| > 5)"
        assert r.status == "FAIL"
        assert r.passed is False


# ---------------------------------------------------------------------------
# 12. Borderline pct_within gate (Minor #6)
# ---------------------------------------------------------------------------

class TestBorderlinePctWithin:
    """Borderline pct_within gate: exactly at threshold and one below."""

    def test_exactly_at_threshold_passes(self):
        """7/10 within tolerance, threshold=0.70 → PASS (>= not >)."""
        from app.analysis.validation.report import build_report, Pair

        # HRV: abs_tol=5 ms, pass_pct_within=0.70
        # 7 pairs within 3 ms (PASS), 3 pairs outside at 10 ms (FAIL)
        pairs = []
        for i in range(7):
            pairs.append(Pair(night=date(2026, 5, 1 + i), metric="hrv",
                              truth=60.0, pred=63.0))   # error=3 ms ≤ 5
        for i in range(3):
            pairs.append(Pair(night=date(2026, 5, 10 + i), metric="hrv",
                              truth=60.0, pred=70.0))   # error=10 ms > 5, >10%

        report = build_report(pairs, metrics=["hrv"])
        r = report["hrv"]
        assert r.pct_within == pytest.approx(0.7)
        assert r.pass_pct is True   # 0.70 >= 0.70 → PASS

    def test_one_below_threshold_fails(self):
        """6/10 within tolerance, threshold=0.70 → FAIL."""
        from app.analysis.validation.report import build_report, Pair

        # 6 within, 4 outside → 0.60 < 0.70
        pairs = []
        for i in range(6):
            pairs.append(Pair(night=date(2026, 5, 1 + i), metric="hrv",
                              truth=60.0, pred=63.0))   # error=3 ms ≤ 5
        for i in range(4):
            pairs.append(Pair(night=date(2026, 5, 10 + i), metric="hrv",
                              truth=60.0, pred=70.0))   # error=10 ms > 5, >10%

        report = build_report(pairs, metrics=["hrv"])
        r = report["hrv"]
        assert r.pct_within == pytest.approx(0.6)
        assert r.pass_pct is False  # 0.60 < 0.70 → FAIL
