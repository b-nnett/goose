use goose_core::{
    activity_candidates::{
        ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA, ActivityCandidateClassifierInput,
        ActivityCandidateClassifierOptions, ActivityCommandSyncEvidence,
        ActivityEvidenceProvenance, ActivityFeatureWindowInput, ActivityGravitySample,
        ActivityHeartRateEvidence, ActivityMotionEvidence, run_activity_candidate_classifier,
    },
    capture_correlation::{CaptureCorrelationOptions, run_capture_correlation_for_store},
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    metric_readiness::{
        MetricInputReadinessOptions, run_metric_input_readiness,
        run_metric_input_readiness_with_activity_classifier,
    },
    protocol::{DeviceType, PACKET_TYPE_HISTORICAL_DATA, build_v5_payload_frame},
    store::GooseStore,
};
use serde_json::json;

const K10_FRAME: &str = "aa0164000001fb212b0a010000000000000000000000000000480000000000000000000000000000 00000000000000000000000000000000000000000000000000000000000000000000000000000000 000000000000000000000000000100feff0300000000000068cc8271";

#[test]
fn metric_input_readiness_marks_motion_ready_after_trusted_extraction_exists() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![
        CapturedFrameInput {
            evidence_id: "app.owned.k10".to_string(),
            frame_id: Some("app.owned.k10.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:00Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: K10_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
        CapturedFrameInput {
            evidence_id: "app.owned.k18".to_string(),
            frame_id: Some("app.owned.k18.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:01Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: historical_k18_frame_hex(77),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
    ];
    let import = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import.pass, "{:?}", import.issues);

    let correlation = run_capture_correlation_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: false,
        },
    )
    .unwrap();
    let report = run_metric_input_readiness(
        &correlation,
        MetricInputReadinessOptions {
            require_scores_ready: false,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.family_count, 6);
    assert_eq!(report.ready_family_count, 2);
    let stress = report
        .families
        .iter()
        .find(|family| family.metric_family == "stress")
        .unwrap();
    let motion = stress
        .inputs
        .iter()
        .find(|input| input.input_name == "motion_intensity_0_to_1")
        .unwrap();
    assert_eq!(motion.candidate_observation_count, 1);
    assert_eq!(motion.trusted_evidence_count, 1);
    assert_eq!(motion.status, "ready");
    assert!(motion.blocker_reasons.is_empty());
    assert!(motion.next_actions.is_empty());
    let heart_rate = stress
        .inputs
        .iter()
        .find(|input| input.input_name == "heart_rate_bpm")
        .unwrap();
    assert_eq!(heart_rate.candidate_observation_count, 1);
    assert_eq!(heart_rate.trusted_evidence_count, 1);
    assert_eq!(heart_rate.status, "ready");
    assert!(heart_rate.blocker_reasons.is_empty());
    assert!(!stress.score_ready);
    let resting_hr = stress
        .inputs
        .iter()
        .find(|input| input.input_name == "resting_hr_bpm")
        .unwrap();
    assert_eq!(resting_hr.status, "ready");
    assert!(
        stress
            .blocker_reasons
            .iter()
            .any(|reason| reason.contains("hrv_rmssd_ms"))
    );
    assert!(stress.next_actions.iter().any(|action| {
        action.scope == "hrv_rmssd_ms"
            && action
                .action
                .contains("Import or live-capture owned frames that decode as r17_optical_or_labrador_filtered")
    }));
    let recovery = report
        .families
        .iter()
        .find(|family| family.metric_family == "recovery")
        .unwrap();
    let skin_temp = recovery
        .inputs
        .iter()
        .find(|input| input.input_name == "skin_temp_delta_c")
        .unwrap();
    assert_eq!(
        skin_temp.source_signal,
        "normal_history_or_event_temperature_candidate"
    );
    assert_eq!(
        skin_temp.required_summary_kinds,
        vec![
            "normal_history".to_string(),
            "event_temperature_level".to_string()
        ]
    );
    assert_eq!(skin_temp.candidate_observation_count, 1);
    assert_eq!(skin_temp.trusted_evidence_count, 1);
    assert_eq!(skin_temp.status, "blocked");
    assert_eq!(
        skin_temp.blocker_reasons,
        vec!["temperature_units_unverified".to_string()]
    );
    assert!(
        skin_temp
            .next_actions
            .iter()
            .any(|action| action.reason == "temperature_units_unverified")
    );
    let strain = report
        .families
        .iter()
        .find(|family| family.metric_family == "strain")
        .unwrap();
    for ready_input_name in [
        "average_hr_bpm",
        "max_hr_bpm",
        "duration_minutes",
        "resting_hr_bpm",
        "hr_zone_minutes",
    ] {
        let input = strain
            .inputs
            .iter()
            .find(|input| input.input_name == ready_input_name)
            .unwrap();
        assert_eq!(input.status, "ready", "{ready_input_name}");
    }
    assert!(strain.score_ready);
    let sleep = report
        .families
        .iter()
        .find(|family| family.metric_family == "sleep")
        .unwrap();
    for ready_input_name in [
        "sleep_duration_minutes",
        "time_in_bed_minutes",
        "midpoint_deviation_minutes",
        "disturbance_count",
    ] {
        let input = sleep
            .inputs
            .iter()
            .find(|input| input.input_name == ready_input_name)
            .unwrap();
        assert_eq!(input.candidate_observation_count, 1, "{ready_input_name}");
        assert_eq!(input.trusted_evidence_count, 1, "{ready_input_name}");
        assert_eq!(input.status, "ready", "{ready_input_name}");
    }
    let sleep_need = sleep
        .inputs
        .iter()
        .find(|input| input.input_name == "sleep_need_minutes")
        .unwrap();
    assert_eq!(sleep_need.status, "ready");
    assert!(sleep.score_ready);
}

#[test]
fn metric_input_readiness_keeps_hrv_blocked_until_r17_interval_scale_is_validated() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app.owned.r17".to_string(),
        frame_id: Some("app.owned.r17.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-27T04:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: r17_frame_hex(&[800, 810, 790, 800]),
        sensitivity: "user-owned-live-notification".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let import = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import.pass, "{:?}", import.issues);

    let correlation = run_capture_correlation_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: false,
        },
    )
    .unwrap();
    let report = run_metric_input_readiness(
        &correlation,
        MetricInputReadinessOptions {
            require_scores_ready: false,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    let hrv = report
        .families
        .iter()
        .find(|family| family.metric_family == "hrv")
        .unwrap();
    let rr_intervals = hrv
        .inputs
        .iter()
        .find(|input| input.input_name == "rr_intervals_ms")
        .unwrap();
    assert_eq!(
        rr_intervals.required_summary_kinds,
        vec!["r17_optical_or_labrador_filtered".to_string()]
    );
    assert_eq!(rr_intervals.candidate_observation_count, 1);
    assert_eq!(rr_intervals.trusted_evidence_count, 1);
    assert_eq!(rr_intervals.status, "blocked");
    assert!(
        rr_intervals
            .blocker_reasons
            .iter()
            .any(|reason| reason == "hrv_rr_interval_scale_unverified")
    );
    assert!(!hrv.score_ready);
    assert!(
        hrv.next_actions
            .iter()
            .any(|action| action.scope == "rr_intervals_ms"
                && action.reason == "hrv_rr_interval_scale_unverified"
                && action.action.contains("Validate the R17 interval scale"))
    );

    let stress = report
        .families
        .iter()
        .find(|family| family.metric_family == "stress")
        .unwrap();
    let baseline = stress
        .inputs
        .iter()
        .find(|input| input.input_name == "hrv_baseline_rmssd_ms")
        .unwrap();
    assert_eq!(baseline.candidate_observation_count, 1);
    assert_eq!(baseline.trusted_evidence_count, 1);
    assert_eq!(baseline.status, "blocked");
    assert!(
        baseline
            .blocker_reasons
            .iter()
            .any(|reason| reason == "hrv_rr_interval_scale_unverified")
    );
    assert!(!stress.score_ready);
}

#[test]
fn metric_input_readiness_can_fail_when_scores_are_required() {
    let store = GooseStore::open_in_memory().unwrap();
    let correlation = run_capture_correlation_for_store(
        &store,
        "empty-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        CaptureCorrelationOptions::default(),
    )
    .unwrap();
    let report = run_metric_input_readiness(
        &correlation,
        MetricInputReadinessOptions {
            require_scores_ready: true,
        },
    );

    assert!(!report.pass);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("capture_correlation_report_not_passed"))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("recovery is not ready"))
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "capture_correlation"
            && action
                .action
                .contains("Run Capture Trust and satisfy its owned-capture next actions")
    }));
    let recovery = report
        .families
        .iter()
        .find(|family| family.metric_family == "recovery")
        .unwrap();
    assert!(recovery.next_actions.iter().any(|action| {
        action.scope == "respiratory_rate_rpm"
            && action
                .action
                .contains("Import or live-capture owned frames that decode as normal_history")
    }));
    assert!(recovery.next_actions.iter().any(|action| {
        action.scope == "respiratory_rate_rpm"
            && action.action.contains(
                "Validate normal-history respiratory-rate candidate offsets against owned captures",
            )
    }));
    let respiratory = recovery
        .inputs
        .iter()
        .find(|input| input.input_name == "respiratory_rate_rpm")
        .unwrap();
    assert_eq!(
        respiratory.source_signal,
        "normal_history_respiratory_rate_candidate"
    );
    assert_eq!(
        respiratory.required_summary_kinds,
        vec!["normal_history".to_string()]
    );
    assert_eq!(
        respiratory.blocker_reasons,
        vec![
            "no trusted owned capture evidence for normal_history".to_string(),
            "respiratory_rate_semantics_unverified".to_string(),
        ]
    );
    assert!(
        respiratory
            .next_actions
            .iter()
            .any(|action| action.reason == "no trusted owned capture evidence for normal_history")
    );
    assert!(
        respiratory
            .next_actions
            .iter()
            .any(|action| action.reason == "respiratory_rate_semantics_unverified")
    );
    let skin_temp = recovery
        .inputs
        .iter()
        .find(|input| input.input_name == "skin_temp_delta_c")
        .unwrap();
    assert_eq!(
        skin_temp.source_signal,
        "normal_history_or_event_temperature_candidate"
    );
    assert_eq!(
        skin_temp.blocker_reasons,
        vec![
            "no trusted owned capture evidence for normal_history|event_temperature_level"
                .to_string(),
            "temperature_units_unverified".to_string(),
        ]
    );
    assert!(skin_temp.next_actions.iter().any(|action| {
        action.reason
            == "no trusted owned capture evidence for normal_history|event_temperature_level"
    }));
    assert!(skin_temp.next_actions.iter().any(|action| {
        action.reason == "temperature_units_unverified"
            && action
                .action
                .contains("Validate temperature event/history units")
    }));
}

