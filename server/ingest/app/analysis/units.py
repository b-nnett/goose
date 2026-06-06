"""
units.py — Real-unit conversions for WHOOP V24 biometric ADC readings.

ALL OUTPUTS ARE APPROXIMATE / UN-CALIBRATED until calibration-fitting routines
in this module have been run against WHOOP-exported ground-truth values.
See the fit_spo2(), fit_skin_temp(), and leave_one_night_out() functions below.

Overview of estimators
──────────────────────
SpO2
    Windowed ratio-of-ratios (AC=MAD-based robust spread, DC=mean) over a
    rolling window (~60 s default).  Single-sample fallback for callers that
    pass one point.  Formula: SpO2 = a − b·R, clamped [70, 100].
    Default (a, b) = (110, 25) are the canonical TI SLAA655 textbook values.
    They are NOT device-calibrated — fit with fit_spo2() when WHOOP export
    data is available.

Skin temperature
    Single-slope linear map raw → °C.  Deviation-from-baseline (offset-free)
    is the more trustworthy output and is what WHOOP actually surfaces.
    Default slope/offset are un-calibrated; fit with fit_skin_temp().

Respiration rate
    Welch-peak spectral estimator (dominant frequency in 0.1–0.5 Hz band).
    No free parameters to fit; tune band/window on a few nights.
    NeuroKit2 xcorr is attempted first on long windows; Welch is the fallback
    and primary path for the coarse 1 Hz field.

References
──────────
- TI SLAA655 (SpO2 = 110 − 25·R, AC/DC): https://www.ti.com/lit/an/slaa655/slaa655.pdf
- Mendelson & Ochs 1988 IEEE TBME (reflectance pulse oximetry, per-subject
  linear R↔SpO2): DOI 10.1109/10.7288
- npj Digital Medicine 2021 (overnight RR from wearables, RMSE ~0.65 BrPM):
  https://www.nature.com/articles/s41746-021-00493-6
- Ametherm NTC / Steinhart-Hart (thermistor physics background):
  https://www.ametherm.com/thermistor/ntc-thermistors-steinhart-and-hart-equation/

Real WHOOP devices compute these values in the cloud with proprietary
calibration. These implementations are reverse-engineered approximations for
research use only.

Reference ADC values from real V24 samples (captured 2026-05-24):
  spo2_red ≈ 587, spo2_ir ≈ 585, skin_temp_raw ≈ 930
"""
from __future__ import annotations

import logging
import statistics
from typing import Sequence, Tuple, Optional

import numpy as np
from scipy.signal import welch

logger = logging.getLogger(__name__)

# ═══════════════════════════════════════════════════════════════════════════════
# §0  Internal helpers
# ═══════════════════════════════════════════════════════════════════════════════

def _clamp(value: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, value))


def _mad(arr: np.ndarray) -> float:
    """Median Absolute Deviation (unscaled)."""
    return float(np.median(np.abs(arr - np.median(arr))))


def _robust_spread(arr: np.ndarray) -> float:
    """
    Robust AC surrogate: 1.4826 * MAD (scaled to approximate σ for a Gaussian).
    Falls back to IQR if MAD is zero (constant region).
    Returns 0.0 for arrays with fewer than 2 elements.
    """
    if len(arr) < 2:
        return 0.0
    mad = _mad(arr)
    if mad > 0.0:
        return 1.4826 * mad
    # MAD = 0 → data is at median; IQR as secondary proxy
    return float(np.percentile(arr, 75) - np.percentile(arr, 25))


# ═══════════════════════════════════════════════════════════════════════════════
# §1  SpO2 — ratio-of-ratios
# ═══════════════════════════════════════════════════════════════════════════════

