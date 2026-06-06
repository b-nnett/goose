"""
strain.py — Cardiovascular load expressed on a 0–21 strain scale.

This is an INDEPENDENT implementation of well-established, published
exercise-physiology methods. It is a WHOOP-*like* approximation, not a
reproduction of any proprietary algorithm, and is not medical advice.

Pipeline
--------
1. Heart-Rate Reserve (Karvonen):  HRR = HRmax − RHR  (requires HRmax > RHR).
2. Per-sample intensity as %HRR:   %HRR = (HR − RHR) / HRR × 100, clamped 0..100.
3. Training-impulse (TRIMP) accumulated over the window, by one of two methods:
     a. Edwards' 5-zone summation (default) — each sample contributes its
        zone weight (1..5, at 50/60/70/80/90 %HRR cut-offs) times its duration.
     b. Banister's exponential TRIMP — each sample contributes a continuous
        intensity-weighted value, avoiding step artifacts at zone boundaries.
4. Compression of accumulated TRIMP onto a bounded 0–21 scale via a logarithmic
   map ``strain = 21 × ln(TRIMP + 1) / ln(D)`` whose denominator D is anchored so
   the theoretical daily ceiling maps to 21 (see ``STRAIN_DENOMINATOR``).

References (primary published sources)
-------------------------------------
  - Karvonen, M.J., Kentala, E. & Mustala, O. (1957). "The effects of training
    on heart rate." *Ann. Med. Exp. Biol. Fenn.*, 35(3), 307–315.  (%HRR.)
  - Edwards, S. (1993). *The Heart Rate Monitor Book.* Sacramento: Fleet Feet
    Press.  (5-zone, weight-1..5 TRIMP.)
  - Banister, E.W. (1991). "Modeling elite athletic performance," in
    *Physiological Testing of the High-Performance Athlete* (2nd ed.),
    Champaign, IL: Human Kinetics, pp. 403–424.  (exponential TRIMP; the
    intensity weight is ``k·e^{b·x}`` with the sex-specific exponent b = 1.92
    for men and 1.67 for women, x = fractional %HRR.)
  - Tanaka, H., Monahan, K.D. & Seals, D.R. (2001). "Age-predicted maximal heart
    rate revisited." *J. Am. Coll. Cardiol.*, 37(1), 153–156.
    (HRmax = 208 − 0.7 × age.)

Window note: physiological "day strain" accumulates over the waking window
(wake → next sleep). Choosing that window is the caller's responsibility; this
module computes strain over whatever HR window it is given.

Backward-compatible function name: ``strain_score`` is an alias for ``strain``.
"""
from __future__ import annotations

import math
from typing import Any, Optional, Sequence

# ===========================================================================
# Named constants
# ===========================================================================

#: Minimum number of HR readings required before computing strain (≈10 min at
#: 1 Hz). Shorter windows do not contain enough load to estimate a daily-style
#: strain reliably, so we return ``None`` instead.
MIN_READINGS: int = 600

#: Top of the strain scale.
MAX_STRAIN: float = 21.0

#: Logarithmic-map denominator D in ``strain = 21 × ln(TRIMP+1) / ln(D)``.
#:
#: D is chosen so the theoretical daily ceiling maps to exactly 21.0. The Edwards
#: ceiling is sustaining the top zone (weight 5) for a full 24 h:
#:     TRIMP_max = 24 h × 60 min/h × 5 = 7200.
#: Setting D = 7200 + 1 = 7201 makes ``ln(TRIMP_max + 1) / ln(D) = 1`` so the
#: ceiling evaluates to 21.0.
#:
#: This is an INDEPENDENT scaling choice grounded in the Edwards zone ceiling, not
#: an empirically fitted value. The curve shape (monotone, concave, anchored at
#: 0 and 21) is correct, but the absolute level is APPROXIMATE. Calibrate D to a
#: user's own reference strain values with ``fit_strain_denominator`` if desired.
STRAIN_DENOMINATOR: float = 7201.0

