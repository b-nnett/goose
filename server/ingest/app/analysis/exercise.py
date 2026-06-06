"""
exercise.py — Retroactive workout/exercise detection from the 1 Hz store.

A workout is detected as a SUSTAINED window (>= ``MIN_EXERCISE_MIN`` minutes) of
**elevated heart rate** (above resting + ``HR_MARGIN_BPM``) AND **sustained
motion** (the gravity-derived ``activity_series`` intensity above
``MOTION_THRESHOLD``). Both gates must hold for a sample to count as
"active-exercise".

Because this runs over the backfilled 1 Hz biometric store, it works
RETROACTIVELY and needs NO raw accelerometer data: a run done while the phone
was disconnected from the strap is still detected on the next sync, once the
type-47 historical store has been offloaded and decoded.

------------------------------------------------------------------------------
Algorithm
------------------------------------------------------------------------------
1. Compute ``activity_series(streams["gravity"])`` → ``[{ts, intensity}]`` (the
   per-record gravity change-magnitude movement proxy — see ``activity.py``).
2. Resting-HR baseline (the HR floor for the day): if ``resting_hr`` is not
   provided, derive it from the HR stream as a LOW PERCENTILE
   (``RESTING_PERCENTILE``, the 10th) of the day's bpm values. The 10th
   percentile is a robust day-resting proxy that does not require a sleep
   session to be passed in, and is insensitive to the elevated samples during
   the workout itself.
3. HRmax: if ``max_hr`` is not provided, derive via ``strain.estimate_hrmax``
   from the day's HR values (falls back through Tanaka → 220-age as needed).
   The personalized HRmax is used for Karvonen zone classification and per-bout
   strain. ``hrmax_source`` on the returned session records which method won.
4. Alignment: HR and gravity are independent 1 Hz streams that may not share
   exact timestamps. We align by NEAREST timestamp — for each gravity sample we
   bind the closest HR sample within ``ALIGN_TOLERANCE_S``; for each HR sample
   we bind the closest motion intensity within the same tolerance. We then walk
   the GRAVITY timeline (motion is the scarcer/decisive signal) and mark a
   sample "active-exercise" when its (nearest) HR > resting + ``HR_MARGIN_BPM``
   AND a short rolling-mean of intensity (``MOTION_SMOOTH_S`` window) is above
   ``MOTION_THRESHOLD``. The rolling mean rejects single-spike noise.
5. Group contiguous active-exercise samples into runs; merge two runs separated
   by a gap shorter than ``MERGE_GAP_S`` (a short-bout-bridging rule so brief
   lulls within a session don't split it). Keep only runs whose duration >=
   ``MIN_EXERCISE_MIN`` minutes.
5a. Intensity qualification: discard any bout whose zone-2+ fraction (time in
   Edwards zones 2–5, i.e. ≥60% HRR) is below ``MIN_INTENSITY_Z2PLUS``. This
   rejects low-intensity blips that pass the HR-floor and motion gates but are
   dominated by zone 0/1 activity. Guard: skipped when HRmax is unknown (zone
   data unavailable) so a real workout is never silently suppressed.
6. Per surviving run, build an ``ExerciseSession`` from the HR samples whose ts
   falls in [start, end]:
     - ``avg_hr`` = mean bpm, ``peak_hr`` = max bpm,
     - ``duration_s`` = end − start (seconds),
     - ``strain`` = ``strain(window_hr, max_hr, resting_hr)`` — returns ``None``
       when the window has < ``strain.MIN_READINGS`` (600) samples; that's fine
       and surfaced as ``strain=None``,
     - ``zone_time_pct`` = Edwards zone (0–5) breakdown: percentage of HR
       samples in each zone.  Always sums to 100.  APPROXIMATE (Karvonen %HRR
       with personalized HRmax + RHR).
     - ``avg_hrr_pct`` = mean Karvonen %HRR over the bout window, clamped [0,100].
     - ``hrmax`` = effective HRmax used for zone math (bpm).
     - ``hrmax_source`` = one of ``"observed"``, ``"tanaka"``, ``"caller"``, or
       ``"unknown"`` — records which estimation path was taken.
     - ``kind`` = ``None``: classification (running / cycling / lifting …) and
       step COUNT require the on-demand RAW accelerometer sample (Task 1.4) —
       the decoded 1 Hz gravity store cannot recover stride/cadence. This is
       documented as a deliberate deferral.

------------------------------------------------------------------------------
Robustness
------------------------------------------------------------------------------
- Empty / missing ``hr``         → ``[]`` (HR is required to detect exercise).
- Empty / missing ``gravity``    → ``[]`` (no motion signal → cannot confirm a
  workout; documented — HR-only streams return ``[]``).
- ``None`` bpm / non-finite intensity samples are skipped.
- Invalid HRR (max_hr <= resting_hr after estimation): bout strain = None,
  zone_time_pct = {}, avg_hrr_pct = None.

------------------------------------------------------------------------------
DB persistence note (integration task)
------------------------------------------------------------------------------
``ExerciseSession`` carries new fields beyond the current ``exercise_sessions``
table (start_ts, end_ts, avg_hr, peak_hr, strain, kind):

  - ``duration_s``     → new INTEGER column.
  - ``zone_time_pct``  → new JSONB column (dict {0:pct, 1:pct, … 5:pct}).
  - ``avg_hrr_pct``    → new REAL column.
  - ``hrmax``          → new REAL column.
  - ``hrmax_source``   → new TEXT column.

The integration task (daily.py wiring + schema migration) is responsible for
persisting these.  Omitting them from the INSERT is backward-safe today.

This detector is an independent design built on published exercise-physiology
primitives (Karvonen %HRR zones, Edwards/Banister TRIMP via ``strain.py``); all
intensity outputs are APPROXIMATE and not medical advice.

References:
  - .activity (activity_series — accelerometer change-magnitude motion proxy)
  - .strain   (Edwards TRIMP / HRR → 0–21; MIN_READINGS guard)
  - .strain   (estimate_hrmax — personalized HRmax from trailing HR history)
  - Karvonen et al. 1957 (%HRR); Edwards 1993 (5-zone TRIMP)
"""
from __future__ import annotations

