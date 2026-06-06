use goose_core::{
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    metric_features::{
        HeartRateFeatureOptions, HrvFeatureOptions, MetricWindowFeatureOptions,
        MotionFeatureOptions, RecoveryFeatureScoreOptions, RecoverySensorDiscoveryOptions,
        RecoverySensorWidgetDiscovery, RestingHeartRateFeatureOptions, SleepFeatureScoreOptions,
        SleepStageKind, StrainFeatureScoreOptions, StressFeatureScoreOptions,
        VitalEventFeatureOptions, run_heart_rate_feature_report_for_store,
        run_hrv_feature_report_for_store, run_metric_window_feature_report_for_store,
        run_motion_feature_report_for_store, run_recovery_feature_score_report_for_store,
        run_recovery_sensor_discovery_report_for_store,
        run_resting_heart_rate_feature_report_for_store, run_sleep_feature_score_report_for_store,
        run_strain_feature_score_report_for_store, run_stress_feature_score_report_for_store,
        run_vital_event_feature_report_for_store,
    },
    protocol::{
        DeviceType, PACKET_TYPE_EVENT, PACKET_TYPE_HISTORICAL_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
        build_v5_payload_frame,
    },
    store::GooseStore,
};

#[test]
fn motion_feature_extraction_normalizes_owned_k10_raw_amplitude() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame(&store, "user-owned-live-notification");

    let report = run_motion_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        MotionFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 1);
    let feature = &report.features[0];
    assert_eq!(feature.body_summary_kind, "raw_motion_k10");
    assert_eq!(feature.parsed_sample_count, 600);
    assert_eq!(feature.axis_count, 6);
    assert_eq!(feature.heart_rate_bpm, Some(72));
    assert!(feature.trusted_metric_input);
    assert_close(feature.raw_mean_abs, 1000.0);
    assert_close(feature.raw_peak_abs, 1000.0);
    assert_close(feature.motion_intensity_0_to_1, 1000.0 / 32767.0);
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "preliminary_raw_i16_scale")
    );
}

#[test]
fn motion_feature_extraction_keeps_synthetic_candidates_untrusted() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame(&store, "synthetic");

    let report = run_motion_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        MotionFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 0);
    assert!(!report.features[0].trusted_metric_input);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "capture_correlation_report_not_passed")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_motion_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_motion_features")
    );
}

#[test]
fn heart_rate_feature_extraction_promotes_owned_normal_history_marker() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame(&store, "user-owned-live-notification", 77);

    let report = run_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 1);
    let feature = &report.features[0];
    assert_eq!(feature.body_summary_kind, "normal_history");
    assert_eq!(feature.heart_rate_bpm, 77.0);
    assert_eq!(feature.marker_offset, 14);
    assert_eq!(feature.marker_value, 77);
    assert_eq!(feature.device_timestamp_seconds, Some(0x11223344));
    assert_eq!(feature.sample_time, "2026-05-27T13:00:00Z");
    assert_eq!(feature.sample_time_source, "captured_at");
    assert!(feature.trusted_metric_input);
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "preliminary_normal_history_hr_marker")
    );
}

#[test]
fn heart_rate_feature_extraction_promotes_owned_k10_live_heart_rate() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame(&store, "user-owned-live-notification");

    let report = run_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 1);
    let feature = &report.features[0];
    assert_eq!(feature.body_summary_kind, "raw_motion_k10");
    assert_eq!(feature.source_signal, "raw_motion_k10_heart_rate");
    assert_eq!(feature.heart_rate_bpm, 72.0);
    assert!(feature.trusted_metric_input);
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "preliminary_raw_motion_k10_heart_rate")
    );
}

#[test]
fn heart_rate_feature_extraction_does_not_promote_zero_marker() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame(&store, "user-owned-live-notification", 0);

    let report = run_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 0);
    assert_eq!(report.trusted_feature_count, 0);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_heart_rate_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_heart_rate_features")
    );
}

#[test]
fn heart_rate_feature_extraction_keeps_synthetic_candidates_untrusted() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame(&store, "synthetic", 77);

    let report = run_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 0);
    assert!(!report.features[0].trusted_metric_input);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "capture_correlation_report_not_passed")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_heart_rate_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_heart_rate_features")
    );
}

#[test]
fn vital_event_features_expose_owned_temperature_candidates_without_promoting_units() {
    let store = GooseStore::open_in_memory().unwrap();
    import_temperature_event(
        &store,
        "user-owned-live-notification",
        &[0xde, 0xad, 0xbe, 0xef],
    );

    let report = run_vital_event_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 1);
    assert_eq!(report.resolved_metric_input_count, 0);
    let feature = &report.features[0];
    assert_eq!(feature.event_id, 17);
    assert_eq!(feature.event_name, "TEMPERATURE_LEVEL");
    assert_eq!(feature.raw_body_hex, "deadbeef");
    assert_eq!(feature.raw_i16_le, Some(-21026));
    assert_eq!(feature.raw_u16_le, Some(44510));
    assert_eq!(feature.raw_i32_le, Some(-272716322));
    assert_eq!(feature.raw_u32_le, Some(4022250974));
    assert!(feature.trusted_candidate_evidence);
    assert!(!feature.resolved_metric_input);
    assert!(!feature.value_semantics_verified);
    assert_eq!(feature.semantic_status, "unresolved_units");
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "not_promoted_to_score_input")
    );
}

#[test]
fn vital_event_features_keep_synthetic_temperature_candidates_untrusted() {
    let store = GooseStore::open_in_memory().unwrap();
    import_temperature_event(&store, "synthetic", &[0xde, 0xad, 0xbe, 0xef]);

    let report = run_vital_event_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 0);
    assert!(!report.features[0].trusted_candidate_evidence);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "capture_correlation_report_not_passed")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_vital_event_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_vital_event_features")
    );
}

#[test]
fn vital_event_features_expose_history_skin_temperature_candidates_without_promoting() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_with_hex(
        &store,
        "user-owned-live-notification",
        77,
        "2026-05-27T13:00:00Z",
        historical_k18_frame_hex_with_skin_temperature(77, 3567),
    );

    let report = run_vital_event_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.decoded_frame_count, 1);
    assert_eq!(report.data_packet_frame_count, 1);
    assert_eq!(report.pulse_information_packet_count, 0);
    assert_eq!(report.candidate_frame_count, 0);
    assert_eq!(report.feature_count, 0);
    assert_eq!(report.skin_temperature_input_count, 1);
    let feature = &report.skin_temperature_inputs[0];
    assert_eq!(
        feature.schema_field,
        "normal_history_k18_body_24_skin_temperature_c"
    );
    assert_eq!(feature.raw_body_offset, 24);
    assert_eq!(feature.raw_absolute_offset, 37);
    assert_eq!(feature.raw_i16_le, Some(3567));
    assert_eq!(feature.raw_u16_le, Some(3567));
    assert_close(feature.skin_temperature_c.unwrap(), 35.67);
    assert_eq!(feature.semantic_status, "plausible_unverified_units");
    assert!(!feature.resolved_metric_input);
    assert!(!feature.value_semantics_verified);
}