#[test]
fn metric_input_readiness_reports_missing_activity_classifier_evidence_by_default() {
    let correlation = trusted_motion_correlation();
    let report = run_metric_input_readiness(
        &correlation,
        MetricInputReadinessOptions {
            require_scores_ready: false,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    assert!(
        !report
            .activity_session_promotion
            .classification_evidence_available
    );
    assert!(!report.activity_session_promotion.pass);
    assert_eq!(
        report.activity_session_promotion.blocker_reasons,
        vec!["classification_evidence_missing".to_string()]
    );
    assert!(
        report
            .activity_session_promotion
            .next_actions
            .iter()
            .any(|action| {
                action.scope == "activity_session_promotion"
                    && action.reason == "classification_evidence_missing"
                    && action
                        .action
                        .contains("Run the activity candidate classifier")
            })
    );
}

#[test]
fn metric_input_readiness_reports_activity_session_promotion_blockers_from_classifier_output() {
    let correlation = trusted_motion_correlation();
    let classifier_report = run_activity_candidate_classifier(&classifier_input(vec![
        window_input(
            "window-hr-only",
            Some(heart_rate_evidence(122.0, 0.95, "hr-only")),
            None,
            None,
            false,
        ),
        window_input(
            "window-motion-only",
            None,
            Some(motion_evidence(0.40, "motion-only")),
            None,
            false,
        ),
        window_input(
            "window-hr-motion",
            Some(heart_rate_evidence(128.0, 0.96, "hr-motion")),
            Some(motion_evidence(0.96, "motion-sync")),
            None,
            false,
        ),
        window_input(
            "window-candidate",
            Some(heart_rate_evidence(132.0, 0.98, "candidate-hr")),
            Some(motion_evidence(0.98, "candidate-motion")),
            Some(command_sync_evidence(true, 0.97, "candidate-sync")),
            true,
        ),
    ]));
    assert!(!classifier_report.pass, "{:?}", classifier_report.issues);

    let report = run_metric_input_readiness_with_activity_classifier(
        &correlation,
        &classifier_report,
        MetricInputReadinessOptions {
            require_scores_ready: false,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    let promotion = &report.activity_session_promotion;
    assert!(promotion.classification_evidence_available);
    assert!(!promotion.pass);
    assert_eq!(promotion.window_count, 4);
    assert_eq!(promotion.candidate_window_count, 1);
    assert_eq!(promotion.unknown_window_count, 3);
    assert_eq!(promotion.blocked_window_count, 0);
    assert_eq!(
        promotion.blocker_reasons,
        vec![
            "low_classification_confidence".to_string(),
            "missing_command_sync".to_string(),
            "missing_heart_rate".to_string(),
            "missing_motion".to_string(),
        ]
    );
    assert!(promotion.next_actions.iter().any(|action| {
        action.scope == "activity_session_promotion"
            && action.reason == "missing_heart_rate"
            && action
                .action
                .contains("Capture trusted heart-rate evidence")
    }));
    assert!(promotion.next_actions.iter().any(|action| {
        action.scope == "activity_session_promotion"
            && action.reason == "missing_motion"
            && action.action.contains("Capture motion evidence")
    }));
    assert!(promotion.next_actions.iter().any(|action| {
        action.scope == "activity_session_promotion"
            && action.reason == "missing_command_sync"
            && action.action.contains("Carry command-sync evidence")
    }));
    assert!(promotion.next_actions.iter().any(|action| {
        action.scope == "activity_session_promotion"
            && action.reason == "low_classification_confidence"
            && action
                .action
                .contains("Tighten the window or improve evidence quality")
    }));
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

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn trusted_motion_correlation() -> goose_core::capture_correlation::CaptureCorrelationReport {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![
        CapturedFrameInput {
            evidence_id: "app.owned.k10".to_string(),
            frame_id: Some("app.owned.k10.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:00Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: K10_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
        CapturedFrameInput {
            evidence_id: "app.owned.k18".to_string(),
            frame_id: Some("app.owned.k18.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:01Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: historical_k18_frame_hex(77),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
    ];
    let import = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import.pass, "{:?}", import.issues);

    run_capture_correlation_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: false,
        },
    )
    .unwrap()
}

fn provenance(source: &str, evidence_id: &str) -> ActivityEvidenceProvenance {
    ActivityEvidenceProvenance {
        source: source.to_string(),
        evidence_id: Some(evidence_id.to_string()),
        capture_session_id: Some("capture-session-1".to_string()),
        frame_id: Some("frame-1".to_string()),
        note: Some("test evidence".to_string()),
        details: json!({
            "source": source,
            "evidence_id": evidence_id,
        }),
    }
}

fn gravity_sample(
    evidence_id: &str,
    x: f64,
    y: f64,
    z: f64,
    confidence: f64,
) -> ActivityGravitySample {
    ActivityGravitySample {
        gravity_x_g: x,
        gravity_y_g: y,
        gravity_z_g: z,
        confidence_0_to_1: confidence,
        provenance: provenance("gravity_sample", evidence_id),
    }
}

fn heart_rate_evidence(bpm: f64, confidence: f64, evidence_id: &str) -> ActivityHeartRateEvidence {
    ActivityHeartRateEvidence {
        heart_rate_bpm: bpm,
        confidence_0_to_1: confidence,
        provenance: provenance("heart_rate", evidence_id),
    }
}

fn motion_evidence(confidence: f64, evidence_id: &str) -> ActivityMotionEvidence {
    ActivityMotionEvidence {
        gravity_samples: vec![
            gravity_sample(&format!("{evidence_id}.g0"), 0.0, 0.0, -1.0, 0.98),
            gravity_sample(&format!("{evidence_id}.g1"), 0.01, -0.01, -0.9999, 0.97),
            gravity_sample(&format!("{evidence_id}.g2"), -0.02, 0.02, -0.9995, 0.96),
        ],
        confidence_0_to_1: confidence,
        provenance: provenance("motion", evidence_id),
    }
}

fn command_sync_evidence(
    synced: bool,
    confidence: f64,
    evidence_id: &str,
) -> ActivityCommandSyncEvidence {
    ActivityCommandSyncEvidence {
        synced,
        confidence_0_to_1: confidence,
        provenance: provenance("command_sync", evidence_id),
    }
}

fn window_input(
    window_id: &str,
    heart_rate: Option<ActivityHeartRateEvidence>,
    motion: Option<ActivityMotionEvidence>,
    command_sync: Option<ActivityCommandSyncEvidence>,
    approved_by_user: bool,
) -> ActivityFeatureWindowInput {
    ActivityFeatureWindowInput {
        window_id: window_id.to_string(),
        start_time: "2026-05-27T00:00:00Z".to_string(),
        end_time: "2026-05-27T00:20:00Z".to_string(),
        heart_rate,
        motion,
        command_sync,
        approved_by_user,
    }
}

fn classifier_input(windows: Vec<ActivityFeatureWindowInput>) -> ActivityCandidateClassifierInput {
    ActivityCandidateClassifierInput {
        schema: ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA.to_string(),
        options: ActivityCandidateClassifierOptions::default(),
        windows,
    }
}
