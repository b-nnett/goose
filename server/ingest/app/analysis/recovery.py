"""
recovery.py — Resting HR during sleep + an HRV-driven recovery score (0–100).

``resting_hr(streams, sleep_session)``
  The lowest sustained HR during the in-bed window: the minimum of a short
  rolling-mean of the HR samples whose ts falls inside the session. Returns an
  ``int`` (rounded bpm), or ``None`` when there are no HR samples in the window.

``recovery_score(hrv, resting_hr, resp, sleep_perf, baselines)``
  A **z-score + logistic** recovery percentage in [0, 100].  APPROXIMATE — not
  WHOOP-identical (WHOOP's exact model is proprietary).  This is a transparent,
  HRV-dominant, baseline-normalized proxy using the formulation from:
    docs/research/03-recovery-strain.md §1.3

  Direction of each driver (vs the person's rolling personal baseline):
    - higher HRV vs baseline       → higher recovery  (dominant driver, W=0.60)
    - lower resting HR vs baseline → higher recovery  (W=0.20)
    - lower resp vs baseline       → higher recovery  (W=0.05)
    - higher sleep_perf            → higher recovery  (W=0.15)

  Each metric is standardized to a z-score using the personal baseline's mean
  and robust dispersion (from baselines.BaselineState).  Missing terms are dropped
  and weights renormalized.  The composite z is squashed via a logistic so that
  Z=0 → ~58% (WHOOP's published average recovery).

  Resp note: resp is raw/uncalibrated ADC counts, but z-score computation is
  scale-invariant (deviation / own dispersion), so the absolute units don't matter
  as long as the baseline and tonight's value use the same units.  Raw resp is
  acceptable here.  Document in caller.

  Sleep-perf note: sleep efficiency (0..1) from the sleep summary is used as the
  sleep-performance proxy.  This differs from WHOOP's "sleep you got / sleep you
  needed" metric (which uses sleep need, not just efficiency), but it is the best
  available signal from our pipeline.  Centered at 0.85 (doc §1.3 anchor: "good
  night"), scale 0.12 (empirical for a plausible ±2σ band).

  Cold-start: if the HRV baseline is not yet usable (fewer than MIN_NIGHTS_SEED=4
  valid nights), the function returns ``None``.  Callers may substitute
  RECOVERY_POPULATION_MEAN (58.0) as a fallback, but should flag it clearly.
  Returning ``None`` is more honest than returning a fake score.

------------------------------------------------------------------------------
Input shapes
------------------------------------------------------------------------------
``streams: dict[str, list[dict]]`` — same as sleep.py; only "hr" is used here:
    "hr": [{"ts": <unix seconds>, "bpm": int}, ...]
``sleep_session: SleepSession`` — has ``.start`` / ``.end`` epoch-seconds floats.

``baselines`` is a mapping/object with attributes:
    ``hrv``        (BaselineState or (mean, dispersion) pair or None)
    ``resting_hr`` (BaselineState or (mean, dispersion) pair or None)
    ``resp``       (BaselineState or (mean, dispersion) pair or None)

  For backward-compatibility with dict-baseline callers (plain float means), a
  mapping ``{"hrv": float, "resting_hr": float, "resp": float}`` is also accepted,
  in which case the metric's own σ_floor is used as the dispersion (the pre-v2
  behavior, gracefully degraded).  Prefer passing ``BaselineState`` objects.

References:
  - docs/research/03-recovery-strain.md §1 (Recovery composite)
  - docs/research/03-recovery-strain.md §2 (personal baseline)
  - docs/research/06-baselines-validation.md §1 (Winsorized EWMA, cold-start)
  - .hrv (RMSSD is the HRV input)
  - .sleep (SleepSession, per-session resting HR convention)
"""
from __future__ import annotations

import math
import statistics
from typing import Any, Mapping, Optional, Sequence

from ._utils import to_epoch as _to_epoch
from .baselines import BaselineState, METRIC_CFG

# ===========================================================================
# Named thresholds — resting_hr
# ===========================================================================

#: Rolling-mean HR window (seconds) for the resting-HR estimate. Matches the
#: sleep module's per-session resting-HR window for consistency.
RESTING_HR_WINDOW_S: float = 5 * 60.0

# ===========================================================================
# Recovery formula constants (doc 03 §1.3) — all tunable
# ===========================================================================

#: HRV-dominant weight (dominant factor in WHOOP's model).
W_HRV: float = 0.60

#: Resting HR weight.
W_RHR: float = 0.20

#: Respiratory rate weight (kept small — near-constant; mostly an illness flag).
W_RESP: float = 0.05

#: Sleep performance weight.
W_SLEEP: float = 0.15

