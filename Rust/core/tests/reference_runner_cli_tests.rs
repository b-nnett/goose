use std::{
    fs,
    process::{Command, Stdio},
};

use goose_core::store::GooseStore;

fn python_cmd() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

#[test]
fn neurokit_hrv_adapter_emits_external_reference_contract() {
    let output = Command::new(python_cmd())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("tools/reference/neurokit_hrv.py")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.neurokit2.hrv")
        .arg("--output-format")
        .arg("goose.external-reference-output.v1")
        .arg("--allow-hand-derived-fallback")
        .output()
        .unwrap();
    assert!(output.status.success());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.external-reference-output.v1");
    assert_eq!(report["algorithm_id"], "reference.hrv.neurokit2.v1");
    assert_eq!(report["provider"], "external.neurokit2.hrv");
    assert_eq!(report["provider_version"], "test-fallback");
    assert_eq!(report["output_units"]["rmssd_ms"], "ms");
    assert_close(
        report["output"]["rmssd_ms"].as_f64().unwrap(),
        14.142135623730951,
    );
    assert_eq!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hand_derived_test_fallback"),
        true
    );
    assert_eq!(
        report["provenance"]["library_docs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|url| url.as_str().unwrap_or("").contains("hrv_time")),
        true
    );
}

#[test]
fn pyhrv_time_domain_adapter_emits_external_reference_contract() {
    let output = Command::new(python_cmd())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("tools/reference/pyhrv_time_domain.py")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.pyhrv.hrv")
        .arg("--output-format")
        .arg("goose.external-reference-output.v1")
        .arg("--allow-hand-derived-fallback")
        .output()
        .unwrap();
    assert!(output.status.success());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.external-reference-output.v1");
    assert_eq!(report["algorithm_id"], "reference.hrv.pyhrv_time_domain.v1");
    assert_eq!(report["provider"], "external.pyhrv.hrv");
    assert_eq!(report["provider_version"], "test-fallback");
    assert_eq!(report["output_units"]["pnn50_fraction"], "fraction");
    assert_close(
        report["output"]["sdnn_ms"].as_f64().unwrap(),
        8.16496580927726,
    );
    assert_eq!(report["output"]["nn50_count"], 0);
    assert_eq!(
        report["parameters"]["pyhrv_functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|function| function == "rmssd"),
        true
    );
    assert_eq!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hand_derived_test_fallback"),
        true
    );
}

#[test]
fn pyactigraphy_sadeh_adapter_emits_external_reference_contract() {
    let output = Command::new(python_cmd())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("tools/reference/pyactigraphy_sadeh.py")
        .arg("--input")
        .arg("fixtures/synthetic/sleep_actigraphy_counts_sadeh_hand_derived.json")
        .arg("--family")
        .arg("sleep")
        .arg("--provider")
        .arg("external.pyactigraphy.sadeh")
        .arg("--output-format")
        .arg("goose.external-reference-output.v1")
        .arg("--allow-hand-derived-fallback")
        .output()
        .unwrap();
    assert!(output.status.success());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.external-reference-output.v1");
    assert_eq!(
        report["algorithm_id"],
        "reference.sleep.pyactigraphy_sadeh.v1"
    );
    assert_eq!(report["provider"], "external.pyactigraphy.sadeh");
    assert_eq!(report["provider_version"], "test-fallback");
    assert_eq!(
        report["input_schema"],
        "goose.sleep-actigraphy-counts-input.v1"
    );
    assert_eq!(
        report["output_units"]["sleep_efficiency_fraction"],
        "fraction"
    );
    assert_eq!(report["output"]["epoch_count"], 15);
    assert_eq!(report["output"]["sleep_epoch_count"], 6);
    assert_eq!(report["output"]["wake_epoch_count"], 9);
    assert_close(
        report["output"]["sleep_efficiency_fraction"]
            .as_f64()
            .unwrap(),
        0.4,
    );
    assert_eq!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hand_derived_test_fallback"),
        true
    );
    assert_eq!(
        report["provenance"]["library_docs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|url| url.as_str().unwrap_or("").contains("ScoringMixin.Sadeh")),
        true
    );
}