#[test]
fn vital_event_features_expose_history_respiratory_rate_candidates_without_promoting() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_with_hex(
        &store,
        "user-owned-live-notification",
        77,
        "2026-05-27T13:00:00Z",
        historical_k18_frame_hex_with_vital_candidates(77, 3567, 145),
    );

    let report = run_vital_event_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.decoded_frame_count, 1);
    assert_eq!(report.data_packet_frame_count, 1);
    assert_eq!(report.respiratory_rate_input_count, 1);
    assert_eq!(report.trusted_respiratory_rate_input_count, 1);
    let feature = &report.respiratory_rate_inputs[0];
    assert_eq!(
        feature.schema_field,
        "normal_history_k18_body_26_respiratory_rate_rpm_candidate"
    );
    assert_eq!(feature.raw_body_offset, 26);
    assert_eq!(feature.raw_absolute_offset, 39);
    assert_eq!(feature.raw_u16_le, Some(145));
    assert_close(feature.respiratory_rate_rpm.unwrap(), 14.5);
    assert_eq!(feature.semantic_status, "plausible_unverified_units");
    assert!(feature.trusted_candidate_evidence);
    assert!(!feature.resolved_metric_input);
    assert!(!feature.value_semantics_verified);
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "not_promoted_to_score_input")
    );
}

#[test]
fn hrv_feature_extraction_builds_goose_hrv_score_from_trusted_r17_samples() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 810, 790, 800],
        "2026-05-27T04:00:00Z",
    );

    let report = run_hrv_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HrvFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.candidate_frame_count, 1);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 1);
    assert_eq!(report.rr_interval_count, 4);
    assert_eq!(report.trusted_rr_interval_count, 4);
    assert_eq!(
        report.hrv_input.as_ref().unwrap().rr_intervals_ms,
        vec![800.0, 810.0, 790.0, 800.0]
    );
    let score = report.score_result.unwrap();
    assert!(score.errors.is_empty(), "{:?}", score.errors);
    let output = score.output.unwrap();
    assert_close(output.rmssd_ms, 14.142135623730951);
    assert_close(output.sdnn_ms, 8.16496580927726);
}

#[test]
fn hrv_feature_extraction_filters_implausible_r17_samples_with_flags() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[100, 800, 2500, 810],
        "2026-05-27T04:00:00Z",
    );

    let report = run_hrv_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HrvFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.rr_interval_count, 2);
    let feature = &report.features[0];
    assert_eq!(feature.raw_sample_count, 4);
    assert_eq!(feature.plausible_sample_count, 2);
    assert_eq!(feature.rejected_sample_count, 2);
    assert_eq!(feature.rr_intervals_ms, vec![800.0, 810.0]);
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "rr_interval_samples_outside_plausible_range")
    );
}

#[test]
fn hrv_feature_extraction_keeps_synthetic_r17_samples_untrusted() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "synthetic",
        &[800, 810, 790, 800],
        "2026-05-27T04:00:00Z",
    );

    let report = run_hrv_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HrvFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.feature_count, 1);
    assert_eq!(report.trusted_feature_count, 0);
    assert_eq!(report.rr_interval_count, 4);
    assert_eq!(report.trusted_rr_interval_count, 0);
    assert!(report.hrv_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_hrv_features")
    );
}

#[test]
fn hrv_feature_extraction_computes_daily_rmssd_baseline_from_trusted_r17_samples() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 810, 790, 800],
        "2026-05-25T04:00:00Z",
    );
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[900, 920, 880, 900],
        "2026-05-26T04:00:00Z",
    );
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[700, 705, 695, 700],
        "2026-05-27T04:00:00Z",
    );

    let report = run_hrv_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HrvFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.feature_count, 3);
    assert_eq!(report.trusted_feature_count, 3);
    assert_eq!(report.daily_count, 3);
    assert_eq!(report.daily.len(), 3);
    assert_close(report.daily[0].rmssd_ms, 14.142135623730951);
    assert_close(report.daily[1].rmssd_ms, 28.284271247461902);
    assert_close(report.daily[2].rmssd_ms, 7.0710678118654755);
    let baseline = report.baseline.unwrap();
    assert_close(baseline.hrv_baseline_rmssd_ms, 14.142135623730951);
    assert_eq!(baseline.method, "median_daily_rmssd");
    assert_eq!(baseline.day_count, 3);
    assert!(baseline.trusted_metric_input);
    assert_eq!(baseline.input_ids.len(), 3);
}

#[test]
fn hrv_feature_extraction_can_require_baseline_days() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 810, 790, 800],
        "2026-05-27T04:00:00Z",
    );

    let report = run_hrv_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        HrvFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.score_result.is_some());
    assert_eq!(report.daily_count, 1);
    assert!(report.baseline.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "hrv_baseline_min_days_not_met")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "hrv_baseline_min_days_not_met")
    );
}

#[test]
fn recovery_sensor_discovery_keeps_unverified_health_widgets_unavailable() {
    let store = GooseStore::open_in_memory().unwrap();
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 810, 790, 800],
        "2026-05-27T04:00:00Z",
    );
    import_history_frame_with_hex(
        &store,
        "user-owned-live-notification",
        77,
        "2026-05-27T04:05:00Z",
        historical_k18_frame_hex_with_vital_candidates(77, 3567, 145),
    );
    import_temperature_event(
        &store,
        "user-owned-live-notification",
        &[0xde, 0xad, 0xbe, 0xef],
    );

    let report = run_recovery_sensor_discovery_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RecoverySensorDiscoveryOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            min_rr_intervals_to_compute: 2,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.schema, "goose.recovery-sensor-discovery-report.v1");
    assert_eq!(report.widgets.len(), 4);

    let hrv = widget(&report.widgets, "hrv_rmssd_ms");
    assert_eq!(hrv.candidate_count, 1);
    assert_eq!(hrv.trusted_candidate_count, 1);
    assert_eq!(hrv.source_kind, "unavailable");
    assert_eq!(hrv.confidence, 0.0);
    assert_eq!(hrv.promotion_status, "candidate_unverified");
    assert!(!hrv.user_visible_value_allowed);
    assert!(
        hrv.blocker_reasons
            .iter()
            .any(|reason| reason == "hrv_rr_interval_scale_unverified")
    );

    let respiratory = widget(&report.widgets, "respiratory_rate_rpm");
    assert_eq!(respiratory.candidate_count, 1);
    assert_eq!(respiratory.trusted_candidate_count, 1);
    assert_eq!(respiratory.source_kind, "unavailable");
    assert_eq!(respiratory.confidence, 0.0);
    assert!(
        respiratory
            .blocker_reasons
            .iter()
            .any(|reason| reason == "respiratory_rate_semantics_unverified")
    );

    let oxygen = widget(&report.widgets, "oxygen_saturation_percent");
    assert_eq!(oxygen.candidate_count, 0);
    assert_eq!(oxygen.confidence, 0.0);
    assert_eq!(oxygen.promotion_status, "unavailable");
    assert!(
        oxygen
            .blocker_reasons
            .iter()
            .any(|reason| reason == "oxygen_saturation_decoder_not_implemented")
    );

    let temperature = widget(&report.widgets, "skin_temperature_delta_c");
    assert_eq!(temperature.candidate_count, 2);
    assert_eq!(temperature.trusted_candidate_count, 2);
    assert_eq!(temperature.confidence, 0.0);
    assert!(
        temperature
            .blocker_reasons
            .iter()
            .any(|reason| reason == "temperature_units_unverified")
    );
    assert!(report.issues.iter().any(
        |issue| issue == "oxygen_saturation_percent:oxygen_saturation_decoder_not_implemented"
    ));
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.scope == "respiratory_rate_rpm"
                && action.reason == "respiratory_rate_semantics_unverified")
    );
}

