use std::{
    ffi::{CStr, CString},
    fs,
    path::Path,
};

use goose_core::{
    bridge::{
        BRIDGE_RESPONSE_SCHEMA, BridgeResponse, goose_bridge_free_string, goose_bridge_handle_json,
        goose_core_version_json, handle_bridge_request_json,
    },
    calibration::{
        CalibrationDataset, CalibrationOptions, calibration_run_record, evaluate_linear_calibration,
    },
    capture_import::{CaptureImportOptions, import_fixture_index},
    commands::{COMMAND_DEFINITIONS, CommandEvidence, validate_commands},
    energy_rollup::{
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID, GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION,
        GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_ID, GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_VERSION,
    },
    export::validate_export_bundle,
    fixtures::build_fixture_index,
    metrics::{GOOSE_HRV_V0_ID, GOOSE_HRV_V0_VERSION, built_in_algorithm_definitions},
    protocol::{
        DeviceType, PACKET_TYPE_EVENT, PACKET_TYPE_HISTORICAL_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
        build_v5_payload_frame, parse_frame_hex,
    },
    recovery_rollup::{
        GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_ID, GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_VERSION,
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID,
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION,
    },
    step_counter::{
        GOOSE_ACTIVITY_UNAVAILABLE_STATUS_V0_ID, GOOSE_ACTIVITY_UNAVAILABLE_STATUS_V0_VERSION,
    },
    step_motion_estimator::{
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_ID, GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_VERSION,
    },
    store::{
        ActivitySessionInput, AlgorithmRunRecord, CURRENT_SCHEMA_VERSION, CalibrationLabelInput,
        CaptureSessionInput, CommandValidationRecord, DailyActivityMetricInput,
        DailyRecoveryMetricInput, DecodedFrameRow, GooseStore, HourlyActivityMetricInput,
        RawEvidenceInput, StepCounterSampleInput,
    },
};
use rusqlite::Connection;

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";
const GET_HELLO_RESPONSE_FRAME: &str = "aa010c000001e7412409910100000000401adc66";
const COMMAND_SERVICE_UUID: &str = "61080001-0000-1000-8000-00805f9b34fb";
const COMMAND_CHARACTERISTIC_UUID: &str = "61080002-0000-1000-8000-00805f9b34fb";
const COMMAND_WRITE_TYPE: &str = "with_response";

#[test]
fn bridge_returns_core_version_payload() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "version-1",
        "method": "core.version",
        "args": {}
    }));

    assert!(response.ok, "{:?}", response.error);
    assert_eq!(response.request_id, "version-1");
    let result = response.result.unwrap();
    assert_eq!(result["bridge_request_schema"], "goose.bridge.request.v1");
    assert_eq!(result["bridge_response_schema"], BRIDGE_RESPONSE_SCHEMA);
    assert_eq!(result["storage_schema_version"], CURRENT_SCHEMA_VERSION);
}

#[test]
fn bridge_returns_openwhoop_reference_report() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "openwhoop-reference-1",
        "method": "openwhoop.reference_report",
        "args": {}
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.openwhoop-reference-report.v1");
    assert_eq!(result["generated_by"], "goose-bridge");
    assert_eq!(result["service_role_count"], 2);
    assert_eq!(result["history_field_count"], 11);
    assert_eq!(
        result["snapshot"]["repository"],
        "https://github.com/bWanShiTong/openwhoop"
    );
    assert_eq!(
        result["snapshot"]["commit"],
        "55c5c1e2e02d3822c33e258838a57bb7d9e2ca53"
    );
    assert!(
        result["snapshot"]["snapshot_url"]
            .as_str()
            .unwrap()
            .contains("openwhoop/tree/55c5c1e2e02d3822c33e258838a57bb7d9e2ca53")
    );
    assert!(
        result["snapshot"]["attribution"]
            .as_str()
            .unwrap()
            .contains("OpenWhoop snapshot")
    );
    assert!(
        result["snapshot"]["license_caveat"]
            .as_str()
            .unwrap()
            .contains("license file")
    );
    assert_eq!(result["service_roles"].as_array().unwrap().len(), 2);
    assert_eq!(result["service_roles"][0]["generation"], "Gen4");
    assert_eq!(result["service_roles"][1]["generation"], "Gen5");
    assert_eq!(
        result["service_roles"][0]["characteristic_roles"][0]["role"],
        "command_to_strap"
    );
    assert_eq!(
        result["service_roles"][1]["characteristic_roles"][4]["role"],
        "memfault"
    );
    assert_eq!(result["history_fields"].as_array().unwrap().len(), 11);
    assert!(
        result["history_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["field"] == "gravity" && field["status"] == "conflicting")
    );
}

#[test]
fn bridge_runs_historical_sync_dry_run() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "historical-sync-dry-run-1",
        "method": "historical_sync.dry_run",
        "args": {
            "schema": "goose.historical-sync-dry-run.v1",
            "generation": "gen5",
            "request_data_range": true,
            "fake_events": [
                {"kind": "history_start"},
                {"kind": "reading", "name": "synthetic-reading"},
                {"kind": "history_end"},
                {"kind": "history_complete"}
            ]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.historical-sync-dry-run-report.v1");
    assert_eq!(result["generated_by"], "goose-historical-sync-dry-run");
    assert_eq!(result["generation"], "gen5");
    assert_eq!(result["pass"], true);
    assert_eq!(result["state"], "complete");
    assert_eq!(result["planned_command_count"], 3);
    assert!(
        result["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["kind"] == "send_historical_data"
                && step["payload_expectation"] == "empty")
    );
}

#[test]
fn bridge_validates_historical_sync_physical_evidence() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "historical-sync-physical-validation-1",
        "method": "historical_sync.validate_physical_evidence",
        "args": {
            "schema": "goose.historical-sync-physical-validation.v1",
            "generation": "gen5",
            "capture_session_id": "strap-capture-2026-01-01",
            "service_uuids": ["fd4b0001-cce1-4033-93ce-002d5875f58a"],
            "characteristics": [
                {
                    "service_uuid": "fd4b0001-cce1-4033-93ce-002d5875f58a",
                    "characteristic_uuid": "fd4b0002-cce1-4033-93ce-002d5875f58a",
                    "role": "command_to_strap",
                    "properties": ["write_without_response"]
                },
                {
                    "service_uuid": "fd4b0001-cce1-4033-93ce-002d5875f58a",
                    "characteristic_uuid": "fd4b0003-cce1-4033-93ce-002d5875f58a",
                    "role": "data_from_strap",
                    "properties": ["notify"]
                },
                {
                    "service_uuid": "fd4b0001-cce1-4033-93ce-002d5875f58a",
                    "characteristic_uuid": "fd4b0004-cce1-4033-93ce-002d5875f58a",
                    "role": "event_from_strap",
                    "properties": ["notify"]
                }
            ],
            "notification_subscriptions": [
                {
                    "characteristic_uuid": "fd4b0003-cce1-4033-93ce-002d5875f58a",
                    "enabled": true,
                    "capture_session_id": "strap-capture-2026-01-01"
                },
                {
                    "characteristic_uuid": "fd4b0004-cce1-4033-93ce-002d5875f58a",
                    "enabled": true,
                    "capture_session_id": "strap-capture-2026-01-01"
                }
            ],
            "auth_events": [
                {"name": "connected", "sequence": 1, "capture_session_id": "strap-capture-2026-01-01"},
                {"name": "authenticated", "sequence": 2, "capture_session_id": "strap-capture-2026-01-01"},
                {"name": "subscribed", "sequence": 3, "capture_session_id": "strap-capture-2026-01-01"}
            ],
            "command_events": [
                {"command": "send_historical_data", "sequence": 4, "response_observed": true, "capture_session_id": "strap-capture-2026-01-01"},
                {"command": "historical_data_result", "sequence": 8, "response_observed": true, "capture_session_id": "strap-capture-2026-01-01"}
            ],
            "metadata_events": [
                {"name": "HistoryStart", "sequence": 5, "capture_session_id": "strap-capture-2026-01-01"},
                {"name": "HistoryEnd", "sequence": 6, "capture_session_id": "strap-capture-2026-01-01"},
                {"name": "HistoryComplete", "sequence": 7, "capture_session_id": "strap-capture-2026-01-01"}
            ],
            "timestamp_evidence": [
                {
                    "packet_kind": "raw_motion_k21",
                    "source_signal": "raw_motion_k21",
                    "captured_at": "2026-01-01T20:00:00Z",
                    "sample_time": "2026-01-01T22:00:00Z",
                    "sample_time_source": "device_timestamp",
                    "device_timestamp_seconds": 1767304800,
                    "device_timestamp_subseconds": 0,
                    "capture_session_id": "strap-capture-2026-01-01"
                },
                {
                    "packet_kind": "normal_history",
                    "source_signal": "heart_rate",
                    "captured_at": "2026-01-01T20:00:00Z",
                    "sample_time": "2026-01-01T22:05:00Z",
                    "sample_time_source": "device_timestamp",
                    "device_timestamp_seconds": 1767305100,
                    "capture_session_id": "strap-capture-2026-01-01"
                }
            ],
            "raw_evidence_anchors": [
                {"evidence_id": "physical-raw-0", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "observation_kind": "notification_subscription", "observation_name": "fd4b0003cce1403393ce002d5875f58a", "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-1", "sha256": "0000000000000000000000000000000000000000000000000000000000000001", "observation_kind": "notification_subscription", "observation_name": "fd4b0004cce1403393ce002d5875f58a", "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-2", "sha256": "0000000000000000000000000000000000000000000000000000000000000002", "observation_kind": "auth_event", "observation_name": "connected", "sequence": 1, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-3", "sha256": "0000000000000000000000000000000000000000000000000000000000000003", "observation_kind": "auth_event", "observation_name": "authenticated", "sequence": 2, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-4", "sha256": "0000000000000000000000000000000000000000000000000000000000000004", "observation_kind": "auth_event", "observation_name": "subscribed", "sequence": 3, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-5", "sha256": "0000000000000000000000000000000000000000000000000000000000000005", "observation_kind": "command_event", "observation_name": "send_historical_data", "sequence": 4, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-6", "sha256": "0000000000000000000000000000000000000000000000000000000000000006", "observation_kind": "metadata_event", "observation_name": "history_start", "sequence": 5, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-7", "sha256": "0000000000000000000000000000000000000000000000000000000000000007", "observation_kind": "metadata_event", "observation_name": "history_end", "sequence": 6, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-8", "sha256": "0000000000000000000000000000000000000000000000000000000000000008", "observation_kind": "metadata_event", "observation_name": "history_complete", "sequence": 7, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-9", "sha256": "0000000000000000000000000000000000000000000000000000000000000009", "observation_kind": "command_event", "observation_name": "historical_data_result", "sequence": 8, "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-10", "sha256": "0000000000000000000000000000000000000000000000000000000000000010", "observation_kind": "timestamp_evidence", "observation_name": "raw_motion_k21:raw_motion_k21", "capture_session_id": "strap-capture-2026-01-01"},
                {"evidence_id": "physical-raw-11", "sha256": "0000000000000000000000000000000000000000000000000000000000000011", "observation_kind": "timestamp_evidence", "observation_name": "normal_history:heart_rate", "capture_session_id": "strap-capture-2026-01-01"}
            ]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(
        result["schema"],
        "goose.historical-sync-physical-validation-report.v1"
    );
    assert_eq!(
        result["generated_by"],
        "goose-historical-sync-physical-validator"
    );
    assert_eq!(result["pass"], true);
    assert_eq!(result["service_uuid_confirmed"], true);
    assert_eq!(result["notification_behavior_confirmed"], true);
    assert_eq!(result["auth_session_handshake_confirmed"], true);
    assert_eq!(result["command_flow_confirmed"], true);
    assert_eq!(result["evidence_session_confirmed"], true);
    assert_eq!(result["raw_evidence_anchored"], true);
    assert_eq!(result["timestamp_fields_confirmed"], true);
}

#[test]
fn bridge_returns_historical_sync_physical_evidence_template() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "historical-sync-physical-template-1",
        "method": "historical_sync.physical_evidence_template",
        "args": {
            "generation": "gen5",
            "capture_session_id": "strap-capture-template"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(
        result["schema"],
        "goose.historical-sync-physical-evidence-template.v1"
    );
    assert_eq!(result["generation"], "gen5");
    assert_eq!(result["capture_session_id"], "strap-capture-template");
    assert_eq!(
        result["input"]["schema"],
        "goose.historical-sync-physical-validation.v1"
    );
    assert_eq!(
        result["expected_service_uuid"],
        "fd4b0001cce1403393ce002d5875f58a"
    );
    assert!(
        result["required_observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["reason"] == "historical_command_flow_incomplete")
    );
}

#[test]
fn bridge_validates_sleep_v1_release_gates_fail_closed() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-v1-release-gates-1",
        "method": "sleep.validate_v1_release_gates",
        "args": {
            "input": {}
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.sleep-v1-release-gate-report.v1");
    assert_eq!(result["pass"], false);
    assert_eq!(result["physical_historical_sync_pass"], false);
    assert_eq!(result["timestamp_evidence_pass"], false);
    assert!(
        result["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "physical_historical_sync_not_validated")
    );
}

#[test]
fn bridge_validates_sleep_v1_evidence_folder_fail_closed() {
    let tempdir = tempfile::tempdir().unwrap();
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-v1-evidence-folder-1",
        "method": "sleep.validate_v1_evidence_folder",
        "args": {
            "evidence_dir": tempdir.path().to_str().unwrap(),
            "expected_manifest_sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(
        result["schema"],
        "goose.sleep-v1-validation-evidence-folder-report.v1"
    );
    assert_eq!(result["pass"], false);
    assert_eq!(result["required_file_count"], 6);
    assert_eq!(
        result["expected_evidence_manifest_sha256"],
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert!(
        result["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "missing_required_file:sleep-v1-release-gate.json")
    );
    assert!(
        result["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "evidence_manifest_sha256_mismatch")
    );
}

#[test]
fn bridge_parses_frame_hex_for_app_import_flow() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "parse-1",
        "method": "protocol.parse_frame_hex",
        "args": {
            "device_type": "GOOSE",
            "frame_hex": GET_HELLO_FRAME
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let parsed = response.result.unwrap();
    assert_eq!(parsed["packet_type"], 35);
    assert_eq!(parsed["packet_type_name"], "COMMAND");
    assert_eq!(parsed["parsed_payload"]["command_name"], "GET_HELLO");
}

#[test]
fn bridge_exposes_algorithm_registry_and_score_methods() {
    let registry = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-registry-1",
        "method": "metrics.built_in_definitions",
        "args": {}
    }));
    assert!(registry.ok, "{:?}", registry.error);
    let definitions = registry.result.unwrap();
    assert_eq!(definitions.as_array().unwrap().len(), 6);
    assert!(
        definitions
            .as_array()
            .unwrap()
            .iter()
            .any(|definition| definition["algorithm_id"] == "goose.recovery.v0")
    );
    assert!(
        definitions
            .as_array()
            .unwrap()
            .iter()
            .any(|definition| definition["algorithm_id"] == "goose.sleep.v1")
    );

    let references = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-reference-registry-1",
        "method": "metrics.reference_definitions",
        "args": {}
    }));
    assert!(references.ok, "{:?}", references.error);
    let reference_definitions = references.result.unwrap();
    assert_eq!(reference_definitions.as_array().unwrap().len(), 4);
    assert!(reference_definitions.as_array().unwrap().iter().any(
        |definition| definition["algorithm_id"] == "reference.sleep.actigraphy_summary.v1"
            && definition["status"] == "benchmark-only"
    ));
    assert!(reference_definitions.as_array().unwrap().iter().any(
        |definition| definition["algorithm_id"] == "reference.stress.hrv_hr_proxy.v1"
            && definition["metric_family"] == "stress"
    ));

    let sleep = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-sleep-1",
        "method": "metrics.goose_sleep_v0",
        "args": {
            "start_time": "2026-05-27T22:30:00Z",
            "end_time": "2026-05-28T06:30:00Z",
            "sleep_duration_minutes": 420.0,
            "sleep_need_minutes": 480.0,
            "time_in_bed_minutes": 480.0,
            "midpoint_deviation_minutes": 30.0,
            "disturbance_count": 4,
            "input_ids": ["bridge.sleep.fixture"]
        }
    }));
    assert!(sleep.ok, "{:?}", sleep.error);
    assert_eq!(sleep.result.unwrap()["output"]["score_0_to_100"], 84.875);

    let sleep_v1 = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-sleep-v1-1",
        "method": "metrics.goose_sleep_v1",
        "args": {
            "start_time": "2026-05-27T22:30:00Z",
            "end_time": "2026-05-28T06:30:00Z",
            "sleep_duration_minutes": 420.0,
            "sleep_need_minutes": 480.0,
            "time_in_bed_minutes": 480.0,
            "midpoint_deviation_minutes": 30.0,
            "disturbance_count": 4,
            "sleep_latency_minutes": 18.0,
            "wake_after_sleep_onset_minutes": 42.0,
            "wake_episode_count": 2,
            "stage_minutes": {
                "awake": 60.0,
                "core": 210.0,
                "deep": 90.0,
                "rem": 120.0
            },
            "heart_rate_dip_percent": 12.5,
            "input_ids": ["bridge.sleep.v1.fixture"],
            "model_status": {
                "sleep_permission_granted": true,
                "imported_platform_sleep_nights": 10,
                "trusted_goose_sleep_nights": 2,
                "motion_coverage_fraction": 0.94,
                "heart_rate_coverage_fraction": 0.82
            },
            "rolling_sleep_debt_minutes": 90.0,
            "bedtime_deviation_minutes": 20.0,
            "wake_time_deviation_minutes": 15.0,
            "sleep_hr_average_bpm": 61.0,
            "sleep_hr_min_bpm": 54.0,
            "sleep_hr_trend_bpm_per_hour": -1.2,
            "naps_minutes": 25.0,
            "prior_day_strain": 8.5,
            "data_coverage_fraction": 0.92
        }
    }));
    assert!(sleep_v1.ok, "{:?}", sleep_v1.error);
    let sleep_v1_result = sleep_v1.result.unwrap();
    assert_eq!(sleep_v1_result["algorithm_id"], "goose.sleep.v1");
    assert_eq!(sleep_v1_result["output"]["model_status"], "baseline_ready");
    assert_eq!(
        sleep_v1_result["output"]["model_status_label"],
        "Baseline ready"
    );
    let sleep_v1_score = sleep_v1_result["output"]["score_0_to_100"]
        .as_f64()
        .unwrap();
    assert!(
        (sleep_v1_score - 82.01361892264234).abs() < 1e-9,
        "expected hand-derived sleep v1 score, got {sleep_v1_score}"
    );
    assert_eq!(sleep_v1_result["output"]["deep_sleep_minutes"], 90.0);
    assert_eq!(
        sleep_v1_result["output"]["sleep_hr_trend_bpm_per_hour"],
        -1.2
    );
    assert_eq!(
        sleep_v1_result["output"]["quality_flags"],
        serde_json::json!([])
    );
    assert_eq!(
        sleep_v1_result["output"]["provenance"]["score_policy"],
        "weighted_sleep_v1_components_with_fragmentation_guardrails"
    );
    assert_eq!(
        sleep_v1_result["output"]["status_report"]["can_show_personal_baseline"],
        true
    );
    assert_eq!(
        sleep_v1_result["output"]["component_provenance"]["continuity"]["policy"],
        "efficiency_latency_waso_and_wake_episode_curve"
    );
    assert_eq!(
        sleep_v1_result["output"]["component_provenance"]["data_confidence"]["inputs"]["heart_rate_coverage_fraction"],
        0.82
    );

    let sleep_v1_quality = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-v1-explanation-stability-1",
        "method": "sleep.validate_v1_explanation_stability",
        "args": {
            "input": {
                "start_time": "2026-05-27T22:30:00Z",
                "end_time": "2026-05-28T06:30:00Z",
                "sleep_duration_minutes": 420.0,
                "sleep_need_minutes": 480.0,
                "time_in_bed_minutes": 480.0,
                "midpoint_deviation_minutes": 30.0,
                "disturbance_count": 4,
                "sleep_latency_minutes": 18.0,
                "wake_after_sleep_onset_minutes": 42.0,
                "wake_episode_count": 2,
                "stage_minutes": {
                    "awake": 60.0,
                    "core": 210.0,
                    "deep": 90.0,
                    "rem": 120.0
                },
                "heart_rate_dip_percent": 12.5,
                "input_ids": ["bridge.sleep.v1.fixture"],
                "prior_nights": [
                    {
                        "night_id": "bridge.sleep.v1.previous",
                        "start_time": "2026-05-26T22:40:00Z",
                        "end_time": "2026-05-27T06:20:00Z",
                        "sleep_duration_minutes": 405.0,
                        "sleep_need_minutes": 480.0,
                        "time_in_bed_minutes": 460.0,
                        "awake_minutes": 55.0,
                        "sleep_latency_minutes": 20.0,
                        "wake_after_sleep_onset_minutes": 35.0,
                        "wake_episode_count": 3,
                        "stage_minutes": {
                            "awake": 55.0,
                            "core": 205.0,
                            "deep": 80.0,
                            "rem": 120.0
                        },
                        "heart_rate_dip_percent": 11.0,
                        "sleep_hr_average_bpm": 62.0,
                        "sleep_hr_min_bpm": 55.0,
                        "pre_sleep_awake_hr_average_bpm": 68.0,
                        "sleep_hr_trend_bpm_per_hour": -1.0,
                        "bedtime_deviation_minutes": 25.0,
                        "wake_time_deviation_minutes": 12.0,
                        "midpoint_deviation_minutes": 28.0
                    }
                ],
                "model_status": {
                    "sleep_permission_granted": true,
                    "imported_platform_sleep_nights": 10,
                    "trusted_goose_sleep_nights": 2,
                    "motion_coverage_fraction": 0.94,
                    "heart_rate_coverage_fraction": 0.82
                },
                "rolling_sleep_debt_minutes": 90.0,
                "bedtime_deviation_minutes": 20.0,
                "wake_time_deviation_minutes": 15.0,
                "sleep_hr_average_bpm": 61.0,
                "sleep_hr_min_bpm": 54.0,
                "sleep_hr_trend_bpm_per_hour": -1.2,
                "naps_minutes": 25.0,
                "prior_day_strain": 8.5,
                "data_coverage_fraction": 0.92
            }
        }
    }));
    assert!(sleep_v1_quality.ok, "{:?}", sleep_v1_quality.error);
    let sleep_v1_quality_result = sleep_v1_quality.result.unwrap();
    assert_eq!(
        sleep_v1_quality_result["schema"],
        "goose.sleep-v1-explanation-stability-report.v1"
    );
    assert_eq!(sleep_v1_quality_result["pass"], true);
    assert_eq!(sleep_v1_quality_result["explanation_pass"], true);
    assert_eq!(sleep_v1_quality_result["repeated_run_stability_pass"], true);
    assert_eq!(sleep_v1_quality_result["perturbation_stability_pass"], true);

    let comparison = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-reference-compare-1",
        "method": "metrics.reference_compare",
        "args": {
            "family": "hrv",
            "input": {
                "start_time": "2026-05-27T00:00:00Z",
                "end_time": "2026-05-27T00:01:00Z",
                "rr_intervals_ms": [800.0, 810.0, 790.0, 800.0],
                "input_ids": ["bridge.hrv.reference"]
            }
        }
    }));
    assert!(comparison.ok, "{:?}", comparison.error);
    let comparison_result = comparison.result.unwrap();
    assert_eq!(
        comparison_result["schema"],
        "goose.algorithm-comparison-report.v1"
    );
    assert_eq!(comparison_result["family"], "hrv");
    assert_eq!(comparison_result["pass"], true);
    assert_eq!(comparison_result["deltas"].as_array().unwrap().len(), 4);
    assert_eq!(comparison_result["deltas"][0]["absolute_delta"], 0.0);

    let stress_comparison = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-reference-stress-1",
        "method": "metrics.reference_compare",
        "args": {
            "family": "stress",
            "input": {
                "start_time": "2026-05-28T12:00:00Z",
                "end_time": "2026-05-28T12:05:00Z",
                "heart_rate_bpm": 90.0,
                "resting_hr_bpm": 60.0,
                "hrv_rmssd_ms": 25.0,
                "hrv_baseline_rmssd_ms": 50.0,
                "motion_intensity_0_to_1": 0.0,
                "input_ids": ["bridge.stress.reference"]
            }
        }
    }));
    assert!(stress_comparison.ok, "{:?}", stress_comparison.error);
    let stress_comparison_result = stress_comparison.result.unwrap();
    assert_eq!(stress_comparison_result["family"], "stress");
    assert_eq!(stress_comparison_result["pass"], true);
    assert_eq!(
        stress_comparison_result["deltas"].as_array().unwrap().len(),
        2
    );

    let external_sleep_comparison = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-reference-external-sleep-1",
        "method": "metrics.reference_compare",
        "args": {
            "family": "sleep",
            "input": {
                "start_time": "2026-05-27T22:30:00Z",
                "end_time": "2026-05-28T06:30:00Z",
                "sleep_duration_minutes": 420.0,
                "sleep_need_minutes": 480.0,
                "time_in_bed_minutes": 480.0,
                "midpoint_deviation_minutes": 30.0,
                "disturbance_count": 4,
                "input_ids": ["bridge.sleep.external-reference"]
            },
            "reference_report": {
                "schema": "goose.reference-algo-report.v1",
                "family": "sleep",
                "algorithm_id": "reference.sleep.ggir_summary.v1",
                "algorithm_version": "1.0.0",
                "start_time": "2026-05-27T22:30:00Z",
                "end_time": "2026-05-28T06:30:00Z",
                "output": {
                    "time_in_bed_minutes": 480.0,
                    "sleep_minutes": 420.0,
                    "wake_minutes": 60.0,
                    "sleep_efficiency_fraction": 0.875,
                    "wake_after_sleep_onset_minutes": 60.0,
                    "disturbance_count": 4,
                    "fragmentation_index_per_hour": 0.5714285714285714
                },
                "quality_flags": [],
                "errors": [],
                "provenance": {
                    "provider_kind": "external_reference",
                    "external_provider": "external.ggir.sleep",
                    "output_units": {
                        "time_in_bed_minutes": "minutes",
                        "sleep_minutes": "minutes",
                        "wake_minutes": "minutes",
                        "sleep_efficiency_fraction": "fraction",
                        "wake_after_sleep_onset_minutes": "minutes",
                        "disturbance_count": "count",
                        "fragmentation_index_per_hour": "events_per_hour"
                    }
                }
            }
        }
    }));
    assert!(
        external_sleep_comparison.ok,
        "{:?}",
        external_sleep_comparison.error
    );
    let external_sleep = external_sleep_comparison.result.unwrap();
    assert_eq!(
        external_sleep["reference_algorithm_id"],
        "reference.sleep.ggir_summary.v1"
    );
    assert_eq!(external_sleep["reference_contract_valid"], true);
    assert_eq!(external_sleep["shared_fields_ready"], true);
    assert_eq!(external_sleep["deltas"].as_array().unwrap().len(), 7);

    let external_sleep_v1_comparison = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-reference-external-sleep-v1-1",
        "method": "metrics.reference_compare",
        "args": {
            "family": "sleep",
            "goose_algorithm_id": "goose.sleep.v1",
            "input": {
                "sleep": {
                    "start_time": "2026-05-27T22:30:00Z",
                    "end_time": "2026-05-28T06:30:00Z",
                    "sleep_duration_minutes": 420.0,
                    "sleep_need_minutes": 480.0,
                    "time_in_bed_minutes": 480.0,
                    "midpoint_deviation_minutes": 30.0,
                    "disturbance_count": 4,
                    "wake_after_sleep_onset_minutes": 60.0,
                    "input_ids": ["bridge.sleep-v1.external-reference"]
                },
                "model_status": {
                    "sleep_permission_granted": true,
                    "imported_platform_sleep_nights": 7,
                    "motion_coverage_fraction": 0.92,
                    "heart_rate_coverage_fraction": 0.80
                },
                "data_coverage_fraction": 0.90
            },
            "reference_report": {
                "schema": "goose.reference-algo-report.v1",
                "family": "sleep",
                "algorithm_id": "reference.sleep.ggir_summary.v1",
                "algorithm_version": "1.0.0",
                "start_time": "2026-05-27T22:30:00Z",
                "end_time": "2026-05-28T06:30:00Z",
                "output": {
                    "time_in_bed_minutes": 480.0,
                    "sleep_minutes": 420.0,
                    "wake_minutes": 60.0,
                    "sleep_efficiency_fraction": 0.875,
                    "wake_after_sleep_onset_minutes": 60.0,
                    "disturbance_count": 4,
                    "fragmentation_index_per_hour": 0.5714285714285714
                },
                "quality_flags": [],
                "errors": [],
                "provenance": {
                    "provider_kind": "external_reference",
                    "external_provider": "external.ggir.sleep",
                    "output_units": {
                        "time_in_bed_minutes": "minutes",
                        "sleep_minutes": "minutes",
                        "wake_minutes": "minutes",
                        "sleep_efficiency_fraction": "fraction",
                        "wake_after_sleep_onset_minutes": "minutes",
                        "disturbance_count": "count",
                        "fragmentation_index_per_hour": "events_per_hour"
                    }
                }
            }
        }
    }));
    assert!(
        external_sleep_v1_comparison.ok,
        "{:?}",
        external_sleep_v1_comparison.error
    );
    let external_sleep_v1 = external_sleep_v1_comparison.result.unwrap();
    assert_eq!(external_sleep_v1["goose_algorithm_id"], "goose.sleep.v1");
    assert_eq!(
        external_sleep_v1["reference_algorithm_id"],
        "reference.sleep.ggir_summary.v1"
    );
    assert_eq!(external_sleep_v1["reference_contract_valid"], true);
    assert_eq!(external_sleep_v1["shared_fields_ready"], true);
    assert_eq!(
        external_sleep_v1["provenance"]["comparison_policy"],
        "sleep_v1_shared_sleep_wake_summary_fields"
    );
}

