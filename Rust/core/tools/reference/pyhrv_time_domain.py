#!/usr/bin/env python3
"""pyHRV time-domain adapter for goose-reference-algo-runner."""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path
from typing import Any

SCHEMA = "goose.external-reference-output.v1"
PROVIDER = "external.pyhrv.hrv"
ALGORITHM_ID = "reference.hrv.pyhrv_time_domain.v1"
ALGORITHM_VERSION = "1.0.0"


def main() -> int:
    parser = argparse.ArgumentParser(description="Run pyHRV time-domain HRV for Goose.")
    parser.add_argument("--input", required=True)
    parser.add_argument("--family", required=True)
    parser.add_argument("--provider", required=True)
    parser.add_argument("--output-format", required=True)
    parser.add_argument(
        "--allow-hand-derived-fallback",
        action="store_true",
        help="Test-only deterministic fallback when pyHRV is not installed.",
    )
    args = parser.parse_args()

    if args.family != "hrv":
        return write_report(
            base_report({}, "unavailable"),
            errors=[f"unsupported_family:{args.family}"],
        )
    if args.provider != PROVIDER:
        return write_report(
            base_report({}, "unavailable"),
            errors=[f"unsupported_provider:{args.provider}"],
        )
    if args.output_format != SCHEMA:
        return write_report(
            base_report({}, "unavailable"),
            errors=[f"unsupported_output_format:{args.output_format}"],
        )

    input_path = Path(args.input)
    try:
        payload = json.loads(input_path.read_text())
    except Exception as exc:  # noqa: BLE001 - external adapters must fail as JSON.
        return write_report(
            base_report({}, "unavailable"),
            errors=[f"input_read_error:{type(exc).__name__}"],
        )

    intervals = payload.get("rr_intervals_ms", [])
    valid_intervals, invalid_count = valid_rr_intervals(intervals)
    quality_flags: list[str] = []
    if invalid_count:
        quality_flags.append("invalid_rr_interval_dropped")
    if len(valid_intervals) < 2:
        return write_report(
            base_report(payload, "unavailable"),
            quality_flags=quality_flags,
            errors=["not_enough_valid_rr_intervals"],
        )

    if importlib.util.find_spec("pyhrv") is None:
        if not args.allow_hand_derived_fallback:
            return write_report(
                base_report(payload, "missing"),
                quality_flags=quality_flags + ["external_provider_unavailable"],
                errors=["pyhrv_not_installed"],
            )
        output = hand_derived_time_domain(valid_intervals)
        return write_report(
            base_report(payload, "test-fallback"),
            output=output | {
                "valid_interval_count": len(valid_intervals),
                "invalid_interval_count": invalid_count,
            },
            quality_flags=quality_flags + ["hand_derived_test_fallback"],
        )

    try:
        td = importlib.import_module("pyhrv.time_domain")
        pyhrv = importlib.import_module("pyhrv")
        nni = td.nni_parameters(nni=valid_intervals)
        sdnn = td.sdnn(nni=valid_intervals)
        rmssd = td.rmssd(nni=valid_intervals)
        nn50 = td.nn50(nni=valid_intervals)
        output = {
            "mean_nn_ms": finite_float(tuple_value(nni, "nni_mean")),
            "rmssd_ms": finite_float(tuple_value(rmssd, "rmssd")),
            "sdnn_ms": finite_float(tuple_value(sdnn, "sdnn")),
            "nn50_count": int(tuple_value(nn50, "nn50")),
            "pnn50_fraction": finite_float(tuple_value(nn50, "pnn50")),
            "valid_interval_count": len(valid_intervals),
            "invalid_interval_count": invalid_count,
        }
        return write_report(
            base_report(payload, getattr(pyhrv, "__version__", "unknown")),
            output=output,
            quality_flags=quality_flags,
        )
    except Exception as exc:  # noqa: BLE001 - external adapters must fail as JSON.
        return write_report(
            base_report(payload, "error"),
            quality_flags=quality_flags + ["external_provider_execution_error"],
            errors=[f"pyhrv_execution_error:{type(exc).__name__}"],
        )


def base_report(payload: dict[str, Any], provider_version: str) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "family": "hrv",
        "provider": PROVIDER,
        "provider_version": provider_version,
        "source": "pyHRV time_domain nni_parameters, sdnn, rmssd, nn50",
        "license": "GPL-3.0",
        "algorithm_id": ALGORITHM_ID,
        "algorithm_version": ALGORITHM_VERSION,
        "display_name": "pyHRV Time Domain",
        "input_schema": "goose.hrv-input.v1",
        "output_schema": "goose.hrv-pyhrv-time-domain-output.v1",
        "start_time": payload.get("start_time", ""),
        "end_time": payload.get("end_time", ""),
        "output": None,
        "output_units": {
            "mean_nn_ms": "ms",
            "rmssd_ms": "ms",
            "sdnn_ms": "ms",
            "nn50_count": "count",
            "pnn50_fraction": "fraction",
            "valid_interval_count": "count",
            "invalid_interval_count": "count",
        },
        "parameters": {
            "rr_cleaning": "drop_nonfinite_or_outside_300_to_2000_ms",
            "pyhrv_functions": ["nni_parameters", "sdnn", "rmssd", "nn50"],
            "pnn50_policy": "pyhrv_ratio_between_nn50_and_interval_differences",
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
            "adapter": "tools/reference/pyhrv_time_domain.py",
            "input_ids": payload.get("input_ids", []),
            "library": "pyHRV",
            "library_docs": [
                "https://pyhrv.readthedocs.io/en/latest/_pages/api/time.html",
                "https://pyhrv.readthedocs.io/en/latest/_pages/api/hrv.html",
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


def hand_derived_time_domain(values: list[float]) -> dict[str, float | int]:
    mean_nn = sum(values) / len(values)
    rmssd = math.sqrt(sum((b - a) ** 2 for a, b in zip(values, values[1:])) / (len(values) - 1))
    sdnn = math.sqrt(sum((value - mean_nn) ** 2 for value in values) / (len(values) - 1))
    nn50_count = sum(1 for a, b in zip(values, values[1:]) if abs(b - a) > 50.0)
    pnn50_fraction = nn50_count / (len(values) - 1)
    return {
        "mean_nn_ms": mean_nn,
        "rmssd_ms": rmssd,
        "sdnn_ms": sdnn,
        "nn50_count": nn50_count,
        "pnn50_fraction": pnn50_fraction,
    }


def tuple_value(value: Any, key: str) -> Any:
    try:
        return value[key]
    except (TypeError, KeyError, IndexError):
        if hasattr(value, "as_dict"):
            return value.as_dict()[key]
        raise


def finite_float(value: Any) -> float:
    number = float(value)
    if not math.isfinite(number):
        raise ValueError("non_finite_pyhrv_value")
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
