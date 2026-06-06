"""
baselines.py — Personal-baseline module: robust rolling EWMA per nightly metric.

Provides a pure, IO-free implementation of the personal-baseline model described in:
  - docs/research/03-recovery-strain.md §2 (EWMA with 14-day half-life, MAD dispersion)
  - docs/research/06-baselines-validation.md §1 (Winsorized EWMA, cold-start gates,
    deviation forms)

Reconciliation of the two docs
--------------------------------
Doc 03 recommends a plain EWMA (14-day half-life, ~alpha=0.0483) plus a *windowed* MAD
over the trailing 30 valid nights for dispersion.  Doc 06 recommends a Winsorized /
Huberized EWMA (clamp before folding) with an *EWMA-of-abs-deviation* as the spread
tracker, plus cold-start flags based on n_valid.  This module prefers the more robust
choice from each:

  - Mean:       Winsorized EWMA from doc 06 (EWMA recency + outlier resistance).
                Half-life H_BASELINE=14 nights (doc 03's 14-night half-life,
                alpha ≈ 0.0483).
  - Dispersion: EWMA-of-abs-deviation from doc 06 (streaming, no buffer needed), with
                a metric-specific σ_floor (doc 03 names HRV=3 ms, RHR=1.5 bpm,
                resp=0.3 brpm; doc 06 names HRV=5 ms, RHR=2 bpm — we take the more
                conservative floor to avoid over-sensitivity).
  - Outlier gate (hard reject): |value - baseline| > HARD_OUTLIER_K * spread, with
                HARD_OUTLIER_K = 5 (doc 06 §1 Step 0).
  - Winsor clamp (soft reject): clamp before EWMA update with WINSOR_K = 3 (doc 06
                §1 Step 1).
  - Cold-start: trusted requires n_valid >= MIN_NIGHTS_TRUST = 14 (doc 06 §1.3
                full-trust threshold).  Below MIN_NIGHTS_SEED = 4 (matches WHOOP's
                grayed-out window), status = "calibrating".
  - Missing nights: carry state forward without decaying (skip-and-hold).

All outputs are APPROXIMATE — see recovery.py for the full disclaimer.

Public API
----------
  MetricCfg      — per-metric configuration namedtuple.
  BaselineState  — frozen dataclass returned from update/fold.
  METRIC_CFG     — default config for "hrv", "resting_hr", "resp".
  update_baseline(state, value, cfg) → BaselineState
  fold_history(values, cfg)          → BaselineState  (replay list → final state)
  deviation(value, state)            → Deviation       (z, delta, ratio)
  trimmed_mean_baseline(values, cfg) → BaselineState   (simpler auditability path)
"""
from __future__ import annotations

import statistics
from dataclasses import dataclass
from typing import NamedTuple, Optional, Sequence

# ===========================================================================
# Configuration
# ===========================================================================

class MetricCfg(NamedTuple):
    """Per-metric configuration for the baseline model."""
    min_val: float          # physiological lower bound (hard reject below)
    max_val: float          # physiological upper bound (hard reject above)
    floor_spread: float     # σ_floor: minimum dispersion (prevents over-sensitivity)
    half_life_b: float      # baseline center half-life in nights (→ λ_b)
    half_life_s: float      # spread half-life in nights (→ λ_s; slower than center)


def _lambda(half_life: float) -> float:
    """Convert a half-life in nights to an EWMA smoothing factor."""
    return 1.0 - 0.5 ** (1.0 / half_life)


#: Winsorization clamp: fold only within ±WINSOR_K * spread from current baseline.
WINSOR_K: float = 3.0

#: Hard-reject gate: drop night from state update if > HARD_OUTLIER_K * spread away.
HARD_OUTLIER_K: float = 5.0

#: Minimum valid nights before the baseline is "provisionally" trusted.
MIN_NIGHTS_SEED: int = 4

#: Minimum valid nights before the baseline is fully trusted (matches WHOOP 4-day
#: grayed-out window extended to two-week adaptation period per doc 06 §1.3).
MIN_NIGHTS_TRUST: int = 14

#: Number of missing nights after which the baseline is marked stale.
STALE_DAYS: int = 14

