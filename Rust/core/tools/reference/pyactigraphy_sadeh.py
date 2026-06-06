#!/usr/bin/env python3
"""pyActigraphy Sadeh adapter for goose-reference-algo-runner."""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA = "goose.external-reference-output.v1"
PROVIDER = "external.pyactigraphy.sadeh"
ALGORITHM_ID = "reference.sleep.pyactigraphy_sadeh.v1"
ALGORITHM_VERSION = "1.0.0"
SADEH_OFFSET = 7.601
SADEH_WEIGHTS = [-0.065, -1.08, -0.056, -0.703]
SADEH_THRESHOLD = 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description="Run pyActigraphy Sadeh sleep scoring for Goose.")
    parser.add_argument("--input", required=True)
    parser.add_argument("--family", required=True)
    parser.add_argument("--provider", required=True)
    parser.add_argument("--output-format", required=True)
    parser.add_argument(
        "--allow-hand-derived-fallback",
        action="store_true",
        help="Test-only deterministic fallback when pyActigraphy/pandas is not installed.",
    )
    args = parser.parse_args()

    if args.family != "sleep":
        return write_report(
            base_report({}, "unavailable"), errors=[f"unsupported_family:{args.family}"]
        )
    if args.provider != PROVIDER:
        return write_report(
            base_report({}, "unavailable"), errors=[f"unsupported_provider:{args.provider}"]
        )
    if args.output_format != SCHEMA:
        return write_report(
            base_report({}, "unavailable"),
            errors=[f"unsupported_output_format:{args.output_format}"],
        )

    try:
        payload = json.loads(Path(args.input).read_text())
    except Exception as exc:  # noqa: BLE001 - external adapters must fail as JSON.
        return write_report(
            base_report({}, "unavailable"), errors=[f"input_read_error:{type(exc).__name__}"]
        )

    counts, invalid_count = valid_activity_counts(payload.get("activity_counts", []))
    epoch_minutes = finite_positive(payload.get("epoch_minutes", 1.0), 1.0)
    quality_flags: list[str] = []
    if invalid_count:
        quality_flags.append("invalid_activity_count_dropped")
    if len(counts) < 11:
        return write_report(
            base_report(payload, "unavailable"),
            quality_flags=quality_flags,
            errors=["not_enough_activity_count_epochs_for_sadeh"],
        )

    pyactigraphy_available = importlib.util.find_spec("pyActigraphy") is not None
    pandas_available = importlib.util.find_spec("pandas") is not None
    if not pyactigraphy_available or not pandas_available:
        if not args.allow_hand_derived_fallback:
            missing = []
            if not pyactigraphy_available:
                missing.append("pyActigraphy")
            if not pandas_available:
                missing.append("pandas")
            return write_report(
                base_report(payload, "missing"),
                quality_flags=quality_flags + ["external_provider_unavailable"],
                errors=[f"missing_optional_dependency:{','.join(missing)}"],
            )
        score = hand_derived_sadeh(counts, epoch_minutes)
        return write_report(
            base_report(payload, "test-fallback"),
            output=score
            | {
                "epoch_count": len(counts),
                "valid_epoch_count": len(counts),
                "invalid_epoch_count": invalid_count,
            },
            quality_flags=quality_flags + ["hand_derived_test_fallback"],
        )

    try:
        pd = importlib.import_module("pandas")
        scoring = importlib.import_module("pyActigraphy.sleep.scoring_base")
        start = parse_time(payload.get("start_time", "1970-01-01T00:00:00Z"))
        index = pd.date_range(
            start=start,
            periods=len(counts),
            freq=f"{int(epoch_minutes * 60)}s",
        )
        series = pd.Series(counts, index=index)
        scored = scoring._sadeh(series, SADEH_OFFSET, SADEH_WEIGHTS, SADEH_THRESHOLD)
        sleep_flags = [bool(value) for value in list(scored.values)]
        score = summarize_sleep_flags(sleep_flags, epoch_minutes)
        return write_report(
            base_report(
                payload, getattr(importlib.import_module("pyActigraphy"), "__version__", "unknown")
            ),
            output=score
            | {
                "epoch_count": len(counts),
                "valid_epoch_count": len(counts),
                "invalid_epoch_count": invalid_count,
            },
            quality_flags=quality_flags,
        )
    except Exception as exc:  # noqa: BLE001 - external adapters must fail as JSON.
        return write_report(
            base_report(payload, "error"),
            quality_flags=quality_flags + ["external_provider_execution_error"],
            errors=[f"pyactigraphy_execution_error:{type(exc).__name__}"],
        )


