"""
sleep.py — Sleep/wake detection, multi-signal sleep staging (APPROXIMATE), and
the daily sleep summary metric. Public entry point for the sleep pipeline.

Multi-signal staging (wake / light / deep / REM) is built on a 30 s epoch grid:

  Stage 0  IN-BED / SLEEP-WAKE SPINE — a rolling accelerometer-stillness
           classifier locates the main sleep period (the wrist is nearly
           motionless while asleep). Cole–Kripke (te Lindert 30 s variant) is
           computed as a citable sleep/wake cross-check, and an HR check confirms
           each candidate run.
  Stage 1  CARDIORESPIRATORY FEATURES per 30 s epoch over a rolling 5-min window
           (HR, Walch DoG HR-variability, neurokit2 HRV RMSSD/SDNN/HF/LFHF,
           respiration rate + RRV, clock proxy).  → app/analysis/sleep_features.py
  Stage 2  TRANSPARENT STAGING CLASSIFIER (``sleep_features.classify_epochs`` — the
           model seam; a learned model can replace it later).
  Stage 3  SMOOTHING (kills isolated 30 s flips) + PHYSIOLOGY re-imposition (no REM
           in the first ~15 min after onset; deep concentrated in the first third).

Then AASM-style metrics are computed from the per-epoch hypnogram: TST, Sleep
Efficiency (TST/TIB), Sleep Latency (SOL), REM latency, WASO, disturbances
(post-onset wake runs), and stage percentages.

HONEST HEDGING
--------------
These stages are APPROXIMATIONS — not PSG-validated and not medical advice. The
EEG-free 4-class ceiling is roughly 65–73 % epoch agreement (Walch et al. 2019).
**Light/deep separation is the weakest link** — the cardiac signal barely
separates N1/N2/N3, so deep-minute estimates are the least reliable output. The
output is validated for physiological plausibility + internal consistency only.

------------------------------------------------------------------------------
Input data shape  (matches the read API's ``query_stream`` output)
------------------------------------------------------------------------------
``streams: dict[str, list[dict]]`` — any key may be empty or absent:

  "hr":        [{"ts": <unix seconds>, "bpm": int}, ...]
  "rr":        [{"ts": <unix seconds>, "rr_ms": int}, ...]
  "resp":      [{"ts": <unix seconds>, "raw": int}, ...]
  "gravity":   [{"ts": <unix seconds>, "x": float, "y": float, "z": float}, ...]
  "skin_temp": [{"ts": <unix seconds>, "raw": int}, ...]  (ACCEPTED, currently IGNORED)

``ts`` may be a float/int (epoch seconds) or a ``datetime``; both coerced to
epoch seconds. Streams are sorted defensively.

------------------------------------------------------------------------------
Output shapes (UNCHANGED — daily.py / hrv.nightly_hrv / dashboard depend on these)
------------------------------------------------------------------------------
``SleepSession`` (dataclass): start, end, efficiency (asleep/in-bed 0..1),
    stages: list[StageSegment], resting_hr, avg_hrv.
``StageSegment`` (dataclass): start, end, stage ∈ {"wake","light","deep","rem"}.
``daily_sleep_summary(...)`` dict keys: date, total_sleep_min, efficiency,
    deep_min, rem_min, light_min, disturbances, resting_hr, avg_hrv,
    sleep_start, sleep_end.

References (primary published methods):
  - te Lindert, B.H.W. & Van Someren, E.J.W. (2013). "Sleep estimates using MEMS."
    *Sleep*, 36(5), 781–789.  (accelerometer-difference movement proxy; 30 s
    activity counts feeding Cole–Kripke.)
  - Cole, R.J. et al. (1992). "Automatic sleep/wake identification from wrist
    activity." *Sleep*, 15(5), 461–469.  (Cole–Kripke sleep/wake.)
  - Walch, O. et al. (2019). "Sleep stage prediction with raw acceleration and
    photoplethysmography." *Sleep*, 42(12), zsz180.  (DoG HR-variability feature.)
  - Berry, R.B. et al. (2017). *The AASM Manual for the Scoring of Sleep and
    Associated Events*, v2.4.  (sleep-metric definitions.)
  - .hrv (rmssd), app/analysis/sleep_features.py (epoch engine + classifier)
"""
from __future__ import annotations