#[test]
fn ggir_sleep_summary_adapter_emits_external_reference_contract() {
    let output = Command::new(python_cmd())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("tools/reference/ggir_sleep_summary.py")
        .arg("--input")
        .arg("fixtures/synthetic/sleep_ggir_summary_hand_derived.json")
        .arg("--family")
        .arg("sleep")
        .arg("--provider")
        .arg("external.ggir.sleep")
        .arg("--output-format")
        .arg("goose.external-reference-output.v1")
        .output()
        .unwrap();
    assert!(output.status.success());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "goose.external-reference-output.v1");
    assert_eq!(report["algorithm_id"], "reference.sleep.ggir_summary.v1");
    assert_eq!(report["provider"], "external.ggir.sleep");
    assert_eq!(report["provider_version"], "3.3-4");
    assert_eq!(report["input_schema"], "goose.sleep-ggir-summary-input.v1");
    assert_eq!(
        report["output_units"]["wake_after_sleep_onset_minutes"],
        "minutes"
    );
    assert_eq!(report["output"]["night_count"], 2);
    assert_eq!(report["output"]["valid_night_count"], 2);
    assert_close(
        report["output"]["time_in_bed_minutes"].as_f64().unwrap(),
        930.0,
    );
    assert_close(report["output"]["sleep_minutes"].as_f64().unwrap(), 780.0);
    assert_close(
        report["output"]["sleep_efficiency_fraction"]
            .as_f64()
            .unwrap(),
        0.8387096774193549,
    );
    assert_close(
        report["output"]["wake_after_sleep_onset_minutes"]
            .as_f64()
            .unwrap(),
        150.0,
    );
    assert_eq!(report["output"]["disturbance_count"], 1);
    assert_eq!(
        report["provenance"]["library_docs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|url| url.as_str().unwrap_or("").contains("GGIRoutput")),
        true
    );
}

#[test]
fn reference_runner_executes_named_neurokit_hrv_adapter_and_stores_run() {
    let tempdir = tempfile::tempdir().unwrap();
    let report_path = tempdir.path().join("neurokit-reference-report.json");
    let db_path = tempdir.path().join("goose-neurokit-reference.sqlite");

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.neurokit2.hrv")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--external-command")
        .arg(python_cmd())
        .arg("--external-arg")
        .arg("tools/reference/neurokit_hrv.py")
        .arg("--external-arg")
        .arg("--allow-hand-derived-fallback")
        .arg("--db")
        .arg(&db_path)
        .arg("--run-id")
        .arg("external-neurokit-adapter-hrv-1")
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert!(status.success());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["provider_kind"], "external_reference");
    assert_eq!(report["algorithm_id"], "reference.hrv.neurokit2.v1");
    assert_eq!(report["input_valid"], true);
    assert_eq!(report["provider_valid"], true);
    assert_eq!(report["output_ready"], true);
    assert_eq!(report["errors_clear"], true);
    assert_eq!(report["provenance_ready"], true);
    assert_eq!(report["storage_ready"], true);
    assert_eq!(report["reference_ready"], true);
    assert_eq!(report["pass"], true);
    assert_eq!(report["next_actions"].as_array().unwrap().len(), 0);
    assert_eq!(report["provenance"]["output_units"]["rmssd_ms"], "ms");
    assert_eq!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hand_derived_test_fallback"),
        true
    );

    let store = GooseStore::open(&db_path).unwrap();
    let definition = store
        .algorithm_definition("reference.hrv.neurokit2.v1", "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(definition.implementation, "external-reference");
    assert_eq!(definition.status, "benchmark-only");
    assert!(definition.params_json.contains("intervals_to_peaks"));

    let run = store
        .algorithm_run("external-neurokit-adapter-hrv-1")
        .unwrap()
        .unwrap();
    assert_eq!(run.algorithm_id, "reference.hrv.neurokit2.v1");
    assert!(run.output_json.contains("\"rmssd_ms\""));
    assert!(run.provenance_json.contains("\"external_command\""));
}

