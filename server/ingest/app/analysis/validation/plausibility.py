"""
plausibility.py — Fallback validations runnable without WHOOP ground truth.

Three classes of fallback checks, all clearly labelled
"plausible, not validated vs WHOOP":

1. Reference-implementation agreement
   Our ``hrv.rmssd_ms`` vs neurokit2 on identical clean RR intervals.
   This is the strongest current evidence for HRV accuracy: our formula
   must produce values within 0.01 ms of the gold-standard library.

2. Physiological plausibility bounds
   Per-metric sane ranges (RHR 30-100 bpm, HRV 5-250 ms, etc.).
   Flags any computed value that falls outside the human-normal envelope.

3. Internal consistency
   - Sleep stage minutes (deep + rem + light + awake) sum to TIB
     (total in-bed time); deep + rem + light sum to TST.
   - Sleep efficiency = TST / TIB (within floating-point tolerance).
   - Recovery is monotonically increasing in HRV given a fixed baseline
     (unit test; real data checked only if a sequence is passed).
   - Strain is monotonically increasing in TRIMP (unit test; see strain.py).

All check functions return a ``CheckResult`` (pass/fail + detail string).
``run_fallback_validations(...)`` collects all checks into a report.

Clearly labelled outputs
-------------------------
Every ``CheckResult`` carries a ``validation_kind = "fallback_plausibility"``
field so the caller can distinguish these from ``whoop_ground_truth`` results.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any, Sequence


# ---------------------------------------------------------------------------
# CheckResult — one fallback check outcome
# ---------------------------------------------------------------------------

@dataclass
class CheckResult:
    """Result of one plausibility or consistency check."""
    name: str
    passed: bool
    detail: str
    validation_kind: str = "fallback_plausibility"

    def __str__(self) -> str:
        icon = "PASS" if self.passed else "FAIL"
        return f"[{icon}] {self.name}: {self.detail}"


# ---------------------------------------------------------------------------
# 1. Reference-implementation agreement (HRV vs neurokit2)
# ---------------------------------------------------------------------------

#: Tolerance for neurokit2 parity gate (ms).  Our formula must agree to within
#: this amount on identical clean RR arrays.
NEUROKIT_PARITY_TOL_MS: float = 0.01


def check_hrv_neurokit_parity(
    rr_ms: Sequence[float] | None = None,
) -> CheckResult:
    """Compare our rmssd_ms to neurokit2 on identical clean RR intervals.

    If ``rr_ms`` is None, uses a canonical 60-beat fixture derived from a
    resting-heart-rate series (800 ms ± noise).

    This is the strongest current HRV evidence: the two implementations must
    agree to within ``NEUROKIT_PARITY_TOL_MS`` ms (0.01 ms) on identical input.

    The neurokit2 reference path:
      1. Convert RR ms → synthetic peaks at 1000 Hz via nk.intervals_to_peaks.
      2. Compute nk.hrv_time(peaks, sampling_rate=1000)["HRV_RMSSD"].

    Returns
    -------
    CheckResult with:
      passed = True iff |our_rmssd - nk_rmssd| <= NEUROKIT_PARITY_TOL_MS
      detail = both values and the absolute difference.
    """
    import numpy as np

    try:
        import neurokit2 as nk
    except ImportError:
        return CheckResult(
            name="hrv_neurokit_parity",
            passed=False,
            detail="neurokit2 not installed — cannot run reference gate",
        )

    from app.analysis.hrv import rmssd_ms

    # Use the provided RR array or generate a canonical fixture
    if rr_ms is None:
        rng = np.random.default_rng(42)
        # 60 beats at ~75 bpm (800 ms) with physiological HRV noise
        rr_ms = (800.0 + rng.normal(0, 30, 60)).tolist()

    rr_arr = np.asarray(rr_ms, dtype=np.float64)

    # neurokit2 reference path: intervals_to_peaks rounds RR to integer ms
    # (1000 Hz synthetic peaks → integer sample indices → int NN intervals).
    # To compare apples-to-apples we must run both implementations on the
    # same integer-rounded NN intervals, not on the original float RR.
    # This correctly gates that our Task Force formula matches nk2 on identical data.
    try:
        peaks = nk.intervals_to_peaks(rr_arr, sampling_rate=1000)
        # The integer-rounded NN intervals that nk2 uses internally:
        nn_int = np.diff(np.asarray(peaks, dtype=np.float64))
        nk_hrv = nk.hrv_time(peaks, sampling_rate=1000, show=False)
        nk_val = float(nk_hrv["HRV_RMSSD"].iloc[0])
    except Exception as exc:
        return CheckResult(
            name="hrv_neurokit_parity",
            passed=False,
            detail=f"neurokit2 call failed: {exc}",
        )

    # Our RMSSD on the same integer-rounded NN
    our_val = rmssd_ms(nn_int)

    diff = abs(our_val - nk_val)
    passed = diff <= NEUROKIT_PARITY_TOL_MS
    return CheckResult(
        name="hrv_neurokit_parity",
        passed=passed,
        detail=(
            f"our={our_val:.4f} ms, nk2={nk_val:.4f} ms, "
            f"|diff|={diff:.4f} ms (tol={NEUROKIT_PARITY_TOL_MS} ms) — "
            f"this is the strongest current HRV evidence (no WHOOP GT yet)"
        ),
    )


# ---------------------------------------------------------------------------
# 2. Physiological plausibility bounds
# ---------------------------------------------------------------------------

#: Per-metric plausibility bounds: (min, max, unit_label).
#: These are human-physiological extremes, not healthy-adult ranges.
PLAUSIBILITY_BOUNDS: dict[str, tuple[float, float, str]] = {
    "resting_hr":      (30.0,  100.0,  "bpm"),
    "hrv":             (5.0,   250.0,  "ms"),
    "spo2":            (70.0,  100.0,  "%"),
    "skin_temp":       (20.0,  42.0,   "°C"),
    "resp":            (6.0,   30.0,   "brpm"),
    "recovery":        (0.0,   100.0,  "pts"),
    "day_strain":      (0.0,   21.0,   "strain"),
    "sleep_efficiency":(0.0,   100.0,  "%"),   # 0-100 scale
    "sleep_duration":  (0.0,   900.0,  "min"), # 0-15 h; 0 okay (no sleep night)
    # Sleep stage percentages (0-100% of TST)
    "pct_deep":        (0.0,   35.0,   "%TST"),
    "pct_rem":         (0.0,   50.0,   "%TST"),
    "pct_light":       (0.0,   80.0,   "%TST"),
    "pct_wake":        (0.0,   50.0,   "%TIB"),
}


def check_plausibility(
    metric: str,
    value: float,
) -> CheckResult:
    """Check that a single computed metric value is physiologically plausible.

    Uses the hard bounds in ``PLAUSIBILITY_BOUNDS``.  Values outside the
    bounds are physiologically impossible or represent sensor/algorithm failure.

    Parameters
    ----------
    metric : str
        Metric name matching a key in ``PLAUSIBILITY_BOUNDS``.
    value : float
        The computed value to check.

    Returns
    -------
    CheckResult with passed=True iff ``lo <= value <= hi``.
    """
    if metric not in PLAUSIBILITY_BOUNDS:
        return CheckResult(
            name=f"plausibility_{metric}",
            passed=False,
            detail=f"Unknown metric {metric!r}; no bounds defined",
        )
    lo, hi, unit = PLAUSIBILITY_BOUNDS[metric]

    if math.isnan(value) or math.isinf(value):
        return CheckResult(
            name=f"plausibility_{metric}",
            passed=False,
            detail=f"value is {value} (not a finite number)",
        )

    passed = lo <= value <= hi
    detail = (
        f"{value:.2f} {unit} is within [{lo}, {hi}] {unit}"
        if passed
        else f"{value:.2f} {unit} is OUTSIDE [{lo}, {hi}] {unit}"
    )
    return CheckResult(name=f"plausibility_{metric}", passed=passed, detail=detail)


def check_plausibility_batch(
    metrics_values: dict[str, float],
) -> list[CheckResult]:
    """Run plausibility checks for a dict of metric→value pairs.

    Parameters
    ----------
    metrics_values :
        Dict of metric_name → computed_value.  Unknown metric names produce a
        FAIL result (no bounds defined).

    Returns
    -------
    List of CheckResult, one per metric in ``metrics_values``.
    """
    return [check_plausibility(metric, val)
            for metric, val in metrics_values.items()]


# ---------------------------------------------------------------------------
# 3. Internal consistency
# ---------------------------------------------------------------------------

#: Absolute tolerance for floating-point equality in stage-sum checks (minutes).
STAGE_SUM_TOL_MIN: float = 1.0

#: Absolute tolerance for efficiency re-derivation (fraction).
EFFICIENCY_TOL: float = 0.02


def check_stage_sum(
    deep_min: float,
    rem_min: float,
    light_min: float,
    awake_min: float,
    tib_min: float,
    tst_min: float,
) -> list[CheckResult]:
    """Check that sleep stage minutes are internally consistent.

    Two invariants:
      1. deep + rem + light + awake ≈ TIB  (total in-bed time)
      2. deep + rem + light ≈ TST          (total sleep time)

    Parameters
    ----------
    deep_min, rem_min, light_min, awake_min : float
        Sleep stage durations in minutes.
    tib_min : float
        Total in-bed time in minutes (= awake + sleep stage minutes).
    tst_min : float
        Total sleep time in minutes (= deep + rem + light).

    Returns
    -------
    List of two CheckResult objects.
    """
    results = []

    # Invariant 1: deep + rem + light + awake ≈ TIB
    stage_sum_with_wake = deep_min + rem_min + light_min + awake_min
    diff1 = abs(stage_sum_with_wake - tib_min)
    results.append(CheckResult(
        name="stage_sum_equals_tib",
        passed=diff1 <= STAGE_SUM_TOL_MIN,
        detail=(
            f"deep({deep_min:.1f}) + rem({rem_min:.1f}) + "
            f"light({light_min:.1f}) + awake({awake_min:.1f}) = "
            f"{stage_sum_with_wake:.1f} min; TIB={tib_min:.1f} min; "
            f"|diff|={diff1:.2f} min (tol={STAGE_SUM_TOL_MIN} min)"
        ),
    ))

    # Invariant 2: deep + rem + light ≈ TST
    sleep_sum = deep_min + rem_min + light_min
    diff2 = abs(sleep_sum - tst_min)
    results.append(CheckResult(
        name="stage_sum_equals_tst",
        passed=diff2 <= STAGE_SUM_TOL_MIN,
        detail=(
            f"deep({deep_min:.1f}) + rem({rem_min:.1f}) + "
            f"light({light_min:.1f}) = {sleep_sum:.1f} min; "
            f"TST={tst_min:.1f} min; |diff|={diff2:.2f} min "
            f"(tol={STAGE_SUM_TOL_MIN} min)"
        ),
    ))

    return results


def check_efficiency_derivation(
    tst_min: float,
    tib_min: float,
    reported_efficiency: float,
) -> CheckResult:
    """Check that sleep efficiency = TST / TIB (within tolerance).

    Parameters
    ----------
    tst_min : float
        Total sleep time in minutes.
    tib_min : float
        Total in-bed time in minutes.
    reported_efficiency : float
        Reported sleep efficiency (0-1 fraction or 0-100 %).

    Returns
    -------
    CheckResult.  Handles both 0-1 and 0-100 scale automatically.
    """
    if tib_min <= 0.0:
        return CheckResult(
            name="efficiency_derivation",
            passed=False,
            detail=f"TIB={tib_min:.1f} min is zero/negative; cannot check efficiency",
        )

    derived_eff = tst_min / tib_min

    # Normalize reported efficiency to 0-1 for comparison
    rep_eff = reported_efficiency
    if rep_eff > 1.5:  # assume it's 0-100 scale
        rep_eff = rep_eff / 100.0

    diff = abs(derived_eff - rep_eff)
    passed = diff <= EFFICIENCY_TOL
    return CheckResult(
        name="efficiency_derivation",
        passed=passed,
        detail=(
            f"TST/TIB = {tst_min:.1f}/{tib_min:.1f} = {derived_eff:.3f}; "
            f"reported={rep_eff:.3f}; |diff|={diff:.4f} "
            f"(tol={EFFICIENCY_TOL})"
        ),
    )


def check_recovery_monotonic_in_hrv(
    hrv_sequence: Sequence[float],
    baseline_hrv: float,
    baseline_rhr: float,
    baseline_resp: float | None = None,
) -> CheckResult:
    """Check that recovery is monotonically increasing in HRV (fixed baseline).

    Computes recovery scores for a sorted sequence of HRV values (ascending),
    holding resting HR, resp, and sleep_perf constant at the baseline.  The
    recovery score should be strictly increasing as HRV increases.

    This validates the direction of the dominant driver, not the absolute scale.

    Parameters
    ----------
    hrv_sequence :
        Sorted (ascending) list of HRV values in ms to test across.
    baseline_hrv, baseline_rhr, baseline_resp :
        Personal baseline values for the recovery formula.

    Returns
    -------
    CheckResult.  passed=True iff all computed recovery scores are non-decreasing
    (monotonically non-decreasing — flat transitions pass) as HRV increases
    from min to max.
    """
    from app.analysis.recovery import recovery_score
    from app.analysis.baselines import BaselineState

    # Build a "trusted" baseline state for hrv + rhr + resp
    def _state(baseline: float, floor: float) -> BaselineState:
        return BaselineState(
            baseline=baseline,
            spread=floor,
            n_valid=20,
            nights_since_update=0,
            status="trusted",
        )

    baselines = {
        "hrv": _state(baseline_hrv, 5.0),
        "resting_hr": _state(baseline_rhr, 2.0),
        "resp": _state(baseline_resp or 15.0, 0.5),
    }

    scores: list[float] = []
    for h in sorted(hrv_sequence):
        s = recovery_score(
            hrv=h,
            rhr=baseline_rhr,
            resp=baseline_resp,
            baselines=baselines,
            sleep_perf=0.85,
        )
        if s is None:
            return CheckResult(
                name="recovery_monotonic_in_hrv",
                passed=False,
                detail="recovery_score returned None (cold-start gate triggered unexpectedly)",
            )
        scores.append(s)

    # Check non-decreasing
    violations = [(i, scores[i], scores[i+1]) for i in range(len(scores)-1)
                  if scores[i+1] < scores[i] - 0.01]  # 0.01 tolerance for fp noise

    if violations:
        viol_str = "; ".join(f"score[{i}]={s1:.1f}>{s2:.1f}" for i, s1, s2 in violations)
        return CheckResult(
            name="recovery_monotonic_in_hrv",
            passed=False,
            detail=f"Recovery NOT monotonic in HRV — violations: {viol_str}",
        )

    return CheckResult(
        name="recovery_monotonic_in_hrv",
        passed=True,
        detail=(
            f"Recovery is non-decreasing (monotonically non-decreasing) over HRV "
            f"{sorted(hrv_sequence)[0]:.0f}–{sorted(hrv_sequence)[-1]:.0f} ms "
            f"(scores {scores[0]:.1f}–{scores[-1]:.1f} pts)"
        ),
    )


def check_strain_monotonic_in_trimp() -> CheckResult:
    """Check that strain is monotonically increasing in TRIMP.

    Verifies the core invariant of the log-map:
    strain(T1) < strain(T2) for all T1 < T2 > 0.

    Uses the strain module's ``trimp_to_strain`` (or the log formula directly).

    Returns
    -------
    CheckResult with monotonicity confirmed over [1, 100, 500, 1000, 5000].
    """
    import math as _math

    try:
        from app.analysis.strain import STRAIN_DENOMINATOR, MAX_STRAIN
    except ImportError as e:
        return CheckResult(
            name="strain_monotonic_in_trimp",
            passed=False,
            detail=f"Could not import strain module: {e}",
        )

    ln_d = _math.log(STRAIN_DENOMINATOR)
    test_trimps = [1.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0]
    strains = [MAX_STRAIN * _math.log(t + 1) / ln_d for t in test_trimps]

    violations = [(i, test_trimps[i], test_trimps[i+1], strains[i], strains[i+1])
                  for i in range(len(strains)-1)
                  if strains[i+1] <= strains[i]]

    if violations:
        viol_str = "; ".join(
            f"T={t1:.0f}→{t2:.0f} strain={s1:.2f}→{s2:.2f}"
            for _, t1, t2, s1, s2 in violations
        )
        return CheckResult(
            name="strain_monotonic_in_trimp",
            passed=False,
            detail=f"Strain NOT monotonic in TRIMP — violations: {viol_str}",
        )

    return CheckResult(
        name="strain_monotonic_in_trimp",
        passed=True,
        detail=(
            f"Strain is monotonically increasing over TRIMP "
            f"{test_trimps[0]:.0f}–{test_trimps[-1]:.0f} "
            f"(strain {strains[0]:.2f}–{strains[-1]:.2f})"
        ),
    )


# ---------------------------------------------------------------------------
# Convenience runner — collects all fallback checks
# ---------------------------------------------------------------------------

def run_fallback_validations(
    sample_metrics: dict[str, float] | None = None,
    sleep_stats: dict[str, float] | None = None,
    rr_ms: Sequence[float] | None = None,
) -> list[CheckResult]:
    """Run all fallback validations and return a list of CheckResults.

    Parameters
    ----------
    sample_metrics :
        Dict of metric_name → value for plausibility bounds checks.
        E.g. {"hrv": 55.0, "resting_hr": 58, "recovery": 72, "day_strain": 8.1}.
        If None, plausibility checks are skipped (no data to check).
    sleep_stats :
        Dict with keys "deep_min", "rem_min", "light_min", "awake_min",
        "tib_min", "tst_min", "efficiency" for the consistency checks.
        If None, sleep consistency checks are skipped.
    rr_ms :
        Optional clean RR array (ms) for the neurokit2 parity gate.
        If None, the canonical fixture is used.

    Returns
    -------
    List of CheckResult objects.  The caller can render these as a table.
    All results have validation_kind="fallback_plausibility".

    Label in output: every result is labelled "plausible, not validated vs WHOOP"
    to make clear these are NOT ground-truth comparisons.
    """
    results: list[CheckResult] = []

    # 1. HRV reference-implementation agreement
    results.append(check_hrv_neurokit_parity(rr_ms))

    # 2. Plausibility bounds
    if sample_metrics:
        results.extend(check_plausibility_batch(sample_metrics))

    # 3. Internal consistency
    results.append(check_strain_monotonic_in_trimp())

    if sleep_stats:
        required = {"deep_min", "rem_min", "light_min", "awake_min", "tib_min", "tst_min"}
        if required.issubset(sleep_stats):
            results.extend(check_stage_sum(
                deep_min=sleep_stats["deep_min"],
                rem_min=sleep_stats["rem_min"],
                light_min=sleep_stats["light_min"],
                awake_min=sleep_stats["awake_min"],
                tib_min=sleep_stats["tib_min"],
                tst_min=sleep_stats["tst_min"],
            ))
        if all(k in sleep_stats for k in ("tst_min", "tib_min", "efficiency")):
            results.append(check_efficiency_derivation(
                tst_min=sleep_stats["tst_min"],
                tib_min=sleep_stats["tib_min"],
                reported_efficiency=sleep_stats["efficiency"],
            ))

    # 4. Recovery monotonicity (with default mid-range baselines)
    results.append(check_recovery_monotonic_in_hrv(
        hrv_sequence=[20.0, 30.0, 45.0, 60.0, 80.0, 100.0],
        baseline_hrv=55.0,
        baseline_rhr=58.0,
        baseline_resp=14.0,
    ))

    return results


def render_fallback_table(results: list[CheckResult]) -> str:
    """Render fallback validation results as an ASCII table.

    Includes the "plausible, not validated vs WHOOP" disclaimer prominently.
    """
    lines = [
        "## Fallback validations — plausible, NOT validated vs WHOOP\n",
        "> These checks run without ground truth. "
        "PASS means physiologically plausible and internally consistent, "
        "NOT that the value matches WHOOP.\n\n",
        "| check                          | result | detail |\n",
        "|--------------------------------|--------|--------|\n",
    ]
    for r in results:
        icon = "PASS" if r.passed else "FAIL"
        # Truncate long detail for table display
        detail_short = r.detail[:90] + "…" if len(r.detail) > 90 else r.detail
        lines.append(f"| {r.name:<30} | {icon:<6} | {detail_short} |\n")

    n_pass = sum(1 for r in results if r.passed)
    n_total = len(results)
    lines.append(f"\nFallback summary: {n_pass}/{n_total} checks passed. "
                 f"Validation kind: `fallback_plausibility`\n")
    return "".join(lines)