import bisect
import math
import statistics
from dataclasses import dataclass, field
from typing import Optional, Sequence

from ._utils import to_epoch as _to_epoch
from .activity import activity_series
from .calories import estimate_bout_calories as _estimate_bout_calories
from .strain import (
    _pct_hrr,
    _zone_weight,
    estimate_hrmax,
    strain as _strain,
)

# ===========================================================================
# Named thresholds  (all tunable knobs live here)
# ===========================================================================

#: Minimum workout duration (minutes). ~5 min sustained, per the mega-plan.
MIN_EXERCISE_MIN: float = 5.0

#: HR must exceed (resting_hr + this) to count as elevated. Keeps everyday
#: posture/HR fluctuation out; ~15 bpm over resting is a clear effort signal.
#: A 15 bpm margin is high enough to reject postural/stress elevations (which
#: rarely exceed 10 bpm over true resting) and low enough to catch a warm-up
#: before the HR stabilises at a higher steady state.
HR_MARGIN_BPM: float = 15.0

#: Gravity change-magnitude intensity above which (after smoothing) a sample is
#: "moving". A still wrist sits near 0; the movement proxy puts real activity
#: well above this. 0.20 g-units per second is conservative: steady walking
#: registers ~0.4–0.8, running ~1.0+; fidget/typing stays below 0.1.
MOTION_THRESHOLD: float = 0.20

#: Rolling-mean window (seconds) applied to the motion intensity before the
#: gate, to reject single-spike noise. 10 s is long enough to absorb 2–3
#: isolated large spikes (e.g. bumping the desk) without masking real motion.
MOTION_SMOOTH_S: float = 10.0

