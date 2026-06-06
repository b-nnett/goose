"""
Tests for the PURE backend-parity algorithms in analysis.daily:
  - ALG-11 training_state_from_lookup() — recovery+strain → training state label
  - ALG-12 sleep_needed() — rolling-7d baseline → personalised sleep need (min)

These are pure functions (no DB, no network): training_state reads a bundled
JSON lookup table; sleep_needed is statistics over a prior-nights list. The
DB-touching compute_day integration is exercised by test_daily.py (requires_docker).

Run offline:
    cd server/ingest && python -m pytest tests/test_daily_alg.py -q
"""
from __future__ import annotations

from app.analysis.daily import sleep_needed, training_state_from_lookup


# ── ALG-11 training_state_from_lookup ─────────────────────────────────────────

def test_training_state_returns_valid_label() -> None:
    result = training_state_from_lookup(75.0, 14.0)
    assert result in ("OPTIMAL", "RESTORATIVE", "OVERREACHING")


def test_training_state_none_recovery() -> None:
    assert training_state_from_lookup(None, 14.0) is None


def test_training_state_none_strain() -> None:
    assert training_state_from_lookup(50.0, None) is None


def test_training_state_never_impossible() -> None:
    # Sweep the whole recovery grid at extreme strains — must never be IMPOSSIBLE.
    for rec in range(0, 101):
        for strn in (0.0, 5.0, 14.0, 21.0, 30.0):
            label = training_state_from_lookup(float(rec), strn)
            assert label in ("OPTIMAL", "RESTORATIVE", "OVERREACHING")


def test_training_state_high_recovery_no_strain_is_restorative() -> None:
    # recovery 100 + strain 0 sits below lower_rec_strain → RESTORATIVE.
    assert training_state_from_lookup(100.0, 0.0) == "RESTORATIVE"


def test_training_state_high_recovery_huge_strain_is_overreaching() -> None:
    # recovery 100 + strain far above upper_rec_strain → OVERREACHING.
    assert training_state_from_lookup(100.0, 30.0) == "OVERREACHING"


def test_training_state_clamps_out_of_range_recovery() -> None:
    # Out-of-range recovery must not raise (clamped to [0, 100]).
    assert training_state_from_lookup(150.0, 14.0) in (
        "OPTIMAL", "RESTORATIVE", "OVERREACHING")
    assert training_state_from_lookup(-10.0, 14.0) in (
        "OPTIMAL", "RESTORATIVE", "OVERREACHING")


# ── ALG-12 sleep_needed ───────────────────────────────────────────────────────

def test_sleep_needed_empty_history_is_none() -> None:
    assert sleep_needed([], None, None) is None


def test_sleep_needed_too_few_nights_is_none() -> None:
    # < 3 valid nights → None.
    assert sleep_needed([420.0, 400.0], 12.0, 410.0) is None


def test_sleep_needed_filters_invalid_then_too_few() -> None:
    # Two valid + two invalid (<= 0) → still < 3 valid → None.
    assert sleep_needed([420.0, 0.0, -5.0, 410.0], 12.0, 410.0) is None


def test_sleep_needed_baseline_no_debt() -> None:
    # Low strain + slept the baseline → no debt → equals baseline.
    assert sleep_needed([420.0] * 6, 10.0, 420.0) == 420.0


def test_sleep_needed_clamped_range() -> None:
    val = sleep_needed([420.0] * 6, 10.0, 420.0)
    assert val is not None and 300.0 <= val <= 660.0


def test_sleep_needed_high_strain_and_debt_increases_need() -> None:
    val = sleep_needed([420.0] * 6, 20.0, 360.0)
    assert val is not None and val > 420.0


def test_sleep_needed_upper_clamp_660() -> None:
    # Huge baseline + max debts must clamp to 660.
    val = sleep_needed([700.0] * 6, 30.0, 0.0)
    assert val == 660.0


def test_sleep_needed_lower_clamp_300() -> None:
    # Tiny baseline (but valid > 0), no debts → clamps up to 300.
    val = sleep_needed([1.0] * 6, 5.0, 1.0)
    assert val == 300.0


def test_sleep_needed_returns_float() -> None:
    val = sleep_needed([420.0] * 6, 10.0, 420.0)
    assert isinstance(val, float)