#[test]
fn reference_runner_reports_machine_readable_blockers_for_insufficient_input() {
    let tempdir = tempfile::tempdir().unwrap();
    let input_path = tempdir.path().join("insufficient-hrv.json");
    let report_path = tempdir.path().join("insufficient-hrv-report.json");
    fs::write(
        &input_path,
        r#"{
  "start_time": "2026-05-27T00:00:00Z",
  "end_time": "2026-05-27T00:01:00Z",
  "rr_intervals_ms": [800.0],
  "input_ids": ["insufficient-fixture"]
}"#,
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("hrv")
        .arg("--input")
        .arg(&input_path)
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["input_valid"], true);
    assert_eq!(report["provider_valid"], true);
    assert_eq!(report["output_ready"], false);
    assert_eq!(report["errors_clear"], false);
    assert_eq!(report["provenance_ready"], true);
    assert_eq!(report["storage_ready"], true);
    assert_eq!(report["reference_ready"], false);
    assert_eq!(report["errors"][0], "not_enough_valid_rr_intervals");
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["reason"] == "reference_output_missing" && action["scope"] == "output"
            })
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["reason"] == "insufficient_reference_input" && action["scope"] == "reference"
            })
    );
}

#[test]
fn reference_runner_executes_named_pyhrv_adapter_and_stores_run() {
    let tempdir = tempfile::tempdir().unwrap();
    let report_path = tempdir.path().join("pyhrv-reference-report.json");
    let db_path = tempdir.path().join("goose-pyhrv-reference.sqlite");

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.pyhrv.hrv")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--external-command")
        .arg(python_cmd())
        .arg("--external-arg")
        .arg("tools/reference/pyhrv_time_domain.py")
        .arg("--external-arg")
        .arg("--allow-hand-derived-fallback")
        .arg("--db")
        .arg(&db_path)
        .arg("--run-id")
        .arg("external-pyhrv-adapter-hrv-1")
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert!(status.success());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["provider_kind"], "external_reference");
    assert_eq!(report["algorithm_id"], "reference.hrv.pyhrv_time_domain.v1");
    assert_eq!(report["provenance"]["output_units"]["rmssd_ms"], "ms");
    assert_eq!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hand_derived_test_fallback"),
        true
    );

    let store = GooseStore::open(&db_path).unwrap();
    let definition = store
        .algorithm_definition("reference.hrv.pyhrv_time_domain.v1", "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(definition.implementation, "external-reference");
    assert_eq!(definition.license, "GPL-3.0");
    assert_eq!(definition.status, "benchmark-only");
    assert!(definition.params_json.contains("nni_parameters"));

    let run = store
        .algorithm_run("external-pyhrv-adapter-hrv-1")
        .unwrap()
        .unwrap();
    assert_eq!(run.algorithm_id, "reference.hrv.pyhrv_time_domain.v1");
    assert!(run.output_json.contains("\"pnn50_fraction\""));
    assert!(run.provenance_json.contains("\"external_command\""));
}

#[test]
fn reference_runner_executes_named_pyactigraphy_sadeh_adapter_and_stores_run() {
    let tempdir = tempfile::tempdir().unwrap();
    let report_path = tempdir.path().join("pyactigraphy-reference-report.json");
    let db_path = tempdir.path().join("goose-pyactigraphy-reference.sqlite");

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("sleep")
        .arg("--provider")
        .arg("external.pyactigraphy.sadeh")
        .arg("--input")
        .arg("fixtures/synthetic/sleep_actigraphy_counts_sadeh_hand_derived.json")
        .arg("--external-command")
        .arg(python_cmd())
        .arg("--external-arg")
        .arg("tools/reference/pyactigraphy_sadeh.py")
        .arg("--external-arg")
        .arg("--allow-hand-derived-fallback")
        .arg("--db")
        .arg(&db_path)
        .arg("--run-id")
        .arg("external-pyactigraphy-sadeh-1")
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert!(status.success());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["provider_kind"], "external_reference");
    assert_eq!(
        report["algorithm_id"],
        "reference.sleep.pyactigraphy_sadeh.v1"
    );
    assert_eq!(
        report["provenance"]["output_units"]["sleep_minutes"],
        "minutes"
    );
    assert_eq!(report["output"]["sleep_epoch_count"], 6);

    let store = GooseStore::open(&db_path).unwrap();
    let definition = store
        .algorithm_definition("reference.sleep.pyactigraphy_sadeh.v1", "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(definition.metric_family, "sleep");
    assert_eq!(definition.implementation, "external-reference");
    assert_eq!(definition.license, "BSD-3-Clause");
    assert_eq!(definition.status, "benchmark-only");
    assert!(definition.params_json.contains("pyActigraphy Sadeh"));

    let run = store
        .algorithm_run("external-pyactigraphy-sadeh-1")
        .unwrap()
        .unwrap();
    assert_eq!(run.algorithm_id, "reference.sleep.pyactigraphy_sadeh.v1");
    assert!(run.output_json.contains("\"sleep_efficiency_fraction\""));
    assert!(run.provenance_json.contains("\"external_command\""));
}

