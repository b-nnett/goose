---
status: investigating
trigger: "Investigate Rust test failures on CI (Linux x86_64) that pass locally (macOS ARM64)"
created: 2026-06-11T00:00:00Z
updated: 2026-06-11T00:10:00Z
---

## Current Focus

hypothesis: ALL 15 failures reproduce LOCALLY on macOS ARM64 — this is NOT a Linux/macOS difference
test: Ran all failing tests locally with cargo test -- --nocapture
expecting: Root causes confirmed by local error messages
next_action: Investigation complete — root causes identified, 4 distinct causes found

reasoning_checkpoint:
  hypothesis: "All 15 failures are pre-existing bugs in Rust source or test assertions, not platform-specific issues"
  confirming_evidence:
    - "bridge_exposes_algorithm_registry, hrv_comparison, keytel, exercise_detection, export all FAIL locally on macOS ARM64"
    - "Error messages match exactly what CI reports"
    - "Failures are deterministic, not intermittent"
  falsification_test: "If failures were platform-specific, they would not reproduce locally on macOS ARM64"
  fix_rationale: "Fix the underlying bugs/test mismatches, not CI environment"
  blind_spots: "We cannot confirm CI output directly, but local reproduction is conclusive"

## Symptoms

expected: All Rust tests pass on both macOS ARM64 (local) and Linux x86_64 (CI)
actual: 15 tests fail — reproduce locally too, so NOT a Linux vs macOS issue
errors: |
  - hrv_comparison: ["goose:not_enough_valid_rr_intervals"] at algorithm_compare_tests.rs:26
  - bridge_exposes_algorithm_registry: left: Bool(false) right: true at bridge_tests.rs:751
  - bridge_builds_local_recovery_score: left: Bool(false) right: true at bridge_tests.rs:6524
  - bridge_builds_local_stress_score: left: Bool(false) right: true at bridge_tests.rs:6715
  - bridge_validates_hrv: left: Bool(false) right: true at bridge_tests.rs:3550
  - bridge_runs_property_suite: panics "valid HRV input" at property_tests.rs:694
  - bridge_persists_algorithm_preferences: left: 6 right: 5 at bridge_tests.rs:1175
  - keytel_male: female Keytel: expected 0.2449..., got 14.687...
  - keytel_female: expected 0.1453..., got 8.712...
  - test_detect_sessions_roundtrip: "expected at least 1 session detected, got 0"
  - test_detect_sessions_gap_merge: "two 6-min windows with 41 s gap should merge into 1 session, got 0"
  - exports_sqlite_timeframe: left: 0 right: 9 at export_tests.rs:518
  - raw_export_can_select_metric_outputs_only: left: 0 right: 9 at export_tests.rs:1146
  - algo_benchmark: assertion failed: status.success() at algo_benchmark_tests.rs:201
reproduction: cargo test --locked in Rust/core on macOS ARM64 (or any platform)
started: Pre-existing, before v9.0 Swift changes

## Eliminated

- hypothesis: failures are Linux x86_64-specific (floating point, platform ABI)
  evidence: all failures reproduce on macOS ARM64 locally with identical error messages
  timestamp: 2026-06-11

- hypothesis: Python script path/dependency issues for algo_benchmark
  evidence: algo_benchmark fails because compare_hrv_goose_to_reference returns pass=false (HRV error), not Python
  timestamp: 2026-06-11

## Evidence

- timestamp: 2026-06-11
  checked: ran all 15 failing tests locally
  found: ALL reproduce on macOS ARM64 — this is not a platform-specific issue
  implication: root causes are in Rust source or test assertions

- timestamp: 2026-06-11
  checked: metrics.rs:981 — goose_hrv_v0 minimum RR interval count
  found: if valid.len() < 20 { errors.push("not_enough_valid_rr_intervals") }
  implication: hardcoded minimum of 20 — ALL test fixtures with 4 RR intervals will fail

- timestamp: 2026-06-11
  checked: algorithm_compare_tests, bridge_tests, algo_benchmark fixtures, property_tests
  found: all use rr_intervals_ms: [800.0, 810.0, 790.0, 800.0] — only 4 intervals
  implication: 4 < 20 → goose_hrv_v0 always fails for these inputs

- timestamp: 2026-06-11
  checked: reference.rs:222 — reference_hrv_time_domain minimum
  found: if valid.len() < 2 { errors.push("not_enough_valid_rr_intervals") }
  implication: reference uses min=2, goose uses min=20 — this mismatch causes compare to fail

