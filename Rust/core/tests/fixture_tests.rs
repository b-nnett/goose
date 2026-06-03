use std::{collections::BTreeMap, fs, path::Path};

use goose_core::activity_candidates::{
    ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA, ACTIVITY_CANDIDATE_CLASSIFIER_REPORT_SCHEMA,
    ActivityCandidateClassifierInput, ActivityCandidateState, run_activity_candidate_classifier,
};
use goose_core::fixtures::{
    ACTIVITY_SESSION_FIXTURE_SCHEMA, CAPTURED_FRAME_BATCH_SCHEMA,
    COMMAND_VALIDATION_FIXTURE_SCHEMA, FRAME_HEX_SCHEMA, OPENWHOOP_REFERENCE_FIXTURE_SCHEMA,
    PAYLOAD_HEX_SCHEMA, build_fixture_index, run_parser_fixtures,
};
use goose_core::health_sync::{
    ActivityHealthSyncDryRunInput, HealthPlatform, run_activity_health_sync_dry_run,
};
use goose_core::historical_sync::{
    HistoricalSyncAckDisposition, HistoricalSyncDryRunInput, HistoricalSyncFakeEvent,
    HistoricalSyncGeneration, HistoricalSyncPayloadExpectation, HistoricalSyncPlanStep,
    HistoricalSyncPlanStepKind, HistoricalSyncState, run_historical_sync_dry_run,
};
use goose_core::openwhoop_reference::{
    OPENWHOOP_REFERENCE_COMMIT, OPENWHOOP_REFERENCE_LICENSE_CAVEAT, WhoopGeneration,
    whoop_generation_from_service_uuid, whoop_generation_reference,
};
use serde::Deserialize;
use serde_json::json;

#[test]
fn indexes_synthetic_fixture_with_required_metadata_and_checksum() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);
    assert!(index.fixtures.len() >= 3);
    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.get_hello_frame")
        .unwrap();
    assert_eq!(fixture.id, "synthetic.goose.v5.get_hello_frame");
    assert_eq!(fixture.schema, FRAME_HEX_SCHEMA);
    assert_eq!(fixture.checksum_sha256.len(), 64);
    assert!(fixture.byte_len == 33 || fixture.byte_len == 34);

    let batch = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.sanitized.corebluetooth.frame_batch")
        .unwrap();
    assert_eq!(batch.schema, CAPTURED_FRAME_BATCH_SCHEMA);
    assert_eq!(batch.kind, "sanitized_capture");
}

