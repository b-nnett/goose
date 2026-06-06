use goose_core::{
    algorithm_compare::compare_sleep_v1_goose_to_reference,
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    historical_sync::{
        HistoricalSyncCharacteristicEvidence, HistoricalSyncGeneration,
        HistoricalSyncNotificationEvidence, HistoricalSyncObservedCommand,
        HistoricalSyncObservedEvent, HistoricalSyncPhysicalValidationInput,
        HistoricalSyncRawEvidenceAnchor, HistoricalSyncTimestampEvidence,
        historical_sync_physical_evidence_template, validate_historical_sync_physical_evidence,
    },
    metrics::{
        SleepInput, SleepModelStatusInput, SleepNightHistoryInput, SleepStageSegment, SleepV1Input,
    },
    protocol::{
        DeviceType, PACKET_TYPE_HISTORICAL_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
        build_v5_payload_frame,
    },
    sleep_validation::{
        SleepStageLabelValidationOptions, SleepStageLabelValidationReport,
        SleepV1EvidenceFolderOptions, SleepV1ExplanationStabilityOptions, SleepV1ReleaseGateInput,
        SleepV1ReleaseGateReport, SleepWindowLabelValidationEvidenceInput,
        SleepWindowLabelValidationOptions, run_sleep_window_label_validation_for_store,
        validate_sleep_v1_evidence_folder, validate_sleep_v1_evidence_folder_with_options,
        validate_sleep_v1_explanation_and_stability, validate_sleep_v1_release_gates,
        validate_sleep_v1_stage_labels_for_store,
    },
    store::{GooseStore, SleepCorrectionLabelInput},
};
use std::{collections::BTreeMap, fs};

#[test]
fn sleep_v1_explanation_stability_validation_passes_complete_stable_output() {
    let report = validate_sleep_v1_explanation_and_stability(
        &sleep_v1_quality_gate_input(),
        SleepV1ExplanationStabilityOptions::default(),
    );

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.explanation_pass);
    assert!(report.explanation_quality_pass);
    assert!(report.repeated_run_stability_pass);
    assert!(report.perturbation_stability_pass);
    assert_eq!(report.v0_component_count, 7);
    assert_eq!(report.v1_component_count, 7);
    assert_eq!(
        report.v1_component_names,
        vec![
            "sleep_need_fulfillment",
            "continuity",
            "schedule_regularity",
            "sleep_architecture",
            "cardiovascular_recovery",
            "context_adjustment",
            "data_confidence",
        ]
    );
    assert_eq!(
        report.provenance["v1_component_names"],
        serde_json::json!(report.v1_component_names)
    );
    assert_eq!(
        report.provenance["required_component_inputs"]["sleep_architecture"],
        serde_json::json!([
            "stage_minutes",
            "stage_segment_count",
            "stage_segment_confidence_0_to_1",
            "sleep_architecture_confidence_0_to_1",
            "stage_prior_calibration",
        ])
    );
    assert!(report.missing_component_provenance.is_empty());
    assert!(report.missing_component_inputs.is_empty());
    assert!(report.missing_component_policy.is_empty());
    assert_eq!(report.explanation_quality_signal_count, 8);
    assert_eq!(
        report.acceptance_summary.policy,
        "sleep_v1_explanation_components_signals_and_stability_must_match_release_contract"
    );
    assert!(report.acceptance_summary.explanation_and_stability_ready);
    assert_eq!(
        report.acceptance_summary.observed_component_names,
        report.v1_component_names
    );
    assert_eq!(
        report.acceptance_summary.explanation_quality_signal_count,
        report.explanation_quality_signal_count
    );
    assert_eq!(report.acceptance_summary.issue_count, 0);
    assert_eq!(report.acceptance_summary.quality_flag_count, 0);
    assert_eq!(report.acceptance_summary.error_count, 0);
    assert_eq!(report.acceptance_summary.next_action_count, 0);
    assert_eq!(
        report.explanation_quality_signals,
        vec![
            "model_status_label_and_reason",
            "score_visibility_gate",
            "previous_night_comparison",
            "component_breakdown_with_provenance",
            "stage_prior_calibration",
            "cardiovascular_recovery_context",
            "confidence_and_window_quality",
            "why_changed_and_score_policy_provenance",
        ]
    );
    assert!(
        report
            .sleep_window_confidence_0_to_1
            .is_some_and(|confidence| confidence > 0.0 && confidence <= 1.0)
    );
    assert!(
        report
            .perturbed_sleep_window_confidence_0_to_1
            .is_some_and(|confidence| confidence > 0.0 && confidence <= 1.0)
    );
    assert_eq!(report.repeated_run_delta, Some(0.0));
    assert!(report.small_perturbation_delta.unwrap() <= 5.0);
}

#[test]
fn sleep_v1_explanation_stability_reports_weak_explanation_quality_gate() {
    let report = validate_sleep_v1_explanation_and_stability(
        &sleep_v1_quality_gate_input(),
        SleepV1ExplanationStabilityOptions {
            min_explanation_quality_signal_count: 9,
            ..Default::default()
        },
    );

    assert!(!report.pass);
    assert!(report.explanation_pass);
    assert!(!report.explanation_quality_pass);
    assert!(
        report
            .issues
            .contains(&"sleep_v1_explanation_quality_signal_count_below_gate".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_explanation_quality_signal_count_below_gate"
            && action.scope == "sleep_v1.explanation_quality"
    }));
}

#[test]
fn sleep_v1_explanation_stability_validation_reports_unstable_perturbation_gate() {
    let report = validate_sleep_v1_explanation_and_stability(
        &sleep_v1_quality_gate_input(),
        SleepV1ExplanationStabilityOptions {
            max_small_perturbation_delta: 0.01,
            ..Default::default()
        },
    );

    assert!(!report.pass);
    assert!(report.explanation_pass);
    assert!(report.repeated_run_stability_pass);
    assert!(!report.perturbation_stability_pass);
    assert!(
        report
            .issues
            .contains(&"sleep_v1_small_perturbation_delta_exceeds_threshold".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "sleep_v1_small_perturbation_delta_exceeds_threshold")
    );
}

#[test]
fn sleep_v1_explanation_stability_validation_blocks_quality_flags() {
    let mut input = sleep_v1_quality_gate_input();
    input.sleep.stage_minutes.clear();
    input.stage_segments.clear();

    let report = validate_sleep_v1_explanation_and_stability(
        &input,
        SleepV1ExplanationStabilityOptions::default(),
    );

    assert!(!report.pass);
    assert!(!report.explanation_pass);
    assert!(
        report
            .quality_flags
            .contains(&"sleep_architecture_unavailable".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_v1_quality_flags_present".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_quality_flags_present" && action.scope == "sleep_v1.quality"
    }));
}

#[test]
fn sleep_v1_explanation_stability_blocks_future_prior_history_flag() {
    let mut input = sleep_v1_quality_gate_input();
    input.prior_nights = vec![
        sleep_v1_prior_night(
            "sleep-history-0",
            "2026-05-25T22:30:00Z",
            "2026-05-26T06:30:00Z",
        ),
        sleep_v1_prior_night(
            "future-history-night",
            "2026-05-28T23:00:00Z",
            "2026-05-29T06:30:00Z",
        ),
    ];

    let report = validate_sleep_v1_explanation_and_stability(
        &input,
        SleepV1ExplanationStabilityOptions::default(),
    );

    assert!(!report.pass);
    assert!(!report.explanation_pass);
    assert!(
        report
            .quality_flags
            .contains(&"sleep_v1_future_prior_nights_ignored".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_v1_quality_flags_present".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_quality_flags_present" && action.scope == "sleep_v1.quality"
    }));
}