#[test]
fn reference_runner_executes_named_ggir_sleep_adapter_and_stores_run() {
    let tempdir = tempfile::tempdir().unwrap();
    let report_path = tempdir.path().join("ggir-reference-report.json");
    let db_path = tempdir.path().join("goose-ggir-reference.sqlite");

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("sleep")
        .arg("--provider")
        .arg("external.ggir.sleep")
        .arg("--input")
        .arg("fixtures/synthetic/sleep_ggir_summary_hand_derived.json")
        .arg("--external-command")
        .arg(python_cmd())
        .arg("--external-arg")
        .arg("tools/reference/ggir_sleep_summary.py")
        .arg("--db")
        .arg(&db_path)
        .arg("--run-id")
        .arg("external-ggir-sleep-1")
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert!(status.success());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["provider_kind"], "external_reference");
    assert_eq!(report["algorithm_id"], "reference.sleep.ggir_summary.v1");
    assert_eq!(
        report["provenance"]["output_units"]["sleep_efficiency_fraction"],
        "fraction"
    );
    assert_eq!(report["output"]["valid_night_count"], 2);

    let store = GooseStore::open(&db_path).unwrap();
    let definition = store
        .algorithm_definition("reference.sleep.ggir_summary.v1", "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(definition.metric_family, "sleep");
    assert_eq!(definition.implementation, "external-reference");
    assert_eq!(definition.license, "Apache-2.0");
    assert_eq!(definition.status, "benchmark-only");
    assert!(definition.params_json.contains("GGIR sleep summary export"));

    let run = store
        .algorithm_run("external-ggir-sleep-1")
        .unwrap()
        .unwrap();
    assert_eq!(run.algorithm_id, "reference.sleep.ggir_summary.v1");
    assert!(
        run.output_json
            .contains("\"wake_after_sleep_onset_minutes\"")
    );
    assert!(run.provenance_json.contains("\"external_command\""));
}

