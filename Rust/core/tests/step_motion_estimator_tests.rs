use goose_core::{
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    protocol::{DeviceType, PACKET_TYPE_REALTIME_RAW_DATA, build_v5_payload_frame},
    step_motion_estimator::{
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_ID, GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_VERSION,
        RawMotionStepEstimateOptions, run_raw_motion_step_estimate_for_store,
    },
    store::GooseStore,
};
use serde_json::json;

#[test]
fn raw_motion_step_estimator_matches_counted_steps_without_writing_metrics() {
    let store = GooseStore::open_in_memory().unwrap();
    import_raw_motion_step_frame(
        &store,
        "user-owned-capture",
        "2026-06-02T12:00:00Z",
        &[10, 25, 40, 55, 70],
    );

    let report = run_raw_motion_step_estimate_for_store(
        &store,
        "test-db",
        "2026-06-02T11:59:00Z",
        "2026-06-02T12:01:00Z",
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sample_rate_hz: 50.0,
            peak_threshold_i16: 1_200.0,
            min_peak_spacing_samples: 10,
            manual_step_delta: Some(5),
            official_whoop_step_delta: Some(5),
            tolerance_steps: 0,
            label_provenance: Some(json!({
                "source": "manual_plus_official_app",
                "official_labels_are_labels": true
            })),
            date_key: None,
            timezone: None,
            write_metric: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.schema, "goose.raw-motion-step-estimate-report.v1");
    assert_eq!(report.algorithm_id, GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_ID);
    assert_eq!(
        report.algorithm_version,
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_VERSION
    );
    assert_eq!(report.source_kind_if_promoted, "local_estimate");
    assert_eq!(report.promotion_status, "validated_candidate");
    assert!(report.user_visible_value_allowed);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.trusted_candidate_frame_count, 1);
    assert_eq!(report.estimated_steps, Some(5));
    assert_eq!(report.estimated_cadence_spm, Some(150.0));
    assert_eq!(report.provided_label_count, 2);
    assert_eq!(report.matching_label_count, 2);
    assert_eq!(report.matches_manual_label, Some(true));
    assert_eq!(report.matches_official_label, Some(true));
    assert_eq!(report.confidence, 0.65);
    assert!(!report.write_metric);
    assert_eq!(report.daily_metric_id, None);
    assert!(!report.daily_metric_written);
    assert_eq!(report.metric_provenance_id, None);
    assert!(!report.metric_provenance_written);
    assert_eq!(report.frames[0].peak_count, 5);
    assert_eq!(report.frames[0].estimated_steps, 5);
    assert_eq!(report.frames[0].cadence_spm, Some(150.0));
    assert_eq!(store.table_count("step_counter_samples").unwrap(), 0);
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
}

#[test]
fn raw_motion_step_estimator_writes_validated_local_estimate_metric_when_requested() {
    let store = GooseStore::open_in_memory().unwrap();
    import_raw_motion_step_frame(
        &store,
        "user-owned-capture",
        "2026-06-02T12:00:00Z",
        &[10, 25, 40, 55, 70],
    );

    let report = run_raw_motion_step_estimate_for_store(
        &store,
        "test-db",
        "2026-06-02T11:59:00Z",
        "2026-06-02T12:01:00Z",
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            manual_step_delta: Some(5),
            official_whoop_step_delta: Some(5),
            tolerance_steps: 0,
            label_provenance: Some(json!({
                "source": "manual_plus_official_app",
                "official_labels_are_labels": true
            })),
            date_key: Some("2026-06-02".to_string()),
            timezone: Some("Europe/London".to_string()),
            write_metric: true,
            ..RawMotionStepEstimateOptions::default()
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.write_metric);
    assert_eq!(report.daily_metric_written, true);
    assert_eq!(report.metric_provenance_written, true);
    assert_eq!(
        report.daily_metric_id.as_deref(),
        Some("daily-activity-raw-motion-steps-2026-06-02-europe-london-local-estimate-v0")
    );
    assert_eq!(
        report.metric_provenance_id.as_deref(),
        Some("prov-daily-activity-raw-motion-steps-2026-06-02-europe-london-local-estimate-v0")
    );

    let metric = store
        .daily_activity_metric(report.daily_metric_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(metric.source_kind, "local_estimate");
    assert_eq!(metric.steps, Some(5));
    assert_eq!(metric.average_cadence_spm, Some(150.0));
    assert_eq!(metric.confidence, 0.65);
    let inputs: serde_json::Value = serde_json::from_str(&metric.inputs_json).unwrap();
    assert_eq!(inputs["manual_step_delta_label"], 5);
    assert_eq!(inputs["official_whoop_step_delta_label"], 5);
    assert_eq!(
        inputs["label_provenance"]["official_labels_are_labels"],
        true
    );
    let provenance: serde_json::Value = serde_json::from_str(&metric.provenance_json).unwrap();
    assert_eq!(
        provenance["algorithm"],
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_ID
    );
    assert_eq!(
        provenance["algorithm_version"],
        GOOSE_STEPS_RAW_MOTION_ESTIMATE_V0_VERSION
    );
    assert_eq!(provenance["source_kind"], "local_estimate");
    assert_eq!(
        provenance["official_labels_policy"],
        "validation_label_only"
    );
    let provenance_rows = store
        .metric_provenance_for_metric("daily_activity", &metric.daily_metric_id)
        .unwrap();
    assert_eq!(provenance_rows.len(), 1);
}

#[test]
fn raw_motion_step_estimator_blocks_official_label_without_provenance_marker() {
    let store = GooseStore::open_in_memory().unwrap();
    import_raw_motion_step_frame(
        &store,
        "user-owned-capture",
        "2026-06-02T12:00:00Z",
        &[10, 25, 40, 55, 70],
    );

    let report = run_raw_motion_step_estimate_for_store(
        &store,
        "test-db",
        "2026-06-02T11:59:00Z",
        "2026-06-02T12:01:00Z",
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sample_rate_hz: 50.0,
            peak_threshold_i16: 1_200.0,
            min_peak_spacing_samples: 10,
            official_whoop_step_delta: Some(5),
            tolerance_steps: 0,
            ..RawMotionStepEstimateOptions::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.estimated_steps, Some(5));
    assert_eq!(report.matching_label_count, 1);
    assert_eq!(report.promotion_status, "candidate_unvalidated");
    assert!(!report.user_visible_value_allowed);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "official_label_provenance_missing")
    );
}