#: Default per-metric configurations.
#: Floor spreads: taking the more conservative (larger) of doc 03 and doc 06 values.
METRIC_CFG: dict[str, MetricCfg] = {
    "hrv": MetricCfg(
        min_val=5.0, max_val=250.0,
        floor_spread=5.0,   # doc 03 says 3 ms; doc 06 says 5 ms → take 5 ms
        half_life_b=14.0,   # 14-night center half-life (doc 03 alpha ≈ 0.0483)
        half_life_s=21.0,   # slower spread tracking
    ),
    "resting_hr": MetricCfg(
        min_val=30.0, max_val=120.0,
        floor_spread=2.0,   # doc 03 says 1.5 bpm; doc 06 says 2 bpm → take 2 bpm
        half_life_b=14.0,
        half_life_s=21.0,
    ),
    "resp": MetricCfg(
        min_val=4.0, max_val=40.0,
        floor_spread=0.5,   # doc 03 says 0.3 brpm; doc 06 says 0.5 → take 0.5
        half_life_b=14.0,
        half_life_s=21.0,
    ),
    "skin_temp": MetricCfg(
        min_val=20.0, max_val=42.0,
        floor_spread=0.3,
        half_life_b=14.0,
        half_life_s=21.0,
    ),
}


# ===========================================================================
# State dataclass
# ===========================================================================

@dataclass(frozen=True)
class BaselineState:
    """Immutable snapshot of a personal baseline for one metric after N nights.

    Attributes
    ----------
    baseline : float
        Robust EWMA center (the personal "mean" for this metric).
    spread : float
        EWMA of absolute deviations, floored at ``cfg.floor_spread``.  Used as a
        robust σ estimate; multiply by ``1.253`` to approximate the Gaussian σ
        (since E[|X-μ|] = σ·√(2/π) ≈ σ/1.253 for ~normal data).
    n_valid : int
        Count of valid nights that have contributed to the state.
    nights_since_update : int
        Number of consecutive nights with no valid value (for staleness detection).
    status : str
        ``"calibrating"``  — fewer than MIN_NIGHTS_SEED valid nights; no score yet.
        ``"provisional"``  — between MIN_NIGHTS_SEED and MIN_NIGHTS_TRUST; usable
                              but uncertainty is higher than the normal range implies.
        ``"trusted"``      — at least MIN_NIGHTS_TRUST valid nights; fully trusted.
        ``"stale"``        — trusted/provisional but no update for > STALE_DAYS nights.
    """
    baseline: float
    spread: float
    n_valid: int
    nights_since_update: int
    status: str

    @property
    def trusted(self) -> bool:
        """True iff the baseline is fully trusted (not calibrating or stale)."""
        return self.status == "trusted"

    @property
    def usable(self) -> bool:
        """True iff the baseline is at least provisionally usable (n_valid ≥ MIN_NIGHTS_SEED)."""
        return self.status in ("provisional", "trusted")


# ===========================================================================
# Deviation result
# ===========================================================================

@dataclass(frozen=True)
class Deviation:
    """Three forms of deviation from the personal baseline (doc 06 §1.4).

    Attributes
    ----------
    z : float
        Robust z-score: ``(value - baseline) / (1.253 * spread)``.  Scale-invariant.
        Positive = above baseline, negative = below.
    delta : float
        Signed absolute delta: ``value - baseline``.  Physical units (ms, bpm, °C).
        Preferred for skin-temp health-monitor alerts.
    ratio : float
        Fractional deviation: ``value / baseline - 1``.  Zero = at baseline.
        Kept for compatibility with recovery's prior ratio-space baseline contract.
    in_normal_range : bool
        True iff ``|z| <= 1.0`` (within one "normal spread" of the personal baseline).
    """
    z: float
    delta: float
    ratio: float
    in_normal_range: bool


# ===========================================================================
# Core update function
# ===========================================================================

def _compute_status(n_valid: int, nights_since_update: int) -> str:
    if nights_since_update > STALE_DAYS and n_valid >= MIN_NIGHTS_SEED:
        return "stale"
    if n_valid < MIN_NIGHTS_SEED:
        return "calibrating"
    if n_valid < MIN_NIGHTS_TRUST:
        return "provisional"
    return "trusted"


