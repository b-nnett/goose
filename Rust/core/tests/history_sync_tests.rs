use std::{fs, path::Path};

use goose_core::{
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    fixtures::build_fixture_index,
    historical_sync::{
        HISTORICAL_SYNC_DRY_RUN_REPORT_SCHEMA, HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA,
        HistoricalSyncAckDisposition, HistoricalSyncCharacteristicEvidence,
        HistoricalSyncDryRunInput, HistoricalSyncFakeEvent, HistoricalSyncGeneration,
        HistoricalSyncNotificationEvidence, HistoricalSyncObservedCommand,
        HistoricalSyncObservedEvent, HistoricalSyncPayloadExpectation,
        HistoricalSyncPhysicalValidationInput, HistoricalSyncPlanStepKind,
        HistoricalSyncRawEvidenceAnchor, HistoricalSyncSafetyGate, HistoricalSyncState,
        HistoricalSyncTimestampEvidence, historical_sync_physical_evidence_template,
        run_historical_sync_dry_run, validate_historical_sync_physical_evidence,
    },
    store::{ActivitySessionInput, CaptureSessionInput, GooseStore},
};
use serde::Deserialize;

#[test]
fn gen5_history_plan_uses_empty_payloads_and_can_skip_the_range_request() {
    let events = happy_path_events();
    let input = base_input(HistoricalSyncGeneration::Gen5, true, events.clone());

    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert!(
        report
            .state_trace
            .contains(&HistoricalSyncState::RangeRequested)
    );
    assert!(
        report
            .state_trace
            .contains(&HistoricalSyncState::AckPending)
    );
    assert_eq!(report.planned_command_count, 3);
    assert_eq!(report.issues, Vec::<String>::new());

    let get_data_range = step(&report, HistoricalSyncPlanStepKind::GetDataRange);
    assert_eq!(
        get_data_range.safety_gate,
        Some(HistoricalSyncSafetyGate::ReadOnly)
    );
    assert_eq!(
        get_data_range.payload_expectation,
        Some(HistoricalSyncPayloadExpectation::Empty)
    );

    let send_historical_data = step(&report, HistoricalSyncPlanStepKind::SendHistoricalData);
    assert_eq!(
        send_historical_data.payload_expectation,
        Some(HistoricalSyncPayloadExpectation::Empty)
    );

    let history_end_ack = step(&report, HistoricalSyncPlanStepKind::HistoricalDataResult);
    assert_eq!(
        history_end_ack.payload_expectation,
        Some(HistoricalSyncPayloadExpectation::HistoryEndAck {
            disposition: HistoricalSyncAckDisposition::Success
        })
    );

    let mut no_range_input = input.clone();
    no_range_input.request_data_range = false;
    let no_range_report = run_historical_sync_dry_run(&no_range_input);

    assert!(no_range_report.pass, "{:?}", no_range_report.issues);
    assert_eq!(no_range_report.planned_command_count, 2);
    assert!(
        !no_range_report
            .steps
            .iter()
            .any(|step| step.kind == HistoricalSyncPlanStepKind::GetDataRange)
    );
    assert!(
        !no_range_report
            .state_trace
            .contains(&HistoricalSyncState::RangeRequested)
    );
}

#[test]
fn physical_historical_sync_evidence_passes_when_capture_confirms_flow_and_timestamps() {
    let input = physical_validation_input();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.schema,
        HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA
    );
    assert_eq!(report.generation, HistoricalSyncGeneration::Gen5);
    assert_eq!(report.capture_session_id, "strap-capture-2026-01-01");
    assert!(report.service_uuid_confirmed);
    assert!(report.characteristic_roles_confirmed);
    assert!(report.notification_behavior_confirmed);
    assert!(report.auth_session_handshake_confirmed);
    assert!(report.command_flow_confirmed);
    assert!(report.event_order_confirmed);
    assert!(report.timestamp_fields_confirmed);
    assert_eq!(
        report.acceptance_summary.policy,
        "historical_sync_physical_must_match_current_flow_timestamp_and_evidence_contract"
    );
    assert!(report.acceptance_summary.physical_sync_ready);
    assert_eq!(
        report.acceptance_summary.capture_session_id,
        "strap-capture-2026-01-01"
    );
    assert_eq!(report.acceptance_summary.issue_count, 0);
    assert_eq!(
        report
            .provenance
            .get("report_integrity_policy")
            .and_then(serde_json::Value::as_str),
        Some(goose_core::historical_sync::HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY)
    );
    assert!(report.issues.is_empty());
    assert!(report.next_actions.is_empty());
}

