"""
targets.py — Per-metric accuracy targets and tolerance configuration.

Encodes the accuracy targets from docs/plans/2026-05-26-metrics-accuracy-overhaul.md
§3 table as a structured config dict.  These are the thresholds the validation
harness uses to gate PASS/FAIL.

Tolerance fields
----------------
abs_tol : float | None
    Absolute tolerance in the metric's native units.  A paired night is
    "within tolerance" if |pred - truth| <= abs_tol  OR  pct_tol check passes.
pct_tol : float | None
    Percentage tolerance (0-100).  A paired night passes if
    |pred - truth| / |truth| * 100 <= pct_tol.  If both abs_tol and pct_tol
    are set, a night passes when EITHER condition is met (OR semantics).
pass_pct_within : float
    Fraction of paired nights that must be within tolerance for the metric to
    PASS (headline gate).  E.g. 0.70 means ≥70% of nights must pass.
max_bias : float | None
    Maximum allowed |mean(pred - truth)| (signed systematic offset).
max_mae : float | None
    Maximum allowed mean absolute error.
unit : str
    Display unit for error reporting.
kind : str
    "continuous" → MAE/RMSE/bias/BA/CCC; "categorical" → kappa + accuracy.
mode : str
    "absolute"   → score on raw values.
    "trend"      → score on night-to-night deltas AND correlation/CCC of the
                   series (used when absolute accuracy is known-poor: skin temp,
                   raw resp).
min_kappa : float | None
    For kind="categorical": minimum Cohen's kappa to PASS.
min_accuracy : float | None
    For kind="categorical": minimum raw epoch agreement fraction to PASS.
"""
from __future__ import annotations

from typing import Any

# ---------------------------------------------------------------------------
# Tolerance spec — straight from the plan §3 accuracy targets table
# ---------------------------------------------------------------------------
# Metrics are scored in the units listed.  The "truth" source for each is
# the corresponding field in GroundTruthDay.
#
# Mapping from metric name → GroundTruthDay field:
#   "hrv"              → gt.hrv_rmssd_milli  (ms)
#   "resting_hr"       → gt.resting_hr       (bpm, int)
#   "spo2"             → gt.spo2_percentage  (%)
#   "skin_temp"        → gt.skin_temp_celsius (°C)
#   "resp"             → gt.respiratory_rate  (brpm)
#   "sleep_duration"   → gt.total_sleep_min   (minutes)
#   "sleep_efficiency" → gt.sleep_efficiency_pct (%)  0-100 scale
#   "recovery"         → gt.recovery_score   (0-100)
#   "day_strain"       → gt.day_strain       (0-21)
#   "sleep_stages"     → epoch-level labels; separate fixture (kind=categorical)
#
# "stress" is qualitative in the plan; omitted here (no numeric GT field).

TOLERANCES: dict[str, dict[str, Any]] = {
    # ─── HRV (RMSSD, ms) ────────────────────────────────────────────────────
    # Plan: ±5 ms absolute OR ±10% relative
    "hrv": {
        "unit": "ms",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 5.0,
        "pct_tol": 10.0,
        "pass_pct_within": 0.70,
        "max_bias": 5.0,
        "max_mae": 8.0,
        "gt_field": "hrv_rmssd_milli",
    },

    # ─── Resting HR (bpm) ────────────────────────────────────────────────────
    # Plan: ±2 bpm
    "resting_hr": {
        "unit": "bpm",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 2.0,
        "pct_tol": None,
        "pass_pct_within": 0.80,
        "max_bias": 2.0,
        "max_mae": 3.0,
        "gt_field": "resting_hr",
    },

    # ─── SpO2 (%) ────────────────────────────────────────────────────────────
    # Plan: ±2%
    "spo2": {
        "unit": "%",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 2.0,
        "pct_tol": None,
        "pass_pct_within": 0.60,
        "max_bias": 2.0,
        "max_mae": 3.0,
        "gt_field": "spo2_percentage",
    },

    # ─── Skin temperature (°C) ───────────────────────────────────────────────
    # Plan: ±0.3 °C.  Absolute accuracy is known-poor (un-calibrated sensor);
    # mode=trend scores the series correlation / CCC as primary evidence.
    "skin_temp": {
        "unit": "°C",
        "kind": "continuous",
        "mode": "trend",
        "abs_tol": 0.3,
        "pct_tol": None,
        "pass_pct_within": 0.60,
        "max_bias": 0.4,
        "max_mae": 0.5,
        "gt_field": "skin_temp_celsius",
    },

    # ─── Respiratory rate (brpm) ─────────────────────────────────────────────
    # Plan: ±1 br/min.  mode=trend (raw resp is un-calibrated ADC).
    "resp": {
        "unit": "brpm",
        "kind": "continuous",
        "mode": "trend",
        "abs_tol": 1.0,
        "pct_tol": None,
        "pass_pct_within": 0.60,
        "max_bias": 1.0,
        "max_mae": 2.0,
        "gt_field": "respiratory_rate",
    },

    # ─── Sleep duration (minutes) ─────────────────────────────────────────────
    # Plan: ±10 min
    "sleep_duration": {
        "unit": "min",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 10.0,
        "pct_tol": None,
        "pass_pct_within": 0.70,
        "max_bias": 10.0,
        "max_mae": 15.0,
        "gt_field": "total_sleep_min",
    },

    # ─── Sleep efficiency (%) ─────────────────────────────────────────────────
    # Plan: ±5%.  GT uses sleep_efficiency_pct which is 0-100 scale.
    "sleep_efficiency": {
        "unit": "%",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 5.0,
        "pct_tol": None,
        "pass_pct_within": 0.70,
        "max_bias": 4.0,
        "max_mae": 5.0,
        "gt_field": "sleep_efficiency_pct",
    },

    # ─── Recovery score (0-100) ───────────────────────────────────────────────
    # Plan: ±7%.  Our recovery is a logistic approximation, so tolerance is loose.
    "recovery": {
        "unit": "pts",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 7.0,
        "pct_tol": None,
        "pass_pct_within": 0.60,
        "max_bias": 8.0,
        "max_mae": 12.0,
        "gt_field": "recovery_score",
    },

    # ─── Day strain (0-21 WHOOP scale) ────────────────────────────────────────
    # Plan: ±1.5 strain units.  Our denominator D is unfitted → loose target.
    "day_strain": {
        "unit": "strain",
        "kind": "continuous",
        "mode": "absolute",
        "abs_tol": 1.5,
        "pct_tol": None,
        "pass_pct_within": 0.60,
        "max_bias": 1.5,
        "max_mae": 2.0,
        "gt_field": "day_strain",
    },

    # ─── Sleep-stage epoch agreement ─────────────────────────────────────────
    # Plan: ≥70% epoch agreement.  Separate epoch-level fixture required.
    "sleep_stages": {
        "unit": "epoch",
        "kind": "categorical",
        "mode": "absolute",
        "abs_tol": None,
        "pct_tol": None,
        "pass_pct_within": None,
        "max_bias": None,
        "max_mae": None,
        "gt_field": None,  # separate sleep_stages.csv fixture
        "min_kappa": 0.40,        # moderate agreement threshold (§4 reference)
        "min_accuracy": 0.70,     # ≥70% epoch agreement
    },
}


def tolerance_for(metric: str) -> dict[str, Any]:
    """Return the tolerance spec for a metric, raising KeyError if unknown."""
    if metric not in TOLERANCES:
        raise KeyError(f"No tolerance spec for metric {metric!r}; "
                       f"known metrics: {sorted(TOLERANCES)}")
    return TOLERANCES[metric]
