"""
stats.py — Agreement statistics for metric validation.

All functions operate on 1-D array-likes of (truth, predicted) pairs.
Pure numpy/sklearn; no IO.

Formulas (cited from docs/research/06-baselines-validation.md §4):

    d_i = pred_i - truth_i          (signed error; positive = over-prediction)
    n   = len(truth)

    MAE    = mean(|d_i|)
    RMSE   = sqrt(mean(d_i^2))
    bias   = mean(d_i)              [signed; positive = systematic over-prediction]

    pct_within(t, p, abs_tol, pct_tol):
        A pair passes when |d_i| <= abs_tol  OR  |d_i|/|t_i|*100 <= pct_tol.
        OR semantics when both tolerances are provided; None skips that check.
        Returns fraction in [0, 1].

    Bland-Altman (Bland & Altman 1986 / Lancet):
        mean_diff = bias
        sd_diff   = std(d_i, ddof=1)
        LoA_low   = mean_diff - 1.96 * sd_diff
        LoA_high  = mean_diff + 1.96 * sd_diff

    Lin's CCC (Lin 1989 / Biometrics):
        CCC = (2 * rho * sigma_t * sigma_p)
              / (sigma_t^2 + sigma_p^2 + (mu_t - mu_p)^2)
        where rho = Pearson correlation, sigma_t/sigma_p = standard deviations,
        mu_t/mu_p = means.  CCC ∈ [-1, 1]; >0.8 good agreement by wearable-
        validation convention.

    Cohen's kappa (Cohen 1960):
        kappa = (p_o - p_e) / (1 - p_e)
        where p_o = observed agreement (fraction of exact matches),
              p_e = expected-by-chance agreement from marginal class counts.
        Uses sklearn.metrics.cohen_kappa_score (unweighted by default).
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

import numpy as np
from sklearn.metrics import cohen_kappa_score as _sklearn_kappa


# ---------------------------------------------------------------------------
# Result types
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class BAResult:
    """Bland-Altman agreement statistics."""
    mean_diff: float    # bias = mean(pred - truth)
    sd_diff: float      # standard deviation of differences (ddof=1)
    loa_low: float      # lower limit of agreement = mean_diff - 1.96 * sd_diff
    loa_high: float     # upper limit of agreement = mean_diff + 1.96 * sd_diff
    n: int              # number of pairs used


@dataclass(frozen=True)
class KappaResult:
    """Cohen's kappa result for categorical (sleep-stage epoch) agreement."""
    kappa: float
    accuracy: float     # raw fraction of exact matches (p_o)
    n: int              # number of epochs used


# ---------------------------------------------------------------------------
# Continuous agreement statistics
# ---------------------------------------------------------------------------

def mae(truth: Sequence[float], pred: Sequence[float]) -> float:
    """Mean absolute error: mean(|pred_i - truth_i|).

    Args:
        truth: Ground-truth values.
        pred:  Predicted/computed values.

    Returns:
        MAE in the same units as the inputs.

    Raises:
        ValueError: if inputs have different lengths or are empty.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p)
    return float(np.mean(np.abs(p - t)))


def rmse(truth: Sequence[float], pred: Sequence[float]) -> float:
    """Root mean squared error: sqrt(mean((pred_i - truth_i)^2)).

    Always >= MAE; RMSE >> MAE signals a few large outlier errors.

    Args:
        truth: Ground-truth values.
        pred:  Predicted/computed values.

    Returns:
        RMSE in the same units as the inputs.

    Raises:
        ValueError: if inputs have different lengths or are empty.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p)
    return float(np.sqrt(np.mean((p - t) ** 2)))