#[test]
fn physical_historical_sync_evidence_reports_missing_capture_requirements() {
    let mut input = physical_validation_input();
    input.capture_session_id.clear();
    input.notification_subscriptions.clear();
    input.auth_events = vec![HistoricalSyncObservedEvent {
        name: "connected".to_string(),
        sequence: 1,
        capture_session_id: Some("strap-capture-2026-01-01".to_string()),
    }];
    input.timestamp_evidence.clear();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.notification_behavior_confirmed);
    assert!(!report.auth_session_handshake_confirmed);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"capture_session_id_required".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_notification_behavior_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"auth_session_handshake_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "historical_motion_timestamp_fields_unproven")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "historical_heart_rate_timestamp_fields_unproven")
    );
}

#[test]
fn physical_historical_sync_evidence_requires_raw_evidence_fingerprints() {
    let mut input = physical_validation_input();
    input.raw_evidence_anchors.clear();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.raw_evidence_anchored);
    assert!(
        report
            .issues
            .contains(&"historical_raw_evidence_fingerprints_missing".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "historical_raw_evidence_fingerprints_missing"
            && action.action.contains("SHA-256")
    }));
}

#[test]
fn physical_historical_sync_evidence_requires_expected_service_for_roles() {
    let mut input = physical_validation_input();
    input.characteristics[0].service_uuid = "00000000-0000-0000-0000-000000000000".to_string();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.characteristic_roles_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_characteristic_roles_incomplete".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_requires_characteristic_properties_for_roles() {
    let mut input = physical_validation_input();
    input.characteristics[0].properties = vec!["notify".to_string()];
    input.characteristics[1].properties.clear();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.characteristic_roles_confirmed);
    assert!(report.notification_behavior_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_characteristic_roles_incomplete".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "historical_characteristic_roles_incomplete")
    );
}