import datetime as _dt
import math
import statistics
from dataclasses import dataclass, field
from typing import Any, Sequence

from ._utils import to_epoch as _to_epoch
from .hrv import rmssd
from . import sleep_features as _sf

# ===========================================================================
# Named thresholds
# ===========================================================================
# These govern the accelerometer-stillness sleep/wake spine (Stage 0). The
# values below are independent empirical choices appropriate to a 1 Hz gravity
# stream and a wrist-worn sensor; they are documented per-constant. They are NOT
# derived from any third-party implementation.

#: Per-sample gravity-vector change (g) at/below which a sample is "still". A
#: motionless wrist holds gravity nearly constant; 0.01 g is comfortably above
#: 1 Hz quantization noise yet well below the change produced by any real stir.
GRAVITY_STILL_THRESHOLD_G: float = 0.01
#: Rolling stillness window (minutes). A quarter-hour smooths over isolated
#: micro-movements while still resolving the boundaries of a sleep period.
STILL_WINDOW_MIN: int = 15
#: Fraction of still samples within the window required to call the centre sleep.
STILL_FRACTION: float = 0.70
#: A data gap larger than this (minutes) always breaks a run — we cannot assume
#: the wrist stayed still across an unobserved gap.
MAX_GAP_MIN: int = 20
#: Runs shorter than this (minutes) are absorbed into their neighbours — a
#: standard short-bout filter that prevents transient flips from fragmenting the
#: night (cf. minimum-bout-length filtering in actigraphy sleep-period detection).
MERGE_MIN: int = 15
#: A sleep run must exceed this duration (minutes) to count as a session.
MIN_SLEEP_MIN: int = 60
#: Sample interval (seconds) assumed when it cannot be inferred from the data.
DEFAULT_INTERVAL_S: float = 60.0
#: Floor on the rolling-window size in samples (keeps the window meaningful when
#: the sample interval is large).
MIN_WINDOW_SAMPLES: int = 3

# --- HR / respiration refinement (mega-plan additive layer) ----------------

#: A "sleep" run is confirmed only if its mean HR is at/below this multiple of
#: the day's HR baseline (median HR). 1.05 = within 5% above median.
HR_SLEEP_BASELINE_MULT: float = 1.05
#: Skip the HR refinement (trust gravity) if a run has fewer HR samples than this.
HR_REFINE_MIN_SAMPLES: int = 30

# --- Sleep-onset persistence (AASM §5) --------------------------------------
#: Require this many consecutive sleep epochs to declare sleep onset (avoids
#: micro-onsets). 3 epochs = 90 s of persistent sleep (AASM-style).
ONSET_PERSIST_EPOCHS: int = 3


# ===========================================================================
# Data shapes  (UNCHANGED public interface)
# ===========================================================================

@dataclass
class StageSegment:
    """A contiguous sleep-stage segment within a session. Times are epoch sec."""
    start: float
    end: float
    stage: str  # "light" | "deep" | "rem" | "wake"


@dataclass
class SleepSession:
    """A detected sleep session (in-bed span) with APPROXIMATE staging.

    efficiency = asleep_seconds / in_bed_seconds, in [0, 1] (AASM TST/TIB), where
    asleep = in-bed minus "wake" stage time.

    resting_hr — lowest 5-min rolling-mean HR during the session (bpm) or None.
    avg_hrv    — mean RMSSD over 5-min windows across the session (ms) or None.
    """
    start: float
    end: float
    efficiency: float
    stages: list[StageSegment] = field(default_factory=list)
    resting_hr: float | None = None
    avg_hrv: float | None = None


# ===========================================================================
# Timestamp coercion / row sorting
# ===========================================================================

def _sorted_rows(rows: Sequence[dict] | None) -> list[dict]:
    if not rows:
        return []
    out = [dict(r) for r in rows]
    for r in out:
        r["ts"] = _to_epoch(r["ts"])
    out.sort(key=lambda r: r["ts"])
    return out


