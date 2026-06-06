"""
calories.py — Heart-rate-based calorie estimation for detected exercise bouts.

This is an INDEPENDENT implementation of published, peer-reviewed energy-
expenditure equations. Outputs are APPROXIMATE estimates, not laboratory
calorimetry and not medical advice.

Two regimes, split by intensity
-------------------------------
ACTIVE (HR at/above the active threshold) — Keytel et al. (2005) HR-based energy
expenditure. Keytel gives gross EE in kilojoules per minute as a sex-specific
linear function of heart rate, body mass, and age:

    men:    EE(kJ/min) = -55.0969 + 0.6309·HR + 0.1988·weight_kg + 0.2017·age
    women:  EE(kJ/min) = -20.4022 + 0.4472·HR - 0.1263·weight_kg + 0.0740·age

We evaluate this per HR sample (1 sample = 1 second of data), so the per-second
kcal rate is ``EE(kJ/min) / (60 s/min × 4.184 kJ/kcal) = EE / 251.04`` (clamped
to ≥ 0, with HR capped at the estimated HRmax to keep the linear fit in range).

RESTING (HR below the active threshold) — basal metabolic rate from the revised
Harris–Benedict equations (Roza & Shock 1984), in kcal/day:

    men:    BMR = 88.362 + 13.397·weight_kg + 4.799·height_cm - 5.677·age
    women:  BMR = 447.593 + 9.247·weight_kg + 3.098·height_cm - 4.330·age

Internally we apply the height coefficient to height in METRES (so the stored
coefficient is 100× the per-cm value, e.g. 479.9 = 4.799 × 100). Resting kcal/s =
``max(0, BMR) / 86400``.

For sex == "nonbinary" / unknown / missing we use the arithmetic mean of the male
and female coefficient sets.

Active/resting split threshold
------------------------------
A sample counts as "active" once HR rises 30 % of the way from resting HR to
HRmax (i.e. ≥ 30 % heart-rate reserve) — a conventional light-activity cut-off.

Units
-----
    kcal (kilocalories / food calories)
    kJ   = kcal × 4.184

References
----------
  - Keytel, L.R. et al. (2005). "Prediction of energy expenditure from heart rate
    monitoring during submaximal exercise." *J. Sports Sci.*, 23(3), 289–297.
  - Roza, A.M. & Shock, H.M. (1984). "The Harris–Benedict equation reevaluated:
    resting energy requirements and the body cell mass." *Am. J. Clin. Nutr.*,
    40(1), 168–182.  (revised Harris–Benedict BMR.)
"""
from __future__ import annotations

from typing import Sequence

# ---------------------------------------------------------------------------
# Sex-keyed coefficient sets.
#
#   workout_*  → Keytel et al. (2005) HR-based EE coefficients (kJ/min form).
#   resting_*  → revised Harris–Benedict BMR coefficients (kcal/day form);
#                resting_height is applied to height in METRES (= per-cm × 100).
#
# "nonbinary" is the element-wise mean of the male and female sets.
# ---------------------------------------------------------------------------

_COEFFS: dict[str, dict[str, float]] = {
    "male": {
        # Harris–Benedict (revised, SI): 88.362 + 13.397·kg + 4.799·cm − 5.677·age
        "resting_alpha":   88.362,
        "resting_weight":  13.397,
        "resting_height":  479.9,   # applied to height in metres (= 4.799 per cm)
        "resting_age":     5.677,
        # Keytel 2005 (men), kJ/min: −55.0969 + 0.6309·HR + 0.1988·kg + 0.2017·age
        "workout_hr":      0.6309,
        "workout_weight":  0.1988,
        "workout_age":     0.2017,
        "workout_alpha":  -55.0969,
    },
    "female": {
        # Harris–Benedict (revised, SI): 447.593 + 9.247·kg + 3.098·cm − 4.330·age
        "resting_alpha":   447.593,
        "resting_weight":    9.247,
        "resting_height":  309.8,   # applied to height in metres (= 3.098 per cm)
        "resting_age":       4.33,
        # Keytel 2005 (women), kJ/min: −20.4022 + 0.4472·HR − 0.1263·kg + 0.0740·age
        "workout_hr":      0.4472,
        "workout_weight": -0.1263,
        "workout_age":     0.0740,
        "workout_alpha":  -20.4022,
    },
    "nonbinary": {
        # Element-wise mean of the male and female sets above.
        "resting_alpha":   267.9775,
        "resting_weight":   11.322,
        "resting_height":  394.85,
        "resting_age":       5.0035,
        "workout_hr":      0.53905,
        "workout_weight":  0.03625,
        "workout_age":     0.13785,
        "workout_alpha":  -37.74955,
    },
}

# ---------------------------------------------------------------------------
# Mifflin–St Jeor RMR coefficients (ALG-13).
#
# Distinct from the revised Harris–Benedict BMR used above for per-bout resting
# burn: ALG-13 uses Mifflin–St Jeor (1990), the modern standard for whole-day
# resting metabolic rate. In kcal/day, applied to height in CENTIMETRES:
#
#     men:    RMR = 10·kg + 6.25·cm − 5·age + 5
#     women:  RMR = 10·kg + 6.25·cm − 5·age − 161
#
# "nonbinary"/unknown uses the mean intercept of the two sex-specific forms
# (−78 = (5 + −161) / 2); weight/height/age coefficients are sex-invariant.
#
#   Mifflin, M.D. et al. (1990). "A new predictive equation for resting energy
#   expenditure in healthy individuals." Am. J. Clin. Nutr., 51(2), 241–247.
# ---------------------------------------------------------------------------