#[test]
fn metric_window_features_aggregate_trusted_hr_and_motion_candidates() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        80,
        "2026-05-27T13:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        100,
        "2026-05-27T13:10:00Z",
    );
    import_motion_frame_at(
        &store,
        "user-owned-live-notification",
        "2026-05-27T13:05:00Z",
    );

    let report = run_metric_window_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T13:00:00Z",
        "2026-05-27T13:15:00Z",
        MetricWindowFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_hr_bpm: Some(60.0),
            max_hr_bpm: Some(180.0),
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.heart_rate_feature_count, 3);
    assert_eq!(report.trusted_heart_rate_feature_count, 3);
    assert_eq!(report.motion_feature_count, 1);
    assert_eq!(report.trusted_motion_feature_count, 1);
    let window = report.window.unwrap();
    assert_eq!(window.heart_rate_sample_count, 3);
    assert_eq!(window.motion_sample_count, 1);
    assert!(window.trusted_metric_input);
    assert_close(window.duration_minutes, 10.0);
    assert_close(window.average_hr_bpm, 84.0);
    assert_close(window.max_hr_bpm, 100.0);
    assert_close(
        window.average_motion_intensity_0_to_1.unwrap(),
        1000.0 / 32767.0,
    );
    assert_eq!(window.hr_zone_minutes.len(), 5);
    assert_close(window.hr_zone_minutes[0], 20.0 / 3.0);
    assert_close(window.hr_zone_minutes[1], 10.0 / 3.0);
    assert_close(window.hr_zone_minutes.iter().sum::<f64>(), 10.0);
    assert_eq!(
        window.provenance["heart_rate_source_signal"],
        "mixed_heart_rate_signals"
    );
}

#[test]
fn metric_window_features_require_trusted_hr_when_requested() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(&store, "synthetic", 80, "2026-05-27T13:00:00Z");
    import_history_frame_at(&store, "synthetic", 100, "2026-05-27T13:10:00Z");

    let report = run_metric_window_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T13:00:00Z",
        "2026-05-27T13:15:00Z",
        MetricWindowFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_hr_bpm: Some(60.0),
            max_hr_bpm: Some(180.0),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.heart_rate_feature_count, 2);
    assert_eq!(report.trusted_heart_rate_feature_count, 0);
    assert!(report.window.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_heart_rate_window_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_heart_rate_window_features")
    );
}

#[test]
fn resting_heart_rate_features_compute_window_and_baseline_from_trusted_markers() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-25T04:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        80,
        "2026-05-25T04:10:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        62,
        "2026-05-26T04:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        90,
        "2026-05-26T04:10:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        58,
        "2026-05-27T04:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        100,
        "2026-05-27T04:10:00Z",
    );

    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            baseline_min_days: 3,
            require_baseline: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.heart_rate_feature_count, 6);
    assert_eq!(report.trusted_heart_rate_feature_count, 6);
    assert_eq!(report.daily_count, 3);
    let resting = report.resting.unwrap();
    assert_close(resting.resting_hr_bpm, 59.0);
    assert_eq!(resting.sample_count, 6);
    assert!(resting.trusted_metric_input);
    let baseline = report.baseline.unwrap();
    assert_close(baseline.resting_hr_baseline_bpm, 60.0);
    assert_eq!(baseline.day_count, 3);
    assert!(baseline.trusted_metric_input);
}

#[test]
fn resting_heart_rate_features_can_require_baseline_days() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-27T04:00:00Z",
    );

    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            baseline_min_days: 3,
            require_baseline: true,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.resting.is_some());
    assert!(report.baseline.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "resting_hr_baseline_min_days_not_met")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "resting_hr_baseline_min_days_not_met")
    );
}

#[test]
fn resting_heart_rate_features_keep_synthetic_candidates_untrusted() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(&store, "synthetic", 60, "2026-05-27T04:00:00Z");

    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            baseline_min_days: 1,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.resting.is_none());
    assert_eq!(report.heart_rate_feature_count, 1);
    assert_eq!(report.trusted_heart_rate_feature_count, 0);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no_trusted_resting_heart_rate_features")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "no_trusted_resting_heart_rate_features")
    );
}

#[test]
fn resting_heart_rate_features_compute_from_owned_k10_live_heart_rate() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame_at_value(
        &store,
        "user-owned-live-notification",
        "2026-05-27T04:00:00Z",
        900,
    );
    import_motion_frame_at_value(
        &store,
        "user-owned-live-notification",
        "2026-05-27T04:10:00Z",
        1100,
    );

    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            baseline_min_days: 1,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.heart_rate_feature_count, 2);
    assert_eq!(report.trusted_heart_rate_feature_count, 2);
    let resting = report.resting.unwrap();
    assert_close(resting.resting_hr_bpm, 72.0);
    assert_eq!(
        resting.method,
        "low_motion_filtered_lowest_quartile_mean_heart_rate_features"
    );
    assert_eq!(resting.sample_count, 2);
    assert!(resting.trusted_metric_input);
    assert!(
        resting
            .quality_flags
            .iter()
            .any(|flag| flag == "preliminary_resting_hr_from_heart_rate_features")
    );
    assert!(
        resting
            .quality_flags
            .iter()
            .any(|flag| flag == "resting_hr_low_motion_filter_applied")
    );
    assert_eq!(
        resting.provenance["source_signal"],
        "raw_motion_k10_heart_rate"
    );
    assert_eq!(
        resting.provenance["source_signals"][0],
        "raw_motion_k10_heart_rate"
    );
}

#[test]
fn resting_heart_rate_features_exclude_high_motion_heart_rate_samples() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame_at_value_and_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T04:00:00Z",
        900,
        65,
    );
    import_motion_frame_at_value_and_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T04:05:00Z",
        12_000,
        45,
    );
    import_motion_frame_at_value_and_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T04:10:00Z",
        1_000,
        66,
    );

    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            baseline_min_days: 1,
            require_baseline: false,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.heart_rate_feature_count, 3);
    assert_eq!(report.trusted_heart_rate_feature_count, 3);
    let resting = report.resting.unwrap();
    assert_close(resting.resting_hr_bpm, 65.0);
    assert_eq!(
        resting.method,
        "low_motion_filtered_lowest_quartile_mean_heart_rate_features"
    );
    assert_eq!(resting.sample_count, 2);
    assert!(resting.trusted_metric_input);
    assert!(
        resting
            .quality_flags
            .contains(&"resting_hr_low_motion_filter_applied".to_string())
    );
    assert!(
        resting
            .quality_flags
            .contains(&"resting_hr_high_motion_samples_excluded".to_string())
    );
    assert_eq!(
        resting.provenance["motion_filter"]["motion_sample_count"],
        3
    );
    assert_eq!(
        resting.provenance["motion_filter"]["selected_heart_rate_sample_count"],
        2
    );
    assert_eq!(
        resting.provenance["motion_filter"]["low_motion_heart_rate_sample_count"],
        2
    );
    assert_eq!(
        resting.provenance["motion_filter"]["high_motion_heart_rate_sample_count"],
        1
    );
}

#[test]
fn sleep_feature_score_report_builds_local_sleep_from_trusted_motion_features() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T22:00:00Z",
        1000,
    );
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T23:00:00Z",
        1000,
    );
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-28T00:00:00Z",
        10000,
    );
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-28T01:00:00Z",
        1000,
    );
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-28T02:00:00Z",
        1000,
    );

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T04:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty(), "{:?}", report.next_actions);
    assert_eq!(report.motion_report.trusted_feature_count, 5);
    let window = report.sleep_window.unwrap();
    assert_close(window.time_in_bed_minutes, 240.0);
    assert_close(window.sleep_duration_minutes, 180.0);
    assert_close(window.motion_coverage_fraction, 1.0);
    assert_close(window.heart_rate_coverage_fraction, 0.0);
    assert_close(window.midpoint_deviation_minutes, 0.0);
    assert_eq!(window.disturbance_count, 1);
    assert!(window.trusted_metric_input);
    let input = report.sleep_input.unwrap();
    assert_close(input.sleep_need_minutes, 240.0);
    let result = report.score_result.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let output = result.output.unwrap();
    assert_close(output.efficiency_fraction, 0.75);
    assert_close(output.sleep_debt_minutes, 60.0);
    assert_close(output.score_0_to_100, 80.75);
}