# ===========================================================================
# Accelerometer-stillness sleep/wake spine  — Stage 0
# ===========================================================================

def _xyz(r: dict) -> tuple[float, float, float] | None:
    """Read a numeric gravity triplet from a row; ``None`` on any missing axis."""
    try:
        return (float(r["x"]), float(r["y"]), float(r["z"]))
    except (KeyError, TypeError, ValueError):
        return None


def _gravity_deltas(grav: Sequence[dict]) -> list[float]:
    """Per-record movement proxy = L2 magnitude of the gravity change vs the
    previous record.

    The first record has no predecessor and is assigned 0.0. A record with any
    missing/non-numeric axis is treated as a maximal (infinite) movement, so a
    dropout is never mistaken for stillness.
    """
    deltas: list[float] = []
    previous: tuple[float, float, float] | None = None
    for position, row in enumerate(grav):
        current = _xyz(row)
        if position == 0:
            deltas.append(0.0)
        elif current is None or previous is None:
            deltas.append(math.inf)
        else:
            deltas.append(
                math.hypot(
                    previous[0] - current[0],
                    previous[1] - current[1],
                    previous[2] - current[2],
                )
            )
        previous = current
    return deltas


def _median_interval_s(times: Sequence[float]) -> float:
    """Median spacing between consecutive timestamps, restricted to plausible
    (0, 300 s) gaps. Falls back to ``DEFAULT_INTERVAL_S`` and never returns < 1 s.
    """
    gaps = sorted(
        gap for gap in (times[i + 1] - times[i] for i in range(len(times) - 1))
        if 0 < gap < 300
    )
    if not gaps:
        return DEFAULT_INTERVAL_S
    return max(gaps[len(gaps) // 2], 1.0)


def _window_size(times: Sequence[float]) -> int:
    """Centered-window length (in samples) covering ``STILL_WINDOW_MIN`` minutes."""
    interval = _median_interval_s(times)
    return max(MIN_WINDOW_SAMPLES, int((STILL_WINDOW_MIN * 60) / interval))


def _classify_still(grav: Sequence[dict], deltas: Sequence[float]) -> list[bool]:
    """Per-record sleep flags from a rolling fraction of "still" samples.

    Over a centered window, an epoch is flagged as sleep when at least
    ``STILL_FRACTION`` of its samples have a movement proxy at/below the still
    threshold. Returns one bool per record (True == sleep).
    """
    n = len(grav)
    if n < 2:
        return [False] * n

    half = _window_size([r["ts"] for r in grav]) // 2
    flags: list[bool] = []
    for i in range(n):
        lo = max(0, i - half)
        hi = min(n, i + half + 1)
        window = deltas[lo:hi]
        still_count = sum(1 for d in window if d < GRAVITY_STILL_THRESHOLD_G)
        flags.append((still_count / len(window)) >= STILL_FRACTION)
    return flags


def _build_runs(grav: Sequence[dict], flags: Sequence[bool]) -> list[dict]:
    """Collapse the per-record sleep/active flags into contiguous runs.

    A run ends at a class change or whenever the gap to the next sample exceeds
    ``MAX_GAP_MIN`` minutes (an unobserved gap cannot be assumed still). Returns
    ``{"stage": "sleep"|"active", "start": ts, "end": ts}`` periods.
    """
    n = len(grav)
    if n == 0:
        return []

    times = [r["ts"] for r in grav]
    max_gap_s = MAX_GAP_MIN * 60
    periods: list[dict] = []
    run_start = 0

    for i in range(1, n + 1):
        at_end = i == n
        if at_end:
            close = True
        else:
            class_changed = flags[i] != flags[run_start]
            gap_exceeded = (times[i] - times[i - 1]) > max_gap_s
            close = class_changed or gap_exceeded

        if close:
            periods.append({
                "stage": "sleep" if flags[run_start] else "active",
                "start": times[run_start],
                "end": times[i - 1],
            })
            run_start = i

    return periods


def _merge_periods(periods: list[dict], *, merge_min: int = MERGE_MIN) -> list[dict]:
    """Absorb runs shorter than ``merge_min`` minutes into their neighbours.

    Short-bout filter (standard in actigraphy sleep-period detection): a too-short
    run between two same-class runs bridges them into one; a too-short run at the
    head extends into the following run; a too-short run at the tail extends the
    preceding run. Runs at/above the threshold pass through unchanged.
    """
    if not periods:
        return []

    pending = [dict(p) for p in periods]
    threshold_s = merge_min * 60
    merged: list[dict] = []
    i = 0
    while i < len(pending):
        current = pending[i]
        too_short = (current["end"] - current["start"]) < threshold_s

        if not too_short:
            merged.append(current)
            i += 1
            continue

        has_prev = i > 0 and bool(merged)
        has_next = i + 1 < len(pending)
        bridges_same_class = (
            has_prev and has_next
            and pending[i - 1]["stage"] == pending[i + 1]["stage"]
        )

        if bridges_same_class:
            # Fuse prev + current + next into a single run of the shared class.
            prev = merged.pop()
            merged.append({
                "stage": prev["stage"],
                "start": prev["start"],
                "end": pending[i + 1]["end"],
            })
            i += 2  # current and next are consumed
        elif has_next:
            # Extend the next run backward to swallow this short head run.
            pending[i + 1] = {
                "stage": pending[i + 1]["stage"],
                "start": current["start"],
                "end": pending[i + 1]["end"],
            }
            i += 1
        elif has_prev:
            # Trailing short run: extend the preceding run forward.
            prev = merged.pop()
            merged.append({
                "stage": prev["stage"],
                "start": prev["start"],
                "end": current["end"],
            })
            i += 1
        else:
            # A lone too-short run with no neighbours to merge into: it carries no
            # usable sleep/active period and is dropped.
            i += 1

    return merged


# ===========================================================================
# HR / respiration refinement
# ===========================================================================

def _rows_between(rows: Sequence[dict], start: float, end: float) -> list[dict]:
    return [r for r in rows if start <= r["ts"] <= end]


def _hr_baseline(hr: Sequence[dict]) -> float | None:
    """The day's HR baseline = median bpm over all HR samples. None if no HR."""
    vals = [float(r["bpm"]) for r in hr if r.get("bpm") is not None]
    if not vals:
        return None
    return statistics.median(vals)


def _confirm_sleep_with_hr(period: dict, hr: Sequence[dict], baseline: float | None) -> bool:
    """A gravity-detected sleep run is confirmed only if its mean HR is at/below
    ``baseline * HR_SLEEP_BASELINE_MULT``. Too few HR samples / no baseline →
    trust gravity and confirm.
    """
    if baseline is None:
        return True
    seg = _rows_between(hr, period["start"], period["end"])
    if len(seg) < HR_REFINE_MIN_SAMPLES:
        return True
    mean_hr = statistics.fmean(float(r["bpm"]) for r in seg)
    return mean_hr <= baseline * HR_SLEEP_BASELINE_MULT


# ===========================================================================
# detect_sleep  (public)
# ===========================================================================

def detect_sleep(streams: dict[str, list[dict]]) -> list[SleepSession]:
    """Detect sleep sessions from 1 Hz biometric streams.

    Algorithm (see module header for the full Stage 0–3 pipeline):
      1. PRIMARY: rolling accelerometer-stillness classifier → per-record
         sleep/active flags → runs (broken on class change or data gap) →
         short-bout merge into neighbours. Cole–Kripke (te Lindert 30 s) is
         computed per epoch as a citable sleep/wake CROSS-CHECK, but the stillness
         spine remains primary for locating the main sleep period.
      2. REFINEMENT: each merged sleep run > MIN_SLEEP_MIN minutes is HR-confirmed.
      3. STAGING: ``_stage_session`` builds a 30 s epoch hypnogram (multi-signal
         classifier + smoothing + physiology) → StageSegments.
      4. efficiency (asleep / in-bed), resting_hr, avg_hrv computed per session.

    Returns sessions sorted ascending by start. Empty/absent streams → [].
    Gravity-only input degrades gracefully (HR/RR/resp refinements skipped).
    """
    grav = _sorted_rows(streams.get("gravity"))
    if len(grav) < 2:
        return []

    hr = _sorted_rows(streams.get("hr"))
    rr = _sorted_rows(streams.get("rr"))
    resp = _sorted_rows(streams.get("resp"))

    deltas = _gravity_deltas(grav)
    flags = _classify_still(grav, deltas)
    runs = _build_runs(grav, flags)
    runs = _merge_periods(runs, merge_min=MERGE_MIN)

    baseline = _hr_baseline(hr)
    min_sleep_s = MIN_SLEEP_MIN * 60

    sessions: list[SleepSession] = []
    for p in runs:
        if p["stage"] != "sleep":
            continue
        if (p["end"] - p["start"]) <= min_sleep_s:
            continue
        if not _confirm_sleep_with_hr(p, hr, baseline):
            continue
        stages = _stage_session(p["start"], p["end"], grav, deltas, hr, rr, resp)
        eff = _efficiency(p["start"], p["end"], stages)
        resting = _session_resting_hr(p["start"], p["end"], hr)
        avg_hrv = _session_avg_hrv(p["start"], p["end"], rr)
        sessions.append(SleepSession(start=p["start"], end=p["end"],
                                     efficiency=eff, stages=stages,
                                     resting_hr=resting, avg_hrv=avg_hrv))

    sessions.sort(key=lambda s: s.start)
    return sessions


def _efficiency(start: float, end: float, stages: Sequence[StageSegment]) -> float:
    """asleep / in-bed, in [0, 1] (AASM TST/TIB). asleep = in-bed minus 'wake'."""
    in_bed = end - start
    if in_bed <= 0:
        return 0.0
    wake = sum(s.end - s.start for s in stages if s.stage == "wake")
    asleep = max(0.0, in_bed - wake)
    return min(1.0, asleep / in_bed)


# ===========================================================================
# Staging — Stage 1–3 over a 30 s epoch grid  (APPROXIMATE)
# ===========================================================================

def _onset_and_final_wake(ck_flags: Sequence[bool]) -> tuple[int, int]:
    """First persistent-sleep epoch (onset) and last sleep epoch (final wake) from
    the epoch sleep/wake flags. Onset requires ONSET_PERSIST_EPOCHS consecutive
    sleep epochs (avoids micro-onsets). Falls back to (0, n-1) if no clear sleep.
    """
    n = len(ck_flags)
    if n == 0:
        return 0, 0
    onset = None
    run = 0
    for i, s in enumerate(ck_flags):
        run = run + 1 if s else 0
        if run >= ONSET_PERSIST_EPOCHS:
            onset = i - ONSET_PERSIST_EPOCHS + 1
            break
    final = None
    for i in range(n - 1, -1, -1):
        if ck_flags[i]:
            final = i
            break
    if onset is None:
        onset = 0
    if final is None or final < onset:
        final = n - 1
    return onset, final


def _stage_session(
    start: float,
    end: float,
    grav: Sequence[dict],
    deltas: Sequence[float],
    hr: Sequence[dict],
    rr: Sequence[dict],
    resp: Sequence[dict],
) -> list[StageSegment]:
    """Build a 30 s epoch hypnogram for [start, end) and return StageSegments.

    APPROXIMATE — see module header. Steps (research §4):
      Stage 1  build the epoch grid + per-epoch cardiorespiratory features.
      Stage 2  classify each epoch via ``sleep_features.classify_epochs`` (the
               model seam).
      Stage 3  smooth (kill isolated flips) + re-impose physiology (no early REM,
               deep in first third). Epochs before sleep onset / after final wake
               that the classifier called a sleep stage are forced to "wake"
               (pre-onset latency / post-wake are not sleep).
      Finally  merge consecutive same-stage epochs into StageSegments tiling
               [start, end] exactly (snap last segment to ``end``).
    """
    g_seg = _rows_between(grav, start, end)
    if len(g_seg) < 2:
        return [StageSegment(start=start, end=end, stage="light")]

    # Δgravity for just this window (recompute on the slice — cheap, correct edges).
    g_deltas = _gravity_deltas(g_seg)
    g_times = [r["ts"] for r in g_seg]

    hr_seg = _rows_between(hr, start, end)
    rr_seg = _rows_between(rr, start, end)
    resp_seg = _rows_between(resp, start, end)

    grid = _sf.build_epoch_grid(start, end, g_times, g_deltas, hr_seg, rr_seg, resp_seg)
    if grid.n_epochs == 0:
        return [StageSegment(start=start, end=end, stage="light")]

    rescaled = _sf.rescale_counts(grid.counts)
    ck_flags = _sf.cole_kripke(rescaled)
    onset_idx, final_wake_idx = _onset_and_final_wake(ck_flags)

    dog_hr = _sf.dog_hr_variability(grid.hr)
    feats = _sf.extract_features(grid, ck_flags, dog_hr, onset_idx, final_wake_idx)

    labels = _sf.classify_epochs(feats)
    labels = _sf.smooth_labels(labels)
    labels = _sf.reimpose_physiology(labels, feats, onset_idx, final_wake_idx)

    # Pre-onset and post-final-wake epochs are not sleep: force to wake unless the
    # classifier already calls them wake. (Latency + after final awakening.)
    for i in range(len(labels)):
        if i < onset_idx or i > final_wake_idx:
            labels[i] = "wake"

    # Merge consecutive same-stage epochs into segments tiling [start, end].
    segments: list[StageSegment] = []
    for i, stage in enumerate(labels):
        seg_start = grid.edges[i]
        seg_end = grid.edges[i + 1]
        if segments and segments[-1].stage == stage:
            segments[-1].end = seg_end
        else:
            segments.append(StageSegment(start=seg_start, end=seg_end, stage=stage))
    if segments:
        segments[-1].end = end  # snap to exact session end
    return segments


# ===========================================================================
# AASM hypnogram metrics  (§5)
# ===========================================================================

def hypnogram_metrics(session: SleepSession) -> dict[str, float | int]:
    """Compute AASM-style metrics (§5) from a session's stage segments.

    Definitions (AASM scoring manual; JCSM 2017):
      TIB  = end − start (in-bed span).
      SPT  = first sleep stage → last sleep stage span.
      TST  = Σ (light + deep + rem) segment seconds.
      Sleep onset = start of the first non-wake segment.
      SOL (sleep latency)   = onset − start.
      REM latency           = first REM start − onset (NaN if no REM).
      WASO                  = wake seconds AFTER onset and BEFORE final wake.
      Sleep Efficiency      = TST / TIB (0..1). [AASM standard denominator.]
      disturbances          = count of distinct WAKE runs after sleep onset.
      stage percentages     = stage minutes / TST.

    Returns seconds-based + minute/percent fields in a dict. NaN where undefined
    (e.g. no sleep at all, no REM).
    """
    nan = float("nan")
    segs = sorted(session.stages, key=lambda s: s.start)
    tib = max(0.0, session.end - session.start)

    sleep_segs = [s for s in segs if s.stage in ("light", "deep", "rem")]
    tst = sum(s.end - s.start for s in sleep_segs)
    deep_s = sum(s.end - s.start for s in segs if s.stage == "deep")
    rem_s = sum(s.end - s.start for s in segs if s.stage == "rem")
    light_s = sum(s.end - s.start for s in segs if s.stage == "light")

    if sleep_segs:
        onset = sleep_segs[0].start
        spt_end = sleep_segs[-1].end
        sol = max(0.0, onset - session.start)
    else:
        onset = session.end
        spt_end = session.end
        sol = tib

    rem_segs = [s for s in segs if s.stage == "rem"]
    rem_latency = (rem_segs[0].start - onset) if rem_segs else nan

    # WASO + disturbances: wake runs strictly after onset and before final wake.
    waso = 0.0
    disturbances = 0
    for s in segs:
        if s.stage != "wake":
            continue
        # clip to (onset, spt_end)
        w0 = max(s.start, onset)
        w1 = min(s.end, spt_end)
        if w1 > w0:
            waso += (w1 - w0)
            disturbances += 1

    se = (tst / tib) if tib > 0 else 0.0
    pct = lambda x: (x / tst * 100.0) if tst > 0 else 0.0
    return {
        "tib_s": tib,
        "tst_s": tst,
        "spt_s": max(0.0, spt_end - onset),
        "sol_s": sol,
        "rem_latency_s": rem_latency,
        "waso_s": waso,
        "efficiency": min(1.0, se),
        "disturbances": disturbances,
        "deep_min": deep_s / 60.0,
        "rem_min": rem_s / 60.0,
        "light_min": light_s / 60.0,
        "deep_pct": pct(deep_s),
        "rem_pct": pct(rem_s),
        "light_pct": pct(light_s),
    }


# ===========================================================================
# Sleep Performance score (ALG-10)
# ===========================================================================

def sleep_performance_score(
    total_sleep_min: float,
    efficiency: float,
    deep_min: float,
    rem_min: float,
    disturbances: int,
    sleep_needed_min: float | None = None,
) -> float:
    """Composite Sleep Performance score in [0.0, 100.0] (ALG-10).

    APPROXIMATE — the proprietary WHOOP formula is not published. This is an
    independent composite over four published sleep-quality dimensions, each
    normalised to [0, 1] and combined with fixed weights:

        W_dur = 0.45  duration vs. need (TST / sleep_needed)
        W_eff = 0.25  sleep efficiency (TST / TIB, already 0..1)
        W_stg = 0.20  restorative staging ((deep + rem) / TST, target 40%)
        W_con = 0.10  sleep consistency (fewer post-onset disturbances)

    Args:
        total_sleep_min:  AASM total sleep time (TST), minutes.
        efficiency:       Sleep efficiency 0.0..1.0 (TST / time-in-bed).
        deep_min:         Deep-sleep minutes.
        rem_min:          REM-sleep minutes.
        disturbances:     Count of post-onset wake runs.
        sleep_needed_min: Personalised sleep need (minutes). If None, falls
                          back to 420 (7 h). ALG-12 (Plan 13-03) will supply
                          a computed value.

    Returns:
        Score rounded to one decimal, clamped to [0.0, 100.0]. Returns 0.0
        when there is no sleep (TST = 0 zeroes every component).

    Pure function — no DB, no streams. Division guards (``max(.., 1.0)``)
    prevent divide-by-zero when TST or the target is 0 (mitigation T-13-02-03).

    No-sleep short-circuit: with ``total_sleep_min <= 0`` there is no sleep to
    score, so the result is 0.0. Without this guard the consistency term
    (W_con) would award 10 points for "0 disturbances" on a night with no
    sleep at all — see ALG-10 must-have "retorna 0 para TST=0".
    """
    if total_sleep_min <= 0:
        return 0.0

    target = sleep_needed_min if sleep_needed_min is not None else 420.0

    w_dur = min(total_sleep_min / max(target, 1.0), 1.0) * 0.45
    w_eff = efficiency * 0.25
    restorative_ratio = (deep_min + rem_min) / max(total_sleep_min, 1.0)
    w_stg = min(restorative_ratio / 0.40, 1.0) * 0.20
    w_con = (1.0 - min(disturbances / 10.0, 1.0)) * 0.10

    score = (w_dur + w_eff + w_stg + w_con) * 100.0
    return round(max(0.0, min(100.0, score)), 1)


# ===========================================================================
# Daily sleep summary  (UNCHANGED output keys)
# ===========================================================================

def daily_sleep_summary(sessions: Sequence[SleepSession], date: _dt.date) -> dict[str, Any]:
    """Aggregate the sessions whose END falls on ``date`` (UTC) into a daily metric.

    Matching rule: a session belongs to ``date`` if the UTC date of its ``end``
    timestamp equals ``date`` (a night ending the morning of ``date``).

    Returns (keys UNCHANGED — daily.py + dashboard depend on these):
        {
          "date": datetime.date,
          "total_sleep_min": float,   # AASM TST (light+deep+rem), summed
          "efficiency": float,        # in-bed-weighted mean efficiency, 0..1 (TST/TIB)
          "deep_min": float,
          "rem_min": float,
          "light_min": float,
          "disturbances": int,        # post-onset wake runs, summed across sessions
          "resting_hr": float | None,
          "avg_hrv": float | None,
          "sleep_start": float | None,
          "sleep_end": float | None,
        }

    With no matching session, returns zeros / None (documented sentinel).
    """
    matched = [s for s in sessions if _end_date_utc(s) == date]

    if not matched:
        return {
            "date": date, "total_sleep_min": 0.0, "efficiency": 0.0,
            "deep_min": 0.0, "rem_min": 0.0, "light_min": 0.0,
            "disturbances": 0, "resting_hr": None, "avg_hrv": None,
            "sleep_start": None, "sleep_end": None,
        }

    deep_s = rem_s = light_s = 0.0
    tst_s = 0.0
    in_bed_s = 0.0
    eff_weighted = 0.0
    disturbances = 0

    for s in matched:
        m = hypnogram_metrics(s)
        in_bed = s.end - s.start
        in_bed_s += in_bed
        eff_weighted += s.efficiency * in_bed
        deep_s += m["deep_min"] * 60.0
        rem_s += m["rem_min"] * 60.0
        light_s += m["light_min"] * 60.0
        tst_s += m["tst_s"]
        disturbances += int(m["disturbances"])

    efficiency = (eff_weighted / in_bed_s) if in_bed_s > 0 else 0.0

    return {
        "date": date,
        "total_sleep_min": tst_s / 60.0,
        "efficiency": efficiency,
        "deep_min": deep_s / 60.0,
        "rem_min": rem_s / 60.0,
        "light_min": light_s / 60.0,
        "disturbances": disturbances,
        "resting_hr": _resting_hr(matched),
        "avg_hrv": _avg_hrv(matched),
        "sleep_start": min(s.start for s in matched),
        "sleep_end": max(s.end for s in matched),
    }


def _end_date_utc(s: SleepSession) -> _dt.date:
    return _dt.datetime.fromtimestamp(s.end, _dt.timezone.utc).date()


def _resting_hr(sessions: Sequence[SleepSession]) -> float | None:
    """Daily resting HR = lowest per-session resting HR across matched sessions."""
    vals = [s.resting_hr for s in sessions if s.resting_hr is not None]
    return min(vals) if vals else None


def _avg_hrv(sessions: Sequence[SleepSession]) -> float | None:
    """Daily avg HRV = in-bed-weighted mean of per-session avg HRV (ms)."""
    pairs = [(s.avg_hrv, s.end - s.start) for s in sessions if s.avg_hrv is not None]
    if not pairs:
        return None
    total = sum(v * w for v, w in pairs)
    weight = sum(w for _, w in pairs)
    return total / weight if weight else None


# --- per-session HR / HRV (computed at detection time) ---------------------

#: Rolling-mean HR window (seconds) for the resting-HR estimate.
_RESTING_HR_WINDOW_S: float = 5 * 60.0
#: HRV tumbling-window length (seconds) for the night-average RMSSD.
_HRV_WINDOW_S: float = 5 * 60.0


def _session_resting_hr(start: float, end: float, hr: Sequence[dict]) -> float | None:
    """Lowest 5-min rolling-mean HR during the session (bpm), or None."""
    seg = [(r["ts"], float(r["bpm"])) for r in hr
           if start <= r["ts"] <= end and r.get("bpm") is not None]
    if not seg:
        return None
    means: list[float] = []
    t = start
    while t < end:
        win = [v for ts, v in seg if t <= ts < t + _RESTING_HR_WINDOW_S]
        if win:
            means.append(statistics.fmean(win))
        t += _RESTING_HR_WINDOW_S
    if not means:
        return statistics.fmean(v for _, v in seg)
    return min(means)


def _session_avg_hrv(start: float, end: float, rr: Sequence[dict]) -> float | None:
    """Mean RMSSD over 5-min tumbling windows across the session (ms), or None."""
    seg = [r for r in rr if start <= r["ts"] <= end]
    if not seg:
        return None
    vals: list[float] = []
    t = start
    while t < end:
        bucket = [r["rr_ms"] for r in seg if t <= r["ts"] < t + _HRV_WINDOW_S]
        try:
            vals.append(rmssd(bucket))
        except ValueError:
            pass
        t += _HRV_WINDOW_S
    return statistics.fmean(vals) if vals else None
