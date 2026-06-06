"""
calibrate_recovery.py — Calibrate Recovery coefficients from WHOOP historical data.

Usage:
    python calibrate_recovery.py --db /path/to/whoop.sqlite --out recovery_coefficients.json

Reads dailyMetric from the iOS GRDB SQLite, computes 28-day rolling HRV baseline,
fits sklearn LinearRegression (hrv_ratio → Recovery 0-100), and exports coefficients.

The iOS LocalMetricsComputer uses: score = hrv_slope * ratio + hrv_intercept
where ratio = currentHRV / 28-day-mean-HRV.
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import statistics
from pathlib import Path


MIN_BASELINE_DAYS = 3   # minimum prior days to compute a valid baseline
MIN_SAMPLES = 14        # minimum valid rows for regression to be meaningful
BASELINE_WINDOW = 28    # rolling window (days)


def load_data(db_path: str) -> list[dict]:
    con = sqlite3.connect(db_path)
    cur = con.execute(
        """
        SELECT day, avgHrv, restingHr, recovery
        FROM dailyMetric
        WHERE avgHrv IS NOT NULL AND recovery IS NOT NULL
        ORDER BY day
        """
    )
    rows = [{"day": r[0], "hrv": r[1], "rhr": r[2], "recovery": r[3] * 100.0}
            for r in cur.fetchall()]
    con.close()
    return rows


def compute_rolling_baseline(values: list[float], window: int) -> list[float | None]:
    """Rolling mean of the prior `window` days (exclusive of current day)."""
    baselines: list[float | None] = []
    for i in range(len(values)):
        prior = values[max(0, i - window):i]
        if len(prior) < MIN_BASELINE_DAYS:
            baselines.append(None)
        else:
            baselines.append(statistics.fmean(prior))
    return baselines


def fit_linear(X: list[float], y: list[float]) -> tuple[float, float]:
    """Fit y = slope * x + intercept via closed-form OLS."""
    n = len(X)
    if n < 2:
        raise ValueError("Need at least 2 samples")
    mean_x = statistics.fmean(X)
    mean_y = statistics.fmean(y)
    num = sum((xi - mean_x) * (yi - mean_y) for xi, yi in zip(X, y))
    den = sum((xi - mean_x) ** 2 for xi in X)
    if den == 0:
        raise ValueError("Zero variance in X — cannot fit")
    slope = num / den
    intercept = mean_y - slope * mean_x
    return slope, intercept


def compute_r2(y_true: list[float], y_pred: list[float]) -> float:
    mean_y = statistics.fmean(y_true)
    ss_res = sum((yt - yp) ** 2 for yt, yp in zip(y_true, y_pred))
    ss_tot = sum((yt - mean_y) ** 2 for yt in y_true)
    return 1.0 - ss_res / ss_tot if ss_tot > 0 else 0.0


def compute_mae(y_true: list[float], y_pred: list[float]) -> float:
    return statistics.fmean(abs(yt - yp) for yt, yp in zip(y_true, y_pred))


def main() -> None:
    parser = argparse.ArgumentParser(description="Calibrate Recovery coefficients")
    parser.add_argument("--db", required=True, help="Path to iOS GRDB SQLite file")
    parser.add_argument("--out", required=True, help="Output JSON path for coefficients")
    args = parser.parse_args()

    print(f"Loading data from: {args.db}")
    rows = load_data(args.db)
    print(f"Total rows with HRV + Recovery: {len(rows)}")

    hrv_values = [r["hrv"] for r in rows]
    hrv_baselines = compute_rolling_baseline(hrv_values, BASELINE_WINDOW)

    # Filter to rows with valid baseline
    valid = [
        (rows[i], hrv_baselines[i])
        for i in range(len(rows))
        if hrv_baselines[i] is not None
    ]
    print(f"Rows with valid HRV baseline (≥{MIN_BASELINE_DAYS} prior days): {len(valid)}")

    if len(valid) < MIN_SAMPLES:
        print(
            f"WARNING: only {len(valid)} valid samples (need {MIN_SAMPLES}). "
            "Coefficients may be unreliable. Proceeding anyway."
        )

    hrv_ratios = [row["hrv"] / baseline for row, baseline in valid]
    recoveries = [row["recovery"] for row, _ in valid]

    # --- Fit 1D model: score = hrv_slope * hrv_ratio + hrv_intercept
    slope_1d, intercept_1d = fit_linear(hrv_ratios, recoveries)
    pred_1d = [slope_1d * r + intercept_1d for r in hrv_ratios]
    r2_1d = compute_r2(recoveries, pred_1d)
    mae_1d = compute_mae(recoveries, pred_1d)

    # --- Baseline model (current iOS formula): score = 66 * ratio + 33
    pred_baseline = [66.0 * r + 33.0 for r in hrv_ratios]
    r2_baseline = compute_r2(recoveries, pred_baseline)
    mae_baseline = compute_mae(recoveries, pred_baseline)

    print()
    print("=== Calibration Results ===")
    print(f"Current formula (66×ratio + 33):")
    print(f"  R² = {r2_baseline:.3f}, MAE = {mae_baseline:.1f} pts")
    print(f"Calibrated (1D HRV ratio):")
    print(f"  hrv_slope = {slope_1d:.2f}, hrv_intercept = {intercept_1d:.2f}")
    print(f"  R² = {r2_1d:.3f}, MAE = {mae_1d:.1f} pts")
    print(f"  Improvement: ΔMAE = {mae_baseline - mae_1d:.1f} pts, ΔR² = {r2_1d - r2_baseline:.3f}")

    # --- Sample comparison
    print()
    print("=== Sample comparison (last 5 rows) ===")
    print(f"{'Day':<12} {'HRV':>6} {'Baseline':>9} {'Ratio':>7} {'WHOOP':>7} {'Old':>6} {'New':>6}")
    for (row, baseline), ratio, pred in zip(valid[-5:], hrv_ratios[-5:], pred_1d[-5:]):
        old_pred = min(100.0, max(33.0, 66.0 * ratio + 33.0))
        print(f"{row['day']:<12} {row['hrv']:>6.1f} {baseline:>9.1f} {ratio:>7.3f} "
              f"{row['recovery']:>7.1f} {old_pred:>6.1f} {min(100.0, max(0.0, pred)):>6.1f}")

    # --- Export JSON
    coefficients = {
        "hrv_slope": round(slope_1d, 4),
        "hrv_intercept": round(intercept_1d, 4),
        "r2": round(r2_1d, 4),
        "mae": round(mae_1d, 2),
        "n_samples": len(valid),
        "baseline_window_days": BASELINE_WINDOW,
    }

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(coefficients, f, indent=2)
    print()
    print(f"Coefficients saved to: {out_path}")
    print(json.dumps(coefficients, indent=2))


if __name__ == "__main__":
    main()