#[test]
#[cfg(unix)]
fn reference_runner_executes_external_provider_contract_and_stores_run() {
    let tempdir = tempfile::tempdir().unwrap();
    let script_path = tempdir.path().join("external-neurokit-fixture.sh");
    let report_path = tempdir.path().join("external-reference-report.json");
    let db_path = tempdir.path().join("goose-reference.sqlite");
    write_executable_script(
        &script_path,
        r#"#!/bin/sh
cat <<'JSON'
{
  "schema": "goose.external-reference-output.v1",
  "family": "hrv",
  "provider": "external.neurokit2.hrv",
  "provider_version": "0.2.10",
  "source": "NeuroKit2 fixture adapter",
  "license": "MIT",
  "algorithm_id": "reference.hrv.neurokit2.v1",
  "algorithm_version": "1.0.0",
  "display_name": "NeuroKit2 HRV Reference Fixture",
  "input_schema": "goose.hrv-input.v1",
  "output_schema": "goose.hrv-neurokit2-reference-output.v1",
  "start_time": "2026-05-27T00:00:00Z",
  "end_time": "2026-05-27T00:01:00Z",
  "output": {
    "mean_nn_ms": 800.0,
    "rmssd_ms": 14.1421356237,
    "sdnn_ms": 8.1649658093
  },
  "output_units": {
    "mean_nn_ms": "ms",
    "rmssd_ms": "ms",
    "sdnn_ms": "ms"
  },
  "parameters": {
    "rr_cleaning": "none",
    "method": "time_domain"
  },
  "input_requirements": {
    "rr_intervals_ms": {
      "unit": "ms",
      "minimum_to_compute": 2
    }
  },
  "quality_gates": [
    "external_provider_exit_zero",
    "units_recorded"
  ],
  "quality_flags": [],
  "errors": [],
  "provenance": {
    "library": "NeuroKit2",
    "library_version": "0.2.10",
    "fixture": "hand-derived-hrv"
  }
}
JSON
"#,
    );

    let status = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.neurokit2.hrv")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--external-command")
        .arg(&script_path)
        .arg("--external-arg")
        .arg("--fixture-provider-mode")
        .arg("--db")
        .arg(&db_path)
        .arg("--run-id")
        .arg("external-neurokit-hrv-1")
        .arg("--output")
        .arg(&report_path)
        .status()
        .unwrap();
    assert!(status.success());

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.reference-algo-report.v1");
    assert_eq!(report["provider_kind"], "external_reference");
    assert_eq!(report["pass"], true);
    assert_eq!(report["algorithm_id"], "reference.hrv.neurokit2.v1");
    assert_eq!(report["output"]["rmssd_ms"], 14.1421356237);
    assert_eq!(
        report["provenance"]["output_units"]["rmssd_ms"],
        serde_json::Value::String("ms".to_string())
    );
    assert_eq!(
        report["provenance"]["external_report_provenance"]["library"],
        "NeuroKit2"
    );
    assert_eq!(
        report["provenance"]["external_command"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--fixture-provider-mode"),
        true
    );
    assert_eq!(
        report["provenance"]["external_command"]["input_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let store = GooseStore::open(&db_path).unwrap();
    let definition = store
        .algorithm_definition("reference.hrv.neurokit2.v1", "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(definition.metric_family, "hrv");
    assert_eq!(definition.implementation, "external-reference");
    assert_eq!(definition.license, "MIT");
    assert_eq!(definition.status, "benchmark-only");
    assert!(definition.params_json.contains("NeuroKit2 fixture adapter"));

    let run = store
        .algorithm_run("external-neurokit-hrv-1")
        .unwrap()
        .unwrap();
    assert_eq!(run.algorithm_id, "reference.hrv.neurokit2.v1");
    assert!(run.output_json.contains("\"rmssd_ms\""));
    assert!(run.provenance_json.contains("\"external_reference\""));
}

#[test]
#[cfg(unix)]
fn reference_runner_rejects_external_provider_with_wrong_schema() {
    let tempdir = tempfile::tempdir().unwrap();
    let script_path = tempdir.path().join("bad-external-reference.sh");
    write_executable_script(
        &script_path,
        r#"#!/bin/sh
cat <<'JSON'
{
  "schema": "not-goose",
  "family": "hrv",
  "provider": "external.neurokit2.hrv",
  "provider_version": "0.2.10",
  "source": "bad fixture",
  "license": "MIT",
  "algorithm_id": "reference.hrv.neurokit2.v1",
  "algorithm_version": "1.0.0",
  "start_time": "2026-05-27T00:00:00Z",
  "end_time": "2026-05-27T00:01:00Z",
  "output": {
    "rmssd_ms": 14.0
  },
  "output_units": {
    "rmssd_ms": "ms"
  },
  "provenance": {
    "fixture": "bad-schema"
  }
}
JSON
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.neurokit2.hrv")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--external-command")
        .arg(&script_path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected external reference schema"));
}

#[test]
#[cfg(unix)]
fn reference_runner_rejects_external_provider_without_output_units() {
    let tempdir = tempfile::tempdir().unwrap();
    let script_path = tempdir.path().join("missing-units-reference.sh");
    write_executable_script(
        &script_path,
        r#"#!/bin/sh
cat <<'JSON'
{
  "schema": "goose.external-reference-output.v1",
  "family": "hrv",
  "provider": "external.neurokit2.hrv",
  "provider_version": "0.2.10",
  "source": "bad fixture",
  "license": "MIT",
  "algorithm_id": "reference.hrv.neurokit2.v1",
  "algorithm_version": "1.0.0",
  "start_time": "2026-05-27T00:00:00Z",
  "end_time": "2026-05-27T00:01:00Z",
  "output": {
    "rmssd_ms": 14.0
  },
  "output_units": {},
  "provenance": {
    "fixture": "missing-units"
  }
}
JSON
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_goose-reference-algo-runner"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--family")
        .arg("hrv")
        .arg("--provider")
        .arg("external.neurokit2.hrv")
        .arg("--input")
        .arg("fixtures/synthetic/hrv_goose_v0_hand_derived.json")
        .arg("--external-command")
        .arg(&script_path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("output_units must record units"));
}

fn write_executable_script(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}