#: Pre-computed ``ln(STRAIN_DENOMINATOR)``.
LN_STRAIN_DENOMINATOR: float = math.log(STRAIN_DENOMINATOR)

#: Sample duration (minutes) assumed when it cannot be inferred (1 s @ 1 Hz).
_FALLBACK_SAMPLE_MIN: float = 1.0 / 60.0

# ---------------------------------------------------------------------------
# HRmax / age defaults
# ---------------------------------------------------------------------------

#: Default age for the Tanaka / 220−age fallbacks when no age or HR history is
#: available. Real callers should pass the user's age.
DEFAULT_AGE: int = 30

#: Default resting HR (bpm) when the caller does not supply one. Real callers
#: should pass the per-night resting HR from the baselines module.
DEFAULT_RESTING_HR: int = 60

# ---------------------------------------------------------------------------
# HRmax estimation
# ---------------------------------------------------------------------------

#: Minimum HR samples before the observed high-percentile estimate is trusted.
HRMAX_MIN_SAMPLES: int = 600  # 10 min at 1 Hz

#: Upper percentile used for the observed-HRmax estimate. 99.5 captures genuine
#: peak efforts while discarding single-beat artifacts.
HRMAX_PERCENTILE: float = 99.5

# ---------------------------------------------------------------------------
# Banister exponential TRIMP coefficients (Banister 1991)
# ---------------------------------------------------------------------------

#: Pre-exponential scale factor on the intensity weight (both sexes).
BANISTER_SCALE: float = 0.64

#: Exponential coefficient for men (steeper high-intensity weighting).
BANISTER_B_MEN: float = 1.92

#: Exponential coefficient for women.
BANISTER_B_WOMEN: float = 1.67


# ===========================================================================
# HRmax helpers
# ===========================================================================

def tanaka_hrmax(age: float) -> float:
    """Tanaka et al. (2001): ``HRmax = 208 − 0.7 × age`` (gender-independent)."""
    return 208.0 - 0.7 * age


def default_max_hr(age: int = DEFAULT_AGE) -> int:
    """Classic ``220 − age`` age-predicted max HR. LAST-RESORT fallback only."""
    return 220 - age


def _percentile(sorted_values: Sequence[float], pct: float) -> float:
    """Linear-interpolated percentile of an already-sorted sequence (numpy-style)."""
    n = len(sorted_values)
    if n == 1:
        return float(sorted_values[0])
    position = (pct / 100.0) * (n - 1)
    lower = int(position)
    upper = min(lower + 1, n - 1)
    frac = position - lower
    return sorted_values[lower] + frac * (sorted_values[upper] - sorted_values[lower])


def estimate_hrmax(
    hr_history: Sequence[float],
    age: Optional[float] = None,
) -> tuple[float, str]:
    """Estimate a personalized HRmax from a trailing HR series.

    Selection logic:
      * With ≥ ``HRMAX_MIN_SAMPLES`` samples, take the 99.5th percentile of the
        history as the observed peak. If age is also known, return the LARGER of
        the observed peak and the Tanaka estimate (so a thin observed window never
        under-caps the zones), and report ``"observed"`` or ``"tanaka"`` according
        to which value won.
      * With thin history but a known age, use the Tanaka formula (it is more
        accurate than ``220 − age`` across the adult range), reporting ``"tanaka"``.
      * With neither, return ``(0.0, "unknown")`` — the caller must guard.

    Returns ``(hrmax_bpm, source)`` where source ∈ {"observed", "tanaka", "unknown"}.
    """
    n = len(hr_history)
    tanaka = tanaka_hrmax(age) if age is not None else None

    if n >= HRMAX_MIN_SAMPLES:
        observed = _percentile(sorted(hr_history), HRMAX_PERCENTILE)
        if tanaka is None:
            return observed, "observed"
        if observed >= tanaka:
            return observed, "observed"
        return tanaka, "tanaka"

    if tanaka is not None:
        return tanaka, "tanaka"

    return 0.0, "unknown"


# ===========================================================================
# Karvonen %HRR and Edwards zone weight
# ===========================================================================