#[test]
fn physical_historical_sync_evidence_requires_ordered_physical_flow() {
    let mut input = physical_validation_input();
    input.metadata_events = vec![
        HistoricalSyncObservedEvent {
            name: "HistoryComplete".to_string(),
            sequence: 5,
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
        HistoricalSyncObservedEvent {
            name: "HistoryStart".to_string(),
            sequence: 6,
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
        HistoricalSyncObservedEvent {
            name: "HistoryEnd".to_string(),
            sequence: 7,
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
    ];

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(report.command_flow_confirmed);
    assert!(!report.event_order_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_event_order_unproven".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "historical_event_order_unproven")
    );
}

#[test]
fn physical_historical_sync_evidence_requires_sample_time_to_match_device_timestamp() {
    let mut input = physical_validation_input();
    input.timestamp_evidence[0].sample_time = Some("2026-01-01T22:00:01Z".to_string());

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        !report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_rejects_invalid_timestamp_subseconds() {
    let mut input = physical_validation_input();
    input.timestamp_evidence[0].device_timestamp_subseconds = Some(1_500);
    input.timestamp_evidence[0].sample_time = Some("2026-01-01T22:00:00.999Z".to_string());

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        !report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_requires_explicit_timestamp_source_signals() {
    let mut input = physical_validation_input();
    input.timestamp_evidence[0].source_signal.clear();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        !report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );

    let mut input = physical_validation_input();
    input.timestamp_evidence[1].source_signal.clear();

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        !report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_requires_distinct_motion_and_hr_timestamp_rows() {
    let mut input = physical_validation_input();
    input.timestamp_evidence = vec![HistoricalSyncTimestampEvidence {
        packet_kind: "raw_motion_k21_normal_history".to_string(),
        source_signal: "raw_motion_k21_heart_rate".to_string(),
        captured_at: "2026-01-01T20:00:00Z".to_string(),
        sample_time: Some("2026-01-01T22:00:00Z".to_string()),
        sample_time_source: Some("device_timestamp".to_string()),
        device_timestamp_seconds: Some(1_767_304_800),
        device_timestamp_subseconds: Some(0),
        capture_session_id: Some("strap-capture-2026-01-01".to_string()),
    }];

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_rejects_ambiguous_motion_hr_timestamp_rows() {
    let mut input = physical_validation_input();
    input.timestamp_evidence = vec![
        HistoricalSyncTimestampEvidence {
            packet_kind: "raw_motion_k21".to_string(),
            source_signal: "heart_rate".to_string(),
            captured_at: "2026-01-01T20:00:00Z".to_string(),
            sample_time: Some("2026-01-01T22:00:00Z".to_string()),
            sample_time_source: Some("device_timestamp".to_string()),
            device_timestamp_seconds: Some(1_767_304_800),
            device_timestamp_subseconds: Some(0),
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
        HistoricalSyncTimestampEvidence {
            packet_kind: "raw_motion_k21".to_string(),
            source_signal: "heart_rate".to_string(),
            captured_at: "2026-01-01T20:00:00Z".to_string(),
            sample_time: Some("2026-01-01T22:05:00Z".to_string()),
            sample_time_source: Some("device_timestamp".to_string()),
            device_timestamp_seconds: Some(1_767_305_100),
            device_timestamp_subseconds: Some(0),
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
    ];

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.timestamp_fields_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn physical_historical_sync_evidence_requires_matching_capture_session_rows() {
    let mut input = physical_validation_input();
    input.command_events[0].capture_session_id = Some("different-capture".to_string());

    let report = validate_historical_sync_physical_evidence(&input);

    assert!(!report.pass);
    assert!(!report.evidence_session_confirmed);
    assert!(
        report
            .issues
            .contains(&"historical_evidence_session_mismatch".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "historical_evidence_session_mismatch")
    );
}

#[test]
fn physical_historical_sync_evidence_template_names_required_capture_fields() {
    let template = historical_sync_physical_evidence_template(
        HistoricalSyncGeneration::Gen5,
        "strap-capture-template",
    );

    assert_eq!(
        template.schema,
        "goose.historical-sync-physical-evidence-template.v1"
    );
    assert_eq!(
        template.expected_service_uuid,
        "fd4b0001cce1403393ce002d5875f58a"
    );
    assert_eq!(
        template.input.schema,
        "goose.historical-sync-physical-validation.v1"
    );
    assert_eq!(template.input.capture_session_id, "strap-capture-template");
    assert!(
        template
            .input
            .characteristics
            .iter()
            .any(|characteristic| characteristic.role == "command_to_strap")
    );
    assert!(
        template
            .input
            .timestamp_evidence
            .iter()
            .any(|evidence| evidence.source_signal == "heart_rate")
    );
    assert!(
        template
            .required_observations
            .iter()
            .any(|action| action.reason == "historical_motion_timestamp_fields_unproven")
    );
    assert!(
        template
            .required_observations
            .iter()
            .any(|action| action.reason == "historical_heart_rate_timestamp_fields_unproven")
    );

    let validation = validate_historical_sync_physical_evidence(&template.input);
    assert!(!validation.pass);
    assert!(
        validation
            .issues
            .contains(&"historical_timestamp_fields_unproven".to_string())
    );
    assert!(
        validation
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        validation
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
}

#[test]
fn historical_sync_validator_cli_writes_template_and_validates_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let template_path = tempdir.path().join("historical-sync-template.json");
    let template_output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-historical-sync-validator"))
            .args([
                "--template",
                "--generation",
                "gen5",
                "--capture-session-id",
                "strap-capture-cli",
                "--output",
                template_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

    assert!(
        template_output.status.success(),
        "{}",
        String::from_utf8_lossy(&template_output.stderr)
    );
    let template_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&template_path).unwrap()).unwrap();
    assert_eq!(
        template_json["schema"],
        "goose.historical-sync-physical-evidence-template.v1"
    );
    assert_eq!(
        template_json["input"]["capture_session_id"],
        "strap-capture-cli"
    );

    let evidence_path = tempdir.path().join("historical-sync-evidence.json");
    fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&physical_validation_input()).unwrap(),
    )
    .unwrap();
    let report_path = tempdir.path().join("historical-sync-validation.json");
    let validation_output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-historical-sync-validator"))
            .args([
                "--evidence",
                evidence_path.to_str().unwrap(),
                "--output",
                report_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

    assert!(
        validation_output.status.success(),
        "{}",
        String::from_utf8_lossy(&validation_output.stderr)
    );
    let report_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(
        report_json["schema"],
        "goose.historical-sync-physical-validation-report.v1"
    );
    assert_eq!(report_json["pass"], true);
}

#[test]
fn gen4_history_plan_uses_zero_byte_payloads() {
    let input = base_input(HistoricalSyncGeneration::Gen4, true, happy_path_events());
    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.planned_command_count, 3);
    assert_eq!(
        step(&report, HistoricalSyncPlanStepKind::GetDataRange).payload_expectation,
        Some(HistoricalSyncPayloadExpectation::ZeroByte)
    );
    assert_eq!(
        step(&report, HistoricalSyncPlanStepKind::SendHistoricalData).payload_expectation,
        Some(HistoricalSyncPayloadExpectation::ZeroByte)
    );
}

#[test]
fn idle_timeout_aborts_then_retries_once() {
    let input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::IdleTimeout,
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::Reading {
                name: "retry-reading".to_string(),
            },
            HistoricalSyncFakeEvent::HistoryEnd,
            HistoricalSyncFakeEvent::HistoryComplete,
        ],
    );
    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.retry_count, 1);
    assert_eq!(report.timeout_count, 1);
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|step| step.kind == HistoricalSyncPlanStepKind::AbortHistoricalTransmits)
            .count(),
        1
    );
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|step| step.kind == HistoricalSyncPlanStepKind::SendHistoricalData)
            .count(),
        2
    );
}

#[test]
fn disconnected_device_blocks_before_planning_commands() {
    let mut input = base_input(HistoricalSyncGeneration::Gen5, true, vec![]);
    input.device_connected = false;

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Blocked);
    assert_eq!(
        report.state_trace,
        vec![HistoricalSyncState::Idle, HistoricalSyncState::Blocked]
    );
    assert_eq!(report.planned_command_count, 0);
    assert_eq!(report.blocked_count, 1);
    assert_eq!(report.failed_count, 0);
    assert_eq!(
        report.issues,
        vec![
            "device_disconnected".to_string(),
            "history_sync_cancelled".to_string()
        ]
    );
    assert_eq!(
        report
            .next_actions
            .iter()
            .map(|action| action.reason.as_str())
            .collect::<Vec<_>>(),
        vec!["device_disconnected", "history_sync_cancelled"]
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "device_disconnected" && action.action.contains("Connect the strap")
    }));
}