- timestamp: 2026-06-11
  checked: energy_rollup.rs:1215-1216 — keytel divisor
  found: "(raw / 4.1868).max(0.0)" — divides by 4.1868 (kJ→kcal conversion factor)
  implication: Tests expect division by 251.04 (the Keytel paper denominator). Code and tests disagree on the formula.

- timestamp: 2026-06-11
  checked: exercise_detection.rs:17 — MOTION_THRESHOLD
  found: "pub const MOTION_THRESHOLD: f64 = 0.20" with comment "0.01 was below MEMS quantisation noise"
  implication: Tests use x=0.15,y=0,z=1.0 → smoothed_mag ≈ 0.011 < 0.20 → all active pairs filtered out → 0 sessions

- timestamp: 2026-06-11
  checked: metrics.rs:533-545 — built_in_default_algorithm_preferences()
  found: returns 6 entries (hrv, sleep, strain, recovery, stress, readiness)
  implication: "readiness" was added but test at bridge_tests.rs:1175 still expects 5 entries

## Resolution

root_cause: |
  4 distinct root causes, all pre-existing bugs:

  ROOT CAUSE A — HRV minimum intervals threshold (affects 11 of 15 failures):
  goose_hrv_v0() has a hardcoded minimum of 20 valid RR intervals (metrics.rs:981).
  All test fixtures and direct test inputs use only 4 RR intervals, which is below this threshold.
  The parameter "hrv_min_rr_intervals_to_compute: 2" in bridge requests controls whether an
  HrvInput is constructed, but does NOT lower the 20-interval minimum inside goose_hrv_v0 itself.
  Affected: hrv_comparison, bridge_exposes_algorithm_registry, bridge_builds_local_recovery_score,
            bridge_builds_local_stress_score, bridge_validates_hrv, bridge_runs_property_suite,
            exports_sqlite_timeframe, raw_export_can_select_metric_outputs_only, algo_benchmark.

  ROOT CAUSE B — Keytel formula divisor mismatch (affects 2 of 15 failures):
  energy_rollup.rs uses /4.1868 (kJ→kcal factor) but the Keytel (2005) formula already
  produces kcal/min when divided by 251.04. Tests expect /251.04.
  Ratio: 251.04 / 4.1868 ≈ 59.96 (≈ 60 minutes) explains the ~60x discrepancy.
  Affected: keytel_active_kcal_per_min_male_exact_coefficients,
            keytel_active_kcal_per_min_female_exact_coefficients

  ROOT CAUSE C — MOTION_THRESHOLD raised without updating tests (affects 2 of 15 failures):
  MOTION_THRESHOLD was changed from 0.01 to 0.20 g in exercise_detection.rs, but test
  helper build_gravity_rows() was not updated. It uses x=0.15,y=0,z=1.0 → smoothed mag ≈ 0.011,
  which is > 0.01 (old threshold) but < 0.20 (new threshold), so all samples are filtered out.
  Affected: test_detect_sessions_roundtrip, test_detect_sessions_gap_merge

  ROOT CAUSE D — Algorithm preference count mismatch (affects 1 of 15 failures):
  built_in_default_algorithm_preferences() now returns 6 entries (added "readiness"),
  but bridge_tests.rs:1175 still asserts == 5.
  Affected: bridge_persists_algorithm_preferences_for_settings_algorithms

fix:
verification:
files_changed: []

## Still-Failing Set (2026-06-12, after Nyquist phase-67 work)

These 14 tests fail at HEAD — confirmed pre-existing (same Root Cause A, different test functions not touched by 9a5d3b3):
- bridge_builds_local_strain_score_from_feature_reports
- bridge_aggregates_metric_window_features_for_debug_score_inputs
- bridge_builds_local_recovery_score_from_feature_reports_and_provided_vitals
- bridge_builds_local_stress_score_from_feature_reports
- bridge_energy_confidence_uses_only_device_counter_step_cadence_support
- bridge_extracts_heart_rate_features_for_debug_score_inputs
- bridge_extracts_resting_heart_rate_features_for_debug_score_inputs
- bridge_exports_raw_timeframe_for_debug_export_flow
- bridge_rolls_up_local_energy_into_daily_activity_metric
- bridge_rolls_up_resting_heart_rate_into_daily_recovery_metric
- bridge_rolls_up_local_energy_into_hourly_activity_metric
- bridge_validates_local_energy_against_whoop_labels_without_writing_metric
- bridge_validates_respiratory_rate_against_whoop_label_without_promoting_metric
- bridge_validates_resting_heart_rate_against_whoop_label_without_writing_metric

Confirmed: NOT caused by phase-67 Nyquist changes (which only touched r22_shadowed_r17_frame_ids dedup logic).
All were introduced in 46f1638 (Ship Goose Swift MVP), 9a5d3b3 did not modify them.