#[test]
fn sleep_v1_stability_validator_cli_reports_complete_stable_output() {
    let tempdir = tempfile::tempdir().unwrap();
    let input_path = tempdir.path().join("sleep-v1-input.json");
    let output_path = tempdir.path().join("sleep-v1-stability.json");
    write_json(&input_path, &sleep_v1_quality_gate_input());

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-v1-stability-validator"))
            .args([
                "--input",
                input_path.to_str().unwrap(),
                "--output",
                output_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        report["schema"],
        "goose.sleep-v1-explanation-stability-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["explanation_pass"], true);
    assert_eq!(report["repeated_run_stability_pass"], true);
    assert_eq!(report["perturbation_stability_pass"], true);
}

#[test]
fn sleep_v1_release_gate_validation_passes_only_when_all_evidence_reports_pass() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    for (index, sleep_id) in [
        "packet-derived-sleep-2026-05-27",
        "packet-derived-sleep-2026-05-28",
        "packet-derived-sleep-2026-05-29",
    ]
    .into_iter()
    .enumerate()
    {
        insert_sleep_window_label_with_sleep_id(
            &store,
            &format!("manual-reviewed-window-pass-{index}"),
            sleep_id,
            1_779_919_800_000,
            1_779_933_000_000,
        );
    }
    let sleep_window_label_validation = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();
    seed_release_gate_sleep_stage_labels(&store);
    let sleep_stage_label_validation = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();

    let input = SleepV1ReleaseGateInput {
        physical_historical_sync: Some(validate_historical_sync_physical_evidence(
            &physical_validation_input(),
        )),
        sleep_window_label_validation: Some(sleep_window_label_validation),
        sleep_stage_label_validation: Some(sleep_stage_label_validation),
        explanation_stability: Some(validate_sleep_v1_explanation_and_stability(
            &sleep_v1_quality_gate_input(),
            SleepV1ExplanationStabilityOptions::default(),
        )),
        benchmark_comparisons: vec![
            compare_sleep_v1_goose_to_reference(&sleep_v1_quality_gate_input()).unwrap(),
        ],
        min_hand_reviewed_window_comparisons: 3,
        min_stage_label_comparisons: 1,
        min_benchmark_comparisons: 1,
    };

    let report = validate_sleep_v1_release_gates(&input);

    let benchmark_acceptance = input.benchmark_comparisons[0]
        .acceptance_summary
        .as_ref()
        .unwrap();
    assert_eq!(benchmark_acceptance["pass"], true);
    assert_eq!(benchmark_acceptance["reference_contract_valid"], true);
    assert_eq!(benchmark_acceptance["goose_output_ready"], true);
    assert_eq!(benchmark_acceptance["reference_output_ready"], true);
    assert_eq!(benchmark_acceptance["shared_fields_ready"], true);
    assert_eq!(benchmark_acceptance["issue_count"], 0);
    assert_eq!(benchmark_acceptance["error_count"], 0);
    assert_eq!(benchmark_acceptance["next_action_count"], 0);

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.physical_historical_sync_pass);
    assert!(report.timestamp_evidence_pass);
    assert!(report.sleep_window_label_pass);
    assert!(report.sleep_stage_label_pass);
    assert!(report.explanation_stability_pass);
    assert!(report.benchmark_comparison_pass);
    let window_acceptance = input
        .sleep_window_label_validation
        .as_ref()
        .unwrap()
        .acceptance_summary
        .clone();
    assert_eq!(window_acceptance.issue_count, 0);
    assert_eq!(window_acceptance.next_action_count, 0);
    let stage_acceptance = input
        .sleep_stage_label_validation
        .as_ref()
        .unwrap()
        .acceptance_summary
        .clone();
    assert_eq!(stage_acceptance.issue_count, 0);
    assert_eq!(stage_acceptance.next_action_count, 0);
    assert_eq!(report.hand_reviewed_window_comparisons, 3);
    assert_eq!(report.stage_label_comparison_count, 2);
    assert_eq!(report.benchmark_comparison_count, 1);
    assert_eq!(
        report.acceptance_summary.policy,
        "sleep_v1_release_gate_must_match_current_subgate_threshold_and_proof_contract"
    );
    assert!(report.acceptance_summary.release_ready);
    assert_eq!(
        report.acceptance_summary.hand_reviewed_window_comparisons,
        3
    );
    assert_eq!(report.acceptance_summary.stage_label_comparison_count, 2);
    assert_eq!(report.acceptance_summary.benchmark_comparison_count, 1);
    assert_eq!(
        report
            .provenance
            .get("report_integrity_policy")
            .and_then(serde_json::Value::as_str),
        Some("sleep_v1_release_gate_requires_current_subgate_integrity_and_empty_proof_arrays")
    );
    assert_eq!(
        report
            .provenance
            .get("threshold_policy")
            .and_then(serde_json::Value::as_str),
        Some(
            "sleep_v1_primary_release_uses_default_or_stricter_review_stage_and_benchmark_thresholds"
        )
    );
    assert_eq!(
        report
            .provenance
            .get("subgate_report_integrity_policies")
            .and_then(|policies| policies.get("historical-sync-validation.json"))
            .and_then(serde_json::Value::as_str),
        Some(goose_core::historical_sync::HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY)
    );
    assert_eq!(
        report
            .provenance
            .get("subgate_report_integrity_policies")
            .and_then(|policies| policies.get("sleep-v1-benchmark.json"))
            .and_then(serde_json::Value::as_str),
        Some("sleep_v1_benchmark_requires_current_comparison_output_and_delta_integrity")
    );
    assert_eq!(
        report
            .provenance
            .get("subgate_report_validation_policies")
            .and_then(|policies| policies.get("historical-sync-validation.json"))
            .and_then(serde_json::Value::as_str),
        Some(
            "service_characteristics_notifications_auth_commands_event_order_and_timestamp_fields"
        )
    );
    assert_eq!(
        report
            .provenance
            .get("subgate_report_validation_policies")
            .and_then(|policies| policies.get("sleep-v1-benchmark.json"))
            .and_then(serde_json::Value::as_str),
        Some(goose_core::algorithm_compare::SLEEP_V1_BENCHMARK_COMPARISON_POLICY)
    );
    let benchmark_coverage = input.benchmark_comparisons[0]
        .data_coverage
        .as_ref()
        .and_then(|coverage| coverage.get("goose_output_data_coverage_fraction"))
        .and_then(serde_json::Value::as_f64)
        .unwrap();
    assert!((0.0..=1.0).contains(&benchmark_coverage));

    let mut motion_timestamp_evidence = physical_validation_input();
    motion_timestamp_evidence.timestamp_evidence[0].sample_time =
        Some("2026-01-01T22:00:01Z".to_string());
    let mut motion_timestamp_input = input.clone();
    motion_timestamp_input.physical_historical_sync = Some(
        validate_historical_sync_physical_evidence(&motion_timestamp_evidence),
    );

    let motion_timestamp_report = validate_sleep_v1_release_gates(&motion_timestamp_input);

    assert!(!motion_timestamp_report.pass);
    assert!(!motion_timestamp_report.physical_historical_sync_pass);
    assert!(!motion_timestamp_report.timestamp_evidence_pass);
    assert!(
        motion_timestamp_report
            .issues
            .contains(&"historical_motion_timestamp_fields_unproven".to_string())
    );
    assert!(
        !motion_timestamp_report
            .issues
            .contains(&"historical_heart_rate_timestamp_fields_unproven".to_string())
    );
    assert!(motion_timestamp_report.next_actions.iter().any(|action| {
        action.reason == "historical_motion_timestamp_fields_unproven"
            && action.scope == "historical_sync.timestamps"
            && action.action.contains("historical motion packets")
    }));

    let mut lowered_threshold_input = input.clone();
    lowered_threshold_input.min_hand_reviewed_window_comparisons = 1;

    let lowered_threshold_report = validate_sleep_v1_release_gates(&lowered_threshold_input);

    assert!(!lowered_threshold_report.pass);
    assert!(lowered_threshold_report.sleep_window_label_pass);
    assert!(
        lowered_threshold_report
            .issues
            .contains(&"release_gate_hand_reviewed_window_threshold_below_default".to_string())
    );
    assert!(lowered_threshold_report.next_actions.iter().any(|action| {
        action.reason == "release_gate_hand_reviewed_window_threshold_below_default"
            && action.scope == "sleep_window.labels"
    }));

    let mut lowered_benchmark_input = input.clone();
    lowered_benchmark_input.min_benchmark_comparisons = 0;

    let lowered_benchmark_report = validate_sleep_v1_release_gates(&lowered_benchmark_input);

    assert!(!lowered_benchmark_report.pass);
    assert!(lowered_benchmark_report.benchmark_comparison_pass);
    assert!(
        lowered_benchmark_report
            .issues
            .contains(&"release_gate_benchmark_threshold_below_default".to_string())
    );
    assert!(lowered_benchmark_report.next_actions.iter().any(|action| {
        action.reason == "release_gate_benchmark_threshold_below_default"
            && action.scope == "sleep_v1.benchmark"
    }));

    let mut missing_stage_input = input.clone();
    missing_stage_input.sleep_stage_label_validation = None;

    let missing_stage_report = validate_sleep_v1_release_gates(&missing_stage_input);

    assert!(!missing_stage_report.pass);
    assert!(!missing_stage_report.sleep_stage_label_pass);
    assert!(
        missing_stage_report
            .issues
            .contains(&"sleep_stage_label_report_missing".to_string())
    );
    assert!(missing_stage_report.next_actions.iter().any(|action| {
        action.reason == "sleep_stage_label_report_missing" && action.scope == "sleep_stage.labels"
    }));

    let mut forged_input = input.clone();
    let mut forged_stage = forged_input.sleep_stage_label_validation.take().unwrap();
    forged_stage
        .acceptance_summary
        .accepted_label_ids
        .push("hand-edited-stage-label".to_string());
    forged_stage.pass = true;
    forged_input.sleep_stage_label_validation = Some(forged_stage);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_stage_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_stage_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stage = forged_input.sleep_stage_label_validation.take().unwrap();
    forged_stage.comparisons = vec![
        forged_stage.comparisons[0].clone(),
        forged_stage.comparisons[0].clone(),
    ];
    forged_stage.label_count = forged_stage.comparisons.len();
    forged_stage.compared_label_count = forged_stage.comparisons.len();
    forged_stage.passing_label_count = forged_stage.comparisons.len();
    forged_stage.acceptance_summary.accepted_label_ids = forged_stage
        .comparisons
        .iter()
        .map(|comparison| comparison.label_id.clone())
        .collect();
    forged_stage.acceptance_summary.accepted_stage_kinds = vec!["deep".to_string()];
    forged_stage.pass = true;
    forged_input.sleep_stage_label_validation = Some(forged_stage);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert_eq!(forged_report.stage_label_comparison_count, 1);
    assert!(!forged_report.sleep_stage_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_stage_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stage = forged_input.sleep_stage_label_validation.take().unwrap();
    forged_stage
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    forged_stage.pass = true;
    forged_input.sleep_stage_label_validation = Some(forged_stage);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_stage_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_stage_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical.evidence_session_confirmed = false;
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_not_validated".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical.generated_by = "hand-edited".to_string();
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );
    assert!(
        forged_report
            .issues
            .contains(&"historical_motion_hr_timestamps_not_proven".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical.event_order_confirmed = false;
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical
        .next_actions
        .push(goose_core::historical_sync::HistoricalSyncNextAction {
            scope: "historical_sync_physical_validation".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited physical proof actions.".to_string(),
        });
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical
        .quality_flags
        .push("hand_edited_physical_quality_flag".to_string());
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical
        .errors
        .push("hand_edited_physical_error".to_string());
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_physical = forged_input.physical_historical_sync.take().unwrap();
    forged_physical.acceptance_summary.issue_count += 1;
    forged_physical.pass = true;
    forged_input.physical_historical_sync = Some(forged_physical);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.physical_historical_sync_pass);
    assert!(!forged_report.timestamp_evidence_pass);
    assert!(
        forged_report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.compared_label_count += 1;
    forged_window.passing_label_count += 1;
    forged_window.distinct_compared_sleep_window_count += 1;
    forged_window.distinct_passing_sleep_window_count += 1;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .acceptance_summary
        .min_observed_label_confidence = 0.0;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.sleep_feature_report.generated_by =
        "hand-edited-sleep-feature-report".to_string();
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.sleep_feature_report.require_trusted_evidence = false;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .sleep_feature_report
        .sleep_input
        .as_mut()
        .unwrap()
        .sleep_duration_minutes += 15.0;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .sleep_feature_report
        .score_result
        .as_mut()
        .unwrap()
        .start_time = "2026-05-27T21:00:00Z".to_string();
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .sleep_feature_report
        .sleep_window
        .as_mut()
        .unwrap()
        .heart_rate_feature_count = 0;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.label_count += 1;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .acceptance_summary
        .accepted_sleep_ids
        .push("hand-edited-sleep-id".to_string());
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.comparisons[0].start_delta_minutes += 10.0;
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.comparisons[0].expected_start_time = "2026-05-27T21:59:00Z".to_string();
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.next_actions.push(
        goose_core::sleep_validation::SleepWindowLabelValidationNextAction {
            scope: "sleep_window_validation".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited sleep-window actions.".to_string(),
        },
    );
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .quality_flags
        .push("hand_edited_sleep_window_quality_flag".to_string());
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .errors
        .push("hand_edited_sleep_window_error".to_string());
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    forged_input.sleep_window_label_validation = Some(
        run_sleep_window_label_validation_for_store(
            &store,
            "test-db",
            "2026-05-27T22:00:00Z",
            "2026-05-28T03:00:00Z",
            SleepWindowLabelValidationOptions {
                min_owned_captures_per_summary: 1,
                require_trusted_evidence: true,
                sleep_need_minutes: 240.0,
                start_tolerance_minutes: 120.0,
                end_tolerance_minutes: 120.0,
                duration_tolerance_minutes: 180.0,
                min_label_confidence: 0.10,
                ..Default::default()
            },
        )
        .unwrap(),
    );

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_threshold_below_default".to_string())
    );
    assert!(forged_report.next_actions.iter().any(|action| {
        action.reason == "sleep_window_label_threshold_below_default"
            && action.scope == "sleep_window.labels"
    }));

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.comparisons[0].label_id = " ".to_string();
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.comparisons[0].source = " ".to_string();
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.provenance["comparison_policy"] =
        serde_json::Value::String("hand_edited_policy".to_string());
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.provenance["start_tolerance_minutes"] = serde_json::Value::from(-1.0);
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_window = forged_input.sleep_window_label_validation.take().unwrap();
    forged_window.comparisons[0].confidence = Some(1.5);
    forged_window.pass = true;
    forged_input.sleep_window_label_validation = Some(forged_window);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.sleep_window_label_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.v1_component_count += 1;
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .acceptance_summary
        .observed_component_names
        .push("hand_edited_stability_component".to_string());
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.acceptance_summary.issue_count += 1;
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("required_component_inputs");
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.provenance["required_component_inputs"]["sleep_architecture"]
        .as_array_mut()
        .unwrap()
        .retain(|value| value.as_str() != Some("stage_prior_calibration"));
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.v1_component_names[0] = forged_stability.v1_component_names[1].clone();
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.v1_component_names.swap(0, 1);
    forged_stability.provenance["v1_component_names"] =
        serde_json::json!(forged_stability.v1_component_names.clone());
    forged_stability.acceptance_summary.observed_component_names =
        forged_stability.v1_component_names.clone();
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.provenance["v1_component_names"] = serde_json::json!([
        "sleep_need_fulfillment",
        "continuity",
        "schedule_regularity",
        "sleep_architecture",
        "cardiovascular_recovery",
        "context_adjustment",
        "legacy_sensor_reliability",
    ]);
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.explanation_quality_signals[7] = "legacy_generic_explanation_copy".to_string();
    forged_stability.provenance["explanation_quality_signals"] =
        serde_json::json!(forged_stability.explanation_quality_signals.clone());
    forged_stability
        .acceptance_summary
        .explanation_quality_signals = forged_stability.explanation_quality_signals.clone();
    forged_stability.pass = true;
    forged_stability.explanation_quality_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.provenance["perturbed_score_0_to_100"] = serde_json::json!(101.0);
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .errors
        .push("hand_edited_stability_error".to_string());
    forged_stability.pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.provenance["v1_component_count"] = serde_json::json!(999);
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.next_actions.push(
        goose_core::sleep_validation::SleepV1ExplanationStabilityNextAction {
            scope: "sleep_v1.stability".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited stability reports.".to_string(),
        },
    );
    forged_stability.pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .quality_flags
        .push("sleep_architecture_unavailable".to_string());
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability
        .v1_component_names
        .push("legacy_extra_component".to_string());
    forged_stability.v1_component_count = 8;
    forged_stability.provenance["v1_component_count"] = serde_json::json!(8);
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.sleep_window_confidence_0_to_1 = Some(1.5);
    forged_stability.pass = true;
    forged_stability.explanation_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.small_perturbation_delta = Some(99.0);
    forged_stability.pass = true;
    forged_stability.perturbation_stability_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.repeated_run_delta = Some(-1.0);
    forged_stability.pass = true;
    forged_stability.repeated_run_stability_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_stability = forged_input.explanation_stability.take().unwrap();
    forged_stability.small_perturbation_delta = Some(-1.0);
    forged_stability.pass = true;
    forged_stability.perturbation_stability_pass = true;
    forged_stability.issues.clear();
    forged_input.explanation_stability = Some(forged_stability);

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    forged_input.explanation_stability = Some(validate_sleep_v1_explanation_and_stability(
        &sleep_v1_quality_gate_input(),
        SleepV1ExplanationStabilityOptions {
            max_small_perturbation_delta: 99.0,
            ..Default::default()
        },
    ));

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.explanation_stability_pass);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_explanation_stability_threshold_below_default".to_string())
    );
    assert!(forged_report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_explanation_stability_threshold_below_default"
            && action.scope == "sleep_v1.explanation_stability"
    }));

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.goose_algorithm_id = "goose.sleep.v0".to_string();
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark
        .provenance
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.acceptance_summary = Some(serde_json::json!({
        "policy": "sleep_v1_benchmark_must_match_reference_contract_deltas_and_embedded_output",
        "pass": true,
        "benchmark_ready": true,
        "reference_contract_valid": true,
        "goose_output_ready": true,
        "reference_output_ready": true,
        "shared_fields_ready": true,
        "goose_algorithm_id": "goose.sleep.v1",
        "goose_algorithm_version": "0.1.0",
        "reference_algorithm_id": "reference.sleep.actigraphy.v1",
        "reference_algorithm_version": "1.0.0",
        "start_time": forged_benchmark.start_time.clone(),
        "end_time": forged_benchmark.end_time.clone(),
        "comparable_fields": forged_benchmark.comparable_fields.clone(),
        "delta_count": 999,
        "non_comparable_field_count": forged_benchmark.non_comparable_fields.len(),
        "data_coverage_fraction": 0.90,
        "goose_quality_flag_count": 0,
        "reference_quality_flag_count": 0,
        "quality_flag_count": 0,
        "error_count": 0,
    }));
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark
        .issues
        .push("hand_edited_benchmark_issue".to_string());
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.next_actions.push(
        goose_core::algorithm_compare::AlgorithmComparisonNextAction {
            scope: "comparison".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited benchmark actions.".to_string(),
        },
    );
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.reference_algorithm_id = "fake.sleep.reference".to_string();
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.data_coverage = None;
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.data_coverage = Some(serde_json::json!({
        "goose_output_data_coverage_fraction": 1.5,
    }));
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark
        .quality_flags
        .push("manual_warning_hidden_by_pass".to_string());
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]
            .as_object_mut()
            .unwrap()
            .remove("calibration_label_count");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"][0]["name"] = serde_json::json!("sleep need fulfillment");
        let component_provenance = goose_output["component_provenance"]
            .as_object_mut()
            .unwrap();
        let sleep_need_provenance = component_provenance
            .remove("sleep_need_fulfillment")
            .unwrap();
        component_provenance.insert("sleep need fulfillment".to_string(), sleep_need_provenance);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": "legacy_extra_component",
                "value": 1.0,
                "unit": "debug",
                "score_0_to_100": 0.0,
                "weight": 0.0,
                "contribution": 0.0
            }));
        goose_output["component_provenance"]
            .as_object_mut()
            .unwrap()
            .insert(
                "legacy_extra_component".to_string(),
                serde_json::json!({
                    "inputs": {"legacy_value": 1.0},
                    "policy": "legacy_extra_component"
                }),
            );
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["component_provenance"]["continuity"]["policy"] =
            serde_json::json!("awake_time_and_waso_continuity");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["component_provenance"]["sleep_architecture"]["inputs"]
            .as_object_mut()
            .unwrap()
            .remove("stage_prior_calibration");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["component_provenance"]["data_confidence"]["policy"] =
            serde_json::json!("combined_sleep_v1_confidence_and_coverage");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["component_provenance"]["data_confidence"]["inputs"]
            .as_object_mut()
            .unwrap()
            .remove("sleep_window_confidence_0_to_1");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|component| component["name"] == "data_confidence")
            .unwrap()["name"] = serde_json::json!("sensor_reliability");
        let provenance = goose_output["component_provenance"]
            .as_object_mut()
            .unwrap();
        let data_confidence = provenance.remove("data_confidence").unwrap();
        provenance.insert("sensor_reliability".to_string(), data_confidence);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["model_status"] = serde_json::json!("hand_edited_status");
        goose_output["model_status_label"] = serde_json::json!("Hand edited");
        goose_output["model_status_reason"] = serde_json::json!("This is not a Sleep V1 status.");
        goose_output["status_report"]["status"] = serde_json::json!("hand_edited_status");
        goose_output["status_report"]["status_label"] = serde_json::json!("Hand edited");
        goose_output["status_report"]["status_reason"] =
            serde_json::json!("This is not a Sleep V1 status.");
        goose_output["status_report"]["report_state"] = serde_json::json!("provisional");
        goose_output["status_report"]["can_show_final_score"] = serde_json::json!(false);
        goose_output["status_report"]["can_show_personal_baseline"] = serde_json::json!(false);
        goose_output["status_report"]["can_show_trained_score"] = serde_json::json!(false);
        goose_output["status_report"]["next_actions"] =
            serde_json::json!(["Regenerate the Sleep V1 status report."]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["nights_until_training"] = serde_json::json!(99);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["excluded_sleep_nights"] = serde_json::json!(-1);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["model_status"] = serde_json::json!("training");
        goose_output["model_status_label"] = serde_json::json!("Training");
        goose_output["model_status_reason"] =
            serde_json::json!("Goose is training a personal sleep model.");
        goose_output["status_report"]["status"] = serde_json::json!("training");
        goose_output["status_report"]["status_label"] = serde_json::json!("Training");
        goose_output["status_report"]["status_reason"] =
            serde_json::json!("Goose is training a personal sleep model.");
        goose_output["status_report"]["calibration_label_count"] = serde_json::json!(14);
        goose_output["status_report"]["nights_until_training"] = serde_json::json!(0);
        goose_output["status_report"]["next_actions"] =
            serde_json::json!(["Collect 4 more Goose packet-derived sleep nights."]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["can_show_final_score"] = serde_json::json!(false);
        goose_output["status_report"]["can_show_provisional_score"] = serde_json::json!(false);
        goose_output["status_report"]["report_state"] = serde_json::json!("pending");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["next_actions"] = serde_json::json!([]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["provenance"]["score_policy"] = serde_json::json!("hand-edited-policy");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["component_provenance"]["sleep_need_fulfillment"]["inputs"] =
            serde_json::json!({});
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"][0]["contribution"] = serde_json::json!(999.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["score_0_to_100"] = serde_json::json!(99.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["stage_minutes"]["awake"] = serde_json::json!(999.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["next_actions"] = serde_json::json!([""]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["model_status"] = serde_json::json!("learning");
        goose_output["model_status_label"] = serde_json::json!("Learning");
        goose_output["model_status_reason"] =
            serde_json::json!("3 valid sleep nights collected; 4 more for baseline.");
        goose_output["status_report"]["status"] = serde_json::json!("learning");
        goose_output["status_report"]["status_label"] = serde_json::json!("Learning");
        goose_output["status_report"]["status_reason"] =
            serde_json::json!("3 valid sleep nights collected; 4 more for baseline.");
        goose_output["status_report"]["report_state"] = serde_json::json!("provisional");
        goose_output["status_report"]["valid_sleep_nights"] = serde_json::json!(3);
        goose_output["status_report"]["trusted_goose_sleep_nights"] = serde_json::json!(3);
        goose_output["status_report"]["imported_platform_sleep_nights"] = serde_json::json!(0);
        goose_output["status_report"]["nights_until_baseline"] = serde_json::json!(4);
        goose_output["status_report"]["can_show_final_score"] = serde_json::json!(false);
        goose_output["status_report"]["can_show_personal_baseline"] = serde_json::json!(false);
        goose_output["status_report"]["next_actions"] = serde_json::json!([]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["quality_flags"] =
            serde_json::json!(["hand_edited_status_quality_flag"]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["valid_sleep_nights"] =
            serde_json::json!((u32::MAX as u64) + 1);
        goose_output["status_report"]["trusted_goose_sleep_nights"] =
            serde_json::json!((u32::MAX as u64) + 1);
        goose_output["status_report"]["imported_platform_sleep_nights"] = serde_json::json!(0);
        goose_output["status_report"]["nights_until_baseline"] = serde_json::json!(0);
        goose_output["status_report"]["nights_until_goose_training"] = serde_json::json!(0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["quality_flags"] = serde_json::json!(["sleep_architecture_unavailable"]);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["provenance"] = serde_json::json!({});
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"] = serde_json::json!([
            {
                "name": "only_component",
                "value": 420.0,
                "unit": "minutes",
                "score_0_to_100": 87.5,
                "weight": 0.25,
                "contribution": 21.875
            }
        ]);
        goose_output["component_provenance"] = serde_json::json!({
            "only_component": {
                "inputs": {"sleep_duration_minutes": 420.0},
                "policy": "too_few_components_for_release_benchmark"
            }
        });
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output
            .as_object_mut()
            .unwrap()
            .remove("component_provenance");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["components"][0]["name"] = serde_json::json!("");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output
            .as_object_mut()
            .unwrap()
            .remove("status_report");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output
            .as_object_mut()
            .unwrap()
            .remove("previous_night_comparison");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["report_state"] = serde_json::json!("final");
        goose_output["status_report"]["can_show_final_score"] = serde_json::json!(false);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["status_report"]["valid_sleep_nights"] = serde_json::json!(999);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["sleep_duration_minutes"] = serde_json::json!(999.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["confidence_0_to_1"] = serde_json::json!(1.5);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output
            .as_object_mut()
            .unwrap()
            .remove("data_coverage_fraction");
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["stage_segment_confidence_0_to_1"] = serde_json::json!(1.5);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["stage_minutes"]["deep"] = serde_json::json!(999.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["stage_minutes"]["unknown"] = serde_json::json!(0.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["previous_night_comparison"] = serde_json::json!({
            "night_id": "",
            "sleep_duration_delta_minutes": "not-a-number",
            "sleep_efficiency_delta_fraction": 0.1,
            "restorative_sleep_delta_minutes": 20.0,
            "bedtime_deviation_delta_minutes": 5.0,
            "wake_time_deviation_delta_minutes": -5.0,
            "sleep_hr_average_delta_bpm": null,
            "sleep_hr_trend_delta_bpm_per_hour": null
        });
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(goose_output) = forged_benchmark.goose_output.as_mut() {
        goose_output["previous_night_comparison"] = serde_json::json!({
            "night_id": "sleep-history-6",
            "sleep_duration_delta_minutes": 20.0,
            "awake_minutes_delta": -10.0,
            "sleep_debt_delta_minutes": -20.0,
            "sleep_efficiency_delta_fraction": 0.03,
            "sleep_latency_delta_minutes": -4.0,
            "wake_after_sleep_onset_delta_minutes": -6.0,
            "wake_episode_count_delta": -1,
            "deep_sleep_delta_minutes": 5.0,
            "rem_sleep_delta_minutes": 10.0,
            "core_sleep_delta_minutes": 5.0,
            "restorative_sleep_delta_minutes": 15.0,
            "bedtime_deviation_delta_minutes": -8.0,
            "wake_time_deviation_delta_minutes": 6.0,
            "sleep_hr_average_delta_bpm": 1.0,
            "sleep_hr_min_delta_bpm": -1.0,
            "sleep_hr_trend_delta_bpm_per_hour": -0.2,
            "sleep_hr_dip_delta_percent": 1.5
        });
        goose_output["provenance"]["previous_night_comparison"] = serde_json::json!({
            "policy": "latest_usable_prior_night_before_scored_sleep",
            "selected_night_id": "different-night",
            "usable_prior_night_count": 7,
            "fields": [
                "sleep_duration_delta_minutes",
                "awake_minutes_delta",
                "sleep_debt_delta_minutes",
                "sleep_efficiency_delta_fraction",
                "sleep_latency_delta_minutes",
                "wake_after_sleep_onset_delta_minutes",
                "wake_episode_count_delta",
                "deep_sleep_delta_minutes",
                "rem_sleep_delta_minutes",
                "core_sleep_delta_minutes",
                "restorative_sleep_delta_minutes",
                "bedtime_deviation_delta_minutes",
                "wake_time_deviation_delta_minutes",
                "sleep_hr_average_delta_bpm",
                "sleep_hr_min_delta_bpm",
                "sleep_hr_trend_delta_bpm_per_hour",
                "sleep_hr_dip_delta_percent"
            ]
        });
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    if let Some(reference_output) = forged_benchmark.reference_output.as_mut() {
        reference_output["sleep_minutes"] = serde_json::json!(999.0);
    }
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.provenance["goose_comparable_inputs"]["disturbance_count"] =
        serde_json::json!(999.0);
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.deltas[0].absolute_delta += 10.0;
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );

    let mut forged_input = input.clone();
    let mut forged_benchmark = forged_input.benchmark_comparisons.remove(0);
    forged_benchmark.generated_by = "hand-edited-benchmark".to_string();
    forged_benchmark.pass = true;
    forged_input.benchmark_comparisons = vec![forged_benchmark];

    let forged_report = validate_sleep_v1_release_gates(&forged_input);

    assert!(!forged_report.pass);
    assert!(!forged_report.benchmark_comparison_pass);
    assert_eq!(forged_report.benchmark_comparison_count, 0);
    assert!(
        forged_report
            .issues
            .contains(&"sleep_v1_benchmark_report_integrity_failed".to_string())
    );
}

#[test]
fn sleep_v1_release_gate_default_requires_multiple_hand_reviewed_nights() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    insert_sleep_window_label(
        &store,
        "manual-reviewed-window-pass",
        1_779_919_800_000,
        1_779_933_000_000,
    );
    let sleep_window_label_validation = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();
    seed_release_gate_sleep_stage_labels(&store);
    let sleep_stage_label_validation = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();

    let input = SleepV1ReleaseGateInput {
        physical_historical_sync: Some(validate_historical_sync_physical_evidence(
            &physical_validation_input(),
        )),
        sleep_window_label_validation: Some(sleep_window_label_validation),
        sleep_stage_label_validation: Some(sleep_stage_label_validation),
        explanation_stability: Some(validate_sleep_v1_explanation_and_stability(
            &sleep_v1_quality_gate_input(),
            SleepV1ExplanationStabilityOptions::default(),
        )),
        benchmark_comparisons: vec![
            compare_sleep_v1_goose_to_reference(&sleep_v1_quality_gate_input()).unwrap(),
        ],
        ..Default::default()
    };

    let report = validate_sleep_v1_release_gates(&input);

    assert!(!report.pass);
    assert_eq!(input.min_hand_reviewed_window_comparisons, 3);
    assert_eq!(report.hand_reviewed_window_comparisons, 1);
    assert!(
        report
            .issues
            .contains(&"hand_reviewed_sleep_window_sample_below_gate".to_string())
    );
}

#[test]
fn sleep_v1_release_gate_counts_distinct_hand_reviewed_windows() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    for index in 0..3 {
        insert_sleep_window_label_with_sleep_id(
            &store,
            &format!("manual-reviewed-window-duplicate-{index}"),
            "packet-derived-sleep-2026-05-27",
            1_779_919_800_000,
            1_779_933_000_000,
        );
    }
    let sleep_window_label_validation = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();
    seed_release_gate_sleep_stage_labels(&store);
    let sleep_stage_label_validation = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();

    let input = SleepV1ReleaseGateInput {
        physical_historical_sync: Some(validate_historical_sync_physical_evidence(
            &physical_validation_input(),
        )),
        sleep_window_label_validation: Some(sleep_window_label_validation),
        sleep_stage_label_validation: Some(sleep_stage_label_validation),
        explanation_stability: Some(validate_sleep_v1_explanation_and_stability(
            &sleep_v1_quality_gate_input(),
            SleepV1ExplanationStabilityOptions::default(),
        )),
        benchmark_comparisons: vec![
            compare_sleep_v1_goose_to_reference(&sleep_v1_quality_gate_input()).unwrap(),
        ],
        ..Default::default()
    };

    let report = validate_sleep_v1_release_gates(&input);

    assert!(!report.pass);
    assert_eq!(report.hand_reviewed_window_comparisons, 1);
    assert!(
        report
            .issues
            .contains(&"hand_reviewed_sleep_window_sample_below_gate".to_string())
    );
}

#[test]
fn sleep_v1_release_gate_validation_fails_closed_without_required_evidence() {
    let report = validate_sleep_v1_release_gates(&SleepV1ReleaseGateInput::default());

    assert!(!report.pass);
    assert!(!report.physical_historical_sync_pass);
    assert!(!report.timestamp_evidence_pass);
    assert!(!report.sleep_window_label_pass);
    assert!(!report.explanation_stability_pass);
    assert!(!report.benchmark_comparison_pass);
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_not_validated".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_report_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"historical_motion_hr_timestamps_not_proven".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"packet_sleep_windows_not_validated_against_hand_review".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_window_label_report_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_stage_label_report_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_v1_explanation_stability_report_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"sleep_v1_benchmark_report_missing".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "historical_sync.physical")
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_window_label_report_missing"
            && action.scope == "sleep_window.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_stage_label_report_missing" && action.scope == "sleep_stage.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_explanation_stability_report_missing"
            && action.scope == "sleep_v1.explanation_stability"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_v1_benchmark_report_missing" && action.scope == "sleep_v1.benchmark"
    }));
}

#[test]
fn sleep_v1_release_gate_rejects_physical_reports_without_proof_counts() {
    let mut physical_report =
        validate_historical_sync_physical_evidence(&physical_validation_input());
    assert!(physical_report.pass);

    physical_report.service_uuid_count = 0;
    physical_report.acceptance_summary.service_uuid_count = 0;

    let report = validate_sleep_v1_release_gates(&SleepV1ReleaseGateInput {
        physical_historical_sync: Some(physical_report),
        ..Default::default()
    });

    assert!(!report.pass);
    assert!(!report.physical_historical_sync_pass);
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );
}