#: Two active runs separated by a gap shorter than this (seconds) are merged
#: into one session (brief lulls — e.g. a water break — don't split a workout).
#: 150 s (2.5 min) bridges brief mid-bout HR dips (treadmill ~2-min cool-down
#: lulls) without fusing distinct bouts from different parts of the day.
#:
#: Why NOT 420 s (7 min):  On 2026-05-27 the soccer evening had ~6 min of calm
#: walking between the two halves, but the surrounding light activity was also
#: above the HR floor — so a 420 s window bridged everything from 03:43→06:31
#: UTC into a single 168-min "workout" (avg116 bpm, z2+ diluted to 51.5%).
#: That was wrong; the user played soccer for ~1 hr, not 2.8 hr.
#:
#: Root cause: no single merge window can bridge the ~6-min soccer halftime
#: gap WITHOUT also gluing unrelated light activity when the evening HR is
#: nearly continuous above the floor.  Showing soccer as 2 separate high-
#: intensity bouts is more accurate than one diluted 2.8-hr blob.
#:
#: 150 s keeps the treadmill case (2-min gap = 120 s < 150 s → merges) and
#: does NOT bridge a 6-min gap between soccer halves (360 s > 150 s → stays 2).
MERGE_GAP_S: float = 150.0

#: Minimum fraction of a bout's HR samples that must fall in zone 2 or above
#: (i.e. ≥60% HRR) for the bout to be classified as a workout.  Below this,
#: the bout is dominated by zone 0/1 (easy walking or daily activity) and is
#: rejected as noise.
#:
#: Empirical calibration (ground-truth data, 2026-05-25/26):
#:   - Every REAL workout: z2+ = 66–100% (worst case: treadmill warmup at 66%)
#:   - Every noise bout:   z2+ ≤ 43%     (best case: avg92 bpm blip at 43%)
#:   - Gap between classes: 66% − 43% = 23 percentage points
#: Threshold of 0.50 (50%) sits cleanly in the middle of that gap.
#:
#: Applied ONLY when zone data can be computed (requires a valid HRmax estimate).
#: Bouts where HRmax is unknown pass through unfiltered so we never suppress a
#: real workout merely because the zone math was unavailable.
MIN_INTENSITY_Z2PLUS: float = 0.50

#: Nearest-ts alignment tolerance (seconds) when binding HR to gravity (and
#: vice-versa). At 1 Hz, samples within this are treated as coincident. 5 s
#: covers firmware clock skew and BLE delivery jitter without mispairing samples
#: from adjacent minutes.
ALIGN_TOLERANCE_S: float = 5.0

#: Percentile (0–100) of the day's bpm used as the resting-HR baseline when the
#: caller does not provide one. The 10th percentile is a robust day-resting
#: floor that ignores the elevated workout samples (which pull the median up)
#: while being less noisy than the absolute minimum (which can capture artefacts).
RESTING_PERCENTILE: float = 10.0


# ===========================================================================
# Result type
# ===========================================================================

@dataclass
class ExerciseSession:
    """A detected workout window.  All intensity fields are APPROXIMATE.

    Core fields (persisted in the current ``exercise_sessions`` DB table):
        start:    window start (unix epoch seconds).
        end:      window end (unix epoch seconds).
        avg_hr:   mean bpm over the window.
        peak_hr:  max bpm over the window (int).
        strain:   WHOOP-like 0–21 strain for the window, or ``None`` when the
                  window has fewer than ``strain.MIN_READINGS`` samples (600).
        kind:     workout classification (e.g. "run"); always ``None`` here —
                  classification and step COUNT require the on-demand raw-accel
                  sample (Task 1.4), not the decoded 1 Hz gravity store.  A
                  future raw-accel classifier (Task 1.4) can fill this field.

    Extended fields (not yet in the DB schema — integration task must migrate):
        duration_s:    bout duration in seconds (end − start).
        zone_time_pct: Edwards zone (0–5) time breakdown as % of HR samples.
                       Always sums to 100 when HR samples exist.  APPROXIMATE.
        avg_hrr_pct:   Mean Karvonen %HRR over the bout, clamped [0, 100].
                       ``None`` when HRR is invalid (max_hr <= resting_hr).
        hrmax:         Effective HRmax used for zone math (bpm).  ``None`` when
                       HRmax could not be estimated.
        hrmax_source:  How HRmax was determined: ``"caller"`` (passed in),
                       ``"observed"`` (p99.5 of day HR), ``"tanaka"``
                       (208−0.7×age formula), or ``"unknown"`` (no data).
    """

    # --- core fields (DB-persisted today) ---
    start: float
    end: float
    avg_hr: float
    peak_hr: int
    strain: Optional[float] = None
    kind: Optional[str] = None

    # --- extended per-bout intensity fields (integration task must persist) ---
    duration_s: float = field(default=0.0)
    zone_time_pct: dict[int, float] = field(default_factory=dict)
    avg_hrr_pct: Optional[float] = None
    hrmax: Optional[float] = None
    hrmax_source: str = "unknown"

    # --- calorie estimation (requires profile; None when no profile supplied) ---
    calories_kcal: Optional[float] = None
    calories_kj: Optional[float] = None