_MIFFLIN_COEFFS: dict[str, dict[str, float]] = {
    "male":      {"weight": 10.0, "height": 6.25, "age": 5.0, "intercept":    5.0},
    "female":    {"weight": 10.0, "height": 6.25, "age": 5.0, "intercept": -161.0},
    "nonbinary": {"weight": 10.0, "height": 6.25, "age": 5.0, "intercept":  -78.0},
}


def rmr_kcal_per_day(profile: dict | None) -> float | None:
    """Resting metabolic rate (kcal/day) via Mifflin–St Jeor (ALG-13).

    Parameters
    ----------
    profile :
        Dict with keys ``weight_kg``, ``height_cm``, ``age``, ``sex``. Missing
        numeric keys fall back to safe defaults (weight 70 kg, height 170 cm,
        age 30); unknown/missing sex → "nonbinary" (mean intercept). ``None``
        propagates as ``None`` (no profile → no whole-day calorie estimate).

    Returns
    -------
    float | None
        RMR in kcal/day, clamped to ≥ 0.0; ``None`` when ``profile is None``.
    """
    if profile is None:
        return None
    weight_kg = float(profile.get("weight_kg") or 70.0)
    height_cm = float(profile.get("height_cm") or 170.0)
    age = float(profile.get("age") or 30.0)
    sex = (profile.get("sex") or "").lower().strip()
    c = _MIFFLIN_COEFFS.get(sex, _MIFFLIN_COEFFS["nonbinary"])
    rmr = (
        weight_kg * c["weight"]
        + height_cm * c["height"]
        - age * c["age"]
        + c["intercept"]
    )
    return max(0.0, rmr)


# Active-threshold fraction of heart-rate reserve. Samples below resting_hr +
# 30 % HRR are counted at the resting (BMR) rate; at/above, the Keytel rate.
_ACTIVE_HRR_FRACTION = 0.30

# kJ/min → kcal/s conversion divisor: 60 s/min × 4.184 kJ/kcal.
_WORKOUT_DIVISOR = 251.04


def _resolve_coeffs(sex: str | None) -> dict[str, float]:
    """Return the coefficient dict for ``sex``.

    Accepts "male", "female", "nonbinary". Anything else (None, empty, unknown)
    falls back to "nonbinary" (the male/female mean).
    """
    if sex in _COEFFS:
        return _COEFFS[sex]
    return _COEFFS["nonbinary"]


def _resting_kcal_per_s(coeffs: dict[str, float], weight_kg: float,
                        height_cm: float, age: float) -> float:
    """Revised Harris–Benedict BMR → kcal/s. Height coefficient applies to metres."""
    height_m = height_cm / 100.0
    bmr_kcal_day = (
        coeffs["resting_alpha"]
        + coeffs["resting_weight"] * weight_kg
        + coeffs["resting_height"] * height_m
        - coeffs["resting_age"] * age
    )
    return max(0.0, bmr_kcal_day) / 86_400.0


def _active_kcal_per_s(coeffs: dict[str, float], hr: float, hrmax: float,
                       weight_kg: float, age: float) -> float:
    """Keytel 2005 HR-based active burn rate (kcal/s), clamped ≥ 0.

    HR is capped at ``hrmax`` so the submaximal linear fit is not extrapolated.
    """
    ee_kj_min = (
        coeffs["workout_hr"] * min(hr, hrmax)
        + coeffs["workout_weight"] * weight_kg
        + coeffs["workout_age"] * age
        + coeffs["workout_alpha"]
    )
    return max(0.0, ee_kj_min) / _WORKOUT_DIVISOR


def estimate_bout_calories(
    hr_samples: Sequence[dict],
    profile: dict,
    hrmax: float | None = None,
    resting_hr: float | None = None,
) -> tuple[float, float]:
    """Estimate active + resting calorie burn for a workout bout.

    Parameters
    ----------
    hr_samples :
        Time-ordered ``[{"ts": float, "bpm": float}, ...]`` for the bout window.
        Each sample is treated as 1 second of data (1 Hz store).
    profile :
        Dict with keys ``weight_kg``, ``height_cm``, ``age``, ``sex``. Missing
        keys fall back to safe defaults (and unknown sex → nonbinary).
    hrmax :
        Effective HRmax for this bout. Defaults to a conservative 220 if None.
    resting_hr :
        Day resting HR, used for the active/resting split threshold. Defaults
        to 60 if None.

    Returns
    -------
    (calories_kcal, calories_kj) : tuple[float, float]
    """
    weight_kg = float(profile.get("weight_kg") or 70.0)
    height_cm = float(profile.get("height_cm") or 170.0)
    age = float(profile.get("age") or 30.0)
    sex = (profile.get("sex") or "").lower().strip()

    coeffs = _resolve_coeffs(sex if sex in _COEFFS else None)

    eff_hrmax = float(hrmax) if hrmax is not None else 220.0
    eff_resting = float(resting_hr) if resting_hr is not None else 60.0

    # Active when HR ≥ resting + 30 % of heart-rate reserve.
    active_threshold = eff_resting + _ACTIVE_HRR_FRACTION * (eff_hrmax - eff_resting)

    resting_rate = _resting_kcal_per_s(coeffs, weight_kg, height_cm, age)

    total_kcal = 0.0
    for sample in hr_samples:
        bpm = sample.get("bpm")
        if bpm is None:
            continue
        bpm = float(bpm)
        if bpm < active_threshold:
            total_kcal += resting_rate
        else:
            total_kcal += _active_kcal_per_s(coeffs, bpm, eff_hrmax, weight_kg, age)

    return total_kcal, total_kcal * 4.184