#[test]
fn bridge_rejects_unsupported_primary_algorithm_for_packet_derived_score() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "packet-derived-primary-algorithm-1",
        "method": "metrics.hrv_features",
        "args": {
            "database_path": db.display().to_string(),
            "start": "0000",
            "end": "9999",
            "algorithm_id": "reference.hrv.time_domain.v1",
            "algorithm_version": "1.0.0"
        }
    }));

    assert!(!response.ok);
    let error = response.error.unwrap();
    assert_eq!(error.code, "method_error");
    assert!(error.message.contains("unsupported primary algorithm"));
    assert!(error.message.contains("goose.hrv.v0@0.1.0"));
}

#[test]
fn bridge_exposes_command_definitions_for_device_and_debug_controls() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-definitions-1",
        "method": "commands.definitions",
        "args": {}
    }));

    assert!(response.ok, "{:?}", response.error);
    let definitions = response.result.unwrap();
    let definitions = definitions.as_array().unwrap();
    assert_eq!(definitions.len(), COMMAND_DEFINITIONS.len());

    for expected in [
        (
            "send_historical_data",
            "historical_sync",
            "user_visible_state_change",
        ),
        ("run_alarm", "alarm_haptics", "user_visible_state_change"),
        (
            "select_wrist",
            "wrist_selection",
            "user_visible_state_change",
        ),
        (
            "start_raw_data",
            "sensor_stream",
            "user_visible_state_change",
        ),
        (
            "set_device_config_value",
            "device_config",
            "critical_state_change",
        ),
        (
            "set_feature_flag_value",
            "feature_flags",
            "critical_state_change",
        ),
        (
            "start_firmware_load_new",
            "firmware_dfu",
            "critical_state_change",
        ),
        (
            "reboot_strap",
            "reboot_maintenance",
            "critical_state_change",
        ),
    ] {
        assert!(
            definitions.iter().any(|definition| {
                definition["id"] == expected.0
                    && definition["family"] == expected.1
                    && definition["risk_gate"] == expected.2
            }),
            "missing command definition {expected:?}"
        );
    }
}

#[test]
fn bridge_runs_ui_coverage_audit_for_debug_coverage_surface() {
    // The UI coverage audit reads the Android APK UI inventory, an external
    // reference artifact that is not committed to this repository. Skip the
    // audit assertions when that inventory is absent so the suite stays green
    // in checkouts (and CI) that do not vendor the APK UI inventory.
    let coverage_map = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../apk-ui-inventory/coverage-map.json");
    if !coverage_map.exists() {
        eprintln!(
            "skipping bridge_runs_ui_coverage_audit_for_debug_coverage_surface: {} not present",
            coverage_map.display()
        );
        return;
    }

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "ui-coverage-1",
        "method": "ui_coverage.audit",
        "args": {}
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["inventory"]["navigation_count"], 236);
    assert_eq!(result["inventory"]["layout_count"], 1419);
    assert_eq!(result["inventory"]["source_class_count"], 857);
    assert_eq!(
        result["inventory"]["navigation_destinations_sha256"],
        "e14d013399520ba43f23eb4f2556b8b98fa4d16733b56ead43b215a20b68014e"
    );
    assert_eq!(
        result["inventory"]["layouts_sha256"],
        "7fee602b72ea2963dff44a1112d10ebe9b4376a81f6836f1bd759049627d99c3"
    );
    assert_eq!(
        result["inventory"]["source_ui_classes_sha256"],
        "39e620e0d40012bf9f509c83ac48d81fc9893b01ad39dde531ac7dd49c40c6c5"
    );
    assert_eq!(result["navigation"]["missing_count"], 0);
    assert_eq!(result["layouts"]["missing_count"], 0);
    assert_eq!(result["source_classes"]["missing_count"], 0);
    assert_eq!(result["has_deferred_review_debt"], false);
    assert_eq!(result["navigation"]["deferred_count"], 0);
    assert_eq!(result["layouts"]["deferred_count"], 0);
    assert_eq!(result["source_classes"]["deferred_count"], 0);
    assert!(
        result["navigation"]["deferred_surfaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        result["layouts"]["deferred_surfaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        result["source_classes"]["deferred_surfaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(result["next_actions"].as_array().unwrap().is_empty());
}

#[test]
fn bridge_runs_perf_budget_for_debug_surface() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "perf-budget-1",
        "method": "diagnostics.perf_budget",
        "args": {
            "scale": 16
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.perf-budget-report.v1");
    assert_eq!(result["generated_by"], "goose-perf-budget");
    assert_eq!(result["scale"], 16);
    assert_eq!(result["pass"], true);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["parser_workload_ready"], true);
    assert_eq!(result["deframer_workload_ready"], true);
    assert_eq!(result["score_workload_ready"], true);
    assert_eq!(result["export_workload_ready"], true);
    assert_eq!(result["duration_budget_ready"], true);
    assert_eq!(result["memory_budget_ready"], true);
    assert_eq!(result["correctness_ready"], true);
    assert_eq!(result["all_workloads_ready"], true);
    assert_eq!(result["perf_budget_ready"], true);
    assert_eq!(result["workloads"].as_array().unwrap().len(), 4);
    assert_eq!(result["next_actions"].as_array().unwrap().len(), 0);
    assert!(
        result["workloads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workload| workload["name"] == "raw_export_bundle")
    );
}

#[test]
fn bridge_runs_property_suite_for_debug_surface() {
    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "property-suite-1",
        "method": "diagnostics.property_suite",
        "args": {
            "seed": 42,
            "cases_per_group": 16
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.property-test-report.v1");
    assert_eq!(result["generated_by"], "goose-property-test-suite");
    assert_eq!(result["seed"], 42);
    assert_eq!(result["cases_per_group"], 16);
    assert_eq!(result["pass"], true);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["parser_properties_valid"], true);
    assert_eq!(result["deframer_properties_valid"], true);
    assert_eq!(result["algorithm_bounds_valid"], true);
    assert_eq!(result["algorithm_metamorphic_valid"], true);
    assert_eq!(result["all_groups_valid"], true);
    assert_eq!(result["property_suite_ready"], true);
    assert_eq!(result["groups"].as_array().unwrap().len(), 4);
    assert!(result["issues"].as_array().unwrap().is_empty());
    assert!(result["next_actions"].as_array().unwrap().is_empty());

    let rejected = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "property-suite-bad-1",
        "method": "diagnostics.property_suite",
        "args": {
            "cases_per_group": 0
        }
    }));
    assert!(!rejected.ok);
    assert!(rejected.error.unwrap().message.contains("cases_per_group"));
}

#[test]
fn bridge_persists_algorithm_preferences_for_settings_algorithms() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let defaults = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-defaults-1",
        "method": "metrics.default_preferences",
        "args": {}
    }));
    assert!(defaults.ok, "{:?}", defaults.error);
    assert_eq!(defaults.result.unwrap().as_array().unwrap().len(), 5);

    let applied = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-apply-1",
        "method": "settings.apply_default_algorithm_preferences",
        "args": {
            "database_path": db_path,
            "scope": "global"
        }
    }));
    assert!(applied.ok, "{:?}", applied.error);
    assert_eq!(applied.result.unwrap().as_array().unwrap().len(), 5);

    let recovery = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-get-1",
        "method": "settings.get_algorithm_preference",
        "args": {
            "database_path": db_path,
            "scope": "global",
            "metric_family": "recovery"
        }
    }));
    assert!(recovery.ok, "{:?}", recovery.error);
    assert_eq!(
        recovery.result.unwrap()["algorithm_id"],
        "goose.recovery.v0"
    );

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-list-1",
        "method": "settings.list_algorithm_preferences",
        "args": {
            "database_path": db_path,
            "scope": "global"
        }
    }));
    assert!(list.ok, "{:?}", list.error);
    assert_eq!(list.result.unwrap().as_array().unwrap().len(), 5);

    let set_debug = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-set-1",
        "method": "settings.set_algorithm_preference",
        "args": {
            "database_path": db_path,
            "scope": "debug-comparison",
            "metric_family": "sleep",
            "algorithm_id": "goose.sleep.v0",
            "version": "0.1.0"
        }
    }));
    assert!(set_debug.ok, "{:?}", set_debug.error);
    assert_eq!(set_debug.result.unwrap()["scope"], "debug-comparison");
}

#[test]
fn bridge_applies_stored_calibration_run_to_local_score() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    seed_recovery_calibration(&db);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-apply-1",
        "method": "calibration.apply",
        "args": {
            "database_path": db_path,
            "metric_family": "recovery",
            "algorithm_id": "goose.recovery.v0",
            "algorithm_version": "0.1.0",
            "raw_score": 70.0,
            "input_run_id": "recovery-run-1",
            "calibration_run_id": "calibration-run-1",
            "score_min": 0.0,
            "score_max": 100.0
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["calibrated_score"], 79.0);
    assert_eq!(result["output_kind"], "goose_calibrated_local_score");
    assert_eq!(result["official_labels_are_labels"], true);
}

#[test]
fn bridge_evaluates_and_persists_calibration_dataset_for_app_metrics() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    let dataset: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/synthetic/recovery_calibration_linear.json"
    ))
    .unwrap();

    let evaluation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-evaluate-1",
        "method": "calibration.evaluate_dataset",
        "args": {
            "database_path": db_path,
            "persist": true,
            "calibration_run_id": "app-recovery-calibration",
            "dataset": dataset,
            "options": {
                "metric_family": "recovery",
                "algorithm_id": "goose.recovery.v0",
                "algorithm_version": "0.1.0",
                "split_at": "2026-05-04T00:00:00Z",
                "min_train_rows": 2,
                "min_holdout_rows": 1
            }
        }
    }));

    assert!(evaluation.ok, "{:?}", evaluation.error);
    let evaluation_result = evaluation.result.unwrap();
    assert_eq!(evaluation_result["pass"], true);
    assert_eq!(evaluation_result["persisted"], true);
    assert_eq!(
        evaluation_result["calibration_run_id"],
        "app-recovery-calibration"
    );
    assert_eq!(evaluation_result["model"]["slope"], 1.2);
    assert_eq!(evaluation_result["model"]["intercept"], -5.0);
    assert_eq!(evaluation_result["holdout_improved"], true);
    assert_eq!(evaluation_result["dataset_valid"], true);
    assert_eq!(evaluation_result["labels_valid"], true);
    assert_eq!(evaluation_result["split_valid"], true);
    assert_eq!(evaluation_result["model_fit_ready"], true);
    assert_eq!(evaluation_result["holdout_metrics_ready"], true);
    assert_eq!(evaluation_result["calibration_ready"], true);

    let applied = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-apply-app-1",
        "method": "calibration.apply",
        "args": {
            "database_path": db_path,
            "metric_family": "recovery",
            "algorithm_id": "goose.recovery.v0",
            "algorithm_version": "0.1.0",
            "raw_score": 70.0,
            "input_run_id": "flutter-sample-recovery",
            "calibration_run_id": "app-recovery-calibration",
            "score_min": 0.0,
            "score_max": 100.0
        }
    }));
    assert!(applied.ok, "{:?}", applied.error);
    let applied_result = applied.result.unwrap();
    assert_eq!(applied_result["pass"], true);
    assert_eq!(applied_result["input_valid"], true);
    assert_eq!(applied_result["calibration_run_valid"], true);
    assert_eq!(applied_result["model_ready"], true);
    assert_eq!(applied_result["model_applied"], true);
    assert_eq!(applied_result["application_ready"], true);
    assert_eq!(applied_result["raw_score"], 70.0);
    assert_eq!(applied_result["calibrated_score"], 79.0);
    assert_eq!(
        applied_result["provenance"]["label_policy"],
        "user_owned_labels_only"
    );
}

#[test]
fn bridge_imports_and_lists_user_owned_calibration_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let imported = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-label-import-1",
        "method": "calibration.import_labels",
        "args": {
            "database_path": db_path,
            "labels": [
                {
                    "label_id": "manual.recovery.2026-05-04",
                    "metric_family": "recovery",
                    "label_source": "manual",
                    "captured_at": "2026-05-04T00:00:00Z",
                    "value": 79.0,
                    "unit": "score_0_to_100",
                    "provenance": {
                        "entry": "typed_by_user",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }
    }));

    assert!(imported.ok, "{:?}", imported.error);
    let import_result = imported.result.unwrap();
    assert_eq!(
        import_result["schema"],
        "goose.calibration-label-import-report.v1"
    );
    assert_eq!(import_result["inserted"], 1);
    assert_eq!(import_result["official_labels_are_labels"], true);
    assert_eq!(import_result["labels"][0]["label_source"], "manual");

    let listed = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-label-list-1",
        "method": "calibration.list_labels",
        "args": {
            "database_path": db_path,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-10T00:00:00Z"
        }
    }));

    assert!(listed.ok, "{:?}", listed.error);
    let list_result = listed.result.unwrap();
    assert_eq!(list_result["schema"], "goose.calibration-label-list.v1");
    assert_eq!(list_result["label_count"], 1);
    assert_eq!(list_result["official_labels_are_labels"], true);
    assert_eq!(list_result["labels"][0]["value"], 79.0);
}

#[test]
fn bridge_evaluates_stored_labels_against_local_algorithm_runs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    seed_stored_recovery_calibration_inputs(&db);

    let evaluation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-stored-labels-1",
        "method": "calibration.evaluate_stored_labels",
        "args": {
            "database_path": db_path,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-06T00:00:00Z",
            "persist": true,
            "calibration_run_id": "stored-recovery-calibration",
            "options": {
                "metric_family": "recovery",
                "algorithm_id": "goose.recovery.v0",
                "algorithm_version": "0.1.0",
                "split_at": "2026-05-04T00:00:00Z",
                "min_train_rows": 2,
                "min_holdout_rows": 1
            }
        }
    }));

    assert!(evaluation.ok, "{:?}", evaluation.error);
    let result = evaluation.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["dataset_record_count"], 5);
    assert_eq!(result["matched_record_count"], 5);
    assert_eq!(result["label_count"], 5);
    assert_eq!(result["persisted"], true);
    assert_eq!(result["model"]["slope"], 1.2);
    assert_eq!(result["model"]["intercept"], -5.0);
    assert_eq!(
        result["matched_records"][0]["algorithm_run_id"],
        "stored-recovery-run-1"
    );
    assert_eq!(result["official_labels_are_labels"], true);

    let applied = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-stored-apply-1",
        "method": "calibration.apply",
        "args": {
            "database_path": db_path,
            "metric_family": "recovery",
            "algorithm_id": "goose.recovery.v0",
            "algorithm_version": "0.1.0",
            "raw_score": 70.0,
            "score_min": 0.0,
            "score_max": 100.0
        }
    }));
    assert!(applied.ok, "{:?}", applied.error);
    assert_eq!(applied.result.unwrap()["calibrated_score"], 79.0);
}

#[test]
fn bridge_evaluates_sleep_v1_stored_labels_with_date_holdout_and_sessions() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    seed_stored_sleep_v1_calibration_inputs(&db);

    let evaluation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-v1-calibration-stored-labels-1",
        "method": "calibration.evaluate_stored_labels",
        "args": {
            "database_path": db_path,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-07T00:00:00Z",
            "persist": true,
            "calibration_run_id": "stored-sleep-v1-calibration",
            "options": {
                "metric_family": "sleep",
                "algorithm_id": "goose.sleep.v1",
                "algorithm_version": "0.1.0",
                "split_at": "2026-05-04T00:00:00Z",
                "min_train_rows": 3,
                "min_holdout_rows": 2
            }
        }
    }));

    assert!(evaluation.ok, "{:?}", evaluation.error);
    let result = evaluation.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["metric_family"], "sleep");
    assert_eq!(result["algorithm_id"], "goose.sleep.v1");
    assert_eq!(result["dataset_record_count"], 6);
    assert_eq!(result["matched_record_count"], 6);
    assert_eq!(result["train_count"], 3);
    assert_eq!(result["holdout_count"], 3);
    assert_eq!(
        result["split_policy"],
        "date_cutoff_train_before_holdout_at_or_after"
    );
    assert_eq!(result["leakage_checks"]["no_session_overlap"], true);
    assert_eq!(result["holdout_improvement_valid"], true);
    assert_eq!(result["official_labels_are_labels"], true);
    assert_eq!(result["matched_records"][0]["unit"], "score_0_to_100");
    assert_eq!(result["persisted"], true);
}

#[test]
fn bridge_reports_missing_calibration_run_without_fallback_to_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "calibration-missing",
        "method": "calibration.apply",
        "args": {
            "database_path": db_path,
            "metric_family": "recovery",
            "algorithm_id": "goose.recovery.v0",
            "algorithm_version": "0.1.0",
            "raw_score": 70.0,
            "calibration_run_id": "missing-calibration",
            "score_min": 0.0,
            "score_max": 100.0
        }
    }));

    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "method_error");
}

#[test]
fn bridge_rejects_wrong_family_algorithm_preference() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "preferences-wrong-family",
        "method": "settings.set_algorithm_preference",
        "args": {
            "database_path": db_path,
            "scope": "global",
            "metric_family": "recovery",
            "algorithm_id": "goose.sleep.v0",
            "version": "0.1.0"
        }
    }));

    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "method_error");
}

#[test]
fn bridge_derives_packet_timeline_from_decoded_rows() {
    let store = GooseStore::open_in_memory().unwrap();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap();
    let parsed = parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME).unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "bridge-frame-1",
            source: "synthetic.fixture",
            captured_at: "2026-05-28T00:00:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &raw,
            sensitivity: "synthetic",
            capture_session_id: None,
        })
        .unwrap();
    store
        .insert_decoded_frame(goose_core::store::DecodedFrameInput {
            frame_id: "bridge-frame-1.frame.0",
            evidence_id: "bridge-frame-1",
            parsed: &parsed,
            parser_version: "bridge-test",
        })
        .unwrap();
    let rows: Vec<DecodedFrameRow> = store
        .decoded_frames_between("2026-05-28T00:00:00Z", "2026-05-29T00:00:00Z")
        .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "timeline-1",
        "method": "timeline.from_decoded_frames",
        "args": {
            "decoded_frames": rows
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["category"], "command");
    assert_eq!(result[0]["title"], "Command GET_HELLO");
}

#[test]
fn bridge_imports_app_captured_frame_batch_into_sqlite_and_timeline() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-import-1",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-capture-valid",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": GET_HELLO_FRAME,
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-capture-malformed",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:01Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": "00010203",
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], false);
    assert_eq!(result["raw_inserted"], 2);
    assert_eq!(result["frames_inserted"], 1);
    assert_eq!(result["timeline_rows"].as_array().unwrap().len(), 1);
    assert_eq!(result["timeline_rows"][0]["category"], "command");
    assert_eq!(result["results"][0]["parse_ok"], true);
    assert_eq!(result["results"][1]["parse_ok"], false);
    assert!(
        result["results"][1]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.as_str().unwrap().contains("does not start"))
    );
    assert!(
        result["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["scope"] == "bridge-capture-malformed"
                && action["reason"] == "frame_parse_failed")
    );

    let timeline = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-timeline-1",
        "method": "capture.timeline",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z"
        }
    }));
    assert!(timeline.ok, "{:?}", timeline.error);
    let timeline_result = timeline.result.unwrap();
    assert_eq!(timeline_result.as_array().unwrap().len(), 1);
    assert_eq!(timeline_result[0]["category"], "command");
    assert_eq!(timeline_result[0]["title"], "Command GET_HELLO");

    let inverted = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-timeline-inverted",
        "method": "capture.timeline",
        "args": {
            "database_path": db_path,
            "start": "2026-05-29T00:00:00Z",
            "end": "2026-05-28T00:00:00Z"
        }
    }));
    assert!(!inverted.ok);
    assert!(
        inverted
            .error
            .unwrap()
            .message
            .contains("start must be earlier than end")
    );
}

#[test]
fn bridge_builds_capture_observability_timeline_from_packets_and_debug_events() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let start_capture = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-start-capture",
        "method": "capture.start_session",
        "args": {
            "database_path": db_path,
            "session_id": "capture-live-observability",
            "source": "ios_core_bluetooth.live_notifications",
            "started_at_unix_ms": 1779840000000i64,
            "device_model": "WHOOP 5.0 Goose",
            "active_device_id": "test-device",
            "provenance": {
                "owner": "user",
                "capture_kind": "live_ble_notification"
            }
        }
    }));
    assert!(start_capture.ok, "{:?}", start_capture.error);

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-observability-test",
            "frames": [
                {
                    "evidence_id": "bridge-observability-capture",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": GET_HELLO_FRAME,
                    "sensitivity": "user-owned-live-notification",
                    "capture_session_id": "capture-live-observability",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let debug_started = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-debug-start",
        "method": "debug.start_session",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-observability",
            "started_at_unix_ms": 1779840000000u64,
            "bridge": {
                "url": "ws://127.0.0.1:49152/goose-debug/stream?token=test",
                "bind_host": "127.0.0.1",
                "token_required": true,
                "token_present": true,
                "remote_bind_enabled": false,
                "visible_remote_bind_toggle": false
            }
        }
    }));
    assert!(debug_started.ok, "{:?}", debug_started.error);

    let story_event = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-story-event",
        "method": "debug.record_event",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-observability",
            "time_unix_ms": 1779840000100u64,
            "source": "app",
            "level": "info",
            "topic": "capture.session.scan",
            "message": "scan started",
            "data": {
                "capture_session_id": "capture-live-observability",
                "capture_session_action_key": "scan",
                "capture_session_action": "scan start"
            }
        }
    }));
    assert!(story_event.ok, "{:?}", story_event.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-timeline",
        "method": "capture.observability_timeline",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "start_unix_ms": 1779840000000i64,
            "end_unix_ms": 1779840001000i64
        }
    }));
    assert!(response.ok, "{:?}", response.error);
    let rows = response.result.unwrap();
    let rows = rows.as_array().unwrap();
    assert!(rows.iter().any(|row| row["stage"] == "capture_session"
        && row["capture_session_id"] == "capture-live-observability"));
    assert!(rows.iter().any(|row| row["stage"] == "raw_frame"
        && row["raw_evidence_id"] == "bridge-observability-capture"
        && row["parent_timeline_id"] == "capture-session.capture-live-observability.scan"));
    assert!(rows.iter().any(|row| row["stage"] == "decoded_packet"
        && row["title"] == "Command GET_HELLO"
        && row["parent_timeline_id"] == "raw.bridge-observability-capture"));

    let inverted = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-observability-inverted",
        "method": "capture.observability_timeline",
        "args": {
            "database_path": db_path,
            "start": "2026-05-29T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "start_unix_ms": 1779840000000i64,
            "end_unix_ms": 1779840001000i64
        }
    }));
    assert!(!inverted.ok);
    assert!(
        inverted
            .error
            .unwrap()
            .message
            .contains("start must be earlier than end")
    );
}