#[test]
fn indexes_pre_device_activity_session_fixture_with_generic_corpus_shape() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.activity.sessions.pre_device.hand_derived")
        .unwrap();
    assert_eq!(fixture.schema, ACTIVITY_SESSION_FIXTURE_SCHEMA);
    assert_eq!(fixture.kind, "synthetic");
    assert_eq!(
        fixture.expected.as_ref().unwrap()["session_count"].as_u64(),
        Some(7)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["metric_count"].as_u64(),
        Some(23)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["interval_count"].as_u64(),
        Some(11)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["label_count"].as_u64(),
        Some(7)
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let corpus: ActivityFixtureSet = serde_json::from_str(&raw).unwrap();

    assert_eq!(corpus.schema, ACTIVITY_SESSION_FIXTURE_SCHEMA);
    assert_eq!(corpus.activity_sessions.len(), 7);
    assert_eq!(corpus.activity_metrics.len(), 23);
    assert_eq!(corpus.activity_intervals.len(), 11);
    assert_eq!(corpus.activity_labels.len(), 7);

    let sessions_by_id: BTreeMap<_, _> = corpus
        .activity_sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect();

    let unknown = sessions_by_id["synthetic.activity.unknown.session"];
    assert_eq!(unknown.activity_type, "unknown");
    assert_eq!(unknown.sync_status, "candidate");
    assert_eq!(
        unknown.duration_ms,
        unknown.end_time_unix_ms - unknown.start_time_unix_ms
    );
    assert!(session_metrics(&corpus, unknown.session_id.as_str()).is_empty());
    assert!(session_intervals(&corpus, unknown.session_id.as_str()).is_empty());
    assert_eq!(
        session_labels(&corpus, unknown.session_id.as_str()).len(),
        1
    );
    assert_provenance_shape(&unknown.provenance_json, "unknown");

    let run_like = sessions_by_id["synthetic.activity.run_like.session"];
    assert_eq!(run_like.activity_type, "running");
    assert_eq!(run_like.external_activity_type_code.as_deref(), Some("run"));
    assert_eq!(run_like.sync_status, "user_confirmed");
    let run_like_metrics = session_metrics(&corpus, run_like.session_id.as_str());
    assert!(
        run_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "heart_rate")
    );
    assert!(
        run_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "distance")
    );
    assert!(
        run_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "speed")
    );
    assert!(
        session_intervals(&corpus, run_like.session_id.as_str())
            .iter()
            .any(|interval| interval.interval_type == "lap")
    );
    assert_eq!(
        session_labels(&corpus, run_like.session_id.as_str())[0].label_type,
        "user"
    );
    assert_provenance_shape(&run_like.provenance_json, "run_like");

    let ride_like = sessions_by_id["synthetic.activity.ride_like.session"];
    assert_eq!(ride_like.activity_type, "cycling");
    assert_eq!(
        ride_like.external_activity_type_code.as_deref(),
        Some("ride")
    );
    assert_eq!(ride_like.sync_status, "verified");
    let ride_like_metrics = session_metrics(&corpus, ride_like.session_id.as_str());
    assert!(
        ride_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "heart_rate")
    );
    assert!(
        ride_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "distance")
    );
    assert!(
        ride_like_metrics
            .iter()
            .any(|metric| metric.metric_name == "power")
    );
    assert!(
        session_intervals(&corpus, ride_like.session_id.as_str())
            .iter()
            .any(|interval| interval.interval_type == "split")
    );
    assert_eq!(
        session_labels(&corpus, ride_like.session_id.as_str())[0].label_type,
        "official_app_comparison"
    );
    assert_provenance_shape(&ride_like.provenance_json, "ride_like");

    let strength_like = sessions_by_id["synthetic.activity.strength_like.session"];
    assert_eq!(strength_like.activity_type, "strength");
    assert_eq!(strength_like.detection_method, "manual_annotation");
    assert_eq!(strength_like.sync_status, "candidate");
    let strength_intervals = session_intervals(&corpus, strength_like.session_id.as_str());
    assert!(
        strength_intervals
            .iter()
            .any(|interval| interval.interval_type == "work")
    );
    assert!(
        strength_intervals
            .iter()
            .any(|interval| interval.interval_type == "rest")
    );
    assert_provenance_shape(&strength_like.provenance_json, "strength_like");

    let hr_only = sessions_by_id["synthetic.activity.hr_only_no_distance.session"];
    assert_eq!(hr_only.activity_type, "other");
    assert_eq!(hr_only.sync_status, "user_confirmed");
    let hr_only_metrics = session_metrics(&corpus, hr_only.session_id.as_str());
    assert!(
        hr_only_metrics
            .iter()
            .any(|metric| metric.metric_name == "heart_rate")
    );
    assert!(
        !hr_only_metrics
            .iter()
            .any(|metric| metric.metric_name == "distance")
    );
    assert_provenance_shape(&hr_only.provenance_json, "hr_only_no_distance");

    let no_hr = sessions_by_id["synthetic.activity.no_hr.session"];
    assert_eq!(no_hr.activity_type, "walking");
    assert_eq!(no_hr.sync_status, "candidate");
    let no_hr_metrics = session_metrics(&corpus, no_hr.session_id.as_str());
    assert!(
        no_hr_metrics
            .iter()
            .any(|metric| metric.metric_name == "distance")
    );
    assert!(
        no_hr_metrics
            .iter()
            .any(|metric| metric.metric_name == "steps")
    );
    assert!(
        !no_hr_metrics
            .iter()
            .any(|metric| metric.metric_name == "heart_rate")
    );
    assert_provenance_shape(&no_hr.provenance_json, "no_hr");

    let multi_interval = sessions_by_id["synthetic.activity.multi_interval.session"];
    assert_eq!(multi_interval.activity_type, "hiit");
    assert_eq!(multi_interval.detection_method, "manual_split");
    assert_eq!(multi_interval.sync_status, "verified");
    let multi_intervals = session_intervals(&corpus, multi_interval.session_id.as_str());
    assert_eq!(multi_intervals.len(), 5);
    assert!(
        multi_intervals
            .iter()
            .any(|interval| interval.interval_type == "window")
    );
    assert!(
        multi_intervals
            .iter()
            .any(|interval| interval.interval_type == "work")
    );
    assert!(
        multi_intervals
            .iter()
            .any(|interval| interval.interval_type == "rest")
    );
    assert_provenance_shape(&multi_interval.provenance_json, "multi_interval");

    assert!(
        corpus
            .activity_labels
            .iter()
            .all(|label| label.label_type == "candidate"
                || label.label_type == "user"
                || label.label_type == "official_app_comparison"
                || label.label_type == "calibration")
    );
}

#[test]
fn indexes_openwhoop_activity_health_sync_comparison_fixture_with_planned_session_fields() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.openwhoop.activity_health_sync_comparison")
        .unwrap();
    assert_eq!(fixture.schema, "goose.activity-health-sync-dry-run.v1");
    assert_eq!(fixture.kind, "synthetic");
    assert!(fixture.source.contains(OPENWHOOP_REFERENCE_COMMIT));
    assert!(fixture.notes.contains(OPENWHOOP_REFERENCE_LICENSE_CAVEAT));

    let expected = fixture.expected.as_ref().unwrap();
    assert_eq!(expected["session_count"].as_u64(), Some(3));
    assert_eq!(expected["planned_session_count"].as_u64(), Some(3));
    assert_eq!(expected["blocked_session_count"].as_u64(), Some(0));
    assert_eq!(expected["permission_grants"], json!(["HKWorkout"]));

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let input: ActivityHealthSyncDryRunInput = serde_json::from_str(&raw).unwrap();
    let report = run_activity_health_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert_eq!(report.platform, HealthPlatform::HealthKit);
    assert_eq!(report.session_count, 3);
    assert_eq!(report.planned_session_count, 3);
    assert_eq!(report.blocked_session_count, 0);
    assert_eq!(report.permission_grants, vec!["HKWorkout".to_string()]);
    assert_eq!(
        report
            .planned_sessions
            .iter()
            .map(planned_activity_session_subset)
            .collect::<Vec<_>>(),
        expected["planned_sessions"].as_array().cloned().unwrap()
    );
}