def update_baseline(
    state: Optional[BaselineState],
    value: Optional[float],
    cfg: MetricCfg,
) -> BaselineState:
    """Incorporate one new nightly value into the baseline state.

    Implements the Winsorized EWMA from doc 06 §1.3 Steps 0–1:
      - Step 0: reject physiologically implausible values and hard outliers.
      - Step 1: Winsorize before EWMA update; track spread via EWMA of abs-deviation.
      - Missing nights: carry state forward (skip-and-hold), no decay.

    Parameters
    ----------
    state :
        Previous ``BaselineState``, or ``None`` for the very first night.
    value :
        Tonight's nightly value, or ``None`` / missing.
    cfg :
        Metric configuration (bounds, floor spread, half-lives).

    Returns
    -------
    BaselineState
        Updated (or carried-forward) state.
    """
    λ_b = _lambda(cfg.half_life_b)
    λ_s = _lambda(cfg.half_life_s)

    # ── First night ever: seed state ──────────────────────────────────────────
    if state is None:
        if value is None or not (cfg.min_val <= value <= cfg.max_val):
            # No valid first value — return a minimal "calibrating" placeholder.
            # Use midpoint of physiological range as a seed until we have data.
            seed = (cfg.min_val + cfg.max_val) / 2.0
            return BaselineState(
                baseline=seed,
                spread=cfg.floor_spread,
                n_valid=0,
                nights_since_update=1,
                status="calibrating",
            )
        return BaselineState(
            baseline=value,
            spread=cfg.floor_spread,
            n_valid=1,
            nights_since_update=0,
            status="calibrating",
        )

    # ── Missing night: skip-and-hold ─────────────────────────────────────────
    if value is None:
        new_missing = state.nights_since_update + 1
        return BaselineState(
            baseline=state.baseline,
            spread=state.spread,
            n_valid=state.n_valid,
            nights_since_update=new_missing,
            status=_compute_status(state.n_valid, new_missing),
        )

    # ── Step 0: sanity gate ───────────────────────────────────────────────────
    if not (cfg.min_val <= value <= cfg.max_val):
        # Physiologically implausible — skip-and-hold (count as missing).
        new_missing = state.nights_since_update + 1
        return BaselineState(
            baseline=state.baseline,
            spread=state.spread,
            n_valid=state.n_valid,
            nights_since_update=new_missing,
            status=_compute_status(state.n_valid, new_missing),
        )

    # Hard outlier rejection: if deviation > HARD_OUTLIER_K * spread, skip state
    # update but still count nights_since_update as 0 (we *saw* a night, just a
    # suspicious one). Don't carry-forward as "missing".
    if state.n_valid >= MIN_NIGHTS_SEED:
        dev = abs(value - state.baseline)
        if dev > HARD_OUTLIER_K * state.spread:
            # Don't update baseline/spread; reset nights_since_update to 0
            # (we had a night, just an outlier one).
            return BaselineState(
                baseline=state.baseline,
                spread=state.spread,
                n_valid=state.n_valid,
                nights_since_update=0,
                status=_compute_status(state.n_valid, 0),
            )

    # ── Step 1: Winsorized EWMA update ────────────────────────────────────────
    # Special case: if we have no real measurements yet (n_valid == 0), the current
    # state is only a None-placeholder seeded at the physiological midpoint.  Treat
    # the first real value as a clean first-night seed — do NOT Winsorize it toward
    # the midpoint, which would permanently corrupt the baseline.
    if state.n_valid == 0:
        return BaselineState(
            baseline=value,
            spread=cfg.floor_spread,
            n_valid=1,
            nights_since_update=0,
            status="calibrating",
        )

    # Clamp value to ±WINSOR_K * spread around current baseline before folding in.
    lo = state.baseline - WINSOR_K * state.spread
    hi = state.baseline + WINSOR_K * state.spread
    clamped = max(lo, min(hi, value))

    new_baseline = λ_b * clamped + (1.0 - λ_b) * state.baseline

    # Use UNCLAMPED value for the spread update (so true deviations are tracked).
    abs_dev = abs(value - new_baseline)
    new_spread = max(cfg.floor_spread, λ_s * abs_dev + (1.0 - λ_s) * state.spread)

    new_n_valid = state.n_valid + 1

    return BaselineState(
        baseline=new_baseline,
        spread=new_spread,
        n_valid=new_n_valid,
        nights_since_update=0,
        status=_compute_status(new_n_valid, 0),
    )


# ===========================================================================
# Replay helper
# ===========================================================================