#[test]
fn bridge_records_capture_session_lifecycle_for_live_owned_capture() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let start = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-session-start",
        "method": "capture.start_session",
        "args": {
            "database_path": db_path,
            "session_id": "capture-live-bridge",
            "source": "ios_core_bluetooth.live_notifications",
            "started_at_unix_ms": 1770000000000i64,
            "device_model": "WHOOP 5.0 Goose",
            "active_device_id": "test-device",
            "provenance": {
                "owner": "user",
                "capture_kind": "live_ble_notification"
            }
        }
    }));
    assert!(start.ok, "{:?}", start.error);
    let start_result = start.result.unwrap();
    assert_eq!(start_result["schema"], "goose.capture-session-result.v1");
    assert_eq!(start_result["inserted"], true);
    assert_eq!(start_result["session"]["status"], "active");

    let finish = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-session-finish",
        "method": "capture.finish_session",
        "args": {
            "database_path": db_path,
            "session_id": "capture-live-bridge",
            "ended_at_unix_ms": 1770000001234i64,
            "frame_count": 7
        }
    }));
    assert!(finish.ok, "{:?}", finish.error);
    let finish_result = finish.result.unwrap();
    assert_eq!(finish_result["session"]["status"], "finished");
    assert_eq!(finish_result["session"]["frame_count"], 7);

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-session-list",
        "method": "capture.list_sessions",
        "args": {
            "database_path": db_path,
            "start_unix_ms": 1769999999999i64,
            "end_unix_ms": 1770000002000i64
        }
    }));
    assert!(list.ok, "{:?}", list.error);
    let list_result = list.result.unwrap();
    assert_eq!(list_result["schema"], "goose.capture-session-list.v1");
    assert_eq!(list_result["session_count"], 1);
    assert_eq!(
        list_result["sessions"][0]["session_id"],
        "capture-live-bridge"
    );
}

#[test]
fn bridge_manages_local_activity_sessions_metrics_and_intervals() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let create = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-create-session",
        "method": "activity.create_session",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge",
            "source": "bridge.test.manual_import",
            "start_time_unix_ms": 1770001200000i64,
            "end_time_unix_ms": 1770004800000i64,
            "activity_type": "running",
            "confidence": 0.72,
            "detection_method": "manual_annotation",
            "sync_status": "verified",
            "provenance": {
                "source": "bridge-test",
                "kind": "manual_import"
            }
        }
    }));
    assert!(create.ok, "{:?}", create.error);
    let create_result = create.result.unwrap();
    assert_eq!(create_result["schema"], "goose.activity-session-result.v1");
    assert_eq!(create_result["inserted"], true);
    assert_eq!(
        create_result["session"]["session_id"],
        "activity-session-bridge"
    );
    assert_eq!(create_result["session"]["duration_ms"], 3600000);

    let fetch = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-get-session",
        "method": "activity.get_session",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge"
        }
    }));
    assert!(fetch.ok, "{:?}", fetch.error);
    let fetch_result = fetch.result.unwrap();
    assert_eq!(fetch_result["schema"], "goose.activity-session-result.v1");
    assert_eq!(fetch_result["session"]["activity_type"], "running");
    assert_eq!(fetch_result["session"]["sync_status"], "verified");

    let update = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-update-session",
        "method": "activity.update_session",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge",
            "source": "bridge.test.manual_import",
            "start_time_unix_ms": 1770001200000i64,
            "end_time_unix_ms": 1770004800000i64,
            "activity_type": "running",
            "external_activity_type_code": "outdoor_run",
            "external_activity_type_name": "Outdoor Run",
            "custom_label": "bridge run",
            "confidence": 0.84,
            "detection_method": "manual_annotation",
            "sync_status": "synced",
            "provenance": {
                "source": "bridge-test",
                "revision": 2
            }
        }
    }));
    assert!(update.ok, "{:?}", update.error);
    let update_result = update.result.unwrap();
    assert_eq!(update_result["schema"], "goose.activity-session-result.v1");
    assert_eq!(update_result["updated"], true);
    assert_eq!(update_result["session"]["custom_label"], "bridge run");
    assert_eq!(
        update_result["session"]["external_activity_type_name"],
        "Outdoor Run"
    );
    assert_eq!(update_result["session"]["sync_status"], "synced");

    let correction_plans = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-correction-plans",
        "method": "activity.correction_plans"
    }));
    assert!(correction_plans.ok, "{:?}", correction_plans.error);
    let correction_plans_result = correction_plans.result.unwrap();
    assert_eq!(
        correction_plans_result["schema"],
        "goose.activity-correction-plans.v1"
    );
    assert_eq!(correction_plans_result["plan_count"], 6);
    assert!(
        correction_plans_result["plans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plan| plan["kind"] == "change_activity_type")
    );

    let missing_type = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-correction-missing-type",
        "method": "activity.apply_correction",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge",
            "kind": "change_activity_type",
            "details": {},
            "provenance": {
                "ui_surface": "today.activity.correction"
            }
        }
    }));
    assert!(!missing_type.ok);
    assert_eq!(
        missing_type.error.unwrap().message,
        "activity_type is required for change_activity_type corrections"
    );

    let correction = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-correction-change-type",
        "method": "activity.apply_correction",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge",
            "kind": "change_activity_type",
            "activity_type": "cycling",
            "external_activity_type_code": "bike",
            "external_activity_type_name": "Bike",
            "custom_label": "bridge ride",
            "details": {
                "reason": "manual review"
            },
            "provenance": {
                "ui_surface": "today.activity.correction",
                "triggering_ui_action": "change_activity_type"
            }
        }
    }));
    assert!(correction.ok, "{:?}", correction.error);
    let correction_result = correction.result.unwrap();
    assert_eq!(
        correction_result["schema"],
        "goose.activity-correction-result.v1"
    );
    assert_eq!(correction_result["kind"], "change_activity_type");
    assert_eq!(correction_result["updated"], true);
    assert_eq!(correction_result["session"]["activity_type"], "cycling");
    assert_eq!(correction_result["session"]["custom_label"], "bridge ride");
    assert_eq!(
        correction_result["session"]["detection_method"],
        "manual_annotation"
    );
    assert_eq!(
        correction_result["session"]["sync_status"],
        "user_confirmed"
    );
    let correction_provenance: serde_json::Value = serde_json::from_str(
        correction_result["session"]["provenance_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(correction_provenance["manually_corrected"], true);
    assert_eq!(
        correction_provenance["correction_kind"],
        "change_activity_type"
    );
    assert_eq!(
        correction_provenance["correction_details"]["previous_activity_type"],
        "running"
    );
    assert_eq!(
        correction_provenance["correction_details"]["updated_activity_type"],
        "cycling"
    );
    assert_eq!(
        correction_provenance["correction_details"]["request_provenance"]["triggering_ui_action"],
        "change_activity_type"
    );
    assert_eq!(
        correction_provenance["correction_history"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-list-sessions",
        "method": "activity.list_sessions",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1770001199000i64,
            "end_time_unix_ms": 1770004801000i64
        }
    }));
    assert!(list.ok, "{:?}", list.error);
    let list_result = list.result.unwrap();
    assert_eq!(list_result["schema"], "goose.activity-session-list.v1");
    assert_eq!(list_result["session_count"], 1);
    assert_eq!(
        list_result["sessions"][0]["session_id"],
        "activity-session-bridge"
    );

    let attach_metric = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-attach-metric",
        "method": "activity.attach_metric",
        "args": {
            "database_path": db_path,
            "metric_id": "activity-metric-bridge",
            "activity_session_id": "activity-session-bridge",
            "metric_name": "heart_rate_bpm",
            "value": 148.0,
            "unit": "bpm",
            "start_time_unix_ms": 1770003000000i64,
            "end_time_unix_ms": 1770003060000i64,
            "quality_flags": ["chartable"],
            "provenance": {
                "source": "bridge-test",
                "sensor": "heart_rate"
            }
        }
    }));
    assert!(attach_metric.ok, "{:?}", attach_metric.error);
    let attach_metric_result = attach_metric.result.unwrap();
    assert_eq!(
        attach_metric_result["schema"],
        "goose.activity-metric-result.v1"
    );
    assert_eq!(attach_metric_result["inserted"], true);
    assert_eq!(
        attach_metric_result["metric"]["metric_id"],
        "activity-metric-bridge"
    );

    let metrics = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-list-metrics",
        "method": "activity.list_metrics",
        "args": {
            "database_path": db_path,
            "activity_session_id": "activity-session-bridge"
        }
    }));
    assert!(metrics.ok, "{:?}", metrics.error);
    let metrics_result = metrics.result.unwrap();
    assert_eq!(metrics_result["schema"], "goose.activity-metric-list.v1");
    assert_eq!(metrics_result["metric_count"], 1);
    assert_eq!(
        metrics_result["metrics"][0]["metric_id"],
        "activity-metric-bridge"
    );

    let attach_interval = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-attach-interval",
        "method": "activity.attach_interval",
        "args": {
            "database_path": db_path,
            "interval_id": "activity-interval-bridge",
            "activity_session_id": "activity-session-bridge",
            "interval_type": "work",
            "start_time_unix_ms": 1770003000000i64,
            "end_time_unix_ms": 1770003300000i64,
            "sequence": 1,
            "metadata": {
                "label": "work block"
            },
            "provenance": {
                "source": "bridge-test",
                "kind": "interval"
            }
        }
    }));
    assert!(attach_interval.ok, "{:?}", attach_interval.error);
    let attach_interval_result = attach_interval.result.unwrap();
    assert_eq!(
        attach_interval_result["schema"],
        "goose.activity-interval-result.v1"
    );
    assert_eq!(attach_interval_result["inserted"], true);
    assert_eq!(
        attach_interval_result["interval"]["interval_id"],
        "activity-interval-bridge"
    );

    let intervals = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-list-intervals",
        "method": "activity.list_intervals",
        "args": {
            "database_path": db_path,
            "activity_session_id": "activity-session-bridge"
        }
    }));
    assert!(intervals.ok, "{:?}", intervals.error);
    let intervals_result = intervals.result.unwrap();
    assert_eq!(
        intervals_result["schema"],
        "goose.activity-interval-list.v1"
    );
    assert_eq!(intervals_result["interval_count"], 1);
    assert_eq!(
        intervals_result["intervals"][0]["interval_id"],
        "activity-interval-bridge"
    );

    let chart = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-metrics-window",
        "method": "activity.metrics_for_session_in_window",
        "args": {
            "database_path": db_path,
            "activity_session_id": "activity-session-bridge",
            "start_time_unix_ms": 1770002900000i64,
            "end_time_unix_ms": 1770003200000i64
        }
    }));
    assert!(chart.ok, "{:?}", chart.error);
    let chart_result = chart.result.unwrap();
    assert_eq!(chart_result["schema"], "goose.activity-metric-window.v1");
    assert_eq!(chart_result["metric_count"], 1);
    assert_eq!(
        chart_result["metrics"][0]["metric_id"],
        "activity-metric-bridge"
    );
    assert_eq!(chart_result["metrics"][0]["value"], 148.0);

    let delete = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-delete-session",
        "method": "activity.delete_session",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge"
        }
    }));
    assert!(delete.ok, "{:?}", delete.error);
    let delete_result = delete.result.unwrap();
    assert_eq!(
        delete_result["schema"],
        "goose.activity-session-delete-result.v1"
    );
    assert_eq!(delete_result["session_id"], "activity-session-bridge");
    assert_eq!(delete_result["deleted"], true);

    let empty_list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-list-sessions-after-delete",
        "method": "activity.list_sessions",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1770001199000i64,
            "end_time_unix_ms": 1770004801000i64
        }
    }));
    assert!(empty_list.ok, "{:?}", empty_list.error);
    let empty_list_result = empty_list.result.unwrap();
    assert_eq!(empty_list_result["session_count"], 0);

    let delete_again = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-delete-session-again",
        "method": "activity.delete_session",
        "args": {
            "database_path": db_path,
            "session_id": "activity-session-bridge"
        }
    }));
    assert!(delete_again.ok, "{:?}", delete_again.error);
    let delete_again_result = delete_again.result.unwrap();
    assert_eq!(delete_again_result["deleted"], false);
}

#[test]
fn bridge_runs_capture_correlation_for_debug_trust_gate() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let db_path = db.display().to_string();

    let permissive = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-correlation-1",
        "method": "capture.correlation_report",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1
        }
    }));
    assert!(permissive.ok, "{:?}", permissive.error);
    let report = permissive.result.unwrap();
    assert_eq!(report["schema"], "goose.capture-correlation-report.v1");
    assert_eq!(report["pass"], true);
    assert!(
        report["summaries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|summary| summary["body_summary_kind"] == "raw_motion_k10"
                && summary["trusted_metric_ready"] == false)
    );

    let required = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-correlation-required",
        "method": "capture.correlation_report",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_owned_captures": true
        }
    }));
    assert!(required.ok, "{:?}", required.error);
    let required_report = required.result.unwrap();
    assert_eq!(required_report["pass"], false);
    assert!(
        required_report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.as_str().unwrap().contains("not trusted"))
    );
    assert!(
        required_report["next_capture_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["scope"] == "raw_motion_k10"
                && action["action"]
                    .as_str()
                    .unwrap()
                    .contains("Capture 1 more user-owned raw_motion_k10 frame"))
    );
}

#[test]
fn bridge_reports_metric_input_readiness_for_debug_scoring_gate() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "metric-readiness-1",
        "method": "metrics.input_readiness",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_owned_captures": true,
            "require_scores_ready": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.metric-input-readiness-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["family_count"], 6);
    assert_eq!(report["ready_family_count"], 0);
    assert!(
        report["families"]
            .as_array()
            .unwrap()
            .iter()
            .any(|family| family["metric_family"] == "stress" && family["score_ready"] == false)
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["scope"] == "capture_correlation"
                && action["action"]
                    .as_str()
                    .unwrap()
                    .contains("Run Capture Trust"))
    );
    assert!(
        report["families"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|family| family["metric_family"] == "recovery")
            .flat_map(|family| family["next_actions"].as_array().unwrap())
            .any(|action| action["scope"] == "respiratory_rate_rpm"
                && action["action"]
                    .as_str()
                    .unwrap()
                    .contains("normal_history"))
    );
}

#[test]
fn bridge_reports_capture_arrival_plan_for_device_day_readiness() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-arrival-plan-1",
        "method": "capture.arrival_plan",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_owned_captures": true,
            "require_scores_ready": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.capture-arrival-plan-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["min_owned_captures"], 1);
    assert_eq!(report["physical_arrival_row_count"], 11);
    let physical_rows = report["physical_arrival_rows"].as_array().unwrap();
    assert_eq!(physical_rows.len(), 11);
    assert!(physical_rows.iter().any(|row| {
        row["id"] == "arrival.service_filters"
            && row["label"] == "Service filters"
            && row["state"] == "blocked"
            && row["next_action"]
                .as_str()
                .unwrap()
                .contains("WHOOP-targeted scan mode")
    }));
    assert!(physical_rows.iter().any(|row| {
        row["id"] == "arrival.command_write_pairs"
            && row["label"] == "Command/write pairs"
            && row["evidence"]
                .as_str()
                .unwrap()
                .contains("whoop-arrival-checklist.md")
    }));
    assert_eq!(
        report["capture_correlation"]["schema"],
        "goose.capture-correlation-report.v1"
    );
    assert_eq!(
        report["metric_input_readiness"]["schema"],
        "goose.metric-input-readiness-report.v1"
    );
    assert_eq!(
        report["recovery_sensor_discovery"]["schema"],
        "goose.recovery-sensor-discovery-report.v1"
    );
    assert_eq!(
        report["local_health_validation_review"]["schema"],
        "goose.local-health-validation-manifest-review.v1"
    );
    assert_eq!(
        report["local_health_validation_review"]["status"],
        "operator_edits_required"
    );
    assert!(
        report["local_health_validation_review"]["acceptance_evidence_case_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(report["action_count"].as_u64().unwrap() > 0);
    assert_eq!(report["next_capture_focus"]["source"], "Capture Trust");
    assert!(
        report["next_capture_focus"]["scope"]
            .as_str()
            .unwrap()
            .len()
            > 0
    );
    assert!(
        report["next_capture_focus"]["reason"]
            .as_str()
            .unwrap()
            .contains("owned_capture")
    );
    assert!(
        report["next_capture_focus"]["summary"]
            .as_str()
            .unwrap()
            .contains("user-owned")
    );
    let actions = report["actions"].as_array().unwrap();
    assert!(actions.iter().any(|action| {
        action["source"] == "Capture Trust"
            && action["summary"]
                .as_str()
                .unwrap()
                .contains("Capture 1 more user-owned raw_motion_k10 frame")
    }));
    assert!(actions.iter().any(|action| {
        action["source"] == "Metric Inputs"
            && action["summary"]
                .as_str()
                .unwrap()
                .contains("Run Capture Trust")
    }));
    assert!(actions.iter().any(|action| {
        action["source"] == "Recovery Sensors"
            && action["reason"] == "oxygen_saturation_decoder_not_implemented"
            && action["summary"]
                .as_str()
                .unwrap()
                .contains("verified SpO2 decoder")
    }));
    assert!(actions.iter().any(|action| {
        action["source"] == "Local Health Validation"
            && action["scope"] == "owned-step-validation"
            && action["summary"]
                .as_str()
                .unwrap()
                .contains("WHOOP app step delta as a validation label")
    }));
}

#[test]
fn bridge_derives_capture_arrival_physical_rows_from_local_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    let db_path = db.display().to_string();
    let provenance = serde_json::json!({
        "whoop_scan_targeted": true,
        "whoop_profile": {
            "generation": "gen5",
            "service_uuids": ["fd4b0001-cce1-4033-93ce-002d5875f58a"],
            "roles": {
                "command_to_strap": "fd4b0002-cce1-4033-93ce-002d5875f58a",
                "command_from_strap": "fd4b0003-cce1-4033-93ce-002d5875f58a",
                "events_from_strap": "fd4b0004-cce1-4033-93ce-002d5875f58a",
                "data_from_strap": "fd4b0005-cce1-4033-93ce-002d5875f58a",
                "memfault": "fd4b0007-cce1-4033-93ce-002d5875f58a"
            }
        },
        "notification_state": {
            "subscribed_characteristics": ["fd4b0004-cce1-4033-93ce-002d5875f58a"],
            "first_notification_timestamp": "2026-02-02T16:00:05Z",
            "reconnect_resubscription": true
        },
        "auth_trace": [{"state": "connect"}, {"state": "auth"}, {"state": "transfer"}],
        "sync_metadata": {
            "HistoryStart": true,
            "HistoryEnd": true,
            "HistoryComplete": true,
            "range_window": "2026-02-02",
            "completion_reason": "complete"
        }
    })
    .to_string();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "arrival-physical-session",
            source: "live_ble",
            started_at_unix_ms: 1_770_000_000_000,
            device_model: "WHOOP 5.0",
            active_device_id: Some("whoop-5"),
            provenance_json: &provenance,
        })
        .unwrap();
    store
        .finish_capture_session("arrival-physical-session", 1_770_000_060_000, 12)
        .unwrap();

    let ready_command = validate_commands(&[CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some("user_owned_official_capture".to_string()),
        provenance_json: Some(
            r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user","triggering_ui_action":"Device screen refresh"}"#.to_string(),
        ),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        official_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        official_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        official_response_frame_hex: Some(GET_HELLO_RESPONSE_FRAME.to_string()),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        triggering_ui_action: Some("Device screen refresh".to_string()),
        ..CommandEvidence::default()
    }])
    .commands
    .into_iter()
    .find(|command| command.command == "get_hello")
    .unwrap();
    store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: ready_command.command.clone(),
            risk_gate: "read_only".to_string(),
            direct_send_ready: ready_command.direct_send_ready,
            report_json: serde_json::to_string(&ready_command).unwrap(),
        })
        .unwrap();

    let activity_provenance = serde_json::json!({
        "activity_type_provenance": "official_capture",
        "packet_fields": ["activity_start", "activity_end", "activity_type"],
        "activity_start": "2026-02-02T16:04:00Z",
        "activity_end": "2026-02-02T16:44:00Z",
        "confidence": 0.91
    })
    .to_string();
    store
        .insert_activity_session(ActivitySessionInput {
            session_id: "arrival-activity-1",
            source: "physical_capture",
            start_time_unix_ms: 1_770_000_240_000,
            end_time_unix_ms: 1_770_002_640_000,
            activity_type: "running",
            external_activity_type_code: Some("run"),
            external_activity_type_name: Some("Run"),
            custom_label: None,
            confidence: 0.91,
            detection_method: "official_capture",
            sync_status: "user_confirmed",
            provenance_json: &activity_provenance,
        })
        .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-arrival-plan-physical-rows",
        "method": "capture.arrival_plan",
        "args": {
            "database_path": db_path,
            "start": "2026-01-01T00:00:00Z",
            "end": "2026-12-31T00:00:00Z",
            "min_owned_captures": 1,
            "require_owned_captures": true,
            "require_scores_ready": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    let rows = report["physical_arrival_rows"].as_array().unwrap();
    for row_id in [
        "arrival.service_filters",
        "arrival.role_labels",
        "arrival.notification_subscriptions",
        "arrival.frame_counts",
        "arrival.capture_statuses",
        "arrival.command_write_pairs",
        "arrival.auth.session",
        "arrival.history.metadata",
        "arrival.activity.boundary_type",
        "arrival.activity.promotion",
    ] {
        assert!(
            rows.iter()
                .any(|row| row["id"] == row_id && row["state"] == "physical-validated"),
            "expected {row_id} to be physical-validated in {rows:?}"
        );
    }
    assert!(
        rows.iter()
            .any(|row| { row["id"] == "arrival.history.fields" && row["state"] == "blocked" })
    );
}

#[test]
fn bridge_extracts_motion_features_for_debug_score_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "motion-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-owned-motion",
                    "frame_id": "bridge-owned-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "motion-features-1",
        "method": "metrics.motion_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.motion-feature-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["feature_count"], 1);
    assert_eq!(report["trusted_feature_count"], 1);
    assert_eq!(report["features"][0]["body_summary_kind"], "raw_motion_k10");
    assert_eq!(report["features"][0]["heart_rate_bpm"], 72);
    assert_eq!(report["features"][0]["trusted_metric_input"], true);
    assert!(
        report["features"][0]["motion_intensity_0_to_1"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}

#[test]
fn bridge_runs_step_packet_discovery_over_decoded_motion_frames() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-discovery-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-step-discovery-motion",
                    "frame_id": "bridge-step-discovery-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-discovery-1",
        "method": "metrics.step_packet_discovery",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "max_candidate_fields": 25
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.step-packet-discovery-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["decoded_frame_count"], 1);
    assert_eq!(report["inspected_frame_count"], 1);
    assert_eq!(report["explicit_step_counter_found"], false);
    assert_eq!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_explicit_step_counter_field_found"),
        true
    );
}

#[test]
fn bridge_runs_step_capture_validation_with_validation_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-step-validation-motion",
                    "frame_id": "bridge-step-validation-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-validation-1",
        "method": "metrics.step_capture_validation",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "capture_kind": "100_counted_steps",
            "manual_step_delta": 100,
            "official_whoop_step_delta": 97,
            "tolerance_steps": 5,
            "label_provenance": {
                "manual_source": "counted_steps",
                "official_source": "whoop_app_screenshot",
                "official_labels_are_labels": true
            }
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.step-capture-validation-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["capture_kind"], "100_counted_steps");
    assert_eq!(report["manual_step_delta"], 100);
    assert_eq!(report["official_whoop_step_delta"], 97);
    assert_eq!(report["counter_delta_candidate_count"], 0);
    assert_eq!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no_counter_delta_candidates"),
        true
    );
}

#[test]
fn bridge_runs_step_counter_daily_rollup_from_persisted_device_samples() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    {
        let store = GooseStore::open(&db).unwrap();
        for (sample_id, sample_time_unix_ms, counter_value, cadence_spm, activity_state) in [
            (
                "bridge-step-s1",
                1_780_387_200_000,
                4_100,
                Some(88.0),
                Some("walking"),
            ),
            (
                "bridge-step-s2",
                1_780_387_260_000,
                4_175,
                Some(94.0),
                Some("walking"),
            ),
            (
                "bridge-step-s3",
                1_780_387_320_000,
                4_205,
                Some(100.0),
                Some("stairs"),
            ),
        ] {
            store
                .insert_step_counter_sample(StepCounterSampleInput {
                    sample_id,
                    sample_time_unix_ms,
                    counter_value,
                    cadence_spm,
                    activity_state,
                    source_kind: "device_counter",
                    packet_family: "K11/raw_stream_counted",
                    json_path: "$.body_summary.step_count",
                    frame_id: None,
                    evidence_id: None,
                    capture_session_id: None,
                    quality_flags_json: "[]",
                    provenance_json: r#"{"owner":"user","bridge_test":true}"#,
                })
                .unwrap();
        }
    }

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-counter-rollup-1",
        "method": "metrics.step_counter_daily_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start_time_unix_ms": 1780355200000_i64,
            "end_time_unix_ms": 1780441600000_i64,
            "min_sample_count": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.step-counter-daily-rollup-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["steps"], 105);
    assert_eq!(report["average_cadence_spm"], 94.0);
    assert_eq!(report["activity_state_counts"]["walking"], 2);
    assert_eq!(report["activity_state_counts"]["stairs"], 1);
    assert_eq!(report["daily_metric_written"], true);
    assert_eq!(report["metric_provenance_written"], true);

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.steps, Some(105));
    assert_eq!(metric.average_cadence_spm, Some(94.0));
    assert_eq!(metric.source_kind, "device_counter");

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "daily-activity-metrics-list-1",
        "method": "metrics.daily_activity_metrics",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1780355200000_i64,
            "end_time_unix_ms": 1780441600000_i64
        }
    }));

    assert!(list.ok, "{:?}", list.error);
    let list_report = list.result.unwrap();
    assert_eq!(list_report["schema"], "goose.daily-activity-metric-list.v1");
    assert_eq!(list_report["metric_count"], 1);
    assert_eq!(list_report["metrics"][0]["steps"], 105);
    assert_eq!(list_report["metrics"][0]["source_kind"], "device_counter");

    let hourly = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "step-counter-hourly-rollup-1",
        "method": "metrics.step_counter_hourly_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start_time_unix_ms": 1780387200000_i64,
            "end_time_unix_ms": 1780390800000_i64,
            "min_sample_count": 2,
            "write_metric": true
        }
    }));

    assert!(hourly.ok, "{:?}", hourly.error);
    let hourly_report = hourly.result.unwrap();
    assert_eq!(
        hourly_report["schema"],
        "goose.step-counter-hourly-rollup-report.v1"
    );
    assert_eq!(hourly_report["pass"], true);
    assert_eq!(hourly_report["steps"], 105);
    assert_eq!(hourly_report["hourly_metric_written"], true);
    assert_eq!(hourly_report["metric_provenance_written"], true);

    let hourly_list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hourly-activity-metrics-list-1",
        "method": "metrics.hourly_activity_metrics",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1780387200000_i64,
            "end_time_unix_ms": 1780390800000_i64
        }
    }));

    assert!(hourly_list.ok, "{:?}", hourly_list.error);
    let hourly_list_report = hourly_list.result.unwrap();
    assert_eq!(
        hourly_list_report["schema"],
        "goose.hourly-activity-metric-list.v1"
    );
    assert_eq!(hourly_list_report["metric_count"], 1);
    assert_eq!(hourly_list_report["metrics"][0]["steps"], 105);
    assert_eq!(
        hourly_list_report["metrics"][0]["source_kind"],
        "device_counter"
    );
}

