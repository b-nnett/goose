"""
CLI entry point for the metrics validation harness.

Usage:

  With ground truth (WHOOP export CSV or JSON):
    python -m app.analysis.validation report \\
        --ground-truth path/to/ground_truth.csv \\
        [--metrics path/to/computed_metrics.json] \\
        [--format markdown|text]

  Without ground truth (fallback plausibility only):
    python -m app.analysis.validation fallback \\
        [--sample-metrics '{"hrv":55,"resting_hr":58,"recovery":72}']

  The two modes produce distinct output sections:
  - "report" mode: ground-truth comparison table (validation_kind=whoop_ground_truth)
  - "fallback" mode: plausibility + consistency table (validation_kind=fallback_plausibility)

  Running "report" with no matching ground-truth data still runs the fallback
  checks at the end of the output.

Ground-truth CSV format (long form, one row per night+metric):
    night,metric,value,source
    2026-05-20,hrv,55,synthetic
    2026-05-20,resting_hr,60,synthetic
    ...

OR: path to a WHOOP CSV export (from app/whoop_api/export_parser.py).

Computed metrics JSON format:
    [{"date": "2026-05-20", "hrv": 65.3, "resting_hr": 55, ...}, ...]
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import date
from pathlib import Path


def _load_gt_from_csv(path: Path) -> list:
    """Load ground truth from either a WHOOP export CSV or the long-form fixture CSV."""
    import csv

    # Try long-form fixture first: columns night,metric,value,source
    with open(path, newline="") as fh:
        reader = csv.DictReader(fh)
        rows = list(reader)

    if not rows:
        print(f"[warn] Ground truth file {path} is empty.", file=sys.stderr)
        return []

    # Detect long-form vs WHOOP export by column names
    fieldnames = set(rows[0].keys())
    if "night" in fieldnames and "metric" in fieldnames and "value" in fieldnames:
        return _parse_long_form_csv(rows)

    # Otherwise assume WHOOP export CSV format → delegate to export_parser
    from app.whoop_api.export_parser import parse_export_csv
    return parse_export_csv(str(path))


def _parse_long_form_csv(rows: list[dict]) -> list:
    """Convert long-form CSV rows to GroundTruthDay-like dicts grouped by night."""
    from app.whoop_api.models import GroundTruthDay

    # Group by night
    by_night: dict[date, dict] = {}
    for row in rows:
        try:
            night = date.fromisoformat(row["night"])
        except (KeyError, ValueError):
            continue
        if night not in by_night:
            by_night[night] = {}
        metric = row.get("metric", "").strip()
        try:
            val = float(row.get("value", ""))
        except (TypeError, ValueError):
            continue
        by_night[night][metric] = val

    # Build GroundTruthDay objects
    result = []
    _field_map = {
        "hrv":             "hrv_rmssd_milli",
        "resting_hr":      "resting_hr",
        "spo2":            "spo2_percentage",
        "skin_temp":       "skin_temp_celsius",
        "resp":            "respiratory_rate",
        "recovery":        "recovery_score",
        "day_strain":      "day_strain",
        "sleep_duration":  None,  # handled via in_bed_milli + awake_milli
        "sleep_efficiency":"sleep_efficiency_pct",
    }
    for night, metrics in sorted(by_night.items()):
        kwargs: dict = {"day": night, "cycle_id": None}
        for metric_name, field_name in _field_map.items():
            if field_name and metric_name in metrics:
                kwargs[field_name] = metrics[metric_name]
        # Sleep duration → set in_bed_milli and awake_milli so total_sleep_min works
        if "sleep_duration" in metrics and "sleep_efficiency" in metrics:
            tst_min = metrics["sleep_duration"]
            eff = metrics["sleep_efficiency"] / 100.0
            if eff > 0:
                tib_min = tst_min / eff
                kwargs["in_bed_milli"] = int(tib_min * 60_000)
                kwargs["awake_milli"] = int((tib_min - tst_min) * 60_000)
        result.append(GroundTruthDay(**kwargs))
    return result


def _load_computed_from_json(path: Path) -> dict[tuple[date, str], float]:
    """Load computed metrics from a JSON array."""
    data = json.loads(path.read_text())
    result: dict[tuple[date, str], float] = {}
    for row in data:
        try:
            d = date.fromisoformat(str(row["date"]))
        except (KeyError, ValueError):
            continue
        for key, val in row.items():
            if key == "date":
                continue
            try:
                result[(d, key)] = float(val)
            except (TypeError, ValueError):
                pass
    return result


def _cmd_report(args: argparse.Namespace) -> None:
    from app.analysis.validation.report import (
        align_by_night, build_report, render_table,
    )
    from app.analysis.validation.plausibility import (
        run_fallback_validations, render_fallback_table,
    )

    gt_path = Path(args.ground_truth)
    if not gt_path.exists():
        print(f"[error] Ground truth file not found: {gt_path}", file=sys.stderr)
        sys.exit(1)

    ground_truth = _load_gt_from_csv(gt_path)
    print(f"Loaded {len(ground_truth)} ground-truth days from {gt_path}", file=sys.stderr)

    computed: dict[tuple[date, str], float] = {}
    if args.metrics:
        metrics_path = Path(args.metrics)
        if not metrics_path.exists():
            print(f"[error] Metrics file not found: {metrics_path}", file=sys.stderr)
            sys.exit(1)
        computed = _load_computed_from_json(metrics_path)
        print(f"Loaded {len(computed)} computed metric values from {metrics_path}",
              file=sys.stderr)

    pairs = align_by_night(ground_truth, computed)
    report = build_report(pairs, ground_truth=ground_truth, computed=computed)

    print(render_table(report))

    # Always also run fallback validations
    print("\n---\n")
    fallback_results = run_fallback_validations()
    print(render_fallback_table(fallback_results))


def _cmd_fallback(args: argparse.Namespace) -> None:
    from app.analysis.validation.plausibility import (
        run_fallback_validations, render_fallback_table,
    )

    sample_metrics: dict | None = None
    if args.sample_metrics:
        try:
            sample_metrics = json.loads(args.sample_metrics)
        except json.JSONDecodeError as e:
            print(f"[error] --sample-metrics is not valid JSON: {e}", file=sys.stderr)
            sys.exit(1)

    results = run_fallback_validations(sample_metrics=sample_metrics)
    print(render_fallback_table(results))


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="python -m app.analysis.validation",
        description="Metrics accuracy validation harness",
    )
    subparsers = parser.add_subparsers(dest="command")

    # 'report' subcommand — ground-truth comparison
    report_parser = subparsers.add_parser(
        "report", help="Compare our metrics vs WHOOP ground truth"
    )
    report_parser.add_argument(
        "--ground-truth", required=True,
        help="Path to ground-truth CSV (long-form or WHOOP export)",
    )
    report_parser.add_argument(
        "--metrics", default=None,
        help="Path to computed metrics JSON (list of {date, metric: value})",
    )
    report_parser.add_argument(
        "--format", choices=["markdown", "text"], default="markdown",
        help="Output format (default: markdown)",
    )

    # 'fallback' subcommand — plausibility only
    fallback_parser = subparsers.add_parser(
        "fallback", help="Run fallback plausibility/consistency checks (no GT needed)"
    )
    fallback_parser.add_argument(
        "--sample-metrics", default=None,
        help='JSON object of metric→value pairs, e.g. \'{"hrv":55,"resting_hr":58}\'',
    )

    args = parser.parse_args(argv)

    if args.command == "report":
        _cmd_report(args)
    elif args.command == "fallback":
        _cmd_fallback(args)
    else:
        # No subcommand → run fallback by default
        from app.analysis.validation.plausibility import (
            run_fallback_validations, render_fallback_table,
        )
        results = run_fallback_validations()
        print(render_fallback_table(results))


if __name__ == "__main__":
    main()
