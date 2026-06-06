"""
hrv.py — HRV metrics (RMSSD, SDNN) from RR intervals.

Task 5 rebuild: float64 Task Force RMSSD, neurokit2-based RR cleaning pipeline
(range filter → Kubios ectopic correction → interpolation), segment-aware gap
pooling, and tiered nightly-window selection (last-SWS → all-SWS → whole-night).

Reference:
  Task Force of the European Society of Cardiology and the North American Society
  of Pacing and Electrophysiology (1996). Heart rate variability: standards of
  measurement, physiological interpretation, and clinical use.
  Eur Heart J 17:354–381 / Circulation 93:1043–1065 (PMID 8598068).

neurokit2 library:
  nk.intervals_to_peaks → nk.signal_fixpeaks(method="kubios") → nk.hrv_time
  Lipponen & Tarvainen (2019) kubios artifact classifier.

All RR values and returned HRV metrics are in milliseconds (ms).
"""
from __future__ import annotations

import math
import statistics
from typing import Any, Sequence

import numpy as np
import neurokit2 as nk

# ---------------------------------------------------------------------------
# Physiological plausibility filter
# ---------------------------------------------------------------------------

#: Minimum plausible RR interval in ms (300 ms ≈ 200 bpm — extreme exercise).
RR_MIN_MS: int = 300
#: Maximum plausible RR interval in ms (2000 ms ≈ 30 bpm — deep sleep / bradycardia).
RR_MAX_MS: int = 2000

#: Minimum beats required to compute a trustworthy RMSSD.
#: Below this the result is physiologically unreliable — return NaN/empty.
MIN_BEATS: int = 20

#: Gap threshold (seconds): successive RR timestamps separated by more than this
#: are treated as a segment boundary (data gap or wake epoch).
#: 3 s = ~4 missed beats at 60 bpm; generous enough to not split normal pauses.
GAP_THRESHOLD_S: float = 3.0

#: Minimum SWS episode duration (seconds) to qualify as the "last SWS" window.
#: 5 min × 60 = 300 s; approximately 120–150 beats at 60 bpm.
SWS_MIN_DURATION_S: float = 5 * 60.0


def _filter_rr(rr_ms: Sequence[int | float]) -> list[float]:
    """Return only physiologically-plausible RR intervals.

    Accepts values in the closed interval [RR_MIN_MS, RR_MAX_MS]
    (300–2000 ms).  Values outside that range are silently dropped.

    Args:
        rr_ms: Sequence of RR-interval measurements in milliseconds.

    Returns:
        List of plausible RR values (as floats), preserving order.
    """
    return [float(v) for v in rr_ms if RR_MIN_MS <= v <= RR_MAX_MS]


# ---------------------------------------------------------------------------
# rmssd_ms — pure Task Force RMSSD in float64 ms (no filtering)
# ---------------------------------------------------------------------------

def rmssd_ms(nn_ms: Sequence[int | float] | np.ndarray) -> float:
    """Root mean square of successive NN-interval differences (ms), float64.

    Implements the Task Force (1996) standard definition exactly:
        RMSSD = sqrt( (1 / N-1) * sum_{i=1}^{N-1} (NN_{i+1} - NN_i)^2 )

    NOTE: This function does NOT filter or clean inputs.  Feed it already-
    cleaned NN intervals (output of ``clean_rr``).  For a convenience wrapper
    that applies the range filter first, use ``rmssd()`` (backward-compatible).

    Args:
        nn_ms: 1-D array-like of NN intervals in milliseconds (≥ 2 values).

    Returns:
        RMSSD in milliseconds (float64).  Returns 0.0 for a single-element
        (or empty) after ``diff`` produces no terms — callers should check.
    """
    nn = np.asarray(nn_ms, dtype=np.float64)
    diff = np.diff(nn)                      # N-1 successive differences
    if diff.size == 0:
        return 0.0
    return float(np.sqrt(np.mean(diff ** 2)))  # mean divides by N-1 ✓


# ---------------------------------------------------------------------------
# clean_rr — range filter → neurokit2 Kubios correction → interpolation
# ---------------------------------------------------------------------------