#[test]
fn indexes_gen5_historical_sync_command_validation_fixture_with_sequence_and_row_metadata() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| {
            fixture.id == "synthetic.goose.v5.historical_sync_gen5_dry_run_command_validation"
        })
        .unwrap();
    assert_eq!(fixture.schema, COMMAND_VALIDATION_FIXTURE_SCHEMA);
    assert_eq!(fixture.kind, "synthetic");
    assert_eq!(
        fixture.expected.as_ref().unwrap()["generation"].as_str(),
        Some("gen5")
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["request_data_range"].as_bool(),
        Some(true)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["command_count"].as_u64(),
        Some(4)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["direct_send_ready_count"].as_u64(),
        Some(4)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["blocked_count"].as_u64(),
        Some(0)
    );
    let expected_sequence = fixture.expected.as_ref().unwrap()["command_sequence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        expected_sequence,
        vec![
            "get_data_range",
            "send_historical_data",
            "historical_data_result",
            "abort_historical_transmits",
        ]
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let corpus: HistoricalSyncCommandValidationFixture = serde_json::from_str(&raw).unwrap();

    assert_eq!(corpus.schema, COMMAND_VALIDATION_FIXTURE_SCHEMA);
    assert_eq!(corpus.generation, "gen5");
    assert!(corpus.request_data_range);
    assert_eq!(corpus.command_sequence, expected_sequence);
    assert_eq!(corpus.command_validation_rows.len(), 4);

    let expected_rows = [
        (
            "get_data_range",
            34,
            "read_only",
            "Read available historical data range.",
        ),
        (
            "send_historical_data",
            22,
            "user_visible_state_change",
            "Request historical data transfer.",
        ),
        (
            "historical_data_result",
            23,
            "user_visible_state_change",
            "Send historical data transfer result or acknowledgement.",
        ),
        (
            "abort_historical_transmits",
            20,
            "user_visible_state_change",
            "Abort active historical transmissions.",
        ),
    ];

    for (row, &(expected_command, expected_number, expected_risk_gate, expected_description)) in
        corpus
            .command_validation_rows
            .iter()
            .zip(expected_rows.iter())
    {
        let expected_triggering_ui_action =
            format!("official app emulator capture > {}", expected_command);
        assert_eq!(row.command, expected_command);
        assert_eq!(row.command_number, Some(expected_number));
        assert_eq!(row.family, "historical_sync");
        assert_eq!(row.risk_gate, expected_risk_gate);
        assert!(row.direct_send_ready);
        assert!(row.missing_requirements.is_empty());
        assert!(row.warnings.is_empty());
        assert!(row.next_capture_actions.is_empty());
        assert_eq!(
            row.validated_service_uuid.as_deref(),
            Some("fd4b0001-cce1-4033-93ce-002d5875f58a")
        );
        assert_eq!(
            row.validated_characteristic_uuid.as_deref(),
            Some("fd4b0002-cce1-4033-93ce-002d5875f58a")
        );
        assert_eq!(row.validated_write_type.as_deref(), Some("with_response"));
        assert_eq!(
            row.validated_evidence_source.as_deref(),
            Some("official_app_capture")
        );
        assert_eq!(
            row.validated_capture_kind.as_deref(),
            Some("official_app_to_macos_emulator")
        );
        assert_eq!(row.validated_owner.as_deref(), Some("user"));
        assert_eq!(
            row.validated_provenance_json.as_deref(),
            Some(
                r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user"}"#
            )
        );
        assert_eq!(
            row.validated_triggering_ui_action.as_deref(),
            Some(expected_triggering_ui_action.as_str())
        );
        assert_eq!(row.report_json["command"].as_str(), Some(expected_command));
        assert_eq!(
            row.report_json["command_number"].as_u64(),
            Some(expected_number as u64)
        );
        assert_eq!(row.report_json["family"].as_str(), Some("historical_sync"));
        assert_eq!(
            row.report_json["risk_gate"].as_str(),
            Some(expected_risk_gate)
        );
        assert_eq!(
            row.report_json["description"].as_str(),
            Some(expected_description)
        );
        assert_eq!(row.report_json["direct_send_ready"].as_bool(), Some(true));
        assert_eq!(
            row.report_json["missing_requirements"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(row.report_json["warnings"].as_array().unwrap().len(), 0);
        assert_eq!(
            row.report_json["next_capture_actions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            row.report_json["validated_service_uuid"].as_str(),
            Some("fd4b0001-cce1-4033-93ce-002d5875f58a")
        );
        assert_eq!(
            row.report_json["validated_characteristic_uuid"].as_str(),
            Some("fd4b0002-cce1-4033-93ce-002d5875f58a")
        );
        assert_eq!(
            row.report_json["validated_write_type"].as_str(),
            Some("with_response")
        );
        assert_eq!(
            row.report_json["validated_evidence_source"].as_str(),
            Some("official_app_capture")
        );
        assert_eq!(
            row.report_json["validated_capture_kind"].as_str(),
            Some("official_app_to_macos_emulator")
        );
        assert_eq!(row.report_json["validated_owner"].as_str(), Some("user"));
        assert_eq!(
            row.report_json["validated_provenance_json"].as_str(),
            Some(
                r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user"}"#
            )
        );
        assert_eq!(
            row.report_json["validated_triggering_ui_action"].as_str(),
            Some(expected_triggering_ui_action.as_str())
        );
    }
}

#[test]
fn indexes_packet_derived_activity_candidate_fixture_with_provenance_and_reasons() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.activity.candidate.packet_derived_windows")
        .unwrap();
    assert_eq!(fixture.schema, ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA);
    assert_eq!(fixture.kind, "synthetic");
    assert_eq!(
        fixture.expected.as_ref().unwrap()["window_count"].as_u64(),
        Some(2)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["candidate_window_count"].as_u64(),
        Some(0)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["unknown_window_count"].as_u64(),
        Some(2)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["blocked_window_count"].as_u64(),
        Some(0)
    );
    assert_eq!(
        fixture.expected.as_ref().unwrap()["window_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let input: ActivityCandidateClassifierInput = serde_json::from_str(&raw).unwrap();
    let report = run_activity_candidate_classifier(&input);

    assert_eq!(report.schema, ACTIVITY_CANDIDATE_CLASSIFIER_REPORT_SCHEMA);
    assert!(!report.pass);
    assert!(report.issues.is_empty());
    assert_eq!(report.window_count, 2);
    assert_eq!(report.candidate_window_count, 0);
    assert_eq!(report.unknown_window_count, 2);
    assert_eq!(report.blocked_window_count, 0);

    let hr_only = report
        .windows
        .iter()
        .find(|window| window.window_id == "synthetic.activity.packet_derived.hr_only.window")
        .unwrap();
    assert_eq!(hr_only.state, ActivityCandidateState::Unknown);
    assert!(
        hr_only
            .blocker_reasons
            .contains(&"missing_motion".to_string())
    );
    assert!(
        hr_only
            .blocker_reasons
            .contains(&"missing_command_sync".to_string())
    );
    assert!(
        hr_only
            .readiness_reasons
            .contains(&"heart_rate_present".to_string())
    );
    assert!(
        hr_only
            .provenance
            .get("heart_rate_provenance")
            .and_then(serde_json::Value::as_object)
            .and_then(|value| value.get("details"))
            .and_then(serde_json::Value::as_object)
            .and_then(|value| value.get("fixture_id"))
            .and_then(serde_json::Value::as_str)
            == Some("synthetic.goose.v5.historical_k18_packet")
    );
    assert!(hr_only.provenance["motion_provenance"].is_null());
    assert!(hr_only.provenance["command_sync_provenance"].is_null());
    assert!(hr_only.next_actions.iter().any(|action| {
        action.reason == "missing_motion"
            && action
                .action
                .contains("motion evidence with gravity samples")
    }));
    assert!(hr_only.next_actions.iter().any(|action| {
        action.reason == "missing_command_sync"
            && action.action.contains("Carry command-sync evidence")
    }));

    let motion_only = report
        .windows
        .iter()
        .find(|window| window.window_id == "synthetic.activity.packet_derived.motion_only.window")
        .unwrap();
    assert_eq!(motion_only.state, ActivityCandidateState::Unknown);
    assert!(
        motion_only
            .blocker_reasons
            .contains(&"missing_heart_rate".to_string())
    );
    assert!(
        motion_only
            .blocker_reasons
            .contains(&"missing_command_sync".to_string())
    );
    assert!(
        motion_only
            .blocker_reasons
            .contains(&"low_confidence".to_string())
    );
    assert!(
        motion_only
            .readiness_reasons
            .contains(&"motion_present".to_string())
    );
    assert!(
        motion_only
            .readiness_reasons
            .iter()
            .any(|reason| reason.starts_with("gravity_stability_reference:"))
    );
    assert_eq!(motion_only.motion_confidence_0_to_1, Some(0.4));
    assert!(motion_only.gravity_stability_0_to_1.unwrap() > 0.80);
    assert!(
        motion_only
            .provenance
            .get("motion_provenance")
            .and_then(serde_json::Value::as_object)
            .and_then(|value| value.get("details"))
            .and_then(serde_json::Value::as_object)
            .and_then(|value| value.get("fixture_id"))
            .and_then(serde_json::Value::as_str)
            == Some("synthetic.goose.v5.k10_motion_summary_short")
    );
    assert!(motion_only.provenance["heart_rate_provenance"].is_null());
    assert!(motion_only.provenance["command_sync_provenance"].is_null());
    assert!(motion_only.next_actions.iter().any(|action| {
        action.reason == "missing_heart_rate"
            && action
                .action
                .contains("Capture trusted heart-rate evidence")
    }));
    assert!(motion_only.next_actions.iter().any(|action| {
        action.reason == "low_confidence"
            && action
                .action
                .contains("confidence score clears the threshold")
    }));
}

#[test]
fn indexes_openwhoop_uuid_fixture_with_generation_detection_and_commit_metadata() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.openwhoop.uuid_detection")
        .unwrap();
    assert_eq!(fixture.schema, OPENWHOOP_REFERENCE_FIXTURE_SCHEMA);
    assert_eq!(fixture.kind, "synthetic");
    assert!(fixture.source.contains(OPENWHOOP_REFERENCE_COMMIT));
    assert!(fixture.notes.contains(OPENWHOOP_REFERENCE_LICENSE_CAVEAT));
    assert_eq!(
        fixture.expected.as_ref().unwrap(),
        &json!({
            "probe_count": 2,
            "probe_labels": [
                "gen4_trimmed_uppercase",
                "gen5_canonical",
            ],
            "detected_generations": ["Gen4", "Gen5"],
            "canonical_service_uuids": [
                whoop_generation_reference(WhoopGeneration::Gen4).service_uuid,
                whoop_generation_reference(WhoopGeneration::Gen5).service_uuid,
            ],
        })
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let corpus: OpenWhoopUuidDetectionFixture = serde_json::from_str(&raw).unwrap();

    assert_eq!(corpus.schema, OPENWHOOP_REFERENCE_FIXTURE_SCHEMA);
    assert_eq!(
        corpus
            .service_uuid_probes
            .iter()
            .map(|probe| probe.label.as_str())
            .collect::<Vec<_>>(),
        vec!["gen4_trimmed_uppercase", "gen5_canonical"]
    );
    assert_eq!(
        corpus
            .service_uuid_probes
            .iter()
            .map(
                |probe| whoop_generation_from_service_uuid(&probe.service_uuid)
                    .unwrap()
                    .as_str()
            )
            .collect::<Vec<_>>(),
        vec!["Gen4", "Gen5"]
    );
    assert_eq!(
        corpus
            .service_uuid_probes
            .iter()
            .map(|probe| probe.service_uuid.trim().to_ascii_lowercase())
            .collect::<Vec<_>>(),
        vec![
            whoop_generation_reference(WhoopGeneration::Gen4)
                .service_uuid
                .to_string(),
            whoop_generation_reference(WhoopGeneration::Gen5)
                .service_uuid
                .to_string(),
        ]
    );
}

#[test]
fn indexes_openwhoop_gen5_history_plan_fixture_with_command_planning() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.openwhoop.gen5.history_plan")
        .unwrap();
    assert_eq!(
        fixture.schema,
        goose_core::historical_sync::HISTORICAL_SYNC_DRY_RUN_SCHEMA
    );
    assert_eq!(fixture.kind, "synthetic");
    assert!(fixture.source.contains(OPENWHOOP_REFERENCE_COMMIT));
    assert!(fixture.notes.contains(OPENWHOOP_REFERENCE_LICENSE_CAVEAT));
    assert_eq!(
        fixture.expected.as_ref().unwrap(),
        &json!({
            "planned_command_count": 3,
            "state": "complete",
            "step_kinds": [
                "connect",
                "get_data_range",
                "send_historical_data",
                "consume_metadata",
                "consume_metadata",
                "consume_reading",
                "consume_metadata",
                "historical_data_result",
                "consume_metadata",
                "consume_metadata",
                "complete",
            ],
            "state_trace": [
                "idle",
                "connected",
                "range_requested",
                "transferring",
                "transferring",
                "transferring",
                "transferring",
                "ack_pending",
                "ack_pending",
                "ack_pending",
                "ack_pending",
                "complete",
            ],
            "command_numbers": [34, 22, 23],
            "command_payload_expectations": [
                "empty",
                "empty",
                "history_end_ack_success",
            ],
        })
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let input: HistoricalSyncDryRunInput = serde_json::from_str(&raw).unwrap();
    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert_eq!(report.generation, HistoricalSyncGeneration::Gen5);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.planned_command_count, 3);
    assert_eq!(report.issues, Vec::<String>::new());
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .map(|step| historical_sync_step_kind_label(step.kind))
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["step_kinds"]
    );
    assert_eq!(
        json!(
            report
                .state_trace
                .iter()
                .copied()
                .map(historical_sync_state_label)
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["state_trace"]
    );
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .filter_map(|step| step.command_number)
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["command_numbers"]
    );
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .filter_map(|step| {
                    step.payload_expectation
                        .map(historical_sync_payload_expectation_label)
                })
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["command_payload_expectations"]
    );
    assert_eq!(
        historical_sync_step(&report, HistoricalSyncPlanStepKind::GetDataRange).payload_expectation,
        Some(HistoricalSyncPayloadExpectation::Empty)
    );
    assert_eq!(
        historical_sync_step(&report, HistoricalSyncPlanStepKind::SendHistoricalData)
            .payload_expectation,
        Some(HistoricalSyncPayloadExpectation::Empty)
    );
    assert_eq!(
        historical_sync_step(&report, HistoricalSyncPlanStepKind::HistoricalDataResult)
            .payload_expectation,
        Some(HistoricalSyncPayloadExpectation::HistoryEndAck {
            disposition: HistoricalSyncAckDisposition::Success,
        })
    );
}

#[test]
fn indexes_openwhoop_gen5_history_metadata_markers_fixture_with_normalized_markers() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    assert!(index.pass, "{:?}", index.issues);
    assert!(index.next_actions.is_empty(), "{:?}", index.next_actions);

    let fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.openwhoop.gen5.history_metadata_markers")
        .unwrap();
    assert_eq!(
        fixture.schema,
        goose_core::historical_sync::HISTORICAL_SYNC_DRY_RUN_SCHEMA
    );
    assert_eq!(fixture.kind, "synthetic");
    assert!(fixture.source.contains(OPENWHOOP_REFERENCE_COMMIT));
    assert!(fixture.notes.contains(OPENWHOOP_REFERENCE_LICENSE_CAVEAT));
    assert_eq!(
        fixture.expected.as_ref().unwrap(),
        &json!({
            "planned_command_count": 2,
            "state": "complete",
            "step_kinds": [
                "connect",
                "send_historical_data",
                "consume_metadata",
                "consume_metadata",
                "historical_data_result",
                "consume_metadata",
                "complete",
            ],
            "state_trace": [
                "idle",
                "connected",
                "transferring",
                "transferring",
                "ack_pending",
                "ack_pending",
                "ack_pending",
                "complete",
            ],
            "command_numbers": [22, 23],
            "command_payload_expectations": ["empty", "history_end_ack_success"],
            "marker_event_names": ["HistoryStart", "HistoryEnd", "HistoryComplete"],
            "marker_notes": ["history_start", "history_end", "history_complete"],
        })
    );

    let raw = fs::read_to_string(root.join(&fixture.path)).unwrap();
    let input: HistoricalSyncDryRunInput = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        input
            .fake_events
            .iter()
            .filter_map(|event| match event {
                HistoricalSyncFakeEvent::Metadata { name } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![" History Start ", "history-end", "HISTORY COMPLETE"]
    );

    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert_eq!(report.generation, HistoricalSyncGeneration::Gen5);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.planned_command_count, 2);
    assert_eq!(report.issues, Vec::<String>::new());
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .map(|step| historical_sync_step_kind_label(step.kind))
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["step_kinds"]
    );
    assert_eq!(
        json!(
            report
                .state_trace
                .iter()
                .copied()
                .map(historical_sync_state_label)
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["state_trace"]
    );
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .filter_map(|step| step.command_number)
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["command_numbers"]
    );
    assert_eq!(
        json!(
            report
                .steps
                .iter()
                .filter_map(|step| {
                    step.payload_expectation
                        .map(historical_sync_payload_expectation_label)
                })
                .collect::<Vec<_>>()
        ),
        fixture.expected.as_ref().unwrap()["command_payload_expectations"]
    );

    let metadata_steps = report
        .steps
        .iter()
        .filter(|step| step.kind == HistoricalSyncPlanStepKind::ConsumeMetadata)
        .collect::<Vec<_>>();
    assert_eq!(
        metadata_steps
            .iter()
            .map(|step| step.event_name.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["HistoryStart", "HistoryEnd", "HistoryComplete"]
    );
    assert_eq!(
        metadata_steps
            .iter()
            .map(|step| step.note.as_str())
            .collect::<Vec<_>>(),
        vec!["history_start", "history_end", "history_complete"]
    );
    assert_eq!(
        historical_sync_step(&report, HistoricalSyncPlanStepKind::SendHistoricalData)
            .payload_expectation,
        Some(HistoricalSyncPayloadExpectation::Empty)
    );
    assert_eq!(
        historical_sync_step(&report, HistoricalSyncPlanStepKind::HistoricalDataResult)
            .payload_expectation,
        Some(HistoricalSyncPayloadExpectation::HistoryEndAck {
            disposition: HistoricalSyncAckDisposition::Success,
        })
    );
}