#[test]
fn sleep_v1_release_gate_cli_composes_individual_evidence_reports() {
    let tempdir = tempfile::tempdir().unwrap();
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    for (index, sleep_id) in [
        "packet-derived-sleep-2026-05-27",
        "packet-derived-sleep-2026-05-28",
        "packet-derived-sleep-2026-05-29",
    ]
    .into_iter()
    .enumerate()
    {
        insert_sleep_window_label_with_sleep_id(
            &store,
            &format!("manual-reviewed-window-pass-{index}"),
            sleep_id,
            1_779_919_800_000,
            1_779_933_000_000,
        );
    }
    let physical_report = validate_historical_sync_physical_evidence(&physical_validation_input());
    let window_report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();
    seed_release_gate_sleep_stage_labels(&store);
    let stage_report = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();
    let stability_report = validate_sleep_v1_explanation_and_stability(
        &sleep_v1_quality_gate_input(),
        SleepV1ExplanationStabilityOptions::default(),
    );
    let benchmark_report =
        compare_sleep_v1_goose_to_reference(&sleep_v1_quality_gate_input()).unwrap();

    let physical_path = tempdir.path().join("physical.json");
    let window_path = tempdir.path().join("window.json");
    let stage_path = tempdir.path().join("stage.json");
    let stability_path = tempdir.path().join("stability.json");
    let benchmark_path = tempdir.path().join("benchmark.json");
    let output_path = tempdir.path().join("release-gate.json");
    let input_output_path = tempdir.path().join("release-gate-input.json");
    write_json(&physical_path, &physical_report);
    write_json(&window_path, &window_report);
    write_json(&stage_path, &stage_report);
    write_json(&stability_path, &stability_report);
    write_json(&benchmark_path, &benchmark_report);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-v1-release-gate"))
        .args([
            "--physical-historical-sync-report",
            physical_path.to_str().unwrap(),
            "--sleep-window-label-report",
            window_path.to_str().unwrap(),
            "--sleep-stage-label-report",
            stage_path.to_str().unwrap(),
            "--explanation-stability-report",
            stability_path.to_str().unwrap(),
            "--benchmark-comparison-reports",
            benchmark_path.to_str().unwrap(),
            "--min-hand-reviewed-window-comparisons",
            "3",
            "--input-output",
            input_output_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(report["schema"], "goose.sleep-v1-release-gate-report.v1");
    assert_eq!(report["pass"], true);
    assert_eq!(report["physical_historical_sync_pass"], true);
    assert_eq!(report["sleep_stage_label_pass"], true);
    assert_eq!(report["benchmark_comparison_pass"], true);
    let input_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&input_output_path).unwrap()).unwrap();
    assert_eq!(
        input_manifest["physical_historical_sync"]["schema"],
        "goose.historical-sync-physical-validation-report.v1"
    );
    assert_eq!(
        input_manifest["benchmark_comparisons"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(input_manifest["min_hand_reviewed_window_comparisons"], 3);
    assert_eq!(input_manifest["min_stage_label_comparisons"], 1);
    assert_eq!(input_manifest["min_benchmark_comparisons"], 1);
}

#[test]
fn sleep_v1_evidence_folder_validation_passes_complete_auditable_folder() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 6);
    assert!(report.unexpected_files.is_empty());
    assert_eq!(report.derivation_check_count, 7);
    assert_eq!(report.passing_derivation_check_count, 7);
    assert_eq!(report.evidence_manifest_sha256.as_ref().unwrap().len(), 64);
    assert_eq!(
        report.acceptance_summary.policy,
        "sleep_v1_evidence_folder_must_match_required_files_derivations_manifest_and_actions"
    );
    assert!(report.acceptance_summary.evidence_folder_ready);
    assert_eq!(
        report.acceptance_summary.evidence_dir,
        tempdir.path().display().to_string()
    );
    assert_eq!(report.acceptance_summary.required_file_count, 6);
    assert_eq!(report.acceptance_summary.passing_required_file_count, 6);
    assert_eq!(report.acceptance_summary.supporting_file_count, 6);
    assert_eq!(report.acceptance_summary.passing_supporting_file_count, 6);
    assert_eq!(report.acceptance_summary.derivation_check_count, 7);
    assert_eq!(report.acceptance_summary.passing_derivation_check_count, 7);
    assert_eq!(report.acceptance_summary.unexpected_file_count, 0);
    assert_eq!(
        report.acceptance_summary.evidence_manifest_sha256,
        report.evidence_manifest_sha256
    );
    assert_eq!(
        report.acceptance_summary.expected_evidence_manifest_sha256,
        report.expected_evidence_manifest_sha256
    );
    assert_eq!(report.acceptance_summary.issue_count, 0);
    assert_eq!(report.acceptance_summary.next_action_count, 0);
    assert_eq!(
        report
            .provenance
            .get("required_report_integrity_policy")
            .and_then(serde_json::Value::as_str),
        Some("required_reports_must_pass_schema_generator_status_and_component_integrity")
    );
    assert_eq!(
        report
            .provenance
            .get("required_report_integrity_policies")
            .and_then(|policies| policies.get("historical-sync-validation.json"))
            .and_then(serde_json::Value::as_str),
        Some(goose_core::historical_sync::HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY)
    );
    assert_eq!(
        report
            .provenance
            .get("required_report_integrity_policies")
            .and_then(|policies| policies.get("sleep-v1-release-gate.json"))
            .and_then(serde_json::Value::as_str),
        Some("sleep_v1_release_gate_requires_current_subgate_integrity_and_empty_proof_arrays")
    );
    assert_eq!(
        report
            .provenance
            .get("required_report_validation_policies")
            .and_then(|policies| policies.get("historical-sync-validation.json"))
            .and_then(serde_json::Value::as_str),
        Some(
            "service_characteristics_notifications_auth_commands_event_order_and_timestamp_fields"
        )
    );
    assert_eq!(
        report
            .provenance
            .get("required_report_validation_policies")
            .and_then(|policies| policies.get("sleep-v1-benchmark.json"))
            .and_then(serde_json::Value::as_str),
        Some(goose_core::algorithm_compare::SLEEP_V1_BENCHMARK_COMPARISON_POLICY)
    );
    assert!(report.required_files.iter().all(|file| file.exists));
    assert!(report.supporting_files.iter().all(|file| file.exists));
    assert!(
        report
            .required_files
            .iter()
            .all(|file| file.byte_size.unwrap() > 0)
    );
    assert!(
        report
            .supporting_files
            .iter()
            .all(|file| file.byte_size.unwrap() > 0)
    );
    assert!(
        report
            .required_files
            .iter()
            .all(|file| file.sha256.as_ref().unwrap().len() == 64)
    );
    assert!(
        report
            .supporting_files
            .iter()
            .all(|file| file.sha256.as_ref().unwrap().len() == 64)
    );
    assert!(
        report
            .required_files
            .iter()
            .any(|file| file.filename == "sleep-v1-release-gate.json"
                && file.schema.as_deref() == Some("goose.sleep-v1-release-gate-report.v1"))
    );
    assert!(
        report
            .required_files
            .iter()
            .all(|file| file.generated_by_pass)
    );
}

