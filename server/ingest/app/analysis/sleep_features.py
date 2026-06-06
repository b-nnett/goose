"""
sleep_features.py — epoch gridding, activity counts, Cole–Kripke sleep/wake,
cardiorespiratory feature extraction, the transparent staging classifier, and
hypnogram post-processing for the sleep-staging rewrite (Task 8).

This module is the FEATURE + CLASSIFIER engine; ``sleep.py`` remains the public
entry point (``detect_sleep`` / ``SleepSession`` / ``daily_sleep_summary``) and
imports from here. Splitting keeps each piece small and independently testable.

Design follows ``docs/research/02-sleep-staging.md`` "Option A structure WITHOUT
sleepecg/TensorFlow": keep the gravity / Cole–Kripke sleep-wake spine, then layer
a transparent cardiorespiratory staging classifier on top. NO sleepecg, NO
TensorFlow (deploy weight + unvalidatable without ground truth). neurokit2 /
scipy / numpy only.

HONEST HEDGING
--------------
These stages are WHOOP-LIKE APPROXIMATIONS, not WHOOP-identical and not
PSG-validated. The literature ceiling for EEG-free 4-class staging is ~65–73%
epoch agreement (Walch 2019; sleepecg). **Light/deep separation is the weakest
link** — the cardiac signal barely distinguishes N1/N2/N3, so deep-minute
estimates are the least reliable output. We have NO ground truth yet, so the
classifier is validated for *physiological plausibility and internal
consistency*, NOT against WHOOP.

Pipeline (epoch = 30 s):
  Stage 0  in-bed/sleep-wake spine (gravity stillness + Cole–Kripke) — sleep.py
  Stage 1  cardiorespiratory features per epoch over a rolling 5-min window  ← here
  Stage 2  transparent classifier features -> {wake,light,deep,rem}          ← here
  Stage 3  smoothing + physiology re-imposition                              ← here

THE CLASSIFIER SEAM
-------------------
``classify_epochs(features) -> list[str]`` is the deterministic Stage-2 rule. It
is a clean function seam: a trained model (sleepecg GRU / LightGBM, per research
§4 Option A/B) can replace this exact function later — feed it the same
``EpochFeatures`` list, return the same per-epoch label list, and the rest of the
pipeline (smoothing, physiology, segment building, AASM metrics) is unchanged.
"""
from __future__ import annotations

import math
import statistics
from dataclasses import dataclass
from typing import Sequence

import numpy as np

# ===========================================================================
# Epoch / windowing constants
# ===========================================================================

#: Epoch length (seconds). AASM/actigraphy/Walch/sleepecg all use 30 s.
EPOCH_S: float = 30.0
#: Rolling feature window (seconds) centered on each epoch. Walch used 10 min;
#: HRV frequency features need >= ~2 min. 5 min is the spec's recommendation —
#: enough beats for RMSSD/HF while staying local.
FEATURE_WINDOW_S: float = 5 * 60.0

# --- Cole–Kripke activity-count scaling (te Lindert 30 s variant, §2.0/§2.1) -
#: Per-epoch activity count = sum of |Δgravity| over the epoch, then rescaled.
#: te Lindert 30 s form: divide by 100, then clip to 300.
CK_COUNT_DIVISOR: float = 100.0
CK_COUNT_CLIP: float = 300.0
#: Per-sample |Δgravity| (g) at/above which a gravity sample counts as "moving".
#: Mirrors sleep.GRAVITY_STILL_THRESHOLD_G so "moving" means the same thing as in
#: the sleep/wake spine. Used to build the scale-robust per-epoch move-fraction.
MOVE_DELTA_THRESHOLD_G: float = 0.01

# --- Walch HR-variability difference-of-Gaussians filter (§1 feature 2) ------
#: Minimum beats in a window before computing frequency-domain HRV (LF/HF). The
#: HF band is 0.15–0.4 Hz → need ~2 min of beats for a meaningful spectrum
#: (spec §4 "where enough beats"). ~120 beats ≈ 2 min at 60 bpm.
FREQ_MIN_BEATS: int = 120