#[test]
fn parser_runner_validates_indexed_frame_fixture_expectations() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();
    let report = run_parser_fixtures(root, &index);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty(), "{:?}", report.next_actions);
    assert_eq!(report.fixtures.len(), 14);
    let fixture = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.get_hello_frame")
        .unwrap();
    assert!(fixture.pass, "{:?}", fixture.issues);
    assert!(
        fixture.next_actions.is_empty(),
        "{:?}",
        fixture.next_actions
    );
    assert_eq!(fixture.parsed.as_ref().unwrap().payload_hex, "23019101");

    let historical = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.historical_k18_packet")
        .unwrap();
    assert_eq!(
        historical
            .parsed
            .as_ref()
            .unwrap()
            .packet_type_name
            .as_deref(),
        Some("HISTORICAL_DATA")
    );

    for (fixture_id, marker_value) in [
        ("owned.live_identity.k24_normal_history_payload", 151),
        ("owned.history_complete.k24_normal_history_payload", 51),
    ] {
        let owned_history = report
            .fixtures
            .iter()
            .find(|fixture| fixture.id == fixture_id)
            .unwrap();
        assert!(owned_history.pass, "{:?}", owned_history.issues);
        assert_eq!(owned_history.schema, PAYLOAD_HEX_SCHEMA);
        assert_eq!(
            owned_history
                .parsed
                .as_ref()
                .unwrap()
                .packet_type_name
                .as_deref(),
            Some("HISTORICAL_DATA")
        );
        let owned_history_payload =
            serde_json::to_value(&owned_history.parsed.as_ref().unwrap().parsed_payload).unwrap();
        assert_eq!(
            owned_history_payload["body_summary"]["kind"],
            "normal_history"
        );
        assert_eq!(
            owned_history_payload["body_summary"]["marker_value"],
            marker_value
        );
    }

    for (fixture_id, expected_summary_kind) in [
        (
            "synthetic.goose.v5.r17_optical_summary",
            "r17_optical_or_labrador_filtered",
        ),
        (
            "synthetic.goose.v5.k10_motion_summary_short",
            "raw_motion_k10",
        ),
        (
            "synthetic.goose.v5.k21_motion_summary_short",
            "raw_motion_k21",
        ),
        ("owned.live_identity.k10_motion_payload", "raw_motion_k10"),
        ("owned.live_identity.k21_motion_payload", "raw_motion_k21"),
        (
            "owned.history_complete.k10_motion_payload",
            "raw_motion_k10",
        ),
        (
            "owned.history_complete.k21_motion_payload",
            "raw_motion_k21",
        ),
    ] {
        let fixture = report
            .fixtures
            .iter()
            .find(|fixture| fixture.id == fixture_id)
            .unwrap();
        assert!(fixture.pass, "{:?}", fixture.issues);
        let parsed_payload =
            serde_json::to_value(&fixture.parsed.as_ref().unwrap().parsed_payload).unwrap();
        assert_eq!(
            parsed_payload["body_summary"]["kind"],
            expected_summary_kind
        );
    }

    let temperature = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "owned.history_complete.temperature_level_event_payload")
        .unwrap();
    assert!(temperature.pass, "{:?}", temperature.issues);
    assert_eq!(temperature.schema, PAYLOAD_HEX_SCHEMA);
    assert_eq!(
        temperature
            .parsed
            .as_ref()
            .unwrap()
            .packet_type_name
            .as_deref(),
        Some("EVENT")
    );
    let temperature_payload =
        serde_json::to_value(&temperature.parsed.as_ref().unwrap().parsed_payload).unwrap();
    assert_eq!(temperature_payload["kind"], "event");
    assert_eq!(temperature_payload["event_id"], 17);
    assert_eq!(temperature_payload["event_name"], "TEMPERATURE_LEVEL");
    assert_eq!(temperature_payload["data_offset"], 12);
    assert_eq!(temperature_payload["data_hex"].as_str().unwrap().len(), 336);
    assert_eq!(temperature_payload["warnings"].as_array().unwrap().len(), 0);

    let batch = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.sanitized.corebluetooth.frame_batch")
        .unwrap();
    assert!(batch.pass, "{:?}", batch.issues);
    assert_eq!(batch.schema, CAPTURED_FRAME_BATCH_SCHEMA);
    assert_eq!(batch.parsed_frames.len(), 2);
    assert_eq!(
        batch.parsed_frames[0].packet_type_name.as_deref(),
        Some("COMMAND")
    );
    assert_eq!(
        batch.parsed_frames[1].packet_type_name.as_deref(),
        Some("REALTIME_RAW_DATA")
    );
}