#: Logistic spread parameter: controls how steeply recovery changes per z-unit.
#: k=1.6 → ±2 z-units covers approximately the full Red–Green band (15%–95%).
LOGISTIC_K: float = 1.6

#: Logistic offset: shifts the midpoint so that Z=0 → 58% (WHOOP published average).
#: 100 / (1 + exp(-k * (0 - Z0))) = 58%  → Z0 ≈ -0.20.
LOGISTIC_Z0: float = -0.20

#: WHOOP-published population-average recovery (% ).  Used as the cold-start fallback.
RECOVERY_POPULATION_MEAN: float = 58.0

#: Recovery band thresholds matching WHOOP color scheme (doc 03 §1.3).
BAND_RED_MAX: float = 34.0    # Red  : [0, 34)
BAND_YELLOW_MAX: float = 67.0 # Yellow: [34, 67)
                               # Green : [67, 100]

#: Sleep performance center (doc §1.3): a "good night" anchor at ~85% efficiency.
SLEEP_PERF_CENTER: float = 0.85

#: Sleep performance scale: empirical, so ±2 z-units spans roughly the normal range.
#: At efficiency=0.73 → z ≈ -1; at efficiency=0.97 → z ≈ +1.
SLEEP_PERF_SCALE: float = 0.12

# LEGACY constant kept for backward-compat with tests that import it:
RECOVERY_BASELINE_SCORE: float = RECOVERY_POPULATION_MEAN


# ===========================================================================
# resting_hr  (UNCHANGED — matches WHOOP's last-slow-wave-sleep approach)
# ===========================================================================

def resting_hr(streams: dict[str, list[dict]], sleep_session: Any) -> int | None:
    """Lowest sustained HR during the sleep window (bpm, int), or ``None``.

    "Sustained" = the minimum of 5-minute windowed (non-overlapping bin) means
    of the HR samples whose ts ∈ [session.start, session.end]. This rejects
    single-beat dips while still capturing the night's true floor.

    Convention — no HR samples in the window: returns ``None`` (documented
    sentinel; callers decide how to treat a missing resting HR).
    """
    hr = streams.get("hr") or []
    start = float(sleep_session.start)
    end = float(sleep_session.end)

    seg: list[tuple[float, float]] = []
    for r in hr:
        if r.get("bpm") is None:
            continue
        ts = _to_epoch(r["ts"])
        if start <= ts <= end:
            seg.append((ts, float(r["bpm"])))

    if not seg:
        return None

    means: list[float] = []
    t = start
    while t < end:
        win = [v for ts, v in seg if t <= ts < t + RESTING_HR_WINDOW_S]
        if win:
            means.append(statistics.fmean(win))
        t += RESTING_HR_WINDOW_S

    floor = min(means) if means else statistics.fmean(v for _, v in seg)
    return round(floor)


# ===========================================================================
# Internal helpers
# ===========================================================================

def _extract_baseline_mean_spread(
    baselines: Any, key: str
) -> tuple[float | None, float | None]:
    """Extract (mean, spread) for ``key`` from a baselines mapping or object.

    Accepts three forms:
      1. ``BaselineState`` attribute: returns (state.baseline, state.spread).
      2. Plain float: returns (float, σ_floor from METRIC_CFG) — legacy fallback.
      3. (mean, spread) tuple: used directly.
      4. Missing/None: returns (None, None).
    """
    if baselines is None:
        return None, None

    val: Any
    if isinstance(baselines, Mapping):
        val = baselines.get(key)
    else:
        val = getattr(baselines, key, None)

    if val is None:
        return None, None

    if isinstance(val, BaselineState):
        return val.baseline, val.spread

    if isinstance(val, (tuple, list)) and len(val) == 2:
        m, s = val
        return (float(m) if m is not None else None,
                float(s) if s is not None else None)

    # Legacy: plain float scalar → use metric σ_floor as dispersion.
    try:
        f = float(val)
        floor = METRIC_CFG.get(key, METRIC_CFG["hrv"]).floor_spread
        return f, floor
    except (TypeError, ValueError):
        return None, None


def _z_score(value: float, mean: float, spread: float) -> float:
    """Robust z-score using EWMA spread (see baselines.py deviation()).

    sigma_approx = 1.253 * spread  (converts EWMA-abs-dev to approx Gaussian σ).
    """
    sigma = max(1.253 * spread, 1e-9)
    return (value - mean) / sigma


def recovery_band(score: float) -> str:
    """Return the WHOOP-style color band for a given recovery score [0, 100]."""
    if score < BAND_RED_MAX:
        return "red"
    if score < BAND_YELLOW_MAX:
        return "yellow"
    return "green"


# ===========================================================================
# recovery_score
# ===========================================================================

