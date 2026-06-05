#!/usr/bin/env python3
"""NeuroKit2 HRV adapter for goose-reference-algo-runner.

The adapter emits Goose's external reference contract on stdout. NeuroKit2 is
an optional local benchmark dependency; tests may use the explicit fallback flag
to verify the contract without installing Python science packages.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path
from typing import Any

SCHEMA = "goose.external-reference-output.v1"
PROVIDER = "external.neurokit2.hrv"
ALGORITHM_ID = "reference.hrv.neurokit2.v1"
ALGORITHM_VERSION = "1.0.0"
SAMPLING_RATE_HZ = 1000


def main() -> int:
    parser = argparse.ArgumentParser(description="Run NeuroKit2 time-domain HRV for Goose.")
    parser.add_argument("--input", required=True)
    parser.add_argument("--family", required=True)
    parser.add_argument("--provider", required=True)
    parser.add_argument("--output-format", required=True)
    parser.add_argument(
        "--allow-hand-derived-fallback",
        action="store_true",
        help="Test-only deterministic fallback when NeuroKit2 is not installed.",
    )
    args = parser.parse_args()

    if args.family != "hrv":
        return write_report(
            base_report(args, {}, "unavailable"),
            errors=[f"unsupported_family:{args.family}"],
        )
    if args.provider != PROVIDER:
        return write_report(
            base_report(args, {}, "unavailable"),
            errors=[f"unsupported_provider:{args.provider}"],
        )
    if args.output_format != SCHEMA:
        return write_report(
            base_report(args, {}, "unavailable"),
            errors=[f"unsupported_output_format:{args.output_format}"],
        )

    input_path = Path(args.input)
    try:
        payload = json.loads(input_path.read_text())
    except Exception as exc:  # noqa: BLE001 - contract should capture all adapter failures.
        return write_report(
            base_report(args, {}, "unavailable"),
            errors=[f"input_read_error:{type(exc).__name__}"],
        )

    intervals = payload.get("rr_intervals_ms", [])
    valid_intervals, invalid_count = valid_rr_intervals(intervals)
    quality_flags: list[str] = []
    if invalid_count:
        quality_flags.append("invalid_rr_interval_dropped")
    if len(valid_intervals) < 2:
        return write_report(
            base_report(args, payload, "unavailable"),
            quality_flags=quality_flags,
            errors=["not_enough_valid_rr_intervals"],
        )

    neurokit2 = importlib.util.find_spec("neurokit2")
    if neurokit2 is None:
        if not args.allow_hand_derived_fallback:
            return write_report(
                base_report(args, payload, "missing"),
                quality_flags=quality_flags + ["external_provider_unavailable"],
                errors=["neurokit2_not_installed"],
            )
        output = hand_derived_time_domain(valid_intervals)
        return write_report(
            base_report(args, payload, "test-fallback"),
            output=output | {
                "valid_interval_count": len(valid_intervals),
                "invalid_interval_count": invalid_count,
            },
            quality_flags=quality_flags + ["hand_derived_test_fallback"],
        )

    try:
        nk = importlib.import_module("neurokit2")
        peaks = nk.intervals_to_peaks(valid_intervals, sampling_rate=SAMPLING_RATE_HZ)
        table = nk.hrv_time(peaks, sampling_rate=SAMPLING_RATE_HZ, show=False)
        output = {
            "mean_nn_ms": finite_float(frame_value(table, "HRV_MeanNN")),
            "rmssd_ms": finite_float(frame_value(table, "HRV_RMSSD")),
            "sdnn_ms": finite_float(frame_value(table, "HRV_SDNN")),
            "pnn50_percent": finite_float(frame_value(table, "HRV_pNN50")),
            "valid_interval_count": len(valid_intervals),
            "invalid_interval_count": invalid_count,
        }
        output["pnn50_fraction"] = output["pnn50_percent"] / 100.0
        return write_report(
            base_report(args, payload, getattr(nk, "__version__", "unknown")),
            output=output,
            quality_flags=quality_flags,
        )
    except Exception as exc:  # noqa: BLE001 - external adapters must fail as JSON.
        return write_report(
            base_report(args, payload, "error"),
            quality_flags=quality_flags + ["external_provider_execution_error"],
            errors=[f"neurokit2_execution_error:{type(exc).__name__}"],
        )


def base_report(
    args: argparse.Namespace,
    payload: dict[str, Any],
    provider_version: str,
) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "family": "hrv",
        "provider": PROVIDER,
        "provider_version": provider_version,
        "source": "NeuroKit2 hrv_time via intervals_to_peaks",
        "license": "MIT",
        "algorithm_id": ALGORITHM_ID,
        "algorithm_version": ALGORITHM_VERSION,
        "display_name": "NeuroKit2 HRV Time Domain",
        "input_schema": "goose.hrv-input.v1",
        "output_schema": "goose.hrv-neurokit2-reference-output.v1",
        "start_time": payload.get("start_time", ""),
        "end_time": payload.get("end_time", ""),
        "output": None,
        "output_units": {
            "mean_nn_ms": "ms",
            "rmssd_ms": "ms",
            "sdnn_ms": "ms",
            "pnn50_percent": "percent",
            "pnn50_fraction": "fraction",
            "valid_interval_count": "count",
            "invalid_interval_count": "count",
        },
        "parameters": {
            "sampling_rate_hz": SAMPLING_RATE_HZ,
            "rr_cleaning": "drop_nonfinite_or_outside_300_to_2000_ms",
            "neurokit_functions": ["intervals_to_peaks", "hrv_time"],
        },
        "input_requirements": {
            "rr_intervals_ms": {
                "unit": "ms",
                "minimum_to_compute": 2,
                "valid_range_inclusive": [300.0, 2000.0],
            }
        },
        "quality_gates": [
            "external_provider_exit_zero",
            "goose_contract_schema_match",
            "units_recorded",
            "non_empty_provenance",
            "at_least_2_valid_rr_intervals_to_compute",
        ],
        "quality_flags": [],
        "errors": [],
        "provenance": {
            "adapter": "tools/reference/neurokit_hrv.py",
            "input_ids": payload.get("input_ids", []),
            "library": "NeuroKit2",
            "library_docs": [
                "https://neuropsychology.github.io/NeuroKit/_modules/neurokit2/hrv/intervals_to_peaks.html",
                "https://neuropsychology.github.io/NeuroKit/_modules/neurokit2/hrv/hrv_time.html",
            ],
            "expected_values_policy": "external-reference-contract",
        },
    }


def valid_rr_intervals(values: Any) -> tuple[list[float], int]:
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
        if math.isfinite(number) and 300.0 <= number <= 2000.0:
            valid.append(number)
        else:
            invalid += 1
    return valid, invalid


def hand_derived_time_domain(values: list[float]) -> dict[str, float]:
    mean_nn = sum(values) / len(values)
    rmssd = math.sqrt(sum((b - a) ** 2 for a, b in zip(values, values[1:])) / (len(values) - 1))
    sdnn = math.sqrt(sum((value - mean_nn) ** 2 for value in values) / (len(values) - 1))
    pnn50_fraction = sum(
        1 for a, b in zip(values, values[1:]) if abs(b - a) > 50.0
    ) / (len(values) - 1)
    return {
        "mean_nn_ms": mean_nn,
        "rmssd_ms": rmssd,
        "sdnn_ms": sdnn,
        "pnn50_percent": pnn50_fraction * 100.0,
        "pnn50_fraction": pnn50_fraction,
    }


def frame_value(frame: Any, key: str) -> Any:
    if hasattr(frame, "iloc"):
        return frame[key].iloc[0]
    if isinstance(frame, dict):
        value = frame[key]
        return value[0] if isinstance(value, list) else value
    raise TypeError("unsupported NeuroKit2 frame shape")


def finite_float(value: Any) -> float:
    number = float(value)
    if not math.isfinite(number):
        raise ValueError("non_finite_neurokit_value")
    return number


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