#[test]
fn index_flags_data_without_sidecar() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::write(tempdir.path().join("orphan.hex"), "aa").unwrap();

    let index = build_fixture_index(tempdir.path()).unwrap();

    assert!(!index.pass);
    assert!(
        index
            .issues
            .iter()
            .any(|issue| issue.contains("no .fixture.json sidecar"))
    );
    assert!(
        index
            .next_actions
            .iter()
            .any(|action| action.reason == "missing_sidecar")
    );
}

#[test]
fn parser_runner_reports_next_actions_for_invalid_frame_fixture() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::write(tempdir.path().join("bad.hex"), "zz").unwrap();
    write_fixture_sidecar(
        tempdir.path(),
        "bad.fixture.json",
        serde_json::json!({
            "id": "synthetic.bad_frame",
            "path": "bad.hex",
            "kind": "parser_regression",
            "source": "synthetic",
            "captured_at": "2026-05-28T00:00:00Z",
            "device_model": "WHOOP 5.0",
            "device_firmware": "synthetic",
            "app_version": "synthetic",
            "schema": FRAME_HEX_SCHEMA,
            "consent": "synthetic",
            "sensitivity": "none"
        }),
    );

    let index = build_fixture_index(tempdir.path()).unwrap();
    assert!(index.pass, "{:?}", index.issues);

    let report = run_parser_fixtures(tempdir.path(), &index);

    assert!(!report.pass);
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "frame_hex_invalid")
    );
    let fixture = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.bad_frame")
        .unwrap();
    assert!(
        fixture
            .next_actions
            .iter()
            .any(|action| action.reason == "frame_hex_invalid")
    );
}