# ── Published default calibration constants ─────────────────────────────────
#
# SpO2 = SPO2_A − SPO2_B · R   (R = ratio-of-ratios; see §1.3 of spec)
#
# Source: TI SLAA655 canonical textbook values (a=110, b=25).
# IMPORTANT — UN-FITTED APPROXIMATION:
#   These are device-generic starting points only.  For a reflectance wrist
#   sensor like WHOOP, the true (a, b) depend on LED wavelengths, photodiode
#   spectral response, optics geometry, and whatever the firmware already did
#   to the 1 Hz field.  Published constants from other sensors are NOT portable.
#   Expect errors of several % SpO2 until calibrated.
#
# To calibrate: call fit_spo2(R_values, whoop_spo2) once WHOOP export data
# is available, and replace these constants with the returned (a, b).
# ─────────────────────────────────────────────────────────────────────────────
SPO2_A: float = 110.0   # [APPROXIMATE — un-calibrated] linear intercept, % SpO2
SPO2_B: float = 25.0    # [APPROXIMATE — un-calibrated] linear slope (> 0 required)

# Physiological clamp: values outside are artefact
SPO2_CLAMP_LO: float = 70.0   # %
SPO2_CLAMP_HI: float = 100.0  # %

# Motion-rejection: perfusion ceiling (AC/DC ratio > this → motion artefact).
# Resting perfusion index is 0.2–2 %; 10 % is a conservative reject threshold.
_SPO2_PERFUSION_CEILING: float = 0.10  # fraction


def _spo2_from_R(R: float, a: float = SPO2_A, b: float = SPO2_B) -> float:
    """Apply empirical linear map  SpO2 = a − b·R, clamped [70, 100]."""
    return _clamp(a - b * R, SPO2_CLAMP_LO, SPO2_CLAMP_HI)


# ── Single-sample fallback ───────────────────────────────────────────────────

def spo2_percent(red: float, ir: float) -> float:
    """
    APPROXIMATE — single-sample SpO2 estimate from raw red and IR ADC counts.

    Method: treats raw red/ir as a crude proxy for the ratio-of-ratios R ≈ red/ir,
    then applies the empirical linear formula SpO2 ≈ SPO2_A − SPO2_B·R,
    clamped [70, 100].

    LIMITATION: with a single sample there is no AC/DC separation, so this is
    highly noisy — often landing in the low-80s for healthy people.  Use
    spo2_percent_window() for a meaningful estimate.

    Args:
        red: Raw red-channel ADC count.
        ir:  Raw IR-channel ADC count.

    Returns:
        SpO2 estimate in percent, clamped to [70.0, 100.0].

    Raises:
        ZeroDivisionError: if ir == 0.
    """
    if ir == 0:
        raise ZeroDivisionError("IR channel is zero; cannot compute SpO2 ratio")
    R = float(red) / float(ir)
    return _spo2_from_R(R)


# ── Windowed AC/DC estimator ─────────────────────────────────────────────────

def spo2_feature_window(
    reds: Sequence[float],
    irs: Sequence[float],
    *,
    detrend: bool = True,
    reject_motion: bool = True,
) -> Optional[float]:
    """
    Compute the robust ratio-of-ratios feature R over a window of 1 Hz samples.

    Method (§1.3 of spec):
      DC_x  = mean(x)                 — baseline
      AC_x  = 1.4826 * MAD(x)        — robust pulsatile surrogate (motion-robust)
      R     = (AC_red / DC_red) / (AC_ir / DC_ir)

    Detrending (default on): removes a linear trend over the window before
    computing AC, so a slow drift in DC doesn't inflate AC.

    Motion rejection (default on): returns None if the perfusion index
    (AC/DC) of either channel exceeds _SPO2_PERFUSION_CEILING (10 %).

    Args:
        reds: Red-channel ADC counts over the window (≥ 2 samples).
        irs:  IR-channel ADC counts over the window.
        detrend: Whether to remove linear trend before AC estimation.
        reject_motion: Whether to reject high-motion windows.

    Returns:
        R value (float) or None if the window is rejected/degenerate.

    Raises:
        ValueError: if windows are empty or have different lengths.
    """
    if len(reds) != len(irs):
        raise ValueError(
            f"Window length mismatch: len(reds)={len(reds)}, len(irs)={len(irs)}"
        )
    if len(reds) == 0:
        raise ValueError("Window must contain at least one sample")

    red_arr = np.array(reds, dtype=float)
    ir_arr  = np.array(irs,  dtype=float)

    dc_red = float(np.mean(red_arr))
    dc_ir  = float(np.mean(ir_arr))

    if dc_red <= 0.0 or dc_ir <= 0.0:
        return None  # sensor off-wrist or saturated

    if len(red_arr) < 2:
        return None  # cannot compute AC

    if detrend:
        t = np.arange(len(red_arr), dtype=float)
        # Remove linear trend (least-squares fit) from each channel
        red_arr = red_arr - np.polyval(np.polyfit(t, red_arr, 1), t)
        ir_arr  = ir_arr  - np.polyval(np.polyfit(t, ir_arr,  1), t)

    ac_red = _robust_spread(red_arr)
    ac_ir  = _robust_spread(ir_arr)

    if ac_ir < 1e-6 * dc_ir:
        return None  # IR channel effectively flat → R undefined (detrend leaves ~1e-13 residual)

    if reject_motion:
        pi_red = ac_red / dc_red
        pi_ir  = ac_ir  / dc_ir
        if pi_red > _SPO2_PERFUSION_CEILING or pi_ir > _SPO2_PERFUSION_CEILING:
            return None  # motion artefact — discard this window

    R = (ac_red / dc_red) / (ac_ir / dc_ir)
    return float(R)