def bias(truth: Sequence[float], pred: Sequence[float]) -> float:
    """Mean signed error: mean(pred_i - truth_i).

    Positive = systematic over-prediction; negative = under-prediction.
    A large |bias| with small MAE is impossible; large MAE with ~0 bias
    means random scatter without systematic offset.

    Args:
        truth: Ground-truth values.
        pred:  Predicted/computed values.

    Returns:
        Signed bias in the same units as inputs.

    Raises:
        ValueError: if inputs have different lengths or are empty.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p)
    return float(np.mean(p - t))


def pct_within_tolerance(
    truth: Sequence[float],
    pred: Sequence[float],
    abs_tol: float | None,
    pct_tol: float | None,
) -> float:
    """Fraction of paired nights whose error falls within the tolerance band.

    A pair passes when:
      |pred_i - truth_i| <= abs_tol   (if abs_tol is provided)
      OR
      |pred_i - truth_i| / |truth_i| * 100 <= pct_tol  (if pct_tol is provided,
                                                          and truth_i != 0)

    If both tolerances are given, OR semantics apply (more lenient).
    At least one of abs_tol / pct_tol must be provided.

    Args:
        truth:    Ground-truth values.
        pred:     Predicted/computed values.
        abs_tol:  Absolute tolerance in native units; None to skip.
        pct_tol:  Percentage tolerance (0-100 scale); None to skip.

    Returns:
        Fraction in [0, 1] of pairs within tolerance.

    Raises:
        ValueError: if both tolerances are None, or inputs are incompatible.
    """
    if abs_tol is None and pct_tol is None:
        raise ValueError("At least one of abs_tol or pct_tol must be provided.")
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p)
    if t.size == 0:
        return float("nan")

    abs_errors = np.abs(p - t)
    passing = np.zeros(t.size, dtype=bool)

    if abs_tol is not None:
        passing |= abs_errors <= abs_tol

    if pct_tol is not None:
        # Avoid divide-by-zero: pairs with truth==0 are never within a %-tol.
        nonzero_mask = t != 0.0
        pct_errors = np.where(nonzero_mask, abs_errors / np.abs(t) * 100.0, np.inf)
        passing |= pct_errors <= pct_tol

    return float(np.mean(passing))


def bland_altman(
    truth: Sequence[float],
    pred: Sequence[float],
) -> BAResult:
    """Bland-Altman limits of agreement (Bland & Altman 1986).

    Computes the mean difference (= bias), SD of differences, and the
    95% limits of agreement (mean ± 1.96 SD).

    The B-A plot (mean vs difference) reveals:
      - Systematic bias (cloud offset from zero on y-axis).
      - Proportional bias (cloud tilts as mean increases — errors grow with
        magnitude, signaling a multiplicative calibration error).
      - Heteroscedasticity (wider cloud at high magnitudes — more scatter at
        higher values).

    Args:
        truth: Ground-truth values (WHOOP).
        pred:  Predicted/computed values (ours).

    Returns:
        BAResult with mean_diff, sd_diff, loa_low, loa_high, n.

    Raises:
        ValueError: if inputs have different lengths or fewer than 2 pairs.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p, min_n=2)
    d = p - t
    mean_d = float(np.mean(d))
    # ddof=1 (sample std) per the original Bland-Altman paper convention
    sd_d = float(np.std(d, ddof=1))
    return BAResult(
        mean_diff=mean_d,
        sd_diff=sd_d,
        loa_low=mean_d - 1.96 * sd_d,
        loa_high=mean_d + 1.96 * sd_d,
        n=int(t.size),
    )