#[test]
fn parser_runner_reports_next_action_for_missing_expected_fields() {
    let tempdir = tempfile::tempdir().unwrap();
    let valid_frame =
        fs::read_to_string("fixtures/synthetic/goose_v5_get_hello_frame.hex").unwrap();
    fs::write(tempdir.path().join("valid.hex"), valid_frame).unwrap();
    write_fixture_sidecar(
        tempdir.path(),
        "valid.fixture.json",
        serde_json::json!({
            "id": "synthetic.missing_expected_fields",
            "path": "valid.hex",
            "kind": "parser_regression",
            "source": "synthetic",
            "captured_at": "2026-05-28T00:00:00Z",
            "device_model": "WHOOP 5.0",
            "device_firmware": "synthetic",
            "app_version": "synthetic",
            "schema": FRAME_HEX_SCHEMA,
            "consent": "synthetic",
            "sensitivity": "none",
            "expected": {}
        }),
    );

    let index = build_fixture_index(tempdir.path()).unwrap();
    assert!(index.pass, "{:?}", index.issues);

    let report = run_parser_fixtures(tempdir.path(), &index);

    assert!(!report.pass);
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "expected_field_missing")
    );
    let fixture = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.missing_expected_fields")
        .unwrap();
    assert!(
        fixture
            .next_actions
            .iter()
            .any(|action| action.reason == "expected_field_missing")
    );
}