def spo2_percent_window(
    reds: Sequence[float],
    irs: Sequence[float],
    *,
    a: float = SPO2_A,
    b: float = SPO2_B,
) -> float:
    """
    APPROXIMATE — windowed SpO2 estimate using robust AC/DC decomposition.

    Method (ratio-of-ratios, Mendelson & Ochs 1988; TI SLAA655):
      AC_x  = 1.4826 * MAD(x)  — robust spread over the window (motion-resistant)
      DC_x  = mean(x)           — baseline
      R     = (AC_red / DC_red) / (AC_ir / DC_ir)
      SpO2  = a − b·R           — empirical linear map, clamped [70, 100]

    Default a=110, b=25 are the TI SLAA655 textbook values.
    UN-CALIBRATED: expect several % error.  Calibrate with fit_spo2().

    Falls back to the single-sample ratio when the window is rejected (motion)
    or degenerate (< 2 samples).

    Args:
        reds: Sequence of raw red-channel ADC counts over the window.
        irs:  Sequence of raw IR-channel ADC counts over the window.
        a:    SpO2 = a − b·R intercept (default SPO2_A = 110; use fitted value).
        b:    SpO2 = a − b·R slope     (default SPO2_B =  25; use fitted value).

    Returns:
        SpO2 estimate in percent, clamped to [70.0, 100.0].

    Raises:
        ValueError:       if windows are empty or have different lengths.
        ZeroDivisionError: if DC (mean) of either channel is zero.
    """
    if len(reds) != len(irs):
        raise ValueError(
            f"Window length mismatch: len(reds)={len(reds)}, len(irs)={len(irs)}"
        )
    if len(reds) == 0:
        raise ValueError("Window must contain at least one sample")

    reds_f = [float(v) for v in reds]
    irs_f  = [float(v) for v in irs]

    if len(reds_f) < 2:
        return spo2_percent(reds_f[0], irs_f[0])

    R = spo2_feature_window(reds_f, irs_f)
    if R is None:
        # Motion-rejected or degenerate — fall back to crude single-sample
        dc_red = statistics.mean(reds_f)
        dc_ir  = statistics.mean(irs_f)
        if dc_ir == 0.0:
            raise ZeroDivisionError("DC_ir is zero; cannot compute SpO2")
        R_crude = dc_red / dc_ir
        return _spo2_from_R(R_crude, a, b)

    return _spo2_from_R(R, a, b)


# ═══════════════════════════════════════════════════════════════════════════════
# §2  Skin temperature
# ═══════════════════════════════════════════════════════════════════════════════

# ── Published default calibration constants ─────────────────────────────────
#
# T_°C = SKIN_TEMP_SLOPE * raw_count + SKIN_TEMP_OFFSET
#
# Anchor: raw=930 → 33 °C (median resting on-wrist skin temp).
# IMPORTANT — UN-FITTED APPROXIMATION:
#   We do not know the thermistor's R₀, β, divider topology, or ADC resolution.
#   The full NTC chain (count → resistance → Steinhart-Hart → °C) cannot be
#   applied without those unknowns.  This linear map is an empirical substitute.
#   It is adequate for TREND analysis but NOT for absolute temperature.
#
#   Deviation-from-baseline (see skin_temp_deviation() below) cancels the
#   unknown offset and is the more trustworthy output.  Only the slope m
#   affects deviation accuracy.
#
# To calibrate: call fit_skin_temp(raw_series, whoop_celsius) once WHOOP
# export data is available; replace these constants with the returned (slope, offset).
# ─────────────────────────────────────────────────────────────────────────────
SKIN_TEMP_SLOPE: float  = 0.02    # [APPROXIMATE — un-calibrated] °C per ADC count
SKIN_TEMP_OFFSET: float = 14.4   # [APPROXIMATE — un-calibrated] °C at count=0


