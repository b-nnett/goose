# Derived-Metrics Methodology & Accuracy (2026-05-26)

This documents how every derived health metric in `app/analysis/` is computed after the
**metrics-accuracy overhaul**: the algorithm, the library it leans on, the personal-baseline
definition where relevant, the validation status, and honest caveats. The deep-research basis for
each choice lives in `docs/research/01..06`.

> **Validation status — read this first.** The overhaul's *mission* is accuracy validated against
> real WHOOP ground truth. At the time of writing **no WHOOP ground truth has been supplied**
> (no API token, no data export, and our raw store only covers the OpenWhoop period from
> 2026-05-23, which WHOOP's cloud never scored — so there is currently zero date-overlap to compare
> against). Therefore every metric below is validated in **fallback mode** only:
> (a) **reference-implementation agreement** (our HRV == neurokit2 on identical RR),
> (b) **physiological plausibility bounds**, and (c) **internal consistency**.
> All metrics are **WHOOP-*like* approximations, not yet validated against WHOOP**. The machinery to
> close this gap is built and one command away (see §"Closing the validation gap").

---

## Library stack (Phase 0d)
Added to `requirements.txt`: **neurokit2 0.2.13** (HRV cleaning, frequency HRV, respiration),
**numpy / scipy / scikit-learn / pandas**, **httpx** (WHOOP API client). All algorithms prefer these
vetted libraries over hand-rolled math where one fits (per the plan's directive).

---

## Per-metric methodology

### Resting Heart Rate (RHR) — `recovery.resting_hr`
- **Algorithm:** lowest 5-minute rolling-mean HR during the sleep window (WHOOP measures RHR during
  the last slow-wave-sleep period; the rolling-min over sleep is a faithful, robust analog). Kept
  from the prior implementation — it was already correct.
- **Baseline:** trailing robust baseline via `baselines.py` (see below) feeds recovery; RHR itself is
  the nightly value.
- **Validation:** plausibility (30–100 bpm) + internal consistency. **Target ±2 bpm vs WHOOP — unverified.**
- **Caveat:** sensor-level ceiling (WHOOP-vs-ECG RHR MAPE ≈ 3%, bias −1.4 bpm) is inherited.

### HRV (RMSSD) — `hrv.py`
- **Algorithm:** Task Force (1996) RMSSD in **float64 ms**, divisor N−1. RR is cleaned first:
  physiologic range filter (300–2000 ms) → **Kubios/Lipponen–Tarvainen artifact correction**
  (`neurokit2.signal_fixpeaks(method="kubios")`) → segment-aware pooling that splits the window at
  wall-clock gaps so concatenation never fabricates successive differences. Nightly value uses a
  tiered window: **last slow-wave-sleep episode** (WHOOP's published convention) → all-SWS
  recency-weighted → whole-night fallback. The whole-night RMSSD is persisted alongside for empirical
  tuning. `pnn50`, `mean_nn`, `SDNN` retained as QC.
- **Library:** neurokit2 (`intervals_to_peaks` → `signal_fixpeaks` → `hrv_time`).
- **Validation:** **reference-implementation agreement is the strongest current evidence** — our
  `rmssd_ms` matches `neurokit2.hrv_time` to **< 0.01 ms** on identical integer-ms RR (hard-gated in
  `tests/test_validation.py` and `tests/test_hrv.py`). Plus a hand-computed formula pin.
  **Target ±5 ms / ±10% vs WHOOP — unverified.**
- **Caveat:** PPG-derived RR is noisier than ECG; nights with high artifact fractions are
  lower-confidence (artifact count is reported). WHOOP's exact SWS weighting is proprietary; we
  approximate it.
- Research: `docs/research/01-hrv.md`.

### Personal baselines — `baselines.py`
- **Algorithm:** robust **Winsorized EWMA** (14-day half-life ⇒ effective ~30-day window) per metric
  (HRV, RHR, resp, skin-temp). Missing nights carry the baseline forward (no decay to zero);
  hard-outlier nights (>~5σ) are rejected so they don't whipsaw the baseline; dispersion is a robust
  MAD→σ with a per-metric floor. Cold-start gates: `usable` after ≥4 valid nights, `trusted` after
  ≥14. Deviation helpers return z-score, signed delta, and ratio. A 20%-trimmed-mean alternative is
  provided for auditability.
- **Why:** baseline-relative standardization is what makes recovery "personalized" (WHOOP compares to
  *your* baseline, not population). EWMA matches WHOOP's "dynamic average that adapts" framing.
- **Validation:** unit-tested incl. the critical leading-None case (a missing first night must not
  freeze the EWMA at the physiological midpoint — fixed and regression-tested).
- Research: `docs/research/06-baselines-validation.md` §1.

### Recovery (0–100) — `recovery.py`
- **Algorithm:** baseline-relative **z-score + logistic** composite. Per-metric signed z in the
  recovery-favorable direction (higher HRV →+, lower RHR →+, lower resp →+, sleep-performance
  centered ~0.85), HRV-dominant weights **W_HRV 0.60 / W_RHR 0.20 / W_SLEEP 0.15 / W_RESP 0.05**
  (missing terms dropped + renormalized), squashed by `100/(1+exp(−k·(Z−Z0)))` with k=1.6, Z0=−0.20
  so **Z=0 → ~58%** (WHOOP's published average recovery). Bands: red <34, yellow 34–66, green ≥67.
  Cold-start returns `None` (honest) rather than a fake midpoint.
- **Note:** resp contributes via a scale-invariant z-score, so it works on the raw (un-calibrated)
  resp signal; sleep "performance" uses sleep efficiency as a proxy. Both documented.
- **Validation:** internal consistency (recovery is monotonic in HRV given a fixed baseline — checked)
  + plausibility (0–100). **Target ±7% vs WHOOP — unverified;** realistic bar per literature is
  ±10–15 pts and ≥70–80% same-band agreement (HRV-sensor-bounded).
- **Weights/knobs (`W_*`, k, Z0) and the community "70/20/10" split are NOT WHOOP's published values**
  — they're a defensible reconstruction, exposed as tunables to fit against ground truth later.
- Research: `docs/research/03-recovery-strain.md` §1–2.

### Strain (0–21) — `strain.py`
- **Algorithm:** Karvonen **%HRR** zones (cutoffs 50/60/70/80/90), **TRIMP** (Edwards default;
  Banister exponential available), log-mapped `strain = 21·ln(TRIMP+1)/ln(D)`. **Personalized HRmax**
  via `estimate_hrmax`: observed 99.5th-percentile HR over trailing data (artifact-robust) →
  Tanaka `208−0.7·age` fallback → 220−age last resort; never bare 220−age when better data exists.
  RHR from the nightly resting-HR. Accumulated over the **WHOOP sleep-to-sleep "day"** (this morning's
  wake → next sleep onset), not midnight-to-midnight.
- **Calibration:** the log denominator `D` (default 7201 ≈ 24 h at zone 5) is an **un-fitted anchor**;
  `fit_strain_denominator(pairs)` is ready to least-squares-fit `D` to WHOOP day-strain when an export
  arrives. Shape is correct; absolute level will shift after fitting.
- **Validation:** internal consistency (monotonic in TRIMP; log curvature — second marathon barely
  moves the score) + plausibility (0–21). **Target ±1.5 vs WHOOP — unverified** (expect a systematic
  offset until D is fitted).
- Research: `docs/research/03-recovery-strain.md` §3.

### Sleep staging (wake/light/deep/REM) — `sleep.py` + `sleep_features.py`
- **Algorithm:** 30-second epoch grid. **Cole–Kripke** (exact te-Lindert 30 s coefficients) gives the
  sleep/wake spine over Δgravity activity counts; the openwhoop stillness spine finds the main sleep
  period. Per-epoch **cardiorespiratory features** over a rolling 5-min window: mean HR, Walch
  difference-of-Gaussians HR-variability, neurokit2 HRV (RMSSD/SDNN + LF/HF when ≥120 beats),
  respiration rate + RRV, and a clock proxy. A **transparent per-night-percentile classifier**
  (`classify_epochs`) assigns deep (still + high parasympathetic + low HR + regular breathing),
  REM (still body + activated cardiac + irregular respiration), wake (motion + elevated HR), else
  light — followed by majority smoothing and physiology re-imposition (no REM in the first 15 min;
  deep concentrated in the first third). **`classify_epochs` is a clean seam** so a trained model
  (sleepecg GRU / LightGBM) can replace the heuristic later without touching the rest of the pipeline.
- **Why not sleepecg now:** the pretrained GRU pulls TensorFlow into the production image (heavy,
  network-fetched weights) and — with no ground truth — a black-box model can't be validated. The
  vetted-components path (Cole–Kripke + neurokit features + transparent rules) is deployable and
  verifiable today; sleepecg is documented as the upgrade path.
- **AASM metrics:** TST, sleep efficiency (TST/TIB), sleep latency, REM latency, WASO (post-onset,
  pre-final-wake), disturbances (post-onset wake runs), stage %.
- **Validation:** internal consistency (stage minutes sum to TST; physiology re-imposition verified
  end-to-end) + plausibility. **Targets — sleep duration ±10 min / efficiency ±5% / stages ≥70%
  epoch agreement — unverified** (needs WHOOP stage durations).
- **Caveat:** **light/deep separation is the weakest link** (cardiac signal barely distinguishes
  N1/N2/N3 without EEG) — deep-minute estimates are the least reliable output and are hedged as such.
  Honest ceiling ~70% 4-class epoch agreement.
- Research: `docs/research/02-sleep-staging.md`.

### Exercise — `exercise.py`
- **Algorithm:** bout detection via HR-margin + smoothed motion gates with nearest-timestamp HR↔motion
  alignment and a minimum duration; per-bout **intensity** computed by reusing the strain module's HRR
  zones — avg/peak HR, per-bout strain (zone TRIMP → log), **% time in each HR zone**, avg %HRR,
  duration, and the personalized HRmax + its provenance. `kind` (sport) stays `None` by design (needs
  raw accelerometer we don't routinely keep).
- **Validation:** detection rejects fever (still + high HR) and driving (motion + no HR rise);
  per-bout strain matches the strain module; zone percentages sum to 100. Plausibility + consistency.
- Research: `docs/research/03-recovery-strain.md` §3 (zones/HRmax).

### Calibrated signals — `units.py`
- **SpO2 %:** ratio-of-ratios `R = (AC_red/DC_red)/(AC_ir/DC_ir)` with AC as a robust windowed spread
  (1.4826·MAD, detrended), `SpO2 = a − b·R` clamped [70,100]. Defaults a=110, b=25 are a **sanity
  range only**; `fit_spo2` is ready to regression-fit (a,b) to WHOOP SpO2 with leave-one-night-out.
- **Skin temp:** single-slope linear raw→°C, but the primary output is **deviation-from-baseline**
  (`skin_temp_deviation`) which cancels the unknown offset and needs only one parameter — matching
  what WHOOP reports. `fit_skin_temp` ready.
- **Respiration:** `resp_rate_from_signal` (Welch peak in 0.1–0.5 Hz; 1 Hz sampling is adequate since
  breathing < Nyquist). NaN/flat-signal-safe. (The per-row stream display in `read.py` remains a crude
  approximation; the spectral estimate runs in the analysis path.)
- **Validation:** synthetic recovery (known R→SpO2; sine→known resp rate; offset-cancellation proof)
  + plausibility. **Targets SpO2 ±2%, skin-temp ±0.3°C, resp ±1 br/min — UNVERIFIED and UN-CALIBRATED.**
  These are the metrics most dependent on ground truth (WHOOP's reported values are the only
  calibration reference). **Treat absolute SpO2/skin-temp values as un-calibrated until fitted.**
- Research: `docs/research/04-signal-calibration.md`.

### Stress (Baevsky) — unchanged
Retained as a qualitative trend; not a focus of this overhaul.

---

## Validation harness — `app/analysis/validation/`
- `targets.py`: per-metric tolerance spec straight from the plan §3.
- `stats.py`: MAE, RMSE, bias, %-within-tolerance, Bland-Altman LoA, Lin's CCC, Cohen's κ (sleep
  epochs) — all hand-verified.
- `report.py`: aligns `GroundTruthDay` records (from the WHOOP API client / CSV export) with our
  computed metrics by date, excludes WHOOP `user_calibrating` days, and emits a per-metric PASS/FAIL
  table (continuous + the categorical sleep-stage gate). Never reports PASS on empty data.
- `plausibility.py`: the fallback validations that run **today** without ground truth.
- CLI: `python -m app.analysis.validation fallback` (no GT) / `... report --ground-truth <file>` (GT).

**Current fallback results:** HRV == neurokit2 to 0.0000 ms; recovery monotonic in HRV; strain
monotonic in TRIMP with correct log curvature; all metrics within physiological plausibility bounds;
all internal-consistency invariants hold. **No WHOOP-ground-truth comparison has been run** (no data).

---

## Closing the validation gap (the one remaining step)
The WHOOP-validated accuracy claims require the user to supply ground truth. Everything to consume it
is built (`app/whoop_api/`):
1. **Provide WHOOP data** — either a **data export** (WHOOP app → Download my data → `*.csv`) or
   **API access** (register an app at developer-dashboard.whoop.com, authorize once → refresh token;
   see `app/whoop_api/README.md`).
2. **Establish an overlap** — because our raw store starts 2026-05-23 (OpenWhoop only), a comparison
   needs either re-offloading pre-switch nights still in the strap's 14-day flash, or a ≥5-night
   validation-wear window on the official app then re-pair + offload.
3. **Run** `python -m app.analysis.validation report --ground-truth <export-or-pull>` to get the
   per-metric error table vs the §3 targets, then **fit** the calibration knobs (`units.fit_spo2`,
   `units.fit_skin_temp`, `strain.fit_strain_denominator`, recovery k/Z0) on a train split and report
   held-out error.

---

## Test status
All suites green at integration: **pure analysis suite 586 passed**, **Docker suite (test_daily +
test_e2e + test_read*) 10 passed**. Per-module counts: HRV 69, recovery 31, baselines, strain 41,
sleep 49, exercise 25, units 71, whoop_api 87, validation 98.

## Honest bottom line
The pipeline now uses vetted algorithms (Task Force HRV via neurokit2, Cole–Kripke + cardiorespiratory
staging, Karvonen/TRIMP strain, baseline-relative recovery, ratio-of-ratios SpO2) with personal
baselines where WHOOP personalizes. Structurally it is much closer to WHOOP than the prior naive
formulas. **But without WHOOP ground truth, accuracy against WHOOP is unproven** — these are
physiologically-sound, reference-agreeing, internally-consistent *approximations*. The SpO2 and
skin-temp absolute values in particular are un-calibrated. Supplying ground truth turns these from
"plausible" into "validated (or not) within stated tolerances."