#[test]
fn bridge_persists_unavailable_activity_step_status_with_provenance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "activity-unavailable-status-1",
        "method": "metrics.activity_unavailable_daily_status",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start_time_unix_ms": 1780355200000_i64,
            "end_time_unix_ms": 1780441600000_i64,
            "min_sample_count": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.activity-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["available_step_metric_count"], 0);
    assert_eq!(report["unavailable_metric_count"], 1);
    assert_eq!(report["written_metric_count"], 1);
    assert_eq!(report["metric_provenance_written_count"], 1);
    assert_eq!(report["statuses"][0]["metric_id"], "steps");
    assert_eq!(report["statuses"][0]["source_kind"], "unavailable");
    assert!(
        report["statuses"][0]["blocker_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "insufficient_step_counter_samples")
    );

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["statuses"][0]["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.steps, None);
    assert_eq!(metric.source_kind, "unavailable");
    assert_eq!(metric.confidence, 0.0);
    let provenance_json: serde_json::Value = serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        provenance_json["algorithm"],
        GOOSE_ACTIVITY_UNAVAILABLE_STATUS_V0_ID
    );
    assert_eq!(
        provenance_json["algorithm_version"],
        GOOSE_ACTIVITY_UNAVAILABLE_STATUS_V0_VERSION
    );
    assert_eq!(provenance_json["source_kind"], "unavailable");
    assert_eq!(
        provenance_json["value_policy"],
        "no_step_value_written_until_whoop_device_counter_or_validated_local_estimator_exists"
    );

    let provenance_rows = store
        .metric_provenance_for_metric("daily_activity", metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
    assert_eq!(provenance_rows[0].source_kind, "unavailable");
    assert_eq!(provenance_rows[0].confidence, Some(0.0));
}

#[test]
fn bridge_writes_validated_raw_motion_step_estimate_as_local_activity_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "raw-motion-step-estimate-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-raw-motion-steps",
                    "frame_id": "bridge-raw-motion-steps.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-06-02T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_step_frame_hex(&[10, 25, 40, 55, 70]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "raw-motion-step-estimate-write",
        "method": "metrics.raw_motion_step_estimate",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start": "2026-06-02T11:59:00Z",
            "end": "2026-06-02T12:01:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "manual_step_delta": 5,
            "official_whoop_step_delta": 5,
            "tolerance_steps": 0,
            "label_provenance": {
                "source": "manual_plus_official_app",
                "official_labels_are_labels": true
            },
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.raw-motion-step-estimate-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["source_kind_if_promoted"], "local_estimate");
    assert_eq!(report["estimated_steps"], 5);
    assert_eq!(report["daily_metric_written"], true);
    assert_eq!(report["metric_provenance_written"], true);

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.steps, Some(5));
    assert_eq!(metric.average_cadence_spm, Some(150.0));
    assert_eq!(metric.source_kind, "local_estimate");
    let provenance: serde_json::Value = serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        provenance["algorithm"],
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_ID
    );
    assert_eq!(
        provenance["algorithm_version"],
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_VERSION
    );
    assert_eq!(
        provenance["official_labels_policy"],
        "validation_label_only"
    );
}

#[test]
fn bridge_extracts_heart_rate_features_for_debug_score_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "heart-rate-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-owned-history",
                    "frame_id": "bridge-owned-history.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(77),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "heart-rate-features-1",
        "method": "metrics.heart_rate_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.heart-rate-feature-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["feature_count"], 1);
    assert_eq!(report["trusted_feature_count"], 1);
    assert_eq!(report["features"][0]["body_summary_kind"], "normal_history");
    assert_eq!(report["features"][0]["heart_rate_bpm"], 77.0);
    assert_eq!(report["features"][0]["trusted_metric_input"], true);
}

#[test]
fn bridge_extracts_vital_event_candidates_without_resolved_units() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "vital-event-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-owned-temperature",
                    "frame_id": "bridge-owned-temperature.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": temperature_event_frame_hex(&[0xde, 0xad, 0xbe, 0xef]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "vital-event-features-1",
        "method": "metrics.vital_event_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.vital-event-feature-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["feature_count"], 1);
    assert_eq!(report["trusted_feature_count"], 1);
    assert_eq!(report["resolved_metric_input_count"], 0);
    assert_eq!(report["features"][0]["event_name"], "TEMPERATURE_LEVEL");
    assert_eq!(report["features"][0]["raw_body_hex"], "deadbeef");
    assert_eq!(report["features"][0]["trusted_candidate_evidence"], true);
    assert_eq!(report["features"][0]["value_semantics_verified"], false);
}

#[test]
fn bridge_validates_respiratory_rate_against_whoop_label_without_promoting_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "respiratory-rate-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-respiratory-rate-validation-k18",
                    "frame_id": "bridge-respiratory-rate-validation-k18.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex_with_vital_candidates(77, 3567, 145),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "respiratory-rate-validation-1",
        "method": "metrics.respiratory_rate_capture_validation",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "capture_kind": "overnight_rest",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "official_whoop_respiratory_rate_rpm": 14.5,
            "tolerance_rpm": 0.1,
            "label_provenance": {
                "source": "whoop_app_manual_read",
                "official_labels_are_labels": true,
                "captured_by": "bridge_test"
            }
        }
    }));
    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.respiratory-rate-capture-validation-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_respiratory_rate_rpm"], 14.5);
    assert_eq!(report["local_respiratory_rate_rpm"], 14.5);
    assert_eq!(report["respiratory_rate_error_rpm"], 0.0);
    assert_eq!(report["respiratory_rate_within_tolerance"], true);
    assert_eq!(report["candidate_count"], 1);
    assert_eq!(report["trusted_candidate_count"], 1);
    assert_eq!(
        report["selected_candidate_schema_field"],
        "normal_history_k18_body_26_respiratory_rate_rpm_candidate"
    );
    assert_eq!(
        report["decoder_id"],
        "goose.respiratory_rate.history_candidate.v0"
    );
    assert_eq!(report["decoder_version"], "0.1.0");
    assert_eq!(
        report["promotion_status"],
        "validation_only_respiratory_rate_semantics_still_unverified"
    );
    assert!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "respiratory_rate_semantics_unverified")
    );
    assert_eq!(
        report["vital_event_report"]["schema"],
        "goose.vital-event-feature-report.v1"
    );
    assert_eq!(report["vital_event_report"]["pass"], true);

    let store = GooseStore::open(&db).unwrap();
    assert!(
        store
            .daily_recovery_metrics_between(0, i64::MAX)
            .unwrap()
            .is_empty()
    );

    let blocked = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "respiratory-rate-validation-missing-label-policy",
        "method": "metrics.respiratory_rate_capture_validation",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "capture_kind": "overnight_rest",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "official_whoop_respiratory_rate_rpm": 14.5,
            "tolerance_rpm": 0.1
        }
    }));
    assert!(blocked.ok, "{:?}", blocked.error);
    let blocked_report = blocked.result.unwrap();
    assert_eq!(blocked_report["pass"], false);
    assert!(
        blocked_report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        blocked_report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "official_whoop_values_are_validation_labels_not_inputs")
    );
}

#[test]
fn bridge_extracts_hrv_features_and_score_for_debug_score_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hrv-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-owned-r17",
                    "frame_id": "bridge-owned-r17.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 810, 790, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hrv-features-1",
        "method": "metrics.hrv_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "baseline_min_days": 1,
            "require_baseline": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.hrv-feature-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["feature_count"], 1);
    assert_eq!(report["trusted_feature_count"], 1);
    assert_eq!(report["rr_interval_count"], 4);
    assert_eq!(report["trusted_rr_interval_count"], 4);
    assert_eq!(report["daily_count"], 1);
    assert_eq!(report["baseline"]["day_count"], 1);
    assert!(
        report["baseline"]["hrv_baseline_rmssd_ms"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_eq!(
        report["hrv_input"]["rr_intervals_ms"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(report["score_result"]["output"]["valid_interval_count"], 4);
}

#[test]
fn bridge_validates_hrv_against_whoop_label_without_promoting_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hrv-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-hrv-validation-r17",
                    "frame_id": "bridge-hrv-validation-r17.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 810, 790, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hrv-validation-1",
        "method": "metrics.hrv_capture_validation",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "capture_kind": "overnight_rest",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "official_whoop_hrv_rmssd_ms": 14.1,
            "tolerance_ms": 0.2,
            "label_provenance": {
                "source": "whoop_app_manual_read",
                "official_labels_are_labels": true,
                "captured_by": "bridge_test"
            }
        }
    }));
    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.hrv-capture-validation-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["capture_kind"], "overnight_rest");
    assert_eq!(report["official_whoop_hrv_rmssd_ms"], 14.1);
    assert!(
        (report["local_hrv_rmssd_ms"].as_f64().unwrap() - 14.142_135_623_730_951).abs() < 0.000_001
    );
    assert_eq!(report["hrv_rmssd_error_ms"], 0.0);
    assert_eq!(report["hrv_rmssd_within_tolerance"], true);
    assert_eq!(report["provided_label_count"], 1);
    assert_eq!(report["matching_label_count"], 1);
    assert_eq!(report["rr_interval_count"], 4);
    assert_eq!(report["trusted_rr_interval_count"], 4);
    assert_eq!(report["algorithm_id"], GOOSE_HRV_V0_ID);
    assert_eq!(report["algorithm_version"], GOOSE_HRV_V0_VERSION);
    assert_eq!(
        report["promotion_status"],
        "validation_only_rr_interval_scale_still_unverified"
    );
    assert!(
        report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "hrv_rr_interval_scale_unverified")
    );
    assert_eq!(
        report["hrv_report"]["schema"],
        "goose.hrv-feature-report.v1"
    );
    assert_eq!(report["hrv_report"]["pass"], true);

    let store = GooseStore::open(&db).unwrap();
    assert!(
        store
            .daily_recovery_metrics_between(0, i64::MAX)
            .unwrap()
            .is_empty()
    );

    let blocked = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "hrv-validation-missing-label-policy",
        "method": "metrics.hrv_capture_validation",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "capture_kind": "overnight_rest",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "official_whoop_hrv_rmssd_ms": 14.1,
            "tolerance_ms": 0.2
        }
    }));
    assert!(blocked.ok, "{:?}", blocked.error);
    let blocked_report = blocked.result.unwrap();
    assert_eq!(blocked_report["pass"], false);
    assert!(
        blocked_report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert!(
        blocked_report["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "official_whoop_values_are_validation_labels_not_inputs")
    );
}

#[test]
fn bridge_aggregates_metric_window_features_for_debug_score_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "window-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-window-history-1",
                    "frame_id": "bridge-window-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(80),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-window-history-2",
                    "frame_id": "bridge-window-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-window-motion",
                    "frame_id": "bridge-window-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:05:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "window-features-1",
        "method": "metrics.window_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.metric-window-feature-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["heart_rate_feature_count"], 3);
    assert_eq!(report["trusted_heart_rate_feature_count"], 3);
    assert_eq!(report["motion_feature_count"], 1);
    assert_eq!(report["trusted_motion_feature_count"], 1);
    assert_eq!(report["window"]["duration_minutes"], 10.0);
    assert_eq!(report["window"]["average_hr_bpm"], 84.0);
    assert_eq!(report["window"]["max_hr_bpm"], 100.0);
    assert_eq!(report["window"]["trusted_metric_input"], true);
    assert_eq!(
        report["window"]["hr_zone_minutes"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn bridge_extracts_resting_heart_rate_features_for_debug_score_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "resting-heart-rate-feature-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-rhr-history-1",
                    "frame_id": "bridge-rhr-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-25T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(60),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-history-2",
                    "frame_id": "bridge-rhr-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-25T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(80),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-history-3",
                    "frame_id": "bridge-rhr-history-3.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-26T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(62),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-history-4",
                    "frame_id": "bridge-rhr-history-4.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-26T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-history-5",
                    "frame_id": "bridge-rhr-history-5.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(58),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-history-6",
                    "frame_id": "bridge-rhr-history-6.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "resting-heart-rate-features-1",
        "method": "metrics.resting_hr_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-25T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "baseline_min_days": 3,
            "require_baseline": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.resting-heart-rate-feature-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["heart_rate_feature_count"], 6);
    assert_eq!(report["trusted_heart_rate_feature_count"], 6);
    assert_eq!(report["daily_count"], 3);
    assert_eq!(report["resting"]["resting_hr_bpm"], 59.0);
    assert_eq!(report["baseline"]["resting_hr_baseline_bpm"], 60.0);
    assert_eq!(report["baseline"]["day_count"], 3);
}

#[test]
fn bridge_rolls_up_resting_heart_rate_into_daily_recovery_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    {
        let store = GooseStore::open(&db).unwrap();
        store
            .insert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "prior-rhr-2026-05-26",
                date_key: "2026-05-26",
                timezone: "Europe/London",
                start_time_unix_ms: 1_779_756_000_000,
                end_time_unix_ms: 1_779_842_400_000,
                resting_hr_bpm: Some(62.0),
                hrv_rmssd_ms: None,
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.82,
                inputs_json: r#"{"fixture":"prior_rhr"}"#,
                quality_flags_json: "[]",
                provenance_json: r#"{"owner":"user","bridge_test":true}"#,
            })
            .unwrap();
        store
            .insert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "same-day-hrv-2026-05-27",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1_779_842_400_000,
                end_time_unix_ms: 1_779_928_800_000,
                resting_hr_bpm: None,
                hrv_rmssd_ms: Some(68.2),
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.61,
                inputs_json: r#"{"fixture":"same_day_hrv_only"}"#,
                quality_flags_json: r#"["rr_interval_scale_unvalidated"]"#,
                provenance_json: r#"{"owner":"user","bridge_test":true,"algorithm":"goose.hrv.device_sensor.v0"}"#,
            })
            .unwrap();
    }

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "rhr-daily-rollup-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-rhr-rollup-history-1",
                    "frame_id": "bridge-rhr-rollup-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(58),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-rollup-history-2",
                    "frame_id": "bridge-rhr-rollup-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "rhr-daily-rollup-1",
        "method": "metrics.resting_hr_daily_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-27",
            "timezone": "Europe/London",
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_sample_count": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.resting-heart-rate-daily-rollup-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["resting_hr_bpm"], 58.0);
    assert_eq!(report["daily_metric_written"], true);
    assert_eq!(report["metric_provenance_written"], true);
    assert_eq!(report["rolling_7_day_average_bpm"], 60.0);
    assert_eq!(report["rolling_7_day_sample_count"], 2);
    assert_eq!(report["selected_vs_7_day_average_bpm"], -2.0);

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "daily-recovery-metrics-list-1",
        "method": "metrics.daily_recovery_metrics",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1_779_842_400_000_i64,
            "end_time_unix_ms": 1_779_928_800_000_i64
        }
    }));

    assert!(list.ok, "{:?}", list.error);
    let list_report = list.result.unwrap();
    assert_eq!(list_report["schema"], "goose.daily-recovery-metric-list.v1");
    assert_eq!(list_report["metric_count"], 2);
    assert!(
        list_report["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| metric["resting_hr_bpm"] == 58.0
                && metric["source_kind"] == "device_sensor")
    );
    assert!(
        list_report["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |metric| metric["hrv_rmssd_ms"] == 68.2 && metric["source_kind"] == "device_sensor"
            )
    );

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_recovery_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.resting_hr_bpm, Some(58.0));
    assert_eq!(metric.source_kind, "device_sensor");
    assert!((metric.confidence - 0.7133333333333334).abs() < 1e-12);
    let inputs_json: serde_json::Value = serde_json::from_str(&metric.inputs_json).unwrap();
    assert_eq!(inputs_json["motion_filter"]["motion_sample_count"], 0);
    assert_eq!(
        inputs_json["motion_filter"]["selected_heart_rate_sample_count"],
        2
    );
    assert_eq!(
        inputs_json["motion_filter"]["unmatched_heart_rate_sample_count"],
        2
    );
    let provenance_json: serde_json::Value = serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        provenance_json["motion_filter"],
        inputs_json["motion_filter"]
    );
    let same_day_recovery_rows = store
        .daily_recovery_metrics_between(1_779_842_400_000, 1_779_928_800_000)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_kind == "device_sensor")
        .collect::<Vec<_>>();
    assert_eq!(same_day_recovery_rows.len(), 2);
    assert!(
        same_day_recovery_rows
            .iter()
            .any(|row| row.resting_hr_bpm == Some(58.0))
    );
    assert!(
        same_day_recovery_rows
            .iter()
            .any(|row| row.hrv_rmssd_ms == Some(68.2))
    );
    let metric_provenance_json: serde_json::Value =
        serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        metric_provenance_json["algorithm"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID
    );
    assert_eq!(
        metric_provenance_json["algorithm_version"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION
    );
    assert_eq!(metric_provenance_json["source_kind"], "device_sensor");
    let provenance_rows = store
        .metric_provenance_for_metric("daily_recovery", metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
    let provenance_json: serde_json::Value =
        serde_json::from_str(&provenance_rows[0].provenance_json).unwrap();
    assert_eq!(
        provenance_json["algorithm"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID
    );
    assert_eq!(
        provenance_json["algorithm_version"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION
    );
}

#[test]
fn bridge_validates_resting_heart_rate_against_whoop_label_without_writing_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "rhr-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-rhr-validation-history-1",
                    "frame_id": "bridge-rhr-validation-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(58),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-rhr-validation-history-2",
                    "frame_id": "bridge-rhr-validation-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "rhr-validation-1",
        "method": "metrics.resting_hr_capture_validation",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-27",
            "timezone": "Europe/London",
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_sample_count": 2,
            "official_whoop_resting_hr_bpm": 58.0,
            "tolerance_bpm": 1.0,
            "label_provenance": {
                "source": "whoop_app_manual_read",
                "official_labels_are_labels": true,
                "captured_by": "bridge_test"
            }
        }
    }));
    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.resting-heart-rate-capture-validation-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["official_whoop_resting_hr_bpm"], 58.0);
    assert_eq!(report["local_resting_hr_bpm"], 58.0);
    assert_eq!(report["resting_hr_error_bpm"], 0.0);
    assert_eq!(report["resting_hr_within_tolerance"], true);
    assert_eq!(report["provided_label_count"], 1);
    assert_eq!(report["matching_label_count"], 1);
    assert_eq!(
        report["algorithm_id"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID
    );
    assert_eq!(
        report["algorithm_version"],
        GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION
    );
    assert_eq!(report["resting_hr_rollup"]["daily_metric_written"], false);

    let store = GooseStore::open(&db).unwrap();
    assert!(
        store
            .daily_recovery_metrics_between(1_779_842_400_000, 1_779_928_800_000)
            .unwrap()
            .is_empty()
    );

    let blocked = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "rhr-validation-missing-label-policy",
        "method": "metrics.resting_hr_capture_validation",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-27",
            "timezone": "Europe/London",
            "start": "2026-05-27T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_sample_count": 2,
            "official_whoop_resting_hr_bpm": 58.0,
            "tolerance_bpm": 1.0
        }
    }));
    assert!(blocked.ok, "{:?}", blocked.error);
    let blocked_report = blocked.result.unwrap();
    assert_eq!(blocked_report["pass"], false);
    assert!(
        blocked_report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
    assert_eq!(
        blocked_report["resting_hr_rollup"]["daily_metric_written"],
        false
    );
}

#[test]
fn bridge_persists_unavailable_recovery_widget_status_with_provenance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-unavailable-status-1",
        "method": "metrics.recovery_unavailable_daily_status",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start": "2026-06-02T00:00:00Z",
            "end": "2026-06-02T08:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.recovery-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["unavailable_metric_count"], 4);
    assert_eq!(report["written_metric_count"], 4);
    assert_eq!(report["metric_provenance_written_count"], 4);
    assert!(report["statuses"].as_array().unwrap().iter().any(|status| {
        status["metric_id"] == "hrv_rmssd_ms"
            && status["source_kind"] == "unavailable"
            && status["blocker_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "no_trusted_hrv_rr_intervals")
    }));
    assert!(report["statuses"].as_array().unwrap().iter().any(|status| {
        status["metric_id"] == "oxygen_saturation_percent"
            && status["blocker_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "oxygen_saturation_decoder_not_implemented")
    }));

    let start_time_unix_ms = report["start_time_unix_ms"].as_i64().unwrap();
    let end_time_unix_ms = report["end_time_unix_ms"].as_i64().unwrap();
    let store = GooseStore::open(&db).unwrap();
    let rows = store
        .daily_recovery_metrics_between(start_time_unix_ms, end_time_unix_ms)
        .unwrap();
    let unavailable_rows = rows
        .iter()
        .filter(|row| row.source_kind == "unavailable")
        .collect::<Vec<_>>();
    assert_eq!(unavailable_rows.len(), 4);
    assert!(unavailable_rows.iter().all(|row| {
        row.resting_hr_bpm.is_none()
            && row.hrv_rmssd_ms.is_none()
            && row.respiratory_rate_rpm.is_none()
            && row.oxygen_saturation_percent.is_none()
            && row.skin_temperature_delta_c.is_none()
            && row.confidence == 0.0
    }));

    let hrv_row = unavailable_rows
        .iter()
        .find(|row| row.daily_metric_id.contains("hrv-rmssd-ms"))
        .unwrap();
    let hrv_provenance_json: serde_json::Value =
        serde_json::from_str(&hrv_row.provenance_json).unwrap();
    assert_eq!(
        hrv_provenance_json["algorithm"],
        GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_ID
    );
    assert_eq!(
        hrv_provenance_json["algorithm_version"],
        GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_VERSION
    );
    assert_eq!(hrv_provenance_json["source_kind"], "unavailable");
    assert_eq!(
        hrv_provenance_json["value_policy"],
        "no_metric_value_written_until_packet_semantics_are_verified"
    );
    assert!(
        hrv_provenance_json["blocker_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "no_trusted_hrv_rr_intervals")
    );

    let provenance_rows = store
        .metric_provenance_for_metric("daily_recovery", &hrv_row.daily_metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
    assert_eq!(provenance_rows[0].source_kind, "unavailable");
    assert_eq!(provenance_rows[0].confidence, Some(0.0));

    let refresh = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-unavailable-status-refresh",
        "method": "metrics.recovery_unavailable_daily_status",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start": "2026-06-02T00:00:00Z",
            "end": "2026-06-02T08:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "write_metric": true
        }
    }));
    assert!(refresh.ok, "{:?}", refresh.error);
    let refresh_report = refresh.result.unwrap();
    assert_eq!(refresh_report["written_metric_count"], 0);
    assert_eq!(refresh_report["metric_provenance_written_count"], 0);
    assert_eq!(
        store
            .daily_recovery_metrics_between(start_time_unix_ms, end_time_unix_ms)
            .unwrap()
            .into_iter()
            .filter(|row| row.source_kind == "unavailable")
            .count(),
        4
    );
}

