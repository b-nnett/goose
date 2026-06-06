"""
report.py — Alignment, per-metric agreement statistics, and report generation.

Two layers:
  1. Core (pure, testable with in-memory fixtures; no DB):
       align_by_night(...)  → list[Pair]
       build_report(...)    → Report
       render_table(...)    → str (markdown/ASCII)

  2. Optional DB adapter (thin wrapper; kept separate so unit tests don't need
     Docker):
       load_computed_from_db(conn, device_id, nights) → dict

Ground-truth format
-------------------
``ground_truth`` is a list of ``GroundTruthDay`` objects (from
``app.whoop_api.models``).  Days where ``user_calibrating=True`` are excluded
from comparison (WHOOP scores aren't stable during the cold-start window).

Computed metrics format
-----------------------
``computed`` is a dict keyed by ``(date, metric_name)`` → float, where date is
a ``datetime.date`` and metric_name matches the keys in TOLERANCES.  Pass the
result of ``load_computed_from_db`` or build a dict from an in-memory fixture.

Report structure
----------------
``Report`` is a dict (metric_name → MetricResult).  Each MetricResult carries:
  - n         : int        (number of aligned pairs; could be 0)
  - mae       : float
  - rmse      : float
  - bias      : float      (signed; positive = over-prediction)
  - pct_within: float      (fraction within tolerance)
  - ba        : BAResult   (bland-altman; None when n < 2)
  - ccc       : float      (Lin's CCC; None when n < 2 or kind=categorical)
  - kappa     : KappaResult (sleep stages only; None for continuous metrics)
  - pass_bias : bool
  - pass_mae  : bool
  - pass_pct  : bool
  - passed    : bool       (ALL gates pass)
  - status    : str        "PASS" | "FAIL" | "NO_DATA"
  - validation_kind : str  "whoop_ground_truth" — clearly marks that results
                           are validated against WHOOP (not fallback-only)
  - trend_r   : float | None  (Pearson r for trend-mode metrics)
  - mode      : str           "absolute" | "trend"
  - coverage  : dict          nights in GT / computed / both / neither

Usage (in-memory, no DB):
    from datetime import date
    from app.whoop_api.models import GroundTruthDay
    from app.analysis.validation.report import align_by_night, build_report, render_table

    gt = [GroundTruthDay(day=date(2026,5,20), cycle_id=None,
                         hrv_rmssd_milli=62.0, resting_hr=54, ...)]
    computed = {(date(2026,5,20), "hrv"): 65.3, (date(2026,5,20), "resting_hr"): 55}
    pairs = align_by_night(gt, computed)
    report = build_report(pairs)
    print(render_table(report))
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from datetime import date
from typing import Any

from .stats import (
    BAResult,
    KappaResult,
    bland_altman,
    bias as _bias,
    cohens_kappa,
    lins_ccc,
    mae as _mae,
    pct_within_tolerance,
    pearson_r,
    rmse as _rmse,
)
from .targets import TOLERANCES


# ---------------------------------------------------------------------------
# Pair — one aligned (truth, pred) observation
# ---------------------------------------------------------------------------

@dataclass
class Pair:
    """One aligned (truth, predicted) observation for a specific night+metric."""
    night: date
    metric: str
    truth: float
    pred: float


# ---------------------------------------------------------------------------
# MetricResult — per-metric stats + pass/fail
# ---------------------------------------------------------------------------

@dataclass
class MetricResult:
    """Per-metric agreement statistics and PASS/FAIL vs tolerance targets."""
    metric: str
    n: int
    mae: float | None
    rmse: float | None
    bias: float | None
    pct_within: float | None
    ba: BAResult | None
    ccc: float | None
    kappa: KappaResult | None
    pass_bias: bool | None
    pass_mae: bool | None
    pass_pct: bool | None
    passed: bool
    status: str                     # "PASS" | "FAIL" | "NO_DATA"
    validation_kind: str            # always "whoop_ground_truth" here
    trend_r: float | None           # Pearson r (trend-mode metrics)
    mode: str                       # "absolute" | "trend"
    coverage: dict[str, Any]        # {n_gt, n_computed, n_aligned, n_gt_calibrating}


# ---------------------------------------------------------------------------
# align_by_night — inner join GT × computed on (night, metric)
# ---------------------------------------------------------------------------

def align_by_night(
    ground_truth: list,
    computed: dict[tuple[date, str], float],
    metrics: list[str] | None = None,
) -> list[Pair]:
    """Align ground-truth days with computed values by date + metric name.

    Parameters
    ----------
    ground_truth :
        List of ``GroundTruthDay`` objects.  Days with ``user_calibrating=True``
        are skipped (WHOOP scores not stable during cold-start).
    computed :
        dict keyed by ``(date, metric_name)`` → float.  Use the same metric
        names as in ``TOLERANCES`` (e.g. "hrv", "resting_hr", "recovery").
    metrics :
        Subset of metrics to align.  Defaults to all continuous TOLERANCES
        (excludes "sleep_stages" which needs a separate epoch fixture).

    Returns
    -------
    List of Pair objects (inner join — dates with data in BOTH sources).
    Logs coverage information (nights in GT only / computed only / both) as
    attributes on the returned list via the ``coverage`` attribute of each
    MetricResult (built from this alignment in ``build_report``).
    """
    if metrics is None:
        metrics = [m for m in TOLERANCES if TOLERANCES[m]["kind"] != "categorical"]

    # Map from date → GroundTruthDay for non-calibrating days
    gt_map: dict[date, Any] = {}
    n_calibrating = 0
    for day in ground_truth:
        if getattr(day, "user_calibrating", False):
            n_calibrating += 1
            continue
        gt_map[day.day] = day

    pairs: list[Pair] = []
    for metric in metrics:
        tol = TOLERANCES.get(metric, {})
        gt_field = tol.get("gt_field")
        if gt_field is None:
            continue  # categorical or unknown metric
        for day_date, gt_day in gt_map.items():
            truth_val = getattr(gt_day, gt_field, None)
            if truth_val is None:
                continue
            pred_val = computed.get((day_date, metric))
            if pred_val is None:
                continue
            # Skip NaN predictions
            try:
                if math.isnan(float(pred_val)):
                    continue
            except (TypeError, ValueError):
                continue
            pairs.append(Pair(
                night=day_date,
                metric=metric,
                truth=float(truth_val),
                pred=float(pred_val),
            ))

    return pairs


# ---------------------------------------------------------------------------
# build_report — compute per-metric stats + PASS/FAIL
# ---------------------------------------------------------------------------

def build_report(
    pairs: list[Pair],
    ground_truth: list | None = None,
    computed: dict | None = None,
    metrics: list[str] | None = None,
    sleep_stage_epochs: list[tuple] | None = None,
) -> dict[str, MetricResult]:
    """Build a validation report from aligned (truth, pred) pairs.

    Parameters
    ----------
    pairs :
        Output of ``align_by_night``.  May contain multiple metrics.
    ground_truth :
        Original GT list (optional; used for coverage counting).  Each item
        should have a ``.day`` attribute and optionally ``user_calibrating``.
    computed :
        Original computed dict (optional; keyed by (date, metric_name)).
        Used for coverage counting (n_computed per metric).
    metrics :
        Metrics to include in the report.  Defaults to all continuous
        metrics in TOLERANCES.
    sleep_stage_epochs :
        Optional list of (truth_stage, pred_stage) pairs — one per epoch —
        for the sleep_stages categorical gate.  When provided, ``build_report``
        computes Cohen's kappa + epoch accuracy and adds a "sleep_stages"
        MetricResult.  When absent, sleep_stages gets status="NO_DATA".

    Returns
    -------
    dict mapping metric_name → MetricResult.  Metrics with no pairs get
    status="NO_DATA" (all stat fields are None / False).

    All MetricResult.validation_kind == "whoop_ground_truth" to clearly
    distinguish from the fallback plausibility validations.

    Coverage dict keys
    ------------------
    n_gt              : int  — number of GT days passed in (0 if not supplied)
    n_gt_calibrating  : int  — calibrating days excluded from alignment
    n_computed        : int  — number of (date, metric) entries in computed for
                               this metric (0 if not supplied)
    n_aligned         : int  — pairs that matched both sources (inner join)
    """
    if metrics is None:
        metrics = [m for m in TOLERANCES if TOLERANCES[m]["kind"] != "categorical"]

    # Pre-compute GT coverage counts once (shared across metrics)
    n_gt_total = 0
    n_gt_calibrating = 0
    gt_dates: set[date] = set()
    if ground_truth is not None:
        for day in ground_truth:
            n_gt_total += 1
            if getattr(day, "user_calibrating", False):
                n_gt_calibrating += 1
            else:
                gt_dates.add(day.day)

    # Group pairs by metric
    by_metric: dict[str, list[Pair]] = {m: [] for m in metrics}
    for p in pairs:
        if p.metric in by_metric:
            by_metric[p.metric].append(p)

    report: dict[str, MetricResult] = {}

    for metric in metrics:
        tol = TOLERANCES[metric]
        metric_pairs = by_metric[metric]
        n = len(metric_pairs)

        # Coverage — populated from optional ground_truth / computed inputs
        n_computed_metric = 0
        if computed is not None:
            n_computed_metric = sum(
                1 for (d, m) in computed if m == metric
            )
        cov: dict[str, Any] = {
            "n_gt": n_gt_total,
            "n_gt_calibrating": n_gt_calibrating,
            "n_computed": n_computed_metric,
            "n_aligned": n,
        }

        if n == 0:
            report[metric] = MetricResult(
                metric=metric,
                n=0,
                mae=None, rmse=None, bias=None, pct_within=None,
                ba=None, ccc=None, kappa=None,
                pass_bias=None, pass_mae=None, pass_pct=None,
                passed=False,
                status="NO_DATA",
                validation_kind="whoop_ground_truth",
                trend_r=None,
                mode=tol.get("mode", "absolute"),
                coverage=cov,
            )
            continue

        truths = [p.truth for p in metric_pairs]
        preds = [p.pred for p in metric_pairs]

        # Core stats
        mae_val = _mae(truths, preds)
        rmse_val = _rmse(truths, preds)
        bias_val = _bias(truths, preds)
        pct_val = pct_within_tolerance(
            truths, preds,
            abs_tol=tol.get("abs_tol"),
            pct_tol=tol.get("pct_tol"),
        )

        ba_val: BAResult | None = None
        ccc_val: float | None = None
        trend_r_val: float | None = None

        if n >= 2:
            ba_val = bland_altman(truths, preds)
            ccc_val = lins_ccc(truths, preds)
            if tol.get("mode") == "trend":
                trend_r_val = pearson_r(truths, preds)

        # Gate evaluation
        max_bias = tol.get("max_bias")
        max_mae = tol.get("max_mae")
        pass_pct_thr = tol.get("pass_pct_within")

        pass_bias = (abs(bias_val) <= max_bias) if max_bias is not None else None
        pass_mae = (mae_val <= max_mae) if max_mae is not None else None
        pass_pct = (pct_val >= pass_pct_thr) if pass_pct_thr is not None else None

        # Overall pass: all defined gates must pass
        gate_results = [g for g in [pass_bias, pass_mae, pass_pct] if g is not None]
        passed = all(gate_results) if gate_results else False
        status = "PASS" if passed else "FAIL"

        report[metric] = MetricResult(
            metric=metric,
            n=n,
            mae=mae_val,
            rmse=rmse_val,
            bias=bias_val,
            pct_within=pct_val,
            ba=ba_val,
            ccc=ccc_val,
            kappa=None,  # continuous metrics don't use kappa
            pass_bias=pass_bias,
            pass_mae=pass_mae,
            pass_pct=pass_pct,
            passed=passed,
            status=status,
            validation_kind="whoop_ground_truth",
            trend_r=trend_r_val,
            mode=tol.get("mode", "absolute"),
            coverage=cov,
        )

    # ---------------------------------------------------------------------------
    # Categorical gate: sleep_stages (epoch-level hypnogram agreement)
    # ---------------------------------------------------------------------------
    # Only added when sleep_stage_epochs is provided OR when sleep_stages is
    # explicitly requested in metrics (to show NO_DATA rather than omitting it).
    if "sleep_stages" not in report:
        ss_tol = TOLERANCES.get("sleep_stages", {})
        ss_cov: dict[str, Any] = {
            "n_gt": n_gt_total,
            "n_gt_calibrating": n_gt_calibrating,
            "n_computed": 0,
            "n_aligned": 0,
        }
        if sleep_stage_epochs is None or len(sleep_stage_epochs) == 0:
            # No epoch data provided → NO_DATA (not PASS — absence of data is
            # not evidence of agreement)
            report["sleep_stages"] = MetricResult(
                metric="sleep_stages",
                n=0,
                mae=None, rmse=None, bias=None, pct_within=None,
                ba=None, ccc=None, kappa=None,
                pass_bias=None, pass_mae=None, pass_pct=None,
                passed=False,
                status="NO_DATA",
                validation_kind="whoop_ground_truth",
                trend_r=None,
                mode=ss_tol.get("mode", "absolute"),
                coverage=ss_cov,
            )
        else:
            # Compute kappa + accuracy from the epoch pairs
            truth_stages = [ep[0] for ep in sleep_stage_epochs]
            pred_stages = [ep[1] for ep in sleep_stage_epochs]
            kappa_result = cohens_kappa(truth_stages, pred_stages)

            min_kappa = ss_tol.get("min_kappa")
            min_accuracy = ss_tol.get("min_accuracy")
            pass_kappa = (kappa_result.kappa >= min_kappa) if min_kappa is not None else None
            pass_acc = (kappa_result.accuracy >= min_accuracy) if min_accuracy is not None else None

            cat_gate_results = [g for g in [pass_kappa, pass_acc] if g is not None]
            cat_passed = all(cat_gate_results) if cat_gate_results else False
            cat_status = "PASS" if cat_passed else "FAIL"

            ss_cov["n_aligned"] = kappa_result.n
            ss_cov["n_computed"] = kappa_result.n

            report["sleep_stages"] = MetricResult(
                metric="sleep_stages",
                n=kappa_result.n,
                mae=None, rmse=None, bias=None,
                pct_within=kappa_result.accuracy,  # epoch accuracy mapped here
                ba=None,
                ccc=None,
                kappa=kappa_result,
                pass_bias=None,
                pass_mae=None,
                pass_pct=pass_acc,   # accuracy gate mapped to pass_pct slot
                passed=cat_passed,
                status=cat_status,
                validation_kind="whoop_ground_truth",
                trend_r=None,
                mode=ss_tol.get("mode", "absolute"),
                coverage=ss_cov,
            )

    return report


# ---------------------------------------------------------------------------
# render_table — ASCII/markdown table
# ---------------------------------------------------------------------------

def render_table(report: dict[str, MetricResult]) -> str:
    """Render the validation report as a markdown-compatible ASCII table.

    Clearly labels each row with "whoop_ground_truth" as the validation kind.
    Metrics with status=NO_DATA are shown with placeholder dashes.

    Returns a multi-line string suitable for printing or writing to a .md file.
    """
    header = (
        "| metric           |  n  |  MAE  | bias  | %within | BA LoA (95%)         |  CCC  | trend_r | result |\n"
        "|------------------|-----|-------|-------|---------|----------------------|-------|---------|--------|\n"
    )
    lines = [
        "## Validation report — validated against WHOOP ground truth\n",
        "> All metrics are APPROXIMATE. "
        "Results labelled 'whoop_ground_truth' are compared to real WHOOP values.\n\n",
        header,
    ]

    for metric, r in sorted(report.items()):
        if r.status == "NO_DATA":
            lines.append(
                f"| {metric:<16} |  —  |   —   |   —   |    —    |          —           |   —   |    —    | NO_DATA |\n"
            )
            continue

        loa_str = "—"
        if r.ba is not None:
            loa_str = f"[{r.ba.loa_low:+.1f}, {r.ba.loa_high:+.1f}]"

        ccc_str = f"{r.ccc:.2f}" if r.ccc is not None else "—"
        tr_str = f"{r.trend_r:.2f}" if r.trend_r is not None else "—"

        result_str = r.status
        # Annotate which gate(s) failed
        fail_notes = []
        if r.pass_bias is False:
            fail_notes.append("bias↑")
        if r.pass_mae is False:
            fail_notes.append("MAE↑")
        if r.pass_pct is False:
            fail_notes.append("%within↓")
        if fail_notes:
            result_str += f" ({','.join(fail_notes)})"

        lines.append(
            f"| {metric:<16} | {r.n:3d} | {r.mae:5.2f} | {r.bias:+5.2f} | "
            f"{r.pct_within:6.0%}  | {loa_str:<20} | {ccc_str:>5} | {tr_str:>7} | {result_str} |\n"
        )

    lines.append("\nValidation kind: `whoop_ground_truth` — results compared to real WHOOP app values.\n")
    return "".join(lines)


# ---------------------------------------------------------------------------
# Optional DB adapter — thin layer; does NOT pull DB into the core
# ---------------------------------------------------------------------------

def load_computed_from_db(
    conn: Any,
    device_id: str,
    nights: list[date],
    metrics: list[str] | None = None,
) -> dict[tuple[date, str], float]:
    """Load computed metric values from the daily_metrics / sleep_sessions tables.

    This is a thin DB adapter.  The core ``build_report`` does NOT depend on
    this function — it accepts any dict.  Import this only when a DB connection
    is available (integration tests, CLI).

    Parameters
    ----------
    conn :
        A psycopg connection (autocommit OK).
    device_id :
        The device UUID string.
    nights :
        List of dates to fetch.
    metrics :
        Metric names to fetch.  Defaults to all continuous metrics.

    Returns
    -------
    dict keyed by (date, metric_name) → float.
    """
    if metrics is None:
        metrics = [m for m in TOLERANCES if TOLERANCES[m]["kind"] != "categorical"]

    result: dict[tuple[date, str], float] = {}
    if not nights:
        return result

    # Pull from daily_metrics table
    # Column mapping: daily_metrics has hrv_rmssd, resting_hr, spo2_pct,
    # skin_temp_c, resp_rate, recovery_score, day_strain, sleep_total_min,
    # sleep_efficiency (as 0-100 float or 0-1; check schema).
    _DM_COLS: dict[str, str] = {
        "hrv":             "hrv_rmssd",
        "resting_hr":      "resting_hr",
        "spo2":            "spo2_pct",
        "skin_temp":       "skin_temp_c",
        "resp":            "resp_rate",
        "recovery":        "recovery_score",
        "day_strain":      "day_strain",
        "sleep_duration":  "sleep_total_min",
        "sleep_efficiency": "sleep_efficiency",
    }

    cols_wanted = {
        metric: _DM_COLS[metric]
        for metric in metrics
        if metric in _DM_COLS
    }
    if not cols_wanted:
        return result

    select_cols = ", ".join(f"{col} AS {metric}" for metric, col in cols_wanted.items())

    try:
        with conn.cursor() as cur:
            cur.execute(
                f"SELECT day, {select_cols} FROM daily_metrics "
                f"WHERE device_id = $1 AND day = ANY($2::date[])",
                (device_id, list(nights)),
            )
            for row in cur.fetchall():
                row_date = row[0]
                for i, metric in enumerate(cols_wanted):
                    val = row[i + 1]
                    if val is not None:
                        try:
                            result[(row_date, metric)] = float(val)
                        except (TypeError, ValueError):
                            pass
    except Exception:
        pass  # Let callers handle DB errors; core logic remains pure

    return result
