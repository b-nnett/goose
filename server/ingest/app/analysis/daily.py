"""
daily.py — the daily analysis orchestrator (Task 2.5).

Unlike the rest of ``analysis/`` (pure functions over stream dicts), this module
TOUCHES the DB: it reads the raw 1 Hz streams for a day, runs the Phase-2 pipeline
(sleep → recovery → strain → exercise), and persists the derived rows idempotently.

Public entry point: ``compute_day(conn, device_id, day)``.

Window choice
-------------
A night's sleep can START the prior evening and END on the target ``day``. To
capture the whole night we read streams over ``[day-1 18:00 UTC, day+1 00:00 UTC)``
(30 h). ``daily_sleep_summary`` then keeps only the session(s) whose END falls on
``day``, so the prior-evening lead-in never leaks into a neighbouring day's metric.

Day STRAIN uses the WHOOP **sleep-to-sleep day** (APPROXIMATE): from this morning's
wake (the end of the sleep session ending on ``day``) to the next sleep onset (the
start of the next detected session in the read window, else end-of-day at day+1
00:00). When there's no night ending on ``day`` we fall back to the calendar day.
HR is sliced from the full read window so the tails either side of midnight count.

Nightly metrics (rebuilt modules)
---------------------------------
- ``avg_hrv`` = ``hrv.nightly_hrv`` last-SWS tiered RMSSD over the merged night
  (replaces the sleep module's coarse 5-min windowed mean); NaN-guarded fallback
  to the sleep summary's avg_hrv.
- HRmax = ``strain.estimate_hrmax`` over the trailing 90 d of HR (observed p99.5
  dominates since age is unknown); fed to both strain and exercise.
- Calibrated nightly signals (ALL APPROXIMATE / un-calibrated) over the sleep
  window: ``spo2_pct`` (windowed ratio-of-ratios), ``skin_temp_dev_c`` (slope ×
  Δraw vs the trailing-30 d median raw baseline), ``resp_rate_bpm`` (Welch peak).

Baseline strategy (recovery)
----------------------------
``recovery_score`` needs a personal baseline (recent norms for HRV / resting HR /
resp). We derive it from the trailing ``_BASELINE_DAYS`` of already-computed
``daily_metrics`` rows, processed through ``baselines.fold_history`` to build a
robust Winsorized-EWMA baseline with:
  - 14-night half-life (EWMA center) with outlier rejection and Winsorization.
  - EWMA-of-abs-deviation spread (robust σ), per-metric floored.
  - Cold-start gate: if the HRV baseline is not yet trusted (< MIN_NIGHTS_SEED
    valid nights), ``recovery_score`` returns ``None`` and the metric column is
    left null for that day.

On the very first run (no history) the baselines are in "calibrating" status and
``recovery_score`` returns None — meaning the day's recovery column is null until
enough nights accumulate.  This is the honest behavior (vs. anchoring at 60).

Resp note: resp has no dedicated ``daily_metrics`` column.  We carry nightly resp
values in a rolling list built from the session-level resp mean (also computed here).
Since we only have access to prior ``daily_metrics`` rows (which don't store resp),
the resp baseline for the very first run will always be None and the resp term is
dropped in the composite.  This is acceptable — resp is the lowest-weight term (W=0.05)
and is documented as mostly an illness flag.

Timestamp coercion
------------------
``read.query_stream`` returns ``ts`` as tz-aware datetimes (psycopg). The analysis
modules accept datetime OR epoch via ``analysis._utils.to_epoch``, but to keep the
contract explicit and the persisted session times unambiguous we coerce every row's
``ts`` to epoch seconds in ``_load_streams`` before handing the dict to the pipeline.
"""
from __future__ import annotations

import datetime as _dt
import json
import logging
import math
import os
import statistics
from typing import Any

from .. import read, store
from . import exercise as _exercise
from . import hrv as _hrv
from . import recovery as _recovery
from . import sleep as _sleep
from . import strain as _strain
from . import units as _units
from . import baselines as _baselines
from . import calories as _calories
from ._utils import to_epoch

_log = logging.getLogger(__name__)