fn write_fixture_sidecar(root: &Path, name: &str, value: serde_json::Value) {
    fs::write(root.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn session_metrics<'a>(
    corpus: &'a ActivityFixtureSet,
    session_id: &str,
) -> Vec<&'a ActivityMetricFixture> {
    corpus
        .activity_metrics
        .iter()
        .filter(|metric| metric.activity_session_id == session_id)
        .collect()
}

fn session_intervals<'a>(
    corpus: &'a ActivityFixtureSet,
    session_id: &str,
) -> Vec<&'a ActivityIntervalFixture> {
    corpus
        .activity_intervals
        .iter()
        .filter(|interval| interval.activity_session_id == session_id)
        .collect()
}

fn session_labels<'a>(
    corpus: &'a ActivityFixtureSet,
    session_id: &str,
) -> Vec<&'a ActivityLabelFixture> {
    corpus
        .activity_labels
        .iter()
        .filter(|label| label.activity_session_id == session_id)
        .collect()
}

fn assert_provenance_shape(provenance: &serde_json::Value, session_kind: &str) {
    let object = provenance.as_object().expect("provenance_json object");
    assert_eq!(
        object.get("source").and_then(serde_json::Value::as_str),
        Some("synthetic.activity.fixture")
    );
    assert_eq!(
        object
            .get("session_kind")
            .and_then(serde_json::Value::as_str),
        Some(session_kind)
    );
    assert_eq!(
        object.get("status").and_then(serde_json::Value::as_str),
        Some("pre_device")
    );
}