def clean_rr(
    rr_ms: Sequence[int | float] | np.ndarray,
) -> tuple[np.ndarray, np.ndarray, int, int]:
    """RR-cleaning pipeline: range filter → Kubios artifact correction.

    Steps (per research doc §2 + §3):
      1. Range filter: drop intervals outside [RR_MIN_MS, RR_MAX_MS].
      2. Convert to synthetic peak indices at 1000 Hz (1 sample == 1 ms).
      3. Kubios / Lipponen–Tarvainen (2019) ectopic-beat detection via
         ``nk.signal_fixpeaks(method="kubios", iterative=True)``.
      4. Return the corrected NN array and cleaned peak indices as float64.

    Graceful degradation:
      If fewer than MIN_BEATS plausible intervals remain after range filtering,
      returns empty arrays (no RMSSD trustworthy) plus the raw counts.

    Args:
        rr_ms: Raw RR/IBI intervals in milliseconds.

    Returns:
        (nn_clean, peaks_clean, n_beats, n_artifacts) where:
          nn_clean    — float64 ndarray of corrected NN intervals.
                        Empty array when too few beats after range filter.
          peaks_clean — float64 ndarray of cleaned peak sample indices (at
                        1000 Hz, so index / 1000.0 == seconds from window
                        start).  Use peaks_clean[:-1] for per-NN timestamps.
                        Always satisfies len(peaks_clean) == len(nn_clean) + 1.
          n_beats     — total count of input intervals (before any filtering).
          n_artifacts — number of intervals flagged/corrected by range filter
                        + Kubios.
    """
    rr_raw = np.asarray(rr_ms, dtype=np.float64)
    n_beats = int(rr_raw.size)

    # Step 1: range filter
    mask = (rr_raw >= RR_MIN_MS) & (rr_raw <= RR_MAX_MS)
    n_range_dropped = int(np.sum(~mask))
    rr_filtered = rr_raw[mask]

    _empty = np.array([], dtype=np.float64)
    if rr_filtered.size < MIN_BEATS:
        return _empty, _empty, n_beats, n_range_dropped

    # Step 2: RR (ms) → synthetic peak indices at 1000 Hz (1 sample == 1 ms)
    peaks = nk.intervals_to_peaks(rr_filtered, sampling_rate=1000)

    # Step 3: Kubios artifact correction (Lipponen–Tarvainen 2019).
    # Narrow try/except: only wraps the fixpeaks call itself so that a counting
    # error cannot silently revert to uncorrected peaks.
    # The artifacts dict contains non-list entries (method str, rr/drrs/… ndarrays,
    # c1/c2 floats) — count only the four annotation lists by name.
    _KUBIOS_ANNOTATION_KEYS = ("ectopic", "missed", "extra", "longshort")
    try:
        artifacts, peaks_clean = nk.signal_fixpeaks(
            peaks, sampling_rate=1000, iterative=True, method="kubios", show=False
        )
    except Exception:
        # Defensive: if neurokit2 fails (e.g. version quirk), fall back to
        # the range-filtered RR without kubios correction.
        peaks_clean = peaks
        n_kubios = 0
    else:
        # Count only the four annotation lists; guard for missing keys.
        n_kubios = sum(
            len(artifacts[k]) for k in _KUBIOS_ANNOTATION_KEYS if k in artifacts
        )

    # Step 4: peaks → NN intervals (ms, float64)
    # peaks_clean is an ndarray of sample indices; diff gives intervals in samples.
    # At 1000 Hz, 1 sample == 1 ms → cast to float64.
    peaks_clean = np.asarray(peaks_clean, dtype=np.float64)
    nn_clean = np.diff(peaks_clean)

    n_artifacts = n_range_dropped + n_kubios
    return nn_clean, peaks_clean, n_beats, n_artifacts


# ---------------------------------------------------------------------------
# _pool_segments — gap-aware RMSSD pooling
# ---------------------------------------------------------------------------