#[test]
fn motion_feature_extraction_normalizes_historical_k21_sample_time() {
    let store = GooseStore::open_in_memory().unwrap();
    import_historical_k21_motion_frame_at_with_device_timestamp(
        &store,
        "user-owned-live-notification",
        "2026-01-01T20:00:00Z",
        1_767_304_800,
    );

    let report = run_motion_feature_report_for_store(
        &store,
        "test-db",
        "2026-01-01T19:00:00Z",
        "2026-01-01T21:00:00Z",
        MotionFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.feature_count, 1);
    let feature = &report.features[0];
    assert_eq!(feature.body_summary_kind, "raw_motion_k21");
    assert_eq!(feature.captured_at, "2026-01-01T20:00:00Z");
    assert_eq!(feature.sample_time, "2026-01-01T22:00:00Z");
    assert_eq!(feature.sample_time_source, "device_timestamp");
    assert_eq!(feature.sample_time_unix_ms, Some(1_767_304_800_000));
    assert_eq!(feature.device_timestamp_seconds, Some(1_767_304_800));
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "sample_time_from_device_timestamp")
    );
}

#[test]
fn motion_feature_extraction_rejects_invalid_device_timestamp_subseconds() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame_at_value_with_device_timestamp_subseconds(
        &store,
        "user-owned-live-notification",
        "2026-01-01T20:00:00Z",
        1000,
        1_767_304_800,
        1_500,
    );

    let report = run_motion_feature_report_for_store(
        &store,
        "test-db",
        "2026-01-01T19:00:00Z",
        "2026-01-01T21:00:00Z",
        MotionFeatureOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.feature_count, 1);
    let feature = &report.features[0];
    assert_eq!(feature.sample_time, "2026-01-01T20:00:00Z");
    assert_eq!(feature.sample_time_source, "captured_at");
    assert_eq!(feature.sample_time_unix_ms, Some(1_767_297_600_000));
    assert_eq!(feature.device_timestamp_seconds, Some(1_767_304_800));
    assert_eq!(feature.device_timestamp_subseconds, Some(1_500));
    assert!(
        feature
            .quality_flags
            .iter()
            .any(|flag| flag == "device_timestamp_subseconds_out_of_range")
    );
    assert!(
        !feature
            .quality_flags
            .iter()
            .any(|flag| flag == "sample_time_from_device_timestamp")
    );
}