#: Stream kinds read for a day (everything the pipeline consumes).
_KINDS = ("hr", "rr", "resp", "gravity", "skin_temp", "spo2")

#: Trailing window (days) of daily_metrics used to build the recovery baseline.
_BASELINE_DAYS = 30

#: Trailing window (days) of HR samples used to estimate personalized HRmax
#: (strain.estimate_hrmax observed-p99.5). 90 days captures genuine peak efforts
#: without letting a single artifact spike dominate (the p99.5 rejects those).
_HRMAX_HISTORY_DAYS = 90

#: Per-stream row cap when pulling the trailing HR history for the HRmax estimate.
#: ~175k HR rows on the live device; allow generous headroom.
_HRMAX_HISTORY_LIMIT = 2_000_000

#: Trailing window (days) of skin-temp samples used to build the skin-temp raw
#: baseline (median). Matches the recovery baseline window for consistency.
_SKIN_TEMP_BASELINE_DAYS = 30

#: Generous per-stream row cap for the 30 h window (1 Hz × 30 h ≈ 108k; allow headroom).
_STREAM_LIMIT = 200_000

#: Per-stream row cap for the 30-day skin-temp baseline (1 Hz × 30 days ≈ 2.6M).
#: _STREAM_LIMIT (~2.3 days) is far too small for this window; a dedicated constant
#: matches the pattern set by _HRMAX_HISTORY_LIMIT and covers the full window.
_SKIN_TEMP_BASELINE_LIMIT = 3_000_000

#: Path to the bundled recovery→strain lookup (ALG-11). Resolved relative to this
#: module so it works regardless of the process CWD inside the container.
_TS_LOOKUP_PATH = os.path.join(os.path.dirname(__file__), "recovery_to_strain.json")

#: Recovery→strain lookup table — loaded once at import time to avoid the
#: check-then-set race under concurrent async callers (CR-04).
try:
    with open(_TS_LOOKUP_PATH, "r", encoding="utf-8") as _fh:
        _data = json.load(_fh)
    _LOOKUP_TABLE: list[dict] = _data if isinstance(_data, list) else []
except (OSError, ValueError) as _exc:
    import logging as _logging_import
    _logging_import.getLogger(__name__).warning(
        "ALG-11: failed to load %s (%s); training_state disabled", _TS_LOOKUP_PATH, _exc)
    _LOOKUP_TABLE = []

#: Sleep-need bounds (ALG-12). WHOOP's published "sleep need" never collapses to a
#: nap nor balloons past ~11 h; clamp keeps the personalised need physiological.
_SLEEP_NEED_MIN = 300.0
_SLEEP_NEED_MAX = 660.0
#: Strain reference above which a debt accrues (WHOOP's mid-scale), and its cap.
_STRAIN_REF = 14.0
_STRAIN_DEBT_CAP = 60.0
#: Sleep-debt is half of the (capped) shortfall vs the baseline.
_SLEEP_DEBT_RAW_CAP = 120.0
_SLEEP_DEBT_FACTOR = 0.5
#: Minimum valid nights of history before a personalised need is meaningful.
_MIN_SLEEP_NIGHTS = 3


def training_state_from_lookup(
    recovery_score: float | None,
    strain: float | None,
) -> str | None:
    """ALG-11 — map (recovery, strain) to a Training State label. APPROXIMATE.

    Returns one of ``"RESTORATIVE"`` / ``"OPTIMAL"`` / ``"OVERREACHING"``, or
    ``None`` when either input is ``None`` (or the lookup table is unavailable).
    NEVER returns ``"IMPOSSIBLE"`` — the lookup defines an optimal strain BAND per
    recovery level; strain below the band is RESTORATIVE, above is OVERREACHING,
    inside (inclusive) is OPTIMAL.

    Parameters
    ----------
    recovery_score :
        Recovery on a 0..100 scale (clamped). ``None`` → ``None``.
    strain :
        Today's day strain (~0..21). ``None`` → ``None``.
    """
    if recovery_score is None or strain is None:
        return None
    table = _LOOKUP_TABLE
    if not table:
        return None

    idx = int(round(max(0.0, min(100.0, float(recovery_score)))))
    # Find the row for this recovery level; fall back to the last row if absent.
    row = next((r for r in table if r.get("recovery") == idx), None)
    if row is None:
        row = table[-1]

    lower = row.get("lower_rec_strain")
    upper = row.get("upper_rec_strain")
    if lower is None or upper is None:
        return None

    if strain < lower:
        return "RESTORATIVE"
    if strain > upper:
        return "OVERREACHING"
    return "OPTIMAL"


