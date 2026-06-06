use goose_core::{
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    protocol::{DeviceType, PACKET_TYPE_REALTIME_RAW_DATA, build_v5_payload_frame},
    store::GooseStore,
};

#[test]
fn metric_feature_report_cli_builds_motion_report_from_owned_capture() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    import_motion_frame(&store);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("motion")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-05-30T00:00:00Z")
        .arg("--end")
        .arg("2026-05-31T00:00:00Z")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.motion-feature-report.v1");
    assert_eq!(report["generated_by"], "goose-motion-feature-extractor");
    assert_eq!(report["pass"], true);
    assert_eq!(report["feature_count"], 1);
    assert_eq!(report["trusted_feature_count"], 1);
    assert_eq!(report["features"][0]["body_summary_kind"], "raw_motion_k10");
    assert_eq!(report["features"][0]["trusted_metric_input"], true);
}

#[test]
fn metric_feature_report_cli_emits_heart_rate_blockers_without_trusted_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("heart-rate")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-05-30T00:00:00Z")
        .arg("--end")
        .arg("2026-05-31T00:00:00Z")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.heart-rate-feature-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["feature_count"], 0);
    assert_eq!(report["trusted_feature_count"], 0);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_trusted_heart_rate_features")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["reason"] == "no_trusted_heart_rate_features")
    );
}

#[test]
fn metric_feature_report_cli_runs_step_packet_discovery_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("step-discovery")
        .arg("--database")
        .arg(&db)
        .arg("--start-time-unix-ms")
        .arg("1780355200000")
        .arg("--end-time-unix-ms")
        .arg("1780441600000")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.step-packet-discovery-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["decoded_frame_count"], 0);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_step_discovery_frames")
    );
}

#[test]
fn metric_feature_report_cli_runs_step_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("step-validation")
        .arg("--database")
        .arg(&db)
        .arg("--start-time-unix-ms")
        .arg("1780355200000")
        .arg("--end-time-unix-ms")
        .arg("1780441600000")
        .arg("--capture-kind")
        .arg("100_counted_steps")
        .arg("--manual-step-delta")
        .arg("100")
        .arg("--official-whoop-step-delta")
        .arg("97")
        .arg("--step-delta-tolerance")
        .arg("5")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.step-capture-validation-report.v1");
    assert_eq!(report["capture_kind"], "100_counted_steps");
    assert_eq!(report["manual_step_delta"], 100);
    assert_eq!(report["official_whoop_step_delta"], 97);
    assert_eq!(report["tolerance_steps"], 5);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_explicit_step_counter_field_found")
    );
}

#[test]
fn metric_feature_report_cli_runs_raw_motion_step_estimate_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("raw-motion-steps")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--manual-step-delta")
        .arg("100")
        .arg("--step-delta-tolerance")
        .arg("10")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.raw-motion-step-estimate-report.v1");
    assert_eq!(report["algorithm_id"], "goose.steps.raw_motion_estimate.v0");
    assert_eq!(report["source_kind_if_promoted"], "local_estimate");
    assert_eq!(report["manual_step_delta"], 100);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_raw_motion_step_estimator_frames")
    );
}

#[test]
fn metric_feature_report_cli_runs_step_counter_ingest_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("step-counter-ingest")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.step-counter-ingest-report.v1");
    assert_eq!(report["pass"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_step_counter_candidates_to_persist")
    );
}

#[test]
fn metric_feature_report_cli_runs_step_counter_daily_rollup_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("step-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start-time-unix-ms")
        .arg("1780355200000")
        .arg("--end-time-unix-ms")
        .arg("1780441600000")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.step-counter-daily-rollup-report.v1"
    );
    assert_eq!(report["pass"], false);
    assert_eq!(report["daily_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "insufficient_step_counter_samples")
    );
}

#[test]
fn metric_feature_report_cli_runs_step_counter_hourly_rollup_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("hourly-step-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start-time-unix-ms")
        .arg("1780387200000")
        .arg("--end-time-unix-ms")
        .arg("1780390800000")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.step-counter-hourly-rollup-report.v1"
    );
    assert_eq!(report["pass"], false);
    assert_eq!(report["hourly_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "insufficient_step_counter_samples")
    );
}

#[test]
fn metric_feature_report_cli_runs_activity_unavailable_status_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("steps-unavailable-status")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start-time-unix-ms")
        .arg("1780355200000")
        .arg("--end-time-unix-ms")
        .arg("1780441600000")
        .arg("--min-step-samples")
        .arg("2")
        .arg("--write-metric")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.activity-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["unavailable_metric_count"], 1);
    assert_eq!(report["written_metric_count"], 1);
    assert_eq!(report["statuses"][0]["metric_id"], "steps");
    assert_eq!(report["statuses"][0]["source_kind"], "unavailable");

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(
        store
            .daily_activity_metrics_between(0, i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|row| row.source_kind == "unavailable")
            .count(),
        1
    );
}