#[cfg(unix)]
#[test]
fn sleep_v1_evidence_folder_validation_rejects_symlinked_required_report() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let external_dir = tempfile::tempdir().unwrap();
    let required_filename = "sleep-v1-release-gate.json";
    let evidence_path = tempdir.path().join(required_filename);
    let external_path = external_dir.path().join(required_filename);
    fs::rename(&evidence_path, &external_path).unwrap();
    std::os::unix::fs::symlink(&external_path, &evidence_path).unwrap();

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"required_file_symlink:sleep-v1-release-gate.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.files"
            && action.reason == "required_file_symlink:sleep-v1-release-gate.json"
            && action.action.contains("Replace symlinked evidence")
    }));
    assert!(report.required_files.iter().any(|file| {
        file.filename == required_filename
            && file.exists
            && file
                .issues
                .contains(&"required_file_symlink:sleep-v1-release-gate.json".to_string())
    }));
}

#[cfg(unix)]
#[test]
fn sleep_v1_evidence_folder_validation_rejects_symlinked_supporting_file() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let external_dir = tempfile::tempdir().unwrap();
    let supporting_filename = "sleep-window-store.sqlite";
    let evidence_path = tempdir.path().join(supporting_filename);
    let external_path = external_dir.path().join(supporting_filename);
    fs::rename(&evidence_path, &external_path).unwrap();
    std::os::unix::fs::symlink(&external_path, &evidence_path).unwrap();

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.passing_supporting_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"supporting_file_symlink:sleep-window-store.sqlite".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.files"
            && action.reason == "supporting_file_symlink:sleep-window-store.sqlite"
            && action.action.contains("Replace symlinked evidence")
    }));
    assert!(report.supporting_files.iter().any(|file| {
        file.filename == supporting_filename
            && file.exists
            && file
                .issues
                .contains(&"supporting_file_symlink:sleep-window-store.sqlite".to_string())
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_physical_report_without_event_order_proof() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let physical_report_path = tempdir.path().join("historical-sync-validation.json");
    let mut physical_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&physical_report_path).unwrap()).unwrap();
    physical_report
        .as_object_mut()
        .unwrap()
        .remove("event_order_confirmed");
    write_json(&physical_report_path, &physical_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:historical-sync-validation.json".to_string())
    );
    assert!(report.required_files.iter().any(|file| {
        file.filename == "historical-sync-validation.json"
            && file
                .issues
                .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "physical_historical_sync_report_integrity_failed"
            && action.scope == "historical_sync.physical"
            && action
                .action
                .contains("including all required physical-flow subgates")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_physical_report_without_integrity_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let physical_report_path = tempdir.path().join("historical-sync-validation.json");
    let mut physical_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&physical_report_path).unwrap()).unwrap();
    physical_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    write_json(&physical_report_path, &physical_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:historical-sync-validation.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "physical_historical_sync_report_integrity_failed"
            && action.scope == "historical_sync.physical"
            && action
                .action
                .contains("including all required physical-flow subgates")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_sleep_window_report_without_integrity_policy()
{
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let window_report_path = tempdir.path().join("sleep-window-validation.json");
    let mut window_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&window_report_path).unwrap()).unwrap();
    window_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    write_json(&window_report_path, &window_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-window-validation.json".to_string())
    );
    assert!(report.required_files.iter().any(|file| {
        file.filename == "sleep-window-validation.json"
            && file
                .issues
                .contains(&"sleep_window_label_report_integrity_failed".to_string())
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_window_label_report_integrity_failed"
            && action.scope == "sleep_window.labels"
            && action
                .action
                .contains("current label-report integrity policy")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_sleep_window_report_without_validation_policy()
{
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let window_report_path = tempdir.path().join("sleep-window-validation.json");
    let mut window_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&window_report_path).unwrap()).unwrap();
    window_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    write_json(&window_report_path, &window_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"sleep_window_label_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-window-validation.json".to_string())
    );
    assert!(report.required_files.iter().any(|file| {
        file.filename == "sleep-window-validation.json"
            && file
                .issues
                .contains(&"sleep_window_label_report_integrity_failed".to_string())
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "sleep_window_label_report_integrity_failed"
            && action.scope == "sleep_window.labels"
            && action
                .action
                .contains("current label-report integrity policy")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_accepts_cli_benchmark_coverage_fields() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    add_cli_benchmark_coverage_fields(tempdir.path(), false);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(
        report
            .derivation_checks
            .iter()
            .any(|check| check.name == "sleep_v1_benchmark_matches_input" && check.pass)
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_invalid_cli_benchmark_coverage_fields() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    add_cli_benchmark_coverage_fields(tempdir.path(), true);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(report.derivation_checks.iter().any(|check| {
        check.name == "sleep_v1_benchmark_matches_input"
            && !check.pass
            && check
                .issues
                .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_unexpected_files() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    fs::write(
        tempdir.path().join("old-sleep-v1-benchmark.json"),
        r#"{"schema":"stale"}"#,
    )
    .unwrap();
    fs::write(tempdir.path().join(".DS_Store"), "ignored").unwrap();

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(
        report.unexpected_files,
        vec!["old-sleep-v1-benchmark.json".to_string()]
    );
    assert!(
        report
            .issues
            .contains(&"unexpected_evidence_file:old-sleep-v1-benchmark.json".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.files"
                && action.reason == "unexpected_evidence_file:old-sleep-v1-benchmark.json"
                && action.action.contains("Remove or archive"))
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_detects_historical_template_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let template_path = tempdir.path().join("historical-sync-template.json");
    let mut template: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&template_path).unwrap()).unwrap();
    template["capture_session_id"] = serde_json::json!("different-capture-session");
    write_json(&template_path, &template);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"historical_sync_template_capture_session_mismatch".to_string())
    );
    assert!(
        report
            .derivation_checks
            .iter()
            .any(|check| check.name == "historical_sync_evidence_matches_template" && !check.pass)
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.derivations")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_fails_closed_for_missing_report() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    fs::remove_file(tempdir.path().join("sleep-v1-benchmark.json")).unwrap();

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert!(report.passing_required_file_count < report.required_file_count);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 6);
    assert!(
        report
            .issues
            .contains(&"missing_required_file:sleep-v1-benchmark.json".to_string())
    );
    let missing = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(missing.byte_size, None);
    assert_eq!(missing.sha256, None);
    assert_eq!(report.evidence_manifest_sha256, None);
    assert!(report.next_actions.iter().any(|action| action.reason
        == "missing_required_file:sleep-v1-benchmark.json"
        && action.scope == "sleep_v1.benchmark"
        && action.action.contains("sleep-v1-benchmark.json")));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_unexpected_report_generator() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let benchmark_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    benchmark_report["generated_by"] = serde_json::json!("hand-edited-benchmark");
    write_json(&benchmark_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert!(report.passing_required_file_count < report.required_file_count);
    assert!(
        report
            .issues
            .contains(&"generated_by_mismatch:sleep-v1-benchmark.json".to_string())
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(
        benchmark.expected_generated_by.as_deref(),
        Some("goose.algorithm_compare")
    );
    assert_eq!(
        benchmark.generated_by.as_deref(),
        Some("hand-edited-benchmark")
    );
    assert!(!benchmark.generated_by_pass);
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.schema")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_report_with_issues() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-release-gate.json");
    let mut release_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    release_report["issues"] = serde_json::json!(["hand-edited-issue"]);
    release_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &release_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"passing_required_file_has_issues:sleep-v1-release-gate.json".to_string())
    );
    let release_gate = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-release-gate.json")
        .unwrap();
    assert_eq!(release_gate.pass, Some(true));
    assert!(
        release_gate
            .issues
            .contains(&"passing_required_file_has_issues:sleep-v1-release-gate.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_report_with_next_actions() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    benchmark_report["next_actions"] = serde_json::json!([
        {
            "scope": "comparison",
            "reason": "forged_next_action",
            "action": "Do not accept passing reports with leftover next actions."
        }
    ]);
    write_json(&report_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report.issues.contains(
            &"passing_required_file_has_next_actions:sleep-v1-benchmark.json".to_string()
        )
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(benchmark.pass, Some(true));
    assert!(
        benchmark.issues.contains(
            &"passing_required_file_has_next_actions:sleep-v1-benchmark.json".to_string()
        )
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.reports")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_report_missing_proof_arrays() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-release-gate.json");
    let mut release_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    release_report.as_object_mut().unwrap().remove("issues");
    release_report
        .as_object_mut()
        .unwrap()
        .remove("next_actions");
    release_report
        .as_object_mut()
        .unwrap()
        .remove("quality_flags");
    release_report.as_object_mut().unwrap().remove("errors");
    release_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &release_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert!(report.passing_required_file_count < report.required_file_count);
    assert!(
        report.issues.contains(
            &"passing_required_file_missing_issues:sleep-v1-release-gate.json".to_string()
        )
    );
    assert!(report.issues.contains(
        &"passing_required_file_missing_next_actions:sleep-v1-release-gate.json".to_string()
    ));
    assert!(
        report.issues.contains(
            &"passing_required_file_missing_quality_flags:sleep-v1-release-gate.json:quality_flags"
                .to_string()
        )
    );
    assert!(
        report.issues.contains(
            &"passing_required_file_missing_errors:sleep-v1-release-gate.json".to_string()
        )
    );
    let release_gate = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-release-gate.json")
        .unwrap();
    assert_eq!(release_gate.pass, Some(true));
    assert!(
        release_gate.issues.contains(
            &"passing_required_file_missing_issues:sleep-v1-release-gate.json".to_string()
        )
    );
    assert!(release_gate.issues.contains(
        &"passing_required_file_missing_next_actions:sleep-v1-release-gate.json".to_string()
    ));
    assert!(
        release_gate.issues.contains(
            &"passing_required_file_missing_quality_flags:sleep-v1-release-gate.json:quality_flags"
                .to_string()
        )
    );
    assert!(
        release_gate.issues.contains(
            &"passing_required_file_missing_errors:sleep-v1-release-gate.json".to_string()
        )
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.reports"
            && action.reason == "passing_required_file_missing_issues:sleep-v1-release-gate.json"
            && action.action.contains("explicit empty issues list")
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.reports"
            && action.reason
                == "passing_required_file_missing_quality_flags:sleep-v1-release-gate.json:quality_flags"
            && action.action.contains("explicit empty quality_flags list")
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.reports"
            && action.reason == "passing_required_file_missing_errors:sleep-v1-release-gate.json"
            && action.action.contains("explicit empty errors list")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_benchmark_missing_issues() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    benchmark_report.as_object_mut().unwrap().remove("issues");
    benchmark_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"passing_required_file_missing_issues:sleep-v1-benchmark.json".to_string())
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(benchmark.pass, Some(true));
    assert!(
        benchmark
            .issues
            .contains(&"passing_required_file_missing_issues:sleep-v1-benchmark.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_benchmark_missing_quality_flag_arrays() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    benchmark_report
        .as_object_mut()
        .unwrap()
        .remove("goose_quality_flags");
    benchmark_report
        .as_object_mut()
        .unwrap()
        .remove("reference_quality_flags");
    benchmark_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(report.issues.contains(
        &"passing_required_file_missing_quality_flags:sleep-v1-benchmark.json:goose_quality_flags"
            .to_string()
    ));
    assert!(report.issues.contains(
        &"passing_required_file_missing_quality_flags:sleep-v1-benchmark.json:reference_quality_flags"
            .to_string()
    ));
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(benchmark.pass, Some(true));
    assert!(benchmark.issues.contains(
        &"passing_required_file_missing_quality_flags:sleep-v1-benchmark.json:goose_quality_flags"
            .to_string()
    ));
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.reports"
            && action
                .reason
                .contains("sleep-v1-benchmark.json:goose_quality_flags")
            && action
                .action
                .contains("explicit empty goose_quality_flags list")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_report_with_quality_flags() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    benchmark_report["goose_quality_flags"] = serde_json::json!(["sleep_architecture_unavailable"]);
    benchmark_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report.issues.contains(
            &"passing_required_file_has_quality_flags:sleep-v1-benchmark.json:goose_quality_flags"
                .to_string()
        )
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(benchmark.pass, Some(true));
    assert!(
        benchmark.issues.contains(
            &"passing_required_file_has_quality_flags:sleep-v1-benchmark.json:goose_quality_flags"
                .to_string()
        )
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.reports")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_passing_report_with_errors() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let report_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    benchmark_report["errors"] = serde_json::json!(["hand_edited_benchmark_error"]);
    benchmark_report["pass"] = serde_json::json!(true);
    write_json(&report_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"passing_required_file_has_errors:sleep-v1-benchmark.json".to_string())
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert_eq!(benchmark.pass, Some(true));
    assert!(
        benchmark
            .issues
            .contains(&"passing_required_file_has_errors:sleep-v1-benchmark.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.evidence_folder.reports"
            && action.reason == "passing_required_file_has_errors:sleep-v1-benchmark.json"
            && action.action.contains("empty error lists")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_accepts_pinned_manifest_hash() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let initial_report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();
    let expected_hash = initial_report.evidence_manifest_sha256.clone().unwrap();

    let report = validate_sleep_v1_evidence_folder_with_options(
        tempdir.path(),
        SleepV1EvidenceFolderOptions {
            expected_evidence_manifest_sha256: Some(expected_hash.clone()),
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.evidence_manifest_sha256.as_deref(),
        Some(expected_hash.as_str())
    );
    assert_eq!(
        report.expected_evidence_manifest_sha256.as_deref(),
        Some(expected_hash.as_str())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_fails_for_pinned_manifest_hash_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());

    let report = validate_sleep_v1_evidence_folder_with_options(
        tempdir.path(),
        SleepV1EvidenceFolderOptions {
            expected_evidence_manifest_sha256: Some("0".repeat(64)),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"evidence_manifest_sha256_mismatch".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.manifest")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_fails_for_invalid_pinned_manifest_hash() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());

    let report = validate_sleep_v1_evidence_folder_with_options(
        tempdir.path(),
        SleepV1EvidenceFolderOptions {
            expected_evidence_manifest_sha256: Some("not-a-sha256".to_string()),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"expected_manifest_sha256_invalid".to_string())
    );
    assert!(
        !report
            .issues
            .contains(&"evidence_manifest_sha256_mismatch".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_requires_reproducible_source_inputs() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    fs::remove_file(tempdir.path().join("sleep-v1-input.json")).unwrap();

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 5);
    assert_eq!(report.derivation_check_count, 7);
    assert_eq!(report.passing_derivation_check_count, 5);
    assert!(
        report
            .issues
            .contains(&"missing_supporting_file:sleep-v1-input.json".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.files")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_detects_source_report_drift() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut drifted_input = sleep_v1_quality_gate_input();
    drifted_input.sleep.sleep_duration_minutes -= 30.0;
    write_json(&tempdir.path().join("sleep-v1-input.json"), &drifted_input);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 6);
    assert_eq!(report.derivation_check_count, 7);
    assert_eq!(report.passing_derivation_check_count, 5);
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-stability.json".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.derivations")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_detects_benchmark_embedded_output_drift() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let benchmark_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    benchmark_report["goose_output"]["provenance"]["hand_edited"] = serde_json::json!(true);
    write_json(&benchmark_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_input_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.derivations")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_stability_contract_file() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let stability_path = tempdir.path().join("sleep-v1-stability.json");
    let mut stability_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&stability_path).unwrap()).unwrap();
    stability_report["v1_component_names"] = serde_json::json!([
        "duration",
        "efficiency",
        "awake",
        "deep",
        "rem",
        "schedule",
        "heart_rate"
    ]);
    write_json(&stability_path, &stability_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"stability_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-stability.json".to_string())
    );
    let stability = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-stability.json")
        .unwrap();
    assert!(
        stability
            .issues
            .contains(&"stability_report_integrity_failed".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.stability"
            && action.reason == "stability_report_integrity_failed"
            && action.action.contains("sleep-v1-stability.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_physical_report_without_validation_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let physical_report_path = tempdir.path().join("historical-sync-validation.json");
    let mut physical_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&physical_report_path).unwrap()).unwrap();
    physical_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    write_json(&physical_report_path, &physical_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"physical_historical_sync_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:historical-sync-validation.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "physical_historical_sync_report_integrity_failed"
            && action.scope == "historical_sync.physical"
            && action
                .action
                .contains("including all required physical-flow subgates")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_stability_report_without_integrity_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let stability_path = tempdir.path().join("sleep-v1-stability.json");
    let mut stability_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&stability_path).unwrap()).unwrap();
    stability_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    write_json(&stability_path, &stability_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"stability_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-stability.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.stability"
            && action.reason == "stability_report_integrity_failed"
            && action.action.contains("sleep-v1-stability.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_stability_report_without_validation_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let stability_path = tempdir.path().join("sleep-v1-stability.json");
    let mut stability_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&stability_path).unwrap()).unwrap();
    stability_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    write_json(&stability_path, &stability_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"stability_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-stability.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.stability"
            && action.reason == "stability_report_integrity_failed"
            && action.action.contains("sleep-v1-stability.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_benchmark_report_without_integrity_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let benchmark_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    benchmark_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    write_json(&benchmark_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"benchmark_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.benchmark"
            && action.reason == "benchmark_report_integrity_failed"
            && action.action.contains("sleep-v1-benchmark.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_benchmark_report_without_validation_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let benchmark_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    benchmark_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("validation_policy");
    write_json(&benchmark_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"benchmark_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.benchmark"
            && action.reason == "benchmark_report_integrity_failed"
            && action.action.contains("sleep-v1-benchmark.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_benchmark_contract_file() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let benchmark_path = tempdir.path().join("sleep-v1-benchmark.json");
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    benchmark_report["goose_output"]["components"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "legacy_extra_component",
            "value": 1.0,
            "unit": "debug",
            "score_0_to_100": 0.0,
            "weight": 0.0,
            "contribution": 0.0
        }));
    benchmark_report["goose_output"]["component_provenance"]
        .as_object_mut()
        .unwrap()
        .insert(
            "legacy_extra_component".to_string(),
            serde_json::json!({
                "inputs": {"legacy_value": 1.0},
                "policy": "legacy_extra_component"
            }),
        );
    write_json(&benchmark_path, &benchmark_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 5);
    assert!(
        report
            .issues
            .contains(&"benchmark_report_integrity_failed".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-benchmark.json".to_string())
    );
    let benchmark = report
        .required_files
        .iter()
        .find(|file| file.filename == "sleep-v1-benchmark.json")
        .unwrap();
    assert!(
        benchmark
            .issues
            .contains(&"benchmark_report_integrity_failed".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.benchmark"
            && action.reason == "benchmark_report_integrity_failed"
            && action.action.contains("sleep-v1-benchmark.json")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_detects_sleep_window_store_drift() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let store = GooseStore::open(&tempdir.path().join("sleep-window-store.sqlite")).unwrap();
    insert_sleep_window_label(
        &store,
        "manual-reviewed-window-extra",
        1_779_927_600_000,
        1_779_940_800_000,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 6);
    assert_eq!(report.derivation_check_count, 7);
    assert_eq!(report.passing_derivation_check_count, 6);
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-window-validation.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_external_sleep_window_store_path() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let input_path = tempdir.path().join("sleep-window-validation-input.json");
    let mut input: SleepWindowLabelValidationEvidenceInput =
        serde_json::from_str(&fs::read_to_string(&input_path).unwrap()).unwrap();
    input.database_path = "/tmp/not-the-reviewed-sleep-window-store.sqlite".to_string();
    write_json(&input_path, &input);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 5);
    assert!(
        report.issues.contains(
            &"supporting_contract_invalid:sleep-window-validation-input.json".to_string()
        )
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.evidence_folder.inputs"
                && action.reason
                    == "supporting_contract_invalid:sleep-window-validation-input.json")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_detects_release_gate_input_report_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_input: SleepV1ReleaseGateInput = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate-input.json")).unwrap(),
    )
    .unwrap();
    release_input.sleep_window_label_validation = None;
    write_json(
        &tempdir.path().join("sleep-v1-release-gate-input.json"),
        &release_input,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.required_file_count, 6);
    assert_eq!(report.passing_required_file_count, 6);
    assert_eq!(report.supporting_file_count, 6);
    assert_eq!(report.passing_supporting_file_count, 6);
    assert_eq!(report.derivation_check_count, 7);
    assert_eq!(report.passing_derivation_check_count, 5);
    assert!(
        report.issues.contains(
            &"release_gate_input_report_mismatch:sleep-window-validation.json".to_string()
        )
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_lowered_release_gate_review_threshold() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_input: SleepV1ReleaseGateInput = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate-input.json")).unwrap(),
    )
    .unwrap();
    release_input.min_hand_reviewed_window_comparisons = 1;
    let release_report = validate_sleep_v1_release_gates(&release_input);
    write_json(
        &tempdir.path().join("sleep-v1-release-gate-input.json"),
        &release_input,
    );
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_hand_reviewed_window_threshold_below_default".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_window.labels")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_lowered_release_gate_benchmark_threshold() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_input: SleepV1ReleaseGateInput = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate-input.json")).unwrap(),
    )
    .unwrap();
    release_input.min_benchmark_comparisons = 0;
    let release_report = validate_sleep_v1_release_gates(&release_input);
    write_json(
        &tempdir.path().join("sleep-v1-release-gate-input.json"),
        &release_input,
    );
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_benchmark_threshold_below_default".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_v1.benchmark")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_lowered_release_gate_stage_label_threshold() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_input: SleepV1ReleaseGateInput = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate-input.json")).unwrap(),
    )
    .unwrap();
    release_input.min_stage_label_comparisons = 0;
    let release_report = validate_sleep_v1_release_gates(&release_input);
    write_json(
        &tempdir.path().join("sleep-v1-release-gate-input.json"),
        &release_input,
    );
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_stage_label_threshold_below_default".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "sleep_stage.labels")
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_hand_edited_release_gate_report_counts() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: SleepV1ReleaseGateReport = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report.hand_reviewed_window_comparisons = 1;
    release_report
        .next_actions
        .push(goose_core::sleep_validation::SleepV1ReleaseGateNextAction {
            scope: "sleep_window.labels".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited release reports.".to_string(),
        });
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_hand_reviewed_sample_below_threshold".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_next_actions_mismatch".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_acceptance_summary_mismatch".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_v1.release_gate"
            && action.reason == "release_gate_report_acceptance_summary_mismatch"
            && action.action.contains("acceptance summary")
    }));
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_hand_edited_release_gate_stage_label_counts() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: SleepV1ReleaseGateReport = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report.stage_label_comparison_count = 0;
    release_report
        .next_actions
        .push(goose_core::sleep_validation::SleepV1ReleaseGateNextAction {
            scope: "sleep_stage.labels".to_string(),
            reason: "forged_next_action".to_string(),
            action: "Do not accept hand-edited stage-label release counts.".to_string(),
        });
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_stage_label_sample_below_threshold".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_next_actions_mismatch".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_acceptance_summary_mismatch".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_stage.labels"
            && action.reason == "release_gate_report_stage_label_sample_below_threshold"
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_hand_edited_release_gate_stage_label_threshold() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let release_report_path = tempdir.path().join("sleep-v1-release-gate.json");
    let mut release_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&release_report_path).unwrap()).unwrap();
    release_report["provenance"]["min_stage_label_comparisons"] = serde_json::json!(0);
    write_json(&release_report_path, &release_report);

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_stage_label_threshold_below_default".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_acceptance_summary_mismatch".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "sleep_stage.labels"
            && action.reason == "release_gate_report_stage_label_threshold_below_default"
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_stale_release_gate_acceptance_summary() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: SleepV1ReleaseGateReport = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report.acceptance_summary.benchmark_comparison_count += 1;
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_acceptance_summary_mismatch".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_hand_edited_release_gate_flags_or_errors() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: SleepV1ReleaseGateReport = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report
        .quality_flags
        .push("hand_edited_release_quality_flag".to_string());
    release_report
        .errors
        .push("hand_edited_release_error".to_string());
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report.issues.contains(
            &"passing_required_file_has_quality_flags:sleep-v1-release-gate.json:quality_flags"
                .to_string()
        )
    );
    assert!(
        report
            .issues
            .contains(&"passing_required_file_has_errors:sleep-v1-release-gate.json".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_quality_flags_present".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_errors_present".to_string())
    );
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_missing_release_gate_promotion_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("promotion_policy");
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_promotion_policy_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_promotion_policy_missing"
            && action.scope == "sleep_v1.release_gate"
            && action.action.contains("promotion policy provenance")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_hand_edited_release_gate_pass_false() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: SleepV1ReleaseGateReport = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report.pass = false;
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"required_file_not_passing:sleep-v1-release-gate.json".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"release_gate_report_pass_state_inconsistent".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "required_file_not_passing:sleep-v1-release-gate.json"
            && action.scope == "sleep_v1.release_gate"
            && action.action.contains("final promotion pass state")
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_pass_state_inconsistent"
            && action.scope == "sleep_v1.release_gate"
            && action.action.contains("pass status")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_missing_release_gate_integrity_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("report_integrity_policy");
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_integrity_policy_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_integrity_policy_missing"
            && action.scope == "sleep_v1.release_gate"
            && action.action.contains("subgate-integrity provenance")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_missing_release_gate_threshold_policy() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("threshold_policy");
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_threshold_policy_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"derived_report_mismatch:sleep-v1-release-gate.json".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_threshold_policy_missing"
            && action.scope == "sleep_v1.release_gate"
            && action
                .action
                .contains("primary-threshold policy provenance")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_missing_release_gate_subgate_integrity_policies() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("subgate_report_integrity_policies");
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_subgate_integrity_policy_missing".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_subgate_integrity_policy_missing"
            && action.scope == "sleep_v1.release_gate"
            && action
                .action
                .contains("per-subgate evidence integrity policies")
    }));
}

#[test]
fn sleep_v1_evidence_folder_validation_rejects_missing_release_gate_subgate_validation_policies() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let mut release_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tempdir.path().join("sleep-v1-release-gate.json")).unwrap(),
    )
    .unwrap();
    release_report["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("subgate_report_validation_policies");
    write_json(
        &tempdir.path().join("sleep-v1-release-gate.json"),
        &release_report,
    );

    let report = validate_sleep_v1_evidence_folder(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .contains(&"release_gate_report_subgate_validation_policy_missing".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "release_gate_report_subgate_validation_policy_missing"
            && action.scope == "sleep_v1.release_gate"
            && action.action.contains("per-subgate validation policies")
    }));
}

#[test]
fn sleep_v1_evidence_folder_cli_reports_complete_auditable_folder() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let output_path = tempdir.path().join("sleep-v1-evidence-folder.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-v1-evidence-folder"))
        .args([
            "--evidence-dir",
            tempdir.path().to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        report["schema"],
        "goose.sleep-v1-validation-evidence-folder-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["required_file_count"], 6);
    assert_eq!(report["passing_required_file_count"], 6);
    assert_eq!(report["supporting_file_count"], 6);
    assert_eq!(report["passing_supporting_file_count"], 6);
    assert_eq!(report["derivation_check_count"], 7);
    assert_eq!(report["passing_derivation_check_count"], 7);
    assert_eq!(
        report["evidence_manifest_sha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(
        report["required_files"][0]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(report["required_files"][0]["byte_size"].as_u64().unwrap() > 0);
}

#[test]
fn sleep_v1_evidence_folder_cli_fails_for_pinned_manifest_hash_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let output_path = tempdir.path().join("sleep-v1-evidence-folder.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-v1-evidence-folder"))
        .args([
            "--evidence-dir",
            tempdir.path().to_str().unwrap(),
            "--expected-manifest-sha256",
            &"0".repeat(64),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(report["pass"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "evidence_manifest_sha256_mismatch")
    );
}

#[test]
fn sleep_v1_evidence_folder_cli_fails_for_invalid_pinned_manifest_hash() {
    let tempdir = tempfile::tempdir().unwrap();
    write_passing_sleep_v1_evidence_folder(tempdir.path());
    let output_path = tempdir.path().join("sleep-v1-evidence-folder.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-v1-evidence-folder"))
        .args([
            "--evidence-dir",
            tempdir.path().to_str().unwrap(),
            "--expected-manifest-sha256",
            "not-a-sha256",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(report["pass"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "expected_manifest_sha256_invalid")
    );
}

#[test]
fn sleep_window_label_validation_passes_hand_reviewed_window_inside_tolerance() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    insert_sleep_window_label(
        &store,
        "manual-reviewed-window-pass",
        1_779_919_800_000,
        1_779_933_000_000,
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 1);
    assert_eq!(report.passing_label_count, 1);
    assert_eq!(report.distinct_compared_sleep_window_count, 1);
    assert_eq!(report.distinct_passing_sleep_window_count, 1);
    assert_eq!(
        report.comparisons[0].expected_start_time,
        "2026-05-27T22:10:00Z"
    );
    assert_eq!(
        report.comparisons[0].expected_end_time,
        "2026-05-28T01:50:00Z"
    );
    assert_eq!(report.comparisons[0].start_delta_minutes, 10.0);
    assert_eq!(report.comparisons[0].end_delta_minutes, 10.0);
    assert_eq!(report.comparisons[0].duration_delta_minutes, 20.0);
    assert_eq!(
        report.acceptance_summary.min_observed_label_confidence,
        0.95
    );
}

#[test]
fn sleep_window_label_validation_rejects_duplicate_reviewed_sleep_id() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    for index in 0..2 {
        insert_sleep_window_label_with_sleep_id(
            &store,
            &format!("manual-reviewed-window-duplicate-{index}"),
            "packet-derived-sleep-2026-05-27",
            1_779_919_800_000,
            1_779_933_000_000,
        );
    }

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.compared_label_count, 2);
    assert_eq!(report.passing_label_count, 2);
    assert_eq!(report.distinct_compared_sleep_window_count, 1);
    assert_eq!(report.distinct_passing_sleep_window_count, 1);
    assert!(
        report
            .issues
            .contains(&"duplicate_reviewed_sleep_id".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "duplicate_reviewed_sleep_id")
    );
}

#[test]
fn sleep_window_validator_cli_reports_hand_reviewed_window_match() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("sleep-window-validation.sqlite");
    let store = GooseStore::open(&db_path).unwrap();
    seed_sleep_window_motion(&store);
    insert_sleep_window_label(
        &store,
        "manual-reviewed-window-pass",
        1_779_919_800_000,
        1_779_933_000_000,
    );
    drop(store);
    let output_path = tempdir.path().join("sleep-window-validation.json");
    let input_output_path = tempdir.path().join("sleep-window-validation-input.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-window-validator"))
        .args([
            "--db",
            db_path.to_str().unwrap(),
            "--start",
            "2026-05-27T22:00:00Z",
            "--end",
            "2026-05-28T03:00:00Z",
            "--min-owned-captures",
            "1",
            "--require-trusted-evidence",
            "--sleep-need-minutes",
            "240",
            "--input-output",
            input_output_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        report["schema"],
        "goose.sleep-window-label-validation-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["compared_label_count"], 1);
    assert_eq!(report["passing_label_count"], 1);
    assert_eq!(
        report["acceptance_summary"]["policy"],
        "packet_sleep_window_must_match_distinct_hand_reviewed_nights"
    );
    assert_eq!(
        report["acceptance_summary"]["accepted_sleep_ids"][0],
        "packet-derived-sleep-2026-05-27"
    );
    assert_eq!(
        report["acceptance_summary"]["min_observed_label_confidence"],
        0.95
    );

    let input_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&input_output_path).unwrap()).unwrap();
    assert_eq!(
        input_manifest["schema"],
        "goose.sleep-window-label-validation-input.v1"
    );
    assert_eq!(input_manifest["database_path"], db_path.to_str().unwrap());
    assert_eq!(input_manifest["start"], "2026-05-27T22:00:00Z");
    assert_eq!(input_manifest["end"], "2026-05-28T03:00:00Z");
    assert_eq!(
        input_manifest["options"]["min_owned_captures_per_summary"],
        1
    );
    assert_eq!(input_manifest["options"]["require_trusted_evidence"], true);
    assert_eq!(input_manifest["options"]["sleep_need_minutes"], 240.0);
}