#[test]
fn malformed_response_fails_without_retrying() {
    let input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![HistoricalSyncFakeEvent::MalformedResponse {
            detail: "unexpected payload shape".to_string(),
        }],
    );

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Failed);
    assert_eq!(report.planned_command_count, 2);
    assert_eq!(report.retry_count, 0);
    assert_eq!(report.timeout_count, 0);
    assert_eq!(report.failed_count, 1);
    assert_eq!(report.issues, vec!["malformed_response".to_string()]);
    assert_eq!(report.next_actions.len(), 1);
    assert_eq!(report.next_actions[0].reason, "malformed_response");
    assert!(report.next_actions[0].action.contains("response parser"));
}

#[test]
fn duplicate_transfer_fails_without_retrying() {
    let input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![HistoricalSyncFakeEvent::DuplicateTransfer {
            detail: "duplicate sample window".to_string(),
        }],
    );

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Failed);
    assert_eq!(report.planned_command_count, 2);
    assert_eq!(report.retry_count, 0);
    assert_eq!(report.timeout_count, 0);
    assert_eq!(report.failed_count, 1);
    assert_eq!(report.issues, vec!["duplicate_transfer".to_string()]);
    assert_eq!(report.next_actions.len(), 1);
    assert_eq!(report.next_actions[0].reason, "duplicate_transfer");
    assert!(report.next_actions[0].action.contains("duplicate transfer"));
}