#[test]
fn metric_feature_report_cli_runs_resting_heart_rate_daily_rollup_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("rhr-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--min-samples")
        .arg("2")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.resting-heart-rate-daily-rollup-report.v1"
    );
    assert_eq!(report["pass"], false);
    assert_eq!(report["daily_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "insufficient_heart_rate_samples")
    );
}

#[test]
fn metric_feature_report_cli_runs_resting_heart_rate_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("rhr-validation")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--min-samples")
        .arg("2")
        .arg("--official-whoop-resting-hr-bpm")
        .arg("56")
        .arg("--rhr-tolerance-bpm")
        .arg("3")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.resting-heart-rate-capture-validation-report.v1"
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["official_whoop_resting_hr_bpm"], 56.0);
    assert_eq!(report["resting_hr_rollup"]["daily_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
}

#[test]
fn metric_feature_report_cli_runs_energy_daily_rollup_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("energy-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--profile-weight-kg")
        .arg("80")
        .arg("--resting-hr-bpm")
        .arg("60")
        .arg("--max-hr-bpm")
        .arg("180")
        .arg("--min-heart-rate-samples")
        .arg("2")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.energy-daily-rollup-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["daily_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "insufficient_heart_rate_samples")
    );
}

#[test]
fn metric_feature_report_cli_runs_energy_unavailable_status_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("calories-unavailable-status")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--profile-weight-kg")
        .arg("80")
        .arg("--resting-hr-bpm")
        .arg("60")
        .arg("--max-hr-bpm")
        .arg("180")
        .arg("--min-heart-rate-samples")
        .arg("2")
        .arg("--write-metric")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.energy-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["unavailable_metric_count"], 3);
    assert_eq!(report["written_metric_count"], 3);
    assert!(
        report["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|status| status["metric_id"] == "active_kcal"
                && status["source_kind"] == "unavailable")
    );

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(
        store
            .daily_activity_metrics_between(0, i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|row| row.source_kind == "unavailable")
            .count(),
        3
    );
}

#[test]
fn metric_feature_report_cli_runs_energy_hourly_rollup_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("hourly-energy-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T12:00:00Z")
        .arg("--end")
        .arg("2026-06-02T13:00:00Z")
        .arg("--profile-weight-kg")
        .arg("80")
        .arg("--resting-hr-bpm")
        .arg("60")
        .arg("--max-hr-bpm")
        .arg("180")
        .arg("--min-heart-rate-samples")
        .arg("2")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.energy-hourly-rollup-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["hourly_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "insufficient_heart_rate_samples")
    );
}

#[test]
fn metric_feature_report_cli_runs_energy_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("energy-validation")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--official-whoop-total-kcal")
        .arg("2100")
        .arg("--energy-tolerance-kcal")
        .arg("250")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.energy-capture-validation-report.v1"
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["official_whoop_total_kcal"], 2100.0);
    assert_eq!(report["energy_rollup"]["daily_metric_written"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "energy_rollup_blocked")
    );
}

#[test]
fn metric_feature_report_cli_merges_extra_json_args_for_hrv_options() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("hrv")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-05-30T00:00:00Z")
        .arg("--end")
        .arg("2026-05-31T00:00:00Z")
        .arg("--require-trusted-evidence")
        .arg("--args-json")
        .arg(r#"{"min_owned_captures":1,"min_rr_intervals_to_compute":4,"require_baseline":true,"baseline_min_days":2}"#)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.hrv-feature-report.v1");
    assert_eq!(report["pass"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_trusted_hrv_features")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["reason"] == "no_trusted_hrv_features")
    );
}

#[test]
fn metric_feature_report_cli_runs_hrv_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("hrv-validation")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--capture-kind")
        .arg("overnight_rest")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--min-rr-intervals-to-compute")
        .arg("2")
        .arg("--official-whoop-hrv-rmssd-ms")
        .arg("42")
        .arg("--hrv-tolerance-ms")
        .arg("10")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.hrv-capture-validation-report.v1");
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_hrv_rmssd_ms"], 42.0);
    assert_eq!(report["tolerance_ms"], 10.0);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "hrv_feature_report_blocked")
    );
    assert!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hrv_rr_interval_scale_unverified")
    );
}

#[test]
fn metric_feature_report_cli_runs_respiratory_rate_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("respiratory-rate-validation")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--capture-kind")
        .arg("overnight_rest")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--official-whoop-respiratory-rate-rpm")
        .arg("14.5")
        .arg("--respiratory-rate-tolerance-rpm")
        .arg("1")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.respiratory-rate-capture-validation-report.v1"
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_respiratory_rate_rpm"], 14.5);
    assert_eq!(report["tolerance_rpm"], 1.0);
    assert_eq!(
        report["promotion_status"],
        "validation_only_respiratory_rate_semantics_still_unverified"
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_respiratory_rate_packet_candidate")
    );
}