#[test]
fn sleep_feature_score_report_uses_device_sample_time_for_sleep_epochs() {
    let store = GooseStore::open_in_memory().unwrap();
    for (timestamp_seconds, sample_value) in [
        (1_767_304_800, 10000),
        (1_767_308_400, 1000),
        (1_767_312_000, 1000),
        (1_767_315_600, 10000),
        (1_767_319_200, 1000),
    ] {
        import_motion_frame_at_value_with_device_timestamp(
            &store,
            "user-owned-live-notification",
            "2026-01-01T20:00:00Z",
            sample_value,
            timestamp_seconds,
        );
    }
    for (timestamp_seconds, marker_value) in [
        (1_767_305_700, 80),
        (1_767_309_300, 65),
        (1_767_312_900, 55),
        (1_767_316_500, 75),
    ] {
        import_history_frame_at_with_device_timestamp(
            &store,
            "user-owned-live-notification",
            marker_value,
            "2026-01-01T20:00:00Z",
            timestamp_seconds,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-01-01T19:00:00Z",
        "2026-01-01T21:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 30.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(
        report
            .motion_report
            .features
            .iter()
            .all(|feature| feature.sample_time_source == "device_timestamp")
    );
    assert!(
        report
            .heart_rate_report
            .features
            .iter()
            .all(|feature| feature.sample_time_source == "device_timestamp")
    );
    let window = report.sleep_window.unwrap();
    assert_eq!(window.start_time, "2026-01-01T22:00:00Z");
    assert_eq!(window.end_time, "2026-01-02T02:00:00Z");
    assert_close(window.time_in_bed_minutes, 240.0);
    assert_eq!(window.stage_segments[0].start_time, "2026-01-01T22:00:00Z");
    assert_eq!(window.stage_segments[0].end_time, "2026-01-01T23:00:00Z");
    assert_eq!(window.provenance["time_basis"], "normalized_sample_time");
}

#[test]
fn sleep_feature_score_report_derives_wake_stages_and_heart_rate_dip() {
    let store = GooseStore::open_in_memory().unwrap();
    for (captured_at, sample_value) in [
        ("2026-05-27T22:00:00Z", 10000),
        ("2026-05-27T23:00:00Z", 1000),
        ("2026-05-28T00:00:00Z", 1000),
        ("2026-05-28T01:00:00Z", 10000),
        ("2026-05-28T02:00:00Z", 1000),
        ("2026-05-28T03:00:00Z", 1000),
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            sample_value,
        );
    }
    for (captured_at, marker_value) in [
        ("2026-05-27T22:15:00Z", 80),
        ("2026-05-27T23:15:00Z", 65),
        ("2026-05-28T00:15:00Z", 55),
        ("2026-05-28T01:15:00Z", 75),
        ("2026-05-28T02:15:00Z", 70),
    ] {
        import_history_frame_at(
            &store,
            "user-owned-live-notification",
            marker_value,
            captured_at,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T04:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 30.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.heart_rate_report.trusted_feature_count, 5);
    let window = report.sleep_window.unwrap();
    assert_close(window.time_in_bed_minutes, 300.0);
    assert_close(window.sleep_duration_minutes, 180.0);
    assert_close(window.motion_coverage_fraction, 1.0);
    assert_close(window.heart_rate_coverage_fraction, 1.0);
    assert_close(window.sleep_latency_minutes, 60.0);
    assert_close(window.wake_after_sleep_onset_minutes, 60.0);
    assert_eq!(window.wake_episode_count, 1);
    assert_eq!(window.disturbance_count, 2);
    assert_eq!(window.stage_segments.len(), 5);
    assert_close(*window.stage_minutes.get("awake").unwrap(), 120.0);
    assert!(
        window
            .stage_segments
            .iter()
            .any(|segment| segment.stage == SleepStageKind::Deep)
    );
    assert!(
        window
            .stage_segments
            .iter()
            .any(|segment| segment.stage == SleepStageKind::Rem)
    );
    for segment in &window.stage_segments {
        assert_eq!(segment.stage_probabilities.len(), 4);
        assert_close(segment.stage_probabilities.values().sum::<f64>(), 1.0);
        assert_close(
            *segment
                .stage_probabilities
                .get(match segment.stage {
                    SleepStageKind::Awake => "awake",
                    SleepStageKind::Core => "core",
                    SleepStageKind::Deep => "deep",
                    SleepStageKind::Rem => "rem",
                })
                .unwrap(),
            segment.confidence_0_to_1,
        );
    }
    assert_close(window.lowest_sleep_hr_bpm.unwrap(), 55.0);
    assert_close(
        window.sleep_hr_trend_bpm_per_hour.unwrap(),
        1.6666666666666667,
    );
    assert_close(window.baseline_awake_hr_bpm.unwrap(), 80.0);
    assert_close(window.heart_rate_dip_percent.unwrap(), 31.25);
    assert!(
        !window
            .quality_flags
            .contains(&"heart_rate_dip_uses_highest_quartile_fallback".to_string())
    );
}

#[test]
fn sleep_feature_score_report_falls_back_to_highest_quartile_hr_dip_baseline() {
    let store = GooseStore::open_in_memory().unwrap();
    for captured_at in [
        "2026-05-27T22:00:00Z",
        "2026-05-27T23:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-28T01:00:00Z",
        "2026-05-28T02:00:00Z",
        "2026-05-28T03:00:00Z",
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            1000,
        );
    }
    for (captured_at, marker_value) in [
        ("2026-05-27T22:15:00Z", 80),
        ("2026-05-27T23:15:00Z", 70),
        ("2026-05-28T00:15:00Z", 60),
        ("2026-05-28T01:15:00Z", 50),
        ("2026-05-28T02:15:00Z", 55),
    ] {
        import_history_frame_at(
            &store,
            "user-owned-live-notification",
            marker_value,
            captured_at,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T04:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 30.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let window = report.sleep_window.unwrap();
    assert_eq!(
        window.stage_minutes.get("awake").copied().unwrap_or(0.0),
        0.0
    );
    assert_close(window.baseline_awake_hr_bpm.unwrap(), 75.0);
    assert_close(window.lowest_sleep_hr_bpm.unwrap(), 50.0);
    assert_close(window.heart_rate_dip_percent.unwrap(), 33.33333333333333);
    assert!(
        window
            .quality_flags
            .contains(&"heart_rate_dip_uses_highest_quartile_fallback".to_string())
    );
}

#[test]
fn sleep_feature_score_report_merges_adjacent_compatible_stage_segments() {
    let store = GooseStore::open_in_memory().unwrap();
    for captured_at in [
        "2026-05-27T22:00:00Z",
        "2026-05-27T23:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-28T01:00:00Z",
        "2026-05-28T09:00:00Z",
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            1000,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T09:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 480.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 120.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let window = report.sleep_window.unwrap();
    assert!(
        window.stage_segments.len() < 4,
        "expected fewer merged segments than raw intervals, got {:?}",
        window.stage_segments
    );
    assert_eq!(window.stage_segments[0].stage, SleepStageKind::Deep);
    assert_eq!(window.stage_segments[0].start_time, "2026-05-27T22:00:00Z");
    assert!(window.stage_segments[0].duration_minutes > 60.0);
    assert_close(
        window.motion_coverage_fraction,
        180.0 / window.time_in_bed_minutes,
    );
    assert_close(window.heart_rate_coverage_fraction, 0.0);
    assert_close(
        window
            .stage_segments
            .iter()
            .map(|segment| segment.duration_minutes)
            .sum::<f64>(),
        window.time_in_bed_minutes,
    );
    assert!(
        window
            .quality_flags
            .contains(&"adjacent_compatible_stage_segments_merged".to_string())
    );
    assert!(
        window
            .quality_flags
            .contains(&"sleep_heart_rate_coverage_low".to_string())
    );
    assert_eq!(
        window.provenance["coverage"]["motion_duplicate_timestamp_count"],
        0
    );
}

#[test]
fn sleep_feature_score_report_smooths_short_non_wake_stage_islands() {
    let store = GooseStore::open_in_memory().unwrap();
    for captured_at in [
        "2026-05-27T22:00:00Z",
        "2026-05-27T22:30:00Z",
        "2026-05-27T22:34:00Z",
        "2026-05-27T23:05:00Z",
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            1000,
        );
    }
    for (captured_at, marker_value) in [
        ("2026-05-27T22:05:00Z", 55),
        ("2026-05-27T22:31:00Z", 65),
        ("2026-05-27T22:35:00Z", 55),
    ] {
        import_history_frame_at(
            &store,
            "user-owned-live-notification",
            marker_value,
            captured_at,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-27T23:10:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 65.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 1350.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let window = report.sleep_window.unwrap();
    assert_eq!(
        window.stage_model_version,
        "goose_sleep_stage_heuristic_v1_transition_smoothed"
    );
    assert_eq!(
        window.stage_segments.len(),
        1,
        "{:?}",
        window.stage_segments
    );
    assert_eq!(window.stage_segments[0].stage, SleepStageKind::Deep);
    assert_eq!(window.stage_segments[0].start_time, "2026-05-27T22:00:00Z");
    assert_eq!(window.stage_segments[0].end_time, "2026-05-27T23:05:00Z");
    assert_close(window.stage_segments[0].duration_minutes, 65.0);
    assert!(
        window.stage_segments[0]
            .quality_flags
            .contains(&"short_stage_transition_smoothed".to_string())
    );
    assert!(
        window
            .quality_flags
            .contains(&"short_stage_transition_smoothed".to_string())
    );
    assert_eq!(
        window.provenance["stage_smoothing_policy"],
        "merge_short_non_awake_stage_islands_between_matching_non_awake_neighbors"
    );
    assert_eq!(
        window.provenance["minimum_smoothed_stage_duration_minutes"],
        5.0
    );
}

#[test]
fn sleep_feature_score_report_preserves_short_awake_stage_islands() {
    let store = GooseStore::open_in_memory().unwrap();
    for (captured_at, sample_value) in [
        ("2026-05-27T22:00:00Z", 1000),
        ("2026-05-27T22:30:00Z", 10000),
        ("2026-05-27T22:34:00Z", 1000),
        ("2026-05-27T23:05:00Z", 1000),
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            sample_value,
        );
    }
    for (captured_at, marker_value) in [
        ("2026-05-27T22:05:00Z", 55),
        ("2026-05-27T22:31:00Z", 70),
        ("2026-05-27T22:35:00Z", 55),
    ] {
        import_history_frame_at(
            &store,
            "user-owned-live-notification",
            marker_value,
            captured_at,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-27T23:10:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 65.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 1350.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let window = report.sleep_window.unwrap();
    assert_eq!(
        window.stage_segments.len(),
        3,
        "{:?}",
        window.stage_segments
    );
    assert_eq!(window.stage_segments[1].stage, SleepStageKind::Awake);
    assert_close(window.stage_segments[1].duration_minutes, 4.0);
    assert_close(window.wake_after_sleep_onset_minutes, 4.0);
    assert_eq!(window.wake_episode_count, 1);
    assert!(
        !window
            .quality_flags
            .contains(&"short_stage_transition_smoothed".to_string())
    );
}

#[test]
fn sleep_feature_score_report_reports_duplicate_and_gap_coverage() {
    let store = GooseStore::open_in_memory().unwrap();
    for (captured_at, sample_value) in [
        ("2026-05-27T22:00:00Z", 1000),
        ("2026-05-27T23:00:00Z", 1000),
        ("2026-05-27T23:00:00Z", 2000),
        ("2026-05-28T01:00:00Z", 1000),
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            sample_value,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T02:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let window = report.sleep_window.unwrap();
    assert!(
        window
            .quality_flags
            .contains(&"duplicate_motion_timestamps".to_string())
    );
    assert!(
        window
            .quality_flags
            .contains(&"large_motion_feature_gap".to_string())
    );
    assert_eq!(
        window.provenance["coverage"]["motion_duplicate_timestamp_count"],
        1
    );
    assert_eq!(
        window.provenance["coverage"]["non_increasing_motion_interval_count"],
        1
    );
    assert_eq!(window.provenance["coverage"]["large_motion_gap_count"], 1);
    assert_eq!(
        window.provenance["coverage"]["largest_motion_gap_minutes"],
        120
    );
}

#[test]
fn sleep_feature_score_report_drops_nonexistent_calendar_timestamps() {
    let store = GooseStore::open_in_memory().unwrap();
    for captured_at in [
        "2026-02-28T22:00:00Z",
        "2026-02-30T23:00:00Z",
        "2026-03-01T00:00:00Z",
    ] {
        import_motion_frame_at_value_without_heart_rate(
            &store,
            "user-owned-live-notification",
            captured_at,
            1000,
        );
    }

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-02-28T21:00:00Z",
        "2026-03-02T00:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
        },
    )
    .unwrap();

    let window = report.sleep_window.unwrap();
    assert!(
        window
            .quality_flags
            .contains(&"unparseable_motion_timestamps_dropped".to_string())
    );
    assert_eq!(window.motion_feature_count, 2);
}

#[test]
fn sleep_feature_score_report_requires_enough_trusted_motion_features() {
    let store = GooseStore::open_in_memory().unwrap();
    import_motion_frame_at_value_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T22:00:00Z",
        1000,
    );

    let report = run_sleep_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.sleep_window.is_none());
    assert!(report.sleep_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "sleep_window_missing")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "sleep_window_missing")
    );
}

