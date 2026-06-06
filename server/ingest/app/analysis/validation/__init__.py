"""
app.analysis.validation — Metrics accuracy validation harness.

Two modes:
  1. Ground-truth comparison (when WHOOP export data is available):
     Aligns our computed metrics against WHOOP values by date, computes
     per-metric agreement statistics (MAE, RMSE, bias, Bland-Altman LoA,
     Lin's CCC, Cohen's kappa for sleep stages), and reports PASS/FAIL vs
     per-metric accuracy targets from the plan §3 table.

  2. Fallback validations (always runnable, no ground truth needed):
     - Reference-implementation agreement: our RMSSD vs neurokit2 on
       identical clean RR intervals (the strongest current HRV evidence).
     - Physiological plausibility bounds: per-metric sane ranges.
     - Internal consistency: stage minutes sum to TST, efficiency = TST/TIB,
       recovery and strain monotonicity.

     All fallback results are clearly labelled "plausible, not validated vs WHOOP".

Usage:
    from app.analysis.validation.report import build_report, render_table
    from app.analysis.validation.plausibility import run_fallback_validations
"""