def skin_temp_celsius(raw: float, *, slope: float = SKIN_TEMP_SLOPE,
                      offset: float = SKIN_TEMP_OFFSET) -> float:
    """
    APPROXIMATE — convert raw thermistor ADC count to degrees Celsius.

    Method: uncalibrated linear fit  T = slope * raw + offset
    Default calibrated so raw ≈ 930 → 33 °C (median resting wrist temp).

    UNCALIBRATED ABSOLUTE — absolute accuracy is poor.  Use for TREND and
    relative comparisons only (e.g. circadian rhythm, nightly deviation).
    Real WHOOP uses a proprietary thermistor calibration computed in-cloud.
    Fit slope/offset against WHOOP ground-truth via fit_skin_temp().

    Args:
        raw:    Raw ADC count from the skin-temperature sensor.
        slope:  °C per ADC count (default SKIN_TEMP_SLOPE; use fitted value).
        offset: °C at count=0    (default SKIN_TEMP_OFFSET; use fitted value).

    Returns:
        Estimated skin temperature in degrees Celsius.
    """
    return slope * float(raw) + offset


def skin_temp_deviation(
    raw_series: Sequence[float],
    baseline_raw: float,
    *,
    slope: float = SKIN_TEMP_SLOPE,
) -> Sequence[float]:
    """
    APPROXIMATE — compute per-sample skin-temperature deviation from a
    personal baseline in °C.

    Because deviation subtracts the baseline:
        deviation = slope * (raw - baseline_raw)
    the unknown additive offset cancels entirely.  Only the slope m matters,
    and a crude m still yields a useful signal.  This mirrors what WHOOP
    actually shows (nightly deviation from trailing-median baseline).

    Args:
        raw_series:   Sequence of raw ADC counts (e.g. nightly mean per sample).
        baseline_raw: Baseline ADC count (e.g. trailing 14-night median of
                      nightly-mean raw values during sleep).
        slope:        °C per ADC count (default SKIN_TEMP_SLOPE; use fitted
                      slope from fit_skin_temp()).

    Returns:
        List of deviation values in °C (same length as raw_series).
    """
    return [slope * (float(r) - float(baseline_raw)) for r in raw_series]


# ═══════════════════════════════════════════════════════════════════════════════
# §3  Respiratory rate — Welch-peak spectral estimator
# ═══════════════════════════════════════════════════════════════════════════════

# Band limits for breathing frequency (0.1–0.5 Hz = 6–30 BrPM).
# Respiration (0.1–0.5 Hz) is AT/BELOW the 0.5 Hz Nyquist for 1 Hz sampling — detectable.
# Cardiac (~1 Hz and above) is ABOVE Nyquist — NOT recoverable at 1 Hz.
_RESP_BAND_LO: float = 0.1   # Hz
_RESP_BAND_HI: float = 0.5   # Hz

# Default Welch window (samples).  nperseg=120 at 1 Hz = 2-min window, which gives
# frequency resolution Δf = 1/120 ≈ 0.008 Hz ≈ 0.5 BrPM — adequate for sleep resp.
_RESP_NPERSEG_DEFAULT: int = 120

# Legacy linear constants retained ONLY for the old single-raw-value API path
# (resp_rate_bpm(raw_scalar)).  These are still synthetic/un-calibrated; the
# spectral estimator (resp_rate_from_signal) is the recommended approach.
RESP_SLOPE: float  = 10.0 / 512.0  # bpm per ADC count  (~0.01953)
RESP_OFFSET: float = 6.0            # bpm at raw=0