# ===========================================================================
# Helpers
# ===========================================================================

def _clean_hr(hr_rows: Sequence[dict]) -> list[tuple[float, float]]:
    """Sorted ``[(ts, bpm), ...]`` with None bpm dropped."""
    seg: list[tuple[float, float]] = []
    for r in hr_rows or []:
        bpm = r.get("bpm")
        if bpm is None:
            continue
        seg.append((_to_epoch(r["ts"]), float(bpm)))
    seg.sort(key=lambda t: t[0])
    return seg


def _derive_resting_hr(hr_seg: Sequence[tuple[float, float]]) -> float:
    """Day resting-HR baseline = ``RESTING_PERCENTILE`` of bpm values."""
    bpms = sorted(v for _, v in hr_seg)
    if not bpms:
        raise ValueError("_derive_resting_hr called with empty hr segment")
    # Nearest-rank percentile (robust, no interpolation surprises at small n).
    rank = max(1, math.ceil(RESTING_PERCENTILE / 100.0 * len(bpms)))
    return bpms[rank - 1]


def _nearest(sorted_ts: Sequence[float], values: Sequence[float],
             ts: float, tol: float) -> float | None:
    """Value whose ts is nearest to ``ts`` within ``tol`` seconds, else None.

    Ties (equidistant candidates) resolve to the LATER timestamp (the ``<=``
    comparison keeps updating ``best_v``), deterministic and harmless at 1 Hz.
    """
    if not sorted_ts:
        return None
    i = bisect.bisect_left(sorted_ts, ts)
    best_v: float | None = None
    best_d = tol
    for j in (i - 1, i):
        if 0 <= j < len(sorted_ts):
            d = abs(sorted_ts[j] - ts)
            if d <= best_d:
                best_d = d
                best_v = values[j]
    return best_v


def _smoothed_intensity(motion: Sequence[dict], window_s: float) -> list[float]:
    """Trailing rolling mean (over ``window_s``) of finite intensities.

    Assumes ``motion`` is sorted ascending by ts (``activity_series`` always
    satisfies this).

    Non-finite intensities (sensor dropout → inf) are treated as 0.0 inside the
    mean so a dropout doesn't fabricate motion; the mean is over real samples.

    Note: the trailing window induces ~one-window onset latency — the rolling
    mean is diluted by preceding still samples for ~``MOTION_SMOOTH_S`` seconds
    at workout onset, so the effective minimum detectable workout is slightly
    longer than ``MIN_EXERCISE_MIN``.
    """
    ts = [p["ts"] for p in motion]
    raw = [p["intensity"] if math.isfinite(p["intensity"]) else 0.0
           for p in motion]
    out: list[float] = []
    lo = 0
    for i in range(len(motion)):
        while ts[i] - ts[lo] > window_s:
            lo += 1
        out.append(statistics.fmean(raw[lo:i + 1]))
    return out