def _pool_rmssd_over_segments(
    rr_ms: np.ndarray,
    ts: np.ndarray,
    gap_threshold_s: float = GAP_THRESHOLD_S,
) -> float:
    """Compute RMSSD pooling within-segment squared diffs across data gaps.

    Algorithm (research doc §4):
      - Split the series at any timestamp gap > gap_threshold_s (a data gap
        or stage boundary).
      - Within each segment compute the successive squared differences.
      - Pool all within-segment squared diffs, then take sqrt(mean).
      - This avoids fabricating a large spurious diff at each gap splice.

    Args:
        rr_ms:           1-D float64 array of NN intervals in ms.
        ts:              1-D float64 array of timestamps in epoch seconds,
                         same length as rr_ms.
        gap_threshold_s: Timestamp gaps larger than this (seconds) split segments.

    Returns:
        Pooled RMSSD in ms, or NaN if no within-segment diffs exist.
    """
    if rr_ms.size < 2:
        return float("nan")

    # Split indices: where consecutive timestamps jump by > gap_threshold_s.
    # Each split point i means the segment boundary falls between i-1 and i.
    time_gaps = np.diff(ts)
    split_at = np.where(time_gaps > gap_threshold_s)[0] + 1  # indices of new segment starts

    # Build segment slices
    seg_starts = np.concatenate([[0], split_at])
    seg_ends = np.concatenate([split_at, [len(rr_ms)]])

    all_sq_diffs: list[float] = []
    for s, e in zip(seg_starts, seg_ends):
        seg = rr_ms[s:e]
        if seg.size < 2:
            continue
        diffs = np.diff(seg)
        all_sq_diffs.extend(diffs ** 2)

    if not all_sq_diffs:
        return float("nan")
    return float(np.sqrt(np.mean(all_sq_diffs)))


# ---------------------------------------------------------------------------
# nightly_hrv — tiered window selection + full nightly pipeline
# ---------------------------------------------------------------------------