def recovery_score(
    hrv: float,
    rhr: float,
    resp: float | None,
    baselines: Any,
    sleep_perf: float | None = None,
) -> float | None:
    """Z-score + logistic recovery score in [0, 100].  APPROXIMATE.

    Returns ``None`` when the HRV baseline is not yet usable (cold-start: fewer
    than MIN_NIGHTS_SEED valid nights).
    Callers may use ``RECOVERY_POPULATION_MEAN`` (58.0) as a fallback.

    Parameters
    ----------
    hrv :
        Tonight's HRV (RMSSD, ms).
    rhr :
        Tonight's resting HR (bpm).  Matches the ``resting_hr`` function's output.
    resp :
        Tonight's respiration (raw ADC counts OR calibrated brpm — z-score is
        scale-invariant, so either is valid as long as baseline uses the same units).
        ``None`` → resp term is dropped and weight renormalized.
    baselines :
        Personal baselines — mapping/object with ``hrv``, ``resting_hr``, ``resp``
        attributes, each a ``BaselineState`` (preferred) or plain float (legacy).
    sleep_perf :
        Sleep performance proxy (efficiency, 0..1).  ``None`` → term dropped and
        weight renormalized.

    Returns
    -------
    float | None
        Recovery in [0, 100], or ``None`` if the HRV baseline is not trusted.
    """
    # ── Extract baseline mean + spread per metric ─────────────────────────────
    b_hrv_mean, b_hrv_spread = _extract_baseline_mean_spread(baselines, "hrv")
    b_rhr_mean, b_rhr_spread = _extract_baseline_mean_spread(baselines, "resting_hr")
    b_resp_mean, b_resp_spread = _extract_baseline_mean_spread(baselines, "resp")

    # ── Cold-start gate ───────────────────────────────────────────────────────
    # Check if the HRV baseline (dominant driver) comes from a BaselineState.
    # If it does and it's not yet trusted, return None (too few nights of data).
    raw_hrv_val: Any
    if isinstance(baselines, Mapping):
        raw_hrv_val = baselines.get("hrv")
    else:
        raw_hrv_val = getattr(baselines, "hrv", None)

    if isinstance(raw_hrv_val, BaselineState) and not raw_hrv_val.usable:
        return None  # cold-start: HRV baseline not yet usable (< MIN_NIGHTS_SEED valid nights)

    # ── Compute per-metric z-scores (recovery-favorable direction) ────────────
    # z_hrv:  higher HRV  → more positive (good)
    # z_rhr:  lower  RHR  → more positive (good)  → flip sign
    # z_resp: lower  resp → more positive (good)  → flip sign
    # z_sleep: higher sleep_perf → more positive (good)
    terms: list[tuple[float, float]] = []  # (z_value, weight) pairs

    # HRV term
    if b_hrv_mean is not None and b_hrv_spread is not None:
        z_hrv = _z_score(float(hrv), b_hrv_mean, b_hrv_spread)
        terms.append((z_hrv, W_HRV))

    # RHR term (inverted: lower RHR is better)
    if b_rhr_mean is not None and b_rhr_spread is not None:
        z_rhr = _z_score(b_rhr_mean, float(rhr), b_rhr_spread)  # (μ - x) / σ
        terms.append((z_rhr, W_RHR))

    # Resp term (inverted: lower resp is better), optional
    if resp is not None and b_resp_mean is not None and b_resp_spread is not None:
        z_resp = _z_score(b_resp_mean, float(resp), b_resp_spread)  # (μ - x) / σ
        terms.append((z_resp, W_RESP))

    # Sleep-perf term — no baseline needed; centered at SLEEP_PERF_CENTER
    if sleep_perf is not None:
        z_sleep = (float(sleep_perf) - SLEEP_PERF_CENTER) / SLEEP_PERF_SCALE
        terms.append((z_sleep, W_SLEEP))

    if not terms:
        # No valid metric at all — return None.
        return None

    # ── Drop + renormalize missing terms ──────────────────────────────────────
    total_weight = sum(w for _, w in terms)
    if total_weight <= 0.0:
        return None
    Z = sum(z * w for z, w in terms) / total_weight

    # ── Logistic squash to [0, 100] ───────────────────────────────────────────
    # recovery = 100 / (1 + exp(-k * (Z - Z0)))
    # At Z=0: recovery ≈ 58% (WHOOP population average).
    score = 100.0 / (1.0 + math.exp(-LOGISTIC_K * (Z - LOGISTIC_Z0)))

    # Clamp for floating-point safety (logistic is theoretically bounded but
    # extreme inputs could produce values just outside due to fp precision).
    return max(0.0, min(100.0, score))