#[test]
fn sleep_window_label_validation_reports_actionable_tolerance_failures() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    insert_sleep_window_label(
        &store,
        "manual-reviewed-window-fail",
        1_779_927_600_000,
        1_779_940_800_000,
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            start_tolerance_minutes: 20.0,
            end_tolerance_minutes: 20.0,
            duration_tolerance_minutes: 30.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 1);
    assert_eq!(report.passing_label_count, 0);
    assert!(
        report.issues.contains(
            &"manual-reviewed-window-fail:sleep_window_start_outside_tolerance".to_string()
        )
    );
    assert!(
        report.issues.contains(
            &"manual-reviewed-window-fail:sleep_window_end_outside_tolerance".to_string()
        )
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "sleep_window_start_outside_tolerance")
    );
}

#[test]
fn sleep_window_label_validation_requires_explicit_reviewer_confidence() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": 1_779_919_800_000i64,
        "corrected_end_time_unix_ms": 1_779_933_000_000i64,
        "review_source": "hand_reviewed"
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-no-confidence",
                sleep_id: Some("packet-derived-sleep-2026-05-27"),
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: None,
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"manual"}"#,
            })
            .unwrap()
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 0);
    assert!(
        report
            .issues
            .contains(&"manual-reviewed-window-no-confidence:label_confidence_missing".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "label_confidence_missing"
                && action.scope == "sleep_window.labels")
    );
}