#[test]
fn idle_timeout_exhausts_retry_budget_and_fails() {
    let input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::IdleTimeout,
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::IdleTimeout,
        ],
    );

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Failed);
    assert_eq!(report.retry_count, 1);
    assert_eq!(report.timeout_count, 2);
    assert_eq!(report.failed_count, 1);
    assert_eq!(report.planned_command_count, 4);
    assert_eq!(
        report.issues,
        vec!["idle_timeout_retry_exhausted".to_string()]
    );
    assert_eq!(report.next_actions.len(), 1);
    assert_eq!(
        report.next_actions[0].reason,
        "idle_timeout_retry_exhausted"
    );
    assert!(
        report.next_actions[0]
            .action
            .contains("transfer timeout and retry budget")
    );
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|step| step.kind == HistoricalSyncPlanStepKind::AbortHistoricalTransmits)
            .count(),
        1
    );
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|step| step.kind == HistoricalSyncPlanStepKind::SendHistoricalData)
            .count(),
        2
    );
}

#[test]
fn cancel_and_resume_restarts_from_blocked_state() {
    let mut input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::CancelRequested,
            HistoricalSyncFakeEvent::ResumeRequested,
            HistoricalSyncFakeEvent::HistoryStart,
            HistoricalSyncFakeEvent::HistoryEnd,
            HistoricalSyncFakeEvent::HistoryComplete,
        ],
    );
    input.cancel.requested = true;
    input.cancel.reason = Some("user_cancelled".to_string());
    input.resume.requested = true;
    input.resume.resume_from_state = Some(HistoricalSyncState::Blocked);

    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.blocked_count, 1);
    assert_eq!(report.cancel_count, 1);
    assert_eq!(report.resume_count, 1);
    assert!(
        report
            .steps
            .iter()
            .any(|step| step.kind == HistoricalSyncPlanStepKind::Blocked)
    );
    assert!(
        report
            .steps
            .iter()
            .any(|step| step.kind == HistoricalSyncPlanStepKind::ResumeRequested)
    );
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|step| step.kind == HistoricalSyncPlanStepKind::Connect)
            .count(),
        2
    );
}

#[test]
fn safety_gate_lock_blocks_before_planning_commands() {
    let mut input = base_input(
        HistoricalSyncGeneration::Gen5,
        true,
        vec![HistoricalSyncFakeEvent::HistoryStart],
    );
    input.safety_gate_ready = false;

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert_eq!(report.state, HistoricalSyncState::Blocked);
    assert_eq!(report.blocked_count, 1);
    assert_eq!(report.planned_command_count, 0);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "safety_gate_locked")
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "safety_gate_locked" && action.action.contains("safety gate")
    }));
}

#[test]
fn stale_preflight_rejects_unsupported_schema_before_any_commands() {
    let mut input = base_input(HistoricalSyncGeneration::Gen5, true, vec![]);
    input.schema = "goose.historical-sync-dry-run.v0".to_string();

    let report = run_historical_sync_dry_run(&input);

    assert!(!report.pass);
    assert!(!report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Failed);
    assert_eq!(
        report.state_trace,
        vec![HistoricalSyncState::Idle, HistoricalSyncState::Failed]
    );
    assert_eq!(report.planned_command_count, 0);
    assert_eq!(report.failed_count, 1);
    assert_eq!(
        report.issues,
        vec!["unsupported schema goose.historical-sync-dry-run.v0".to_string()]
    );
    assert_eq!(report.next_actions.len(), 1);
    assert_eq!(
        report.next_actions[0].reason,
        "unsupported schema goose.historical-sync-dry-run.v0"
    );
    assert!(
        report.next_actions[0].action.contains(
            "Use goose.historical-sync-dry-run.v1 input before planning historical sync."
        )
    );
}