#[test]
fn bridge_rolls_up_local_energy_into_daily_activity_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-daily-rollup-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-energy-history-1",
                    "frame_id": "bridge-energy-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-history-2",
                    "frame_id": "bridge-energy-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(120),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-motion",
                    "frame_id": "bridge-energy-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:05:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-daily-rollup-1",
        "method": "metrics.energy_daily_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-28",
            "timezone": "Europe/London",
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.energy-daily-rollup-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["daily_metric_written"], true);
    assert_eq!(report["metric_provenance_written"], true);
    assert_eq!(report["active_kcal"], 17.9);
    assert_eq!(report["resting_kcal"], 12.2);
    assert_eq!(report["total_kcal"], 30.1);
    assert_eq!(report["confidence"], 0.77);
    assert_eq!(report["heart_rate_sample_count"], 3);
    assert_eq!(report["motion_sample_count"], 1);
    assert_eq!(report["covered_minutes"], 10.0);
    assert_eq!(report["profile_weight_kg"], 80.0);

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.source_kind, "local_estimate");
    assert_eq!(metric.active_kcal, Some(17.9));
    assert_eq!(metric.resting_kcal, Some(12.2));
    assert_eq!(metric.total_kcal, Some(30.1));
    assert_eq!(metric.confidence, 0.77);
    let metric_provenance_json: serde_json::Value =
        serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        metric_provenance_json["algorithm"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID
    );
    assert_eq!(
        metric_provenance_json["algorithm_version"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION
    );
    assert_eq!(metric_provenance_json["source_kind"], "local_estimate");
    assert_eq!(metric_provenance_json["official_labels_policy"], "not_used");
    let provenance_rows = store
        .metric_provenance_for_metric("daily_activity", metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
    let provenance_json: serde_json::Value =
        serde_json::from_str(&provenance_rows[0].provenance_json).unwrap();
    assert_eq!(
        provenance_json["algorithm"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID
    );
    assert_eq!(
        provenance_json["algorithm_version"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION
    );
}

#[test]
fn bridge_energy_confidence_uses_only_device_counter_step_cadence_support() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    {
        let store = GooseStore::open(&db).unwrap();
        store
            .upsert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-step-device-counter-energy-support",
                date_key: "2026-05-28",
                timezone: "Europe/London",
                start_time_unix_ms: 0,
                end_time_unix_ms: i64::MAX,
                steps: Some(500),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(90.0),
                source_kind: "device_counter",
                confidence: 0.82,
                inputs_json: r#"{"fixture":"device_counter_steps"}"#,
                quality_flags_json: r#"["device_counter_step_rollup"]"#,
                provenance_json: r#"{"source_kind":"device_counter","algorithm":"goose.steps.device_counter.v0"}"#,
            })
            .unwrap();
        store
            .upsert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-step-local-estimate-energy-ignored",
                date_key: "2026-05-28",
                timezone: "Europe/London",
                start_time_unix_ms: 0,
                end_time_unix_ms: i64::MAX,
                steps: Some(999),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(120.0),
                source_kind: "local_estimate",
                confidence: 0.50,
                inputs_json: r#"{"fixture":"local_estimate_steps"}"#,
                quality_flags_json: r#"["raw_motion_step_estimate"]"#,
                provenance_json: r#"{"source_kind":"local_estimate","algorithm":"goose.steps.raw_motion_estimate.v0"}"#,
            })
            .unwrap();
    }

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-daily-step-support-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-energy-step-support-history-1",
                    "frame_id": "bridge-energy-step-support-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-step-support-history-2",
                    "frame_id": "bridge-energy-step-support-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(120),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-step-support-motion",
                    "frame_id": "bridge-energy-step-support-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:05:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-daily-step-support-1",
        "method": "metrics.energy_daily_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-28",
            "timezone": "Europe/London",
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.energy-daily-rollup-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["confidence"], 0.81);
    assert_eq!(report["step_cadence_source_kind"], "device_counter");
    assert_eq!(report["step_metric_count"], 1);
    assert_eq!(report["step_count"], 500);
    assert_eq!(report["average_cadence_spm"], 90.0);

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["daily_metric_id"].as_str().unwrap();
    let metric = store.daily_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.confidence, 0.81);
    let inputs_json: serde_json::Value = serde_json::from_str(&metric.inputs_json).unwrap();
    assert_eq!(
        inputs_json["step_cadence_support"]["source_kind"],
        "device_counter"
    );
    assert_eq!(inputs_json["step_cadence_support"]["metric_count"], 1);
    assert_eq!(inputs_json["step_cadence_support"]["steps"], 500);
    assert_eq!(
        inputs_json["step_cadence_support"]["metric_ids"][0],
        "daily-step-device-counter-energy-support"
    );
    let provenance_json: serde_json::Value = serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        provenance_json["step_cadence_support"],
        inputs_json["step_cadence_support"]
    );
}

#[test]
fn bridge_persists_unavailable_energy_status_with_provenance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-unavailable-status-1",
        "method": "metrics.energy_unavailable_daily_status",
        "args": {
            "database_path": db_path,
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "start": "2026-06-02T00:00:00Z",
            "end": "2026-06-03T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.energy-unavailable-daily-status-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["energy_daily_rollup"]["pass"], false);
    assert_eq!(report["available_energy_metric_count"], 0);
    assert_eq!(report["unavailable_metric_count"], 3);
    assert_eq!(report["written_metric_count"], 3);
    assert_eq!(report["metric_provenance_written_count"], 3);
    assert!(report["statuses"].as_array().unwrap().iter().any(|status| {
        status["metric_id"] == "total_kcal"
            && status["source_kind"] == "unavailable"
            && status["blocker_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "insufficient_heart_rate_samples")
    }));

    let store = GooseStore::open(&db).unwrap();
    let rows = store.daily_activity_metrics_between(0, i64::MAX).unwrap();
    let unavailable_rows = rows
        .iter()
        .filter(|row| row.source_kind == "unavailable")
        .collect::<Vec<_>>();
    assert_eq!(unavailable_rows.len(), 3);
    assert!(unavailable_rows.iter().all(|row| {
        row.steps.is_none()
            && row.active_kcal.is_none()
            && row.resting_kcal.is_none()
            && row.total_kcal.is_none()
            && row.confidence == 0.0
    }));

    let total_row = unavailable_rows
        .iter()
        .find(|row| row.daily_metric_id.contains("total-kcal"))
        .unwrap();
    let provenance_json: serde_json::Value =
        serde_json::from_str(&total_row.provenance_json).unwrap();
    assert_eq!(
        provenance_json["algorithm"],
        GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_ID
    );
    assert_eq!(
        provenance_json["algorithm_version"],
        GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_VERSION
    );
    assert_eq!(provenance_json["source_kind"], "unavailable");
    assert_eq!(provenance_json["metric_id"], "total_kcal");

    let provenance_rows = store
        .metric_provenance_for_metric("daily_activity", &total_row.daily_metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
    assert_eq!(provenance_rows[0].source_kind, "unavailable");
    assert_eq!(provenance_rows[0].confidence, Some(0.0));
}

#[test]
fn bridge_rolls_up_local_energy_into_hourly_activity_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    {
        let store = GooseStore::open(&db).unwrap();
        store
            .upsert_hourly_activity_metric(HourlyActivityMetricInput {
                hourly_metric_id: "hourly-step-device-counter-energy-support",
                date_key: "2026-05-28",
                timezone: "Europe/London",
                start_time_unix_ms: 0,
                end_time_unix_ms: i64::MAX,
                steps: Some(125),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(96.0),
                source_kind: "device_counter",
                confidence: 0.84,
                inputs_json: r#"{"fixture":"hourly_device_counter_steps"}"#,
                quality_flags_json: r#"["device_counter_step_hourly_rollup"]"#,
                provenance_json: r#"{"source_kind":"device_counter","algorithm":"goose.steps.device_counter.v0"}"#,
            })
            .unwrap();
    }

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-hourly-rollup-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-energy-hour-history-1",
                    "frame_id": "bridge-energy-hour-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-hour-history-2",
                    "frame_id": "bridge-energy-hour-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(120),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-hour-motion",
                    "frame_id": "bridge-energy-hour-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:05:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-hourly-rollup-1",
        "method": "metrics.energy_hourly_rollup",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-28",
            "timezone": "Europe/London",
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "write_metric": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.energy-hourly-rollup-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["hourly_metric_written"], true);
    assert_eq!(report["metric_provenance_written"], true);
    assert_eq!(report["active_kcal"], 17.9);
    assert_eq!(report["resting_kcal"], 12.2);
    assert_eq!(report["total_kcal"], 30.1);
    assert_eq!(report["confidence"], 0.81);
    assert_eq!(report["step_cadence_source_kind"], "device_counter");
    assert_eq!(report["step_metric_count"], 1);
    assert_eq!(report["step_count"], 125);
    assert_eq!(report["average_cadence_spm"], 96.0);

    let store = GooseStore::open(&db).unwrap();
    let metric_id = report["hourly_metric_id"].as_str().unwrap();
    let metric = store.hourly_activity_metric(metric_id).unwrap().unwrap();
    assert_eq!(metric.source_kind, "local_estimate");
    assert_eq!(metric.active_kcal, Some(17.9));
    assert_eq!(metric.resting_kcal, Some(12.2));
    assert_eq!(metric.total_kcal, Some(30.1));
    assert_eq!(metric.confidence, 0.81);
    let inputs_json: serde_json::Value = serde_json::from_str(&metric.inputs_json).unwrap();
    assert_eq!(
        inputs_json["step_cadence_support"]["source_kind"],
        "device_counter"
    );
    assert_eq!(inputs_json["step_cadence_support"]["metric_count"], 1);
    assert_eq!(inputs_json["step_cadence_support"]["steps"], 125);
    let metric_provenance_json: serde_json::Value =
        serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        metric_provenance_json["algorithm"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID
    );
    assert_eq!(
        metric_provenance_json["algorithm_version"],
        GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION
    );
    assert_eq!(metric_provenance_json["source_kind"], "local_estimate");
    assert_eq!(metric_provenance_json["rollup_kind"], "hourly_activity");
    assert_eq!(
        metric_provenance_json["step_cadence_support"],
        inputs_json["step_cadence_support"]
    );
    assert_eq!(metric_provenance_json["official_labels_policy"], "not_used");
    let provenance_rows = store
        .metric_provenance_for_metric("hourly_activity", metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);

    let listed = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-hourly-list-1",
        "method": "metrics.hourly_activity_metrics",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1779969600000i64,
            "end_time_unix_ms": 1779970500000i64
        }
    }));
    assert!(listed.ok, "{:?}", listed.error);
    let list_report = listed.result.unwrap();
    assert_eq!(list_report["metric_count"], 2);
    assert!(
        list_report["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| {
                metric["hourly_metric_id"] == metric_id && metric["source_kind"] == "local_estimate"
            })
    );
}

#[test]
fn bridge_validates_local_energy_against_whoop_labels_without_writing_metric() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-energy-validation-history-1",
                    "frame_id": "bridge-energy-validation-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-validation-history-2",
                    "frame_id": "bridge-energy-validation-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(120),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-energy-validation-motion",
                    "frame_id": "bridge-energy-validation-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:05:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-validation-1",
        "method": "metrics.energy_capture_validation",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-28",
            "timezone": "Europe/London",
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "capture_kind": "walk",
            "official_whoop_total_kcal": 24.0,
            "tolerance_kcal": 500.0,
            "relative_tolerance_fraction": 0.25,
            "label_provenance": {
                "source": "manual_whoop_app_readout",
                "captured_by": "test",
                "official_labels_are_labels": true
            }
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.energy-capture-validation-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["provided_label_count"], 1);
    assert_eq!(report["matching_label_count"], 1);
    assert_eq!(report["total_kcal_within_tolerance"], true);
    assert_eq!(report["energy_rollup"]["daily_metric_written"], false);

    let missing_policy_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "energy-validation-missing-label-policy",
        "method": "metrics.energy_capture_validation",
        "args": {
            "database_path": db_path,
            "date_key": "2026-05-28",
            "timezone": "Europe/London",
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:15:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_heart_rate_samples": 2,
            "capture_kind": "walk",
            "official_whoop_total_kcal": 24.0,
            "tolerance_kcal": 500.0,
            "relative_tolerance_fraction": 0.25,
            "label_provenance": {
                "source": "manual_whoop_app_readout",
                "captured_by": "test"
            }
        }
    }));

    assert!(
        missing_policy_response.ok,
        "{:?}",
        missing_policy_response.error
    );
    let missing_policy_report = missing_policy_response.result.unwrap();
    assert_eq!(missing_policy_report["pass"], false);
    assert_eq!(missing_policy_report["matching_label_count"], 1);
    assert!(
        missing_policy_report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "official_label_policy_not_marked")
    );

    let store = GooseStore::open(&db).unwrap();
    assert!(
        store
            .daily_activity_metrics_between(0, i64::MAX)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn bridge_builds_local_strain_score_from_feature_reports() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "strain-feature-score-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-strain-history-1",
                    "frame_id": "bridge-strain-history-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(60),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-strain-history-2",
                    "frame_id": "bridge-strain-history-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(80),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-strain-history-3",
                    "frame_id": "bridge-strain-history-3.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:20:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "strain-feature-score-1",
        "method": "metrics.strain_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:30:00Z",
            "resting_start": "2026-05-28T00:00:00Z",
            "resting_end": "2026-05-29T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "resting_baseline_min_days": 1
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.strain-feature-score-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["max_hr_basis"], "observed_window_max_hr_bpm");
    assert_eq!(report["strain_input"]["resting_hr_bpm"], 60.0);
    assert_eq!(report["strain_input"]["average_hr_bpm"], 80.0);
    assert_eq!(report["strain_input"]["max_hr_bpm"], 100.0);
    assert_eq!(
        report["score_result"]["output"]["score_0_to_21"],
        serde_json::json!(5.25)
    );
}

#[test]
fn bridge_builds_local_sleep_score_from_motion_features() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-feature-score-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-sleep-motion-1",
                    "frame_id": "bridge-sleep-motion-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T22:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-sleep-motion-2",
                    "frame_id": "bridge-sleep-motion-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T23:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-sleep-motion-3",
                    "frame_id": "bridge-sleep-motion-3.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T00:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(10000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-sleep-motion-4",
                    "frame_id": "bridge-sleep-motion-4.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T01:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-sleep-motion-5",
                    "frame_id": "bridge-sleep-motion-5.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T02:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-feature-score-1",
        "method": "metrics.sleep_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T22:00:00Z",
            "end": "2026-05-28T03:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "sleep_need_minutes": 240.0,
            "low_motion_threshold_0_to_1": 0.05,
            "disturbance_motion_threshold_0_to_1": 0.20,
            "target_midpoint_minutes_since_midnight": 0.0,
            "persist_algorithm_run": true,
            "algorithm_run_id": "bridge-sleep-feature-run-1"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.sleep-feature-score-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["sleep_window"]["time_in_bed_minutes"], 240.0);
    assert_eq!(report["sleep_window"]["sleep_duration_minutes"], 180.0);
    assert_eq!(report["sleep_window"]["disturbance_count"], 1);
    assert_eq!(report["score_result"]["output"]["score_0_to_100"], 80.75);
    assert_eq!(
        report["persisted_algorithm_run"]["run_id"],
        "bridge-sleep-feature-run-1"
    );
    assert_eq!(report["persisted_algorithm_run"]["inserted"], true);

    let external_sessions = (0..7)
        .map(|index| {
            let start = 1_779_400_000_000i64 + index * 86_400_000;
            let stage_minutes = if index == 0 {
                serde_json::json!({
                    "awake": 40.0,
                    "asleep": 220.0,
                    "asleep_deep": 80.0,
                    "asleep_rem": 140.0
                })
            } else {
                serde_json::json!({
                    "awake": 40.0,
                    "core": 220.0,
                    "deep": 80.0,
                    "rem": 140.0
                })
            };
            serde_json::json!({
                "sleep_id": format!("bridge-healthkit-sleep-{index}"),
                "source": "Apple Watch",
                "platform": "healthkit",
                "platform_record_id": format!("hk-sleep-{index}"),
                "start_time_unix_ms": start,
                "end_time_unix_ms": start + 8 * 60 * 60 * 1000,
                "timezone": "Europe/London",
                "stage_summary": {
                    "minutes_by_stage": stage_minutes,
                    "stage_count": 4
                },
                "confidence": 0.86,
                "provenance": {
                    "source": "healthkit_sleep_analysis",
                    "import_policy": "external_history_context_only"
                }
            })
        })
        .collect::<Vec<_>>();
    let mut external_sessions = external_sessions;
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-prior-extra-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-prior-extra-0",
        "start_time_unix_ms": 1_779_313_600_000i64,
        "end_time_unix_ms": 1_779_342_400_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 40.0,
                "core": 220.0,
                "deep": 80.0,
                "rem": 140.0
            },
            "stage_count": 4
        },
        "confidence": 0.86,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-low-confidence-0",
        "source": "Manual entry",
        "platform": "healthkit",
        "platform_record_id": "hk-low-confidence-0",
        "start_time_unix_ms": 1_779_486_400_000i64,
        "end_time_unix_ms": 1_779_515_200_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 70.0,
                "core": 210.0,
                "deep": 70.0,
                "rem": 130.0
            },
            "stage_count": 4
        },
        "confidence": 0.40,
        "provenance": {
            "source": "manual_sleep_edit",
            "import_policy": "external_history_context_only"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-overlap-conflict-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-overlap-conflict-0",
        "start_time_unix_ms": 1_779_572_800_000i64,
        "end_time_unix_ms": 1_779_601_600_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 30.0,
                "core": 230.0,
                "deep": 80.0,
                "rem": 140.0
            },
            "stage_count": 4
        },
        "confidence": 0.86,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only",
            "overlap_conflict": true,
            "overlap_policy": "kept_as_external_context_for_review"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-travel-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-travel-0",
        "start_time_unix_ms": 1_779_659_200_000i64,
        "end_time_unix_ms": 1_779_688_000_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 25.0,
                "core": 240.0,
                "deep": 75.0,
                "rem": 140.0
            },
            "stage_count": 4
        },
        "confidence": 0.88,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only",
            "journal_tags": ["travel"]
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-manual-entry-0",
        "source": "Manual entry",
        "platform": "healthkit",
        "platform_record_id": "hk-manual-entry-0",
        "start_time_unix_ms": 1_779_313_600_000i64,
        "end_time_unix_ms": 1_779_342_400_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 35.0,
                "core": 235.0,
                "deep": 75.0,
                "rem": 135.0
            },
            "stage_count": 4
        },
        "confidence": 0.86,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only",
            "manual_entry": true
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-future-sleep-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-future-sleep-0",
        "start_time_unix_ms": 1_800_000_000_000i64,
        "end_time_unix_ms": 1_800_028_800_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 30.0,
                "core": 230.0,
                "deep": 80.0,
                "rem": 140.0
            },
            "stage_count": 4
        },
        "confidence": 0.90,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-nap-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-nap-0",
        "start_time_unix_ms": 1_779_890_400_000i64,
        "end_time_unix_ms": 1_779_893_100_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "core": 35.0,
                "deep": 10.0
            },
            "stage_count": 2
        },
        "confidence": 0.82,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only",
            "detected_context": "daytime_nap"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-low-confidence-nap-0",
        "source": "Manual entry",
        "platform": "healthkit",
        "platform_record_id": "hk-low-confidence-nap-0",
        "start_time_unix_ms": 1_779_897_600_000i64,
        "end_time_unix_ms": 1_779_899_400_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "core": 30.0
            },
            "stage_count": 1
        },
        "confidence": 0.40,
        "provenance": {
            "source": "manual_sleep_edit",
            "import_policy": "external_history_context_only",
            "detected_context": "daytime_nap"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-impossible-stage-total-0",
        "source": "Apple Watch",
        "platform": "healthkit",
        "platform_record_id": "hk-impossible-stage-total-0",
        "start_time_unix_ms": 1_778_968_000_000i64,
        "end_time_unix_ms": 1_778_996_800_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "awake": 120.0,
                "core": 300.0,
                "deep": 180.0,
                "rem": 180.0
            },
            "stage_count": 4
        },
        "confidence": 0.86,
        "provenance": {
            "source": "healthkit_sleep_analysis",
            "import_policy": "external_history_context_only"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-unknown-only-0",
        "source": "Health Connect",
        "platform": "health_connect",
        "platform_record_id": "hc-unknown-only-0",
        "start_time_unix_ms": 1_779_745_600_000i64,
        "end_time_unix_ms": 1_779_774_400_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "unknown": 240.0,
                "not_applicable": 240.0
            },
            "stage_count": 2
        },
        "confidence": 0.86,
        "provenance": {
            "source": "health_connect_sleep_session",
            "import_policy": "external_history_context_only"
        }
    }));
    external_sessions.push(serde_json::json!({
        "sleep_id": "bridge-healthkit-not-applicable-nap-0",
        "source": "Health Connect",
        "platform": "health_connect",
        "platform_record_id": "hc-not-applicable-nap-0",
        "start_time_unix_ms": 1_779_900_000_000i64,
        "end_time_unix_ms": 1_779_903_600_000i64,
        "timezone": "Europe/London",
        "stage_summary": {
            "minutes_by_stage": {
                "not_applicable": 60.0
            },
            "stage_count": 1
        },
        "confidence": 0.86,
        "provenance": {
            "source": "health_connect_sleep_session",
            "import_policy": "external_history_context_only"
        }
    }));
    let external_import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-feature-score-external-history",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path,
            "sessions": external_sessions,
            "stages": []
        }
    }));
    assert!(external_import.ok, "{:?}", external_import.error);

    let v1_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-feature-score-v1",
        "method": "metrics.sleep_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T22:00:00Z",
            "end": "2026-05-28T03:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "sleep_need_minutes": 240.0,
            "low_motion_threshold_0_to_1": 0.05,
            "disturbance_motion_threshold_0_to_1": 0.20,
            "target_midpoint_minutes_since_midnight": 0.0,
            "algorithm_id": "goose.sleep.v1",
            "algorithm_version": "0.1.0",
            "history_import_in_progress": true
        }
    }));
    assert!(v1_response.ok, "{:?}", v1_response.error);
    let v1_report = v1_response.result.unwrap();
    assert_eq!(v1_report["score_result"]["algorithm_id"], "goose.sleep.v1");
    assert_eq!(
        v1_report["score_result"]["output"]["model_status"],
        "importing_history"
    );
    assert_eq!(
        v1_report["score_result"]["output"]["status_report"]["imported_platform_sleep_nights"],
        0
    );
    assert_eq!(
        v1_report["score_result"]["output"]["status_report"]["excluded_sleep_nights"],
        12
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["model_status"]["excluded_sleep_nights"],
        12
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["model_status"]["repeated_low_confidence_nights"],
        false
    );
    let v1_stage_segments = v1_report["sleep_v1_input"]["stage_segments"]
        .as_array()
        .unwrap();
    assert!(!v1_stage_segments.is_empty());
    for segment in v1_stage_segments {
        let probabilities = segment["stage_probabilities"].as_object().unwrap();
        assert_eq!(probabilities.len(), 4);
        let probability_sum = probabilities
            .values()
            .map(|value| value.as_f64().unwrap())
            .sum::<f64>();
        assert!(
            (probability_sum - 1.0).abs() < 1e-9,
            "expected stage probabilities to sum to 1.0, got {probability_sum}: {probabilities:?}"
        );
        let stage_kind = segment["stage_kind"].as_str().unwrap();
        assert!(probabilities[stage_kind].as_f64().unwrap() > 0.0);
    }
    assert_eq!(
        v1_report["score_result"]["output"]["baseline"],
        serde_json::Value::Null
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(|night| night["night_id"] == "bridge-healthkit-sleep-0"
                && night["stage_minutes"]["core"] == 220.0
                && night["stage_minutes"]["deep"] == 80.0
                && night["stage_minutes"]["rem"] == 140.0
                && night["stage_minutes"]["asleep"].is_null()
                && night["excluded_from_baseline"] == true)
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |night| night["night_id"] == "bridge-healthkit-impossible-stage-total-0"
                    && (night["stage_minutes"]["awake"].as_f64().unwrap() - 73.84615384615384)
                        .abs()
                        < 1e-9
                    && (night["stage_minutes"]["core"].as_f64().unwrap() - 184.6153846153846).abs()
                        < 1e-9
                    && (night["stage_minutes"]["deep"].as_f64().unwrap() - 110.76923076923077)
                        .abs()
                        < 1e-9
                    && (night["stage_minutes"]["rem"].as_f64().unwrap() - 110.76923076923077).abs()
                        < 1e-9
                    && night["excluded_from_baseline"] == true
            )
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |night| night["night_id"] == "bridge-healthkit-low-confidence-0"
                    && night["excluded_from_baseline"] == true
            )
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |night| night["night_id"] == "bridge-healthkit-overlap-conflict-0"
                    && night["excluded_from_baseline"] == true
            )
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(|night| night["night_id"] == "bridge-healthkit-travel-0"
                && night["excluded_from_baseline"] == true)
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |night| night["night_id"] == "bridge-healthkit-manual-entry-0"
                    && night["excluded_from_baseline"] == true
            )
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .all(|night| night["night_id"] != "bridge-healthkit-unknown-only-0")
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .all(|night| night["night_id"] != "bridge-healthkit-future-sleep-0")
    );
    assert!(
        v1_report["sleep_v1_input"]["prior_nights"]
            .as_array()
            .unwrap()
            .iter()
            .all(|night| night["night_id"] != "bridge-healthkit-sleep-6")
    );
    assert_eq!(v1_report["sleep_v1_input"]["naps_minutes"], 0.0);
    assert_eq!(
        v1_report["sleep_v1_input"]["model_status"]["history_import_in_progress"],
        true
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["bedtime_deviation_minutes"],
        0.0
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["wake_time_deviation_minutes"],
        0.0
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["prior_nights"][0]["bedtime_deviation_minutes"],
        0.0
    );
    assert_eq!(
        v1_report["sleep_v1_input"]["prior_nights"][0]["wake_time_deviation_minutes"],
        0.0
    );

    let export_dir = tempdir.path().join("sleep-feature-export");
    let export = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-feature-score-export",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db_path,
            "output_dir": export_dir.display().to_string(),
            "start": "2026-05-27T22:00:00Z",
            "end": "2026-05-28T03:00:00Z",
            "data_families": ["algorithm_runs"]
        }
    }));
    assert!(export.ok, "{:?}", export.error);
    let export_report = export.result.unwrap();
    assert_eq!(export_report["algorithm_run_rows"], 1);
    let algorithm_runs = fs::read_to_string(export_dir.join("data/algorithm_runs.jsonl")).unwrap();
    assert!(algorithm_runs.contains("bridge-sleep-feature-run-1"));
    assert!(algorithm_runs.contains("bridge-sleep-motion"));
}

#[test]
fn bridge_canonicalizes_external_sleep_stage_aliases_on_import() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("external-sleep-stage-aliases.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-stage-aliases",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-stage-aliases-1",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-stage-aliases-1",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "minutes_by_stage": {
                            "asleep": 120.0,
                            "asleep_deep": 40.0,
                            "asleep_rem": 60.0,
                            "awake": 20.0,
                            "in_bed": 480.0
                        }
                    },
                    "confidence": 0.88,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ],
            "stages": [
                {
                    "stage_id": "external-sleep-stage-alias-in-bed",
                    "sleep_id": "external-sleep-stage-aliases-1",
                    "stage_kind": "inBed",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_923_400_000i64,
                    "confidence": 0.90,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                },
                {
                    "stage_id": "external-sleep-stage-alias-deep",
                    "sleep_id": "external-sleep-stage-aliases-1",
                    "stage_kind": "asleep_deep",
                    "start_time_unix_ms": 1_779_923_400_000i64,
                    "end_time_unix_ms": 1_779_925_800_000i64,
                    "confidence": 0.84,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                },
                {
                    "stage_id": "external-sleep-stage-alias-rem",
                    "sleep_id": "external-sleep-stage-aliases-1",
                    "stage_kind": "asleep_rem",
                    "start_time_unix_ms": 1_779_925_800_000i64,
                    "end_time_unix_ms": 1_779_929_400_000i64,
                    "confidence": 0.82,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let store = GooseStore::open(&db_path).unwrap();
    let stages = store
        .external_sleep_stages_for_session("external-sleep-stage-aliases-1")
        .unwrap();
    assert_eq!(stages.len(), 3);
    assert_eq!(stages[0].stage_kind, "in_bed");
    assert_eq!(stages[1].stage_kind, "deep");
    assert_eq!(stages[2].stage_kind, "rem");
}

#[test]
fn bridge_rejects_external_sleep_stage_outside_parent_session() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir
        .path()
        .join("external-sleep-stage-outside-session.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-stage-outside-session",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-stage-parent",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-stage-parent",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "minutes_by_stage": {
                            "asleep": 220.0,
                            "awake": 20.0
                        }
                    },
                    "confidence": 0.88,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ],
            "stages": [
                {
                    "stage_id": "external-sleep-stage-before-parent",
                    "sleep_id": "external-sleep-stage-parent",
                    "stage_kind": "asleep_deep",
                    "start_time_unix_ms": 1_779_919_740_000i64,
                    "end_time_unix_ms": 1_779_923_400_000i64,
                    "confidence": 0.84,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(message.contains("must be within parent sleep session"));

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_session("external-sleep-stage-parent")
            .unwrap()
            .is_none()
    );
}