#: HRV/respiration windows slide only 30 s per epoch while spanning 5 min, so the
#: feature value changes negligibly between adjacent epochs. Compute the expensive
#: neurokit HRV + respiration features every ``HRV_STRIDE_EPOCHS`` epochs and
#: forward-fill to the epochs in between (the window content is ~90% shared). This
#: is a pure perf optimization — set to 1 to disable. 4 epochs = 2 min stride.
HRV_STRIDE_EPOCHS: int = 4

#: σ for the two Gaussians (seconds). Walch: σ1=120 s, σ2=600 s. The DoG of HR
#: amplifies rapid HR change; the windowed std of the result is the HR-variability
#: feature (a strong wake/REM signal — it is variability, NOT level).
HR_DOG_SIGMA1_S: float = 120.0
HR_DOG_SIGMA2_S: float = 600.0

# --- Staging classifier percentile bands (per-night-relative, §4 Stage 2) ----
#: HR percentile (over the sleep-period epochs) at/below which HR is "low".
STAGE_HR_LOW_PCT: float = 25.0
#: HR percentile at/above which HR is "elevated".
STAGE_HR_HIGH_PCT: float = 70.0
#: HF / RMSSD percentile at/above which parasympathetic tone is "high" (deep).
STAGE_HRV_HIGH_PCT: float = 70.0
#: HR-variability (Walch DoG std) percentile at/above which cardiac is "activated"
#: (REM / wake signature).
STAGE_HRV_VAR_HIGH_PCT: float = 65.0
#: Respiratory-rate-variability percentile at/above which respiration is
#: "irregular" (REM signature).
STAGE_RRV_HIGH_PCT: float = 65.0
#: Respiratory-rate-variability percentile at/below which respiration is "regular"
#: (deep signature).
STAGE_RRV_LOW_PCT: float = 50.0
# --- Motion gates on the epoch grid (use the scale-robust move-fraction) -----
#: An epoch is a movement/arousal candidate when this fraction (or more) of its
#: per-sample gravity deltas exceed the still threshold. Reuses the same
#: still-threshold semantics as the sleep/wake spine. Tuned against real overnight
#: data: ~0.15 surfaces genuine disturbances without flagging every still stir.
STAGE_WAKE_MOVE_FRAC: float = 0.15
#: At/below this moving-sample fraction the body is treated as essentially still
#: (a prerequisite for deep/REM, both low-movement stages).
STAGE_STILL_MOVE_FRAC: float = 0.10

# --- Post-processing (§4 Stage 3) -------------------------------------------
#: Median-smoothing window in epochs (must be odd). 5 epochs = 2.5 min — kills
#: isolated 30 s flips while preserving real stage blocks.
SMOOTH_EPOCHS: int = 5
#: No REM allowed in the first N minutes after sleep onset (physiology: REM
#: latency is typically 70–120 min; we use a conservative 15-min floor).
NO_REM_AFTER_ONSET_MIN: float = 15.0
#: Deep sleep is concentrated in the first third of the night; deep detected in
#: the last third is downgraded to light (physiology re-imposition, §4 Stage 3).
DEEP_FIRST_FRACTION: float = 1.0 / 3.0


# ===========================================================================
# Cole–Kripke sleep/wake  (§2.1, te Lindert 30 s coefficients)
# ===========================================================================

#: te Lindert 30 s Cole–Kripke weights for [A₋₄, A₋₃, A₋₂, A₋₁, A₀, A₊₁, A₊₂].
#: SI = 0.001 × Σ wᵢ·Aᵢ ; sleep iff SI < 1. Source: dipetkov/actigraph.sleepr
#: apply_cole_kripke.R. DO NOT change these — pinned by test against the cited form.
CK_WEIGHTS: tuple[float, ...] = (106.0, 54.0, 58.0, 76.0, 230.0, 74.0, 67.0)
CK_SCALE: float = 0.001
#: Window layout relative to the current epoch: 4 before, current, 2 after.
CK_BACK: int = 4
CK_FWD: int = 2


def rescale_counts(counts: Sequence[float]) -> list[float]:
    """Rescale raw per-epoch activity counts for Cole–Kripke: ÷100, clip to 300.

    te Lindert 30 s variant (§2.0). ``counts`` are the per-epoch summed |Δgravity|
    activity proxies. Returns the rescaled, clipped counts used by ``cole_kripke``.
    """
    return [min(float(c) / CK_COUNT_DIVISOR, CK_COUNT_CLIP) for c in counts]