def sleep_needed(
    prior_sleep_min: list[float],
    strain_yesterday: float | None,
    sleep_yesterday: float | None,
) -> float | None:
    """ALG-12 — personalised sleep need (minutes) from a rolling baseline. APPROXIMATE.

    Returns ``None`` when fewer than ``_MIN_SLEEP_NIGHTS`` valid (> 0) nights of
    history are available (cold-start). Otherwise:

        baseline    = mean(valid prior nights)
        strain_debt = clamp((strain_yesterday - 14) * 3, 0, 60)
        sleep_debt  = min(max(0, baseline - sleep_yesterday), 120) * 0.5
        need        = clamp(baseline + strain_debt + sleep_debt, 300, 660)

    Parameters
    ----------
    prior_sleep_min :
        Total-sleep-minutes for the prior nights (any order). Non-positive entries
        are dropped before the count/mean.
    strain_yesterday :
        Yesterday's day strain; ``None`` → no strain debt.
    sleep_yesterday :
        Yesterday's total sleep (min); ``None`` → no sleep debt.
    """
    valid = [float(v) for v in prior_sleep_min if v is not None and v > 0]
    if len(valid) < _MIN_SLEEP_NIGHTS:
        return None

    baseline = statistics.mean(valid)

    strain_debt = 0.0
    if strain_yesterday is not None and strain_yesterday > _STRAIN_REF:
        strain_debt = min(_STRAIN_DEBT_CAP, (strain_yesterday - _STRAIN_REF) * 3.0)

    sleep_debt = 0.0
    if sleep_yesterday is not None:
        raw_debt = max(0.0, baseline - sleep_yesterday)
        sleep_debt = min(raw_debt, _SLEEP_DEBT_RAW_CAP) * _SLEEP_DEBT_FACTOR

    need = baseline + strain_debt + sleep_debt
    return round(max(_SLEEP_NEED_MIN, min(_SLEEP_NEED_MAX, need)), 1)


def _day_bounds_utc(day: _dt.date) -> tuple[float, float]:
    """Calendar-day [start, end) in epoch seconds (UTC)."""
    start = _dt.datetime.combine(day, _dt.time(0, 0), _dt.timezone.utc)
    end = start + _dt.timedelta(days=1)
    return start.timestamp(), end.timestamp()


def _window_bounds_utc(day: _dt.date) -> tuple[float, float]:
    """Sleep-aware read window [day-1 18:00, day+1 00:00) in epoch seconds (UTC)."""
    lead = _dt.datetime.combine(day, _dt.time(0, 0), _dt.timezone.utc) - _dt.timedelta(hours=6)
    _, day_end = _day_bounds_utc(day)
    return lead.timestamp(), day_end


def _load_streams(conn, device_id: str, start: float, end: float) -> dict[str, list[dict]]:
    """Read the pipeline's streams over the HALF-OPEN window [start, end) epoch
    seconds, coercing each row's ``ts`` (tz-aware datetime from psycopg) to epoch
    seconds.

    ``read.query_stream`` filters ``ts <= to`` (its end bound is INCLUSIVE), so a
    sample landing exactly on ``end`` (e.g. a calendar-day boundary at midnight)
    would otherwise be pulled into BOTH adjacent days. We pass ``int(end) - 1`` (one
    second before the exclusive bound) to keep our window half-open without changing
    query_stream's shared contract for other callers."""
    streams: dict[str, list[dict]] = {}
    for kind in _KINDS:
        rows = read.query_stream(conn, kind, device_id, int(start), int(end) - 1, limit=_STREAM_LIMIT)
        if len(rows) == _STREAM_LIMIT:
            _log.warning(
                "stream %s hit the per-stream cap (%d rows); "
                "window data may be truncated", kind, _STREAM_LIMIT)
        for r in rows:
            r["ts"] = to_epoch(r["ts"])
        streams[kind] = rows
    return streams