#[test]
fn bridge_rejects_external_sleep_stage_without_parent_session() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("external-sleep-orphan-stage.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-orphan-stage",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [],
            "stages": [
                {
                    "stage_id": "external-sleep-orphan-stage",
                    "sleep_id": "external-sleep-missing-parent",
                    "stage_kind": "asleep_deep",
                    "start_time_unix_ms": 1_779_923_400_000i64,
                    "end_time_unix_ms": 1_779_925_800_000i64,
                    "confidence": 0.84,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(message.contains("external sleep session external-sleep-missing-parent not found"));

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_stages_for_session("external-sleep-missing-parent")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn bridge_rejects_malformed_external_sleep_stage_summary_atomically() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir
        .path()
        .join("external-sleep-malformed-stage-summary.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-malformed-stage-summary",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-malformed-stage-summary",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-malformed-stage-summary",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "asleep": 220.0,
                        "awake": 20.0
                    },
                    "confidence": 0.88,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(message.contains("stage_summary_json must contain minutes_by_stage object"));

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_session("external-sleep-malformed-stage-summary")
            .unwrap()
            .is_none()
    );
}

#[test]
fn bridge_rejects_malformed_external_sleep_confidence_atomically() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir
        .path()
        .join("external-sleep-malformed-confidence.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-malformed-confidence",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-malformed-confidence",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-malformed-confidence",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "minutes_by_stage": {
                            "asleep": 220.0,
                            "awake": 20.0
                        }
                    },
                    "confidence": null,
                    "provenance": {
                        "source": "healthkit_sleep_analysis",
                        "baseline_exclusion_reasons": ["invalid_confidence"]
                    }
                }
            ],
            "stages": [
                {
                    "stage_id": "external-sleep-malformed-confidence-stage",
                    "sleep_id": "external-sleep-malformed-confidence",
                    "stage_kind": "asleep_deep",
                    "start_time_unix_ms": 1_779_923_400_000i64,
                    "end_time_unix_ms": 1_779_925_800_000i64,
                    "confidence": 0.84,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(message.contains("invalid args"));
    assert!(message.contains("invalid type: null"));

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_session("external-sleep-malformed-confidence")
            .unwrap()
            .is_none()
    );
}

#[test]
fn bridge_rejects_malformed_external_sleep_stage_confidence_atomically() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir
        .path()
        .join("external-sleep-malformed-stage-confidence.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-malformed-stage-confidence",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-malformed-stage-confidence",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-malformed-stage-confidence",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "minutes_by_stage": {
                            "asleep": 220.0,
                            "awake": 20.0
                        }
                    },
                    "confidence": 0.88,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ],
            "stages": [
                {
                    "stage_id": "external-sleep-malformed-stage-confidence-stage",
                    "sleep_id": "external-sleep-malformed-stage-confidence",
                    "stage_kind": "asleep_deep",
                    "start_time_unix_ms": 1_779_923_400_000i64,
                    "end_time_unix_ms": 1_779_925_800_000i64,
                    "confidence": null,
                    "provenance": {
                        "source": "healthkit_sleep_analysis",
                        "baseline_exclusion_reasons": ["invalid_confidence"]
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(message.contains("invalid args"));
    assert!(message.contains("invalid type: null"));

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_session("external-sleep-malformed-stage-confidence")
            .unwrap()
            .is_none()
    );
}

#[test]
fn bridge_rejects_unknown_external_sleep_stage_kind_atomically() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir
        .path()
        .join("external-sleep-unknown-stage-kind.sqlite");
    let db_path_text = db_path.to_str().unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "external-sleep-unknown-stage-kind",
        "method": "sleep.import_external_history",
        "args": {
            "database_path": db_path_text,
            "sessions": [
                {
                    "sleep_id": "external-sleep-unknown-stage-kind",
                    "source": "Apple Watch",
                    "platform": "healthkit",
                    "platform_record_id": "hk-unknown-stage-kind",
                    "start_time_unix_ms": 1_779_919_800_000i64,
                    "end_time_unix_ms": 1_779_933_000_000i64,
                    "timezone": "Europe/London",
                    "stage_summary": {
                        "minutes_by_stage": {
                            "asleep": 220.0,
                            "awake": 20.0
                        }
                    },
                    "confidence": 0.88,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ],
            "stages": [
                {
                    "stage_id": "external-sleep-stage-typo",
                    "sleep_id": "external-sleep-unknown-stage-kind",
                    "stage_kind": "deeep",
                    "start_time_unix_ms": 1_779_923_400_000i64,
                    "end_time_unix_ms": 1_779_925_800_000i64,
                    "confidence": 0.84,
                    "provenance": {
                        "source": "healthkit_sleep_analysis"
                    }
                }
            ]
        }
    }));

    assert!(!response.ok);
    let message = response.error.unwrap().message;
    assert!(
        message.contains(
            "external sleep stage external-sleep-stage-typo kind deeep is not recognized"
        )
    );

    let store = GooseStore::open(&db_path).unwrap();
    assert!(
        store
            .external_sleep_session("external-sleep-unknown-stage-kind")
            .unwrap()
            .is_none()
    );
}

#[test]
fn bridge_persists_sleep_manual_corrections_as_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite").display().to_string();

    let correction = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-correction-label-1",
        "method": "sleep.add_correction_label",
        "args": {
            "database_path": db_path,
            "label_id": "manual-sleep-window-2026-05-28",
            "sleep_id": "packet-derived-sleep-2026-05-28",
            "label_type": "sleep_window",
            "start_time_unix_ms": 1779919200000i64,
            "end_time_unix_ms": 1779948000000i64,
            "value": {
                "corrected_start_time_unix_ms": 1779920100000i64,
                "corrected_end_time_unix_ms": 1779947100000i64,
                "is_nap": false
            },
            "source": "manual",
            "confidence": 0.98,
            "provenance": {
                "ui_surface": "metrics.sleep.manual_correction",
                "storage_policy": "label_only"
            }
        }
    }));

    assert!(correction.ok, "{:?}", correction.error);
    let result = correction.result.unwrap();
    assert_eq!(
        result["storage_policy"],
        "manual_corrections_are_labels_not_raw_packet_edits"
    );
    assert_eq!(result["label"]["label_type"], "sleep_window");
    assert_eq!(
        result["label"]["value_json"],
        "{\"corrected_end_time_unix_ms\":1779947100000,\"corrected_start_time_unix_ms\":1779920100000,\"is_nap\":false}"
    );

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-correction-label-list-1",
        "method": "sleep.list_correction_labels",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1779910000000i64,
            "end_time_unix_ms": 1779950000000i64
        }
    }));

    assert!(list.ok, "{:?}", list.error);
    let list_result = list.result.unwrap();
    assert_eq!(list_result["label_count"], 1);
    assert_eq!(list_result["sleep_window_label_count"], 1);
    assert_eq!(list_result["sleep_stage_label_count"], 0);
    assert_eq!(list_result["nap_label_count"], 0);
    assert_eq!(list_result["distinct_sleep_window_sleep_id_count"], 1);
    assert_eq!(
        list_result["labels"][0]["label_id"],
        "manual-sleep-window-2026-05-28"
    );
}

#[test]
fn bridge_lists_sleep_correction_label_proof_counts() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite").display().to_string();

    for (label_id, sleep_id, label_type, value) in [
        (
            "manual-window-1",
            "packet-derived-sleep-1",
            "sleep_window",
            serde_json::json!({
                "corrected_start_time_unix_ms": 1779920100000i64,
                "corrected_end_time_unix_ms": 1779947100000i64
            }),
        ),
        (
            "manual-window-2",
            "packet-derived-sleep-1",
            "sleep_window",
            serde_json::json!({
                "corrected_start_time_unix_ms": 1779920200000i64,
                "corrected_end_time_unix_ms": 1779947000000i64
            }),
        ),
        (
            "manual-stage-1",
            "packet-derived-sleep-1",
            "sleep_stage",
            serde_json::json!({"stage_kind": "deep"}),
        ),
        (
            "manual-nap-1",
            "packet-derived-sleep-2",
            "nap",
            serde_json::json!({"is_nap": true}),
        ),
    ] {
        let correction = request(serde_json::json!({
            "schema": "goose.bridge.request.v1",
            "request_id": format!("sleep-correction-{label_id}"),
            "method": "sleep.add_correction_label",
            "args": {
                "database_path": db_path,
                "label_id": label_id,
                "sleep_id": sleep_id,
                "label_type": label_type,
                "start_time_unix_ms": 1779919200000i64,
                "end_time_unix_ms": 1779948000000i64,
                "value": value,
                "source": "manual",
                "confidence": 0.98,
                "provenance": {
                    "ui_surface": "metrics.sleep.manual_correction",
                    "storage_policy": "label_only"
                }
            }
        }));
        assert!(correction.ok, "{:?}", correction.error);
    }

    let list = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-correction-proof-counts",
        "method": "sleep.list_correction_labels",
        "args": {
            "database_path": db_path,
            "start_time_unix_ms": 1779910000000i64,
            "end_time_unix_ms": 1779950000000i64
        }
    }));

    assert!(list.ok, "{:?}", list.error);
    let list_result = list.result.unwrap();
    assert_eq!(list_result["label_count"], 4);
    assert_eq!(list_result["sleep_window_label_count"], 2);
    assert_eq!(list_result["sleep_stage_label_count"], 1);
    assert_eq!(list_result["nap_label_count"], 1);
    assert_eq!(list_result["distinct_sleep_window_sleep_id_count"], 1);
}

#[test]
fn bridge_validates_packet_sleep_window_against_manual_label_tolerance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite").display().to_string();

    let frames = [
        ("sleep-window-motion-0", "2026-05-27T22:00:00Z", 10000),
        ("sleep-window-motion-1", "2026-05-27T23:00:00Z", 1000),
        ("sleep-window-motion-2", "2026-05-28T00:00:00Z", 1000),
        ("sleep-window-motion-3", "2026-05-28T01:00:00Z", 1000),
        ("sleep-window-motion-4", "2026-05-28T02:00:00Z", 1000),
    ]
    .into_iter()
    .map(|(id, captured_at, sample_value)| {
        serde_json::json!({
            "evidence_id": id,
            "frame_id": format!("{id}.frame.0"),
            "source": "ios.corebluetooth.notification",
            "captured_at": captured_at,
            "device_model": "WHOOP 5.0 Goose",
            "frame_hex": k10_motion_frame_hex_with_value(sample_value),
            "sensitivity": "user-owned-capture",
            "device_type": "GOOSE"
        })
    })
    .collect::<Vec<_>>();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-window-label-validation-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": frames
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let correction = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-window-label-validation-label",
        "method": "sleep.add_correction_label",
        "args": {
            "database_path": db_path,
            "label_id": "manual-reviewed-window-2026-05-27",
            "sleep_id": "packet-derived-sleep-2026-05-27",
            "label_type": "sleep_window",
            "start_time_unix_ms": 1779919800000i64,
            "end_time_unix_ms": 1779933000000i64,
            "value": {
                "corrected_start_time_unix_ms": 1779919800000i64,
                "corrected_end_time_unix_ms": 1779933000000i64,
                "review_source": "hand_reviewed"
            },
            "source": "manual",
            "confidence": 0.95,
            "provenance": {
                "ui_surface": "metrics.sleep.manual_correction",
                "review_policy": "hand_reviewed_sleep_window",
                "source": "manual"
            }
        }
    }));
    assert!(correction.ok, "{:?}", correction.error);

    let validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "sleep-window-label-validation-1",
        "method": "sleep.validate_window_labels",
        "args": {
            "database_path": db_path,
            "start": "2026-05-27T22:00:00Z",
            "end": "2026-05-28T03:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "sleep_need_minutes": 240.0,
            "start_tolerance_minutes": 20.0,
            "end_tolerance_minutes": 20.0,
            "duration_tolerance_minutes": 30.0
        }
    }));

    assert!(validation.ok, "{:?}", validation.error);
    let report = validation.result.unwrap();
    assert_eq!(
        report["schema"],
        "goose.sleep-window-label-validation-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["label_count"], 1);
    assert_eq!(report["compared_label_count"], 1);
    assert_eq!(report["passing_label_count"], 1);
    assert_eq!(
        report["provenance"]["comparison_policy"],
        "packet_derived_sleep_window_vs_hand_reviewed_sleep_window_label"
    );
    assert_eq!(report["comparisons"][0]["start_delta_minutes"], 10.0);
    assert_eq!(report["comparisons"][0]["end_delta_minutes"], 10.0);
    assert_eq!(report["comparisons"][0]["duration_delta_minutes"], 20.0);
}

#[test]
fn bridge_builds_local_recovery_score_from_feature_reports_and_provided_vitals() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-feature-score-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-recovery-r17-current",
                    "frame_id": "bridge-recovery-r17-current.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 825, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-r17-baseline",
                    "frame_id": "bridge-recovery-r17-baseline.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 850, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-resting-hr",
                    "frame_id": "bridge-recovery-resting-hr.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(55),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-strain-hr-1",
                    "frame_id": "bridge-recovery-strain-hr-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(60),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-strain-hr-2",
                    "frame_id": "bridge-recovery-strain-hr-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T12:10:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(80),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-strain-hr-3",
                    "frame_id": "bridge-recovery-strain-hr-3.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T12:20:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(100),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-sleep-motion-1",
                    "frame_id": "bridge-recovery-sleep-motion-1.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T22:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-sleep-motion-2",
                    "frame_id": "bridge-recovery-sleep-motion-2.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T23:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-sleep-motion-3",
                    "frame_id": "bridge-recovery-sleep-motion-3.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T00:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(10000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-sleep-motion-4",
                    "frame_id": "bridge-recovery-sleep-motion-4.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T01:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-recovery-sleep-motion-5",
                    "frame_id": "bridge-recovery-sleep-motion-5.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T02:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex_with_value(1000),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-feature-score-1",
        "method": "metrics.recovery_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T06:00:00Z",
            "end": "2026-05-28T06:05:00Z",
            "hrv_start": "2026-05-28T04:00:00Z",
            "hrv_end": "2026-05-28T05:00:00Z",
            "hrv_baseline_start": "2026-05-27T00:00:00Z",
            "hrv_baseline_end": "2026-05-28T00:00:00Z",
            "resting_start": "2026-05-28T00:00:00Z",
            "resting_end": "2026-05-29T00:00:00Z",
            "sleep_start": "2026-05-27T22:00:00Z",
            "sleep_end": "2026-05-28T03:00:00Z",
            "prior_strain_start": "2026-05-27T12:00:00Z",
            "prior_strain_end": "2026-05-27T12:30:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "resting_baseline_min_days": 1,
            "hrv_min_rr_intervals_to_compute": 2,
            "hrv_baseline_min_days": 1,
            "sleep_need_minutes": 240.0,
            "low_motion_threshold_0_to_1": 0.05,
            "disturbance_motion_threshold_0_to_1": 0.20,
            "target_midpoint_minutes_since_midnight": 0.0,
            "prior_strain_resting_baseline_min_days": 1,
            "respiratory_rate_rpm": 14.0,
            "respiratory_rate_baseline_rpm": 14.0,
            "skin_temp_delta_c": 0.0,
            "provided_vitals_source": "metrics.recovery_sensor_discovery",
            "provided_vitals_provenance_json": "{\"source_kind\":\"device_sensor\",\"decoder\":\"goose_packet_decoder\",\"packet_family\":\"vital_event\",\"source\":\"bridge_test\"}",
            "persist_algorithm_run": true,
            "algorithm_run_id": "bridge-recovery-feature-run-1"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.recovery-feature-score-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["recovery_input"]["hrv_rmssd_ms"], 25.0);
    assert_eq!(report["recovery_input"]["hrv_baseline_rmssd_ms"], 50.0);
    assert_eq!(report["recovery_input"]["resting_hr_bpm"], 72.0);
    assert_eq!(report["recovery_input"]["sleep_score_0_to_100"], 80.75);
    assert_eq!(
        report["provided_vitals"]["source"],
        "metrics.recovery_sensor_discovery"
    );
    assert_eq!(
        report["provided_vitals"]["provenance"]["provided_vitals_provenance"]["source_kind"],
        "device_sensor"
    );
    assert_eq!(
        report["score_result"]["provenance"]["provided_vitals"]["source"],
        "metrics.recovery_sensor_discovery"
    );
    assert_eq!(
        report["persisted_algorithm_run"]["run_id"],
        "bridge-recovery-feature-run-1"
    );
    assert!(
        !report["score_result"]["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "provided_resp_temp_inputs_not_packet_derived")
    );

    let export_dir = tempdir.path().join("recovery-export.goosebundle");
    let export = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-feature-export",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db_path,
            "output_dir": export_dir.display().to_string(),
            "start": "2026-05-28T06:00:00Z",
            "end": "2026-05-28T06:05:00Z",
            "data_families": ["algorithm_runs"]
        }
    }));
    assert!(export.ok, "{:?}", export.error);
    let export_report = export.result.unwrap();
    assert_eq!(export_report["algorithm_run_rows"], 1);
    let algorithm_runs = fs::read_to_string(export_dir.join("data/algorithm_runs.jsonl")).unwrap();
    assert!(algorithm_runs.contains("bridge-recovery-feature-run-1"));
    assert!(algorithm_runs.contains("metrics.recovery_sensor_discovery"));
    assert!(algorithm_runs.contains("provided_vitals"));

    let blocked = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "recovery-feature-score-untrusted-vitals",
        "method": "metrics.recovery_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T06:00:00Z",
            "end": "2026-05-28T06:05:00Z",
            "hrv_start": "2026-05-28T04:00:00Z",
            "hrv_end": "2026-05-28T05:00:00Z",
            "hrv_baseline_start": "2026-05-27T00:00:00Z",
            "hrv_baseline_end": "2026-05-28T00:00:00Z",
            "resting_start": "2026-05-28T00:00:00Z",
            "resting_end": "2026-05-29T00:00:00Z",
            "sleep_start": "2026-05-27T22:00:00Z",
            "sleep_end": "2026-05-28T03:00:00Z",
            "prior_strain_start": "2026-05-27T12:00:00Z",
            "prior_strain_end": "2026-05-27T12:30:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "resting_baseline_min_days": 1,
            "hrv_min_rr_intervals_to_compute": 2,
            "hrv_baseline_min_days": 1,
            "sleep_need_minutes": 240.0,
            "low_motion_threshold_0_to_1": 0.05,
            "disturbance_motion_threshold_0_to_1": 0.20,
            "target_midpoint_minutes_since_midnight": 0.0,
            "prior_strain_resting_baseline_min_days": 1,
            "respiratory_rate_rpm": 14.0,
            "respiratory_rate_baseline_rpm": 14.0,
            "skin_temp_delta_c": 0.0,
            "provided_vitals_source": "manual_bridge_test",
            "persist_algorithm_run": true,
            "algorithm_run_id": "bridge-recovery-untrusted-vitals"
        }
    }));
    assert!(blocked.ok, "{:?}", blocked.error);
    let blocked_report = blocked.result.unwrap();
    assert_eq!(blocked_report["pass"], false);
    assert_eq!(
        blocked_report["persisted_algorithm_run"]["blocked_reason"],
        "report_not_passed"
    );
}