def _pct_hrr(bpm: float, resting_hr: float, hr_reserve: float) -> float:
    """Karvonen percentage of heart-rate reserve, clamped to [0, 100]."""
    pct = (float(bpm) - float(resting_hr)) / hr_reserve * 100.0
    if pct < 0.0:
        return 0.0
    if pct > 100.0:
        return 100.0
    return pct


#: Edwards zone cut-offs as (%HRR threshold, weight), highest-first.
_EDWARDS_ZONES: tuple[tuple[float, int], ...] = (
    (90.0, 5),
    (80.0, 4),
    (70.0, 3),
    (60.0, 2),
    (50.0, 1),
)


def _zone_weight(bpm: float, resting_hr: float, hr_reserve: float) -> int:
    """Edwards 5-zone weight (0–5) from %HRR.

    Cut-offs (≥): 90→5, 80→4, 70→3, 60→2, 50→1, otherwise 0. %HRR is computed
    raw (unclamped) here, but values <0 fall through to weight 0 and values >100
    satisfy the ≥90 branch (weight 5), so it agrees with the clamped Banister path
    at both extremes.
    """
    pct = (float(bpm) - float(resting_hr)) / hr_reserve * 100.0
    for threshold, weight in _EDWARDS_ZONES:
        if pct >= threshold:
            return weight
    return 0


# ===========================================================================
# TRIMP accumulation (Edwards and Banister)
# ===========================================================================

def _sample_duration_minutes(hr_series: Sequence[dict]) -> float:
    """Infer the per-sample duration (minutes) from the first two timestamps.

    Falls back to 1 s (1/60 min) when there are fewer than two samples or the
    first two timestamps coincide.
    """
    if len(hr_series) < 2:
        return _FALLBACK_SAMPLE_MIN
    delta_s = abs(float(hr_series[1]["ts"]) - float(hr_series[0]["ts"]))
    return delta_s / 60.0 if delta_s else _FALLBACK_SAMPLE_MIN


def _edwards_trimp(
    hr_series: Sequence[dict],
    resting_hr: float,
    hr_reserve: float,
    sample_duration_min: float,
) -> float:
    """Edwards' zone TRIMP: Σ (sample_duration × zone_weight) over the window."""
    weighted_samples = 0
    for sample in hr_series:
        weighted_samples += _zone_weight(sample["bpm"], resting_hr, hr_reserve)
    return weighted_samples * sample_duration_min


def _banister_trimp(
    hr_series: Sequence[dict],
    resting_hr: float,
    hr_reserve: float,
    sample_duration_min: float,
    b: float = BANISTER_B_MEN,
) -> float:
    """Banister exponential TRIMP.

    Each sample contributes ``duration × x × 0.64 × e^{b·x}`` where ``x`` is the
    fractional %HRR (0..1). This continuously weights intensity, so a hard minute
    counts far more than an easy one without the step discontinuities of zones.
    """
    accumulated = 0.0
    for sample in hr_series:
        x = _pct_hrr(sample["bpm"], resting_hr, hr_reserve) / 100.0
        if x > 0.0:
            accumulated += sample_duration_min * x * BANISTER_SCALE * math.exp(b * x)
    return accumulated


# ===========================================================================
# Logarithmic map onto the 0–21 scale
# ===========================================================================

def _trimp_to_strain(trimp: float, denominator: float = STRAIN_DENOMINATOR) -> float:
    """Map accumulated TRIMP onto [0, 21] via ``21 × ln(TRIMP+1) / ln(D)``.

    TRIMP ≤ 0 maps to 0.0. Result is rounded to 2 dp. APPROXIMATE — see
    ``STRAIN_DENOMINATOR``.
    """
    if trimp <= 0.0:
        return 0.0
    value = MAX_STRAIN * math.log(trimp + 1.0) / math.log(denominator)
    return round(value, 2)


# ===========================================================================
# Denominator calibration helper
# ===========================================================================