def _bout_intensity(
    hr_series: Sequence[dict],
    resting_hr: float,
    max_hr: float,
) -> tuple[dict[int, float], Optional[float]]:
    """Compute per-bout Edwards zone breakdown and mean %HRR.  APPROXIMATE.

    Uses the same Karvonen %HRR and zone-weight machinery as ``strain.py``
    (imported directly; not re-implemented).

    Parameters
    ----------
    hr_series :
        ``[{"ts": float, "bpm": float}, ...]`` for the bout window.
    resting_hr :
        Resting HR (bpm) — the HRR denominator baseline.
    max_hr :
        Effective HRmax (bpm).  Must be > resting_hr; caller guards.

    Returns
    -------
    (zone_time_pct, avg_hrr_pct) :
        ``zone_time_pct`` — ``{0: pct, 1: pct, …, 5: pct}`` summing to 100.0.
            Empty dict when ``hr_series`` is empty.
        ``avg_hrr_pct`` — mean Karvonen %HRR in [0, 100], or ``None`` when
            ``hr_series`` is empty or HRR is invalid.
    """
    if not hr_series or max_hr <= resting_hr:
        return {}, None

    hr_reserve = max_hr - resting_hr
    zone_counts: dict[int, int] = {z: 0 for z in range(6)}
    pct_hrr_vals: list[float] = []

    for r in hr_series:
        bpm = float(r["bpm"])
        zone_counts[_zone_weight(bpm, resting_hr, hr_reserve)] += 1
        pct_hrr_vals.append(_pct_hrr(bpm, resting_hr, hr_reserve))

    n = len(hr_series)
    zone_time_pct = {z: round(cnt / n * 100.0, 1) for z, cnt in zone_counts.items()}
    avg_hrr_pct = round(statistics.fmean(pct_hrr_vals), 1)
    return zone_time_pct, avg_hrr_pct


# ===========================================================================
# Public API
# ===========================================================================