def cole_kripke(rescaled_counts: Sequence[float]) -> list[bool]:
    """Per-epoch sleep flags via Cole–Kripke (te Lindert 30 s form, §2.1).

        SI = 0.001 × (106·A₋₄ + 54·A₋₃ + 58·A₋₂ + 76·A₋₁ + 230·A₀ + 74·A₊₁ + 67·A₊₂)
        epoch is SLEEP if SI < 1, else WAKE

    ``rescaled_counts`` must already be ÷100/clip-300 (use ``rescale_counts``).
    Out-of-range neighbours (start/end of record) contribute 0 (standard
    edge handling). Returns one bool per input epoch (True == sleep).
    """
    n = len(rescaled_counts)
    flags: list[bool] = []
    for i in range(n):
        si = 0.0
        for k, w in enumerate(CK_WEIGHTS):
            j = i - CK_BACK + k  # k=0 -> i-4 ... k=6 -> i+2
            a = rescaled_counts[j] if 0 <= j < n else 0.0
            si += w * a
        si *= CK_SCALE
        flags.append(si < 1.0)
    return flags


# ===========================================================================
# Epoch grid
# ===========================================================================

@dataclass
class EpochGrid:
    """A fixed 30 s epoch grid over [start, end) with the four streams bucketed.

    ``edges`` has ``n_epochs + 1`` entries (epoch i spans [edges[i], edges[i+1])).
    The per-stream lists are aligned to epoch index.
    """
    start: float
    end: float
    edges: list[float]
    #: per-epoch summed |Δgravity| activity proxy (raw, pre-rescale)
    counts: list[float]
    #: per-epoch fraction of gravity samples whose |Δ| >= the still threshold.
    #: This is the SCALE-ROBUST motion gate (see ``build_epoch_grid``): the
    #: Cole–Kripke ÷100 count scaling is calibrated for 30–50 Hz raw accel, but
    #: we only have ~30 gravity samples/epoch at 1 Hz, so even violent motion
    #: rescales to a small count. The moving-fraction is rate-independent and is
    #: what the wake/still gates actually use; the counts feed Cole–Kripke.
    move_frac: list[float]
    #: per-epoch mean HR (bpm) or NaN if no samples
    hr: list[float]
    #: per-epoch list of RR intervals (ms) whose ts fall in the epoch
    rr: list[list[float]]
    #: per-epoch list of raw respiration samples
    resp: list[list[float]]

    @property
    def n_epochs(self) -> int:
        return len(self.counts)

    def epoch_mid(self, i: int) -> float:
        return self.edges[i] + EPOCH_S / 2.0