#[test]
fn fake_history_sync_can_seed_capture_evidence_and_a_candidate_activity_session() {
    let input = base_input(HistoricalSyncGeneration::Gen5, true, happy_path_events());
    let report = run_historical_sync_dry_run(&input);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert_eq!(report.state, HistoricalSyncState::Complete);
    assert_eq!(report.planned_command_count, 3);
    assert!(report.issues.is_empty());

    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    let captured_frame_fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.sanitized.corebluetooth.frame_batch")
        .expect("missing synthetic captured-frame batch fixture");
    let activity_fixture = index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.activity.sessions.pre_device.hand_derived")
        .expect("missing pre-device activity fixture");

    let store = GooseStore::open_in_memory().unwrap();
    let capture_session_id = "fake.history-sync.capture-session";
    let capture_session_provenance = serde_json::json!({
        "source": "historical_sync_dry_run",
        "fixture_id": captured_frame_fixture.id.as_str(),
        "report_schema": HISTORICAL_SYNC_DRY_RUN_REPORT_SCHEMA,
        "report_state": report.state,
        "planned_command_count": report.planned_command_count,
    })
    .to_string();

    assert!(
        store
            .start_capture_session(CaptureSessionInput {
                session_id: capture_session_id,
                source: "historical_sync_dry_run",
                started_at_unix_ms: 1_770_000_000_000,
                device_model: "WHOOP 5.0 Goose",
                active_device_id: None,
                provenance_json: &capture_session_provenance,
            })
            .unwrap()
    );

    let captured_frames: CapturedFrameBatchFixture =
        load_json_fixture(&root.join(&captured_frame_fixture.path));
    let mut historical_frame = captured_frames
        .frames
        .into_iter()
        .find(|frame| frame.evidence_id == "synthetic.sanitized.corebluetooth.k10_motion")
        .expect("missing historical motion frame in captured-frame batch fixture");
    historical_frame.capture_session_id = Some(capture_session_id.to_string());

    let import = import_captured_frame_batch(
        &store,
        &[historical_frame],
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(import.pass, "{:?}", import.issues);
    assert_eq!(import.frame_count, 1);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 1);
    assert_eq!(
        store
            .raw_evidence("synthetic.sanitized.corebluetooth.k10_motion")
            .unwrap()
            .unwrap()
            .capture_session_id
            .as_deref(),
        Some(capture_session_id)
    );

    let finished_capture = store
        .finish_capture_session(capture_session_id, 1_770_000_010_000, 1)
        .unwrap();
    assert_eq!(finished_capture.status, "finished");
    assert_eq!(finished_capture.frame_count, 1);

    let activity_fixture_set: ActivityFixtureSet =
        load_json_fixture(&root.join(&activity_fixture.path));
    let candidate = activity_fixture_set
        .activity_sessions
        .into_iter()
        .find(|session| session.session_id == "synthetic.activity.no_hr.session")
        .expect("missing pre-device candidate activity session fixture");

    assert_eq!(candidate.sync_status, "candidate");

    let mut provenance = candidate.provenance_json.as_object().cloned().unwrap();
    provenance.insert(
        "historical_sync".to_string(),
        serde_json::json!({
            "report_schema": HISTORICAL_SYNC_DRY_RUN_REPORT_SCHEMA,
            "report_state": report.state,
            "capture_session_id": capture_session_id,
            "evidence_id": "synthetic.sanitized.corebluetooth.k10_motion",
            "decoded_frame_id": "synthetic.sanitized.corebluetooth.k10_motion.frame.0",
            "fixture_id": activity_fixture.id.as_str(),
        }),
    );

    let activity_provenance = serde_json::Value::Object(provenance).to_string();
    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: &candidate.session_id,
                source: &candidate.source,
                start_time_unix_ms: candidate.start_time_unix_ms,
                end_time_unix_ms: candidate.end_time_unix_ms,
                activity_type: &candidate.activity_type,
                external_activity_type_code: candidate.external_activity_type_code.as_deref(),
                external_activity_type_name: candidate.external_activity_type_name.as_deref(),
                custom_label: candidate.custom_label.as_deref(),
                confidence: candidate.confidence,
                detection_method: &candidate.detection_method,
                sync_status: &candidate.sync_status,
                provenance_json: &activity_provenance,
            })
            .unwrap()
    );

    let saved_candidate = store
        .activity_session(&candidate.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(saved_candidate.source, "synthetic.pre_device");
    assert_eq!(saved_candidate.activity_type, "walking");
    assert_eq!(saved_candidate.sync_status, "candidate");
    assert!(saved_candidate.provenance_json.contains(capture_session_id));
    assert!(
        saved_candidate
            .provenance_json
            .contains("synthetic.sanitized.corebluetooth.k10_motion")
    );
    assert_eq!(
        store
            .activity_sessions_by_sync_status("candidate")
            .unwrap()
            .len(),
        1
    );
}