def nightly_hrv(
    rr: Sequence[dict[str, Any]],
    sleep_session: Any,
    stages: Sequence[Any] | None = None,
) -> dict[str, Any]:
    """Compute WHOOP-style nightly RMSSD with tiered SWS window selection.

    Window selection (per research doc §4):
      1. Primary (last_sws):    last contiguous "deep" episode ≥ SWS_MIN_DURATION_S
      2. Secondary (all_sws):   recency-weighted mean RMSSD over all "deep" episodes
                                when the last episode is too short but others exist
      3. Fallback (whole_night): entire sleep session (stages=None always lands here)

    Always computes and returns the whole-night RMSSD alongside (for empirical
    rollout tuning and WHOOP comparison).

    Cleaning pipeline (per research doc §2–3):
      Range filter [300–2000 ms] → Kubios artifact correction → pooled RMSSD
      (segment-aware: no diffs fabricated across timestamp gaps).

    Args:
        rr:            List of {"ts": float_epoch_s, "rr_ms": int} dicts.
                       May be empty → returns NaN RMSSD.
        sleep_session: Object with .start and .end (epoch seconds).
        stages:        Optional list of objects with .start, .end, .stage
                       (strings: "deep"/"rem"/"light"/"wake"). If None,
                       whole-night fallback is used directly.

    Returns:
        dict with keys:
          rmssd             — nightly RMSSD from the chosen window (ms, float)
          tier              — "last_sws" | "all_sws" | "whole_night"
          window_start      — epoch seconds of the window used
          window_end        — epoch seconds of the window used
          n_beats           — count of RR intervals in the chosen window (pre-clean)
          n_artifacts       — intervals corrected/removed in the chosen window
          rmssd_whole_night — whole-night RMSSD (always populated for comparison)
          pnn50             — % of successive NN diffs > 50 ms (QC / validation)
          mean_nn           — mean NN interval in ms (QC / validation)
    """
    session_start = float(sleep_session.start)
    session_end = float(sleep_session.end)

    # Filter RR to the full session window
    def _rr_in_window(start: float, end: float) -> tuple[np.ndarray, np.ndarray]:
        """Return (rr_ms_array, ts_array) for rows within [start, end]."""
        rows = [(float(r["ts"]), float(r["rr_ms"])) for r in rr
                if start <= float(r["ts"]) <= end]
        if not rows:
            return np.array([], dtype=np.float64), np.array([], dtype=np.float64)
        ts_arr = np.array([t for t, _ in rows], dtype=np.float64)
        rr_arr = np.array([v for _, v in rows], dtype=np.float64)
        return rr_arr, ts_arr

    def _compute_rmssd_window(
        start: float, end: float
    ) -> tuple[float, int, int, float, float]:
        """Segment-first clean + pool RMSSD for an arbitrary window.

        Gaps are detected on the ORIGINAL epoch-second RR timestamps so that
        a real wall-clock gap (e.g. 60 s of missing data) correctly splits the
        series into separate segments rather than being collapsed to a single
        ~0.8 s NN interval by the cumulative synthetic-time approach.

        Algorithm (research doc §4):
          1. Split the in-window RR list into contiguous runs wherever the gap
             between consecutive original timestamps exceeds GAP_THRESHOLD_S.
          2. Run clean_rr + compute within-segment squared successive diffs per run.
          3. Pool across runs as sqrt(sum(sq_diffs) / count).

        Returns:
            (rmssd, n_beats, n_art, pnn50, mean_nn)
        """
        rr_arr, ts_arr = _rr_in_window(start, end)
        if rr_arr.size == 0:
            return float("nan"), 0, 0, float("nan"), float("nan")

        # Split on original timestamps (gap-first, before cleaning)
        if ts_arr.size > 1:
            time_gaps = np.diff(ts_arr)
            split_at = np.where(time_gaps > GAP_THRESHOLD_S)[0] + 1
        else:
            split_at = np.array([], dtype=np.intp)
        seg_starts = np.concatenate([[0], split_at])
        seg_ends = np.concatenate([split_at, [rr_arr.size]])

        all_sq_diffs: list[float] = []
        all_nn: list[float] = []
        total_beats = 0
        total_art = 0

        for s, e in zip(seg_starts, seg_ends):
            seg_rr = rr_arr[s:e]
            nn_seg, _peaks_seg, n_b, n_a = clean_rr(seg_rr)
            total_beats += n_b
            total_art += n_a
            if nn_seg.size < 2:
                continue
            sq = np.diff(nn_seg) ** 2
            all_sq_diffs.extend(sq.tolist())
            all_nn.extend(nn_seg.tolist())

        if not all_sq_diffs:
            return float("nan"), total_beats, total_art, float("nan"), float("nan")

        rmssd_val = float(np.sqrt(np.mean(all_sq_diffs)))

        # QC metrics (research §3) — computed over all cleaned NN across segments
        nn_all = np.array(all_nn, dtype=np.float64)
        diffs_abs = np.abs(np.diff(nn_all))
        pnn50 = (
            float(np.sum(diffs_abs > 50.0) / diffs_abs.size * 100.0)
            if diffs_abs.size > 0
            else float("nan")
        )
        mean_nn = float(np.mean(nn_all))
        return rmssd_val, total_beats, total_art, pnn50, mean_nn

    # Always compute whole-night RMSSD via the segment-first pipeline.
    # _compute_rmssd_window is reused for the whole_night tier to avoid
    # computing the same data twice.
    wn_rmssd, wn_n_beats, wn_n_art, wn_pnn50, wn_mean_nn = _compute_rmssd_window(
        session_start, session_end
    )
    rmssd_wn = wn_rmssd

    # Also fetch raw wn_rr/wn_ts arrays for _compute_all_sws_recency_weighted
    # (needed to slice per-episode RR without re-querying the full stream).
    wn_rr, wn_ts = _rr_in_window(session_start, session_end)

    # ---- Tier selection ------------------------------------------------
    tier: str
    win_start: float
    win_end: float

    if stages is None:
        # Whole-night fallback (no staging available)
        tier = "whole_night"
        win_start, win_end = session_start, session_end
    else:
        deep_episodes = [s for s in stages if getattr(s, "stage", None) == "deep"]

        if not deep_episodes:
            tier = "whole_night"
            win_start, win_end = session_start, session_end
        else:
            # Sort by start to find the LAST episode
            deep_sorted = sorted(deep_episodes, key=lambda s: s.start)
            last = deep_sorted[-1]
            last_dur = float(last.end) - float(last.start)

            if last_dur >= SWS_MIN_DURATION_S:
                # Primary: last SWS episode is long enough
                tier = "last_sws"
                win_start, win_end = float(last.start), float(last.end)
            else:
                # Secondary: check if other deep episodes collectively have enough
                # Use recency-weighted mean RMSSD over all SWS episodes
                tier = "all_sws"
                win_start = float(deep_sorted[0].start)
                win_end = float(deep_sorted[-1].end)

    # ---- Compute RMSSD for the chosen window ---------------------------
    pnn50_chosen: float
    mean_nn_chosen: float
    if tier == "whole_night":
        # Reuse the already-computed whole-night result — no double computation.
        rmssd_chosen = wn_rmssd
        n_beats = wn_n_beats
        n_art = wn_n_art
        pnn50_chosen = wn_pnn50
        mean_nn_chosen = wn_mean_nn
    elif tier == "last_sws":
        rmssd_chosen, n_beats, n_art, pnn50_chosen, mean_nn_chosen = _compute_rmssd_window(win_start, win_end)
    else:
        # all_sws: recency-weighted mean RMSSD over all SWS episodes
        # pnn50/mean_nn are not weighted across episodes; compute from the full SWS window for QC
        rmssd_chosen, n_beats, n_art = _compute_all_sws_recency_weighted(
            deep_episodes, wn_rr, wn_ts, session_start, session_end
        )
        _, _, _, pnn50_chosen, mean_nn_chosen = _compute_rmssd_window(win_start, win_end)

    return {
        "rmssd": rmssd_chosen,
        "tier": tier,
        "window_start": win_start,
        "window_end": win_end,
        "n_beats": n_beats,
        "n_artifacts": n_art,
        "rmssd_whole_night": rmssd_wn,
        "pnn50": pnn50_chosen,
        "mean_nn": mean_nn_chosen,
    }