def _slice_day(streams: dict[str, list[dict]], day_start: float, day_end: float) -> dict[str, list[dict]]:
    """Sub-streams restricted to the calendar day [day_start, day_end)."""
    return {
        kind: [r for r in rows if day_start <= r["ts"] < day_end]
        for kind, rows in streams.items()
    }


def _mean_resp(rows: list[dict], start: float, end: float) -> float | None:
    """Mean raw respiration over [start, end], or None if no samples."""
    vals = [float(r["raw"]) for r in rows if start <= r["ts"] <= end and r.get("raw") is not None]
    return statistics.fmean(vals) if vals else None


def _build_baselines(
    conn,
    device_id: str,
    day: _dt.date,
) -> dict[str, _baselines.BaselineState | None]:
    """Build per-metric Winsorized-EWMA baselines from the trailing _BASELINE_DAYS.

    Reads prior ``daily_metrics`` rows (oldest → newest) and replays them through
    ``baselines.fold_history`` for HRV and resting-HR.  Resp is omitted because
    it has no dedicated ``daily_metrics`` column (the resp baseline will be None,
    dropping the resp term from the recovery composite on its first run — acceptable
    since W_RESP=0.05 is the lowest weight and the resp term is mostly an illness flag).

    Returns a dict with keys ``"hrv"`` and ``"resting_hr"`` mapping to
    ``BaselineState`` objects (or None if zero prior rows).  The dict can be
    passed directly to ``recovery_score`` as the ``baselines`` argument.
    """
    prior_start = day - _dt.timedelta(days=_BASELINE_DAYS)
    prior_end = day - _dt.timedelta(days=1)
    # query_daily returns rows ordered by day ascending (oldest first for fold_history).
    prior = read.query_daily(conn, device_id, prior_start, prior_end)

    hrv_series: list[float | None] = [
        float(r["avg_hrv"]) if r.get("avg_hrv") is not None else None
        for r in prior
    ]
    rhr_series: list[float | None] = [
        float(r["resting_hr"]) if r.get("resting_hr") is not None else None
        for r in prior
    ]

    hrv_state = (
        _baselines.fold_history(hrv_series, _baselines.METRIC_CFG["hrv"])
        if hrv_series else None
    )
    rhr_state = (
        _baselines.fold_history(rhr_series, _baselines.METRIC_CFG["resting_hr"])
        if rhr_series else None
    )

    return {
        "hrv": hrv_state,
        "resting_hr": rhr_state,
        "resp": None,  # no daily column available; resp term dropped in composite
    }


def _trailing_hr_history(conn, device_id: str, day: _dt.date) -> list[float]:
    """Flat list of HR bpm over the trailing _HRMAX_HISTORY_DAYS (for HRmax p99.5).

    Reads from [day-_HRMAX_HISTORY_DAYS 00:00, day+1 00:00) UTC so today's own HR
    (which includes any peak effort) also counts toward the observed maximum.
    """
    hist_start = _dt.datetime.combine(
        day - _dt.timedelta(days=_HRMAX_HISTORY_DAYS), _dt.time(0, 0), _dt.timezone.utc
    ).timestamp()
    _, day_end = _day_bounds_utc(day)
    rows = read.query_stream(
        conn, "hr", device_id, int(hist_start), int(day_end), limit=_HRMAX_HISTORY_LIMIT)
    return [float(r["bpm"]) for r in rows if r.get("bpm") is not None]


def _skin_temp_baseline_raw(conn, device_id: str, day: _dt.date) -> float | None:
    """Trailing-window robust baseline of raw skin-temp ADC counts (median), or None.

    APPROXIMATE: the baseline is the median of ALL skin-temp samples over the
    trailing _SKIN_TEMP_BASELINE_DAYS (whole-day, not sleep-gated). The strap's
    type-47 backfill store is dominated by sleep-period samples, and the median is
    robust to the daytime tail, so this is a reasonable personal baseline without a
    dedicated per-night raw column. Excludes the target day so tonight is measured
    against PRIOR nights.
    """
    base_start = _dt.datetime.combine(
        day - _dt.timedelta(days=_SKIN_TEMP_BASELINE_DAYS), _dt.time(0, 0), _dt.timezone.utc
    ).timestamp()
    day_start, _ = _day_bounds_utc(day)
    rows = read.query_stream(
        conn, "skin_temp", device_id, int(base_start), int(day_start) - 1,
        limit=_SKIN_TEMP_BASELINE_LIMIT)
    vals = [float(r["raw"]) for r in rows if r.get("raw") is not None]
    return statistics.median(vals) if vals else None