#[test]
fn bridge_builds_local_stress_score_from_feature_reports() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let import = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "stress-feature-score-import",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "bridge-stress-current-hr",
                    "frame_id": "bridge-stress-current-hr.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(90),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-stress-motion",
                    "frame_id": "bridge-stress-motion.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:10Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": k10_motion_frame_hex(),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-stress-resting-hr",
                    "frame_id": "bridge-stress-resting-hr.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": historical_k18_frame_hex(60),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-stress-current-r17",
                    "frame_id": "bridge-stress-current-r17.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:01:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 825, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "bridge-stress-baseline-r17",
                    "frame_id": "bridge-stress-baseline-r17.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-27T04:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": r17_frame_hex(&[800, 850, 800]),
                    "sensitivity": "user-owned-capture",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import.ok, "{:?}", import.error);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "stress-feature-score-1",
        "method": "metrics.stress_score_from_features",
        "args": {
            "database_path": db_path,
            "start": "2026-05-28T12:00:00Z",
            "end": "2026-05-28T12:05:00Z",
            "resting_start": "2026-05-28T00:00:00Z",
            "resting_end": "2026-05-28T06:00:00Z",
            "hrv_start": "2026-05-28T12:00:00Z",
            "hrv_end": "2026-05-28T12:05:00Z",
            "hrv_baseline_start": "2026-05-27T00:00:00Z",
            "hrv_baseline_end": "2026-05-28T00:00:00Z",
            "min_owned_captures": 1,
            "require_trusted_evidence": true,
            "resting_baseline_min_days": 1,
            "hrv_min_rr_intervals_to_compute": 2,
            "hrv_baseline_min_days": 1
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let report = response.result.unwrap();
    assert_eq!(report["schema"], "goose.stress-feature-score-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["stress_input"]["heart_rate_bpm"], 81.0);
    assert_eq!(report["stress_input"]["resting_hr_bpm"], 60.0);
    assert_eq!(report["stress_input"]["hrv_rmssd_ms"], 25.0);
    assert_eq!(report["stress_input"]["hrv_baseline_rmssd_ms"], 50.0);
    assert!(
        report["score_result"]["output"]["score_0_to_100"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}

#[test]
fn bridge_persists_command_validation_and_returns_direct_send_gates() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let missing_gate = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-gate-missing",
        "method": "commands.direct_send_gate",
        "args": {
            "database_path": db_path,
            "command": "get_hello"
        }
    }));
    assert!(missing_gate.ok, "{:?}", missing_gate.error);
    let missing_gate_result = missing_gate.result.unwrap();
    assert_eq!(missing_gate_result["direct_send_allowed"], false);
    assert!(
        missing_gate_result["missing_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement == "command_validation_record")
    );

    let template = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-template",
        "method": "commands.evidence_template",
        "args": {}
    }));
    assert!(template.ok, "{:?}", template.error);
    assert_eq!(
        template.result.unwrap()["evidence"]
            .as_array()
            .unwrap()
            .len(),
        COMMAND_DEFINITIONS.len()
    );

    let validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-validate",
        "method": "commands.validate_evidence",
        "args": {
            "database_path": db_path,
            "persist": true,
            "evidence": [
                {
                    "command_id": "get_hello",
                    "officialCaptureCount": 1,
                    "evidenceSource": "user_owned_official_capture",
                    "provenance": {
                        "capture_app": "whoop_official",
                        "capture_kind": "passive_ble_observation",
                        "owner": "user"
                    },
                    "officialFrameHex": GET_HELLO_FRAME,
                    "localFrameHex": GET_HELLO_FRAME,
                    "officialServiceUuid": COMMAND_SERVICE_UUID,
                    "localServiceUuid": COMMAND_SERVICE_UUID,
                    "officialCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "localCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "officialWriteType": COMMAND_WRITE_TYPE,
                    "localWriteType": COMMAND_WRITE_TYPE,
                    "officialResponseFrameHex": GET_HELLO_RESPONSE_FRAME,
                    "responseParser": true,
                    "visibleUserIntent": true,
                    "eventLogging": true,
                    "timeoutBehavior": true
                }
            ]
        }
    }));
    assert!(validation.ok, "{:?}", validation.error);
    let validation_result = validation.result.unwrap();
    assert_eq!(validation_result["direct_send_ready_count"], 1);
    assert!(
        validation_result["evidence_source_summary"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |summary| summary["evidence_source"] == "user_owned_official_capture"
                    && summary["capture_kind"] == "passive_ble_observation"
                    && summary["owner"] == "user"
                    && summary["trusted_for_promotion_count"] == 1
            )
    );
    assert_eq!(
        validation_result["blocked_count"],
        COMMAND_DEFINITIONS.len() - 1
    );

    let emulator_log = format!(
        "[1.000s] Write 16 bytes to command_to_strap: {GET_HELLO_FRAME}\n\
         [1.010s] Notify command_from_strap queued=true: {GET_HELLO_RESPONSE_FRAME}\n"
    );
    let emulator_evidence = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-emulator-log-evidence",
        "method": "commands.evidence_from_emulator_log",
        "args": {
            "source_log": "whoop-emulator.log",
            "log_text": emulator_log,
            "visible_user_intent": true
        }
    }));
    assert!(emulator_evidence.ok, "{:?}", emulator_evidence.error);
    let emulator_report = emulator_evidence.result.unwrap();
    assert_eq!(emulator_report["schema"], "goose.command-evidence.v1");
    assert_eq!(emulator_report["source_log"], "whoop-emulator.log");
    assert_eq!(emulator_report["pass"], true);
    assert_eq!(emulator_report["official_capture_ready"], true);
    assert_eq!(emulator_report["local_frame_match_ready"], false);
    assert_eq!(emulator_report["direct_validation_ready"], false);
    assert_eq!(emulator_report["evidence_count"], 1);
    assert_eq!(emulator_report["evidence"][0]["command"], "get_hello");
    assert_eq!(
        emulator_report["evidence"][0]["local_frame_hex"],
        serde_json::Value::Null
    );
    assert!(
        emulator_report["evidence"][0]["provenance_json"]
            .as_str()
            .unwrap()
            .contains("official_app_to_macos_emulator")
    );

    let local_match = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-local-frame-match",
        "method": "commands.promote_local_frame_matches",
        "args": {
            "evidence": emulator_report["evidence"].clone(),
            "candidates": [
                {
                    "command": "get_hello",
                    "dryRunFrameHex": GET_HELLO_FRAME,
                    "source": "whoop-rev build-command GET_HELLO --frame",
                    "provenance": {"builder": "whoop-rev", "dry_run": true}
                }
            ]
        }
    }));
    assert!(local_match.ok, "{:?}", local_match.error);
    let local_match_report = local_match.result.unwrap();
    assert_eq!(
        local_match_report["schema"],
        "goose.command-local-frame-match-report.v1"
    );
    assert_eq!(local_match_report["matched_count"], 1);
    assert_eq!(local_match_report["promotion_ready"], true);
    assert_eq!(local_match_report["all_frames_matched"], true);
    assert!(
        local_match_report["next_actions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        local_match_report["evidence"][0]["local_frame_hex"],
        GET_HELLO_FRAME
    );

    let ready_gate = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-gate-ready",
        "method": "commands.direct_send_gate",
        "args": {
            "database_path": db_path,
            "command": "get_hello"
        }
    }));
    assert!(ready_gate.ok, "{:?}", ready_gate.error);
    let ready_gate_result = ready_gate.result.unwrap();
    assert_eq!(ready_gate_result["command"], "get_hello");
    assert_eq!(ready_gate_result["command_number"], 145);
    assert_eq!(ready_gate_result["risk_gate"], "read_only");
    assert_eq!(ready_gate_result["direct_send_allowed"], true);
    assert_eq!(
        ready_gate_result["validated_local_frame_hex"],
        GET_HELLO_FRAME
    );
    assert_eq!(
        ready_gate_result["validated_service_uuid"],
        COMMAND_SERVICE_UUID
    );
    assert_eq!(
        ready_gate_result["validated_characteristic_uuid"],
        COMMAND_CHARACTERISTIC_UUID
    );
    assert_eq!(
        ready_gate_result["validated_write_type"],
        COMMAND_WRITE_TYPE
    );

    let blocked_preflight = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-preflight-blocked",
        "method": "commands.direct_send_preflight",
        "args": {
            "database_path": db_path,
            "command": "get_hello",
            "now_unix_ms": 1_000,
            "visible_user_intent": true,
            "dry_run_bytes_shown": true,
            "dry_run_frame_hex": "aa01",
            "dry_run_service_uuid": COMMAND_SERVICE_UUID,
            "dry_run_characteristic_uuid": COMMAND_CHARACTERISTIC_UUID,
            "dry_run_write_type": COMMAND_WRITE_TYPE,
            "session_log_ready": true,
            "connection_state": "connected",
            "active_device_id": "strap-1"
        }
    }));
    assert!(blocked_preflight.ok, "{:?}", blocked_preflight.error);
    let blocked_preflight_result = blocked_preflight.result.unwrap();
    assert_eq!(blocked_preflight_result["direct_send_allowed"], false);
    assert!(
        blocked_preflight_result["missing_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement == "short_lived_user_override")
    );

    let allowed_preflight = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-preflight-allowed",
        "method": "commands.direct_send_preflight",
        "args": {
            "database_path": db_path,
            "command": "get_hello",
            "now_unix_ms": 1_000,
            "override_expires_at_unix_ms": 16_000,
            "visible_user_intent": true,
            "dry_run_bytes_shown": true,
            "dry_run_frame_hex": GET_HELLO_FRAME,
            "dry_run_service_uuid": COMMAND_SERVICE_UUID,
            "dry_run_characteristic_uuid": COMMAND_CHARACTERISTIC_UUID,
            "dry_run_write_type": COMMAND_WRITE_TYPE,
            "session_log_ready": true,
            "connection_state": "connected",
            "active_device_id": "strap-1"
        }
    }));
    assert!(allowed_preflight.ok, "{:?}", allowed_preflight.error);
    let allowed_preflight_result = allowed_preflight.result.unwrap();
    assert_eq!(
        allowed_preflight_result["schema"],
        "goose.command-direct-send-preflight.v1"
    );
    assert_eq!(allowed_preflight_result["command"], "get_hello");
    assert_eq!(allowed_preflight_result["direct_send_allowed"], true);
    assert_eq!(allowed_preflight_result["override_expires_in_ms"], 15_000);
    assert_eq!(
        allowed_preflight_result["dry_run_frame_hex"],
        GET_HELLO_FRAME
    );
    assert_eq!(
        allowed_preflight_result["dry_run_service_uuid"],
        COMMAND_SERVICE_UUID
    );
    assert_eq!(
        allowed_preflight_result["dry_run_characteristic_uuid"],
        COMMAND_CHARACTERISTIC_UUID
    );
    assert_eq!(
        allowed_preflight_result["dry_run_write_type"],
        COMMAND_WRITE_TYPE
    );

    let blocked_gate = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-gate-blocked",
        "method": "commands.direct_send_gate",
        "args": {
            "database_path": db_path,
            "command": "select_wrist"
        }
    }));
    assert!(blocked_gate.ok, "{:?}", blocked_gate.error);
    let blocked_gate_result = blocked_gate.result.unwrap();
    assert_eq!(blocked_gate_result["direct_send_allowed"], false);
    assert_eq!(
        blocked_gate_result["risk_gate"],
        "user_visible_state_change"
    );
    assert!(
        blocked_gate_result["missing_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement == "official_capture_evidence")
    );

    let records = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-records",
        "method": "commands.list_validation_records",
        "args": {
            "database_path": db_path
        }
    }));
    assert!(records.ok, "{:?}", records.error);
    let records_result = records.result.unwrap();
    assert_eq!(
        records_result.as_array().unwrap().len(),
        COMMAND_DEFINITIONS.len()
    );
    assert!(
        records_result
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["command"] == "get_hello"
                && record["risk_gate"] == "read_only"
                && record["direct_send_ready"] == true)
    );
}

#[test]
fn bridge_reports_command_capture_plan_for_official_validation_work() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-plan-validate",
        "method": "commands.validate_evidence",
        "args": {
            "database_path": db_path,
            "persist": true,
            "evidence": [
                {
                    "command_id": "get_hello",
                    "officialCaptureCount": 1,
                    "evidenceSource": "user_owned_official_capture",
                    "provenance": {
                        "capture_app": "whoop_official",
                        "capture_kind": "passive_ble_observation",
                        "owner": "user"
                    },
                    "officialFrameHex": GET_HELLO_FRAME,
                    "localFrameHex": GET_HELLO_FRAME,
                    "officialServiceUuid": COMMAND_SERVICE_UUID,
                    "localServiceUuid": COMMAND_SERVICE_UUID,
                    "officialCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "localCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "officialWriteType": COMMAND_WRITE_TYPE,
                    "localWriteType": COMMAND_WRITE_TYPE,
                    "officialResponseFrameHex": GET_HELLO_RESPONSE_FRAME,
                    "responseParser": true,
                    "visibleUserIntent": true,
                    "eventLogging": true,
                    "timeoutBehavior": true
                }
            ]
        }
    }));
    assert!(validation.ok, "{:?}", validation.error);

    let plan = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-plan",
        "method": "commands.capture_plan",
        "args": {
            "database_path": db_path,
            "commands": ["get_hello", "select_wrist", "start_firmware_load_new"]
        }
    }));
    assert!(plan.ok, "{:?}", plan.error);
    let report = plan.result.unwrap();
    assert_eq!(report["schema"], "goose.command-capture-plan-report.v1");
    assert_eq!(report["pass"], false);
    assert_eq!(report["requested_commands_valid"], true);
    assert_eq!(report["validation_records_valid"], true);
    assert_eq!(report["all_selected_gates_ready"], false);
    assert_eq!(report["critical_gates_ready"], false);
    assert_eq!(report["capture_actions_ready"], false);
    assert_eq!(report["command_count"], 3);
    assert_eq!(report["ready_count"], 1);
    assert_eq!(report["locked_count"], 2);
    assert_eq!(report["critical_locked_count"], 1);
    assert_eq!(report["gates"]["get_hello"]["direct_send_allowed"], true);
    assert_eq!(
        report["next_command_focus"]["command"],
        "start_firmware_load_new"
    );
    assert_eq!(
        report["next_command_focus"]["risk_gate"],
        "critical_state_change"
    );
    assert_eq!(
        report["gates"]["select_wrist"]["risk_gate"],
        "user_visible_state_change"
    );
    assert!(
        report["family_summaries"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |family| family["family"] == "firmware_dfu" && family["critical_locked_count"] == 1
            )
    );
    assert!(
        report["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["command"] == "select_wrist"
                && action["requirement"] == "official_capture_evidence"
                && action["summary"].as_str().unwrap().contains("official app"))
    );
}

#[test]
fn bridge_imports_exported_command_validation_records_with_provenance_gate() {
    let validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-validate-for-import",
        "method": "commands.validate_evidence",
        "args": {
            "persist": false,
            "evidence": [
                {
                    "command_id": "get_hello",
                    "officialCaptureCount": 1,
                    "evidenceSource": "user_owned_official_capture",
                    "provenance": {
                        "capture_app": "whoop_official",
                        "capture_kind": "passive_ble_observation",
                        "owner": "user"
                    },
                    "officialFrameHex": GET_HELLO_FRAME,
                    "localFrameHex": GET_HELLO_FRAME,
                    "officialServiceUuid": COMMAND_SERVICE_UUID,
                    "localServiceUuid": COMMAND_SERVICE_UUID,
                    "officialCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "localCharacteristicUuid": COMMAND_CHARACTERISTIC_UUID,
                    "officialWriteType": COMMAND_WRITE_TYPE,
                    "localWriteType": COMMAND_WRITE_TYPE,
                    "officialResponseFrameHex": GET_HELLO_RESPONSE_FRAME,
                    "responseParser": true,
                    "visibleUserIntent": true,
                    "eventLogging": true,
                    "timeoutBehavior": true
                }
            ]
        }
    }));
    assert!(validation.ok, "{:?}", validation.error);
    let validation_result = validation.result.unwrap();
    let get_hello = validation_result["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["command"] == "get_hello")
        .unwrap()
        .clone();
    assert_eq!(get_hello["direct_send_ready"], true);
    assert_eq!(get_hello["validated_owner"], "user");

    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite").display().to_string();
    let imported = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-validation-import",
        "method": "commands.import_validation_records",
        "args": {
            "database_path": db_path.clone(),
            "records": [
                {
                    "command": "get_hello",
                    "risk_gate": "read_only",
                    "direct_send_ready": true,
                    "report_json": get_hello.clone()
                }
            ]
        }
    }));
    assert!(imported.ok, "{:?}", imported.error);
    let imported_result = imported.result.unwrap();
    assert_eq!(imported_result["pass"], true);
    assert_eq!(imported_result["inserted_count"], 1);
    assert_eq!(imported_result["ready_count"], 1);

    let ready_gate = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-gate-after-import",
        "method": "commands.direct_send_gate",
        "args": {
            "database_path": db_path,
            "command": "get_hello"
        }
    }));
    assert!(ready_gate.ok, "{:?}", ready_gate.error);
    let ready_gate_result = ready_gate.result.unwrap();
    assert_eq!(ready_gate_result["direct_send_allowed"], true);
    assert_eq!(
        ready_gate_result["validated_capture_kind"],
        "passive_ble_observation"
    );

    let mut bad_report = get_hello.clone();
    bad_report
        .as_object_mut()
        .unwrap()
        .remove("validated_provenance_json");
    let bad_tempdir = tempfile::tempdir().unwrap();
    let bad_db_path = bad_tempdir
        .path()
        .join("goose.sqlite")
        .display()
        .to_string();
    let rejected = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "command-validation-import-rejected",
        "method": "commands.import_validation_records",
        "args": {
            "database_path": bad_db_path,
            "records": [
                {
                    "command": "get_hello",
                    "risk_gate": "read_only",
                    "direct_send_ready": true,
                    "report_json": bad_report
                }
            ]
        }
    }));
    assert!(rejected.ok, "{:?}", rejected.error);
    let rejected_result = rejected.result.unwrap();
    assert_eq!(rejected_result["pass"], false);
    assert_eq!(rejected_result["inserted_count"], 0);
    assert!(
        rejected_result["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap()
                .contains("validated_provenance_json_required"))
    );
}

#[test]
fn bridge_runs_storage_check_against_app_database_path() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "storage-1",
        "method": "storage.check",
        "args": {
            "database_path": db,
            "self_test": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["schema_version_valid"], true);
    assert_eq!(result["foreign_keys_valid"], true);
    assert_eq!(result["integrity_valid"], true);
    assert_eq!(result["tables_present"], true);
    assert_eq!(result["required_columns_present"], true);
    assert_eq!(result["row_counts_ready"], true);
    assert_eq!(result["self_test_ready"], true);
    assert_eq!(result["storage_ready"], true);
    assert_eq!(result["actual_schema_version"], CURRENT_SCHEMA_VERSION);
    assert_eq!(result["self_test"]["foreign_key_rejected"], true);
    assert!(result["next_actions"].as_array().unwrap().is_empty());
}

#[test]
fn bridge_records_debug_session_command_events_for_debug_tab_stream() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();

    let started = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "debug-start-session",
        "method": "debug.start_session",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-bridge",
            "started_at_unix_ms": 1779840000000u64,
            "bridge": {
                "url": "ws://127.0.0.1:49152/goose-debug/stream?token=session-token",
                "bind_host": "127.0.0.1",
                "token_required": true,
                "token_present": true,
                "remote_bind_enabled": false,
                "visible_remote_bind_toggle": false
            }
        }
    }));
    assert!(started.ok, "{:?}", started.error);
    assert_eq!(
        started.result.as_ref().unwrap()["contract_report"]["pass"],
        true
    );
    assert_eq!(
        started.result.as_ref().unwrap()["contract_report"]["contract_ready"],
        true
    );
    assert_eq!(
        started.result.as_ref().unwrap()["contract_report"]["bridge_valid"],
        true
    );
    assert_eq!(
        started.result.as_ref().unwrap()["contract_report"]["command_results_correlated"],
        true
    );

    let command_started = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "debug-start-command",
        "method": "debug.start_command",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-bridge",
            "received_at_unix_ms": 1779840000100u64,
            "command": {
                "schema": "goose.debug.command.v1",
                "command_id": "cmd-debug-export",
                "command": "export.raw_timeframe",
                "args": {
                    "start": "2026-05-27T00:00:00Z",
                    "end": "2026-05-28T00:00:00Z"
                },
                "dry_run": true
            }
        }
    }));
    assert!(command_started.ok, "{:?}", command_started.error);
    let command_started_result = command_started.result.unwrap();
    assert_eq!(command_started_result["events"][0]["sequence"], 1);
    assert_eq!(
        command_started_result["events"][0]["topic"],
        "command.started"
    );
    assert_eq!(command_started_result["contract_report"]["pass"], false);
    assert_eq!(
        command_started_result["contract_report"]["command_results_correlated"],
        false
    );
    assert_eq!(
        command_started_result["contract_report"]["contract_ready"],
        false
    );
    assert!(
        command_started_result["contract_report"]["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["reason"] == "command_missing_result_event"
                    && action["action"]
                        .as_str()
                        .unwrap()
                        .contains("debug.finish_command")
            })
    );

    let app_event = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "debug-record-event",
        "method": "debug.record_event",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-bridge",
            "time_unix_ms": 1779840000150u64,
            "source": "sqlite",
            "level": "debug",
            "topic": "export.rows.counted",
            "message": "export dry-run counted rows",
            "data": {
                "raw_evidence": 0,
                "decoded_frames": 0
            }
        }
    }));
    assert!(app_event.ok, "{:?}", app_event.error);
    assert_eq!(app_event.result.unwrap()["sequence"], 2);

    let command_finished = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "debug-finish-command",
        "method": "debug.finish_command",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-bridge",
            "time_unix_ms": 1779840000200u64,
            "command_id": "cmd-debug-export",
            "ok": true,
            "message": "export.raw_timeframe dry-run completed",
            "data": {
                "planned_files": ["manifest.json"]
            }
        }
    }));
    assert!(command_finished.ok, "{:?}", command_finished.error);
    let final_snapshot = command_finished.result.unwrap();
    assert_eq!(final_snapshot["contract_report"]["pass"], true);
    assert_eq!(final_snapshot["contract_report"]["contract_ready"], true);
    assert_eq!(
        final_snapshot["contract_report"]["command_results_correlated"],
        true
    );
    assert_eq!(final_snapshot["commands"].as_array().unwrap().len(), 1);
    assert_eq!(final_snapshot["events"].as_array().unwrap().len(), 3);

    let snapshot = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "debug-snapshot",
        "method": "debug.session_snapshot",
        "args": {
            "database_path": db_path,
            "session_id": "debug-session-bridge"
        }
    }));
    assert!(snapshot.ok, "{:?}", snapshot.error);
    let snapshot_result = snapshot.result.unwrap();
    assert_eq!(snapshot_result["contract_report"]["pass"], true);
    assert_eq!(snapshot_result["contract_report"]["contract_ready"], true);
}

#[test]
fn bridge_persists_battery_status_history_debug_events() {
    let tempdir = tempfile::tempdir().unwrap();
    let database_path = tempdir.path().join("goose.sqlite");
    let database_path = database_path.to_str().unwrap().to_string();
    let session_id = "debug-session-battery-status-history";

    let start_session = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "battery-status-start-session",
        "method": "debug.start_session",
        "args": {
            "database_path": database_path.as_str(),
            "session_id": session_id,
            "started_at_unix_ms": 1779840000000u64,
            "bridge": {
                "url": "ws://127.0.0.1:49152/goose-debug/stream?token=test",
                "bind_host": "127.0.0.1",
                "token_required": true,
                "token_present": true,
                "remote_bind_enabled": false,
                "visible_remote_bind_toggle": false
            }
        }
    }));
    assert!(start_session.ok, "{:?}", start_session.error);

    let first_observation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "battery-status-event-1",
        "method": "debug.record_event",
        "args": {
            "database_path": database_path.as_str(),
            "session_id": session_id,
            "time_unix_ms": 1779840000100u64,
            "source": "ble",
            "level": "info",
            "topic": "device.battery_status.observed",
            "message": "battery/status observation recorded",
            "data": {
                "adapter_source": "ios.platform_adapter",
                "battery_level_percent": 87,
                "battery_updated_at_unix_ms": 1779840000100u64,
                "device_status": "subscribed",
                "status_updated_at_unix_ms": 1779840000100u64,
                "service_uuid": "180f",
                "characteristic_uuid": "2a19"
            }
        }
    }));
    assert!(first_observation.ok, "{:?}", first_observation.error);
    let first_result = first_observation.result.unwrap();
    assert_eq!(first_result["sequence"].as_u64().unwrap(), 1);
    assert_eq!(first_result["source"], "ble");
    assert_eq!(first_result["topic"], "device.battery_status.observed");
    assert_eq!(
        first_result["data"]["battery_level_percent"]
            .as_u64()
            .unwrap(),
        87
    );
    assert_eq!(first_result["data"]["device_status"], "subscribed");

    let second_observation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "battery-status-event-2",
        "method": "debug.record_event",
        "args": {
            "database_path": database_path.as_str(),
            "session_id": session_id,
            "time_unix_ms": 1779840005100u64,
            "source": "ble",
            "level": "info",
            "topic": "device.battery_status.observed",
            "message": "battery/status observation recorded",
            "data": {
                "adapter_source": "ios.platform_adapter",
                "battery_level_percent": 86,
                "battery_updated_at_unix_ms": 1779840005100u64,
                "device_status": "connected",
                "status_updated_at_unix_ms": 1779840005100u64,
                "service_uuid": "180f",
                "characteristic_uuid": "2a19"
            }
        }
    }));
    assert!(second_observation.ok, "{:?}", second_observation.error);
    let second_result = second_observation.result.unwrap();
    assert_eq!(second_result["sequence"].as_u64().unwrap(), 2);
    assert_eq!(second_result["source"], "ble");
    assert_eq!(
        second_result["data"]["battery_level_percent"]
            .as_u64()
            .unwrap(),
        86
    );
    assert_eq!(second_result["data"]["device_status"], "connected");

    let snapshot = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "battery-status-session-snapshot",
        "method": "debug.session_snapshot",
        "args": {
            "database_path": database_path.as_str(),
            "session_id": session_id
        }
    }));
    assert!(snapshot.ok, "{:?}", snapshot.error);
    let snapshot_result = snapshot.result.unwrap();
    assert_eq!(snapshot_result["contract_report"]["pass"], true);
    assert_eq!(snapshot_result["contract_report"]["contract_ready"], true);
    assert_eq!(snapshot_result["events"].as_array().unwrap().len(), 2);
    assert_eq!(
        snapshot_result["events"][0]["sequence"].as_u64().unwrap(),
        1
    );
    assert_eq!(snapshot_result["events"][0]["source"], "ble");
    assert_eq!(
        snapshot_result["events"][0]["topic"],
        "device.battery_status.observed"
    );
    assert_eq!(
        snapshot_result["events"][0]["data"]["battery_level_percent"]
            .as_u64()
            .unwrap(),
        87
    );
    assert_eq!(
        snapshot_result["events"][1]["data"]["battery_level_percent"]
            .as_u64()
            .unwrap(),
        86
    );
    assert_eq!(
        snapshot_result["events"][1]["data"]["device_status"],
        "connected"
    );
}

#[test]
fn bridge_exports_raw_timeframe_for_debug_export_flow() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let export_dir = tempdir.path().join("debug-export.goosebundle");
    let zip_path = tempdir.path().join("debug-export.goosebundle.zip");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-raw-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db,
            "output_dir": export_dir,
            "zip_output_path": zip_path,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "include_sqlite": true
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["manifest_ready"], true);
    assert_eq!(result["files_written"], true);
    assert_eq!(result["zip_ready"], true);
    assert_eq!(result["export_ready"], true);
    assert_eq!(result["raw_rows"], 8);
    assert_eq!(result["decoded_frame_rows"], 8);
    assert_eq!(result["packet_timeline_rows"], 8);
    assert_eq!(result["sensor_sample_rows"], 19);
    assert_eq!(result["metric_feature_report_rows"], 7);
    assert_eq!(result["metric_value_rows"], 0);
    assert_eq!(result["metric_component_rows"], 0);
    assert_eq!(result["zip_path"], zip_path.to_str().unwrap());
    assert!(
        result["manifest"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "data/goose.sqlite")
    );

    let validation =
        validate_export_bundle(Path::new(result["output_dir"].as_str().unwrap())).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    let zip_validation = validate_export_bundle(&zip_path).unwrap();
    assert!(zip_validation.pass, "{:?}", zip_validation.issues);

    let bridge_validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-validate-1",
        "method": "export.validate_bundle",
        "args": {
            "path": result["output_dir"].as_str().unwrap()
        }
    }));
    assert!(bridge_validation.ok, "{:?}", bridge_validation.error);
    let bridge_validation_result = bridge_validation.result.unwrap();
    assert_eq!(bridge_validation_result["pass"], true);
    assert_eq!(bridge_validation_result["content"]["csv_valid"], true);
    assert!(
        bridge_validation_result["content"]["csv_row_count_checks"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        bridge_validation_result["bundle_path"],
        result["output_dir"].as_str().unwrap()
    );

    let bridge_zip_validation = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-validate-zip-1",
        "method": "export.validate_bundle",
        "args": {
            "path": zip_path
        }
    }));
    assert!(
        bridge_zip_validation.ok,
        "{:?}",
        bridge_zip_validation.error
    );
    assert_eq!(bridge_zip_validation.result.unwrap()["pass"], true);

    let bridge_privacy_lint = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "privacy-lint-1",
        "method": "privacy.lint",
        "args": {
            "path": result["output_dir"].as_str().unwrap()
        }
    }));
    assert!(bridge_privacy_lint.ok, "{:?}", bridge_privacy_lint.error);
    let privacy_result = bridge_privacy_lint.result.unwrap();
    assert_eq!(privacy_result["schema"], "goose.privacy-lint-report.v1");
    assert_eq!(privacy_result["pass"], true);
    assert_eq!(privacy_result["input_valid"], true);
    assert_eq!(privacy_result["files_readable"], true);
    assert_eq!(privacy_result["scan_coverage_ready"], true);
    assert_eq!(privacy_result["auth_tokens_clear"], true);
    assert_eq!(privacy_result["debug_tokens_clear"], true);
    assert_eq!(privacy_result["private_api_clear"], true);
    assert_eq!(privacy_result["direct_identifiers_clear"], true);
    assert_eq!(privacy_result["privacy_ready"], true);
    assert_eq!(privacy_result["next_actions"].as_array().unwrap().len(), 0);
    assert!(
        privacy_result["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "data/decoded_frames.jsonl")
    );

    let bridge_zip_privacy_lint = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "privacy-lint-zip-1",
        "method": "privacy.lint",
        "args": {
            "path": zip_path
        }
    }));
    assert!(
        bridge_zip_privacy_lint.ok,
        "{:?}",
        bridge_zip_privacy_lint.error
    );
    assert_eq!(bridge_zip_privacy_lint.result.unwrap()["pass"], true);
}

#[test]
fn bridge_privacy_lint_serializes_next_actions_for_leaky_artifact() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("leaky-debug.log");
    fs::write(
        &path,
        "ws://127.0.0.1/goose-debug/stream?token=secret\n\
         GET https://api-7.whoop.com/metrics-service/v1/metrics user_id=123\n",
    )
    .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "privacy-lint-next-actions-1",
        "method": "privacy.lint",
        "args": {
            "path": path
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.privacy-lint-report.v1");
    assert_eq!(result["pass"], false);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["files_readable"], true);
    assert_eq!(result["scan_coverage_ready"], true);
    assert_eq!(result["auth_tokens_clear"], true);
    assert_eq!(result["debug_tokens_clear"], false);
    assert_eq!(result["private_api_clear"], false);
    assert_eq!(result["direct_identifiers_clear"], false);
    assert_eq!(result["privacy_ready"], false);
    let actions = result["next_actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|action| action["reason"] == "debug_query_token"),
        "{actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|action| action["reason"] == "private_whoop_api_material"),
        "{actions:?}"
    );
}