#[test]
fn sleep_window_label_storage_rejects_out_of_range_reviewer_confidence() {
    let store = GooseStore::open_in_memory().unwrap();
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": 1_779_919_800_000i64,
        "corrected_end_time_unix_ms": 1_779_933_000_000i64,
        "review_source": "hand_reviewed"
    })
    .to_string();
    let error = store
        .insert_sleep_correction_label(SleepCorrectionLabelInput {
            label_id: "manual-reviewed-window-bad-confidence",
            sleep_id: Some("packet-derived-sleep-2026-05-27"),
            label_type: "sleep_window",
            start_time_unix_ms: 1_779_919_800_000,
            end_time_unix_ms: 1_779_933_000_000,
            value_json: &value_json,
            source: "manual",
            confidence: Some(1.20),
            provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"manual"}"#,
        })
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("confidence must be between 0.0 and 1.0")
    );
}

#[test]
fn sleep_window_label_validation_requires_hand_review_policy() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": 1_779_919_800_000i64,
        "corrected_end_time_unix_ms": 1_779_933_000_000i64
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-no-policy",
                sleep_id: Some("packet-derived-sleep-2026-05-27"),
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"ui_surface":"metrics.sleep.manual_correction"}"#,
            })
            .unwrap()
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 0);
    assert!(
        report
            .issues
            .contains(&"manual-reviewed-window-no-policy:label_review_policy_missing".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "label_review_policy_missing"
                && action.scope == "sleep_window.labels")
    );
}