def resp_rate_from_signal(
    signal: Sequence[float],
    *,
    fs: float = 1.0,
    nperseg: int = _RESP_NPERSEG_DEFAULT,
    method: str = "welch",
) -> Optional[float]:
    """
    APPROXIMATE — estimate respiratory rate (BrPM) from a 1 Hz waveform signal.

    Uses a Welch periodogram to identify the dominant frequency in the
    respiratory band (0.1–0.5 Hz = 6–30 BrPM), then converts to breaths/min.
    This is the primary / recommended path for the dedicated resp field.

    NeuroKit2 xcorr is attempted as an alternative when method='nk2xcorr';
    it may fail on short or flat windows and falls back to Welch.

    Notes on 1 Hz adequacy:
      - Respiration (0.1–0.5 Hz) is BELOW the Nyquist for 1 Hz sampling (0.5 Hz),
        but just barely.  Welch uses overlapping windows so effective resolution
        is adequate for sleep-rate estimation.
      - Cardiac AC is NOT recoverable at 1 Hz (Nyquist problem) — that is why
        SpO2 uses a cross-channel robust spread approach, not a band-pass.

    Args:
        signal:  1D array-like of the raw resp field (or a detrended variant).
        fs:      Sampling rate in Hz (default 1.0).
        nperseg: Welch window length in samples (default 120; i.e. 2 min at 1 Hz).
        method:  'welch' (default) or 'nk2xcorr' (NeuroKit2 cross-correlation).

    Returns:
        Estimated respiratory rate in breaths/min, or None if estimation failed
        (too few samples, flat signal, or no peak in band).
    """
    arr = np.asarray(signal, dtype=float)

    if len(arr) < 2 * int(1.0 / _RESP_BAND_LO):  # need at least 2 full cycles
        return None

    if method == "nk2xcorr":
        return _resp_rate_nk2_xcorr(arr, fs=fs)

    return _resp_rate_welch(arr, fs=fs, nperseg=nperseg)