fn base_input(
    generation: HistoricalSyncGeneration,
    request_data_range: bool,
    fake_events: Vec<HistoricalSyncFakeEvent>,
) -> HistoricalSyncDryRunInput {
    HistoricalSyncDryRunInput {
        schema: "goose.historical-sync-dry-run.v1".to_string(),
        generation,
        device_connected: true,
        safety_gate_ready: true,
        request_data_range,
        retry: Default::default(),
        timeout: Default::default(),
        cancel: Default::default(),
        resume: Default::default(),
        fake_events,
    }
}

fn happy_path_events() -> Vec<HistoricalSyncFakeEvent> {
    vec![
        HistoricalSyncFakeEvent::HistoryStart,
        HistoricalSyncFakeEvent::Metadata {
            name: "interval_metadata".to_string(),
        },
        HistoricalSyncFakeEvent::Reading {
            name: "reading_payload".to_string(),
        },
        HistoricalSyncFakeEvent::HistoryEnd,
        HistoricalSyncFakeEvent::Metadata {
            name: "tail_metadata".to_string(),
        },
        HistoricalSyncFakeEvent::HistoryComplete,
    ]
}

fn physical_validation_input() -> HistoricalSyncPhysicalValidationInput {
    HistoricalSyncPhysicalValidationInput {
        schema: "goose.historical-sync-physical-validation.v1".to_string(),
        generation: HistoricalSyncGeneration::Gen5,
        capture_session_id: "strap-capture-2026-01-01".to_string(),
        service_uuids: vec!["fd4b0001-cce1-4033-93ce-002d5875f58a".to_string()],
        characteristics: vec![
            HistoricalSyncCharacteristicEvidence {
                service_uuid: "fd4b0001-cce1-4033-93ce-002d5875f58a".to_string(),
                characteristic_uuid: "fd4b0002-cce1-4033-93ce-002d5875f58a".to_string(),
                role: "command_to_strap".to_string(),
                properties: vec!["write_without_response".to_string()],
            },
            HistoricalSyncCharacteristicEvidence {
                service_uuid: "fd4b0001-cce1-4033-93ce-002d5875f58a".to_string(),
                characteristic_uuid: "fd4b0003-cce1-4033-93ce-002d5875f58a".to_string(),
                role: "data_from_strap".to_string(),
                properties: vec!["notify".to_string()],
            },
            HistoricalSyncCharacteristicEvidence {
                service_uuid: "fd4b0001-cce1-4033-93ce-002d5875f58a".to_string(),
                characteristic_uuid: "fd4b0004-cce1-4033-93ce-002d5875f58a".to_string(),
                role: "event_from_strap".to_string(),
                properties: vec!["notify".to_string()],
            },
        ],
        notification_subscriptions: vec![
            HistoricalSyncNotificationEvidence {
                characteristic_uuid: "fd4b0003-cce1-4033-93ce-002d5875f58a".to_string(),
                enabled: true,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncNotificationEvidence {
                characteristic_uuid: "fd4b0004-cce1-4033-93ce-002d5875f58a".to_string(),
                enabled: true,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
        ],
        auth_events: vec![
            HistoricalSyncObservedEvent {
                name: "connected".to_string(),
                sequence: 1,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncObservedEvent {
                name: "authenticated".to_string(),
                sequence: 2,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncObservedEvent {
                name: "subscribed".to_string(),
                sequence: 3,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
        ],
        command_events: vec![
            HistoricalSyncObservedCommand {
                command: "send_historical_data".to_string(),
                sequence: 4,
                response_observed: true,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncObservedCommand {
                command: "historical_data_result".to_string(),
                sequence: 8,
                response_observed: true,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
        ],
        metadata_events: vec![
            HistoricalSyncObservedEvent {
                name: "HistoryStart".to_string(),
                sequence: 5,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncObservedEvent {
                name: "HistoryEnd".to_string(),
                sequence: 6,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncObservedEvent {
                name: "HistoryComplete".to_string(),
                sequence: 7,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
        ],
        timestamp_evidence: vec![
            HistoricalSyncTimestampEvidence {
                packet_kind: "raw_motion_k21".to_string(),
                source_signal: "raw_motion_k21".to_string(),
                captured_at: "2026-01-01T20:00:00Z".to_string(),
                sample_time: Some("2026-01-01T22:00:00Z".to_string()),
                sample_time_source: Some("device_timestamp".to_string()),
                device_timestamp_seconds: Some(1_767_304_800),
                device_timestamp_subseconds: Some(0),
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
            HistoricalSyncTimestampEvidence {
                packet_kind: "normal_history".to_string(),
                source_signal: "heart_rate".to_string(),
                captured_at: "2026-01-01T20:00:00Z".to_string(),
                sample_time: Some("2026-01-01T22:05:00Z".to_string()),
                sample_time_source: Some("device_timestamp".to_string()),
                device_timestamp_seconds: Some(1_767_305_100),
                device_timestamp_subseconds: None,
                capture_session_id: Some("strap-capture-2026-01-01".to_string()),
            },
        ],
        raw_evidence_anchors: physical_raw_evidence_anchors(),
    }
}

fn physical_raw_evidence_anchors() -> Vec<HistoricalSyncRawEvidenceAnchor> {
    [
        (
            "notification_subscription",
            "fd4b0003cce1403393ce002d5875f58a",
            None,
        ),
        (
            "notification_subscription",
            "fd4b0004cce1403393ce002d5875f58a",
            None,
        ),
        ("auth_event", "connected", Some(1)),
        ("auth_event", "authenticated", Some(2)),
        ("auth_event", "subscribed", Some(3)),
        ("command_event", "send_historical_data", Some(4)),
        ("metadata_event", "history_start", Some(5)),
        ("metadata_event", "history_end", Some(6)),
        ("metadata_event", "history_complete", Some(7)),
        ("command_event", "historical_data_result", Some(8)),
        ("timestamp_evidence", "raw_motion_k21:raw_motion_k21", None),
        ("timestamp_evidence", "normal_history:heart_rate", None),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (kind, name, sequence))| HistoricalSyncRawEvidenceAnchor {
            evidence_id: format!("physical-raw-evidence-{index}"),
            sha256: format!("{index:064x}"),
            observation_kind: kind.to_string(),
            observation_name: name.to_string(),
            sequence,
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
    )
    .collect()
}

fn step<'a>(
    report: &'a goose_core::historical_sync::HistoricalSyncDryRunReport,
    kind: HistoricalSyncPlanStepKind,
) -> &'a goose_core::historical_sync::HistoricalSyncPlanStep {
    report
        .steps
        .iter()
        .find(|step| step.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} step"))
}

fn load_json_fixture<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[derive(Debug, Deserialize)]
struct CapturedFrameBatchFixture {
    frames: Vec<CapturedFrameInput>,
}

#[derive(Debug, Deserialize)]
struct ActivityFixtureSet {
    activity_sessions: Vec<ActivitySessionFixture>,
}

#[derive(Debug, Deserialize)]
struct ActivitySessionFixture {
    session_id: String,
    source: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    activity_type: String,
    external_activity_type_code: Option<String>,
    external_activity_type_name: Option<String>,
    custom_label: Option<String>,
    confidence: f64,
    detection_method: String,
    sync_status: String,
    provenance_json: serde_json::Value,
}