#[test]
fn sleep_window_label_validation_requires_matching_provenance_source() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": 1_779_919_800_000i64,
        "corrected_end_time_unix_ms": 1_779_933_000_000i64,
        "review_source": "hand_reviewed"
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-source-missing",
                sleep_id: Some("packet-derived-sleep-2026-05-27"),
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-source-mismatch",
                sleep_id: Some("packet-derived-sleep-2026-05-28"),
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"imported_export"}"#,
            })
            .unwrap()
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 2);
    assert_eq!(report.compared_label_count, 0);
    assert!(report.issues.contains(
        &"manual-reviewed-window-source-missing:label_provenance_source_missing".to_string()
    ));
    assert!(report.issues.contains(
        &"manual-reviewed-window-source-mismatch:label_provenance_source_mismatch".to_string()
    ));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_provenance_source_missing" && action.scope == "sleep_window.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_provenance_source_mismatch" && action.scope == "sleep_window.labels"
    }));
}

#[test]
fn sleep_window_label_validation_requires_reviewed_sleep_id() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": 1_779_919_800_000i64,
        "corrected_end_time_unix_ms": 1_779_933_000_000i64,
        "review_source": "hand_reviewed"
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-no-sleep-id",
                sleep_id: None,
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"manual"}"#,
            })
            .unwrap()
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 0);
    assert!(
        report
            .issues
            .contains(&"manual-reviewed-window-no-sleep-id:label_sleep_id_missing".to_string())
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "label_sleep_id_missing"
                && action.scope == "sleep_window.labels")
    );
}

#[test]
fn sleep_window_label_validation_rejects_malformed_corrected_times() {
    let store = GooseStore::open_in_memory().unwrap();
    seed_sleep_window_motion(&store);
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": "1779919800000",
        "corrected_end_time_unix_ms": 1_779_933_000_000i64,
        "review_source": "hand_reviewed"
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id: "manual-reviewed-window-malformed-time",
                sleep_id: Some("packet-derived-sleep-2026-05-27"),
                label_type: "sleep_window",
                start_time_unix_ms: 1_779_919_800_000,
                end_time_unix_ms: 1_779_933_000_000,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"manual"}"#,
            })
            .unwrap()
    );

    let report = run_sleep_window_label_validation_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 1);
    assert_eq!(report.compared_label_count, 0);
    assert!(report.issues.contains(
        &"manual-reviewed-window-malformed-time:label_corrected_start_time_invalid".to_string()
    ));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_corrected_start_time_invalid"
            && action.scope == "sleep_window.labels"
    }));
}

#[test]
fn sleep_stage_label_validation_compares_user_owned_stage_labels() {
    let store = GooseStore::open_in_memory().unwrap();
    insert_sleep_stage_label(
        &store,
        "stage-label-deep",
        "deep",
        1_779_934_500_000,
        1_779_938_100_000,
        r#"{"review_policy":"user_owned_sleep_stage_label","source":"user_export"}"#,
    );
    insert_sleep_stage_label(
        &store,
        "stage-label-rem",
        "asleep_rem",
        1_779_939_900_000,
        1_779_943_500_000,
        r#"{"official_labels_are_labels":true,"source":"user_export"}"#,
    );

    let report = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.schema,
        "goose.sleep-stage-label-validation-report.v1"
    );
    assert_eq!(report.label_count, 2);
    assert_eq!(report.compared_label_count, 2);
    assert_eq!(report.passing_label_count, 2);
    assert_eq!(report.stage_segment_count, 5);
    assert_eq!(
        report.acceptance_summary.policy,
        "sleep_v1_stages_must_match_user_owned_stage_labels"
    );
    assert_eq!(report.acceptance_summary.accepted_label_ids.len(), 2);
    assert!(report.acceptance_summary.user_owned_stage_sample_ready);
    assert!(
        report
            .comparisons
            .iter()
            .all(|comparison| comparison.sleep_id.as_deref()
                == Some("sleep-v1-quality-gate-input"))
    );
    assert_eq!(
        report.provenance["validation_policy"],
        serde_json::json!("sleep_v1_stage_segments_vs_user_owned_sleep_stage_labels")
    );
    assert_eq!(
        report.provenance["report_integrity_policy"],
        serde_json::json!(goose_core::sleep_validation::SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY)
    );
    assert!(
        report
            .comparisons
            .iter()
            .all(|comparison| comparison.overlap_fraction >= 0.50)
    );
}

#[test]
fn sleep_stage_label_validation_blocks_mismatched_or_untrusted_stage_labels() {
    let store = GooseStore::open_in_memory().unwrap();
    insert_sleep_stage_label(
        &store,
        "stage-label-mismatch",
        "rem",
        1_779_934_500_000,
        1_779_938_100_000,
        r#"{"official_labels_are_labels":true,"source":"user_export"}"#,
    );
    insert_sleep_stage_label(
        &store,
        "stage-label-untrusted",
        "deep",
        1_779_939_900_000,
        1_779_943_500_000,
        r#"{"source":"hand_edited_without_policy"}"#,
    );
    insert_sleep_stage_label_with_sleep_id(
        &store,
        "stage-label-other-sleep",
        "different-sleep-id",
        "rem",
        1_779_939_900_000,
        1_779_943_500_000,
        r#"{"review_policy":"user_owned_sleep_stage_label","source":"user_export"}"#,
    );
    insert_sleep_stage_label(
        &store,
        "stage-label-missing-source",
        "deep",
        1_779_934_500_000,
        1_779_938_100_000,
        r#"{"review_policy":"user_owned_sleep_stage_label"}"#,
    );
    insert_sleep_stage_label(
        &store,
        "stage-label-source-mismatch",
        "deep",
        1_779_934_500_000,
        1_779_938_100_000,
        r#"{"review_policy":"user_owned_sleep_stage_label","source":"other_export"}"#,
    );

    let report = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_quality_gate_input(),
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.label_count, 5);
    assert_eq!(report.compared_label_count, 1);
    assert_eq!(report.passing_label_count, 0);
    assert!(
        report
            .issues
            .contains(&"stage-label-mismatch:stage_label_kind_mismatch".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"stage-label-untrusted:label_provenance_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"stage-label-other-sleep:label_sleep_id_mismatch".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"stage-label-missing-source:label_provenance_source_missing".to_string())
    );
    assert!(
        report
            .issues
            .contains(&"stage-label-source-mismatch:label_provenance_source_mismatch".to_string())
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "stage_label_kind_mismatch" && action.scope == "sleep_stage.comparison"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_provenance_missing" && action.scope == "sleep_stage.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_sleep_id_mismatch" && action.scope == "sleep_stage.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_provenance_source_missing" && action.scope == "sleep_stage.labels"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "label_provenance_source_mismatch" && action.scope == "sleep_stage.labels"
    }));
}

#[test]
fn sleep_stage_label_validator_cli_reports_user_owned_stage_matches() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    seed_release_gate_sleep_stage_labels(&store);
    let input_path = tempdir.path().join("sleep-v1-input.json");
    let output_path = tempdir.path().join("sleep-stage-validation.json");
    write_json(&input_path, &sleep_v1_quality_gate_input());

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-sleep-stage-label-validator"))
            .args([
                "--db",
                db.to_str().unwrap(),
                "--input",
                input_path.to_str().unwrap(),
                "--output",
                output_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: SleepStageLabelValidationReport =
        serde_json::from_str(&fs::read_to_string(output_path).unwrap()).unwrap();
    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.passing_label_count, 2);
    assert_eq!(
        report.acceptance_summary.accepted_stage_kinds,
        vec!["deep".to_string(), "rem".to_string()]
    );
    assert_eq!(
        report.provenance["validation_policy"],
        serde_json::json!("sleep_v1_stage_segments_vs_user_owned_sleep_stage_labels")
    );
}