def fold_history(
    values: Sequence[Optional[float]],
    cfg: MetricCfg,
) -> BaselineState:
    """Replay an ordered sequence of nightly values (oldest first) to build state.

    Parameters
    ----------
    values :
        Ordered list of nightly values (oldest → newest).  ``None`` entries are
        treated as missing nights (skip-and-hold).
    cfg :
        Metric configuration.

    Returns
    -------
    BaselineState
        The final baseline state after all nights have been processed.
    """
    state: Optional[BaselineState] = None
    for v in values:
        state = update_baseline(state, v, cfg)
    if state is None:
        # Empty sequence — return a zero-data calibrating state with seeded spread.
        seed = (cfg.min_val + cfg.max_val) / 2.0
        return BaselineState(
            baseline=seed,
            spread=cfg.floor_spread,
            n_valid=0,
            nights_since_update=0,
            status="calibrating",
        )
    return state


# ===========================================================================
# Deviation helper
# ===========================================================================

def deviation(value: float, state: BaselineState) -> Deviation:
    """Compute the three deviation forms from doc 06 §1.4.

    Parameters
    ----------
    value :
        Tonight's nightly value.
    state :
        Current baseline state.

    Returns
    -------
    Deviation
        z (robust z-score), delta (signed physical-units delta), ratio (fractional),
        and in_normal_range (|z| ≤ 1.0).

    Notes
    -----
    The robust z is: ``(value - baseline) / (1.253 * spread)``.
    The factor 1.253 converts the EWMA-of-abs-deviation ("MAD-like" spread) to
    an approximate Gaussian σ, since E[|X-μ|] = σ·√(2/π) ≈ σ/1.253 for normal X.
    """
    sigma_approx = 1.253 * state.spread  # convert spread to approx Gaussian σ
    sigma_approx = max(sigma_approx, 1e-9)  # guard against zero spread

    z = (value - state.baseline) / sigma_approx
    delta = value - state.baseline
    ratio = (value / state.baseline - 1.0) if state.baseline != 0.0 else 0.0

    return Deviation(
        z=z,
        delta=delta,
        ratio=ratio,
        in_normal_range=abs(z) <= 1.0,
    )


# ===========================================================================
# Trimmed-mean alternative (auditability / cross-check)
# ===========================================================================

def trimmed_mean_baseline(
    values: Sequence[Optional[float]],
    cfg: MetricCfg,
    trim_fraction: float = 0.20,
) -> BaselineState:
    """Compute a 20%-trimmed-mean baseline from the trailing valid values.

    This is the simpler, no-state alternative from doc 03 §2.2.  It has no
    recency weighting (a night 29 days ago counts the same as last night) but is
    maximally robust (50% breakdown point at the median, ~20% trim here).

    Useful for auditability, cross-checking the EWMA, or cold-start seeding before
    there are enough nights for a robust EWMA.  The EWMA path is preferred in
    production (more responsive to genuine fitness change).

    Parameters
    ----------
    values :
        Ordered list of nightly values (oldest → newest).  ``None`` entries dropped.
    cfg :
        Metric configuration (bounds, floor_spread).
    trim_fraction :
        Fraction to trim from each tail (default 0.20 → 20%).

    Returns
    -------
    BaselineState
        Baseline state with the trimmed mean as center, MAD-based spread, and
        appropriate status flag.
    """
    # Filter out None and physiologically implausible values.
    valid = [v for v in values if v is not None and cfg.min_val <= v <= cfg.max_val]
    n = len(valid)

    if n == 0:
        seed = (cfg.min_val + cfg.max_val) / 2.0
        return BaselineState(
            baseline=seed,
            spread=cfg.floor_spread,
            n_valid=0,
            nights_since_update=0,
            status="calibrating",
        )

    sorted_vals = sorted(valid)
    k = max(0, round(n * trim_fraction))
    trimmed = sorted_vals[k: n - k] if n - k > k else sorted_vals  # guard very short lists

    center = statistics.fmean(trimmed)

    # MAD-based spread (robust scale estimator, doc 03 §2.2).
    med = statistics.median(valid)
    mad = statistics.median([abs(v - med) for v in valid])
    sigma_mad = 1.4826 * mad  # scale to Gaussian-σ-equivalent

    # Apply the σ_floor in σ-space first, then convert to the internal abs-dev
    # space that deviation() expects (it multiplies by 1.253 to recover σ).
    # We must NOT re-apply the floor after dividing: that would clamp
    # spread_internal to floor_spread and make deviation() return 1.253*floor
    # instead of the intended floor.
    sigma_floored = max(cfg.floor_spread, sigma_mad)
    spread_internal = sigma_floored / 1.253

    status = _compute_status(n, 0)
    return BaselineState(
        baseline=center,
        spread=spread_internal,
        n_valid=n,
        nights_since_update=0,
        status=status,
    )