def lins_ccc(
    truth: Sequence[float],
    pred: Sequence[float],
) -> float:
    """Lin's Concordance Correlation Coefficient (Lin 1989, Biometrics).

    Measures agreement with the 45° line of identity — combines precision
    (Pearson correlation) and accuracy (mean shift + scale shift):

        CCC = (2 * rho * sigma_t * sigma_p)
              / (sigma_t^2 + sigma_p^2 + (mu_t - mu_p)^2)

    where rho = Pearson r, sigma_t/sigma_p = sample standard deviations,
    mu_t/mu_p = means.

    Interpretation:
      - CCC = 1.0: perfect agreement (all points on the identity line).
      - CCC > 0.8: good agreement (wearable-validation convention).
      - CCC ≈ 0.0: no agreement.
      - CCC = -1.0: perfect disagreement (mirror image).

    Note: Unlike Pearson r, CCC is penalized when the means or variances
    differ — you can have r=0.99 but low CCC if there's a systematic offset.

    Note on estimator: this implementation uses sample variance (ddof=1) for
    sigma_t, sigma_p, and the covariance, which differs slightly from Lin
    1989's original population estimator (ddof=0).  The two are equivalent
    in the limit of large n but diverge for small samples (n < ~20).

    Args:
        truth: Ground-truth values.
        pred:  Predicted/computed values.

    Returns:
        CCC in [-1, 1].  Returns NaN for constant series (zero variance).

    Raises:
        ValueError: if inputs have different lengths or fewer than 2 pairs.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p, min_n=2)

    mu_t, mu_p = float(np.mean(t)), float(np.mean(p))
    # Use ddof=1 for sample variances / covariance
    var_t = float(np.var(t, ddof=1))
    var_p = float(np.var(p, ddof=1))

    # Pearson covariance (using ddof=1 to match sample variance convention)
    cov_tp = float(np.cov(t, p, ddof=1)[0, 1])

    denom = var_t + var_p + (mu_t - mu_p) ** 2
    if denom < 1e-12:
        # Constant series — CCC is undefined
        return float("nan")

    return float(2.0 * cov_tp / denom)


def cohens_kappa(
    truth_labels: Sequence,
    pred_labels: Sequence,
) -> KappaResult:
    """Cohen's kappa for categorical label agreement (e.g. sleep-stage epochs).

    kappa = (p_o - p_e) / (1 - p_e)

    where p_o = observed agreement fraction (identical pairs / n),
          p_e = expected-by-chance agreement from the marginal class frequencies.

    Rules of thumb (Cohen 1960; Landis & Koch 1977):
      0.00-0.20  slight agreement
      0.21-0.40  fair agreement
      0.41-0.60  moderate agreement
      0.61-0.80  substantial agreement
      0.81-1.00  almost perfect agreement

    Delegates to sklearn.metrics.cohen_kappa_score for the calculation, which
    handles multi-class and computes expected agreement from marginal counts.

    Args:
        truth_labels: Ground-truth category labels (e.g. stage strings).
        pred_labels:  Our predicted/computed category labels.

    Returns:
        KappaResult with kappa, raw accuracy (p_o), and n.

    Raises:
        ValueError: if inputs have different lengths or are empty.
    """
    t_arr = list(truth_labels)
    p_arr = list(pred_labels)
    if len(t_arr) != len(p_arr):
        raise ValueError(
            f"cohens_kappa: truth and pred must have the same length "
            f"(got {len(t_arr)} vs {len(p_arr)})"
        )
    if not t_arr:
        raise ValueError("cohens_kappa: inputs must be non-empty")

    kappa_val = float(_sklearn_kappa(t_arr, p_arr))
    accuracy_val = float(sum(a == b for a, b in zip(t_arr, p_arr)) / len(t_arr))
    return KappaResult(kappa=kappa_val, accuracy=accuracy_val, n=len(t_arr))


# ---------------------------------------------------------------------------
# Trend-mode helpers
# ---------------------------------------------------------------------------

def pearson_r(
    truth: Sequence[float],
    pred: Sequence[float],
) -> float:
    """Pearson correlation coefficient between two series.

    Used in trend-mode scoring (skin temp, resp) where absolute accuracy
    is known-poor but tracking the series direction is the primary goal.

    Returns NaN for constant series.
    """
    t = np.asarray(truth, dtype=np.float64)
    p = np.asarray(pred, dtype=np.float64)
    _check_shapes(t, p, min_n=2)
    if np.std(t) < 1e-12 or np.std(p) < 1e-12:
        return float("nan")
    return float(np.corrcoef(t, p)[0, 1])


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _check_shapes(
    t: np.ndarray,
    p: np.ndarray,
    min_n: int = 1,
) -> None:
    """Validate array shapes; raise ValueError with a readable message."""
    if t.shape != p.shape:
        raise ValueError(
            f"truth and pred must have the same shape "
            f"(got {t.shape} vs {p.shape})"
        )
    if t.size < min_n:
        raise ValueError(
            f"At least {min_n} paired observations required "
            f"(got {t.size})"
        )