#[test]
fn raw_motion_step_estimator_requires_validation_labels_before_writing_metric() {
    let store = GooseStore::open_in_memory().unwrap();
    import_raw_motion_step_frame(
        &store,
        "user-owned-capture",
        "2026-06-02T12:00:00Z",
        &[10, 25, 40, 55, 70],
    );

    let report = run_raw_motion_step_estimate_for_store(
        &store,
        "test-db",
        "2026-06-02T11:59:00Z",
        "2026-06-02T12:01:00Z",
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            date_key: Some("2026-06-02".to_string()),
            timezone: Some("Europe/London".to_string()),
            write_metric: true,
            ..RawMotionStepEstimateOptions::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.estimated_steps, Some(5));
    assert_eq!(report.promotion_status, "candidate_unvalidated");
    assert!(!report.user_visible_value_allowed);
    assert_eq!(report.daily_metric_id, None);
    assert!(!report.daily_metric_written);
    assert_eq!(report.metric_provenance_id, None);
    assert!(!report.metric_provenance_written);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_step_estimator_validation_label")
    );
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
}

#[test]
fn raw_motion_step_estimator_surfaces_truncated_single_axis_candidates_without_promotion() {
    let store = GooseStore::open_in_memory().unwrap();
    import_partial_axis_raw_motion_step_frame(
        &store,
        "user-owned-capture",
        "2026-06-02T12:00:00Z",
        &[10, 25, 40, 55, 70],
    );

    let report = run_raw_motion_step_estimate_for_store(
        &store,
        "test-db",
        "2026-06-02T11:59:00Z",
        "2026-06-02T12:01:00Z",
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sample_rate_hz: 50.0,
            peak_threshold_i16: 1_200.0,
            min_peak_spacing_samples: 10,
            ..RawMotionStepEstimateOptions::default()
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.trusted_candidate_frame_count, 1);
    assert_eq!(report.estimated_steps, Some(5));
    assert_eq!(report.promotion_status, "candidate_unvalidated");
    assert!(!report.user_visible_value_allowed);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_step_estimator_validation_label")
    );
    assert_eq!(report.frames[0].axis_count, 1);
    assert_eq!(report.frames[0].sample_count, 100);
    assert!(
        report.frames[0]
            .quality_flags
            .iter()
            .any(|flag| flag == "frame_truncated")
    );
    assert!(
        report.frames[0]
            .quality_flags
            .iter()
            .any(|flag| flag == "single_axis_motion_estimator")
    );
    assert!(
        report.frames[0]
            .quality_flags
            .iter()
            .any(|flag| flag == "partial_axis_motion_estimator")
    );
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
}

fn import_raw_motion_step_frame(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    peak_indices: &[usize],
) {
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("raw-motion-step-estimator-{sensitivity}-{captured_at}"),
        frame_id: Some(format!(
            "raw-motion-step-estimator-{sensitivity}-{captured_at}.frame.0"
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: k10_motion_step_frame_hex(peak_indices),
        sensitivity: sensitivity.to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let report = import_captured_frame_batch(
        store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/raw-motion-step-estimator-test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(report.pass, "{:?}", report.issues);
}

fn import_partial_axis_raw_motion_step_frame(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    peak_indices: &[usize],
) {
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("partial-axis-raw-motion-step-estimator-{sensitivity}-{captured_at}"),
        frame_id: Some(format!(
            "partial-axis-raw-motion-step-estimator-{sensitivity}-{captured_at}.frame.0"
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: partial_axis_k10_motion_step_frame_hex(peak_indices),
        sensitivity: sensitivity.to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let report = import_captured_frame_batch(
        store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/raw-motion-step-estimator-test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(report.pass, "{:?}", report.issues);
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

fn partial_axis_k10_motion_step_frame_hex(peak_indices: &[usize]) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 84;
    for index in peak_indices {
        put_i16(&mut payload, 85 + index * 2, 4_000);
    }
    let mut frame = build_v5_payload_frame(&payload);
    frame.truncate(8 + 285);
    hex::encode(frame)
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