def _compute_all_sws_recency_weighted(
    deep_episodes: Sequence[Any],
    wn_rr: np.ndarray,
    wn_ts: np.ndarray,
    session_start: float,
    session_end: float,
) -> tuple[float, int, int]:
    """Recency-weighted mean RMSSD over all SWS episodes.

    Each episode is weighted by its recency (later episodes get higher weight).
    Uses the segment-first approach: gaps within an episode are detected on the
    original epoch-second timestamps before cleaning, consistent with the
    window computation in _compute_rmssd_window.

    Returns (weighted_rmssd, total_n_beats, total_n_artifacts).
    """
    episodes_sorted = sorted(deep_episodes, key=lambda s: s.start)
    n_episodes = len(episodes_sorted)
    if n_episodes == 0:
        return float("nan"), 0, 0

    # Weights: 1, 2, 3, … n (later = higher weight)
    weights = list(range(1, n_episodes + 1))

    ep_rmssd_vals: list[float] = []
    ep_weights: list[float] = []
    total_beats = 0
    total_art = 0

    for ep, w in zip(episodes_sorted, weights):
        ep_start = float(ep.start)
        ep_end = float(ep.end)
        mask = (wn_ts >= ep_start) & (wn_ts <= ep_end)
        rr_ep = wn_rr[mask]
        ts_ep = wn_ts[mask]
        if rr_ep.size == 0:
            continue

        # Segment-first: split on original timestamps, clean per segment,
        # pool squared diffs across segments within the episode.
        if ts_ep.size > 1:
            time_gaps = np.diff(ts_ep)
            split_at = np.where(time_gaps > GAP_THRESHOLD_S)[0] + 1
        else:
            split_at = np.array([], dtype=np.intp)
        seg_starts_ep = np.concatenate([[0], split_at])
        seg_ends_ep = np.concatenate([split_at, [rr_ep.size]])

        ep_sq_diffs: list[float] = []
        for s, e in zip(seg_starts_ep, seg_ends_ep):
            seg_rr = rr_ep[s:e]
            nn_seg, _peaks_seg, n_b, n_a = clean_rr(seg_rr)
            total_beats += n_b
            total_art += n_a
            if nn_seg.size < 2:
                continue
            sq = np.diff(nn_seg) ** 2
            ep_sq_diffs.extend(sq.tolist())

        if not ep_sq_diffs:
            continue
        r = float(np.sqrt(np.mean(ep_sq_diffs)))
        if not math.isnan(r):
            ep_rmssd_vals.append(r)
            ep_weights.append(float(w))

    if not ep_rmssd_vals:
        return float("nan"), total_beats, total_art

    # Weighted mean
    weighted = sum(v * w for v, w in zip(ep_rmssd_vals, ep_weights))
    weight_sum = sum(ep_weights)
    return weighted / weight_sum, total_beats, total_art


# ---------------------------------------------------------------------------
# rmssd — backward-compatible public wrapper (range-filters, then rmssd_ms)
# ---------------------------------------------------------------------------

def rmssd(rr_ms: Sequence[int | float]) -> float:
    """Root mean square of successive RR differences (RMSSD) in ms.

    Algorithm:
      1. Filter to physiologically-plausible RR (300–2000 ms).
      2. Compute squared successive differences: ``(rr[i+1] - rr[i])^2``.
      3. Return ``sqrt(mean(squared_diffs))``.

    Convention — fewer than 2 valid RR intervals after filtering:
      Raises ``ValueError``.  Callers that want a sentinel should catch it.

    Args:
        rr_ms: Sequence of RR-interval measurements in milliseconds.

    Returns:
        RMSSD in milliseconds (float).

    Raises:
        ValueError: if fewer than 2 physiologically-plausible RR intervals
            remain after filtering.
    """
    rr_ms_list = list(rr_ms)
    nn = _filter_rr(rr_ms_list)
    if len(nn) < 2:
        raise ValueError(
            f"rmssd requires ≥2 plausible RR intervals (got {len(nn)} "
            f"after filtering {len(rr_ms_list)} inputs to [{RR_MIN_MS}, {RR_MAX_MS}] ms)"
        )
    return rmssd_ms(nn)