fn write_passing_sleep_v1_evidence_folder(evidence_dir: &std::path::Path) {
    let sleep_window_store_path = evidence_dir.join("sleep-window-store.sqlite");
    let store = GooseStore::open(&sleep_window_store_path).unwrap();
    seed_sleep_window_motion(&store);
    for (index, sleep_id) in [
        "packet-derived-sleep-2026-05-27",
        "packet-derived-sleep-2026-05-28",
        "packet-derived-sleep-2026-05-29",
    ]
    .into_iter()
    .enumerate()
    {
        insert_sleep_window_label_with_sleep_id(
            &store,
            &format!("manual-reviewed-window-pass-{index}"),
            sleep_id,
            1_779_919_800_000,
            1_779_933_000_000,
        );
    }

    let window_options = SleepWindowLabelValidationOptions {
        min_owned_captures_per_summary: 1,
        require_trusted_evidence: true,
        sleep_need_minutes: 240.0,
        ..Default::default()
    };
    let window_input = SleepWindowLabelValidationEvidenceInput {
        schema: "goose.sleep-window-label-validation-input.v1".to_string(),
        database_path: "sleep-window-store.sqlite".to_string(),
        start: "2026-05-27T22:00:00Z".to_string(),
        end: "2026-05-28T03:00:00Z".to_string(),
        options: window_options.clone(),
    };
    let physical_report = validate_historical_sync_physical_evidence(&physical_validation_input());
    let physical_input = physical_validation_input();
    let physical_template = historical_sync_physical_evidence_template(
        HistoricalSyncGeneration::Gen5,
        "strap-capture-2026-01-01".to_string(),
    );
    let sleep_v1_input = sleep_v1_quality_gate_input();
    let window_report = run_sleep_window_label_validation_for_store(
        &store,
        &window_input.database_path,
        &window_input.start,
        &window_input.end,
        window_options,
    )
    .unwrap();
    let stability_report = validate_sleep_v1_explanation_and_stability(
        &sleep_v1_input,
        SleepV1ExplanationStabilityOptions::default(),
    );
    seed_release_gate_sleep_stage_labels(&store);
    let stage_report = validate_sleep_v1_stage_labels_for_store(
        &store,
        &sleep_v1_input,
        SleepStageLabelValidationOptions::default(),
    )
    .unwrap();
    let benchmark_report = compare_sleep_v1_goose_to_reference(&sleep_v1_input).unwrap();
    let release_gate_input = SleepV1ReleaseGateInput {
        physical_historical_sync: Some(physical_report.clone()),
        sleep_window_label_validation: Some(window_report.clone()),
        sleep_stage_label_validation: Some(stage_report.clone()),
        explanation_stability: Some(stability_report.clone()),
        benchmark_comparisons: vec![benchmark_report.clone()],
        min_hand_reviewed_window_comparisons: 3,
        min_stage_label_comparisons: 1,
        min_benchmark_comparisons: 1,
    };
    let release_gate_report = validate_sleep_v1_release_gates(&release_gate_input);

    write_json(
        &evidence_dir.join("historical-sync-template.json"),
        &physical_template,
    );
    write_json(
        &evidence_dir.join("historical-sync-evidence.json"),
        &physical_input,
    );
    write_json(
        &evidence_dir.join("sleep-window-validation-input.json"),
        &window_input,
    );
    write_json(&evidence_dir.join("sleep-v1-input.json"), &sleep_v1_input);
    write_json(
        &evidence_dir.join("sleep-v1-release-gate-input.json"),
        &release_gate_input,
    );
    write_json(
        &evidence_dir.join("historical-sync-validation.json"),
        &physical_report,
    );
    write_json(
        &evidence_dir.join("sleep-window-validation.json"),
        &window_report,
    );
    write_json(
        &evidence_dir.join("sleep-stage-validation.json"),
        &stage_report,
    );
    write_json(
        &evidence_dir.join("sleep-v1-stability.json"),
        &stability_report,
    );
    write_json(
        &evidence_dir.join("sleep-v1-benchmark.json"),
        &benchmark_report,
    );
    write_json(
        &evidence_dir.join("sleep-v1-release-gate.json"),
        &release_gate_report,
    );
}

fn add_cli_benchmark_coverage_fields(evidence_dir: &std::path::Path, corrupt: bool) {
    let benchmark_path = evidence_dir.join("sleep-v1-benchmark.json");
    let input_path = evidence_dir.join("sleep-v1-input.json");
    let release_input_path = evidence_dir.join("sleep-v1-release-gate-input.json");
    let input_raw = fs::read_to_string(&input_path).unwrap();
    let input_value: serde_json::Value = serde_json::from_str(&input_raw).unwrap();
    let input_ids_count = input_value
        .get("input_ids")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let mut benchmark_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&benchmark_path).unwrap()).unwrap();
    let input_bytes = input_raw.len() as u64 + u64::from(corrupt);
    benchmark_report["data_coverage"]["input_path"] =
        serde_json::json!(input_path.to_str().unwrap());
    benchmark_report["data_coverage"]["input_bytes"] = serde_json::json!(input_bytes);
    benchmark_report["data_coverage"]["input_ids_count"] = serde_json::json!(input_ids_count);
    benchmark_report["data_coverage"]["start_time"] = benchmark_report["start_time"].clone();
    benchmark_report["data_coverage"]["end_time"] = benchmark_report["end_time"].clone();
    benchmark_report["data_coverage"]["output_present"] = serde_json::json!(true);
    benchmark_report["data_coverage"]["quality_flag_count"] = serde_json::json!(0);
    benchmark_report["data_coverage"]["error_count"] = serde_json::json!(0);
    write_json(&benchmark_path, &benchmark_report);

    let mut release_input: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&release_input_path).unwrap()).unwrap();
    release_input["benchmark_comparisons"] = serde_json::json!([benchmark_report]);
    write_json(&release_input_path, &release_input);
}

fn seed_sleep_window_motion(store: &GooseStore) {
    for (index, (captured_at, sample_value)) in [
        ("2026-05-27T22:00:00Z", 10000),
        ("2026-05-27T23:00:00Z", 1000),
        ("2026-05-28T00:00:00Z", 1000),
        ("2026-05-28T01:00:00Z", 1000),
        ("2026-05-28T02:00:00Z", 1000),
    ]
    .into_iter()
    .enumerate()
    {
        let frames = vec![CapturedFrameInput {
            evidence_id: format!("sleep-window-motion-{index}"),
            frame_id: Some(format!("sleep-window-motion-{index}.frame.0")),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: captured_at.to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: k10_motion_frame_hex_with_value(sample_value),
            sensitivity: "user-owned-capture".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        }];
        let report = import_captured_frame_batch(
            store,
            &frames,
            CapturedFrameBatchOptions {
                parser_version: "goose-core/test",
                active_device_id: None,
            },
        )
        .unwrap();
        assert!(report.pass, "{:?}", report.issues);
    }
    seed_sleep_window_heart_rate(store);
}

fn seed_sleep_window_heart_rate(store: &GooseStore) {
    for (index, (captured_at, marker_value)) in [
        ("2026-05-27T22:00:00Z", 78),
        ("2026-05-27T23:00:00Z", 70),
        ("2026-05-28T00:00:00Z", 64),
        ("2026-05-28T01:00:00Z", 61),
        ("2026-05-28T02:00:00Z", 63),
    ]
    .into_iter()
    .enumerate()
    {
        let frames = vec![CapturedFrameInput {
            evidence_id: format!("sleep-window-heart-rate-{index}"),
            frame_id: Some(format!("sleep-window-heart-rate-{index}.frame.0")),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: captured_at.to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: historical_k18_frame_hex(marker_value),
            sensitivity: "user-owned-capture".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        }];
        let report = import_captured_frame_batch(
            store,
            &frames,
            CapturedFrameBatchOptions {
                parser_version: "goose-core/test",
                active_device_id: None,
            },
        )
        .unwrap();
        assert!(report.pass, "{:?}", report.issues);
    }
}

fn insert_sleep_window_label(
    store: &GooseStore,
    label_id: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) {
    insert_sleep_window_label_with_sleep_id(
        store,
        label_id,
        "packet-derived-sleep-2026-05-27",
        start_time_unix_ms,
        end_time_unix_ms,
    );
}

fn insert_sleep_window_label_with_sleep_id(
    store: &GooseStore,
    label_id: &str,
    sleep_id: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) {
    let value_json = serde_json::json!({
        "corrected_start_time_unix_ms": start_time_unix_ms,
        "corrected_end_time_unix_ms": end_time_unix_ms,
        "review_source": "hand_reviewed"
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id,
                sleep_id: Some(sleep_id),
                label_type: "sleep_window",
                start_time_unix_ms,
                end_time_unix_ms,
                value_json: &value_json,
                source: "manual",
                confidence: Some(0.95),
                provenance_json: r#"{"review_policy":"hand_reviewed_sleep_window","source":"manual"}"#,
            })
            .unwrap()
    );
}

fn insert_sleep_stage_label(
    store: &GooseStore,
    label_id: &str,
    stage_kind: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    provenance_json: &str,
) {
    insert_sleep_stage_label_with_sleep_id(
        store,
        label_id,
        "sleep-v1-quality-gate-input",
        stage_kind,
        start_time_unix_ms,
        end_time_unix_ms,
        provenance_json,
    );
}

fn insert_sleep_stage_label_with_sleep_id(
    store: &GooseStore,
    label_id: &str,
    sleep_id: &str,
    stage_kind: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    provenance_json: &str,
) {
    let value_json = serde_json::json!({
        "stage_kind": stage_kind,
    })
    .to_string();
    assert!(
        store
            .insert_sleep_correction_label(SleepCorrectionLabelInput {
                label_id,
                sleep_id: Some(sleep_id),
                label_type: "sleep_stage",
                start_time_unix_ms,
                end_time_unix_ms,
                value_json: &value_json,
                source: "user_export",
                confidence: Some(0.95),
                provenance_json,
            })
            .unwrap()
    );
}

fn seed_release_gate_sleep_stage_labels(store: &GooseStore) {
    insert_sleep_stage_label(
        store,
        "release-stage-label-deep",
        "deep",
        1_779_934_500_000,
        1_779_938_100_000,
        r#"{"review_policy":"user_owned_sleep_stage_label","source":"user_export"}"#,
    );
    insert_sleep_stage_label(
        store,
        "release-stage-label-rem",
        "rem",
        1_779_939_900_000,
        1_779_943_500_000,
        r#"{"official_labels_are_labels":true,"source":"user_export"}"#,
    );
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
            evidence_id: format!("sleep-v1-physical-raw-evidence-{index}"),
            sha256: format!("{index:064x}"),
            observation_kind: kind.to_string(),
            observation_name: name.to_string(),
            sequence,
            capture_session_id: Some("strap-capture-2026-01-01".to_string()),
        },
    )
    .collect()
}

fn write_json(path: &std::path::Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn sleep_v1_quality_gate_input() -> SleepV1Input {
    SleepV1Input {
        sleep: SleepInput {
            start_time: "2026-05-27T22:30:00Z".to_string(),
            end_time: "2026-05-28T06:30:00Z".to_string(),
            sleep_duration_minutes: 420.0,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes: 480.0,
            midpoint_deviation_minutes: 30.0,
            disturbance_count: 2,
            sleep_latency_minutes: 18.0,
            wake_after_sleep_onset_minutes: 42.0,
            wake_episode_count: 2,
            stage_minutes: BTreeMap::from([
                ("awake".to_string(), 60.0),
                ("core".to_string(), 210.0),
                ("deep".to_string(), 90.0),
                ("rem".to_string(), 120.0),
            ]),
            heart_rate_dip_percent: Some(12.5),
            input_ids: vec!["sleep-v1-quality-gate-input".to_string()],
        },
        model_status: SleepModelStatusInput {
            sleep_permission_granted: true,
            imported_platform_sleep_nights: 10,
            trusted_goose_sleep_nights: 2,
            motion_coverage_fraction: Some(0.94),
            heart_rate_coverage_fraction: Some(0.82),
            ..Default::default()
        },
        rolling_sleep_debt_minutes: 90.0,
        bedtime_deviation_minutes: 20.0,
        wake_time_deviation_minutes: 15.0,
        sleep_hr_average_bpm: Some(61.0),
        sleep_hr_min_bpm: Some(54.0),
        sleep_hr_trend_bpm_per_hour: Some(-1.2),
        naps_minutes: 25.0,
        prior_day_strain: Some(8.5),
        data_coverage_fraction: Some(0.92),
        stage_segments: vec![
            sleep_stage_segment(
                "awake",
                "2026-05-27T22:30:00Z",
                "2026-05-27T22:45:00Z",
                15.0,
                0.82,
            ),
            sleep_stage_segment(
                "core",
                "2026-05-27T22:45:00Z",
                "2026-05-28T02:15:00Z",
                210.0,
                0.84,
            ),
            sleep_stage_segment(
                "deep",
                "2026-05-28T02:15:00Z",
                "2026-05-28T03:45:00Z",
                90.0,
                0.80,
            ),
            sleep_stage_segment(
                "rem",
                "2026-05-28T03:45:00Z",
                "2026-05-28T05:45:00Z",
                120.0,
                0.82,
            ),
            sleep_stage_segment(
                "awake",
                "2026-05-28T05:45:00Z",
                "2026-05-28T06:30:00Z",
                45.0,
                0.82,
            ),
        ],
        prior_nights: vec![sleep_v1_prior_night(
            "sleep-history-quality-gate-previous",
            "2026-05-26T22:35:00Z",
            "2026-05-27T06:35:00Z",
        )],
        ..Default::default()
    }
}

fn sleep_stage_segment(
    stage_kind: &str,
    start_time: &str,
    end_time: &str,
    duration_minutes: f64,
    confidence_0_to_1: f64,
) -> SleepStageSegment {
    SleepStageSegment {
        stage_kind: stage_kind.to_string(),
        start_time: start_time.to_string(),
        end_time: end_time.to_string(),
        duration_minutes,
        confidence_0_to_1,
        stage_probabilities: BTreeMap::new(),
    }
}

fn sleep_v1_prior_night(
    night_id: &str,
    start_time: &str,
    end_time: &str,
) -> SleepNightHistoryInput {
    SleepNightHistoryInput {
        night_id: night_id.to_string(),
        start_time: start_time.to_string(),
        end_time: end_time.to_string(),
        sleep_duration_minutes: 420.0,
        sleep_need_minutes: 480.0,
        time_in_bed_minutes: 480.0,
        awake_minutes: 60.0,
        sleep_latency_minutes: 18.0,
        wake_after_sleep_onset_minutes: 42.0,
        wake_episode_count: 2,
        stage_minutes: BTreeMap::from([
            ("core".to_string(), 210.0),
            ("deep".to_string(), 90.0),
            ("rem".to_string(), 120.0),
        ]),
        heart_rate_dip_percent: Some(12.5),
        sleep_hr_average_bpm: Some(61.0),
        sleep_hr_min_bpm: Some(54.0),
        pre_sleep_awake_hr_average_bpm: Some(68.0),
        sleep_hr_trend_bpm_per_hour: Some(-1.2),
        bedtime_deviation_minutes: 20.0,
        wake_time_deviation_minutes: 15.0,
        midpoint_deviation_minutes: 30.0,
        naps_minutes: 0.0,
        confidence_0_to_1: 0.90,
        source: "healthkit".to_string(),
        excluded_from_baseline: false,
    }
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

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