#[test]
fn recovery_feature_score_report_builds_local_recovery_from_trusted_feature_reports_and_packet_vitals()
 {
    let store = GooseStore::open_in_memory().unwrap();
    import_recovery_feature_inputs(&store);

    let report = run_recovery_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-28T06:00:00Z",
        "2026-05-28T06:05:00Z",
        "2026-05-28T04:00:00Z",
        "2026-05-28T05:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-29T00:00:00Z",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        RecoveryFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
            prior_strain_resting_baseline_min_days: 1,
            prior_strain_max_hr_bpm: None,
            respiratory_rate_rpm: Some(14.0),
            respiratory_rate_baseline_rpm: Some(14.0),
            skin_temp_delta_c: Some(0.0),
            provided_vitals_source: Some("metrics.recovery_sensor_discovery".to_string()),
            provided_vitals_provenance_json: Some(
                r#"{"source_kind":"device_sensor","decoder":"goose_packet_decoder","packet_family":"vital_event","source":"metric_features_test"}"#.to_string(),
            ),
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty(), "{:?}", report.next_actions);
    let provided = report.provided_vitals.as_ref().unwrap();
    assert_eq!(provided.source, "metrics.recovery_sensor_discovery");
    assert!(provided.trusted_metric_input);
    assert_eq!(
        provided.provenance["provided_vitals_provenance"]["source_kind"],
        "device_sensor"
    );
    let input = report.recovery_input.unwrap();
    assert_close(input.hrv_rmssd_ms, 25.0);
    assert_close(input.hrv_baseline_rmssd_ms, 50.0);
    assert_close(input.resting_hr_bpm, 54.5);
    assert_close(input.resting_hr_baseline_bpm, 55.0);
    assert_close(input.respiratory_rate_rpm, 14.0);
    assert_close(input.respiratory_rate_baseline_rpm, 14.0);
    assert_close(input.skin_temp_delta_c, 0.0);
    assert_close(input.sleep_score_0_to_100, 80.75);
    assert_close(input.prior_strain_0_to_21, 5.25);
    let result = report.score_result.unwrap();
    assert_eq!(
        result.provenance["provided_vitals"]["source"],
        "metrics.recovery_sensor_discovery"
    );
    assert_eq!(
        result.provenance["provided_vitals"]["provenance"]["provided_vitals_provenance"]["source_kind"],
        "device_sensor"
    );
    assert!(
        !result
            .quality_flags
            .iter()
            .any(|flag| flag == "provided_resp_temp_inputs_not_packet_derived")
    );
    let output = result.output.unwrap();
    assert_close(output.score_0_to_100, 62.1125);
}

#[test]
fn recovery_feature_score_report_blocks_manual_vitals_even_with_provenance() {
    let store = GooseStore::open_in_memory().unwrap();
    import_recovery_feature_inputs(&store);

    let report = run_recovery_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-28T06:00:00Z",
        "2026-05-28T06:05:00Z",
        "2026-05-28T04:00:00Z",
        "2026-05-28T05:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-29T00:00:00Z",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        RecoveryFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
            prior_strain_resting_baseline_min_days: 1,
            prior_strain_max_hr_bpm: None,
            respiratory_rate_rpm: Some(14.0),
            respiratory_rate_baseline_rpm: Some(14.0),
            skin_temp_delta_c: Some(0.0),
            provided_vitals_source: Some("manual_test_entry".to_string()),
            provided_vitals_provenance_json: Some(
                r#"{"owner":"user","entry_method":"manual_test","source":"metric_features_test"}"#
                    .to_string(),
            ),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.recovery_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "provided_resp_temp_inputs_not_packet_derived")
    );
    let provided = report.provided_vitals.as_ref().unwrap();
    assert!(!provided.trusted_metric_input);
    assert!(
        provided
            .quality_flags
            .iter()
            .any(|flag| flag == "provided_resp_temp_inputs_not_packet_derived")
    );
}

#[test]
fn recovery_feature_score_report_requires_provided_resp_temp_until_score_promotion_is_verified() {
    let store = GooseStore::open_in_memory().unwrap();
    import_recovery_feature_inputs(&store);

    let report = run_recovery_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-28T06:00:00Z",
        "2026-05-28T06:05:00Z",
        "2026-05-28T04:00:00Z",
        "2026-05-28T05:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-29T00:00:00Z",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        RecoveryFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
            prior_strain_resting_baseline_min_days: 1,
            prior_strain_max_hr_bpm: None,
            respiratory_rate_rpm: None,
            respiratory_rate_baseline_rpm: Some(14.0),
            skin_temp_delta_c: Some(0.0),
            provided_vitals_source: Some("manual_test_entry".to_string()),
            provided_vitals_provenance_json: Some(
                r#"{"owner":"user","entry_method":"manual_test","source":"metric_features_test"}"#
                    .to_string(),
            ),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "provided_resp_temp_inputs_missing")
    );
    assert!(report.provided_vitals.is_none());
    assert!(report.recovery_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "provided_resp_temp_inputs_missing")
    );
}

#[test]
fn recovery_feature_score_report_blocks_unproven_manual_vitals() {
    let store = GooseStore::open_in_memory().unwrap();
    import_recovery_feature_inputs(&store);

    let report = run_recovery_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-28T06:00:00Z",
        "2026-05-28T06:05:00Z",
        "2026-05-28T04:00:00Z",
        "2026-05-28T05:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-28T00:00:00Z",
        "2026-05-25T00:00:00Z",
        "2026-05-29T00:00:00Z",
        "2026-05-27T22:00:00Z",
        "2026-05-28T03:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        RecoveryFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
            sleep_need_minutes: 240.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 0.0,
            prior_strain_resting_baseline_min_days: 1,
            prior_strain_max_hr_bpm: None,
            respiratory_rate_rpm: Some(14.0),
            respiratory_rate_baseline_rpm: Some(14.0),
            skin_temp_delta_c: Some(0.0),
            provided_vitals_source: Some("manual_test_entry".to_string()),
            provided_vitals_provenance_json: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.recovery_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "provided_resp_temp_provenance_untrusted")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "provided_resp_temp_inputs_not_packet_derived")
    );
    let provided = report.provided_vitals.as_ref().unwrap();
    assert!(!provided.trusted_metric_input);
    assert!(
        provided
            .quality_flags
            .iter()
            .any(|flag| flag == "provided_resp_temp_provenance_untrusted")
    );
}