def build_epoch_grid(
    start: float,
    end: float,
    grav_times: Sequence[float],
    grav_deltas: Sequence[float],
    hr: Sequence[dict],
    rr: Sequence[dict],
    resp: Sequence[dict],
) -> EpochGrid:
    """Resample all four streams onto a common 30 s epoch grid over [start, end).

    Activity count per epoch = Σ |Δgravity| of the gravity samples falling in the
    epoch (§2.0: "sum the per-sample motion proxy over each epoch"). Non-finite
    deltas (missing-sample sentinel) contribute a large fixed count so the epoch
    reads as motion.

    ``grav_times`` / ``grav_deltas`` are the parallel arrays from
    ``sleep._gravity_deltas`` (kept in sleep.py — reused, not duplicated).
    """
    if end <= start:
        return EpochGrid(start, end, [start], [], [], [], [], [])
    n_epochs = max(1, int(math.ceil((end - start) / EPOCH_S)))
    edges = [start + i * EPOCH_S for i in range(n_epochs + 1)]
    edges[-1] = max(edges[-1], end)

    counts = [0.0] * n_epochs
    move_n = [0] * n_epochs       # gravity samples this epoch with |Δ| >= still thr
    grav_n = [0] * n_epochs       # total gravity samples this epoch
    hr_sum = [0.0] * n_epochs
    hr_cnt = [0] * n_epochs
    rr_buckets: list[list[float]] = [[] for _ in range(n_epochs)]
    resp_buckets: list[list[float]] = [[] for _ in range(n_epochs)]

    def _idx(ts: float) -> int | None:
        if ts < start or ts >= end:
            if ts == end:
                return n_epochs - 1
            return None
        i = int((ts - start) // EPOCH_S)
        return min(i, n_epochs - 1)

    # Gravity → activity counts + scale-robust move-fraction
    BIG = CK_COUNT_CLIP * CK_COUNT_DIVISOR  # ensures rescale → clipped max
    for ts, d in zip(grav_times, grav_deltas):
        i = _idx(ts)
        if i is None:
            continue
        moving = (not math.isfinite(d)) or float(d) >= MOVE_DELTA_THRESHOLD_G
        counts[i] += BIG if not math.isfinite(d) else float(d)
        grav_n[i] += 1
        if moving:
            move_n[i] += 1

    for r in hr:
        b = r.get("bpm")
        if b is None:
            continue
        i = _idx(float(r["ts"]))
        if i is None:
            continue
        hr_sum[i] += float(b)
        hr_cnt[i] += 1

    for r in rr:
        v = r.get("rr_ms")
        if v is None:
            continue
        i = _idx(float(r["ts"]))
        if i is None:
            continue
        rr_buckets[i].append(float(v))

    for r in resp:
        v = r.get("raw")
        if v is None:
            continue
        i = _idx(float(r["ts"]))
        if i is None:
            continue
        resp_buckets[i].append(float(v))

    hr_mean = [
        (hr_sum[i] / hr_cnt[i]) if hr_cnt[i] else float("nan")
        for i in range(n_epochs)
    ]
    # Move-fraction: epochs with no gravity coverage default to 1.0 (treat as
    # moving / not-still, conservative — matches the old classifier).
    move_frac = [
        (move_n[i] / grav_n[i]) if grav_n[i] else 1.0
        for i in range(n_epochs)
    ]
    return EpochGrid(start, end, edges, counts, move_frac, hr_mean,
                     rr_buckets, resp_buckets)


# ===========================================================================
# Walch difference-of-Gaussians HR-variability feature  (§1 feature 2)
# ===========================================================================

def _gaussian_kernel(sigma_s: float, dt_s: float = EPOCH_S) -> np.ndarray:
    """A normalized 1-D Gaussian kernel with σ given in seconds, sampled at the
    epoch spacing ``dt_s``. Truncated at ±3σ."""
    sigma = max(sigma_s / dt_s, 1e-6)  # σ in epochs
    radius = max(1, int(math.ceil(3 * sigma)))
    x = np.arange(-radius, radius + 1, dtype=float)
    k = np.exp(-0.5 * (x / sigma) ** 2)
    k /= k.sum()
    return k


def dog_hr_variability(hr_per_epoch: Sequence[float]) -> np.ndarray:
    """Difference-of-Gaussians filtered HR (Walch §1 feature 2).

    HR interpolated to the epoch grid, then filtered with DoG (σ1=120 s minus
    σ2=600 s) to amplify rapid HR change. The per-epoch local std of this signal
    (over the feature window) is the actual classifier feature; this function
    returns the DoG-filtered series itself.

    NaNs (epochs with no HR) are filled by linear interpolation before filtering
    so the convolution is well-defined; if there is no HR at all, returns zeros.
    """
    hr = np.asarray(hr_per_epoch, dtype=float)
    n = hr.size
    if n == 0:
        return np.zeros(0)
    mask = ~np.isnan(hr)
    if not mask.any():
        return np.zeros(n)
    idx = np.arange(n)
    hr_filled = np.interp(idx, idx[mask], hr[mask])

    k1 = _gaussian_kernel(HR_DOG_SIGMA1_S)
    k2 = _gaussian_kernel(HR_DOG_SIGMA2_S)
    g1 = _convolve_reflect(hr_filled, k1)
    g2 = _convolve_reflect(hr_filled, k2)
    return g1 - g2


def _convolve_reflect(x: np.ndarray, kernel: np.ndarray) -> np.ndarray:
    """Same-length convolution with reflect padding (edge-stable)."""
    r = len(kernel) // 2
    if r == 0 or x.size == 0:
        return x.copy()
    padded = np.pad(x, r, mode="reflect")
    return np.convolve(padded, kernel, mode="valid")[: x.size]


# ===========================================================================
# Respiration rate + RRV from the raw 1 Hz resp signal
# ===========================================================================

def resp_rate_and_rrv(resp_raw: Sequence[float], dt_s: float = 1.0) -> tuple[float, float]:
    """Estimate respiratory rate (breaths/min) and RRV from a raw resp window.

    Our resp channel is a coarse ~1 Hz raw a.u. signal — neurokit2's ``rsp_*``
    pipeline is unreliable at this rate, so we derive both robustly ourselves
    (research §4 explicitly allows "else derive variability robustly"):

      - Detrend (subtract mean), find zero-crossing / peak intervals to get
        breath-to-breath intervals.
      - Respiratory rate = 60 / median breath interval.
      - RRV = std of breath-to-breath intervals (s) — higher == more irregular
        (the REM signature, §3).

    Returns (rate_bpm, rrv_s). Returns (nan, nan) if too few samples / no clear
    breathing rhythm. Pure numpy/scipy — no neurokit dependency for this path.
    """
    x = np.asarray(resp_raw, dtype=float)
    if x.size < 8:
        return float("nan"), float("nan")
    x = x - np.mean(x)
    if np.allclose(x, 0.0):
        return float("nan"), float("nan")

    # Peak detection via local maxima above a small fraction of the signal's std.
    # (scipy.find_peaks gives breath peaks; intervals between them = breaths.)
    from scipy.signal import find_peaks

    std = np.std(x)
    if std <= 0:
        return float("nan"), float("nan")
    # A breath at ~6–30 br/min over a 1 Hz signal => >=2 s between peaks.
    peaks, _ = find_peaks(x, distance=max(2, int(round(2.0 / dt_s))), height=0.0)
    if peaks.size < 3:
        return float("nan"), float("nan")
    intervals_s = np.diff(peaks) * dt_s
    intervals_s = intervals_s[(intervals_s >= 1.5) & (intervals_s <= 12.0)]
    if intervals_s.size < 2:
        return float("nan"), float("nan")
    rate = 60.0 / float(np.median(intervals_s))
    rrv = float(np.std(intervals_s))
    return rate, rrv


# ===========================================================================
# HRV from RR (neurokit2)
# ===========================================================================

def hrv_from_rr(rr_ms: Sequence[float]) -> dict[str, float]:
    """Time + frequency HRV from a window of RR intervals (ms) via neurokit2.

    Uses ``nk.hrv_time`` (RMSSD, SDNN) and ``nk.hrv_frequency`` (LF, HF, LF/HF).
    Short windows are handled gracefully: frequency features need a longer record
    and are returned as NaN when neurokit can't compute them. Returns a dict with
    keys rmssd, sdnn, lf, hf, lfhf (all ms²/ms/ratio per neurokit); missing => NaN.

    We compute on the RAW RR (range-filtered) rather than the full Kubios clean
    pipeline because this runs per 30 s epoch over a 5-min window many times; the
    range filter removes gross artifacts cheaply. (The nightly RMSSD that feeds
    recovery still uses the full ``hrv.clean_rr`` path elsewhere.)
    """
    nan = float("nan")
    out = {"rmssd": nan, "sdnn": nan, "lf": nan, "hf": nan, "lfhf": nan}
    vals = [float(v) for v in rr_ms if 300.0 <= float(v) <= 2000.0]
    if len(vals) < 5:
        return out

    import warnings
    import neurokit2 as nk

    arr = np.asarray(vals, dtype=float)
    try:
        peaks = nk.intervals_to_peaks(arr, sampling_rate=1000)
    except Exception:
        return out

    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            t = nk.hrv_time(peaks, sampling_rate=1000)
            out["rmssd"] = _df_get(t, "HRV_RMSSD")
            out["sdnn"] = _df_get(t, "HRV_SDNN")
        except Exception:
            pass
        # Frequency features (LF/HF) need a longer, denser record: the HF band is
        # 0.15–0.4 Hz, so we need at least ~2 min of beats for a meaningful
        # spectrum (spec §4: "where enough beats"). Below that, skip the (~10×
        # more expensive) frequency call and leave LF/HF as NaN — this is both
        # the physiologically-honest choice and a real per-epoch perf win.
        if len(vals) >= FREQ_MIN_BEATS:
            try:
                f = nk.hrv_frequency(peaks, sampling_rate=1000)
                out["lf"] = _df_get(f, "HRV_LF")
                out["hf"] = _df_get(f, "HRV_HF")
                out["lfhf"] = _df_get(f, "HRV_LFHF")
            except Exception:
                pass
    return out


def _df_get(df, col: str) -> float:
    try:
        v = float(df[col].iloc[0])
        return v if math.isfinite(v) else float("nan")
    except Exception:
        return float("nan")


# ===========================================================================
# Per-epoch feature extraction  (Stage 1, §3/§4)
# ===========================================================================

@dataclass
class EpochFeatures:
    """Cardiorespiratory + motion + clock features for one 30 s epoch.

    All "_pct" gating happens in the classifier against the session distribution;
    these are the raw per-epoch values. NaN means "not enough data this epoch".
    """
    index: int
    mid_ts: float
    count: float          # rescaled Cole–Kripke activity count
    move_frac: float      # fraction of moving gravity samples (scale-robust gate)
    ck_sleep: bool        # Cole–Kripke sleep flag for this epoch
    hr: float             # mean HR over the feature window (bpm)
    hr_var: float         # Walch DoG-HR windowed std (HR-variability feature)
    rmssd: float          # ms, over the feature window
    sdnn: float           # ms
    hf: float             # HRV high-frequency power
    lfhf: float           # LF/HF ratio
    resp_rate: float      # breaths/min over the feature window
    rrv: float            # respiratory-rate variability (s)
    clock: float          # normalized time since sleep onset, 0..1


def extract_features(
    grid: EpochGrid,
    ck_flags: Sequence[bool],
    dog_hr: Sequence[float],
    onset_idx: int,
    final_wake_idx: int,
) -> list[EpochFeatures]:
    """Build per-epoch ``EpochFeatures`` over a rolling FEATURE_WINDOW_S window.

    For each epoch the window is centered on the epoch and spans ±FEATURE_WINDOW_S/2
    (clamped to the grid). HRV/resp features are computed over the pooled samples
    in that window; HR is the window-mean; ``hr_var`` is the std of the
    DoG-filtered HR over the window (Walch design). ``clock`` is the normalized
    time-since-onset within the sleep period [onset_idx, final_wake_idx].
    """
    n = grid.n_epochs
    rescaled = rescale_counts(grid.counts)
    half_w = int(round((FEATURE_WINDOW_S / EPOCH_S) / 2))
    dog = np.asarray(dog_hr, dtype=float)
    span = max(1, final_wake_idx - onset_idx)

    # The expensive neurokit HRV + respiration features are computed only on
    # strided "anchor" epochs and forward-filled (window content barely changes
    # over 30 s — see HRV_STRIDE_EPOCHS). HR mean / DoG-std / move-frac are cheap
    # and computed every epoch.
    stride = max(1, HRV_STRIDE_EPOCHS)
    hrv_cache: dict[str, float] = {"rmssd": float("nan"), "sdnn": float("nan"),
                                   "hf": float("nan"), "lfhf": float("nan")}
    resp_cache: tuple[float, float] = (float("nan"), float("nan"))

    feats: list[EpochFeatures] = []
    for i in range(n):
        lo = max(0, i - half_w)
        hi = min(n, i + half_w + 1)

        win_hr = [grid.hr[j] for j in range(lo, hi) if not math.isnan(grid.hr[j])]
        hr_mean = float(np.mean(win_hr)) if win_hr else float("nan")

        win_dog = dog[lo:hi] if dog.size else np.zeros(0)
        hr_var = float(np.std(win_dog)) if win_dog.size >= 2 else float("nan")

        if i % stride == 0:
            win_rr: list[float] = []
            for j in range(lo, hi):
                win_rr.extend(grid.rr[j])
            hrv_cache = hrv_from_rr(win_rr)

            win_resp: list[float] = []
            for j in range(lo, hi):
                win_resp.extend(grid.resp[j])
            resp_cache = resp_rate_and_rrv(win_resp)

        clock = min(1.0, max(0.0, (i - onset_idx) / span))

        feats.append(EpochFeatures(
            index=i,
            mid_ts=grid.epoch_mid(i),
            count=rescaled[i],
            move_frac=grid.move_frac[i],
            ck_sleep=bool(ck_flags[i]) if i < len(ck_flags) else True,
            hr=hr_mean,
            hr_var=hr_var,
            rmssd=hrv_cache["rmssd"],
            sdnn=hrv_cache["sdnn"],
            hf=hrv_cache["hf"],
            lfhf=hrv_cache["lfhf"],
            resp_rate=resp_cache[0],
            rrv=resp_cache[1],
            clock=clock,
        ))
    return feats


# ===========================================================================
# Percentile helper (session-relative bands)
# ===========================================================================

def _pct(values: Sequence[float], pct: float) -> float | None:
    vals = sorted(v for v in values if v is not None and math.isfinite(v))
    if not vals:
        return None
    return float(np.percentile(vals, pct))


# ===========================================================================
# THE CLASSIFIER SEAM — Stage 2 (§4)
# ===========================================================================

def classify_epochs(features: Sequence[EpochFeatures]) -> list[str]:
    """Per-epoch staging classifier: features -> ["wake"|"light"|"deep"|"rem", ...].

    ===================  THE MODEL SEAM  ===================
    This is the deterministic Stage-2 rule from research §4. It is the swap point:
    a trained model (sleepecg GRU / LightGBM, §4 Option A/B) can replace THIS
    FUNCTION verbatim — same input (``EpochFeatures`` list), same output (per-epoch
    label list) — and the rest of the pipeline is untouched. Keep this signature
    stable.
    ========================================================

    Physiology encoded (research §3):
      wake  = motion (high activity count) AND elevated cardiac (HR high OR
              HR-variability high), OR motion with no HR to vet it.
      deep  = still body + HIGH parasympathetic tone (high HF/RMSSD, session high
              pct) + LOW HR (session low pct) + REGULAR respiration (low RRV).
      rem   = still body + ACTIVATED cardiac (HR up OR HR-variability up) +
              IRREGULAR respiration (high RRV).
      light = everything else (the bulk of the night; also the honest default
              when HRV/resp are missing).

    Bands are PER-NIGHT-RELATIVE percentiles computed over the SLEEP-PERIOD epochs
    (Cole–Kripke sleep == True) so the classifier adapts to the wearer/night, not
    to absolute thresholds. **Deep/light separation is the weakest link** — see
    the module docstring.
    """
    n = len(features)
    if n == 0:
        return []

    # Session-relative reference distributions, computed over SLEEP-PERIOD epochs
    # (so wake epochs don't skew the cardiac bands).
    sleep_feats = [f for f in features if f.ck_sleep] or list(features)
    hr_vals = [f.hr for f in sleep_feats]
    hf_vals = [f.hf for f in sleep_feats]
    rmssd_vals = [f.rmssd for f in sleep_feats]
    hrvar_vals = [f.hr_var for f in sleep_feats]
    rrv_vals = [f.rrv for f in sleep_feats]

    hr_lo = _pct(hr_vals, STAGE_HR_LOW_PCT)
    hr_hi = _pct(hr_vals, STAGE_HR_HIGH_PCT)
    hf_hi = _pct(hf_vals, STAGE_HRV_HIGH_PCT)
    rmssd_hi = _pct(rmssd_vals, STAGE_HRV_HIGH_PCT)
    hrvar_hi = _pct(hrvar_vals, STAGE_HRV_VAR_HIGH_PCT)
    rrv_hi = _pct(rrv_vals, STAGE_RRV_HIGH_PCT)
    rrv_lo = _pct(rrv_vals, STAGE_RRV_LOW_PCT)

    labels: list[str] = []
    for f in features:
        labels.append(_classify_one(
            f, hr_lo, hr_hi, hf_hi, rmssd_hi, hrvar_hi, rrv_hi, rrv_lo,
        ))
    return labels


def _classify_one(
    f: EpochFeatures,
    hr_lo: float | None,
    hr_hi: float | None,
    hf_hi: float | None,
    rmssd_hi: float | None,
    hrvar_hi: float | None,
    rrv_hi: float | None,
    rrv_lo: float | None,
) -> str:
    """Classify one epoch from session-relative bands. See ``classify_epochs``."""
    has_hr = math.isfinite(f.hr)
    hr_low = has_hr and hr_lo is not None and f.hr <= hr_lo
    hr_high = has_hr and hr_hi is not None and f.hr >= hr_hi

    hf_high = math.isfinite(f.hf) and hf_hi is not None and f.hf >= hf_hi
    rmssd_high = math.isfinite(f.rmssd) and rmssd_hi is not None and f.rmssd >= rmssd_hi
    parasymp_high = hf_high or rmssd_high

    hrvar_high = math.isfinite(f.hr_var) and hrvar_hi is not None and f.hr_var >= hrvar_hi
    cardiac_activated = hr_high or hrvar_high

    rrv_irregular = math.isfinite(f.rrv) and rrv_hi is not None and f.rrv >= rrv_hi
    # Deliberate pro-deep bias: missing respiration (NaN RRV) is treated as
    # "regular" rather than "unknown". Deep can be classified from stillness +
    # high parasympathetic tone + low HR alone when no resp data is present;
    # this degrades gracefully (no resp → deep still reachable, REM harder).
    rrv_regular = (not math.isfinite(f.rrv)) or (rrv_lo is not None and f.rrv <= rrv_lo)

    # Motion gate: scale-robust move-fraction (NOT the Cole–Kripke ÷100 count,
    # which under-reads at our 1 Hz sample rate — see EpochGrid.move_frac).
    still = f.move_frac <= STAGE_STILL_MOVE_FRAC
    moving = f.move_frac >= STAGE_WAKE_MOVE_FRAC

    # ── WAKE ──: sustained motion + activated cardiac (or no HR to vet the
    # motion). A still stir with calm HR is NOT wake — keeps disturbances real.
    if moving and (cardiac_activated or not has_hr):
        return "wake"

    # ── DEEP ──: still + high parasympathetic tone + low HR + regular respiration.
    if still and parasymp_high and hr_low and rrv_regular:
        return "deep"

    # ── REM ──: still body + activated cardiac + irregular respiration.
    if still and cardiac_activated and rrv_irregular:
        return "rem"
    # REM fallback when respiration is unavailable (no RRV to test irregularity):
    # require BOTH cardiac-activation signals — HR elevated AND HR-variability up
    # — the strong "still body, wake-like cardiac" tell. Single-signal elevation
    # alone stays light (avoids over-calling REM on flat-resp data).
    if still and hr_high and hrvar_high and not math.isfinite(f.rrv):
        return "rem"

    return "light"


# ===========================================================================
# Post-processing — Stage 3 (§4)
# ===========================================================================

def smooth_labels(labels: Sequence[str], window: int = SMOOTH_EPOCHS) -> list[str]:
    """Majority/median smoothing over the epoch label sequence (kills isolated
    30 s flips). Centered window of ``window`` epochs (odd); ties keep the
    incumbent label (autocorrelation-preserving)."""
    n = len(labels)
    if n == 0 or window <= 1:
        return list(labels)
    if window % 2 == 0:
        window += 1
    half = window // 2
    out: list[str] = []
    for i in range(n):
        lo = max(0, i - half)
        hi = min(n, i + half + 1)
        win = labels[lo:hi]
        counts: dict[str, int] = {}
        for s in win:
            counts[s] = counts.get(s, 0) + 1
        best = max(counts.values())
        winners = [s for s, c in counts.items() if c == best]
        # Incumbent-preservation handles the common 2-way tie. On a full 3-way
        # tie where the incumbent is not in the top-2, winners[0] (first by
        # majority-count insertion order) is used deterministically.
        out.append(labels[i] if labels[i] in winners else winners[0])
    return out


def reimpose_physiology(
    labels: Sequence[str],
    features: Sequence[EpochFeatures],
    onset_idx: int,
    final_wake_idx: int,
) -> list[str]:
    """Re-impose sleep physiology on the hypnogram (§4 Stage 3):

      - No REM in the first NO_REM_AFTER_ONSET_MIN after sleep onset → relabel
        such early-REM epochs to light.
      - Deep is concentrated in the first third of the night → deep detected in
        the last third (by clock) is downgraded to light.

    Operates on epoch labels; ``onset_idx``/``final_wake_idx`` bound the sleep
    period. Returns a new label list.
    """
    out = list(labels)
    no_rem_epochs = int(round((NO_REM_AFTER_ONSET_MIN * 60.0) / EPOCH_S))
    for i, f in enumerate(features):
        if i < onset_idx or i > final_wake_idx:
            continue
        if out[i] == "rem" and (i - onset_idx) < no_rem_epochs:
            out[i] = "light"
        if out[i] == "deep" and f.clock > DEEP_FIRST_FRACTION:
            out[i] = "light"
    return out