def _nightly_signals(
    conn,
    device_id: str,
    day: _dt.date,
    streams: dict[str, list[dict]],
    night_start: float | None,
    night_end: float | None,
) -> dict[str, float | None]:
    """Compute APPROXIMATE calibrated nightly signals over the sleep window.

    - spo2_pct        — windowed ratio-of-ratios over the night's red/IR samples.
    - skin_temp_dev_c — slope·(tonight_mean_raw − trailing_baseline_raw), °C.
    - resp_rate_bpm   — Welch-peak respiratory rate over the night's resp samples.

    All un-calibrated (default units.py constants); returns None per-field when
    there is no sleep window or insufficient samples. Labels stay honest.
    """
    out: dict[str, float | None] = {
        "spo2_pct": None, "skin_temp_dev_c": None, "resp_rate_bpm": None}
    if night_start is None or night_end is None:
        return out

    def _in_night(rows: list[dict]) -> list[dict]:
        return [r for r in rows if night_start <= r["ts"] <= night_end]

    # SpO2 — windowed AC/DC ratio over the whole night's red/IR.
    spo2_rows = _in_night(streams.get("spo2") or [])
    reds = [float(r["red"]) for r in spo2_rows if r.get("red") is not None]
    irs = [float(r["ir"]) for r in spo2_rows if r.get("ir") is not None]
    if len(reds) >= 2 and len(reds) == len(irs):
        try:
            out["spo2_pct"] = round(_units.spo2_percent_window(reds, irs), 1)
        except (ZeroDivisionError, ValueError):
            out["spo2_pct"] = None

    # Skin-temp deviation — tonight's mean raw vs trailing baseline raw.
    st_rows = _in_night(streams.get("skin_temp") or [])
    st_raw = [float(r["raw"]) for r in st_rows if r.get("raw") is not None]
    if st_raw:
        baseline_raw = _skin_temp_baseline_raw(conn, device_id, day)
        if baseline_raw is not None:
            mean_raw = statistics.fmean(st_raw)
            dev = _units.skin_temp_deviation([mean_raw], baseline_raw)[0]
            out["skin_temp_dev_c"] = round(dev, 2)

    # Respiratory rate — Welch-peak over the night's resp waveform.
    resp_rows = _in_night(streams.get("resp") or [])
    resp_sig = [float(r["raw"]) for r in resp_rows if r.get("raw") is not None]
    if len(resp_sig) >= 2:
        rr = _units.resp_rate_from_signal(resp_sig)
        out["resp_rate_bpm"] = round(rr, 1) if rr is not None else None

    return out


def _session_to_dict(s: _sleep.SleepSession) -> dict[str, Any]:
    return {
        "start": s.start,
        "end": s.end,
        "efficiency": s.efficiency,
        "resting_hr": s.resting_hr,
        "avg_hrv": s.avg_hrv,
        "stages": [{"start": seg.start, "end": seg.end, "stage": seg.stage} for seg in s.stages],
    }


def _exercise_to_dict(e: _exercise.ExerciseSession) -> dict[str, Any]:
    return {
        "start": e.start,
        "end": e.end,
        "avg_hr": e.avg_hr,
        "peak_hr": e.peak_hr,
        "strain": e.strain,
        "kind": e.kind,
        # Per-bout intensity (Task 11a). JSON-serializable: zone keys → strings for
        # the API/JSONB; APPROXIMATE.
        "duration_s": e.duration_s,
        "zone_time_pct": {str(z): pct for z, pct in (e.zone_time_pct or {}).items()},
        "avg_hrr_pct": e.avg_hrr_pct,
        "hrmax": e.hrmax,
        "hrmax_source": e.hrmax_source,
        # Calorie estimation (WHOOP/Keytel formula). None when no profile is set.
        "calories_kcal": e.calories_kcal,
        "calories_kj": e.calories_kj,
    }