#[test]
fn metric_feature_report_cli_runs_oxygen_saturation_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("spo2-validation")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--capture-kind")
        .arg("overnight_rest")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--official-whoop-spo2-percent")
        .arg("97.0")
        .arg("--spo2-tolerance-percent")
        .arg("2.0")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.oxygen-saturation-capture-validation-report.v1"
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_oxygen_saturation_percent"], 97.0);
    assert_eq!(report["tolerance_percent"], 2.0);
    assert_eq!(report["source_kind"], "unavailable");
    assert_eq!(
        report["promotion_status"],
        "validation_only_oxygen_saturation_decoder_not_implemented"
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "oxygen_saturation_decoder_not_implemented")
    );
}

#[test]
fn metric_feature_report_cli_runs_temperature_capture_validation_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("temperature-validation")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--capture-kind")
        .arg("overnight_rest")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--official-whoop-skin-temperature-delta-c")
        .arg("0.2")
        .arg("--skin-temperature-tolerance-c")
        .arg("0.3")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.temperature-capture-validation-report.v1"
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_skin_temperature_delta_c"], 0.2);
    assert_eq!(report["tolerance_c"], 0.3);
    assert_eq!(report["source_kind"], "unavailable");
    assert_eq!(
        report["promotion_status"],
        "validation_only_temperature_units_still_unverified"
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_temperature_packet_candidate")
    );
}

#[test]
fn metric_feature_report_cli_runs_recovery_sensor_discovery_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("recovery-sensors")
        .arg("--database")
        .arg(&db)
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-03T00:00:00Z")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.recovery-sensor-discovery-report.v1"
    );
    assert_eq!(report["pass"], false);
    assert_eq!(report["widgets"].as_array().unwrap().len(), 4);
    assert!(report["issues"].as_array().unwrap().iter().any(
        |issue| issue == "oxygen_saturation_percent:oxygen_saturation_decoder_not_implemented"
    ));
    assert!(
        report["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|widget| widget["metric_id"] == "hrv_rmssd_ms"
                && widget["source_kind"] == "unavailable"
                && widget["confidence"] == 0.0)
    );
}

#[test]
fn metric_feature_report_cli_runs_recovery_unavailable_status_alias() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("recovery-unavailable-status")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-02T08:00:00Z")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--min-rr-intervals-to-compute")
        .arg("2")
        .arg("--write-metric")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.recovery-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["unavailable_metric_count"], 4);
    assert_eq!(report["written_metric_count"], 4);
    assert!(
        report["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|status| status["metric_id"] == "skin_temperature_delta_c"
                && status["source_kind"] == "unavailable")
    );

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(
        store
            .daily_recovery_metrics_between(0, i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|row| row.source_kind == "unavailable")
            .count(),
        4
    );
}

#[test]
fn metric_feature_report_cli_runs_recovery_sensor_daily_rollup_alias_without_promoting_blocked_candidates()
 {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-metric-feature-report"))
        .arg("--method")
        .arg("recovery-sensor-rollup")
        .arg("--database")
        .arg(&db)
        .arg("--date-key")
        .arg("2026-06-02")
        .arg("--timezone")
        .arg("Europe/London")
        .arg("--start")
        .arg("2026-06-02T00:00:00Z")
        .arg("--end")
        .arg("2026-06-02T08:00:00Z")
        .arg("--min-owned-captures")
        .arg("1")
        .arg("--require-trusted-evidence")
        .arg("--min-rr-intervals-to-compute")
        .arg("2")
        .arg("--write-metric")
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.recovery-sensor-daily-rollup-report.v1"
    );
    assert_eq!(report["pass"], false);
    assert_eq!(report["metric_count"], 4);
    assert_eq!(report["promotable_metric_count"], 0);
    assert_eq!(report["promoted_metric_count"], 0);
    assert_eq!(report["written_metric_count"], 0);
    assert!(
        report["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|status| status["metric_id"] == "hrv_rmssd_ms"
                && status["source_kind"] == "unavailable"
                && status["local_value"].is_null())
    );

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(
        store
            .daily_recovery_metrics_between(0, i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|row| row.source_kind == "device_sensor"
                && (row.hrv_rmssd_ms.is_some()
                    || row.respiratory_rate_rpm.is_some()
                    || row.oxygen_saturation_percent.is_some()
                    || row.skin_temperature_delta_c.is_some()))
            .count(),
        0
    );
}

fn import_motion_frame(store: &GooseStore) {
    let frames = vec![CapturedFrameInput {
        evidence_id: "metric-feature-cli-motion".to_string(),
        frame_id: Some("metric-feature-cli-motion.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-30T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: k10_motion_frame_hex(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let report = import_captured_frame_batch(
        store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/metric-feature-cli-test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(report.pass, "{:?}", report.issues);
}

fn k10_motion_frame_hex() -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    for offset in [85, 285, 485, 688, 888, 1088] {
        for index in 0..100 {
            put_i16(&mut payload, offset + index * 2, 1000);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