def fit_strain_denominator(
    trimp_whoop_pairs: Sequence[tuple[float, float]],
) -> float:
    """Calibrate the log-map denominator D from (TRIMP, reference_strain) pairs.

    Each pair satisfies ``strain = 21 × ln(TRIMP+1) / ln(D)``. Writing
    ``x = ln(TRIMP+1)`` and ``y = strain``, this is the through-origin line
    ``y = (21 / ln D) · x``. The least-squares slope is ``Σ(xy) / Σ(x²)``, so

        ln(D) = 21 × Σ(x²) / Σ(xy)   →   D = exp(ln D).

    Use this once a couple of weeks of (TRIMP, reference-strain) pairs are
    available; until then the default ``STRAIN_DENOMINATOR`` is an uncalibrated
    approximation.

    Raises ``ValueError`` when fewer than 2 usable pairs (TRIMP>0 and strain>0)
    are given or the system is degenerate.
    """
    usable = [(t, s) for t, s in trimp_whoop_pairs if t > 0 and s > 0]
    if len(usable) < 2:
        raise ValueError(
            "fit_strain_denominator requires at least 2 valid (TRIMP, strain) pairs "
            "(TRIMP > 0 and strain > 0)."
        )

    sum_xx = 0.0
    sum_xy = 0.0
    for trimp, strain_value in usable:
        x = math.log(trimp + 1.0)
        sum_xx += x * x
        sum_xy += x * strain_value

    if sum_xy <= 0 or sum_xx <= 0:
        raise ValueError(
            "Degenerate fit: Σ(x·y) or Σ(x²) is zero. "
            "Ensure pairs span a range of TRIMP values."
        )

    return math.exp(MAX_STRAIN * sum_xx / sum_xy)


# ===========================================================================
# Public API
# ===========================================================================

def strain(
    hr_series: Sequence[dict[str, Any]],
    max_hr: Optional[float] = None,
    resting_hr: float = DEFAULT_RESTING_HR,
    method: str = "edwards",
    sex: str = "male",
    denominator: float = STRAIN_DENOMINATOR,
) -> Optional[float]:
    """Cardiovascular strain (0–21) from an HR series. APPROXIMATE.

    Computes strain over whatever HR window it is given (the caller selects the
    physiologically meaningful window).

    Parameters
    ----------
    hr_series :
        Time-ordered ``[{"ts": <unix s>, "bpm": int}, ...]``.
    max_hr :
        Max HR (bpm). Defaults to ``220 − DEFAULT_AGE`` when omitted; use
        ``estimate_hrmax`` for a personalized value.
    resting_hr :
        Resting HR (bpm) for the HRR denominator. Defaults to
        ``DEFAULT_RESTING_HR`` (60); real callers should pass a baseline RHR.
    method :
        ``"edwards"`` (default; zone step weights) or ``"banister"`` (continuous
        exponential weighting).
    sex :
        ``"male"`` / ``"female"`` — selects the Banister coefficient (1.92 / 1.67).
        Ignored by the Edwards method.
    denominator :
        Log-map denominator D (default ``STRAIN_DENOMINATOR``); pass a fitted D
        from ``fit_strain_denominator`` once available.

    Returns
    -------
    float or None :
        Strain in [0, 21] (2 dp), or ``None`` when there are fewer than
        ``MIN_READINGS`` samples or ``max_hr ≤ resting_hr`` (invalid HRR).
    """
    if max_hr is None:
        max_hr = float(default_max_hr())

    if len(hr_series) < MIN_READINGS or max_hr <= resting_hr:
        return None

    sample_duration_min = _sample_duration_minutes(hr_series)
    hr_reserve = float(max_hr) - float(resting_hr)

    if method == "banister":
        b = BANISTER_B_WOMEN if sex.lower().startswith("f") else BANISTER_B_MEN
        trimp = _banister_trimp(hr_series, resting_hr, hr_reserve, sample_duration_min, b=b)
    else:
        trimp = _edwards_trimp(hr_series, resting_hr, hr_reserve, sample_duration_min)

    return _trimp_to_strain(trimp, denominator=denominator)


#: Backward-compatible alias used by daily.py and downstream callers.
strain_score = strain