def _resp_rate_welch(arr: np.ndarray, *, fs: float = 1.0,
                     nperseg: int = _RESP_NPERSEG_DEFAULT) -> Optional[float]:
    """Welch periodogram peak in 0.1–0.5 Hz → BrPM.  Primary path."""
    # Guard: NaN in input → all-NaN PSD → argmax silently returns index 0 → 6.0 BrPM floor
    if not np.all(np.isfinite(arr)):
        return None
    arr_detrended = arr - np.mean(arr)
    if np.all(arr_detrended == 0):
        return None

    nperseg_actual = min(nperseg, len(arr))
    try:
        freqs, psd = welch(arr_detrended, fs=fs, nperseg=nperseg_actual,
                           noverlap=nperseg_actual // 2)
    except Exception as exc:
        logger.debug("Welch PSD failed: %s", exc)
        return None

    mask = (freqs >= _RESP_BAND_LO) & (freqs <= min(_RESP_BAND_HI, fs / 2.0))
    if not mask.any():
        return None

    peak_freq = float(freqs[mask][np.argmax(psd[mask])])
    return peak_freq * 60.0  # Hz → BrPM


def _resp_rate_nk2_xcorr(arr: np.ndarray, *, fs: float = 1.0) -> Optional[float]:
    """NeuroKit2 xcorr estimator (cross-correlation with sinusoid bank).
    Falls back to Welch on any exception (NeuroKit2's rsp_rate requires fs > 6
    for its internal filter, so we guard and fall through)."""
    try:
        import neurokit2 as nk
        rate_series = nk.rsp_rate(arr, sampling_rate=fs, method="xcorr")
        rate = float(np.nanmedian(rate_series))
        if np.isnan(rate) or not (6.0 <= rate <= 30.0):
            raise ValueError(f"nk2 xcorr out of band: {rate}")
        return rate
    except Exception as exc:
        logger.debug("nk2 xcorr failed (%s), falling back to Welch", exc)
        return _resp_rate_welch(arr, fs=fs)


def resp_rate_bpm(raw: float) -> float:
    """
    APPROXIMATE — single-sample legacy interface: convert a raw ADC/sensor count
    to breaths per minute via a synthetic linear map.

    This is retained for backwards compatibility with read.py, which calls
    resp_rate_bpm(row["raw"]) once per row.  For a physiologically meaningful
    estimate over a window of samples, use resp_rate_from_signal() instead.

    Method: `bpm = RESP_SLOPE * raw + RESP_OFFSET`  (synthetic, un-calibrated)

    IMPORTANT: This mapping is fully synthetic — it is NOT a frequency estimate
    and cannot track real breathing.  Real WHOOP derives resp rate in-cloud from
    accelerometer + PPG.  Use resp_rate_from_signal() on a window of values.

    Args:
        raw: Raw ADC/sensor count (any float).

    Returns:
        Estimated respiratory rate in breaths per minute.
    """
    return RESP_SLOPE * float(raw) + RESP_OFFSET


# ═══════════════════════════════════════════════════════════════════════════════
# §4  Calibration-fitting routines
#     (ready to run once WHOOP export ground truth is available)
# ═══════════════════════════════════════════════════════════════════════════════
#
# Anti-overfit rules (per spec §4.2):
#   - ≤ ~1 free parameter per several independent nights.
#   - Split by WHOLE NIGHT (leave-one-night-out, LONO) — never random splits
#     within a night (intra-night samples are autocorrelated).
#   - Fit linear before quadratic; add quadratic only if LONO error improves.
#   - Enforce physical sign constraints: SpO2 b > 0 (monotone), skin-temp
#     slope of the correct NTC sign.
#   - Report MAE/RMSE in physical units on held-out nights.

def fit_spo2(
    R_values: Sequence[float],
    whoop_spo2: Sequence[float],
    *,
    night_ids: Optional[Sequence[int]] = None,
    quadratic: bool = False,
) -> Tuple[float, float]:
    """
    Fit SpO2 = a − b·R to WHOOP-reported SpO2 values.

    Uses ordinary least squares.  When night_ids is provided, also runs
    leave-one-night-out cross-validation and logs per-night held-out MAE.

    OVERFIT GUARD: keep ≤ 2 free parameters (a, b).  Only call with quadratic=True
    if you have ≥ 6 independent overlap nights AND linear LONO error is clearly
    above the noise floor.  With < 4 nights, use only the defaults.

    Args:
        R_values:   Per-sample ratio-of-ratios R from spo2_feature_window().
        whoop_spo2: Corresponding WHOOP-reported SpO2 (%), aligned per sample.
        night_ids:  Integer night index per sample (for LONO; optional).
                    If None, no cross-validation is performed.
        quadratic:  If True, fit SpO2 = a·R² + b·R + c (3 params).
                    Returns (a, b) of the quadratic; c is logged and discarded
                    since _spo2_from_R only uses 2 params (extend if needed).

    Returns:
        (a, b) — fitted intercept and slope (SpO2 = a − b·R).
        For the quadratic case, returns (c, -b) matching the linear convention
        where a = c (bias at R=0) and b = −slope of R^1 term.

    Raises:
        ValueError: if lengths differ, too few samples, or fit violates
                    monotonicity (b ≤ 0).
    """
    R_arr = np.asarray(R_values, dtype=float)
    y_arr = np.asarray(whoop_spo2, dtype=float)

    if len(R_arr) != len(y_arr):
        raise ValueError("R_values and whoop_spo2 must have the same length")
    if len(R_arr) < 2:
        raise ValueError("Need at least 2 samples to fit")

    if quadratic:
        X = np.column_stack([R_arr ** 2, R_arr, np.ones_like(R_arr)])
        coeffs, _, _, _ = np.linalg.lstsq(X, y_arr, rcond=None)
        a_q, b_q, c_q = coeffs
        logger.info("SpO2 quadratic fit: %.4f·R² + %.4f·R + %.4f", a_q, b_q, c_q)
        # Return (c, -b_q) so it is consistent with the linear (a, b) convention
        # where SpO2 = a − b·R at R=0
        a_ret, b_ret = c_q, -b_q
    else:
        # OLS: SpO2 = a − b·R  →  y = β₀ + β₁·R, a=β₀, b=−β₁
        X = np.column_stack([np.ones_like(R_arr), R_arr])
        coeffs, _, _, _ = np.linalg.lstsq(X, y_arr, rcond=None)
        a_ret = float(coeffs[0])
        b_ret = float(-coeffs[1])

    if b_ret <= 0.0:
        raise ValueError(
            f"Fitted b={b_ret:.4f} ≤ 0 — SpO2 must decrease with R.  "
            "Check that AC feature is correct (try detrend=True / different window)."
        )

    logger.info("SpO2 fit: a=%.3f, b=%.3f (SpO2 = a − b·R)", a_ret, b_ret)

    if night_ids is not None:
        _lono_spo2(R_arr, y_arr, np.asarray(night_ids), quadratic=quadratic)

    return a_ret, b_ret


def _lono_spo2(
    R_arr: np.ndarray,
    y_arr: np.ndarray,
    night_ids: np.ndarray,
    *,
    quadratic: bool = False,
) -> None:
    """Leave-one-night-out cross-validation for SpO2 fit.  Logs per-night MAE."""
    unique_nights = np.unique(night_ids)
    if len(unique_nights) < 2:
        logger.warning("LONO SpO2: need ≥ 2 nights; skipping CV")
        return

    maes = []
    for night in unique_nights:
        test_mask  = night_ids == night
        train_mask = ~test_mask
        if train_mask.sum() < 2:
            continue
        R_tr, y_tr = R_arr[train_mask], y_arr[train_mask]
        R_te, y_te = R_arr[test_mask],  y_arr[test_mask]

        if quadratic:
            X_tr = np.column_stack([R_tr ** 2, R_tr, np.ones_like(R_tr)])
            c, _, _, _ = np.linalg.lstsq(X_tr, y_tr, rcond=None)
            pred = c[0] * R_te ** 2 + c[1] * R_te + c[2]
        else:
            X_tr = np.column_stack([np.ones_like(R_tr), R_tr])
            c, _, _, _ = np.linalg.lstsq(X_tr, y_tr, rcond=None)
            pred = c[0] + c[1] * R_te

        mae = float(np.mean(np.abs(pred - y_te)))
        maes.append(mae)
        logger.info("  LONO SpO2 — left-out night %s: MAE=%.2f %%SpO2", night, mae)

    if maes:
        logger.info("  LONO SpO2 — mean MAE=%.2f %%SpO2 (n=%d nights)", np.mean(maes), len(maes))


def fit_skin_temp(
    raw_values: Sequence[float],
    whoop_celsius: Sequence[float],
    *,
    night_ids: Optional[Sequence[int]] = None,
    fit_offset: bool = True,
) -> Tuple[float, float]:
    """
    Fit T_°C = slope * raw + offset (or deviation = slope * Δraw) to WHOOP data.

    For deviation calibration (the recommended primary target because it cancels
    the unknown additive offset), pass nightly Δraw and WHOOP nightly deviation,
    and set fit_offset=False (forces intercept=0, fitting slope only).

    OVERFIT GUARD: prefer fit_offset=False (1 free param) whenever you are
    calibrating deviation.  With only a few nights, adding the offset as a 2nd
    free param is rarely justified.

    Args:
        raw_values:    Raw ADC counts (nightly means or per-sample).
        whoop_celsius: Corresponding WHOOP-reported temperature in °C.
        night_ids:     Integer night index per sample (for LONO; optional).
        fit_offset:    If True (default), fit both slope and offset (2 params).
                       If False, force offset=0 (1 param; recommended for deviation).

    Returns:
        (slope, offset) in (°C/count, °C).

    Raises:
        ValueError: if lengths differ or too few samples.
    """
    raw_arr = np.asarray(raw_values,    dtype=float)
    y_arr   = np.asarray(whoop_celsius, dtype=float)

    if len(raw_arr) != len(y_arr):
        raise ValueError("raw_values and whoop_celsius must have the same length")
    if len(raw_arr) < 2:
        raise ValueError("Need at least 2 samples to fit")

    if fit_offset:
        X = np.column_stack([raw_arr, np.ones_like(raw_arr)])
        coeffs, _, _, _ = np.linalg.lstsq(X, y_arr, rcond=None)
        slope, offset = float(coeffs[0]), float(coeffs[1])
    else:
        # Force offset=0: slope = (raw · y) / (raw · raw)
        slope  = float(np.dot(raw_arr, y_arr) / np.dot(raw_arr, raw_arr))
        offset = 0.0

    logger.info(
        "Skin-temp fit: slope=%.6f °C/count, offset=%.3f °C  "
        "(T = slope*raw + offset)", slope, offset
    )

    if night_ids is not None:
        _lono_skin_temp(raw_arr, y_arr, np.asarray(night_ids), fit_offset=fit_offset)

    return slope, offset


def _lono_skin_temp(
    raw_arr: np.ndarray,
    y_arr: np.ndarray,
    night_ids: np.ndarray,
    *,
    fit_offset: bool = True,
) -> None:
    """Leave-one-night-out cross-validation for skin-temp fit.  Logs per-night MAE."""
    unique_nights = np.unique(night_ids)
    if len(unique_nights) < 2:
        logger.warning("LONO skin-temp: need ≥ 2 nights; skipping CV")
        return

    maes = []
    for night in unique_nights:
        test_mask  = night_ids == night
        train_mask = ~test_mask
        if train_mask.sum() < 2:
            continue
        r_tr, y_tr = raw_arr[train_mask], y_arr[train_mask]
        r_te, y_te = raw_arr[test_mask],  y_arr[test_mask]

        if fit_offset:
            X_tr = np.column_stack([r_tr, np.ones_like(r_tr)])
            c, _, _, _ = np.linalg.lstsq(X_tr, y_tr, rcond=None)
            pred = c[0] * r_te + c[1]
        else:
            s = float(np.dot(r_tr, y_tr) / np.dot(r_tr, r_tr))
            pred = s * r_te

        mae = float(np.mean(np.abs(pred - y_te)))
        maes.append(mae)
        logger.info("  LONO skin-temp — left-out night %s: MAE=%.3f °C", night, mae)

    if maes:
        logger.info("  LONO skin-temp — mean MAE=%.3f °C (n=%d nights)", np.mean(maes), len(maes))


def leave_one_night_out(
    features: Sequence[float],
    labels: Sequence[float],
    night_ids: Sequence[int],
    *,
    n_params: int = 2,
) -> dict:
    """
    Generic leave-one-night-out evaluation helper.

    Fits a degree-(n_params−1) polynomial of `features` on `labels` using all
    but one night, then evaluates on the left-out night.  Returns a summary dict
    with per-night and aggregate MAE/RMSE.

    OVERFIT GUARD: n_params ≤ ~(n_nights / 3).  E.g. with 5 overlap nights,
    n_params > 1–2 will overfit.  Prefer n_params=2 (linear, one slope + one
    intercept) unless held-out error clearly improves with 3.

    Args:
        features:  Per-sample feature values (e.g. ratio R, raw ADC count).
        labels:    Per-sample reference values in physical units.
        night_ids: Integer night index per sample.
        n_params:  Number of model parameters (polynomial degree = n_params − 1).

    Returns:
        Dict with keys: 'night_maes' (list of per-night MAE), 'mean_mae',
        'mean_rmse', 'n_nights'.
    """
    feat_arr = np.asarray(features, dtype=float)
    lab_arr  = np.asarray(labels,   dtype=float)
    nid_arr  = np.asarray(night_ids, dtype=int)

    unique_nights = np.unique(nid_arr)
    deg = n_params - 1  # polynomial degree

    night_maes  = []
    night_rmses = []

    for night in unique_nights:
        test_mask  = nid_arr == night
        train_mask = ~test_mask
        if train_mask.sum() < n_params:
            logger.warning("LONO: night %s — not enough training samples; skipping", night)
            continue

        f_tr, y_tr = feat_arr[train_mask], lab_arr[train_mask]
        f_te, y_te = feat_arr[test_mask],  lab_arr[test_mask]

        coeffs = np.polyfit(f_tr, y_tr, deg)
        pred   = np.polyval(coeffs, f_te)
        errs   = pred - y_te
        night_maes.append(float(np.mean(np.abs(errs))))
        night_rmses.append(float(np.sqrt(np.mean(errs ** 2))))

    return {
        "night_maes":  night_maes,
        "night_rmses": night_rmses,
        "mean_mae":    float(np.mean(night_maes))  if night_maes  else float("nan"),
        "mean_rmse":   float(np.mean(night_rmses)) if night_rmses else float("nan"),
        "n_nights":    len(night_maes),
    }