fn planned_activity_session_subset(
    session: &goose_core::health_sync::PlannedActivityHealthWrite,
) -> serde_json::Value {
    json!({
        "session_id": &session.session_id,
        "session_kind": session.session_kind,
        "activity_type": &session.activity_type,
        "destination_type": &session.destination_type,
        "destination_activity_type": &session.destination_activity_type,
        "raw_activity_type": &session.raw_activity_type,
        "custom_label": &session.custom_label,
        "start_time": &session.start_time,
        "end_time": &session.end_time,
        "attached_metric_count": session.attached_metric_count,
        "attached_interval_count": session.attached_interval_count,
        "goose_marker": &session.goose_marker,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenWhoopUuidDetectionFixture {
    schema: String,
    service_uuid_probes: Vec<OpenWhoopUuidProbe>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenWhoopUuidProbe {
    label: String,
    service_uuid: String,
}

fn historical_sync_step(
    report: &goose_core::historical_sync::HistoricalSyncDryRunReport,
    kind: HistoricalSyncPlanStepKind,
) -> &HistoricalSyncPlanStep {
    report
        .steps
        .iter()
        .find(|step| step.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} step"))
}

fn historical_sync_step_kind_label(kind: HistoricalSyncPlanStepKind) -> &'static str {
    match kind {
        HistoricalSyncPlanStepKind::Connect => "connect",
        HistoricalSyncPlanStepKind::GetDataRange => "get_data_range",
        HistoricalSyncPlanStepKind::SendHistoricalData => "send_historical_data",
        HistoricalSyncPlanStepKind::ConsumeMetadata => "consume_metadata",
        HistoricalSyncPlanStepKind::ConsumeReading => "consume_reading",
        HistoricalSyncPlanStepKind::HistoricalDataResult => "historical_data_result",
        HistoricalSyncPlanStepKind::AbortHistoricalTransmits => "abort_historical_transmits",
        HistoricalSyncPlanStepKind::ResumeRequested => "resume_requested",
        HistoricalSyncPlanStepKind::Blocked => "blocked",
        HistoricalSyncPlanStepKind::Failed => "failed",
        HistoricalSyncPlanStepKind::Complete => "complete",
    }
}

fn historical_sync_state_label(state: HistoricalSyncState) -> &'static str {
    match state {
        HistoricalSyncState::Idle => "idle",
        HistoricalSyncState::Connected => "connected",
        HistoricalSyncState::RangeRequested => "range_requested",
        HistoricalSyncState::Transferring => "transferring",
        HistoricalSyncState::AckPending => "ack_pending",
        HistoricalSyncState::Complete => "complete",
        HistoricalSyncState::Blocked => "blocked",
        HistoricalSyncState::Failed => "failed",
    }
}

fn historical_sync_payload_expectation_label(
    expectation: HistoricalSyncPayloadExpectation,
) -> &'static str {
    match expectation {
        HistoricalSyncPayloadExpectation::Empty => "empty",
        HistoricalSyncPayloadExpectation::ZeroByte => "zero_byte",
        HistoricalSyncPayloadExpectation::HistoryEndAck {
            disposition: HistoricalSyncAckDisposition::Success,
        } => "history_end_ack_success",
        HistoricalSyncPayloadExpectation::HistoryEndAck {
            disposition: HistoricalSyncAckDisposition::Failure,
        } => "history_end_ack_failure",
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityFixtureSet {
    schema: String,
    activity_sessions: Vec<ActivitySessionFixture>,
    activity_metrics: Vec<ActivityMetricFixture>,
    activity_intervals: Vec<ActivityIntervalFixture>,
    activity_labels: Vec<ActivityLabelFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivitySessionFixture {
    session_id: String,
    source: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    duration_ms: i64,
    activity_type: String,
    external_activity_type_code: Option<String>,
    external_activity_type_name: Option<String>,
    custom_label: Option<String>,
    confidence: f64,
    detection_method: String,
    sync_status: String,
    provenance_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityMetricFixture {
    metric_id: String,
    activity_session_id: String,
    metric_name: String,
    value: f64,
    unit: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    quality_flags_json: Vec<String>,
    provenance_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityIntervalFixture {
    interval_id: String,
    activity_session_id: String,
    interval_type: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    duration_ms: i64,
    sequence: i64,
    metadata_json: serde_json::Value,
    provenance_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityLabelFixture {
    label_id: String,
    activity_session_id: String,
    label_type: String,
    value: String,
    source: String,
    confidence: Option<f64>,
    provenance_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalSyncCommandValidationFixture {
    schema: String,
    generation: String,
    request_data_range: bool,
    command_sequence: Vec<String>,
    command_validation_rows: Vec<HistoricalSyncCommandValidationRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalSyncCommandValidationRow {
    command: String,
    command_number: Option<u16>,
    family: String,
    risk_gate: String,
    direct_send_ready: bool,
    missing_requirements: Vec<String>,
    warnings: Vec<String>,
    next_capture_actions: Vec<serde_json::Value>,
    validated_service_uuid: Option<String>,
    validated_characteristic_uuid: Option<String>,
    validated_write_type: Option<String>,
    validated_evidence_source: Option<String>,
    validated_capture_kind: Option<String>,
    validated_owner: Option<String>,
    validated_provenance_json: Option<String>,
    validated_triggering_ui_action: Option<String>,
    report_json: serde_json::Value,
}