#[test]
fn strain_feature_score_report_builds_local_strain_from_trusted_features() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-27T12:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        80,
        "2026-05-27T12:10:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        100,
        "2026-05-27T12:20:00Z",
    );

    let report = run_strain_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        StrainFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 1,
            max_hr_bpm: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty(), "{:?}", report.next_actions);
    assert_eq!(
        report.max_hr_basis.as_deref(),
        Some("observed_window_max_hr_bpm")
    );
    let input = report.strain_input.unwrap();
    assert_close(input.duration_minutes, 20.0);
    assert_close(input.resting_hr_bpm, 60.0);
    assert_close(input.average_hr_bpm, 80.0);
    assert_close(input.max_hr_bpm, 100.0);
    assert_eq!(input.hr_zone_minutes.len(), 5);
    assert_close(input.hr_zone_minutes.iter().sum::<f64>(), 20.0);
    let result = report.score_result.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(
        result
            .quality_flags
            .iter()
            .any(|flag| flag == "observed_window_max_hr_basis")
    );
    let output = result.output.unwrap();
    assert_close(output.zone_load, 60.0);
    assert_close(output.average_hr_reserve_fraction, 0.5);
    assert_close(output.score_0_to_21, 5.25);
}

#[test]
fn strain_feature_score_report_requires_trusted_resting_hr_when_requested() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(&store, "synthetic", 60, "2026-05-27T12:00:00Z");
    import_history_frame_at(&store, "synthetic", 100, "2026-05-27T12:20:00Z");

    let report = run_strain_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        StrainFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 1,
            max_hr_bpm: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.strain_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "resting_heart_rate_report_not_passed")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "resting_hr_missing")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "resting_hr_missing")
    );
}

#[test]
fn strain_feature_score_report_rejects_max_hr_below_resting_hr() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-27T12:00:00Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        80,
        "2026-05-27T12:10:00Z",
    );

    let report = run_strain_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:30:00Z",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        StrainFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 1,
            max_hr_bpm: Some(55.0),
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.max_hr_basis.as_deref(), Some("provided_max_hr_bpm"));
    assert!(report.strain_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "max_hr_basis_must_exceed_resting_hr")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "max_hr_basis_must_exceed_resting_hr")
    );
}

#[test]
fn stress_feature_score_report_builds_local_stress_from_trusted_features() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        90,
        "2026-05-27T12:00:00Z",
    );
    import_motion_frame_at_without_heart_rate(
        &store,
        "user-owned-live-notification",
        "2026-05-27T12:00:10Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-27T04:00:00Z",
    );
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 825, 800],
        "2026-05-27T12:01:00Z",
    );
    for captured_at in [
        "2026-05-24T04:00:00Z",
        "2026-05-25T04:00:00Z",
        "2026-05-26T04:00:00Z",
    ] {
        import_r17_frame_at(
            &store,
            "user-owned-live-notification",
            &[800, 850, 800],
            captured_at,
        );
    }

    let report = run_stress_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:05:00Z",
        "2026-05-27T00:00:00Z",
        "2026-05-27T06:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:05:00Z",
        "2026-05-24T00:00:00Z",
        "2026-05-27T00:00:00Z",
        StressFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 1,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty(), "{:?}", report.next_actions);
    assert_eq!(report.heart_rate_report.trusted_feature_count, 1);
    assert_eq!(report.motion_report.trusted_feature_count, 1);
    assert_eq!(report.hrv_baseline_report.daily_count, 3);
    let input = report.stress_input.unwrap();
    assert_close(input.heart_rate_bpm, 90.0);
    assert_close(input.resting_hr_bpm, 60.0);
    assert_close(input.hrv_rmssd_ms, 25.0);
    assert_close(input.hrv_baseline_rmssd_ms, 50.0);
    assert_close(input.motion_intensity_0_to_1, 1000.0 / 32767.0);
    assert!(input.input_ids.len() >= 7);
    let result = report.score_result.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let output = result.output.unwrap();
    let expected_motion_adjusted_hr = 50.0 * (1.0 - (1000.0 / 32767.0) * 0.50);
    let expected_score = expected_motion_adjusted_hr * 0.60 + 50.0 * 0.40;
    assert_close(output.heart_rate_elevation_score, 50.0);
    assert_close(output.hrv_suppression_score, 50.0);
    assert_close(output.motion_adjusted_hr_score, expected_motion_adjusted_hr);
    assert_close(output.score_0_to_100, expected_score);
}

#[test]
fn stress_feature_score_report_requires_trusted_hrv_baseline() {
    let store = GooseStore::open_in_memory().unwrap();
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        90,
        "2026-05-27T12:00:00Z",
    );
    import_motion_frame_at(
        &store,
        "user-owned-live-notification",
        "2026-05-27T12:00:10Z",
    );
    import_history_frame_at(
        &store,
        "user-owned-live-notification",
        60,
        "2026-05-27T04:00:00Z",
    );
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 825, 800],
        "2026-05-27T12:01:00Z",
    );
    import_r17_frame_at(
        &store,
        "user-owned-live-notification",
        &[800, 850, 800],
        "2026-05-26T04:00:00Z",
    );

    let report = run_stress_feature_score_report_for_store(
        &store,
        "test-db",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:05:00Z",
        "2026-05-27T00:00:00Z",
        "2026-05-27T06:00:00Z",
        "2026-05-27T12:00:00Z",
        "2026-05-27T12:05:00Z",
        "2026-05-26T00:00:00Z",
        "2026-05-27T00:00:00Z",
        StressFeatureScoreOptions {
            min_owned_captures_per_summary: 1,
            require_trusted_evidence: true,
            resting_baseline_min_days: 1,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(report.stress_input.is_none());
    assert!(report.score_result.is_none());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "hrv_baseline_report_not_passed")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "hrv_baseline_missing")
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "hrv_baseline_missing")
    );
}

fn import_motion_frame(store: &GooseStore, sensitivity: &str) {
    import_motion_frame_at(store, sensitivity, "2026-05-27T12:00:00Z");
}

fn import_recovery_feature_inputs(store: &GooseStore) {
    for (captured_at, marker) in [
        ("2026-05-25T04:00:00Z", 56),
        ("2026-05-26T04:00:00Z", 55),
        ("2026-05-27T04:00:00Z", 54),
        ("2026-05-28T04:10:00Z", 55),
        ("2026-05-27T12:00:00Z", 60),
        ("2026-05-27T12:10:00Z", 80),
        ("2026-05-27T12:20:00Z", 100),
    ] {
        import_history_frame_at(store, "user-owned-live-notification", marker, captured_at);
    }

    import_r17_frame_at(
        store,
        "user-owned-live-notification",
        &[800, 825, 800],
        "2026-05-28T04:00:00Z",
    );
    for captured_at in [
        "2026-05-25T04:00:00Z",
        "2026-05-26T04:00:00Z",
        "2026-05-27T04:00:00Z",
    ] {
        import_r17_frame_at(
            store,
            "user-owned-live-notification",
            &[800, 850, 800],
            captured_at,
        );
    }

    for (captured_at, sample_value) in [
        ("2026-05-27T22:00:00Z", 1000),
        ("2026-05-27T23:00:00Z", 1000),
        ("2026-05-28T00:00:00Z", 10000),
        ("2026-05-28T01:00:00Z", 1000),
        ("2026-05-28T02:00:00Z", 1000),
    ] {
        import_motion_frame_at_value_without_heart_rate(
            store,
            "user-owned-live-notification",
            captured_at,
            sample_value,
        );
    }
}

fn import_motion_frame_at(store: &GooseStore, sensitivity: &str, captured_at: &str) {
    import_motion_frame_at_value(store, sensitivity, captured_at, 1000);
}

fn import_motion_frame_at_without_heart_rate(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
) {
    import_motion_frame_at_value_without_heart_rate(store, sensitivity, captured_at, 1000);
}

fn import_motion_frame_at_value(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
) {
    import_motion_frame_with_hex(
        store,
        sensitivity,
        captured_at,
        sample_value,
        k10_motion_frame_hex_with_value(sample_value),
    );
}