def detect_exercises(
    streams: dict[str, list[dict]],
    *,
    resting_hr: Optional[float] = None,
    max_hr: Optional[float] = None,
    age: Optional[float] = None,
    profile: Optional[dict] = None,
) -> list[ExerciseSession]:
    """Detect workouts from the 1 Hz HR + gravity store. See module docstring.

    Args:
        streams:    ``{"hr": [{ts,bpm}], "gravity": [{ts,x,y,z}], ...}``.
        resting_hr: day resting-HR baseline (bpm). If ``None``, derived as the
                    ``RESTING_PERCENTILE`` of the day's HR. **Caveat**: the
                    auto-derived 10th-percentile baseline is accurate only when
                    the stream contains meaningful REST-period data. A
                    workout-only stream pushes the percentile up, raising the HR
                    floor and suppressing detection. Callers with workout-only
                    streams — and the Task 2.5 orchestrator — should pass
                    ``resting_hr`` explicitly (from the recovery module).
        max_hr:     HRmax (bpm) for zone classification and strain.  If ``None``,
                    derived automatically via ``strain.estimate_hrmax`` from the
                    day's HR values (falls through Tanaka → unknown as needed).
                    Pass explicitly to override auto-estimation (e.g. from a
                    prior peak-effort session or user profile).
        age:        User age in years.  Used only when ``max_hr`` is not passed
                    and there is insufficient HR history for the observed-p99.5
                    estimate; feeds the Tanaka formula (208 − 0.7 × age).
        profile:    Optional user profile dict with keys ``weight_kg``,
                    ``height_cm``, ``age``, ``sex``. When provided, calorie
                    estimation (WHOOP/Keytel formula) is run for each detected
                    bout and ``calories_kcal``/``calories_kj`` are populated.
                    When ``None`` (the default), calories stay ``None`` and ALL
                    existing detection behavior is unchanged.

    Returns:
        Time-ordered ``list[ExerciseSession]``; ``[]`` when there is no HR, no
        gravity, or no qualifying window.
    """
    hr_seg = _clean_hr(streams.get("hr") or [])
    motion = activity_series(streams.get("gravity") or [])
    if not hr_seg or not motion:
        return []

    if resting_hr is None:
        resting_hr = _derive_resting_hr(hr_seg)
    hr_floor = float(resting_hr) + HR_MARGIN_BPM

    # Determine effective HRmax and record its provenance.
    if max_hr is not None:
        eff_max_hr: Optional[float] = float(max_hr)
        hrmax_source = "caller"
    else:
        day_bpms = [v for _, v in hr_seg]
        eff_max_hr, hrmax_source = estimate_hrmax(day_bpms, age=age)
        if eff_max_hr == 0.0:
            # estimate_hrmax returns (0.0, "unknown") when it cannot estimate.
            eff_max_hr = None

    hr_ts = [t for t, _ in hr_seg]
    hr_bpm = [v for _, v in hr_seg]

    smooth = _smoothed_intensity(motion, MOTION_SMOOTH_S)

    # Walk the gravity timeline; flag samples where BOTH gates hold.
    active_ts: list[float] = []
    for p, inten in zip(motion, smooth):
        if inten <= MOTION_THRESHOLD:
            continue
        bpm = _nearest(hr_ts, hr_bpm, p["ts"], ALIGN_TOLERANCE_S)
        if bpm is None or bpm <= hr_floor:
            continue
        active_ts.append(p["ts"])

    if not active_ts:
        return []

    # Group contiguous active samples into runs, merging gaps < MERGE_GAP_S.
    runs: list[tuple[float, float]] = []
    run_start = active_ts[0]
    prev = active_ts[0]
    for ts in active_ts[1:]:
        if ts - prev > MERGE_GAP_S:
            runs.append((run_start, prev))
            run_start = ts
        prev = ts
    runs.append((run_start, prev))

    # Keep only runs >= MIN_EXERCISE_MIN; build a session for each.
    min_dur_s = MIN_EXERCISE_MIN * 60.0
    sessions: list[ExerciseSession] = []
    for start, end in runs:
        # end - start measures the span of ACTIVE gravity samples, not
        # wall-clock.  The rolling-mean window (MOTION_SMOOTH_S) dilutes
        # intensity at bout onset, so the first active sample arrives ~
        # MOTION_SMOOTH_S seconds after the real start — trimming up to that
        # much from the measured span.  We apply a tolerance equal to the
        # smoothing window so that a genuinely MIN_EXERCISE_MIN-long workout
        # (whose active span is slightly shorter due to onset latency) is
        # still accepted.
        if end - start < min_dur_s - MOTION_SMOOTH_S:
            continue
        window = [(t, v) for t, v in hr_seg if start <= t <= end]
        if not window:
            continue
        bpms = [v for _, v in window]
        hr_series = [{"ts": t, "bpm": v} for t, v in window]

        # Per-bout intensity: zone breakdown + mean %HRR.
        if eff_max_hr is not None and eff_max_hr > resting_hr:
            zone_pct, avg_hrr = _bout_intensity(hr_series, resting_hr, eff_max_hr)
        else:
            zone_pct, avg_hrr = {}, None

        # Intensity qualification filter: require at least MIN_INTENSITY_Z2PLUS
        # of the bout's time in zone 2 or above (≥60% HRR).  This rejects
        # low-intensity blips (zone-0-dominated daily activity, easy walking)
        # that pass the HR-floor and motion gates but are not workouts.
        #
        # Guard: only apply when zone_pct is populated (requires a valid HRmax).
        # If zone_pct is empty (unknown HRmax), pass the bout through — we must
        # not suppress genuine workouts merely because the zone math failed.
        if zone_pct:
            z2plus_frac = sum(zone_pct.get(z, 0.0) for z in (2, 3, 4, 5)) / 100.0
            if z2plus_frac < MIN_INTENSITY_Z2PLUS:
                continue

        # Calorie estimation — only when a profile is supplied; otherwise None.
        calories_kcal: Optional[float] = None
        calories_kj: Optional[float] = None
        if profile is not None:
            calories_kcal, calories_kj = _estimate_bout_calories(
                hr_series,
                profile=profile,
                hrmax=eff_max_hr,
                resting_hr=resting_hr,
            )

        sessions.append(
            ExerciseSession(
                start=start,
                end=end,
                avg_hr=statistics.fmean(bpms),
                peak_hr=int(round(max(bpms))),
                strain=_strain(
                    hr_series,
                    max_hr=eff_max_hr,
                    resting_hr=resting_hr,
                ),
                kind=None,
                duration_s=end - start,
                zone_time_pct=zone_pct,
                avg_hrr_pct=avg_hrr,
                hrmax=eff_max_hr,
                hrmax_source=hrmax_source,
                calories_kcal=calories_kcal,
                calories_kj=calories_kj,
            )
        )
    return sessions