# ---------------------------------------------------------------------------
# SDNN
# ---------------------------------------------------------------------------

def sdnn(rr_ms: Sequence[int | float]) -> float:
    """Standard deviation of NN (filtered RR) intervals in ms.

    Uses the *sample* standard deviation (ddof=1 to match neurokit2 HRV_SDNN /
    sample SD).  Do NOT change to ddof=0.

    Convention — fewer than 2 valid RR intervals after filtering:
      Raises ``ValueError``.

    Args:
        rr_ms: Sequence of RR-interval measurements in milliseconds.

    Returns:
        SDNN in milliseconds (float, sample stdev / ddof=1).

    Raises:
        ValueError: if fewer than 2 physiologically-plausible RR intervals
            remain after filtering.
    """
    rr_ms_list = list(rr_ms)
    nn = _filter_rr(rr_ms_list)
    if len(nn) < 2:
        raise ValueError(
            f"sdnn requires ≥2 plausible RR intervals (got {len(nn)} "
            f"after filtering {len(rr_ms_list)} inputs to [{RR_MIN_MS}, {RR_MAX_MS}] ms)"
        )
    # ddof=1 to match neurokit2 HRV_SDNN / sample SD — do NOT change to ddof=0.
    return statistics.stdev(nn)  # sample stdev (ddof=1)


# ---------------------------------------------------------------------------
# Windowed HRV series (charting / time-series; backward-compatible)
# ---------------------------------------------------------------------------

def hrv_series(
    rr_rows: Sequence[dict[str, Any]],
    window_s: float,
) -> list[dict[str, float]]:
    """Compute a time-windowed RMSSD series from a sequence of RR records.

    Row shape:
      Each element of ``rr_rows`` must be a dict with:
        ``"ts"``    — Unix timestamp in seconds (int or float).
        ``"rr_ms"`` — RR-interval measurement in milliseconds (int or float).

    Window strategy — **tumbling (non-overlapping)**:
      The time axis is divided into non-overlapping windows of ``window_s``
      seconds starting from the timestamp of the first row.  Each window
      collects all RR values whose ``ts`` falls within
      ``[window_start, window_start + window_s)``.  RMSSD is computed from
      plausible values in that window.  Windows with fewer than 2 plausible
      values are skipped (no output point).

    Output:
      Each output dict contains:
        ``"ts"``    — Start timestamp of the window (same units as input).
        ``"rmssd"`` — RMSSD for that window, in milliseconds (float).

    Args:
        rr_rows:  Time-ordered sequence of ``{"ts": float, "rr_ms": int}`` dicts.
        window_s: Window duration in seconds (must be > 0).

    Returns:
        List of ``{"ts": float, "rmssd": float}`` dicts, one per non-empty window,
        in chronological order.

    Raises:
        ValueError: if ``window_s`` ≤ 0 or any row is missing required keys.
    """
    if window_s <= 0:
        raise ValueError(f"window_s must be > 0, got {window_s}")
    if not rr_rows:
        return []

    # Validate required keys before processing
    for row in rr_rows:
        if "ts" not in row:
            raise ValueError("hrv_series row missing required key 'ts'")
        if "rr_ms" not in row:
            raise ValueError("hrv_series row missing required key 'rr_ms'")

    # Validate and sort by ts (accept pre-sorted input; sort defensively)
    rows = sorted(rr_rows, key=lambda r: r["ts"])

    t_start = float(rows[0]["ts"])
    t_end   = float(rows[-1]["ts"])

    results: list[dict[str, float]] = []
    win_start = t_start

    while win_start <= t_end:
        win_end = win_start + window_s
        bucket = [
            r["rr_ms"]
            for r in rows
            if win_start <= float(r["ts"]) < win_end
        ]
        try:
            r = rmssd(bucket)
            results.append({"ts": win_start, "rmssd": r})
        except ValueError:
            pass  # <2 valid RR in this window — skip
        win_start = win_end

    return results