fn import_motion_frame_at_value_without_heart_rate(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
) {
    import_motion_frame_with_hex(
        store,
        sensitivity,
        captured_at,
        sample_value,
        k10_motion_frame_hex_with_value_and_heart_rate(sample_value, 0),
    );
}

fn import_motion_frame_at_value_and_heart_rate(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
    heart_rate: u8,
) {
    import_motion_frame_with_hex(
        store,
        sensitivity,
        captured_at,
        sample_value,
        k10_motion_frame_hex_with_value_and_heart_rate(sample_value, heart_rate),
    );
}

fn import_motion_frame_at_value_with_device_timestamp(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
    timestamp_seconds: u32,
) {
    import_motion_frame_at_value_with_device_timestamp_subseconds(
        store,
        sensitivity,
        captured_at,
        sample_value,
        timestamp_seconds,
        0,
    );
}

fn import_motion_frame_at_value_with_device_timestamp_subseconds(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
    timestamp_seconds: u32,
    timestamp_subseconds: u16,
) {
    import_motion_frame_with_hex(
        store,
        sensitivity,
        captured_at,
        sample_value,
        k10_motion_frame_hex_with_value_and_timestamp_subseconds(
            sample_value,
            timestamp_seconds,
            timestamp_subseconds,
        ),
    );
}

fn import_motion_frame_with_hex(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    sample_value: i16,
    frame_hex: String,
) {
    let frame_tag = &frame_hex[..frame_hex.len().min(48)];
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("app.motion.{sensitivity}.{captured_at}.{sample_value}.{frame_tag}"),
        frame_id: Some(format!(
            "app.motion.{sensitivity}.{captured_at}.{sample_value}.{frame_tag}.frame.0"
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex,
        sensitivity: sensitivity.to_string(),
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

fn import_historical_k21_motion_frame_at_with_device_timestamp(
    store: &GooseStore,
    sensitivity: &str,
    captured_at: &str,
    timestamp_seconds: u32,
) {
    let frame_hex = historical_k21_motion_frame_hex_with_timestamp(timestamp_seconds);
    let frame_tag = &frame_hex[..frame_hex.len().min(48)];
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("app.k21-history.{sensitivity}.{captured_at}.{frame_tag}"),
        frame_id: Some(format!(
            "app.k21-history.{sensitivity}.{captured_at}.{frame_tag}.frame.0"
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex,
        sensitivity: sensitivity.to_string(),
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

fn import_history_frame(store: &GooseStore, sensitivity: &str, marker_value: u8) {
    import_history_frame_at(store, sensitivity, marker_value, "2026-05-27T13:00:00Z");
}

fn import_history_frame_at(
    store: &GooseStore,
    sensitivity: &str,
    marker_value: u8,
    captured_at: &str,
) {
    import_history_frame_with_hex(
        store,
        sensitivity,
        marker_value,
        captured_at,
        historical_k18_frame_hex(marker_value),
    );
}

fn import_history_frame_at_with_device_timestamp(
    store: &GooseStore,
    sensitivity: &str,
    marker_value: u8,
    captured_at: &str,
    timestamp_seconds: u32,
) {
    import_history_frame_with_hex(
        store,
        sensitivity,
        marker_value,
        captured_at,
        historical_k18_frame_hex_with_timestamp(marker_value, timestamp_seconds),
    );
}

fn import_history_frame_with_hex(
    store: &GooseStore,
    sensitivity: &str,
    marker_value: u8,
    captured_at: &str,
    frame_hex: String,
) {
    let frame_tag = &frame_hex[..frame_hex.len().min(48)];
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("app.history.{sensitivity}.{marker_value}.{captured_at}.{frame_tag}"),
        frame_id: Some(format!(
            "app.history.{sensitivity}.{marker_value}.{captured_at}.{frame_tag}.frame.0"
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex,
        sensitivity: sensitivity.to_string(),
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

fn import_r17_frame_at(
    store: &GooseStore,
    sensitivity: &str,
    rr_candidates: &[i16],
    captured_at: &str,
) {
    let frame_stem = format!(
        "app.r17.{sensitivity}.{}.{}",
        rr_candidates.len(),
        captured_at
    );
    let frames = vec![CapturedFrameInput {
        evidence_id: frame_stem.clone(),
        frame_id: Some(format!("{frame_stem}.frame.0")),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: r17_frame_hex(rr_candidates),
        sensitivity: sensitivity.to_string(),
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

fn import_temperature_event(store: &GooseStore, sensitivity: &str, body: &[u8]) {
    let frames = vec![CapturedFrameInput {
        evidence_id: format!("app.temperature.{sensitivity}.{}", hex::encode(body)),
        frame_id: Some(format!(
            "app.temperature.{sensitivity}.{}.frame.0",
            hex::encode(body)
        )),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-27T00:10:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: temperature_event_frame_hex(body),
        sensitivity: sensitivity.to_string(),
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

fn k10_motion_frame_hex_with_value(sample_value: i16) -> String {
    k10_motion_frame_hex_with_value_and_heart_rate(sample_value, 72)
}

fn k10_motion_frame_hex_with_value_and_heart_rate(sample_value: i16, heart_rate: u8) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = heart_rate;
    for offset in [85, 285, 485, 688, 888, 1088] {
        for index in 0..100 {
            put_i16(&mut payload, offset + index * 2, sample_value);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn k10_motion_frame_hex_with_value_and_timestamp_subseconds(
    sample_value: i16,
    timestamp_seconds: u32,
    timestamp_subseconds: u16,
) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    put_u32(&mut payload, 7, timestamp_seconds);
    put_u16(&mut payload, 11, timestamp_subseconds);
    for offset in [85, 285, 485, 688, 888, 1088] {
        for index in 0..100 {
            put_i16(&mut payload, offset + index * 2, sample_value);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn historical_k21_motion_frame_hex_with_timestamp(timestamp_seconds: u32) -> String {
    let mut payload = vec![0; 1038];
    payload[0] = PACKET_TYPE_HISTORICAL_DATA;
    payload[1] = 21;
    put_u32(&mut payload, 7, timestamp_seconds);
    put_u16(&mut payload, 14, 321);
    put_u16(&mut payload, 16, 3);
    put_u16(&mut payload, 622, 3);
    put_i16(&mut payload, 20, -1);
    put_i16(&mut payload, 22, 2);
    put_i16(&mut payload, 24, -3);
    put_i16(&mut payload, 1032, 50);
    put_i16(&mut payload, 1034, 60);
    put_i16(&mut payload, 1036, 70);
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

fn historical_k18_frame_hex_with_skin_temperature(
    marker_value: u8,
    temperature_centi_c: i16,
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
    payload.resize(39, 0);
    put_i16(&mut payload, 37, temperature_centi_c);
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

fn historical_k18_frame_hex_with_timestamp(marker_value: u8, timestamp_seconds: u32) -> String {
    let mut payload = vec![
        PACKET_TYPE_HISTORICAL_DATA,
        18,
        1,
        0x04,
        0x03,
        0x02,
        0x01,
        0,
        0,
        0,
        0,
        0,
        0,
        0xaa,
        marker_value,
        0xbb,
        0xcc,
        0xdd,
        0xee,
        0xff,
    ];
    put_u32(&mut payload, 7, timestamp_seconds);
    payload.resize(24, 0);
    hex::encode(build_v5_payload_frame(&payload))
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn widget<'a>(
    widgets: &'a [RecoverySensorWidgetDiscovery],
    metric_id: &str,
) -> &'a RecoverySensorWidgetDiscovery {
    widgets
        .iter()
        .find(|widget| widget.metric_id == metric_id)
        .unwrap_or_else(|| panic!("missing recovery sensor widget {metric_id}"))
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}