def base_report(payload: dict[str, Any], provider_version: str) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "family": "sleep",
        "provider": PROVIDER,
        "provider_version": provider_version,
        "source": "pyActigraphy Sadeh sleep scoring",
        "license": "BSD-3-Clause",
        "algorithm_id": ALGORITHM_ID,
        "algorithm_version": ALGORITHM_VERSION,
        "display_name": "pyActigraphy Sadeh Sleep/Wake",
        "input_schema": "goose.sleep-actigraphy-counts-input.v1",
        "output_schema": "goose.sleep-pyactigraphy-sadeh-output.v1",
        "start_time": payload.get("start_time", ""),
        "end_time": payload.get("end_time", ""),
        "output": None,
        "output_units": {
            "epoch_count": "count",
            "valid_epoch_count": "count",
            "invalid_epoch_count": "count",
            "sleep_epoch_count": "count",
            "wake_epoch_count": "count",
            "time_in_bed_minutes": "minutes",
            "sleep_minutes": "minutes",
            "wake_minutes": "minutes",
            "sleep_efficiency_fraction": "fraction",
            "wake_after_sleep_onset_minutes": "minutes",
            "disturbance_count": "count",
            "fragmentation_index_per_hour": "events_per_hour",
        },
        "parameters": {
            "epoch_minutes": payload.get("epoch_minutes", 1.0),
            "offset": SADEH_OFFSET,
            "weights": SADEH_WEIGHTS,
            "threshold": SADEH_THRESHOLD,
            "activity_count_cleaning": "drop_nonfinite_or_negative_counts",
            "pyactigraphy_function": "pyActigraphy.sleep.scoring_base._sadeh",
        },
        "input_requirements": {
            "activity_counts": {
                "unit": "counts_per_epoch",
                "minimum_to_compute": 11,
                "epoch_minutes": payload.get("epoch_minutes", 1.0),
            }
        },
        "quality_gates": [
            "external_provider_exit_zero",
            "goose_contract_schema_match",
            "units_recorded",
            "non_empty_provenance",
            "at_least_11_epochs_for_sadeh_window",
        ],
        "quality_flags": [],
        "errors": [],
        "provenance": {
            "adapter": "tools/reference/pyactigraphy_sadeh.py",
            "input_ids": payload.get("input_ids", []),
            "library": "pyActigraphy",
            "library_docs": [
                "https://ghammad.github.io/pyActigraphy/_autosummary/pyActigraphy.sleep.ScoringMixin.Sadeh.html",
                "https://ghammad.github.io/pyActigraphy/_modules/pyActigraphy/sleep/scoring_base.html",
            ],
            "expected_values_policy": "external-reference-contract",
        },
    }


def valid_activity_counts(values: Any) -> tuple[list[float], int]:
    valid: list[float] = []
    invalid = 0
    if not isinstance(values, list):
        return valid, 1
    for value in values:
        try:
            number = float(value)
        except (TypeError, ValueError):
            invalid += 1
            continue
        if math.isfinite(number) and number >= 0.0:
            valid.append(number)
        else:
            invalid += 1
    return valid, invalid


def finite_positive(value: Any, fallback: float) -> float:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return fallback
    if math.isfinite(number) and number > 0.0:
        return number
    return fallback


def hand_derived_sadeh(counts: list[float], epoch_minutes: float) -> dict[str, Any]:
    sleep_flags = []
    probability_scores = []
    for index, count in enumerate(counts):
        window_11 = counts[max(0, index - 5) : min(len(counts), index + 6)]
        last_6 = counts[max(0, index - 5) : index + 1]
        mean_w5 = sum(window_11) / len(window_11)
        nat = sum(1 for value in window_11 if 50.0 <= value < 100.0)
        sd_last6 = population_sd(last_6)
        log_act = math.log(count + 1.0)
        ps = (
            SADEH_OFFSET
            + SADEH_WEIGHTS[0] * mean_w5
            + SADEH_WEIGHTS[1] * nat
            + SADEH_WEIGHTS[2] * sd_last6
            + SADEH_WEIGHTS[3] * log_act
        )
        probability_scores.append(ps)
        sleep_flags.append(ps >= SADEH_THRESHOLD)
    return summarize_sleep_flags(sleep_flags, epoch_minutes) | {
        "sadeh_probability_scores": probability_scores,
    }


def summarize_sleep_flags(sleep_flags: list[bool], epoch_minutes: float) -> dict[str, Any]:
    sleep_epoch_count = sum(1 for flag in sleep_flags if flag)
    wake_epoch_count = len(sleep_flags) - sleep_epoch_count
    sleep_minutes = sleep_epoch_count * epoch_minutes
    wake_minutes = wake_epoch_count * epoch_minutes
    if sleep_epoch_count == 0:
        wake_after_sleep_onset = 0.0
        disturbance_count = 0
    else:
        first_sleep = sleep_flags.index(True)
        last_sleep = len(sleep_flags) - 1 - list(reversed(sleep_flags)).index(True)
        sleep_period = sleep_flags[first_sleep : last_sleep + 1]
        wake_after_sleep_onset = sum(1 for flag in sleep_period if not flag) * epoch_minutes
        disturbance_count = sum(
            1
            for previous, current in zip(sleep_period, sleep_period[1:])
            if previous and not current
        )
    return {
        "sleep_epoch_count": sleep_epoch_count,
        "wake_epoch_count": wake_epoch_count,
        "time_in_bed_minutes": len(sleep_flags) * epoch_minutes,
        "sleep_minutes": sleep_minutes,
        "wake_minutes": wake_minutes,
        "sleep_efficiency_fraction": sleep_epoch_count / len(sleep_flags) if sleep_flags else 0.0,
        "wake_after_sleep_onset_minutes": wake_after_sleep_onset,
        "disturbance_count": disturbance_count,
        "fragmentation_index_per_hour": (
            disturbance_count / (sleep_minutes / 60.0) if sleep_minutes > 0.0 else 0.0
        ),
    }


def population_sd(values: list[float]) -> float:
    if not values:
        return 0.0
    mean = sum(values) / len(values)
    return math.sqrt(sum((value - mean) ** 2 for value in values) / len(values))


def parse_time(value: str) -> datetime:
    if value.endswith("Z"):
        value = value[:-1] + "+00:00"
    return datetime.fromisoformat(value).astimezone(timezone.utc)


def write_report(
    report: dict[str, Any],
    *,
    output: dict[str, Any] | None = None,
    quality_flags: list[str] | None = None,
    errors: list[str] | None = None,
) -> int:
    report["output"] = output
    report["quality_flags"] = quality_flags or []
    report["errors"] = errors or []
    json.dump(report, sys.stdout, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
