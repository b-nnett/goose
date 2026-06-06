"""
Tests for ALG-13 resting metabolic rate (RMR) via Mifflin–St Jeor.

  - rmr_kcal_per_day(profile) — sex-keyed Mifflin–St Jeor RMR in kcal/day.

This is a pure function (no DB, no network): RMR is a linear function of
weight, height, age and sex. The full total_calories integration in compute_day
is exercised by test_daily.py (requires_docker).

Run offline:
    cd server/ingest && python -m pytest tests/test_calories_rmr.py -q
"""
from __future__ import annotations

from app.analysis.calories import rmr_kcal_per_day


def test_rmr_male_known_value() -> None:
    # Mifflin: 10×70 + 6.25×175 − 5×30 + 5 = 700 + 1093.75 − 150 + 5 = 1648.75
    r = rmr_kcal_per_day({"sex": "male", "weight_kg": 70, "height_cm": 175, "age": 30})
    assert abs(r - 1648.75) < 1.0, f"Expected ~1648.75, got {r}"


def test_rmr_female_known_value() -> None:
    # Mifflin: 10×60 + 6.25×165 − 5×25 − 161 = 600 + 1031.25 − 125 − 161 = 1345.25
    r = rmr_kcal_per_day({"sex": "female", "weight_kg": 60, "height_cm": 165, "age": 25})
    assert abs(r - 1345.25) < 1.0, f"Expected ~1345.25, got {r}"


def test_rmr_none_profile_is_none() -> None:
    assert rmr_kcal_per_day(None) is None


def test_rmr_empty_profile_uses_defaults_positive() -> None:
    # defaults: weight=70, height=170, age=30, nonbinary intercept (−78)
    r = rmr_kcal_per_day({})
    assert r is not None and r > 0


def test_rmr_nonbinary_intercept() -> None:
    # nonbinary: 10×70 + 6.25×170 − 5×30 − 78 = 700 + 1062.5 − 150 − 78 = 1534.5
    r = rmr_kcal_per_day({"sex": "nonbinary", "weight_kg": 70, "height_cm": 170, "age": 30})
    assert abs(r - 1534.5) < 1.0, f"Expected ~1534.5, got {r}"


def test_rmr_unknown_sex_falls_back_nonbinary() -> None:
    r_unknown = rmr_kcal_per_day({"sex": "xyz", "weight_kg": 70, "height_cm": 170, "age": 30})
    r_nb = rmr_kcal_per_day({"sex": "nonbinary", "weight_kg": 70, "height_cm": 170, "age": 30})
    assert r_unknown == r_nb


def test_rmr_never_negative() -> None:
    # Extreme inputs must clamp to >= 0, never negative.
    r = rmr_kcal_per_day({"sex": "female", "weight_kg": 1, "height_cm": 1, "age": 120})
    assert r >= 0.0