#[test]
fn bridge_capture_sanitize_redacts_owned_capture_before_privacy_lint() {
    let tempdir = tempfile::tempdir().unwrap();
    let input_dir = tempdir.path().join("owned-capture");
    let input_file = input_dir.join("ble/events.jsonl");
    let output_dir = tempdir.path().join("owned-capture.sanitized");
    fs::create_dir_all(input_file.parent().unwrap()).unwrap();
    fs::write(
        &input_file,
        r#"{"frame_hex":"aa0108000001e67123019101363e5c8d","access_token":"secret-token","email":"person@example.com","bluetooth_address":"AA:BB:CC:DD:EE:FF"}"#,
    )
    .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-sanitize-1",
        "method": "capture.sanitize",
        "args": {
            "input_path": input_dir.to_string_lossy(),
            "output_path": output_dir.to_string_lossy(),
            "salt": "bridge-test-salt"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["schema"], "goose.capture-sanitize-report.v1");
    assert_eq!(result["pass"], true);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["output_ready"], true);
    assert_eq!(result["supported_files_written"], true);
    assert_eq!(result["unsupported_files_omitted"], true);
    assert_eq!(result["redaction_scan_clear"], true);
    assert_eq!(result["warnings_clear"], true);
    assert_eq!(result["evidence_complete"], true);
    assert_eq!(result["sanitize_ready"], true);
    assert_eq!(result["totals"]["files_written"], 1);
    assert!(
        result["totals"]["secret_redactions"].as_u64().unwrap() > 0,
        "{result:?}"
    );

    let sanitized = fs::read_to_string(output_dir.join("ble/events.jsonl")).unwrap();
    assert!(sanitized.contains("aa0108000001e67123019101363e5c8d"));
    assert!(!sanitized.contains("secret-token"));
    assert!(!sanitized.contains("person@example.com"));
    assert!(!sanitized.contains("AA:BB:CC:DD:EE:FF"));

    let lint_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-sanitize-privacy-1",
        "method": "privacy.lint",
        "args": {
            "path": output_dir.to_string_lossy()
        }
    }));
    assert!(lint_response.ok, "{:?}", lint_response.error);
    assert_eq!(lint_response.result.unwrap()["pass"], true);
}

#[test]
fn bridge_export_validation_serializes_next_actions_for_failed_bundle() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::write(tempdir.path().join("raw.jsonl"), "{}\n").unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        r#"{
  "schema_version": "goose.export.v1",
  "app_version": "goose-app/bridge-test",
  "core_version": "goose-core/bridge-test",
  "time_window": {"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"},
  "data_families": ["raw_evidence"],
  "files": [{"path": "raw.jsonl", "sha256": "not-the-checksum", "kind": "jsonl"}]
}"#,
    )
    .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-validate-failed-actions-1",
        "method": "export.validate_bundle",
        "args": {
            "path": tempdir.path()
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], false);
    assert!(
        result["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["scope"] == "raw.jsonl" && action["reason"] == "checksum_mismatch"
            })
    );
}

#[test]
fn bridge_raw_export_honors_selected_data_families() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let export_dir = tempdir.path().join("selected-export.goosebundle");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-selected-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db,
            "output_dir": export_dir,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "include_sqlite": true,
            "data_families": ["raw_evidence", "decoded_frames"]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["raw_rows"], 8);
    assert_eq!(result["decoded_frame_rows"], 8);
    assert_eq!(result["packet_timeline_rows"], 0);
    assert_eq!(result["sensor_sample_rows"], 0);
    assert_eq!(result["metric_feature_report_rows"], 0);
    assert_eq!(result["metric_value_rows"], 0);
    assert_eq!(result["metric_component_rows"], 0);
    assert_eq!(result["manifest"]["data_families"][0], "raw_evidence");
    assert_eq!(result["manifest"]["data_families"][1], "decoded_frames");
    assert!(
        result["manifest"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["path"].as_str().is_some_and(
                |path| path.contains("raw_evidence") || path.contains("decoded_frames")
            ))
    );
    assert!(!export_dir.join("data/goose.sqlite").exists());

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
}

#[test]
fn bridge_raw_export_honors_metric_and_algorithm_filters() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let export_dir = tempdir.path().join("filtered-export.goosebundle");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-filtered-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db,
            "output_dir": export_dir,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "data_families": ["metric_features"],
            "metric_families": [" hrv ", "hrv"],
            "algorithm_ids": ["goose.hrv.v0"],
            "algorithm_versions": ["0.1.0"]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["metric_feature_report_rows"], 1);
    assert_eq!(result["manifest"]["filters"]["metric_families"][0], "hrv");
    assert_eq!(
        result["manifest"]["filters"]["algorithm_ids"][0],
        "goose.hrv.v0"
    );
    assert_eq!(
        result["manifest"]["filters"]["algorithm_versions"][0],
        "0.1.0"
    );

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.metric_feature_report_rows, 1);
}

#[test]
fn bridge_raw_export_honors_capture_session_filter() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let db_path = db.display().to_string();
    let export_dir = tempdir.path().join("capture-filtered-export.goosebundle");

    for session_id in ["capture-session-a", "capture-session-b"] {
        let response = request(serde_json::json!({
            "schema": "goose.bridge.request.v1",
            "request_id": format!("start-{session_id}"),
            "method": "capture.start_session",
            "args": {
                "database_path": db_path,
                "session_id": session_id,
                "source": "ios.corebluetooth.notification",
                "started_at_unix_ms": 1770000000000i64,
                "device_model": "WHOOP 5.0 Goose",
                "provenance": {}
            }
        }));
        assert!(response.ok, "{:?}", response.error);
    }

    let import_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "capture-import-filtered",
        "method": "capture.import_frame_batch",
        "args": {
            "database_path": db_path,
            "parser_version": "goose-core/bridge-test",
            "frames": [
                {
                    "evidence_id": "capture-a-command",
                    "frame_id": "capture-a-command.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:00Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": GET_HELLO_FRAME,
                    "sensitivity": "user-owned-capture",
                    "capture_session_id": "capture-session-a",
                    "device_type": "GOOSE"
                },
                {
                    "evidence_id": "capture-b-command",
                    "frame_id": "capture-b-command.frame.0",
                    "source": "ios.corebluetooth.notification",
                    "captured_at": "2026-05-28T12:00:01Z",
                    "device_model": "WHOOP 5.0 Goose",
                    "frame_hex": GET_HELLO_FRAME,
                    "sensitivity": "user-owned-capture",
                    "capture_session_id": "capture-session-b",
                    "device_type": "GOOSE"
                }
            ]
        }
    }));
    assert!(import_response.ok, "{:?}", import_response.error);
    assert_eq!(import_response.result.unwrap()["frames_inserted"], 2);

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-capture-filtered-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db_path,
            "output_dir": export_dir,
            "start": "2026-05-28T00:00:00Z",
            "end": "2026-05-29T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "data_families": ["raw_evidence", "decoded_frames", "packet_timeline"],
            "capture_session_ids": [" capture-session-a ", "capture-session-a"]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["raw_rows"], 1);
    assert_eq!(result["decoded_frame_rows"], 1);
    assert_eq!(result["packet_timeline_rows"], 1);
    assert_eq!(
        result["manifest"]["filters"]["capture_session_ids"][0],
        "capture-session-a"
    );

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 1);
    assert_eq!(validation.content.decoded_frame_rows, 1);
    assert_eq!(validation.content.packet_timeline_rows, 1);
}

#[test]
fn bridge_raw_export_honors_packet_and_sensor_filters() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let export_dir = tempdir
        .path()
        .join("packet-signal-filtered-export.goosebundle");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-packet-signal-filtered-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db,
            "output_dir": export_dir,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "data_families": ["sensor_samples"],
            "packet_type_names": [" REALTIME_RAW_DATA ", "REALTIME_RAW_DATA"],
            "sensor_source_signals": ["raw_motion_k10", "raw_motion_k10"]
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert!(result["sensor_sample_rows"].as_u64().unwrap() > 0);
    assert_eq!(
        result["manifest"]["filters"]["packet_type_names"][0],
        "REALTIME_RAW_DATA"
    );
    assert_eq!(
        result["manifest"]["filters"]["sensor_source_signals"][0],
        "raw_motion_k10"
    );

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert!(validation.content.sensor_sample_rows > 0);
}

#[test]
fn bridge_raw_export_can_omit_raw_bytes() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_fixture_database(&db);
    let export_dir = tempdir.path().join("hash-only-export.goosebundle");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "export-hash-only-1",
        "method": "export.raw_timeframe",
        "args": {
            "database_path": db,
            "output_dir": export_dir,
            "start": "2026-05-01T00:00:00Z",
            "end": "2026-05-28T00:00:00Z",
            "app_version": "goose-app/bridge-test",
            "core_version": "goose-core/bridge-test",
            "data_families": ["raw_evidence", "decoded_frames"],
            "include_raw_bytes": false
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["manifest"]["filters"]["include_raw_bytes"], false);

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 8);
    assert_eq!(validation.content.decoded_frame_rows, 8);
}

#[test]
fn bridge_scaffolds_local_health_validation_manifest_from_database() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "bridge-walk-capture-session",
            source: "synthetic.bridge.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true}"#,
        })
        .unwrap();
    for (evidence_id, captured_at, packet_k, domain, sequence) in [
        (
            "bridge-raw-walk-k10",
            "2026-06-02T10:00:30Z",
            10,
            "raw_motion_stream_result",
            10,
        ),
        (
            "bridge-raw-walk-k11",
            "2026-06-02T10:04:30Z",
            11,
            "raw_stream_counted",
            11,
        ),
    ] {
        let payload = [packet_k as u8, sequence as u8];
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.bridge.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &payload,
                sensitivity: "public-test-fixture",
                capture_session_id: Some("bridge-walk-capture-session"),
            })
            .unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                r#"
                INSERT INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, ?3, 'DATA', ?4, NULL, ?5, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(packet_k),
                    i64::from(sequence),
                    serde_json::json!({
                        "packet_k": packet_k,
                        "domain": domain,
                        "body_summary": {
                            "kind": domain
                        }
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "local-health-validation-scaffold-1",
        "method": "validation.local_health_manifest_scaffold",
        "args": {
            "database_path": db.display().to_string(),
            "manifest_id": "bridge-walk-capture-scaffold",
            "timezone": "Europe/London"
        }
    }));

    assert!(response.ok, "{:?}", response.error);
    let manifest = response.result.unwrap();
    assert_eq!(
        manifest["schema"],
        "goose.local-health-validation-manifest.v1"
    );
    assert_eq!(manifest["manifest_id"], "bridge-walk-capture-scaffold");
    assert_eq!(manifest["start"], "2026-06-02T10:00:30Z");
    assert_eq!(manifest["end"], "2026-06-02T10:04:31Z");
    assert_eq!(manifest["date_key"], "2026-06-02");
    assert_eq!(manifest["timezone"], "Europe/London");
    assert_eq!(
        manifest["capture_session_id"],
        "bridge-walk-capture-session"
    );
    assert_eq!(
        manifest["generated_evidence"]["database_source_kind"],
        "direct_database"
    );
    assert_eq!(
        manifest["generated_evidence"]["window_source"],
        "raw_evidence_bounds"
    );
    assert_eq!(
        manifest["generated_evidence"]["capture_session_default"],
        "single_session_defaulted"
    );
    assert_eq!(
        manifest["generated_evidence"]["packet_family_counts"]["K10/raw_motion_stream_result"],
        1
    );
    assert_eq!(
        manifest["generated_evidence"]["packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    let case_ids = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for expected in [
        "owned-step-discovery",
        "owned-step-validation",
        "owned-raw-motion-steps",
        "owned-energy-rollup",
        "owned-energy-validation",
    ] {
        assert!(
            case_ids.contains(expected),
            "bridge scaffold missing case {expected}"
        );
    }
    let step_validation = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "owned-step-validation")
        .unwrap();
    assert!(step_validation["manual_step_delta"].is_null());
    assert!(step_validation["official_whoop_step_delta"].is_null());
    assert_eq!(manifest["run_validation"]["args"][1], "--database");
    assert_eq!(
        manifest["run_validation"]["args"][2],
        db.display().to_string()
    );
    assert!(
        manifest["operator_checklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "bind_capture_sessions"
                && item["status"] == "single_capture_session_defaulted")
    );
    assert_eq!(
        manifest["label_provenance"]["official_labels_are_labels"],
        true
    );

    let runbook_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "local-health-validation-runbook-1",
        "method": "validation.local_health_manifest_runbook",
        "args": {
            "manifest": manifest
        }
    }));

    assert!(runbook_response.ok, "{:?}", runbook_response.error);
    let runbook = runbook_response.result.unwrap();
    assert_eq!(
        runbook["schema"],
        "goose.local-health-validation-runbook.v1"
    );
    assert_eq!(
        runbook["manifest_schema"],
        "goose.local-health-validation-manifest.v1"
    );
    assert_eq!(
        runbook["json_report_path"],
        "local-health-validation-report.json"
    );
    assert_eq!(
        runbook["markdown_report_path"],
        "local-health-validation-report.md"
    );
    let markdown = runbook["markdown"].as_str().unwrap();
    assert!(markdown.contains("# Local Health Validation Runbook"));
    assert!(markdown.contains("bridge-walk-capture-scaffold"));
    assert!(markdown.contains("goose-local-health-validation-suite"));
    assert!(markdown.contains("bridge-walk-capture-session"));
    assert!(markdown.contains("K10/raw_motion_stream_result"));
    assert!(markdown.contains("official_whoop_step_delta"));
    assert!(markdown.contains("validation labels only"));

    let review_response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "local-health-validation-review-1",
        "method": "validation.local_health_manifest_review",
        "args": {
            "manifest": manifest
        }
    }));

    assert!(review_response.ok, "{:?}", review_response.error);
    let review = review_response.result.unwrap();
    assert_eq!(
        review["schema"],
        "goose.local-health-validation-manifest-review.v1"
    );
    assert_eq!(review["manifest_id"], "bridge-walk-capture-scaffold");
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["schema_valid"], true);
    assert_eq!(review["label_policy_valid"], true);
    assert_eq!(review["placeholder_field_count"].as_u64().unwrap(), 15);
    assert!(
        review["placeholder_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "official_whoop_step_delta")
    );
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["generated_command_writes_json"], true);
    assert_eq!(review["generated_command_writes_markdown"], true);
    assert_eq!(review["generated_command_writes_review"], true);
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "validation_placeholders_unfilled")
    );
}

#[test]
fn bridge_dry_runs_health_sync_policy_for_app_sync_screen() {
    let args: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/synthetic/health_sync_dry_run_healthkit.json"
    ))
    .unwrap();

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "health-sync-1",
        "method": "health_sync.dry_run",
        "args": args
    }));

    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["pass"], true);
    assert_eq!(result["input_valid"], true);
    assert_eq!(result["all_candidate_writes_planned"], false);
    assert_eq!(result["all_requested_deletes_planned"], true);
    assert_eq!(result["all_records_ready"], false);
    assert_eq!(result["permissions_ready"], true);
    assert_eq!(result["mappings_ready"], false);
    assert_eq!(result["units_ready"], true);
    assert_eq!(result["provenance_ready"], true);
    assert_eq!(result["source_policy_ready"], false);
    assert_eq!(result["idempotency_ready"], true);
    assert_eq!(result["cleanup_scope_ready"], true);
    assert_eq!(result["planned_write_count"], 2);
    assert_eq!(result["blocked_count"], 2);
    assert_eq!(result["delete_policy"], "none");
    assert_eq!(result["planned_delete_count"], 0);
    assert_eq!(result["blocked_delete_count"], 0);
    let blocked_records = result["blocked_records"].as_array().unwrap();
    assert!(blocked_records.iter().any(|record| {
        record["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "healthkit_rmssd_must_not_be_written_as_sdnn")
    }));
    assert!(blocked_records.iter().any(|record| {
        record["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["action"]
                    .as_str()
                    .unwrap()
                    .contains("Do not write RMSSD to HealthKit SDNN")
            })
    }));
    assert!(blocked_records.iter().any(|record| {
        record["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark_only_algorithm_not_syncable")
    }));
    assert!(
        result["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["reason"] == "benchmark_only_algorithm_not_syncable")
    );
}

#[test]
fn bridge_errors_are_structured_for_bad_input() {
    let invalid_json = handle_bridge_request_json("{");
    let response: BridgeResponse = serde_json::from_str(&invalid_json).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "invalid_json");

    let response = request(serde_json::json!({
        "schema": "goose.bridge.request.v1",
        "request_id": "bad-method",
        "method": "unknown.method",
        "args": {}
    }));
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "unknown_method");
}

#[test]
fn c_abi_bridge_roundtrips_json_and_allows_freeing_results() {
    let version_ptr = goose_core_version_json();
    assert!(!version_ptr.is_null());
    let version = unsafe { CStr::from_ptr(version_ptr) }
        .to_str()
        .unwrap()
        .to_string();
    assert!(version.contains("bridge_request_schema"));
    unsafe { goose_bridge_free_string(version_ptr) };

    let request = CString::new(
        serde_json::json!({
            "schema": "goose.bridge.request.v1",
            "request_id": "ffi-parse-1",
            "method": "protocol.parse_frame_hex",
            "args": {"frame_hex": GET_HELLO_FRAME}
        })
        .to_string(),
    )
    .unwrap();
    let response_ptr = unsafe { goose_bridge_handle_json(request.as_ptr()) };
    assert!(!response_ptr.is_null());
    let response_json = unsafe { CStr::from_ptr(response_ptr) }.to_str().unwrap();
    let response: BridgeResponse = serde_json::from_str(response_json).unwrap();
    assert!(response.ok, "{:?}", response.error);
    unsafe { goose_bridge_free_string(response_ptr) };

    let null_response_ptr = unsafe { goose_bridge_handle_json(std::ptr::null()) };
    assert!(!null_response_ptr.is_null());
    let null_response_json = unsafe { CStr::from_ptr(null_response_ptr) }
        .to_str()
        .unwrap();
    let null_response: BridgeResponse = serde_json::from_str(null_response_json).unwrap();
    assert!(!null_response.ok);
    assert_eq!(null_response.error.unwrap().code, "null_request");
    unsafe { goose_bridge_free_string(null_response_ptr) };
}

fn request(value: serde_json::Value) -> BridgeResponse {
    serde_json::from_str(&handle_bridge_request_json(&value.to_string())).unwrap()
}

fn seed_recovery_calibration(db: &std::path::Path) {
    let store = GooseStore::open(db).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    let dataset: CalibrationDataset = serde_json::from_str(include_str!(
        "../fixtures/synthetic/recovery_calibration_linear.json"
    ))
    .unwrap();
    let report = evaluate_linear_calibration(
        &dataset,
        &CalibrationOptions {
            metric_family: "recovery".to_string(),
            algorithm_id: "goose.recovery.v0".to_string(),
            algorithm_version: "0.1.0".to_string(),
            split_at: "2026-05-04T00:00:00Z".to_string(),
            min_train_rows: 2,
            min_holdout_rows: 1,
        },
    );
    assert!(report.pass);
    let record = calibration_run_record("calibration-run-1", &report).unwrap();
    assert!(store.insert_calibration_run(&record).unwrap());
}

fn seed_stored_recovery_calibration_inputs(db: &std::path::Path) {
    let store = GooseStore::open(db).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    for (index, (prediction, label)) in [
        (40.0, 43.0),
        (50.0, 55.0),
        (60.0, 67.0),
        (70.0, 79.0),
        (80.0, 91.0),
    ]
    .into_iter()
    .enumerate()
    {
        let day = index + 1;
        let run_id = format!("stored-recovery-run-{day}");
        let start_time = format!("2026-05-{day:02}T00:00:00Z");
        let end_time = format!("2026-05-{day:02}T23:59:00Z");
        let output_json = serde_json::json!({
            "algorithm_id": "goose.recovery.v0",
            "algorithm_version": "0.1.0",
            "score_0_to_100": prediction,
            "components": []
        })
        .to_string();
        assert!(
            store
                .insert_algorithm_run(&AlgorithmRunRecord {
                    run_id: run_id.clone(),
                    algorithm_id: "goose.recovery.v0".to_string(),
                    version: "0.1.0".to_string(),
                    start_time: start_time.clone(),
                    end_time,
                    output_json,
                    quality_flags_json: "[]".to_string(),
                    provenance_json: serde_json::json!({
                        "input_ids": [format!("stored-recovery-input-{day}")]
                    })
                    .to_string(),
                })
                .unwrap()
        );
        let provenance_json = serde_json::json!({
            "entry": "typed_by_user",
            "algorithm_run_id": run_id,
            "session_id": format!("stored-recovery-session-{day}"),
            "official_labels_are_labels": true
        })
        .to_string();
        assert!(
            store
                .insert_calibration_label(CalibrationLabelInput {
                    label_id: &format!("manual.recovery.2026-05-{day:02}"),
                    metric_family: "recovery",
                    label_source: "manual",
                    captured_at: &format!("2026-05-{day:02}T12:00:00Z"),
                    value: label,
                    unit: "score_0_to_100",
                    provenance_json: &provenance_json,
                })
                .unwrap()
        );
    }
}

fn seed_stored_sleep_v1_calibration_inputs(db: &std::path::Path) {
    let store = GooseStore::open(db).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    for (index, (prediction, label)) in [
        (50.0, 55.0),
        (55.0, 61.0),
        (60.0, 67.0),
        (65.0, 73.0),
        (70.0, 79.0),
        (75.0, 85.0),
    ]
    .into_iter()
    .enumerate()
    {
        let day = index + 1;
        let run_id = format!("stored-sleep-v1-run-{day}");
        let start_time = format!("2026-05-{day:02}T22:00:00Z");
        let end_time = format!("2026-05-{:02}T06:00:00Z", day + 1);
        let output_json = serde_json::json!({
            "algorithm_id": "goose.sleep.v1",
            "algorithm_version": "0.1.0",
            "score_0_to_100": prediction,
            "model_status": "baseline_ready",
            "status_report": {
                "report_state": "final"
            }
        })
        .to_string();
        assert!(
            store
                .insert_algorithm_run(&AlgorithmRunRecord {
                    run_id: run_id.clone(),
                    algorithm_id: "goose.sleep.v1".to_string(),
                    version: "0.1.0".to_string(),
                    start_time: start_time.clone(),
                    end_time,
                    output_json,
                    quality_flags_json: "[]".to_string(),
                    provenance_json: serde_json::json!({
                        "input_ids": [format!("stored-sleep-v1-input-{day}")]
                    })
                    .to_string(),
                })
                .unwrap()
        );
        let provenance_json = serde_json::json!({
            "entry": "typed_by_user",
            "algorithm_run_id": run_id,
            "session_id": format!("stored-sleep-v1-session-{day}"),
            "official_labels_are_labels": true
        })
        .to_string();
        assert!(
            store
                .insert_calibration_label(CalibrationLabelInput {
                    label_id: &format!("manual.sleep-v1.2026-05-{day:02}"),
                    metric_family: "sleep",
                    label_source: "manual",
                    captured_at: &format!("2026-05-{day:02}T23:00:00Z"),
                    value: label,
                    unit: "score_0_to_100",
                    provenance_json: &provenance_json,
                })
                .unwrap()
        );
    }
}

fn seed_fixture_database(db: &Path) {
    let store = GooseStore::open(db).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: db,
            parser_version: "goose-core/bridge-test",
        },
    );
    assert!(report.pass, "{:?}", report.issues);
}

fn k10_motion_frame_hex() -> String {
    k10_motion_frame_hex_with_value(1000)
}

fn k10_motion_frame_hex_with_value(sample_value: i16) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    for offset in [85, 285, 485, 688, 888, 1088] {
        for index in 0..100 {
            put_i16(&mut payload, offset + index * 2, sample_value);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn k10_motion_step_frame_hex(peak_indices: &[usize]) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 84;
    for offset in [85, 285, 485] {
        for index in peak_indices {
            put_i16(&mut payload, offset + index * 2, 4_000);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn historical_k18_frame_hex(marker_value: u8) -> String {
    let mut payload = vec![
        PACKET_TYPE_HISTORICAL_DATA,
        18,
        1,
        0x04,
        0x03,
        0x02,
        0x01,
        0x44,
        0x33,
        0x22,
        0x11,
        0x66,
        0x55,
        0xaa,
        marker_value,
        0xbb,
        0xcc,
        0xdd,
        0xee,
        0xff,
    ];
    payload.resize(24, 0);
    hex::encode(build_v5_payload_frame(&payload))
}

fn temperature_event_frame_hex(body: &[u8]) -> String {
    let mut payload = vec![
        PACKET_TYPE_EVENT,
        2,
        17,
        0,
        0x04,
        0x03,
        0x02,
        0x01,
        0x06,
        0x05,
        0,
        0,
    ];
    payload.extend_from_slice(body);
    hex::encode(build_v5_payload_frame(&payload))
}

fn historical_k18_frame_hex_with_vital_candidates(
    marker_value: u8,
    temperature_centi_c: i16,
    respiratory_rate_tenths_rpm: u16,
) -> String {
    let mut payload = vec![
        PACKET_TYPE_HISTORICAL_DATA,
        18,
        1,
        0x04,
        0x03,
        0x02,
        0x01,
        0x44,
        0x33,
        0x22,
        0x11,
        0x66,
        0x55,
        0xaa,
        marker_value,
        0xbb,
        0xcc,
        0xdd,
        0xee,
        0xff,
    ];
    payload.resize(41, 0);
    put_i16(&mut payload, 37, temperature_centi_c);
    put_u16(&mut payload, 39, respiratory_rate_tenths_rpm);
    hex::encode(build_v5_payload_frame(&payload))
}

fn r17_frame_hex(rr_candidates: &[i16]) -> String {
    let mut payload = vec![0; 26 + rr_candidates.len() * 2];
    payload[0] = PACKET_TYPE_HISTORICAL_DATA;
    payload[1] = 17;
    payload[2] = 1;
    put_u16(&mut payload, 13, (1 << 9) | (1 << 11));
    payload[15..=20].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    put_u16(&mut payload, 24, rr_candidates.len() as u16);
    for (index, value) in rr_candidates.iter().enumerate() {
        put_i16(&mut payload, 26 + index * 2, *value);
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