def compute_day(conn, device_id: str, day: _dt.date) -> dict[str, Any]:
    """Run the full analysis pipeline for (device_id, day), persist, and return a
    JSON-serializable summary. Idempotent: re-running fully replaces the day's rows.

    Empty-day skip rule
    -------------------
    If the day has NO data at all — i.e. NO detected sleep session whose END falls on
    ``day`` AND NO HR or gravity samples within the calendar day [day 00:00, day+1
    00:00) — we write NOTHING (no daily_metrics, sleep_sessions, or exercise_sessions
    row) and return ``{"status": "no_data", "date": <iso>}``. This keeps a genuine
    "0-minute sleep" day (which DOES have HR/gravity streams) distinguishable from a
    "no data yet" day. When there IS data we always write a normal row, even if sleep
    happens to be 0.

    Consistency on recompute
    ------------------------
    Before inserting, we DELETE the day's existing sleep_sessions (END date == day)
    and exercise_sessions (start within the calendar day) and then insert the freshly
    computed set — all in the caller's transaction — so a recompute that yields FEWER
    sessions can't leave stale rows that desync ``daily_metrics.exercise_count``.

    Returns ``{sleep_summary, recovery, strain, exercises, hrv, resting_hr}`` (or the
    no_data marker above).
    """
    win_start, win_end = _window_bounds_utc(day)
    day_start, day_end = _day_bounds_utc(day)
    streams = _load_streams(conn, device_id, win_start, win_end)

    # ── Sleep (over the sleep-aware window) ──────────────────────────────────
    sessions = _sleep.detect_sleep(streams)
    sleep_summary = _sleep.daily_sleep_summary(sessions, day)
    resting_hr = sleep_summary["resting_hr"]

    # Sessions whose END falls on `day` are the night we attribute to this day.
    night_sessions = [s for s in sessions if _sleep._end_date_utc(s) == day]

    # ── Empty-day skip ───────────────────────────────────────────────────────
    # No sleep night ending on `day` AND no HR/gravity samples in the calendar day
    # → there is genuinely nothing to attribute; don't write a degenerate row.
    day_streams = _slice_day(streams, day_start, day_end)
    has_day_streams = bool(day_streams.get("hr") or day_streams.get("gravity"))
    if not night_sessions and not has_day_streams:
        return {"status": "no_data", "date": day.isoformat()}

    # ── Nightly HRV (last-SWS tiered RMSSD; primary avg_hrv) ──────────────────
    # Replace the sleep module's coarse 5-min-window HRV mean with the rebuilt
    # nightly_hrv (range filter → Kubios → segment-pooled RMSSD, last-SWS window).
    # We attribute ONE merged night to `day`: stitch the night_sessions into a
    # single span [earliest start, latest end] with pooled stages so the tiered
    # window selection sees the whole night. Falls back to the sleep summary's
    # avg_hrv when there's no night session / RR is too sparse (NaN guard).
    night_start = min((s.start for s in night_sessions), default=None)
    night_end = max((s.end for s in night_sessions), default=None)
    avg_hrv = sleep_summary["avg_hrv"]
    if night_sessions:
        merged_stages: list[_sleep.StageSegment] = []
        for s in night_sessions:
            merged_stages.extend(s.stages)
        merged_stages.sort(key=lambda seg: seg.start)
        # resting_hr/avg_hrv intentionally omitted: nightly_hrv only uses start/end/stages.
        merged_night = _sleep.SleepSession(
            start=night_start, end=night_end, efficiency=sleep_summary["efficiency"],
            stages=merged_stages)
        hrv_res = _hrv.nightly_hrv(
            streams.get("rr") or [], merged_night,
            stages=merged_stages or None)
        nightly_rmssd = hrv_res.get("rmssd")
        if nightly_rmssd is not None and math.isfinite(nightly_rmssd):
            avg_hrv = nightly_rmssd

    # ── Recovery (needs the night's resp, sleep_perf, + a personal baseline) ──
    night_resp = None
    if night_sessions:
        night_resp = _mean_resp(streams.get("resp") or [], night_start, night_end)

    # Build Winsorized-EWMA baselines from the trailing _BASELINE_DAYS of history.
    baselines = _build_baselines(conn, device_id, day)

    # ── ALG-12 Sleep Needed: rolling prior-7d sleep baseline + yesterday's load ──
    # Read the trailing 7 calendar days of daily_metrics (oldest → newest). The
    # personalised need needs >= 3 valid nights; with fewer it returns None.
    _prior_7d = read.query_daily(
        conn, device_id, day - _dt.timedelta(days=7), day - _dt.timedelta(days=1))
    # Exclude yesterday (last row) from the baseline so the debt formula
    # baseline - sleep_yesterday does not subtract a value that is part of the
    # baseline itself (double-counting bias that underestimates sleep debt).
    _baseline_rows = _prior_7d[:-1] if len(_prior_7d) > 1 else []
    _prior_sleep_min = [
        float(r["total_sleep_min"]) for r in _baseline_rows
        if r.get("total_sleep_min") is not None
    ]
    _last = _prior_7d[-1] if _prior_7d else None
    _strain_yesterday = (
        float(_last["strain"]) if _last and _last.get("strain") is not None else None
    )
    _sleep_yesterday = (
        float(_last["total_sleep_min"])
        if _last and _last.get("total_sleep_min") is not None else None
    )
    _sleep_needed = sleep_needed(_prior_sleep_min, _strain_yesterday, _sleep_yesterday)

    # Sleep efficiency (0..1) as the sleep-performance proxy.
    sleep_perf: float | None = sleep_summary.get("efficiency")

    # ALG-10 Sleep Performance composite (0..100) — separate from sleep_perf,
    # which stays as efficiency (0..1) for recovery_score. sleep_needed_min is the
    # ALG-12 personalised need (or None → internal 420-min fallback when there is
    # insufficient history).
    _sleep_perf_score: float | None = None
    if (sleep_summary.get("total_sleep_min") or 0.0) > 0:
        _sleep_perf_score = _sleep.sleep_performance_score(
            total_sleep_min=sleep_summary.get("total_sleep_min") or 0.0,
            efficiency=sleep_summary.get("efficiency") or 0.0,
            deep_min=sleep_summary.get("deep_min") or 0.0,
            rem_min=sleep_summary.get("rem_min") or 0.0,
            disturbances=int(sleep_summary.get("disturbances") or 0),
            sleep_needed_min=_sleep_needed,  # ALG-12; None → 420-min fallback
        )

    recovery = None
    if avg_hrv is not None and resting_hr is not None:
        recovery = _recovery.recovery_score(
            avg_hrv,
            resting_hr,
            night_resp,       # may be None → resp term dropped
            baselines,
            sleep_perf=sleep_perf,
        )

    # ── Personalized HRmax (observed p99.5 over trailing HR history) ─────────
    # Age is unknown, so the observed p99.5 dominates (estimate_hrmax falls through
    # to Tanaka/220−age only when history is thin). ~175k HR rows on the live device.
    hr_history = _trailing_hr_history(conn, device_id, day)
    hrmax, _hrmax_source = _strain.estimate_hrmax(hr_history, age=None)
    eff_max_hr = hrmax if hrmax > 0.0 else None

    # ── Strain over the WHOOP sleep-to-sleep day ─────────────────────────────
    # APPROXIMATION of WHOOP's wake→next-sleep accumulation window: from THIS
    # morning's wake (night_end — the sleep session ending on `day`) to the NEXT
    # sleep onset. The next onset is the start of the earliest session in the read
    # window that begins after `night_end` (tonight's bedtime if it landed before
    # the window's day+1 00:00 edge); otherwise we cap at end-of-day (day+1 00:00).
    # When there's no night ending today (e.g. a midday-only capture) we fall back
    # to the calendar day. HR is sliced from the full read window so the pre-dawn /
    # evening tails on either side of midnight are included.
    if night_end is not None:
        strain_lo = night_end
        later_onsets = [s.start for s in sessions if s.start > night_end]
        strain_hi = min(later_onsets) if later_onsets else day_end
    else:
        strain_lo, strain_hi = day_start, day_end
    strain_hr = [r for r in (streams.get("hr") or []) if strain_lo <= r["ts"] < strain_hi]
    strain_val = _strain.strain(
        strain_hr,
        max_hr=eff_max_hr,
        resting_hr=float(resting_hr) if resting_hr is not None else _strain.DEFAULT_RESTING_HR)

    # ── ALG-11 Training State (today's recovery + strain → optimal band lookup) ──
    # recovery_score is already on a 0..100 scale (recovery.py returns [0,100]), so
    # it is passed through unchanged. None when recovery or strain is None.
    _training_state = training_state_from_lookup(recovery, strain_val)

    # ── Exercise (calendar day; explicit resting_hr + personalized HRmax) ─────
    # Read the device profile for calorie estimation (None → calories stay None).
    device_profile = read.query_profile(conn, device_id)
    exercises = _exercise.detect_exercises(
        day_streams,
        resting_hr=float(resting_hr) if resting_hr is not None else None,
        max_hr=eff_max_hr,
        profile=device_profile)

    # ── Calibrated nightly signals (APPROXIMATE; over the sleep window) ───────
    signals = _nightly_signals(conn, device_id, day, streams, night_start, night_end)

    # ── Persist (idempotent upserts) ─────────────────────────────────────────
    night_dicts = [_session_to_dict(s) for s in night_sessions]
    ex_dicts = [_exercise_to_dict(e) for e in exercises]

    # ── Total daily calories (ALG-13): RMR (Mifflin–St Jeor) + exercise burn ──
    # None when there's no device profile (no RMR basis) → iOS hides the card.
    _rmr = _calories.rmr_kcal_per_day(device_profile)
    _exercise_kcal = sum((e.get("calories_kcal") or 0.0) for e in ex_dicts)
    _total_calories = round(_rmr + _exercise_kcal, 1) if _rmr is not None else None

    metrics = {
        "total_sleep_min": sleep_summary["total_sleep_min"],
        "efficiency": sleep_summary["efficiency"],
        "deep_min": sleep_summary["deep_min"],
        "rem_min": sleep_summary["rem_min"],
        "light_min": sleep_summary["light_min"],
        "disturbances": sleep_summary["disturbances"],
        "resting_hr": resting_hr,
        "avg_hrv": avg_hrv,
        "recovery": recovery,
        "strain": strain_val,
        "exercise_count": len(ex_dicts),
        "sleep_start": sleep_summary["sleep_start"],
        "sleep_end": sleep_summary["sleep_end"],
        "spo2_pct": signals["spo2_pct"],
        "skin_temp_dev_c": signals["skin_temp_dev_c"],
        "resp_rate_bpm": signals["resp_rate_bpm"],
        "sleep_performance": _sleep_perf_score,
        "training_state": _training_state,
        "sleep_needed_min": _sleep_needed,
        "total_calories_kcal": _total_calories,
    }
    # Delete the day's existing session rows first, then insert the fresh set, so a
    # recompute yielding FEWER sessions can't leave stale rows (which would desync
    # daily_metrics.exercise_count). All in the caller's transaction → atomic.
    store.delete_sessions_for_day(conn, device_id, day)
    store.upsert_daily_metrics(conn, device_id, day, metrics)
    store.upsert_sleep_sessions(conn, device_id, night_dicts)
    store.upsert_exercise_sessions(conn, device_id, ex_dicts)

    # ── Return JSON-serializable summary (date → ISO string) ─────────────────
    summary = dict(sleep_summary)
    summary["date"] = day.isoformat()
    return {
        "sleep_summary": summary,
        "recovery": recovery,
        "strain": strain_val,
        "exercises": ex_dicts,
        "hrv": avg_hrv,
        "resting_hr": resting_hr,
        "spo2_pct": signals["spo2_pct"],
        "skin_temp_dev_c": signals["skin_temp_dev_c"],
        "resp_rate_bpm": signals["resp_rate_bpm"],
    }
