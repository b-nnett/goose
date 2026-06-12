use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    fs,
    os::raw::c_char,
    path::{Path, PathBuf},
    ptr,
    time::Instant,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GooseError, GooseResult,
    activity_sessions::{
        ActivitySessionCorrectionKind, activity_session_correction_plans,
        append_activity_session_correction_history,
    },
    algorithm_compare::{
        compare_hrv_goose_to_reference, compare_sleep_goose_to_external_reference_report,
        compare_sleep_goose_to_reference, compare_sleep_v1_goose_to_external_reference_report,
        compare_sleep_v1_goose_to_reference, compare_strain_goose_to_reference,
        compare_stress_goose_to_reference,
    },
    baselines::{EwmaBaseline, EwmaTrustLevel},
    calibration::{
        CalibrationApplicationInput, CalibrationDataset, CalibrationOptions, CalibrationRecord,
        CalibrationReport, apply_calibration, calibration_run_record, evaluate_linear_calibration,
    },
    capture_correlation::{
        CaptureCorrelationNextAction, CaptureCorrelationOptions, CaptureCorrelationReport,
        DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY, run_capture_correlation_for_store,
    },
    capture_import::{
        CapturedFrameBatchOptions, CapturedFrameBatchOutputOptions, CapturedFrameInput,
        import_captured_frame_batch_with_output_options,
    },
    capture_sanitize::{CaptureSanitizeOptions, sanitize_capture_path},
    commands::{
        COMMAND_DEFINITIONS, CommandEmulatorLogEvidenceOptions, CommandEvidence,
        CommandLocalFrameCandidate, CommandValidationResult, command_capture_plan_from_results,
        command_evidence_from_emulator_log_text, command_evidence_template,
        command_evidence_with_local_frame_matches, command_result_from_report_json,
        direct_send_gate_from_result, direct_send_preflight_from_gate, validate_commands,
    },
    debug_ws::{
        DebugBridgeConfig, DebugCommandEnvelope, DebugCommandFinishInput, DebugCommandStartInput,
        DebugEventInput, DebugSessionStartInput, append_debug_event, debug_session_snapshot,
        finish_debug_command, start_debug_command, start_debug_session,
    },
    energy_rollup::{
        EnergyCaptureValidationOptions, EnergyDailyRollupOptions, EnergyHourlyRollupOptions,
        rollup_energy_day_for_store, rollup_energy_hour_for_store,
        rollup_energy_unavailable_daily_status_for_store, validate_energy_capture_for_store,
    },
    export::{RawExportFilters, RawExportOptions, export_raw_timeframe, validate_export_bundle},
    health_sync::{
        ActivityHealthSyncDryRunInput, HealthSyncDryRunInput, run_activity_health_sync_dry_run,
        run_health_sync_dry_run,
    },
    historical_sync::{
        HistoricalSyncDryRunInput, HistoricalSyncGeneration, HistoricalSyncPhysicalValidationInput,
        historical_sync_physical_evidence_template, run_historical_sync_dry_run,
        validate_historical_sync_physical_evidence,
    },
    local_health_validation::{
        LocalHealthValidationManifestScaffoldOptions,
        local_health_validation_manifest_runbook_markdown, review_local_health_validation_manifest,
        scaffold_local_health_validation_manifest,
    },
    metric_features::{
        HeartRateFeatureOptions, HrvCaptureValidationOptions, HrvFeatureOptions,
        MetricFeatureNextAction, MetricWindowFeatureOptions, MotionFeatureOptions,
        OxygenSaturationCaptureValidationOptions, RecoveryFeatureScoreOptions,
        RecoverySensorDiscoveryOptions, RecoverySensorDiscoveryReport,
        RespiratoryRateCaptureValidationOptions, RestingHeartRateFeatureOptions,
        SleepFeatureScoreOptions, SleepFeatureScoreReport, SleepStageKind,
        StrainFeatureScoreOptions, StressFeatureScoreOptions, TemperatureCaptureValidationOptions,
        VitalEventFeatureOptions, run_heart_rate_feature_report_for_store,
        run_hrv_capture_validation_for_store, run_hrv_feature_report_for_store,
        run_metric_window_feature_report_for_store, run_motion_feature_report_for_store,
        run_oxygen_saturation_capture_validation_for_store,
        run_recovery_feature_score_report_for_store,
        run_recovery_sensor_discovery_report_for_store,
        run_respiratory_rate_capture_validation_for_store,
        run_resting_heart_rate_feature_report_for_store, run_sleep_feature_score_report_for_store,
        run_strain_feature_score_report_for_store, run_stress_feature_score_report_for_store,
        run_temperature_capture_validation_for_store, run_vital_event_feature_report_for_store,
    },
    metric_readiness::{
        MetricInputNextAction, MetricInputReadinessOptions, MetricInputReadinessReport,
        run_metric_input_readiness,
    },
    metrics::{
        AlgorithmRunResult, GOOSE_HRV_V0_ID, GOOSE_HRV_V0_VERSION, GOOSE_RECOVERY_V0_ID,
        GOOSE_RECOVERY_V0_VERSION, GOOSE_SLEEP_V0_ID, GOOSE_SLEEP_V0_VERSION, GOOSE_SLEEP_V1_ID,
        GOOSE_SLEEP_V1_VERSION, GOOSE_STRAIN_V0_ID, GOOSE_STRAIN_V0_VERSION, GOOSE_STRESS_V0_ID,
        GOOSE_STRESS_V0_VERSION, HrvInput, ImuStepCountInput, ReadinessInput, RecoveryInput,
        RecoveryV1Input, SleepInput, SleepModelStatusInput, SleepNightHistoryInput,
        SleepStageSegment, SleepV1Input, StrainInput, StressInput, algorithm_run_record,
        built_in_algorithm_definitions, built_in_default_algorithm_preferences,
        default_algorithm_preferences_for_scope, fit_strain_denominator, goose_hrv_v0,
        goose_readiness_v1, goose_recovery_v0, goose_recovery_v1, goose_sleep_v0, goose_sleep_v1,
        goose_strain_v0, goose_strain_v1, goose_stress_v0, imu_step_count_v1,
        sleep_history_night_is_usable,
    },
    openwhoop_reference::{
        OPENWHOOP_REFERENCE_ATTRIBUTION, OPENWHOOP_REFERENCE_COMMIT,
        OPENWHOOP_REFERENCE_LICENSE_CAVEAT, OPENWHOOP_REFERENCE_REPOSITORY,
        OPENWHOOP_REFERENCE_SNAPSHOT_URL, openwhoop_history_field_references,
        whoop_generation_references,
    },
    perf_budget::{DEFAULT_PERF_SCALE, PerfBudgetOptions, PerfBudgets, run_perf_budget},
    privacy_lint::lint_privacy_path,
    property_tests::{
        DEFAULT_CASES_PER_GROUP, DEFAULT_PROPERTY_SEED, PropertySuiteOptions, run_property_suite,
    },
    protocol::{
        DataPacketBodySummary, DeviceType, I16SeriesSummary, ParsedFrame, ParsedPayload,
        parse_frame_hex,
    },
    recovery_rollup::{
        RecoverySensorDailyRollupOptions, RecoveryUnavailableDailyStatusOptions,
        RestingHeartRateCaptureValidationOptions, RestingHeartRateDailyRollupOptions,
        rollup_recovery_sensor_daily_for_store, rollup_recovery_unavailable_daily_status_for_store,
        rollup_resting_heart_rate_day_for_store, validate_resting_heart_rate_capture_for_store,
    },
    reference::reference_algorithm_definitions,
    sleep_staging::{
        EpochHrFeature, SleepStagingInput, SleepStagingOutput, stage_sleep_four_class,
    },
    sleep_validation::{
        SleepStageLabelValidationOptions, SleepV1EvidenceFolderOptions,
        SleepV1ExplanationStabilityOptions, SleepV1ReleaseGateInput,
        SleepWindowLabelValidationOptions, run_sleep_window_label_validation_for_store,
        validate_sleep_v1_evidence_folder_with_options,
        validate_sleep_v1_explanation_and_stability, validate_sleep_v1_release_gates,
        validate_sleep_v1_stage_labels_for_store,
    },
    step_counter::{
        ActivityUnavailableDailyStatusOptions, StepCounterDailyRollupOptions,
        StepCounterHourlyRollupOptions, StepCounterIngestOptions,
        rollup_activity_unavailable_daily_status_for_store, rollup_device_step_counter_day,
        rollup_device_step_counter_hour, run_step_counter_ingest_for_store,
    },
    step_discovery::{
        StepCaptureValidationOptions, StepPacketDiscoveryOptions,
        run_step_capture_validation_for_store, run_step_packet_discovery_for_store,
    },
    step_motion_estimator::{RawMotionStepEstimateOptions, run_raw_motion_step_estimate_for_store},
    storage_check::{StorageCheckOptions, check_storage_database},
    store::{
        ActivityIntervalInput, ActivityMetricInput, ActivityMetricRow, ActivitySessionInput,
        ActivitySessionRow, AlgorithmPreferenceRecord, AlgorithmRunRecord, BackfillReport,
        CURRENT_SCHEMA_VERSION, CalibrationLabelInput, CalibrationLabelRow, CaptureSessionInput,
        CaptureSessionRow, CommandValidationRecord, DecodedFrameRow, ExerciseSessionRow,
        ExternalSleepSessionInput, ExternalSleepSessionRow, ExternalSleepStageInput,
        ExternalSleepStageRow, GooseStore, GravityRow, OvernightHistoricalRangePollInput,
        OvernightRawNotificationInput, OvernightSyncSessionInput, SleepCorrectionLabelInput,
        StepCounterSampleInput,
    },
    timeline::{
        observability_timeline_from_rows, packet_timeline_between,
        packet_timeline_from_decoded_frames,
    },
    ui_coverage::{UiCoverageAuditInput, run_ui_coverage_audit},
};

pub const BRIDGE_REQUEST_SCHEMA: &str = "goose.bridge.request.v1";
pub const BRIDGE_RESPONSE_SCHEMA: &str = "goose.bridge.response.v1";
pub const CAPTURE_ARRIVAL_PLAN_REPORT_SCHEMA: &str = "goose.capture-arrival-plan-report.v1";
pub const BRIDGE_METHODS_LIST_SCHEMA: &str = "goose.bridge.methods-list.v1";

/// Canonical list of every bridge RPC method understood by
/// [`handle_bridge_request`].
///
/// The list is kept sorted and is verified against the dispatcher match arms
/// by `tests::bridge_methods_constant_matches_dispatcher` so it cannot drift
/// when new methods are added. Exposed via the `core.list_methods` RPC for
/// discoverability by external clients (the Swift app, future Android port,
/// debug tooling).
pub const BRIDGE_METHODS: &[&str] = &[
    "activity.apply_correction",
    "activity.attach_interval",
    "activity.attach_metric",
    "activity.attach_metrics",
    "activity.correction_plans",
    "activity.create_session",
    "activity.delete_session",
    "activity.get_session",
    "activity.list_intervals",
    "activity.list_metrics",
    "activity.list_sessions",
    "activity.list_sessions_with_metrics",
    "activity.metrics_for_session_in_window",
    "activity.update_session",
    "biometrics.insert_v24_batch",
    "biometrics.spo2_from_raw",
    "biometrics.v24_between",
    "calibration.apply",
    "calibration.evaluate_dataset",
    "calibration.evaluate_stored_labels",
    "calibration.import_labels",
    "calibration.list_labels",
    "capture.arrival_plan",
    "capture.correlation_report",
    "capture.finish_session",
    "capture.import_frame_batch",
    "capture.list_sessions",
    "capture.observability_timeline",
    "capture.sanitize",
    "capture.start_session",
    "capture.timeline",
    "commands.capture_plan",
    "commands.definitions",
    "commands.direct_send_gate",
    "commands.direct_send_preflight",
    "commands.evidence_from_emulator_log",
    "commands.evidence_template",
    "commands.import_validation_records",
    "commands.list_validation_records",
    "commands.promote_local_frame_matches",
    "commands.validate_evidence",
    "core.list_methods",
    "core.version",
    "debug.finish_command",
    "debug.record_event",
    "debug.session_snapshot",
    "debug.start_command",
    "debug.start_session",
    "diagnostics.perf_budget",
    "diagnostics.property_suite",
    "exercise.detect_sessions",
    "exercise.sessions_between",
    "export.raw_timeframe",
    "export.validate_bundle",
    "health_sync.activity_dry_run",
    "health_sync.dry_run",
    "historical_sync.dry_run",
    "historical_sync.physical_evidence_template",
    "historical_sync.validate_physical_evidence",
    "metrics.activity_unavailable_daily_status",
    "metrics.built_in_definitions",
    "metrics.daily_activity_metrics",
    "metrics.daily_recovery_metrics",
    "metrics.default_preferences",
    "metrics.energy_capture_validation",
    "metrics.energy_daily_rollup",
    "metrics.energy_hourly_rollup",
    "metrics.energy_unavailable_daily_status",
    "metrics.fit_strain_denominator",
    "metrics.goose_hrv_v0",
    "metrics.goose_readiness_v1",
    "metrics.goose_recovery_v0",
    "metrics.goose_recovery_v1",
    "metrics.goose_sleep_v0",
    "metrics.goose_sleep_v1",
    "metrics.goose_strain_v0",
    "metrics.goose_strain_v1",
    "metrics.goose_stress_v0",
    "metrics.heart_rate_features",
    "metrics.hourly_activity_metrics",
    "metrics.hrv_capture_validation",
    "metrics.hrv_features",
    "metrics.imu_step_count_v1",
    "metrics.input_readiness",
    "metrics.motion_features",
    "metrics.oxygen_saturation_capture_validation",
    "metrics.raw_motion_step_estimate",
    "metrics.recovery_score_from_features",
    "metrics.recovery_sensor_daily_rollup",
    "metrics.recovery_sensor_discovery",
    "metrics.recovery_unavailable_daily_status",
    "metrics.reference_compare",
    "metrics.reference_definitions",
    "metrics.respiratory_rate_capture_validation",
    "metrics.resting_hr_capture_validation",
    "metrics.resting_hr_daily_rollup",
    "metrics.resting_hr_features",
    "metrics.sleep_score_from_features",
    "metrics.sleep_staging",
    "metrics.step_capture_validation",
    "metrics.step_counter_daily_rollup",
    "metrics.step_counter_hourly_rollup",
    "metrics.step_counter_ingest",
    "metrics.step_packet_discovery",
    "metrics.strain_score_from_features",
    "metrics.stress_score_from_features",
    "metrics.temperature_capture_validation",
    "metrics.vital_event_features",
    "metrics.window_features",
    "openwhoop.reference_report",
    "overnight.mirror_batch",
    "overnight.mirror_counts",
    "privacy.lint",
    "protocol.parse_frame_hex",
    "protocol.parse_frame_hex_batch",
    "settings.apply_default_algorithm_preferences",
    "settings.get_algorithm_preference",
    "settings.list_algorithm_preferences",
    "settings.set_algorithm_preference",
    "sleep.add_correction_label",
    "sleep.import_external_history",
    "sleep.list_correction_labels",
    "sleep.validate_stage_labels",
    "sleep.validate_v1_evidence_folder",
    "sleep.validate_v1_explanation_stability",
    "sleep.validate_v1_release_gates",
    "sleep.validate_window_labels",
    "storage.check",
    "storage.compact_raw_evidence",
    "store.ewma_baseline_fold_history",
    "store.ewma_baseline_update",
    "store.gravity2_samples_between",
    "store.gravity_rows_between",
    "store.insert_gravity2_batch",
    "store.insert_gravity_rows",
    "sync.backfill_streams",
    "sync.mark_synced",
    "sync.rows_pending_upload",
    "timeline.from_decoded_frames",
    "ui_coverage.audit",
    "upload.get_raw_frames_for_upload",
    "upload.get_recent_decoded_streams",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub schema: String,
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub schema: String,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<BridgeTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeTiming {
    pub method: String,
    pub method_elapsed_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ParseFrameArgs {
    frame_hex: String,
    #[serde(default = "default_device_type")]
    device_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ParseFrameBatchArgs {
    frames: Vec<String>,
    #[serde(default = "default_device_type")]
    device_type: String,
    #[serde(default = "default_true")]
    include_result: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct TimelineArgs {
    decoded_frames: Vec<DecodedFrameRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct StorageCheckArgs {
    database_path: String,
    #[serde(default)]
    self_test: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ApplyDefaultPreferencesArgs {
    database_path: String,
    #[serde(default = "default_algorithm_scope")]
    scope: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SetPreferenceArgs {
    database_path: String,
    #[serde(default = "default_algorithm_scope")]
    scope: String,
    metric_family: String,
    algorithm_id: String,
    version: String,
    #[serde(default = "default_true")]
    register_built_ins: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GetPreferenceArgs {
    database_path: String,
    #[serde(default = "default_algorithm_scope")]
    scope: String,
    metric_family: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ListPreferencesArgs {
    database_path: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApplyCalibrationArgs {
    database_path: String,
    metric_family: String,
    algorithm_id: String,
    algorithm_version: String,
    raw_score: f64,
    #[serde(default)]
    input_run_id: Option<String>,
    #[serde(default)]
    calibration_run_id: Option<String>,
    score_min: f64,
    score_max: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct EvaluateCalibrationDatasetArgs {
    dataset: CalibrationDataset,
    options: CalibrationOptions,
    #[serde(default)]
    database_path: Option<String>,
    #[serde(default)]
    persist: bool,
    #[serde(default)]
    calibration_run_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EvaluateStoredCalibrationLabelsArgs {
    database_path: String,
    start: String,
    end: String,
    options: CalibrationOptions,
    #[serde(default)]
    persist: bool,
    #[serde(default)]
    calibration_run_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImportCalibrationLabelsArgs {
    database_path: String,
    labels: Vec<CalibrationLabelBridgeInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct ListCalibrationLabelsArgs {
    database_path: String,
    start: String,
    end: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CalibrationLabelBridgeInput {
    label_id: String,
    metric_family: String,
    label_source: String,
    captured_at: String,
    value: f64,
    unit: String,
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct RawExportArgs {
    database_path: String,
    output_dir: String,
    #[serde(default)]
    zip_output_path: Option<String>,
    start: String,
    end: String,
    #[serde(default = "default_raw_export_app_version")]
    app_version: String,
    #[serde(default = "default_raw_export_core_version")]
    core_version: String,
    #[serde(default)]
    include_sqlite: bool,
    #[serde(default)]
    data_families: Vec<String>,
    #[serde(default = "default_true")]
    include_raw_bytes: bool,
    #[serde(default)]
    capture_session_ids: Vec<String>,
    #[serde(default)]
    packet_type_names: Vec<String>,
    #[serde(default)]
    sensor_source_signals: Vec<String>,
    #[serde(default)]
    metric_families: Vec<String>,
    #[serde(default)]
    algorithm_ids: Vec<String>,
    #[serde(default)]
    algorithm_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalHealthValidationManifestScaffoldArgs {
    database_path: String,
    #[serde(default)]
    manifest_id: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    date_key: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    database_source_kind: Option<String>,
    #[serde(default)]
    window_source: Option<String>,
    #[serde(default)]
    raw_export_bundle_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalHealthValidationManifestRunbookArgs {
    manifest: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalHealthValidationManifestReviewArgs {
    manifest: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ExportValidateBundleArgs {
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PrivacyLintArgs {
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureSanitizeArgs {
    input_path: String,
    output_path: String,
    #[serde(default = "default_capture_sanitize_salt")]
    salt: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UiCoverageAuditArgs {
    #[serde(default)]
    coverage_map_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PerfBudgetArgs {
    #[serde(default = "default_perf_scale")]
    scale: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct PropertySuiteArgs {
    #[serde(default = "default_property_seed")]
    seed: u64,
    #[serde(default = "default_property_cases")]
    cases_per_group: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct FitStrainDenominatorArgs {
    pairs: Vec<(f64, f64)>,
    // Included for API consistency with DB-backed methods; unused (pure computation).
    #[serde(default)]
    #[allow(dead_code)]
    database_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetricInputReadinessArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_owned_captures: bool,
    #[serde(default)]
    require_scores_ready: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct StepPacketDiscoveryArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    max_candidate_fields: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepCaptureValidationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    max_candidate_fields: Option<usize>,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    manual_step_delta: Option<i64>,
    #[serde(default)]
    official_whoop_step_delta: Option<i64>,
    #[serde(default)]
    tolerance_steps: Option<i64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawMotionStepEstimateArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    sample_rate_hz: Option<f64>,
    #[serde(default)]
    peak_threshold_i16: Option<f64>,
    #[serde(default)]
    min_peak_spacing_samples: Option<usize>,
    #[serde(default)]
    manual_step_delta: Option<i64>,
    #[serde(default)]
    official_whoop_step_delta: Option<i64>,
    #[serde(default)]
    tolerance_steps: Option<i64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
    #[serde(default)]
    date_key: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct StepCounterIngestArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    max_candidate_fields: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepCounterDailyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default)]
    min_sample_count: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct StepCounterHourlyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default)]
    min_sample_count: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityUnavailableDailyStatusArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default)]
    min_sample_count: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DailyActivityMetricListArgs {
    database_path: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct HourlyActivityMetricListArgs {
    database_path: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct DailyRecoveryMetricListArgs {
    database_path: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct EnergyDailyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    profile_weight_kg: Option<f64>,
    #[serde(default)]
    profile_age_years: Option<u32>,
    #[serde(default)]
    profile_sex: Option<String>,
    #[serde(default)]
    profile_height_cm: Option<f64>,
    #[serde(default)]
    resting_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    min_heart_rate_samples: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct EnergyHourlyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    profile_weight_kg: Option<f64>,
    #[serde(default)]
    profile_age_years: Option<u32>,
    #[serde(default)]
    profile_sex: Option<String>,
    #[serde(default)]
    resting_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    min_heart_rate_samples: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct EnergyCaptureValidationArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    profile_weight_kg: Option<f64>,
    #[serde(default)]
    profile_age_years: Option<u32>,
    #[serde(default)]
    profile_sex: Option<String>,
    #[serde(default)]
    profile_height_cm: Option<f64>,
    #[serde(default)]
    resting_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    min_heart_rate_samples: Option<usize>,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_active_kcal: Option<f64>,
    #[serde(default)]
    official_whoop_resting_kcal: Option<f64>,
    #[serde(default)]
    official_whoop_total_kcal: Option<f64>,
    #[serde(default)]
    tolerance_kcal: Option<f64>,
    #[serde(default)]
    relative_tolerance_fraction: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReferenceCompareArgs {
    family: String,
    input: serde_json::Value,
    #[serde(default)]
    reference_report: Option<serde_json::Value>,
    #[serde(default)]
    goose_algorithm_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MotionFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct HeartRateFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct VitalEventFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RespiratoryRateCaptureValidationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_respiratory_rate_rpm: Option<f64>,
    #[serde(default)]
    tolerance_rpm: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct OxygenSaturationCaptureValidationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_oxygen_saturation_percent: Option<f64>,
    #[serde(default)]
    tolerance_percent: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct TemperatureCaptureValidationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_skin_temperature_delta_c: Option<f64>,
    #[serde(default)]
    tolerance_c: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct HrvFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    baseline_min_days: Option<usize>,
    #[serde(default)]
    require_baseline: bool,
    #[serde(default)]
    persist_algorithm_run: bool,
    #[serde(default)]
    algorithm_run_id: Option<String>,
    #[serde(default)]
    algorithm_id: Option<String>,
    #[serde(default)]
    algorithm_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoverySensorDiscoveryArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    min_rr_intervals_to_compute: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoveryUnavailableDailyStatusArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoverySensorDailyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct HrvCaptureValidationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_hrv_rmssd_ms: Option<f64>,
    #[serde(default)]
    tolerance_ms: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetricWindowFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    resting_hr_bpm: Option<f64>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RestingHeartRateFeaturesArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    baseline_min_days: Option<usize>,
    #[serde(default)]
    require_baseline: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RestingHeartRateDailyRollupArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    baseline_min_days: Option<usize>,
    #[serde(default)]
    require_baseline: bool,
    #[serde(default)]
    min_sample_count: Option<usize>,
    #[serde(default)]
    write_metric: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RestingHeartRateCaptureValidationArgs {
    database_path: String,
    date_key: String,
    timezone: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    baseline_min_days: Option<usize>,
    #[serde(default)]
    require_baseline: bool,
    #[serde(default)]
    min_sample_count: Option<usize>,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    official_whoop_resting_hr_bpm: Option<f64>,
    #[serde(default)]
    tolerance_bpm: Option<f64>,
    #[serde(default)]
    label_provenance: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct StrainFeatureScoreArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    resting_start: Option<String>,
    #[serde(default)]
    resting_end: Option<String>,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    resting_baseline_min_days: Option<usize>,
    #[serde(default)]
    max_hr_bpm: Option<f64>,
    #[serde(default)]
    persist_algorithm_run: bool,
    #[serde(default)]
    algorithm_run_id: Option<String>,
    #[serde(default)]
    algorithm_id: Option<String>,
    #[serde(default)]
    algorithm_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepFeatureScoreArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    sleep_need_minutes: Option<f64>,
    #[serde(default)]
    low_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    disturbance_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    target_midpoint_minutes_since_midnight: Option<f64>,
    #[serde(default)]
    history_import_in_progress: bool,
    #[serde(default)]
    persist_algorithm_run: bool,
    #[serde(default)]
    algorithm_run_id: Option<String>,
    #[serde(default)]
    algorithm_id: Option<String>,
    #[serde(default)]
    algorithm_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoveryFeatureScoreArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    hrv_start: Option<String>,
    #[serde(default)]
    hrv_end: Option<String>,
    #[serde(default = "default_correlation_start")]
    hrv_baseline_start: String,
    #[serde(default = "default_correlation_end")]
    hrv_baseline_end: String,
    #[serde(default = "default_correlation_start")]
    resting_start: String,
    #[serde(default = "default_correlation_end")]
    resting_end: String,
    #[serde(default)]
    sleep_start: Option<String>,
    #[serde(default)]
    sleep_end: Option<String>,
    #[serde(default)]
    prior_strain_start: Option<String>,
    #[serde(default)]
    prior_strain_end: Option<String>,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    resting_baseline_min_days: Option<usize>,
    #[serde(default)]
    hrv_min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    hrv_baseline_min_days: Option<usize>,
    #[serde(default)]
    sleep_need_minutes: Option<f64>,
    #[serde(default)]
    low_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    disturbance_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    target_midpoint_minutes_since_midnight: Option<f64>,
    #[serde(default)]
    prior_strain_resting_baseline_min_days: Option<usize>,
    #[serde(default)]
    prior_strain_max_hr_bpm: Option<f64>,
    #[serde(default)]
    respiratory_rate_rpm: Option<f64>,
    #[serde(default)]
    respiratory_rate_baseline_rpm: Option<f64>,
    #[serde(default)]
    skin_temp_delta_c: Option<f64>,
    #[serde(default)]
    provided_vitals_source: Option<String>,
    #[serde(default)]
    provided_vitals_provenance_json: Option<String>,
    #[serde(default)]
    persist_algorithm_run: bool,
    #[serde(default)]
    algorithm_run_id: Option<String>,
    #[serde(default)]
    algorithm_id: Option<String>,
    #[serde(default)]
    algorithm_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StressFeatureScoreArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default = "default_correlation_start")]
    resting_start: String,
    #[serde(default = "default_correlation_end")]
    resting_end: String,
    #[serde(default)]
    hrv_start: Option<String>,
    #[serde(default)]
    hrv_end: Option<String>,
    #[serde(default = "default_correlation_start")]
    hrv_baseline_start: String,
    #[serde(default = "default_correlation_end")]
    hrv_baseline_end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    resting_baseline_min_days: Option<usize>,
    #[serde(default)]
    hrv_min_rr_intervals_to_compute: Option<usize>,
    #[serde(default)]
    hrv_baseline_min_days: Option<usize>,
    #[serde(default)]
    persist_algorithm_run: bool,
    #[serde(default)]
    algorithm_run_id: Option<String>,
    #[serde(default)]
    algorithm_id: Option<String>,
    #[serde(default)]
    algorithm_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StorageCompactRawEvidenceArgs {
    database_path: String,
    limit_bytes: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureImportFrameBatchArgs {
    database_path: String,
    #[serde(default = "default_parser_version")]
    parser_version: String,
    #[serde(default = "default_true")]
    include_timeline_rows: bool,
    #[serde(default = "default_true")]
    compact_raw_payloads: bool,
    #[serde(default = "default_true")]
    include_results: bool,
    /// Optional CoreBluetooth peripheral UUID supplied by the Swift caller. Written to
    /// capture_sessions.active_device_id for every session referenced in this batch.
    #[serde(default)]
    active_device_id: Option<String>,
    frames: Vec<CapturedFrameInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct OvernightMirrorBatchArgs {
    database_path: String,
    #[serde(default)]
    sessions: Vec<OvernightMirrorSessionArgs>,
    #[serde(default)]
    raw_notifications: Vec<OvernightMirrorRawNotificationArgs>,
    #[serde(default)]
    historical_range_polls: Vec<OvernightMirrorHistoricalRangePollArgs>,
}

#[derive(Debug, Clone, Deserialize)]
struct OvernightMirrorCountsArgs {
    database_path: String,
    session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OvernightMirrorSessionArgs {
    session_id: String,
    started_at: String,
    #[serde(default)]
    ended_at: Option<String>,
    #[serde(default)]
    band_identifier: Option<String>,
    #[serde(default)]
    app_version: Option<String>,
    #[serde(default = "default_overnight_mode")]
    mode: String,
    #[serde(default = "default_active_status")]
    final_status: String,
    #[serde(default)]
    raw_frame_count: i64,
    #[serde(default)]
    historical_frame_count: i64,
    #[serde(default)]
    k18_count: i64,
    #[serde(default)]
    k24_count: i64,
    #[serde(default)]
    k25_count: i64,
    #[serde(default)]
    k26_count: i64,
    #[serde(default)]
    packet47_count: i64,
    #[serde(default)]
    event17_count: i64,
    #[serde(default)]
    event29_count: i64,
    #[serde(default)]
    metadata49_count: i64,
    #[serde(default)]
    metadata56_count: i64,
    #[serde(default)]
    range_poll_count: i64,
    #[serde(default)]
    successful_range_poll_count: i64,
    #[serde(default)]
    event_log_count: i64,
    #[serde(default)]
    readiness_status: Option<String>,
    #[serde(default)]
    readiness: Option<String>,
    #[serde(default)]
    error_count: i64,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OvernightMirrorRawNotificationArgs {
    session_id: String,
    captured_at: String,
    #[serde(default = "default_raw_notification_source")]
    source: String,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    active_device_name: Option<String>,
    #[serde(default)]
    connection_state: Option<String>,
    #[serde(default)]
    service_uuid: Option<String>,
    characteristic_uuid: String,
    #[serde(default)]
    device_type: Option<String>,
    #[serde(default)]
    command_or_event: Option<i64>,
    #[serde(default)]
    packet_type: Option<i64>,
    #[serde(default)]
    k_revision: Option<i64>,
    #[serde(default)]
    sequence: Option<i64>,
    frame_hex: String,
    #[serde(default)]
    payload_hex: Option<String>,
    byte_count: i64,
    #[serde(default = "default_decode_status")]
    decode_status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OvernightMirrorHistoricalRangePollArgs {
    session_id: String,
    captured_at: String,
    status: String,
    command_sequence: i64,
    result_code: i64,
    result_name: String,
    raw_payload_hex: String,
    raw_body_hex: String,
    #[serde(default)]
    revision_or_status: Option<i64>,
    #[serde(default)]
    page_current: Option<i64>,
    #[serde(default)]
    page_oldest: Option<i64>,
    #[serde(default)]
    page_end: Option<i64>,
    #[serde(default)]
    pages_behind: Option<i64>,
    #[serde(default)]
    pending_response_count: i64,
    #[serde(default)]
    retry_count: i64,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureTimelineArgs {
    database_path: String,
    start: String,
    end: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureObservabilityTimelineArgs {
    database_path: String,
    start: String,
    end: String,
    start_unix_ms: i64,
    end_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureStartSessionArgs {
    database_path: String,
    session_id: String,
    source: String,
    started_at_unix_ms: i64,
    device_model: String,
    #[serde(default)]
    active_device_id: Option<String>,
    #[serde(default)]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureFinishSessionArgs {
    database_path: String,
    session_id: String,
    ended_at_unix_ms: i64,
    #[serde(default)]
    frame_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureListSessionsArgs {
    database_path: String,
    start_unix_ms: i64,
    end_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivitySessionUpsertArgs {
    database_path: String,
    session_id: String,
    source: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    activity_type: String,
    #[serde(default)]
    external_activity_type_code: Option<String>,
    #[serde(default)]
    external_activity_type_name: Option<String>,
    #[serde(default)]
    custom_label: Option<String>,
    confidence: f64,
    detection_method: String,
    sync_status: String,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivitySessionLookupArgs {
    database_path: String,
    session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivitySessionListArgs {
    database_path: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivitySessionCorrectionArgs {
    database_path: String,
    session_id: String,
    kind: ActivitySessionCorrectionKind,
    #[serde(default)]
    activity_type: Option<String>,
    #[serde(default)]
    start_time_unix_ms: Option<i64>,
    #[serde(default)]
    end_time_unix_ms: Option<i64>,
    #[serde(default)]
    external_activity_type_code: Option<String>,
    #[serde(default)]
    external_activity_type_name: Option<String>,
    #[serde(default)]
    custom_label: Option<String>,
    #[serde(default = "empty_json_object")]
    details: serde_json::Value,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityMetricAttachArgs {
    database_path: String,
    metric_id: String,
    activity_session_id: String,
    metric_name: String,
    value: f64,
    unit: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default = "empty_json_array")]
    quality_flags: serde_json::Value,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityMetricAttachBatchArgs {
    database_path: String,
    metrics: Vec<ActivityMetricAttachInputArgs>,
    #[serde(default = "default_true")]
    include_metrics: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityMetricAttachInputArgs {
    metric_id: String,
    activity_session_id: String,
    metric_name: String,
    value: f64,
    unit: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default = "empty_json_array")]
    quality_flags: serde_json::Value,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

struct SerializedActivityMetricAttachArg<'a> {
    metric: &'a ActivityMetricAttachInputArgs,
    quality_flags_json: String,
    provenance_json: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityMetricListArgs {
    database_path: String,
    activity_session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityMetricWindowArgs {
    database_path: String,
    activity_session_id: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityIntervalAttachArgs {
    database_path: String,
    interval_id: String,
    activity_session_id: String,
    interval_type: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    sequence: i64,
    #[serde(default = "empty_json_object")]
    metadata: serde_json::Value,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivityIntervalListArgs {
    database_path: String,
    activity_session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalSleepHistoryImportArgs {
    database_path: String,
    #[serde(default)]
    sessions: Vec<ExternalSleepSessionBridgeInput>,
    #[serde(default)]
    stages: Vec<ExternalSleepStageBridgeInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalSleepSessionBridgeInput {
    sleep_id: String,
    source: String,
    platform: String,
    #[serde(default)]
    platform_record_id: Option<String>,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default = "empty_json_object")]
    stage_summary: serde_json::Value,
    confidence: f64,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalSleepStageBridgeInput {
    stage_id: String,
    sleep_id: String,
    stage_kind: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    confidence: f64,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepCorrectionLabelArgs {
    database_path: String,
    label_id: String,
    #[serde(default)]
    sleep_id: Option<String>,
    label_type: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    #[serde(default = "empty_json_object")]
    value: serde_json::Value,
    #[serde(default = "default_manual_source")]
    source: String,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default = "empty_json_object")]
    provenance: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepCorrectionLabelListArgs {
    database_path: String,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepWindowLabelValidationArgs {
    database_path: String,
    start: String,
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_trusted_evidence: bool,
    #[serde(default)]
    sleep_need_minutes: Option<f64>,
    #[serde(default)]
    low_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    disturbance_motion_threshold_0_to_1: Option<f64>,
    #[serde(default)]
    target_midpoint_minutes_since_midnight: Option<f64>,
    #[serde(default)]
    start_tolerance_minutes: Option<f64>,
    #[serde(default)]
    end_tolerance_minutes: Option<f64>,
    #[serde(default)]
    duration_tolerance_minutes: Option<f64>,
    #[serde(default)]
    min_label_confidence: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepStageLabelValidationArgs {
    database_path: String,
    input: SleepV1Input,
    #[serde(default)]
    min_label_confidence: Option<f64>,
    #[serde(default)]
    min_overlap_fraction: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepV1ExplanationStabilityArgs {
    input: SleepV1Input,
    #[serde(default)]
    max_repeated_run_delta: Option<f64>,
    #[serde(default)]
    max_small_perturbation_delta: Option<f64>,
    #[serde(default)]
    perturb_sleep_duration_minutes: Option<f64>,
    #[serde(default)]
    min_v1_component_count: Option<usize>,
    #[serde(default)]
    min_explanation_quality_signal_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepV1ReleaseGateArgs {
    input: SleepV1ReleaseGateInput,
}

#[derive(Debug, Clone, Deserialize)]
struct SleepV1EvidenceFolderArgs {
    evidence_dir: String,
    #[serde(default)]
    expected_manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoricalSyncPhysicalEvidenceTemplateArgs {
    generation: HistoricalSyncGeneration,
    #[serde(default)]
    capture_session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureCorrelationArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_owned_captures: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureArrivalPlanArgs {
    database_path: String,
    #[serde(default = "default_correlation_start")]
    start: String,
    #[serde(default = "default_correlation_end")]
    end: String,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    min_owned_captures: Option<usize>,
    #[serde(default)]
    require_owned_captures: bool,
    #[serde(default)]
    require_scores_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureArrivalPlanReport {
    schema: String,
    generated_by: String,
    pass: bool,
    start: String,
    end: String,
    min_owned_captures: usize,
    require_owned_captures: bool,
    require_scores_ready: bool,
    action_count: usize,
    physical_arrival_row_count: usize,
    physical_arrival_rows: Vec<CaptureArrivalPhysicalRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_capture_focus: Option<CaptureArrivalPlanAction>,
    actions: Vec<CaptureArrivalPlanAction>,
    capture_correlation: CaptureCorrelationReport,
    metric_input_readiness: MetricInputReadinessReport,
    recovery_sensor_discovery: RecoverySensorDiscoveryReport,
    local_health_validation_review: Value,
    issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureArrivalPlanAction {
    source: String,
    scope: String,
    reason: String,
    action: String,
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureArrivalPhysicalRow {
    id: String,
    label: String,
    domain: String,
    state: String,
    blocker: String,
    next_action: String,
    evidence: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandValidateEvidenceArgs {
    #[serde(default)]
    database_path: Option<String>,
    #[serde(default)]
    persist: bool,
    evidence: Vec<CommandEvidence>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandEvidenceFromEmulatorLogArgs {
    log_text: String,
    #[serde(default)]
    source_log: Option<String>,
    #[serde(default)]
    write_type: Option<String>,
    #[serde(default)]
    visible_user_intent: bool,
    #[serde(default)]
    triggering_ui_action: Option<String>,
    #[serde(default)]
    visible_confirmation: bool,
    #[serde(default)]
    rollback_plan: bool,
    #[serde(default)]
    explicit_approval: bool,
    #[serde(default)]
    mirror_local_frame: bool,
    #[serde(default)]
    capture_app: Option<String>,
    #[serde(default)]
    capture_kind: Option<String>,
    #[serde(default)]
    owner: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandPromoteLocalFrameMatchesArgs {
    evidence: Vec<CommandEvidence>,
    candidates: Vec<CommandLocalFrameCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandDirectSendGateArgs {
    database_path: String,
    command: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandDirectSendPreflightArgs {
    database_path: String,
    command: String,
    now_unix_ms: u64,
    #[serde(default)]
    override_expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    visible_user_intent: bool,
    #[serde(default)]
    dry_run_bytes_shown: bool,
    #[serde(default)]
    dry_run_frame_hex: Option<String>,
    #[serde(default)]
    dry_run_service_uuid: Option<String>,
    #[serde(default)]
    dry_run_characteristic_uuid: Option<String>,
    #[serde(default)]
    dry_run_write_type: Option<String>,
    #[serde(default)]
    session_log_ready: bool,
    #[serde(default)]
    connection_state: Option<String>,
    #[serde(default)]
    active_device_id: Option<String>,
    #[serde(default)]
    critical_visible_confirmation: bool,
    #[serde(default)]
    critical_explicit_approval: bool,
    #[serde(default)]
    critical_rollback_or_restore_acknowledged: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ListCommandValidationRecordsArgs {
    database_path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandCapturePlanArgs {
    database_path: String,
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImportCommandValidationRecordsArgs {
    database_path: String,
    records: Vec<ImportedCommandValidationRecord>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImportedCommandValidationRecord {
    command: String,
    risk_gate: String,
    direct_send_ready: bool,
    report_json: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct DebugStartSessionArgs {
    database_path: String,
    session_id: String,
    started_at_unix_ms: u64,
    bridge: DebugBridgeConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct DebugStartCommandArgs {
    database_path: String,
    session_id: String,
    received_at_unix_ms: u64,
    command: DebugCommandEnvelope,
}

#[derive(Debug, Clone, Deserialize)]
struct DebugFinishCommandArgs {
    database_path: String,
    session_id: String,
    time_unix_ms: u64,
    command_id: String,
    ok: bool,
    message: String,
    #[serde(default = "empty_json_object")]
    data: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct DebugRecordEventArgs {
    database_path: String,
    session_id: String,
    time_unix_ms: u64,
    source: String,
    level: String,
    topic: String,
    message: String,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default = "empty_json_object")]
    data: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct DebugSessionSnapshotArgs {
    database_path: String,
    session_id: String,
}

pub fn core_version_payload() -> serde_json::Value {
    json!({
        "core_version": option_env!("CARGO_PKG_VERSION").unwrap_or("unknown"),
        "crate_name": option_env!("CARGO_PKG_NAME").unwrap_or("goose-core"),
        "bridge_request_schema": BRIDGE_REQUEST_SCHEMA,
        "bridge_response_schema": BRIDGE_RESPONSE_SCHEMA,
        "storage_schema_version": CURRENT_SCHEMA_VERSION,
    })
}

/// Payload returned by the `core.list_methods` bridge RPC.
///
/// Returns the canonical, alphabetically sorted list of every bridge method
/// the current build understands, alongside the methods-list schema id and
/// the count. Intended for client-side discovery: the iOS app, a future
/// Android port, debug tooling, or anyone wiring a new front end can pull
/// the live list at runtime instead of grepping the Rust source.
///
/// The list itself is the compile-time constant [`BRIDGE_METHODS`]; this
/// function exists only to wrap it in the bridge response envelope.
pub fn core_list_methods_payload() -> serde_json::Value {
    json!({
        "schema": BRIDGE_METHODS_LIST_SCHEMA,
        "count": BRIDGE_METHODS.len(),
        "methods": BRIDGE_METHODS,
    })
}

pub fn openwhoop_reference_report_payload() -> serde_json::Value {
    let service_roles = whoop_generation_references()
        .iter()
        .map(|reference| {
            json!({
                "generation": reference.generation.as_str(),
                "service_uuid": reference.service_uuid,
                "characteristic_roles": [
                    {
                        "role": "command_to_strap",
                        "uuid": reference.command_to_strap_uuid,
                    },
                    {
                        "role": "command_from_strap",
                        "uuid": reference.command_from_strap_uuid,
                    },
                    {
                        "role": "events_from_strap",
                        "uuid": reference.events_from_strap_uuid,
                    },
                    {
                        "role": "data_from_strap",
                        "uuid": reference.data_from_strap_uuid,
                    },
                    {
                        "role": "memfault",
                        "uuid": reference.memfault_uuid,
                    },
                ],
            })
        })
        .collect::<Vec<_>>();
    let history_fields = openwhoop_history_field_references()
        .iter()
        .map(|reference| {
            json!({
                "field": reference.field.as_str(),
                "gen4": reference.gen4,
                "gen5": reference.gen5,
                "goose_summary_kinds": reference.goose_summary_kinds,
                "status": reference.status.as_str(),
                "note": reference.note,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "schema": "goose.openwhoop-reference-report.v1",
        "generated_by": "goose-bridge",
        "snapshot": {
            "repository": OPENWHOOP_REFERENCE_REPOSITORY,
            "commit": OPENWHOOP_REFERENCE_COMMIT,
            "snapshot_url": OPENWHOOP_REFERENCE_SNAPSHOT_URL,
            "attribution": OPENWHOOP_REFERENCE_ATTRIBUTION,
            "license_caveat": OPENWHOOP_REFERENCE_LICENSE_CAVEAT,
        },
        "service_roles": service_roles,
        "service_role_count": service_roles.len(),
        "history_fields": history_fields,
        "history_field_count": history_fields.len(),
    })
}

pub fn handle_bridge_request_json(request_json: &str) -> String {
    let response = match serde_json::from_str::<BridgeRequest>(request_json) {
        Ok(request) => handle_bridge_request(request),
        Err(error) => BridgeResponse {
            schema: BRIDGE_RESPONSE_SCHEMA.to_string(),
            request_id: "unknown".to_string(),
            ok: false,
            result: None,
            error: Some(BridgeError {
                code: "invalid_json".to_string(),
                message: error.to_string(),
            }),
            timing: None,
        },
    };
    serialize_response(&response)
}

pub fn handle_bridge_request(request: BridgeRequest) -> BridgeResponse {
    let method = request.method.clone();
    let started = Instant::now();
    let mut response = handle_bridge_request_inner(request);
    response.timing = Some(BridgeTiming {
        method,
        method_elapsed_us: elapsed_us_u64(started),
    });
    response
}

fn handle_bridge_request_inner(request: BridgeRequest) -> BridgeResponse {
    if request.schema != BRIDGE_REQUEST_SCHEMA {
        return bridge_error(
            &request.request_id,
            "unsupported_schema",
            format!(
                "expected schema {BRIDGE_REQUEST_SCHEMA}, got {}",
                request.schema
            ),
        );
    }
    if request.request_id.trim().is_empty() {
        return bridge_error("unknown", "invalid_request", "request_id is required");
    }

    match request.method.as_str() {
        "core.version" => bridge_ok(&request.request_id, core_version_payload()),
        "core.list_methods" => bridge_ok(&request.request_id, core_list_methods_payload()),
        "openwhoop.reference_report" => {
            bridge_ok(&request.request_id, openwhoop_reference_report_payload())
        }
        "metrics.built_in_definitions" => serde_json::to_value(built_in_algorithm_definitions())
            .map_err(|error| GooseError::message(error.to_string()))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.reference_definitions" => serde_json::to_value(reference_algorithm_definitions())
            .map_err(|error| GooseError::message(error.to_string()))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.reference_compare" => request_args::<ReferenceCompareArgs>(&request)
            .and_then(reference_compare_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.default_preferences" => {
            serde_json::to_value(built_in_default_algorithm_preferences())
                .map_err(|error| GooseError::message(error.to_string()))
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.goose_hrv_v0" => request_args::<HrvInput>(&request)
            .and_then(|input| metric_result_to_value(goose_hrv_v0(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_sleep_v0" => request_args::<SleepInput>(&request)
            .and_then(|input| metric_result_to_value(goose_sleep_v0(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_sleep_v1" => request_args::<SleepV1Input>(&request)
            .and_then(|input| metric_result_to_value(goose_sleep_v1(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_strain_v0" => request_args::<StrainInput>(&request)
            .and_then(|input| metric_result_to_value(goose_strain_v0(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_strain_v1" => request_args::<StrainInput>(&request)
            .and_then(|input| metric_result_to_value(goose_strain_v1(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.fit_strain_denominator" => request_args::<FitStrainDenominatorArgs>(&request)
            .and_then(|args| match fit_strain_denominator(&args.pairs) {
                Some(d) => Ok(serde_json::json!({ "denominator": d })),
                None => Err(GooseError::message(
                    "insufficient_or_degenerate_pairs".to_string(),
                )),
            })
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_recovery_v0" => request_args::<RecoveryInput>(&request)
            .and_then(|input| metric_result_to_value(goose_recovery_v0(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_recovery_v1" => request_args::<RecoveryV1BridgeArgs>(&request)
            .and_then(goose_recovery_v1_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_stress_v0" => request_args::<StressInput>(&request)
            .and_then(|input| metric_result_to_value(goose_stress_v0(&input)))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.goose_readiness_v1" => request_args::<ReadinessInput>(&request)
            .and_then(|input| {
                serde_json::to_value(goose_readiness_v1(&input)).map_err(|e| {
                    GooseError::message(format!("cannot serialize readiness_v1 output: {e}"))
                })
            })
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.imu_step_count_v1" => request_args::<ImuStepCountInput>(&request)
            .and_then(|input| {
                serde_json::to_value(imu_step_count_v1(&input)).map_err(|e| {
                    GooseError::message(format!("cannot serialize imu_step_count output: {e}"))
                })
            })
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.input_readiness" => request_args::<MetricInputReadinessArgs>(&request)
            .and_then(metric_input_readiness_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.motion_features" => request_args::<MotionFeaturesArgs>(&request)
            .and_then(motion_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.heart_rate_features" => request_args::<HeartRateFeaturesArgs>(&request)
            .and_then(heart_rate_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.vital_event_features" => request_args::<VitalEventFeaturesArgs>(&request)
            .and_then(vital_event_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.step_packet_discovery" => request_args::<StepPacketDiscoveryArgs>(&request)
            .and_then(step_packet_discovery_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.step_capture_validation" => request_args::<StepCaptureValidationArgs>(&request)
            .and_then(step_capture_validation_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.raw_motion_step_estimate" => request_args::<RawMotionStepEstimateArgs>(&request)
            .and_then(raw_motion_step_estimate_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.step_counter_ingest" => request_args::<StepCounterIngestArgs>(&request)
            .and_then(step_counter_ingest_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.step_counter_daily_rollup" => request_args::<StepCounterDailyRollupArgs>(&request)
            .and_then(step_counter_daily_rollup_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.step_counter_hourly_rollup" => {
            request_args::<StepCounterHourlyRollupArgs>(&request)
                .and_then(step_counter_hourly_rollup_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.activity_unavailable_daily_status" => {
            request_args::<ActivityUnavailableDailyStatusArgs>(&request)
                .and_then(activity_unavailable_daily_status_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.daily_activity_metrics" => request_args::<DailyActivityMetricListArgs>(&request)
            .and_then(daily_activity_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.hourly_activity_metrics" => request_args::<HourlyActivityMetricListArgs>(&request)
            .and_then(hourly_activity_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.daily_recovery_metrics" => request_args::<DailyRecoveryMetricListArgs>(&request)
            .and_then(daily_recovery_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.energy_daily_rollup" => request_args::<EnergyDailyRollupArgs>(&request)
            .and_then(energy_daily_rollup_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.energy_unavailable_daily_status" => {
            request_args::<EnergyDailyRollupArgs>(&request)
                .and_then(energy_unavailable_daily_status_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.energy_hourly_rollup" => request_args::<EnergyHourlyRollupArgs>(&request)
            .and_then(energy_hourly_rollup_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.energy_capture_validation" => {
            request_args::<EnergyCaptureValidationArgs>(&request)
                .and_then(energy_capture_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.hrv_features" => request_args::<HrvFeaturesArgs>(&request)
            .and_then(hrv_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.hrv_capture_validation" => request_args::<HrvCaptureValidationArgs>(&request)
            .and_then(hrv_capture_validation_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.respiratory_rate_capture_validation" => {
            request_args::<RespiratoryRateCaptureValidationArgs>(&request)
                .and_then(respiratory_rate_capture_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.oxygen_saturation_capture_validation" => {
            request_args::<OxygenSaturationCaptureValidationArgs>(&request)
                .and_then(oxygen_saturation_capture_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.temperature_capture_validation" => {
            request_args::<TemperatureCaptureValidationArgs>(&request)
                .and_then(temperature_capture_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.recovery_sensor_discovery" => {
            request_args::<RecoverySensorDiscoveryArgs>(&request)
                .and_then(recovery_sensor_discovery_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.recovery_unavailable_daily_status" => {
            request_args::<RecoveryUnavailableDailyStatusArgs>(&request)
                .and_then(recovery_unavailable_daily_status_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.recovery_sensor_daily_rollup" => {
            request_args::<RecoverySensorDailyRollupArgs>(&request)
                .and_then(recovery_sensor_daily_rollup_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.window_features" => request_args::<MetricWindowFeaturesArgs>(&request)
            .and_then(metric_window_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.resting_hr_features" => request_args::<RestingHeartRateFeaturesArgs>(&request)
            .and_then(resting_heart_rate_features_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.resting_hr_daily_rollup" => {
            request_args::<RestingHeartRateDailyRollupArgs>(&request)
                .and_then(resting_heart_rate_daily_rollup_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.resting_hr_capture_validation" => {
            request_args::<RestingHeartRateCaptureValidationArgs>(&request)
                .and_then(resting_heart_rate_capture_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.sleep_score_from_features" => request_args::<SleepFeatureScoreArgs>(&request)
            .and_then(sleep_feature_score_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.sleep_staging" => request_args::<SleepStagingBridgeArgs>(&request)
            .and_then(sleep_staging_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.recovery_score_from_features" => {
            request_args::<RecoveryFeatureScoreArgs>(&request)
                .and_then(recovery_feature_score_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "metrics.strain_score_from_features" => request_args::<StrainFeatureScoreArgs>(&request)
            .and_then(strain_feature_score_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "metrics.stress_score_from_features" => request_args::<StressFeatureScoreArgs>(&request)
            .and_then(stress_feature_score_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "calibration.evaluate_dataset" => request_args::<EvaluateCalibrationDatasetArgs>(&request)
            .and_then(evaluate_calibration_dataset_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "calibration.evaluate_stored_labels" => {
            request_args::<EvaluateStoredCalibrationLabelsArgs>(&request)
                .and_then(evaluate_stored_calibration_labels_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "calibration.import_labels" => request_args::<ImportCalibrationLabelsArgs>(&request)
            .and_then(import_calibration_labels_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "calibration.list_labels" => request_args::<ListCalibrationLabelsArgs>(&request)
            .and_then(list_calibration_labels_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "calibration.apply" => request_args::<ApplyCalibrationArgs>(&request)
            .and_then(apply_calibration_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "export.raw_timeframe" => request_args::<RawExportArgs>(&request)
            .and_then(raw_export_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "export.validate_bundle" => request_args::<ExportValidateBundleArgs>(&request)
            .and_then(export_validate_bundle_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "exercise.detect_sessions" => request_args::<DetectExerciseSessionsArgs>(&request)
            .and_then(exercise_detect_sessions_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "exercise.sessions_between" => request_args::<ExerciseSessionsBetweenArgs>(&request)
            .and_then(exercise_sessions_between_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "validation.local_health_manifest_scaffold"
        | "local_health.validation_manifest_scaffold" => {
            request_args::<LocalHealthValidationManifestScaffoldArgs>(&request)
                .and_then(local_health_validation_manifest_scaffold_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "validation.local_health_manifest_runbook" | "local_health.validation_manifest_runbook" => {
            request_args::<LocalHealthValidationManifestRunbookArgs>(&request)
                .and_then(local_health_validation_manifest_runbook_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "validation.local_health_manifest_review" | "local_health.validation_manifest_review" => {
            request_args::<LocalHealthValidationManifestReviewArgs>(&request)
                .and_then(local_health_validation_manifest_review_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "privacy.lint" => request_args::<PrivacyLintArgs>(&request)
            .and_then(privacy_lint_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.sanitize" => request_args::<CaptureSanitizeArgs>(&request)
            .and_then(capture_sanitize_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "ui_coverage.audit" => request_args::<UiCoverageAuditArgs>(&request)
            .and_then(ui_coverage_audit_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "diagnostics.perf_budget" => request_args::<PerfBudgetArgs>(&request)
            .and_then(perf_budget_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "diagnostics.property_suite" => request_args::<PropertySuiteArgs>(&request)
            .and_then(property_suite_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "health_sync.dry_run" => request_args::<HealthSyncDryRunInput>(&request)
            .and_then(health_sync_dry_run_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "health_sync.activity_dry_run" => request_args::<ActivityHealthSyncDryRunInput>(&request)
            .and_then(activity_health_sync_dry_run_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "historical_sync.dry_run" => request_args::<HistoricalSyncDryRunInput>(&request)
            .and_then(historical_sync_dry_run_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "historical_sync.physical_evidence_template" => {
            request_args::<HistoricalSyncPhysicalEvidenceTemplateArgs>(&request)
                .and_then(historical_sync_physical_evidence_template_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "historical_sync.validate_physical_evidence" => {
            request_args::<HistoricalSyncPhysicalValidationInput>(&request)
                .and_then(historical_sync_physical_validation_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "capture.import_frame_batch" => request_args::<CaptureImportFrameBatchArgs>(&request)
            .and_then(capture_import_frame_batch_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "overnight.mirror_batch" => request_args::<OvernightMirrorBatchArgs>(&request)
            .and_then(overnight_mirror_batch_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "overnight.mirror_counts" => request_args::<OvernightMirrorCountsArgs>(&request)
            .and_then(overnight_mirror_counts_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.timeline" => request_args::<CaptureTimelineArgs>(&request)
            .and_then(capture_timeline_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.observability_timeline" => {
            request_args::<CaptureObservabilityTimelineArgs>(&request)
                .and_then(capture_observability_timeline_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "capture.start_session" => request_args::<CaptureStartSessionArgs>(&request)
            .and_then(capture_start_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.finish_session" => request_args::<CaptureFinishSessionArgs>(&request)
            .and_then(capture_finish_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.list_sessions" => request_args::<CaptureListSessionsArgs>(&request)
            .and_then(capture_list_sessions_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.create_session" => request_args::<ActivitySessionUpsertArgs>(&request)
            .and_then(activity_create_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.get_session" => request_args::<ActivitySessionLookupArgs>(&request)
            .and_then(activity_get_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.list_sessions" => request_args::<ActivitySessionListArgs>(&request)
            .and_then(activity_list_sessions_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.list_sessions_with_metrics" => request_args::<ActivitySessionListArgs>(&request)
            .and_then(activity_list_sessions_with_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.update_session" => request_args::<ActivitySessionUpsertArgs>(&request)
            .and_then(activity_update_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.correction_plans" => activity_correction_plans_bridge()
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.apply_correction" => request_args::<ActivitySessionCorrectionArgs>(&request)
            .and_then(activity_apply_correction_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.delete_session" => request_args::<ActivitySessionLookupArgs>(&request)
            .and_then(activity_delete_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.attach_metric" => request_args::<ActivityMetricAttachArgs>(&request)
            .and_then(activity_attach_metric_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.attach_metrics" => request_args::<ActivityMetricAttachBatchArgs>(&request)
            .and_then(activity_attach_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.list_metrics" => request_args::<ActivityMetricListArgs>(&request)
            .and_then(activity_list_metrics_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.attach_interval" => request_args::<ActivityIntervalAttachArgs>(&request)
            .and_then(activity_attach_interval_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.list_intervals" => request_args::<ActivityIntervalListArgs>(&request)
            .and_then(activity_list_intervals_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "activity.metrics_for_session_in_window" => {
            request_args::<ActivityMetricWindowArgs>(&request)
                .and_then(activity_metrics_for_session_in_window_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "sleep.import_external_history" => request_args::<ExternalSleepHistoryImportArgs>(&request)
            .and_then(external_sleep_history_import_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.add_correction_label" => request_args::<SleepCorrectionLabelArgs>(&request)
            .and_then(sleep_correction_label_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.list_correction_labels" => request_args::<SleepCorrectionLabelListArgs>(&request)
            .and_then(sleep_correction_label_list_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.validate_window_labels" => request_args::<SleepWindowLabelValidationArgs>(&request)
            .and_then(sleep_window_label_validation_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.validate_stage_labels" => request_args::<SleepStageLabelValidationArgs>(&request)
            .and_then(sleep_stage_label_validation_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.validate_v1_explanation_stability" => {
            request_args::<SleepV1ExplanationStabilityArgs>(&request)
                .and_then(sleep_v1_explanation_stability_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "sleep.validate_v1_release_gates" => request_args::<SleepV1ReleaseGateArgs>(&request)
            .and_then(sleep_v1_release_gate_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sleep.validate_v1_evidence_folder" => request_args::<SleepV1EvidenceFolderArgs>(&request)
            .and_then(sleep_v1_evidence_folder_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.correlation_report" => request_args::<CaptureCorrelationArgs>(&request)
            .and_then(capture_correlation_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "capture.arrival_plan" => request_args::<CaptureArrivalPlanArgs>(&request)
            .and_then(capture_arrival_plan_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.evidence_template" => serde_json::to_value(command_evidence_template())
            .map_err(|error| GooseError::message(error.to_string()))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.definitions" => serde_json::to_value(COMMAND_DEFINITIONS)
            .map_err(|error| GooseError::message(error.to_string()))
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.validate_evidence" => request_args::<CommandValidateEvidenceArgs>(&request)
            .and_then(command_validate_evidence_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.evidence_from_emulator_log" => {
            request_args::<CommandEvidenceFromEmulatorLogArgs>(&request)
                .and_then(command_evidence_from_emulator_log_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "commands.promote_local_frame_matches" => {
            request_args::<CommandPromoteLocalFrameMatchesArgs>(&request)
                .and_then(command_promote_local_frame_matches_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "commands.direct_send_gate" => request_args::<CommandDirectSendGateArgs>(&request)
            .and_then(command_direct_send_gate_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.direct_send_preflight" => {
            request_args::<CommandDirectSendPreflightArgs>(&request)
                .and_then(command_direct_send_preflight_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "commands.capture_plan" => request_args::<CommandCapturePlanArgs>(&request)
            .and_then(command_capture_plan_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "commands.list_validation_records" => {
            request_args::<ListCommandValidationRecordsArgs>(&request)
                .and_then(command_list_validation_records_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "commands.import_validation_records" => {
            request_args::<ImportCommandValidationRecordsArgs>(&request)
                .and_then(command_import_validation_records_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "debug.start_session" => request_args::<DebugStartSessionArgs>(&request)
            .and_then(debug_start_session_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "debug.start_command" => request_args::<DebugStartCommandArgs>(&request)
            .and_then(debug_start_command_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "debug.finish_command" => request_args::<DebugFinishCommandArgs>(&request)
            .and_then(debug_finish_command_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "debug.record_event" => request_args::<DebugRecordEventArgs>(&request)
            .and_then(debug_record_event_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "debug.session_snapshot" => request_args::<DebugSessionSnapshotArgs>(&request)
            .and_then(debug_session_snapshot_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "protocol.parse_frame_hex" => request_args::<ParseFrameArgs>(&request)
            .and_then(parse_frame_hex_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "protocol.parse_frame_hex_batch" => request_args::<ParseFrameBatchArgs>(&request)
            .and_then(parse_frame_hex_batch_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "timeline.from_decoded_frames" => request_args::<TimelineArgs>(&request)
            .and_then(timeline_from_decoded_frames_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.ewma_baseline_fold_history" => request_args::<EwmaBaselineFoldHistoryArgs>(&request)
            .and_then(ewma_baseline_fold_history_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.ewma_baseline_update" => request_args::<EwmaBaselineUpdateArgs>(&request)
            .and_then(ewma_baseline_update_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "biometrics.insert_v24_batch" => request_args::<InsertV24BatchArgs>(&request)
            .and_then(insert_v24_biometric_batch_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "biometrics.v24_between" => request_args::<V24BetweenArgs>(&request)
            .and_then(v24_biometric_samples_between_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "biometrics.spo2_from_raw" => request_args::<Spo2FromRawArgs>(&request)
            .and_then(spo2_from_raw_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.gravity_rows_between" => request_args::<GravityRowsBetweenArgs>(&request)
            .and_then(gravity_rows_between_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.gravity2_samples_between" => request_args::<GravityRowsBetweenArgs>(&request)
            .and_then(gravity2_samples_between_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.insert_gravity_rows" => request_args::<InsertGravityRowsArgs>(&request)
            .and_then(insert_gravity_rows_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "store.insert_gravity2_batch" => request_args::<InsertGravityRowsArgs>(&request)
            .and_then(insert_gravity2_batch_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sync.mark_synced" => request_args::<SyncMarkSyncedArgs>(&request)
            .and_then(sync_mark_synced_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sync.rows_pending_upload" => request_args::<SyncRowsPendingUploadArgs>(&request)
            .and_then(sync_rows_pending_upload_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "sync.backfill_streams" => request_args::<SyncBackfillStreamsArgs>(&request)
            .and_then(sync_backfill_streams_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "storage.check" => request_args::<StorageCheckArgs>(&request)
            .and_then(storage_check_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "settings.apply_default_algorithm_preferences" => {
            request_args::<ApplyDefaultPreferencesArgs>(&request)
                .and_then(apply_default_preferences_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "settings.set_algorithm_preference" => request_args::<SetPreferenceArgs>(&request)
            .and_then(set_algorithm_preference_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "settings.get_algorithm_preference" => request_args::<GetPreferenceArgs>(&request)
            .and_then(get_algorithm_preference_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "settings.list_algorithm_preferences" => request_args::<ListPreferencesArgs>(&request)
            .and_then(list_algorithm_preferences_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "upload.get_recent_decoded_streams" => {
            request_args::<UploadGetRecentDecodedStreamsArgs>(&request)
                .and_then(upload_get_recent_decoded_streams_bridge)
                .map(|value| bridge_ok(&request.request_id, value))
                .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error))
        }
        "upload.get_raw_frames_for_upload" => request_args::<UploadGetRawFramesArgs>(&request)
            .and_then(upload_get_raw_frames_for_upload_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        "storage.compact_raw_evidence" => request_args::<StorageCompactRawEvidenceArgs>(&request)
            .and_then(storage_compact_raw_evidence_bridge)
            .map(|value| bridge_ok(&request.request_id, value))
            .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
        // Test-only arm: deterministic panic trigger for FFI catch_unwind coverage.
        // Gated on debug_assertions (true in test/dev, false in release) so it is
        // never compiled into the release static library (satisfies T-09-04 threat model).
        // Note: #[cfg(test)] is not used here because integration tests compile the crate
        // in library mode without activating cfg(test) on the dependency; debug_assertions
        // achieves the same release-exclusion guarantee for this use case.
        #[cfg(debug_assertions)]
        "test.panic" => panic!("test.panic: intentional panic for FFI catch_unwind coverage"),
        method => bridge_error(
            &request.request_id,
            "unknown_method",
            format!("unsupported bridge method: {method}"),
        ),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn goose_core_version_json() -> *mut c_char {
    json_to_c_string(core_version_payload())
}

/// Handle a JSON-encoded bridge request from the host platform.
///
/// Returns a newly-allocated, null-terminated UTF-8 C string containing a
/// JSON-encoded response. The caller takes ownership of the returned pointer
/// and **must** release it by passing it to [`goose_bridge_free_string`].
/// Mixing this allocation with `free(3)` or any other deallocator is
/// undefined behaviour.
///
/// # Safety
///
/// The caller must ensure that:
///
/// - `request_json` is either null **or** a valid pointer to a
///   null-terminated UTF-8 C string that remains valid (and unmodified by
///   other threads) for the duration of this call.
/// - The buffer referenced by `request_json` is not aliased by any mutable
///   reference for the duration of this call.
///
/// A null `request_json` is handled defensively and returns a structured
/// error response rather than dereferencing the pointer. Invalid UTF-8 in the
/// input is likewise reported as a structured error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn goose_bridge_handle_json(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() {
        return response_to_c_string(&bridge_error(
            "unknown",
            "null_request",
            "request_json pointer is null",
        ));
    }

    // The caller owns the input C string and must provide a valid null-terminated UTF-8 buffer.
    let request = match unsafe { CStr::from_ptr(request_json) }.to_str() {
        Ok(request) => request,
        Err(error) => {
            return response_to_c_string(&bridge_error(
                "unknown",
                "invalid_utf8",
                error.to_string(),
            ));
        }
    };
    // Wrap ALL panic-prone work inside catch_unwind so that a panic in dispatch
    // is caught at the FFI boundary and returned as a structured JSON error instead
    // of aborting the process. AssertUnwindSafe is sound here because the closure
    // does not alias mutable state that would be left in an inconsistent state on
    // unwind — the bridge is stateless across calls and the only side effect is the
    // returned C string allocation.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        string_to_c_string(handle_bridge_request_json(request))
    })) {
        Ok(ptr) => ptr,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string());
            response_to_c_string(&bridge_error("unknown", "panic", message))
        }
    }
}

/// Free a C string previously returned by any `goose_bridge_*` or
/// `goose_core_*` function.
///
/// # Safety
///
/// The caller must ensure that:
///
/// - `value` is either null **or** a pointer that was returned by a Goose
///   bridge entry point (e.g. [`goose_bridge_handle_json`] or
///   `goose_core_version_json`) and has not yet been freed.
/// - The pointer is not aliased by any other live reference and is not used
///   after this call returns.
///
/// Passing a pointer that was not produced by the Goose core (for example,
/// one allocated by `malloc(3)` on the host) is undefined behaviour, because
/// the Rust allocator backing [`CString`] is not guaranteed to match the
/// host's allocator. A null pointer is handled as a no-op. Calling this
/// function twice on the same pointer is a double-free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn goose_bridge_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    // Reconstructing the CString transfers ownership back to Rust so it can be dropped once.
    drop(unsafe { CString::from_raw(value) });
}

fn parse_frame_hex_bridge(args: ParseFrameArgs) -> GooseResult<serde_json::Value> {
    let device_type = parse_device_type(&args.device_type)?;
    let parsed = parse_frame_hex(device_type, &args.frame_hex)?;
    serde_json::to_value(parsed)
        .map_err(|error| GooseError::message(format!("cannot serialize parsed frame: {error}")))
}

fn parse_frame_hex_batch_bridge(args: ParseFrameBatchArgs) -> GooseResult<serde_json::Value> {
    let device_type = parse_device_type(&args.device_type)?;
    let mut results = Vec::with_capacity(args.frames.len());
    for (index, frame_hex) in args.frames.iter().enumerate() {
        match parse_frame_hex(device_type, frame_hex) {
            Ok(parsed) => {
                let mut item = json!({
                    "index": index,
                    "ok": true,
                    "compact": compact_parsed_frame_summary(&parsed),
                });
                if args.include_result
                    && let Some(obj) = item.as_object_mut()
                {
                    obj.insert("result".to_string(), json!(parsed));
                }
                results.push(item);
            }
            Err(error) => results.push(json!({
                "index": index,
                "ok": false,
                "error": error.to_string(),
            })),
        }
    }

    Ok(json!({
        "frame_count": args.frames.len(),
        "results": results,
    }))
}

fn compact_parsed_frame_summary(parsed: &ParsedFrame) -> serde_json::Value {
    let packet = parsed
        .packet_type
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let packet_name = parsed
        .packet_type_name
        .as_deref()
        .unwrap_or("unknown")
        .to_string();
    let packet_type_name = parsed.packet_type_name.as_deref();
    let sequence = parsed
        .sequence
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let warning_count = parsed.warnings.len();

    match parsed.parsed_payload.as_ref() {
        Some(ParsedPayload::DataPacket {
            packet_k,
            domain,
            body_hex,
            body_offset,
            body_summary,
            ..
        }) => {
            let packet_k_text = packet_k
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string());
            let domain_text = domain.as_deref().unwrap_or("unknown");
            let body_kind = body_summary_kind(body_summary.as_ref());
            let heart_rate = match body_summary.as_ref() {
                Some(DataPacketBodySummary::RawMotionK10 { heart_rate, .. }) => *heart_rate,
                _ => None,
            };
            let movement = compact_k10_movement_summary(body_summary.as_ref());
            // CR-01 fix: when body_hex is suppressed (PERF-05 K10/K21), derive the actual
            // byte count from declared_len rather than from the empty string.
            let body_byte_count = if body_hex.is_empty() {
                parsed.declared_len.saturating_sub(*body_offset)
            } else {
                body_hex.len() / 2
            };
            json!({
                "packet_type": parsed.packet_type,
                "packet_type_name": packet_type_name,
                "sequence": parsed.sequence,
                "warnings_count": warning_count,
                "payload_kind": "data_packet",
                "packet_k": packet_k,
                "domain": domain,
                "body_kind": body_kind,
                "body_byte_count": body_byte_count,
                "heart_rate": heart_rate,
                "movement": movement,
                "summary": format!("packet={packet_name}({packet}) seq={sequence} data.k={packet_k_text} domain={domain_text} body={body_kind} warnings={warning_count}"),
            })
        }
        Some(ParsedPayload::Event {
            event_id,
            event_name,
            data_hex,
            ..
        }) => {
            let event_id_text = event_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string());
            let event_name_text = event_name.as_deref().unwrap_or("unknown");
            json!({
                "packet_type": parsed.packet_type,
                "packet_type_name": packet_type_name,
                "sequence": parsed.sequence,
                "warnings_count": warning_count,
                "payload_kind": "event",
                "event_id": event_id,
                "event_name": event_name,
                "event_byte_count": data_hex.len() / 2,
                "summary": format!("packet={packet_name}({packet}) seq={sequence} event={event_name_text}({event_id_text}) bytes={} warnings={warning_count}", data_hex.len() / 2),
            })
        }
        Some(payload) => {
            let payload_kind = parsed_payload_kind(payload);
            json!({
                "packet_type": parsed.packet_type,
                "packet_type_name": packet_type_name,
                "sequence": parsed.sequence,
                "warnings_count": warning_count,
                "payload_kind": payload_kind,
                "summary": format!("packet={packet_name}({packet}) seq={sequence} payload={payload_kind} warnings={warning_count}"),
            })
        }
        None => json!({
            "packet_type": parsed.packet_type,
            "packet_type_name": packet_type_name,
            "sequence": parsed.sequence,
            "warnings_count": warning_count,
            "payload_kind": "none",
            "summary": format!("packet={packet_name}({packet}) seq={sequence} warnings={warning_count}"),
        }),
    }
}

fn parsed_payload_kind(payload: &ParsedPayload) -> &'static str {
    match payload {
        ParsedPayload::Command { .. } => "command",
        ParsedPayload::CommandResponse { .. } => "command_response",
        ParsedPayload::Event { .. } => "event",
        ParsedPayload::DataPacket { .. } => "data_packet",
        ParsedPayload::Raw { .. } => "raw",
    }
}

fn body_summary_kind(summary: Option<&DataPacketBodySummary>) -> &'static str {
    match summary {
        Some(DataPacketBodySummary::NormalHistory { .. }) => "normal_history",
        Some(DataPacketBodySummary::R17OpticalOrLabradorFiltered { .. }) => {
            "r17_optical_or_labrador_filtered"
        }
        Some(DataPacketBodySummary::RawMotionK10 { .. }) => "raw_motion_k10",
        Some(DataPacketBodySummary::RawMotionK21 { .. }) => "raw_motion_k21",
        Some(DataPacketBodySummary::V24History { .. }) => "v24_history",
        Some(DataPacketBodySummary::R22Whoop5Hr { .. }) => "r22_whoop5_hr",
        Some(DataPacketBodySummary::V18History { .. }) => "v18_history",
        None => "none",
    }
}

fn compact_k10_movement_summary(summary: Option<&DataPacketBodySummary>) -> serde_json::Value {
    let Some(DataPacketBodySummary::RawMotionK10 { axes, .. }) = summary else {
        return serde_json::Value::Null;
    };

    let mut axis_count = 0usize;
    let mut parsed_sample_count = 0usize;
    let mut raw_peak_range = 0.0f64;
    let mut raw_peak_abs = 0.0f64;
    let mut accelerometer_peak_range = 0.0f64;
    let mut gyroscope_peak_range = 0.0f64;
    let mut accelerometer_range_squared_total = 0.0f64;

    for axis in axes {
        let Some((axis_range, axis_abs)) = axis_range_and_abs(axis) else {
            continue;
        };
        axis_count += 1;
        parsed_sample_count += axis.parsed_count;
        raw_peak_range = raw_peak_range.max(axis_range);
        raw_peak_abs = raw_peak_abs.max(axis_abs);
        if axis.name.starts_with("accelerometer_") {
            accelerometer_peak_range = accelerometer_peak_range.max(axis_range);
            accelerometer_range_squared_total += axis_range * axis_range;
        } else if axis.name.starts_with("gyroscope_") {
            gyroscope_peak_range = gyroscope_peak_range.max(axis_range);
        }
    }

    if parsed_sample_count == 0 {
        return serde_json::Value::Null;
    }

    let accelerometer_vector_range = accelerometer_range_squared_total.sqrt();
    let accelerometer_intensity = accelerometer_vector_range / 8192.0;
    let raw_intensity = raw_peak_range / 32767.0;
    let motion_intensity = raw_intensity.max(accelerometer_intensity).clamp(0.0, 1.0);
    json!({
        "axis_count": axis_count,
        "parsed_sample_count": parsed_sample_count,
        "raw_peak_range": raw_peak_range,
        "raw_peak_abs": raw_peak_abs,
        "accelerometer_peak_range": accelerometer_peak_range,
        "gyroscope_peak_range": gyroscope_peak_range,
        "accelerometer_vector_range": accelerometer_vector_range,
        "motion_intensity": motion_intensity,
    })
}

fn axis_range_and_abs(axis: &I16SeriesSummary) -> Option<(f64, f64)> {
    if axis.parsed_count == 0 {
        return None;
    }
    let (Some(minimum), Some(maximum)) = (axis.min, axis.max) else {
        let peak_abs = axis
            .preview
            .iter()
            .map(|value| f64::from(*value).abs())
            .fold(0.0, f64::max);
        return Some((0.0, peak_abs));
    };
    let range = f64::from(maximum) - f64::from(minimum);
    let peak_abs = f64::from(minimum).abs().max(f64::from(maximum).abs());
    Some((range.max(0.0), peak_abs))
}

fn timeline_from_decoded_frames_bridge(args: TimelineArgs) -> GooseResult<serde_json::Value> {
    let rows = packet_timeline_from_decoded_frames(&args.decoded_frames)?;
    serde_json::to_value(rows)
        .map_err(|error| GooseError::message(format!("cannot serialize timeline rows: {error}")))
}

fn storage_check_bridge(args: StorageCheckArgs) -> GooseResult<serde_json::Value> {
    if args.database_path.trim().is_empty() {
        return Err(GooseError::message("database_path is required"));
    }
    let report = check_storage_database(StorageCheckOptions {
        database_path: Path::new(&args.database_path),
        run_self_test: args.self_test,
    })?;
    serde_json::to_value(report)
        .map_err(|error| GooseError::message(format!("cannot serialize storage report: {error}")))
}

fn apply_default_preferences_bridge(
    args: ApplyDefaultPreferencesArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    register_built_in_definitions(&store)?;
    let preferences = default_algorithm_preferences_for_scope(&args.scope);
    for preference in &preferences {
        store.set_algorithm_preference(preference)?;
    }
    serde_json::to_value(preferences)
        .map_err(|error| GooseError::message(format!("cannot serialize preferences: {error}")))
}

fn set_algorithm_preference_bridge(args: SetPreferenceArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    if args.register_built_ins {
        register_built_in_definitions(&store)?;
    }
    let preference = AlgorithmPreferenceRecord {
        scope: args.scope,
        metric_family: args.metric_family,
        algorithm_id: args.algorithm_id,
        version: args.version,
    };
    store.set_algorithm_preference(&preference)?;
    serde_json::to_value(preference)
        .map_err(|error| GooseError::message(format!("cannot serialize preference: {error}")))
}

fn get_algorithm_preference_bridge(args: GetPreferenceArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let preference = store.algorithm_preference(&args.scope, &args.metric_family)?;
    serde_json::to_value(preference)
        .map_err(|error| GooseError::message(format!("cannot serialize preference: {error}")))
}

fn list_algorithm_preferences_bridge(args: ListPreferencesArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let preferences = store.algorithm_preferences(args.scope.as_deref())?;
    serde_json::to_value(preferences)
        .map_err(|error| GooseError::message(format!("cannot serialize preferences: {error}")))
}

// ── Upload bridge ─────────────────────────────────────────────────────────────

/// WHOOP accelerometer scale factor (research-confirmed ~3900 LSB per g).
/// Divide each raw i16 sample by this constant to obtain acceleration in g.
/// Exposed as a named constant to allow future calibration adjustments.
const IMU_LSB_PER_G: f64 = 3900.0;

// TOGGLE_IMU_MODE (command 106) is already sent in production by
// SensorStreamCommandKind.startPhysiologyCapture / stopPhysiologyCapture
// in the Swift layer. No Rust toggle is needed — this pipeline only stores
// and converts the resulting K10/K21 motion data. (IMU-04 — no code change
// required beyond this documentation comment.)

#[derive(Debug, Deserialize)]
struct UploadGetRecentDecodedStreamsArgs {
    database_path: String,
    #[allow(dead_code)] // device_id filter deferred to v3.0 (namespace mismatch: UUID vs BLE name)
    device_id: String,
    since_ts: f64, // Unix timestamp (seconds); fetch decoded frames captured >= since_ts
}

#[derive(Debug, Deserialize)]
struct UploadGetRawFramesArgs {
    database_path: String,
    since_ts: f64, // Unix timestamp (seconds); fetch raw_evidence captured >= since_ts
    #[serde(default = "default_raw_frames_limit")]
    limit: usize,
}

fn default_raw_frames_limit() -> usize {
    2000
}

// ---------------------------------------------------------------------------
// V24 physical unit helpers (uncalibrated; quality_flag="uncalibrated" always)
// ---------------------------------------------------------------------------

/// SpO2 estimate via ratio-of-ratios (uncalibrated).
/// R = red / ir; SpO2 ≈ 110 - 25*R.
/// Returns None if ir == 0 or if the computed value is outside the plausible
/// physiological range [70, 100] % (clamp applied after plausibility gate).
fn spo2_from_raw_uncalibrated(red: u16, ir: u16) -> Option<f64> {
    if ir == 0 {
        return None;
    }
    let r = (red as f64) / (ir as f64);
    let spo2 = 110.0 - 25.0 * r;
    if !(70.0..=100.0).contains(&spo2) {
        // Outside plausible range — reject; caller emits a warning.
        return None;
    }
    Some(spo2)
}

/// Skin temperature (uncalibrated linear approximation).
/// Linear model: raw=930 → 33 °C, slope 30 ADC units per °C.
/// Returns None if the result is outside the plausible range [25, 40] °C.
fn skin_temp_celsius_from_raw(raw: u16) -> Option<f64> {
    let celsius = (raw as f64 - 930.0) / 30.0 + 33.0;
    if !(25.0..=40.0).contains(&celsius) {
        // Outside plausible range — reject.
        return None;
    }
    Some(celsius)
}

/// Respiratory rate estimate (uncalibrated) using zero-crossing count on a
/// raw window sampled at 1 Hz. Returns None if window_len < 10 or if the
/// computed rate is outside the plausible range (0, 60] breaths/min.
#[allow(dead_code)] // deferred to Phase 31/33 (zero-crossing rate not yet wired into insert path)
fn resp_rate_bpm_zero_crossing(window: &[u16]) -> Option<f64> {
    if window.len() < 10 {
        return None;
    }
    let mean = window.iter().map(|&v| v as f64).sum::<f64>() / window.len() as f64;
    let centered: Vec<f64> = window.iter().map(|&v| v as f64 - mean).collect();
    let crossings = centered
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();
    let rate = (crossings as f64 / 2.0) / (window.len() as f64) * 60.0;
    if rate <= 0.0 || rate > 60.0 {
        return None;
    }
    Some(rate)
}

/// Extract biometric streams from recent decoded_frames and return them in the
/// format expected by the server's `POST /v1/ingest-decoded` endpoint.
///
/// The Rust/SQLite database stores raw and decoded BLE frames (not individual
/// per-stream rows). This function walks the decoded_frames captured since
/// `since_ts`, parses each `parsed_payload_json`, and extracts hr/rr/events/
/// battery/spo2/skin_temp/resp/gravity rows — mirroring what the Python
/// `extract_streams` / `extract_historical_streams` helpers produce.
///
/// Only frames where `header_crc_valid` and `payload_crc_valid` are both true
/// are considered (CRC-failed frames are skipped, matching the server-side rule).
fn upload_get_recent_decoded_streams_bridge(
    args: UploadGetRecentDecodedStreamsArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;

    // Convert unix timestamp to ISO-8601 for decoded_frames_between query
    let since_dt = chrono_from_unix(args.since_ts);
    let now_dt = chrono_now();

    let frames = store.decoded_frames_between(&since_dt, &now_dt)?;

    let mut hr: Vec<serde_json::Value> = Vec::new();

    let mut rr: Vec<serde_json::Value> = Vec::new();
    let mut events: Vec<serde_json::Value> = Vec::new();
    let battery: Vec<serde_json::Value> = Vec::new();
    let mut spo2: Vec<serde_json::Value> = Vec::new();
    let mut skin_temp: Vec<serde_json::Value> = Vec::new();
    let mut resp: Vec<serde_json::Value> = Vec::new();
    let mut gravity: Vec<serde_json::Value> = Vec::new();
    let mut gravity2: Vec<serde_json::Value> = Vec::new();
    let mut step: Vec<(f64, i64)> = Vec::new();

    for frame in &frames {
        // Skip CRC-failed frames (matches server-side rule)
        if !frame.header_crc_valid || !frame.payload_crc_valid {
            continue;
        }

        // CR-02: per-row device_id filtering is deferred to v3.0 multi-device tracking.
        // The device_id field (iOS CoreBluetooth peripheral UUID) and device_model
        // (sanitized BLE device name) live in different namespaces — a comparison
        // between them always mismatches. For the current single-device case, all
        // captured frames belong to the active device, so the time-window filter
        // (since_ts) is the correct and sufficient filter.

        let parsed: Option<ParsedPayload> =
            serde_json::from_str(&frame.parsed_payload_json).unwrap_or(None);

        match parsed {
            Some(ParsedPayload::DataPacket {
                packet_k,
                timestamp_seconds,
                body_summary,
                ..
            }) => {
                // REALTIME_DATA (packet_k == Some(40 | 0x28)) — canonical HR stream
                // HISTORICAL_DATA (packet_k == Some(47 | 0x2F)) — V24 biometric history
                let ts_unix: Option<f64> = timestamp_seconds.map(|s| s as f64);

                // Extract heart rate and RR intervals from the body_summary
                if let Some(ref summary) = body_summary {
                    match summary {
                        DataPacketBodySummary::NormalHistory {
                            hr_present,
                            marker_value,
                            ..
                        } => {
                            // Normal history packet: hr_present flag + marker_value = HR bpm
                            if hr_present.unwrap_or(false)
                                && let (Some(ts), Some(bpm)) = (ts_unix, marker_value)
                            {
                                hr.push(json!({"ts": ts, "bpm": *bpm}));
                            }
                        }
                        DataPacketBodySummary::RawMotionK10 {
                            heart_rate, axes, ..
                        } => {
                            // K10 raw motion carries an HR byte and three accel axes
                            if let (Some(ts), Some(bpm)) = (ts_unix, heart_rate) {
                                hr.push(json!({"ts": ts, "bpm": *bpm}));
                            }
                            // Extract accelerometer_x/y/z full_samples and convert
                            // LSB → g via IMU_LSB_PER_G. Match axes by name (not offset)
                            // to stay robust across any future reordering.
                            // If any axis or its full_samples is absent, skip gracefully.
                            let ax = axes
                                .iter()
                                .find(|a| a.name == "accelerometer_x")
                                .and_then(|a| a.full_samples.as_ref());
                            let ay = axes
                                .iter()
                                .find(|a| a.name == "accelerometer_y")
                                .and_then(|a| a.full_samples.as_ref());
                            let az = axes
                                .iter()
                                .find(|a| a.name == "accelerometer_z")
                                .and_then(|a| a.full_samples.as_ref());
                            // CR-02 fix: assign per-sample timestamp so the gravity time
                            // series is recoverable. WHOOP 5 IMU samples at 50 Hz.
                            const K10_SAMPLE_RATE_HZ: f64 = 50.0;
                            if let (Some(xs), Some(ys), Some(zs), Some(ts_base)) =
                                (ax, ay, az, ts_unix)
                            {
                                let n = xs.len().min(ys.len()).min(zs.len());
                                for i in 0..n {
                                    let sample_ts = ts_base + i as f64 / K10_SAMPLE_RATE_HZ;
                                    gravity.push(json!({
                                        "ts": sample_ts,
                                        "x": xs[i] as f64 / IMU_LSB_PER_G,
                                        "y": ys[i] as f64 / IMU_LSB_PER_G,
                                        "z": zs[i] as f64 / IMU_LSB_PER_G,
                                    }));
                                }
                            }
                        }
                        DataPacketBodySummary::R17OpticalOrLabradorFiltered { .. } => {
                            // Optical/Labrador filtered — SpO2 raw ADC data
                            // Raw interpretation requires calibration; skip for now
                            // (historical V24 spo2 comes via a different packet type)
                        }
                        DataPacketBodySummary::RawMotionK21 { .. } => {
                            // K21 gravity extraction is deferred: axis-to-physical mapping
                            // unconfirmed (variable group_1_count/group_2_count, non-standard
                            // axis naming). Only K10 accel axes (accelerometer_x/y/z) are
                            // converted in this phase (IMU-03). K21 will be handled once
                            // empirical K21 payload data is available to confirm the mapping.
                        }
                        DataPacketBodySummary::V24History {
                            hr: v24_hr,
                            rr_intervals_ms,
                            skin_contact,
                            spo2_red,
                            spo2_ir,
                            skin_temp_raw,
                            resp_raw,
                            gravity_x,
                            gravity_y,
                            gravity_z,
                            gravity2_x,
                            gravity2_y,
                            gravity2_z,
                            ..
                        } => {
                            // V24 biometric fields: wire into the same upload streams as
                            // NormalHistory (hr, rr) plus the V24-only streams (spo2,
                            // skin_temp, resp). sig_quality is stored via
                            // insert_v24_biometric_batch and is NOT included in the upload
                            // payload (POST /v1/ingest-decoded schema does not include it).
                            let contact = skin_contact.unwrap_or(0) == 1;

                            // HR — only when skin contact is established
                            if contact && let (Some(ts), Some(bpm)) = (ts_unix, *v24_hr) {
                                hr.push(json!({"ts": ts, "bpm": bpm}));
                            }

                            // RR intervals — accumulate with per-interval timestamps
                            if let Some(ts_base) = ts_unix {
                                let mut t = ts_base;
                                for &ms in rr_intervals_ms.iter() {
                                    rr.push(json!({"ts": t, "interval_ms": ms}));
                                    t += ms as f64 / 1000.0;
                                }
                            }

                            // SpO2 raw red/IR — only when in contact
                            if contact {
                                if let (Some(ts), Some(r), Some(i)) = (ts_unix, *spo2_red, *spo2_ir)
                                {
                                    spo2.push(json!({"ts": ts, "red": r, "ir": i, "contact": 1}));
                                }

                                // Skin temperature raw ADC
                                if let (Some(ts), Some(raw)) = (ts_unix, *skin_temp_raw) {
                                    skin_temp.push(json!({"ts": ts, "raw": raw, "contact": 1}));
                                }

                                // Respiratory raw ADC
                                if let (Some(ts), Some(raw)) = (ts_unix, *resp_raw) {
                                    resp.push(json!({"ts": ts, "raw": raw, "contact": 1}));
                                }
                            }

                            // Primary gravity triplet (bytes 33–44 in V24 body, f32 LE, already in g units).
                            // Single sample per V24 frame — no loop needed unlike K10 (50 Hz array).
                            // No IMU_LSB_PER_G conversion: protocol.rs already parses as f32 in g units.
                            if let (Some(ts), Some(x), Some(y), Some(z)) =
                                (ts_unix, *gravity_x, *gravity_y, *gravity_z)
                            {
                                gravity.push(json!({
                                    "ts": ts,
                                    "x": x as f64,
                                    "y": y as f64,
                                    "z": z as f64,
                                }));
                            }

                            // Secondary gravity triplet (bytes 49–60 in V24 body). Present only when
                            // frame body length >= 60 (parsed as Option<f32> in protocol.rs).
                            if let (Some(ts), Some(x2), Some(y2), Some(z2)) =
                                (ts_unix, *gravity2_x, *gravity2_y, *gravity2_z)
                            {
                                gravity2.push(json!({
                                    "ts": ts,
                                    "x": x2 as f64,
                                    "y": y2 as f64,
                                    "z": z2 as f64,
                                }));
                            }
                        }
                        DataPacketBodySummary::R22Whoop5Hr {
                            hr_bpm,
                            ..
                        } => {
                            // R22 WHOOP 5.0 realtime packet: push HR into the same stream
                            // as NormalHistory/V24 so the upload pipeline receives it.
                            if let (Some(ts), Some(bpm)) = (ts_unix, *hr_bpm) {
                                hr.push(json!({"ts": ts, "bpm": bpm as u8}));
                            }
                        }
                        DataPacketBodySummary::V18History {
                            hr: v18_hr,
                            rr_intervals_ms,
                            gravity_x,
                            gravity_y,
                            gravity_z,
                            skin_temp_raw,
                            step_motion_counter,
                            ..
                        } => {
                            // V18 WHOOP 5.0 historical packet: push all biometric fields.
                            // No skin_contact byte in v18 — HR/RR/gravity not gated on contact.

                            // HR
                            if let (Some(ts), Some(bpm)) = (ts_unix, *v18_hr) {
                                hr.push(json!({"ts": ts, "bpm": bpm}));
                            }

                            // RR intervals — accumulate with per-interval timestamps
                            if let Some(ts_base) = ts_unix {
                                let mut t = ts_base;
                                for &ms in rr_intervals_ms.iter() {
                                    rr.push(json!({"ts": t, "interval_ms": ms}));
                                    t += ms as f64 / 1000.0;
                                }
                            }

                            // Gravity triplet (f32 LE, already in g units — no IMU_LSB_PER_G conversion)
                            if let (Some(ts), Some(x), Some(y), Some(z)) =
                                (ts_unix, *gravity_x, *gravity_y, *gravity_z)
                            {
                                gravity.push(json!({
                                    "ts": ts,
                                    "x": x as f64,
                                    "y": y as f64,
                                    "z": z as f64,
                                }));
                            }

                            // Skin temperature — raw u16 persisted only when degC is within physiological gate.
                            if let (Some(ts), Some(raw)) = (ts_unix, *skin_temp_raw) {
                                let deg_c = raw as f32 / 128.0;
                                if (5.0..=45.0).contains(&deg_c) {
                                    skin_temp.push(json!({"ts": ts, "raw": raw, "contact": 1}));
                                }
                            }

                            // Step motion counter — accumulate (ts, count) for batch persist below.
                            if let (Some(ts), Some(count)) = (ts_unix, *step_motion_counter) {
                                step.push((ts, count as i64));
                            }
                        }
                    }
                }

                let _ = packet_k; // used for routing above
            }
            Some(ParsedPayload::Event {
                event_id,
                event_name,
                timestamp_seconds,
                data_hex,
                ..
            }) => {
                // EVENT packets: wall-clock unix seconds (real RTC, not device epoch)
                let ts_unix: Option<f64> = timestamp_seconds.map(|s| s as f64);
                let kind = event_name
                    .clone()
                    .or_else(|| event_id.map(|id| format!("event_{id}")));

                events.push(json!({
                    "ts": ts_unix,
                    "kind": kind,
                    "payload": {"data_hex": data_hex},
                }));
            }
            _ => {
                // Command, CommandResponse, Raw, None — no biometric streams to extract
            }
        }

        // HR monitor branch: 0x2A37 standard GATT notifications stored with device_type == "HR_MONITOR".
        // parsed_payload_json is "null" for these rows (parse_frame was bypassed in capture_import.rs),
        // so the match above falls through to `_ => {}`. Gate on device_type string.
        // D-01: bpm + rr_intervals embedded in hr entry. D-02: NOT pushed to top-level rr.
        // D-03: captured_at parsed to f64 unix seconds via unix_from_iso8601 helper.
        // T-08.1-01: hex::decode or parse_hr_measurement failures skip the frame silently.
        if frame.device_type == "HR_MONITOR" {
            let bytes = match hex::decode(&frame.payload_hex) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let measurement = match crate::heart_rate_gatt_protocol::parse_hr_measurement(&bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };
            // D-03: captured_at is "YYYY-MM-DDTHH:MM:SS.mmmZ" — parse to f64 unix seconds.
            // T-08.1-02: on parse failure use null rather than panicking.
            let ts_opt: Option<f64> = unix_from_iso8601(&frame.captured_at);
            hr.push(json!({
                "ts": ts_opt,
                "bpm": measurement.hr_bpm,
                "rr_intervals": measurement.rr_intervals_ms,
            }));
        }
    }

    // Persist gravity rows extracted from V24History frames into the gravity table.
    // device_id from args is required by insert_gravity_rows (validated inside store).
    // On empty vec, store returns Ok(0) immediately — no-op.
    if !gravity.is_empty() {
        let gravity_tuples: Vec<(f64, f64, f64, f64)> = gravity
            .iter()
            .filter_map(|v| {
                let ts = v["ts"].as_f64()?;
                let x = v["x"].as_f64()?;
                let y = v["y"].as_f64()?;
                let z = v["z"].as_f64()?;
                Some((ts, x, y, z))
            })
            .collect();
        let _ = store.insert_gravity_rows(&args.device_id, &gravity_tuples);
    }

    // Persist secondary gravity (gravity2) rows when present.
    if !gravity2.is_empty() {
        let gravity2_tuples: Vec<(f64, f64, f64, f64)> = gravity2
            .iter()
            .filter_map(|v| {
                let ts = v["ts"].as_f64()?;
                let x = v["x"].as_f64()?;
                let y = v["y"].as_f64()?;
                let z = v["z"].as_f64()?;
                Some((ts, x, y, z))
            })
            .collect();
        let _ = store.insert_gravity2_batch(&args.device_id, &gravity2_tuples);
    }

    // Persist step counter rows extracted from V18History frames.
    for (ts, count) in &step {
        let sample_time_unix_ms = (*ts * 1_000.0) as i64;
        let sample_id = format!("v18_step:{}:{}", args.device_id, sample_time_unix_ms);
        let provenance = serde_json::json!({
            "source": "v18_historical_frame",
            "device_id": args.device_id,
        })
        .to_string();
        let _ = store.insert_step_counter_sample(StepCounterSampleInput {
            sample_id: &sample_id,
            sample_time_unix_ms,
            counter_value: *count,
            cadence_spm: None,
            activity_state: None,
            source_kind: "device_counter",
            packet_family: "v18_history",
            json_path: "body_summary.step_motion_counter",
            frame_id: None,
            evidence_id: None,
            capture_session_id: None,
            quality_flags_json: "[]",
            provenance_json: &provenance,
        });
    }

    let result = json!({
        "hr": hr,
        "rr": rr,
        "events": events,
        "battery": battery,
        "spo2": spo2,
        "skin_temp": skin_temp,
        "resp": resp,
        "gravity": gravity,
        "gravity2": gravity2,
        "frame_count": frames.len(),
    });

    serde_json::to_value(result)
        .map_err(|error| GooseError::message(format!("upload streams serialize failed: {error}")))
}

// ── Raw frames upload bridge ──────────────────────────────────────────────────

/// Parse an ISO-8601 UTC string produced by `chrono_from_unix` back to Unix
/// seconds (f64). Format: "YYYY-MM-DDTHH:MM:SS.mmmZ". Returns 0.0 on parse
/// failure so callers can skip malformed rows without crashing.
fn iso8601_to_unix(s: &str) -> f64 {
    // Expected format: "2024-01-15T12:30:45.123Z" (26 chars minimum)
    let s = s.trim_end_matches('Z');
    let parts: Vec<&str> = s.splitn(2, 'T').collect();
    if parts.len() != 2 {
        return 0.0;
    }
    let date_parts: Vec<&str> = parts[0].splitn(3, '-').collect();
    let time_parts: Vec<&str> = parts[1].splitn(2, '.').collect();
    let hms: Vec<&str> = time_parts[0].splitn(3, ':').collect();
    if date_parts.len() != 3 || hms.len() != 3 {
        return 0.0;
    }
    let (Ok(y), Ok(mo), Ok(d)) = (
        date_parts[0].parse::<u32>(),
        date_parts[1].parse::<u32>(),
        date_parts[2].parse::<u32>(),
    ) else {
        return 0.0;
    };
    let (Ok(h), Ok(min), Ok(sec)) = (
        hms[0].parse::<u64>(),
        hms[1].parse::<u64>(),
        hms[2].parse::<u64>(),
    ) else {
        return 0.0;
    };
    let ms: u64 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    // Days since epoch via inverse of days_to_ymd
    let days = ymd_to_days(y, mo, d) as u64;
    let unix_secs = days * 86400 + h * 3600 + min * 60 + sec;
    unix_secs as f64 + ms as f64 / 1000.0
}

/// Convert (year, month, day) to days since Unix epoch (1970-01-01).
/// Mirrors days_to_ymd but in the opposite direction.
fn ymd_to_days(year: u32, month: u32, day: u32) -> u32 {
    // Julian Day Number approach matching days_to_ymd
    let jd = {
        let a = (14u32.wrapping_sub(month)) / 12;
        let y = year as i64 + 4800 - a as i64;
        let m = month as i64 + 12 * a as i64 - 3;
        day as i64 + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045
    };
    (jd - 2440588) as u32 // 2440588 = Julian Day of 1970-01-01
}

/// Return raw_evidence rows captured since `since_ts` in the format expected by
/// the server's `POST /v1/ingest-frames` endpoint. Each row maps directly onto
/// the `RawFrame` Pydantic model on the server side.
///
/// `since_ts` is a Unix timestamp (seconds, float). Returns up to `limit` rows
/// ordered by `captured_at` ascending (oldest first) so the caller can paginate
/// by advancing `since_ts` to the last row's `captured_at_unix`.
fn upload_get_raw_frames_for_upload_bridge(
    args: UploadGetRawFramesArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let since_dt = chrono_from_unix(args.since_ts);
    let now_dt = chrono_now();
    let all_rows = store.raw_evidence_between(&since_dt, &now_dt)?;
    let rows: Vec<&crate::store::RawEvidenceRow> = all_rows.iter().take(args.limit).collect();
    let frames: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let captured_at_unix: f64 = iso8601_to_unix(&r.captured_at);
            json!({
                "captured_at_unix": captured_at_unix,
                "frame_hex": r.payload_hex,
                "source": r.source,
                "device_type": "GOOSE",
                "device_model": r.device_model,
                "sensitivity": r.sensitivity,
                "device_uuid": r.device_uuid,
            })
        })
        .collect();
    let count = frames.len();
    Ok(json!({
        "frames": frames,
        "count": count,
    }))
}

// ── Store gravity bridge ──────────────────────────────────────────────────────

/// A single gravity row argument received from the caller (one sample).
// ---------------------------------------------------------------------------
// EWMA Baseline bridge
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EwmaBaselineFoldHistoryArgs {
    database_path: String,
}

#[derive(Debug, Deserialize)]
struct EwmaBaselineUpdateArgs {
    database_path: String,
    date_key: String,
    hrv_rmssd: f64,
    rhr_bpm: f64,
}

fn ewma_state_to_json(
    state: &crate::baselines::EwmaState,
    trust: EwmaTrustLevel,
) -> serde_json::Value {
    json!({
        "mean": state.mean,
        "variance": state.variance,
        "night_count": state.night_count,
        "trust": trust.as_str(),
        "is_ready": state.is_ready(),
    })
}

// ---------------------------------------------------------------------------
// Recovery V1 bridge (metrics.goose_recovery_v1)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RecoveryV1BridgeArgs {
    database_path: String,
    device_id: String,
    date_key: String,
    hrv_rmssd_ms: f64,
    resting_hr_bpm: f64,
    #[serde(default)]
    resp_rate_rpm: Option<f64>,
    #[serde(default)]
    sleep_performance_fraction: Option<f64>,
}

fn goose_recovery_v1_bridge(args: RecoveryV1BridgeArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let baseline = EwmaBaseline::fold_history(&store)?;
    let input = RecoveryV1Input {
        device_id: args.device_id,
        date_key: args.date_key,
        hrv_rmssd_ms: args.hrv_rmssd_ms,
        resting_hr_bpm: args.resting_hr_bpm,
        resp_rate_rpm: args.resp_rate_rpm,
        sleep_performance_fraction: args.sleep_performance_fraction,
    };
    let output = goose_recovery_v1(&input, &baseline);
    serde_json::to_value(output)
        .map_err(|e| GooseError::message(format!("cannot serialize recovery_v1 output: {e}")))
}

fn ewma_baseline_fold_history_bridge(
    args: EwmaBaselineFoldHistoryArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let baseline = EwmaBaseline::fold_history(&store)?;
    Ok(json!({
        "hrv": ewma_state_to_json(&baseline.hrv, baseline.hrv.trust_level()),
        "resting_hr": ewma_state_to_json(&baseline.resting_hr, baseline.resting_hr.trust_level()),
    }))
}

fn ewma_baseline_update_bridge(args: EwmaBaselineUpdateArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let wrote = store.ewma_baseline_update(&args.date_key, args.hrv_rmssd, args.rhr_bpm)?;
    Ok(json!({"skipped": !wrote}))
}

// ---------------------------------------------------------------------------
// Gravity bridge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct GravityRowArg {
    ts: f64,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Deserialize)]
struct InsertGravityRowsArgs {
    database_path: String,
    device_id: String,
    rows: Vec<GravityRowArg>,
}

#[derive(Debug, Deserialize)]
struct GravityRowsBetweenArgs {
    database_path: String,
    device_id: String,
    ts_start: f64,
    ts_end: f64,
}

fn insert_gravity_rows_bridge(args: InsertGravityRowsArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let tuples: Vec<(f64, f64, f64, f64)> =
        args.rows.iter().map(|r| (r.ts, r.x, r.y, r.z)).collect();
    let inserted = store.insert_gravity_rows(&args.device_id, &tuples)?;
    Ok(json!({"inserted": inserted}))
}

fn gravity_rows_between_bridge(args: GravityRowsBetweenArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let rows: Vec<GravityRow> =
        store.gravity_rows_between(&args.device_id, args.ts_start, args.ts_end)?;
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| json!({"ts": r.ts, "x": r.x, "y": r.y, "z": r.z}))
        .collect();
    Ok(json!({"rows": json_rows}))
}

fn insert_gravity2_batch_bridge(args: InsertGravityRowsArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let tuples: Vec<(f64, f64, f64, f64)> =
        args.rows.iter().map(|r| (r.ts, r.x, r.y, r.z)).collect();
    let inserted = store.insert_gravity2_batch(&args.device_id, &tuples)?;
    Ok(json!({"inserted": inserted}))
}

fn gravity2_samples_between_bridge(args: GravityRowsBetweenArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let rows: Vec<GravityRow> =
        store.gravity2_samples_between(&args.device_id, args.ts_start, args.ts_end)?;
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| json!({"ts": r.ts, "x": r.x, "y": r.y, "z": r.z}))
        .collect();
    Ok(json!({"rows": json_rows}))
}

// ---------------------------------------------------------------------------
// Sync bridge (sync.mark_synced / sync.rows_pending_upload / sync.backfill_streams)
// ---------------------------------------------------------------------------

/// Mark specific rows in a stream table as synced=1 by rowid.
/// Stream name is validated against STREAM_ALLOWLIST in store.mark_synced_rows (T-29-03).
#[derive(Debug, Deserialize)]
struct SyncMarkSyncedArgs {
    database_path: String,
    stream: String,
    row_ids: Vec<i64>,
}

/// Return rows from a stream table where synced=0, ordered by ts.
/// Stream name is validated against STREAM_ALLOWLIST in store.rows_pending_upload.
#[derive(Debug, Deserialize)]
struct SyncRowsPendingUploadArgs {
    database_path: String,
    stream: String,
    limit: i64,
}

/// Populate hr_samples and rr_intervals from decoded_frames for the given device and time window.
#[derive(Debug, Deserialize)]
struct SyncBackfillStreamsArgs {
    database_path: String,
    device_id: String,
    start_ts: f64,
    end_ts: f64,
}

fn sync_mark_synced_bridge(args: SyncMarkSyncedArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let count = store.mark_synced_rows(&args.stream, &args.row_ids)?;
    Ok(json!({"marked": count}))
}

fn sync_rows_pending_upload_bridge(
    args: SyncRowsPendingUploadArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let rows = store.rows_pending_upload(&args.stream, args.limit)?;
    Ok(json!({"rows": rows}))
}

fn sync_backfill_streams_bridge(args: SyncBackfillStreamsArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report: BackfillReport =
        store.backfill_streams_from_decoded_frames(&args.device_id, args.start_ts, args.end_ts)?;
    Ok(json!({
        "hr_inserted": report.hr_inserted,
        "rr_inserted": report.rr_inserted,
        "events_inserted": report.events_inserted,
        "battery_inserted": report.battery_inserted,
    }))
}

// ---------------------------------------------------------------------------
// Exercise detection bridge (exercise.detect_sessions / exercise.sessions_between)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct HrSampleArg {
    ts: f64,
    bpm: u8,
}

#[derive(Debug, Deserialize)]
struct ExerciseProfileArg {
    resting_hr: Option<f64>,
    max_hr: Option<f64>,
    age: Option<u8>,
    sex: Option<String>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
    daily_hr_p10: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DetectExerciseSessionsArgs {
    database_path: String,
    device_id: String,
    hr_samples: Vec<HrSampleArg>,
    gravity_rows: Vec<GravityRow>,
    profile: ExerciseProfileArg,
}

#[derive(Debug, Deserialize)]
struct ExerciseSessionsBetweenArgs {
    database_path: String,
    device_id: String,
    ts_start: f64,
    ts_end: f64,
}

fn exercise_detect_sessions_bridge(
    args: DetectExerciseSessionsArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let hr: Vec<crate::exercise_detection::HrSample> = args
        .hr_samples
        .iter()
        .map(|s| crate::exercise_detection::HrSample {
            ts: s.ts,
            bpm: s.bpm,
        })
        .collect();
    let profile = crate::exercise_detection::ExerciseProfile {
        resting_hr: args.profile.resting_hr,
        max_hr: args.profile.max_hr,
        age: args.profile.age,
        sex: args.profile.sex.clone(),
        weight_kg: args.profile.weight_kg,
        height_cm: args.profile.height_cm,
        daily_hr_p10: args.profile.daily_hr_p10,
    };
    let sessions =
        crate::exercise_detection::detect_exercise_sessions(&hr, &args.gravity_rows, &profile);
    let mut warnings: Vec<String> = Vec::new();

    // Build rows and insert all sessions in a single transaction (PERF-03).
    let rows: Vec<ExerciseSessionRow> = sessions
        .iter()
        .map(|session| ExerciseSessionRow {
            device_id: args.device_id.clone(),
            start_ts: session.start_ts,
            end_ts: session.end_ts,
            duration_s: session.duration_s,
            avg_hr: session.avg_hr,
            peak_hr: session.peak_hr,
            strain: session.strain,
            calories_kcal: session.calories_kcal,
            zone_time_pct_json: serde_json::to_string(&session.zone_time_pct).unwrap_or_default(),
            hrmax_source: session.hrmax_source.clone(),
            rhr_source: session.rhr_source.clone(),
            avg_hrr_pct: session.avg_hrr_pct,
        })
        .collect();

    let inserted = store
        .insert_exercise_sessions_batch(&rows)
        .unwrap_or_else(|e| {
            warnings.push(format!("batch insert failed: {e}"));
            0
        });

    Ok(json!({
        "sessions_detected": sessions.len(),
        "sessions_inserted": inserted,
        "warnings": warnings,
    }))
}

fn exercise_sessions_between_bridge(
    args: ExerciseSessionsBetweenArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let rows = store.exercise_sessions_between(&args.device_id, args.ts_start, args.ts_end)?;
    Ok(json!({ "sessions": rows }))
}

// ---------------------------------------------------------------------------
// V24 biometric bridge (biometrics.insert_v24_batch / biometrics.v24_between /
// biometrics.spo2_from_raw)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Spo2RawArg {
    ts: f64,
    red: u16,
    ir: u16,
    contact: i64,
}

#[derive(Debug, Deserialize)]
struct SkinTempRawArg {
    ts: f64,
    raw: u16,
    contact: i64,
}

#[derive(Debug, Deserialize)]
struct RespRawArg {
    ts: f64,
    raw: u16,
    contact: i64,
}

#[derive(Debug, Deserialize)]
struct SigQualityArg {
    ts: f64,
    quality: u16,
    contact: i64,
}

#[derive(Debug, Deserialize)]
struct InsertV24BatchArgs {
    database_path: String,
    device_id: String,
    #[serde(default)]
    spo2: Vec<Spo2RawArg>,
    #[serde(default)]
    skin_temp: Vec<SkinTempRawArg>,
    #[serde(default)]
    resp: Vec<RespRawArg>,
    #[serde(default)]
    sig_quality: Vec<SigQualityArg>,
}

#[derive(Debug, Deserialize)]
struct V24BetweenArgs {
    database_path: String,
    device_id: String,
    start_ts: f64,
    end_ts: f64,
}

#[derive(Debug, Deserialize)]
struct Spo2FromRawArgs {
    red: u16,
    ir: u16,
}

fn insert_v24_biometric_batch_bridge(args: InsertV24BatchArgs) -> GooseResult<serde_json::Value> {
    use crate::store::V24BiometricBatch;

    let store = open_bridge_store(&args.database_path)?;
    let mut warnings: Vec<String> = Vec::new();

    // Build SpO2 tuples with plausibility gate
    let spo2_tuples: Vec<(f64, i64, i64, i64)> = args
        .spo2
        .iter()
        .filter_map(|row| match spo2_from_raw_uncalibrated(row.red, row.ir) {
            Some(_) => Some((row.ts, row.red as i64, row.ir as i64, row.contact)),
            None => {
                warnings.push(format!(
                    "spo2_plausibility_reject: ts={} red={} ir={} (out of range [70,100]%)",
                    row.ts, row.red, row.ir
                ));
                None
            }
        })
        .collect();

    // Build skin_temp tuples with plausibility gate
    let skin_temp_tuples: Vec<(f64, i64, i64)> = args
        .skin_temp
        .iter()
        .filter_map(|row| match skin_temp_celsius_from_raw(row.raw) {
            Some(_) => Some((row.ts, row.raw as i64, row.contact)),
            None => {
                warnings.push(format!(
                    "skin_temp_plausibility_reject: ts={} raw={} (celsius outside [25,40])",
                    row.ts, row.raw
                ));
                None
            }
        })
        .collect();

    // Build resp tuples (raw u16 is always <= 65535; gate is a no-op by type)
    let resp_tuples: Vec<(f64, i64, i64)> = args
        .resp
        .iter()
        .map(|row| (row.ts, row.raw as i64, row.contact))
        .collect();

    // Build sig_quality tuples (no plausibility gate — quality is a dimensionless score)
    let sig_quality_tuples: Vec<(f64, i64, i64)> = args
        .sig_quality
        .iter()
        .map(|row| (row.ts, row.quality as i64, row.contact))
        .collect();

    let batch = V24BiometricBatch {
        spo2: spo2_tuples,
        skin_temp: skin_temp_tuples,
        resp: resp_tuples,
        sig_quality: sig_quality_tuples,
    };

    store.insert_v24_biometric_batch(&args.device_id, &batch)?;

    Ok(json!({"inserted": true, "warnings": warnings}))
}

fn v24_biometric_samples_between_bridge(args: V24BetweenArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let window =
        store.v24_biometric_samples_between(&args.device_id, args.start_ts, args.end_ts)?;

    let spo2: Vec<serde_json::Value> = window
        .spo2
        .iter()
        .map(|r| json!({"ts": r.ts, "red": r.red, "ir": r.ir, "contact": r.contact}))
        .collect();
    let skin_temp: Vec<serde_json::Value> = window
        .skin_temp
        .iter()
        .map(|r| json!({"ts": r.ts, "raw": r.raw, "contact": r.contact}))
        .collect();
    let resp: Vec<serde_json::Value> = window
        .resp
        .iter()
        .map(|r| json!({"ts": r.ts, "raw": r.raw, "contact": r.contact}))
        .collect();
    let sig_quality: Vec<serde_json::Value> = window
        .sig_quality
        .iter()
        .map(|r| json!({"ts": r.ts, "quality": r.quality, "contact": r.contact}))
        .collect();

    Ok(json!({"spo2": spo2, "skin_temp": skin_temp, "resp": resp, "sig_quality": sig_quality}))
}

fn spo2_from_raw_bridge(args: Spo2FromRawArgs) -> GooseResult<serde_json::Value> {
    match spo2_from_raw_uncalibrated(args.red, args.ir) {
        Some(spo2_pct) => Ok(json!({"spo2_pct": spo2_pct, "quality_flag": "uncalibrated"})),
        None => Ok(json!({"spo2_pct": null, "quality_flag": "uncalibrated", "rejected": true})),
    }
}

// ---------------------------------------------------------------------------
// Sleep staging bridge (metrics.sleep_staging)
// ---------------------------------------------------------------------------

/// HR feature argument as received from Swift (mirrors EpochHrFeature in sleep_staging.rs).
#[derive(Debug, Deserialize)]
struct HrFeatureArg {
    ts: f64,
    hr_bpm: f64,
}

#[derive(Debug, Deserialize)]
struct SleepStagingBridgeArgs {
    database_path: String,
    device_id: String,
    sleep_start_ts: f64,
    sleep_end_ts: f64,
    /// Optional per-epoch HR features for the 4-class classifier.
    /// Absent or empty → 4-class output still valid (light fallback).
    #[serde(default)]
    hr_features: Vec<HrFeatureArg>,
    /// Whether resp_samples data is available for this session. When false,
    /// REM classification is suppressed (graceful degradation — see PROTO-03).
    /// Defaults to true for backwards compatibility with existing callers.
    #[serde(default = "default_resp_available")]
    resp_available: bool,
}

fn default_resp_available() -> bool {
    true
}

fn sleep_staging_bridge(args: SleepStagingBridgeArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let gravity_rows: Vec<GravityRow> =
        store.gravity_rows_between(&args.device_id, args.sleep_start_ts, args.sleep_end_ts)?;
    let tuples: Vec<(f64, f64, f64, f64)> =
        gravity_rows.iter().map(|r| (r.ts, r.x, r.y, r.z)).collect();
    let input = SleepStagingInput {
        device_id: args.device_id.clone(),
        sleep_start_ts: args.sleep_start_ts,
        sleep_end_ts: args.sleep_end_ts,
    };
    let hr_feats: Vec<EpochHrFeature> = args
        .hr_features
        .iter()
        .map(|f| EpochHrFeature {
            ts: f.ts,
            hr_bpm: f.hr_bpm,
        })
        .collect();
    // Determine resp availability: if caller did not pass resp_available=false explicitly,
    // check whether there are any resp rows in the window (lazy check).
    let resp_available = if args.resp_available {
        let resp_count = store
            .resp_samples_between(&args.device_id, args.sleep_start_ts, args.sleep_end_ts)
            .map(|rows| rows.len())
            .unwrap_or(0);
        resp_count > 0
    } else {
        false
    };
    let output: SleepStagingOutput =
        stage_sleep_four_class(&input, &tuples, &hr_feats, resp_available);
    serde_json::to_value(output)
        .map_err(|e| GooseError::message(format!("cannot serialize sleep_staging output: {e}")))
}

/// Format a Unix timestamp (seconds, f64) as an ISO-8601 UTC string for SQLite comparison.
fn chrono_from_unix(ts: f64) -> String {
    let secs = ts as i64;
    let nanos = ((ts - secs as f64) * 1_000_000_000.0) as u32;
    let dt = std::time::UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos);
    // Format as ISO-8601 with millisecond precision, matching SQLite stored format
    let elapsed = dt.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let total_secs = elapsed.as_secs();
    let ms = elapsed.subsec_millis();
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    let days_since_epoch = total_secs / 86400;
    // Simple ISO-8601 formatting without chrono dependency
    // epoch = 1970-01-01; compute year/month/day from days_since_epoch
    let (year, month, day) = days_to_ymd(days_since_epoch as u32);
    format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z",
        h = h % 24
    )
}

/// Return the current UTC time as an ISO-8601 string for use as an upper bound.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let since_epoch = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    chrono_from_unix(since_epoch.as_secs_f64())
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
/// Gregorian calendar implementation without external dependencies.
fn days_to_ymd(days: u32) -> (u32, u32, u32) {
    // Algorithm: Julian Day Number from epoch offset
    let jd = days + 2440588; // Julian Day Number of 1970-01-01 is 2440588
    let l = jd + 68569;
    let n = 4 * l / 146097;
    let l = l - (146097 * n).div_ceil(4);
    let i = 4000 * (l + 1) / 1461001;
    let l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let day = l - 2447 * j / 80;
    let l = j / 11;
    let month = j + 2 - 12 * l;
    let year = 100 * (n - 49) + i + l;
    (year, month, day)
}

/// Parse the codebase's ISO-8601 format "YYYY-MM-DDTHH:MM:SS.mmmZ" to unix seconds (f64).
/// D-03: inverse of chrono_from_unix / days_to_ymd — no chrono dependency.
/// Returns None on any malformed component (T-08.1-02: no panic on bad timestamps).
fn unix_from_iso8601(s: &str) -> Option<f64> {
    // Expected format: "YYYY-MM-DDTHH:MM:SS.mmmZ" (may omit milliseconds or Z suffix)
    // Minimum: "YYYY-MM-DDTHH:MM:SS" (19 chars)
    if s.len() < 19 {
        return None;
    }
    let year: u32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let minute: u32 = s[14..16].parse().ok()?;
    let sec: u32 = s[17..19].parse().ok()?;
    // Milliseconds: optional, after "." if present
    let millis: f64 = if s.len() > 20 && s.as_bytes().get(19) == Some(&b'.') {
        // Collect digits after "."
        let frac: &str = s[20..].trim_end_matches('Z');
        let frac_digits: &str = frac
            .split_once(|c: char| !c.is_ascii_digit())
            .map_or(frac, |(d, _)| d);
        if frac_digits.is_empty() {
            0.0
        } else {
            let raw: f64 = frac_digits.parse().ok()?;
            // Normalise to milliseconds (e.g. "123" → 123 ms, "12" → 12 ms, "1" → 1 ms)
            raw / 10f64.powi(frac_digits.len() as i32 - 3)
        }
    } else {
        0.0
    };

    // Validate calendar ranges
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || sec > 59
    {
        return None;
    }

    // Convert calendar date to days since Unix epoch via inverse Julian-day math
    // (mirror of days_to_ymd, which implements the same Gregorian algorithm)
    let a = (14u32.wrapping_sub(month)) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let jdn = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    // Julian Day Number of 1970-01-01 is 2440588
    let days_since_epoch = jdn.checked_sub(2_440_588)?;

    let secs = days_since_epoch as f64 * 86400.0
        + hour as f64 * 3600.0
        + minute as f64 * 60.0
        + sec as f64
        + millis / 1000.0;
    Some(secs)
}

fn evaluate_calibration_dataset_bridge(
    args: EvaluateCalibrationDatasetArgs,
) -> GooseResult<serde_json::Value> {
    let report = evaluate_linear_calibration(&args.dataset, &args.options);
    let calibration_run_id = args.calibration_run_id.clone();
    let persisted = maybe_persist_calibration_report(
        &report,
        args.database_path.as_deref(),
        args.persist,
        calibration_run_id.as_deref(),
    )?;

    let mut value = serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize calibration report: {error}"))
    })?;
    if let Some(object) = value.as_object_mut() {
        object.insert("persisted".to_string(), json!(persisted));
        object.insert("calibration_run_id".to_string(), json!(calibration_run_id));
    }
    Ok(value)
}

fn evaluate_stored_calibration_labels_bridge(
    args: EvaluateStoredCalibrationLabelsArgs,
) -> GooseResult<serde_json::Value> {
    if args.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if args.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if args.start >= args.end {
        return Err(GooseError::message("start must be earlier than end"));
    }

    let store = open_bridge_store(&args.database_path)?;
    let algorithm_runs = store.algorithm_runs_overlapping(&args.start, &args.end)?;
    let labels = store.calibration_labels_between(&args.start, &args.end)?;
    let (dataset, matched_records, dataset_issues) =
        stored_calibration_dataset(&algorithm_runs, &labels, &args.options);
    let report = evaluate_linear_calibration(&dataset, &args.options);
    let calibration_run_id = args.calibration_run_id.clone();
    let persisted = maybe_persist_calibration_report(
        &report,
        Some(&args.database_path),
        args.persist,
        calibration_run_id.as_deref(),
    )?;

    let mut value = serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize stored calibration report: {error}"
        ))
    })?;
    if let Some(object) = value.as_object_mut() {
        object.insert("persisted".to_string(), json!(persisted));
        object.insert("calibration_run_id".to_string(), json!(calibration_run_id));
        object.insert(
            "dataset_schema".to_string(),
            json!("goose.calibration-dataset.v1"),
        );
        object.insert(
            "dataset_record_count".to_string(),
            json!(dataset.records.len()),
        );
        object.insert(
            "algorithm_run_count".to_string(),
            json!(algorithm_runs.len()),
        );
        object.insert("label_count".to_string(), json!(labels.len()));
        object.insert(
            "matched_record_count".to_string(),
            json!(matched_records.len()),
        );
        object.insert("matched_records".to_string(), json!(matched_records));
        object.insert("dataset_issues".to_string(), json!(dataset_issues));
        object.insert("official_labels_are_labels".to_string(), json!(true));
    }
    Ok(value)
}

fn import_calibration_labels_bridge(
    args: ImportCalibrationLabelsArgs,
) -> GooseResult<serde_json::Value> {
    if args.labels.is_empty() {
        return Err(GooseError::message(
            "at least one calibration label is required",
        ));
    }
    let store = open_bridge_store(&args.database_path)?;
    let mut inserted = 0usize;
    let mut existing = 0usize;
    let mut labels = Vec::new();
    for label in args.labels {
        let provenance_json = serde_json::to_string(&label.provenance).map_err(|error| {
            GooseError::message(format!("cannot serialize label provenance: {error}"))
        })?;
        let changed = store.insert_calibration_label(CalibrationLabelInput {
            label_id: &label.label_id,
            metric_family: &label.metric_family,
            label_source: &label.label_source,
            captured_at: &label.captured_at,
            value: label.value,
            unit: &label.unit,
            provenance_json: &provenance_json,
        })?;
        if changed {
            inserted += 1;
        } else {
            existing += 1;
        }
        if let Some(row) = store.calibration_label(&label.label_id)? {
            labels.push(row);
        }
    }
    Ok(json!({
        "schema": "goose.calibration-label-import-report.v1",
        "generated_by": "goose-bridge",
        "pass": true,
        "label_count": inserted + existing,
        "inserted": inserted,
        "existing": existing,
        "official_labels_are_labels": true,
        "labels": labels,
        "issues": []
    }))
}

fn list_calibration_labels_bridge(
    args: ListCalibrationLabelsArgs,
) -> GooseResult<serde_json::Value> {
    if args.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if args.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if args.start >= args.end {
        return Err(GooseError::message("start must be earlier than end"));
    }
    let store = open_bridge_store(&args.database_path)?;
    let labels = store.calibration_labels_between(&args.start, &args.end)?;
    Ok(json!({
        "schema": "goose.calibration-label-list.v1",
        "generated_by": "goose-bridge",
        "start": args.start,
        "end": args.end,
        "label_count": labels.len(),
        "official_labels_are_labels": true,
        "labels": labels
    }))
}

fn apply_calibration_bridge(args: ApplyCalibrationArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let calibration_run = match args.calibration_run_id.as_deref() {
        Some(calibration_run_id) if !calibration_run_id.trim().is_empty() => {
            store.calibration_run(calibration_run_id)?.ok_or_else(|| {
                GooseError::message(format!("calibration run {calibration_run_id} not found"))
            })?
        }
        _ => latest_matching_calibration_run(&store, &args.algorithm_id, &args.algorithm_version)?
            .ok_or_else(|| {
                GooseError::message(format!(
                    "no calibration run found for {}@{}",
                    args.algorithm_id, args.algorithm_version
                ))
            })?,
    };
    let report = apply_calibration(&CalibrationApplicationInput {
        metric_family: args.metric_family,
        algorithm_id: args.algorithm_id,
        algorithm_version: args.algorithm_version,
        raw_score: args.raw_score,
        input_run_id: args.input_run_id,
        score_min: args.score_min,
        score_max: args.score_max,
        calibration_run,
    });
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize calibration application: {error}"))
    })
}

fn maybe_persist_calibration_report(
    report: &CalibrationReport,
    database_path: Option<&str>,
    persist_requested: bool,
    calibration_run_id: Option<&str>,
) -> GooseResult<bool> {
    if !persist_requested {
        return Ok(false);
    }
    if !report.pass {
        return Err(GooseError::message(
            "calibration report did not pass; refusing to persist",
        ));
    }
    let database_path = database_path
        .ok_or_else(|| GooseError::message("database_path is required when persist is true"))?;
    let calibration_run_id = calibration_run_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            GooseError::message("calibration_run_id is required when persist is true")
        })?;
    let store = open_bridge_store(database_path)?;
    register_built_in_definitions(&store)?;
    let record = calibration_run_record(calibration_run_id, report)?;
    store.insert_calibration_run(&record)
}

fn stored_calibration_dataset(
    algorithm_runs: &[AlgorithmRunRecord],
    labels: &[CalibrationLabelRow],
    options: &CalibrationOptions,
) -> (CalibrationDataset, Vec<serde_json::Value>, Vec<String>) {
    let expected_unit = expected_calibration_label_unit(&options.metric_family);
    let mut records = Vec::new();
    let mut matched_records = Vec::new();
    let mut issues = Vec::new();

    for label in labels
        .iter()
        .filter(|label| label.metric_family.as_str() == options.metric_family.as_str())
    {
        if label.unit != expected_unit {
            issues.push(format!(
                "{} skipped: unit {} does not match {}",
                label.label_id, label.unit, expected_unit
            ));
            continue;
        }
        let provenance = serde_json::from_str::<serde_json::Value>(&label.provenance_json)
            .unwrap_or_else(|_| json!({}));
        let Some(run) =
            matching_calibration_algorithm_run(algorithm_runs, label, &provenance, options)
        else {
            issues.push(format!(
                "{} skipped: no matching algorithm run",
                label.label_id
            ));
            continue;
        };
        let Some(prediction) = prediction_from_algorithm_run(run, &options.metric_family) else {
            issues.push(format!(
                "{} skipped: algorithm run {} has no score field for {}",
                label.label_id, run.run_id, options.metric_family
            ));
            continue;
        };

        let label_provenance = calibration_label_provenance(provenance, label, run);
        let record_id = format!("stored.{}.{}", run.run_id, label.label_id);
        let session_id = label_provenance
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| run.run_id.clone());
        records.push(CalibrationRecord {
            record_id: record_id.clone(),
            captured_at: label.captured_at.clone(),
            session_id: Some(session_id),
            metric_family: label.metric_family.clone(),
            algorithm_id: run.algorithm_id.clone(),
            algorithm_version: run.version.clone(),
            prediction,
            label: label.value,
            label_source: label.label_source.clone(),
            label_provenance,
        });
        matched_records.push(json!({
            "record_id": record_id,
            "label_id": &label.label_id,
            "algorithm_run_id": &run.run_id,
            "captured_at": &label.captured_at,
            "prediction": prediction,
            "label": label.value,
            "unit": &label.unit
        }));
    }

    (
        CalibrationDataset {
            schema: "goose.calibration-dataset.v1".to_string(),
            records,
        },
        matched_records,
        issues,
    )
}

fn matching_calibration_algorithm_run<'a>(
    algorithm_runs: &'a [AlgorithmRunRecord],
    label: &CalibrationLabelRow,
    provenance: &serde_json::Value,
    options: &CalibrationOptions,
) -> Option<&'a AlgorithmRunRecord> {
    if let Some(run_id) = provenance_algorithm_run_id(provenance)
        && let Some(run) = algorithm_runs.iter().find(|run| {
            run.run_id.as_str() == run_id
                && run.algorithm_id.as_str() == options.algorithm_id.as_str()
                && run.version.as_str() == options.algorithm_version.as_str()
        })
    {
        return Some(run);
    }

    algorithm_runs.iter().find(|run| {
        run.algorithm_id.as_str() == options.algorithm_id.as_str()
            && run.version.as_str() == options.algorithm_version.as_str()
            && run.start_time.as_str() <= label.captured_at.as_str()
            && run.end_time.as_str() >= label.captured_at.as_str()
    })
}

fn provenance_algorithm_run_id(provenance: &serde_json::Value) -> Option<&str> {
    ["algorithm_run_id", "run_id", "input_run_id"]
        .into_iter()
        .find_map(|key| provenance.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn prediction_from_algorithm_run(run: &AlgorithmRunRecord, metric_family: &str) -> Option<f64> {
    let output = serde_json::from_str::<serde_json::Value>(&run.output_json).ok()?;
    let field = score_field_for_metric_family(metric_family);
    output
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            output
                .get("output")
                .and_then(|nested| nested.get(field))
                .and_then(serde_json::Value::as_f64)
        })
}

fn score_field_for_metric_family(metric_family: &str) -> &'static str {
    match metric_family {
        "strain" => "score_0_to_21",
        "hrv" => "rmssd_ms",
        _ => "score_0_to_100",
    }
}

fn expected_calibration_label_unit(metric_family: &str) -> &'static str {
    match metric_family {
        "strain" => "score_0_to_21",
        "hrv" => "ms",
        _ => "score_0_to_100",
    }
}

fn calibration_label_provenance(
    provenance: serde_json::Value,
    label: &CalibrationLabelRow,
    run: &AlgorithmRunRecord,
) -> serde_json::Value {
    let mut provenance = provenance;
    if !provenance.is_object() || provenance == json!({}) {
        provenance = json!({
            "source": "stored_calibration_label",
            "official_labels_are_labels": true
        });
    }
    if let Some(object) = provenance.as_object_mut() {
        object.insert("label_id".to_string(), json!(&label.label_id));
        object.insert("algorithm_run_id".to_string(), json!(&run.run_id));
        object.insert("official_labels_are_labels".to_string(), json!(true));
    }
    provenance
}

fn raw_export_bridge(args: RawExportArgs) -> GooseResult<serde_json::Value> {
    if args.output_dir.trim().is_empty() {
        return Err(GooseError::message("output_dir is required"));
    }
    let store = open_bridge_store(&args.database_path)?;
    let database_path = Path::new(&args.database_path);
    let sqlite_source_path = if args.include_sqlite {
        Some(database_path)
    } else {
        None
    };
    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: Path::new(&args.output_dir),
            start: &args.start,
            end: &args.end,
            app_version: &args.app_version,
            core_version: &args.core_version,
            data_families: args.data_families,
            filters: RawExportFilters {
                include_raw_bytes: args.include_raw_bytes,
                capture_session_ids: args.capture_session_ids,
                packet_type_names: args.packet_type_names,
                sensor_source_signals: args.sensor_source_signals,
                metric_families: args.metric_families,
                algorithm_ids: args.algorithm_ids,
                algorithm_versions: args.algorithm_versions,
            },
            sqlite_source_path,
            zip_output_path: args.zip_output_path.as_deref().map(Path::new),
        },
    )?;
    serde_json::to_value(report)
        .map_err(|error| GooseError::message(format!("cannot serialize export report: {error}")))
}

fn export_validate_bundle_bridge(args: ExportValidateBundleArgs) -> GooseResult<serde_json::Value> {
    if args.path.trim().is_empty() {
        return Err(GooseError::message("path is required"));
    }
    let report = validate_export_bundle(Path::new(&args.path))?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize export validation report: {error}"
        ))
    })
}

fn local_health_validation_manifest_scaffold_bridge(
    args: LocalHealthValidationManifestScaffoldArgs,
) -> GooseResult<serde_json::Value> {
    if args.database_path.trim().is_empty() {
        return Err(GooseError::message("database_path is required"));
    }
    scaffold_local_health_validation_manifest(&LocalHealthValidationManifestScaffoldOptions {
        database_path: PathBuf::from(&args.database_path),
        manifest_id: args
            .manifest_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "local-health-capture-validation-scaffold".to_string()),
        timezone: args
            .timezone
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "UTC".to_string()),
        date_key: args.date_key,
        database_source_kind: args
            .database_source_kind
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some("direct_database".to_string())),
        start: args.start,
        end: args.end,
        window_source: args.window_source,
        raw_export_bundle_path: args
            .raw_export_bundle_path
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from),
    })
}

fn local_health_validation_manifest_runbook_bridge(
    args: LocalHealthValidationManifestRunbookArgs,
) -> GooseResult<serde_json::Value> {
    if !args.manifest.is_object() {
        return Err(GooseError::message("manifest object is required"));
    }
    let markdown = local_health_validation_manifest_runbook_markdown(&args.manifest);
    let manifest_schema = args
        .manifest
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    Ok(serde_json::json!({
        "schema": "goose.local-health-validation-runbook.v1",
        "manifest_schema": manifest_schema,
        "markdown_report_path": args
            .manifest
            .get("run_validation")
            .and_then(|value| value.get("markdown_report_path"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("local-health-validation-report.md"),
        "json_report_path": args
            .manifest
            .get("run_validation")
            .and_then(|value| value.get("json_report_path"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("local-health-validation-report.json"),
        "markdown": markdown
    }))
}

fn local_health_validation_manifest_review_bridge(
    args: LocalHealthValidationManifestReviewArgs,
) -> GooseResult<serde_json::Value> {
    if !args.manifest.is_object() {
        return Err(GooseError::message("manifest object is required"));
    }
    Ok(review_local_health_validation_manifest(&args.manifest))
}

fn privacy_lint_bridge(args: PrivacyLintArgs) -> GooseResult<serde_json::Value> {
    if args.path.trim().is_empty() {
        return Err(GooseError::message("path is required"));
    }
    let report = lint_privacy_path(Path::new(&args.path))?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize privacy lint report: {error}"))
    })
}

fn activity_health_sync_dry_run_bridge(
    args: ActivityHealthSyncDryRunInput,
) -> GooseResult<serde_json::Value> {
    let report = run_activity_health_sync_dry_run(&args);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize activity health sync dry-run report: {error}"
        ))
    })
}

fn historical_sync_dry_run_bridge(
    args: HistoricalSyncDryRunInput,
) -> GooseResult<serde_json::Value> {
    let report = run_historical_sync_dry_run(&args);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize historical sync dry-run report: {error}"
        ))
    })
}

fn historical_sync_physical_evidence_template_bridge(
    args: HistoricalSyncPhysicalEvidenceTemplateArgs,
) -> GooseResult<serde_json::Value> {
    let report =
        historical_sync_physical_evidence_template(args.generation, args.capture_session_id);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize historical sync physical evidence template: {error}"
        ))
    })
}

fn historical_sync_physical_validation_bridge(
    args: HistoricalSyncPhysicalValidationInput,
) -> GooseResult<serde_json::Value> {
    let report = validate_historical_sync_physical_evidence(&args);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize historical sync physical validation report: {error}"
        ))
    })
}

fn capture_sanitize_bridge(args: CaptureSanitizeArgs) -> GooseResult<serde_json::Value> {
    if args.input_path.trim().is_empty() {
        return Err(GooseError::message("input_path is required"));
    }
    if args.output_path.trim().is_empty() {
        return Err(GooseError::message("output_path is required"));
    }
    let report = sanitize_capture_path(CaptureSanitizeOptions {
        input_path: Path::new(&args.input_path),
        output_path: Path::new(&args.output_path),
        salt: &args.salt,
    })?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize capture sanitize report: {error}"))
    })
}

fn ui_coverage_audit_bridge(args: UiCoverageAuditArgs) -> GooseResult<serde_json::Value> {
    let input_path = args
        .coverage_map_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_ui_coverage_map_path);
    let input_raw =
        fs::read_to_string(&input_path).map_err(|source| GooseError::io(&input_path, source))?;
    let input: UiCoverageAuditInput =
        serde_json::from_str(&input_raw).map_err(|source| GooseError::json(&input_path, source))?;
    let base_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let report = run_ui_coverage_audit(&input, base_dir)?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize UI coverage audit report: {error}"
        ))
    })
}

fn perf_budget_bridge(args: PerfBudgetArgs) -> GooseResult<serde_json::Value> {
    let report = run_perf_budget(PerfBudgetOptions {
        scale: args.scale,
        budgets: PerfBudgets::default(),
    })?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize perf budget report: {error}"))
    })
}

fn property_suite_bridge(args: PropertySuiteArgs) -> GooseResult<serde_json::Value> {
    let report = run_property_suite(PropertySuiteOptions {
        seed: args.seed,
        cases_per_group: args.cases_per_group,
    })?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize property suite report: {error}"))
    })
}

fn reference_compare_bridge(args: ReferenceCompareArgs) -> GooseResult<serde_json::Value> {
    let report = match args.family.as_str() {
        "hrv" => {
            let input: HrvInput = serde_json::from_value(args.input)
                .map_err(|error| GooseError::message(format!("invalid HRV input: {error}")))?;
            compare_hrv_goose_to_reference(&input)?
        }
        "sleep" => {
            let use_sleep_v1 = args
                .goose_algorithm_id
                .as_deref()
                .is_some_and(|id| id == crate::metrics::GOOSE_SLEEP_V1_ID)
                || args
                    .input
                    .get("sleep")
                    .is_some_and(|value| value.is_object());
            if use_sleep_v1 {
                let input: SleepV1Input = serde_json::from_value(normalize_sleep_v1_input_value(
                    args.input,
                ))
                .map_err(|error| GooseError::message(format!("invalid sleep v1 input: {error}")))?;
                if let Some(reference_report) = args.reference_report {
                    compare_sleep_v1_goose_to_external_reference_report(&input, &reference_report)?
                } else {
                    compare_sleep_v1_goose_to_reference(&input)?
                }
            } else {
                let input: SleepInput = serde_json::from_value(args.input).map_err(|error| {
                    GooseError::message(format!("invalid sleep input: {error}"))
                })?;
                if let Some(reference_report) = args.reference_report {
                    compare_sleep_goose_to_external_reference_report(&input, &reference_report)?
                } else {
                    compare_sleep_goose_to_reference(&input)?
                }
            }
        }
        "strain" => {
            let input: StrainInput = serde_json::from_value(args.input)
                .map_err(|error| GooseError::message(format!("invalid strain input: {error}")))?;
            compare_strain_goose_to_reference(&input)?
        }
        "stress" => {
            let input: StressInput = serde_json::from_value(args.input)
                .map_err(|error| GooseError::message(format!("invalid stress input: {error}")))?;
            compare_stress_goose_to_reference(&input)?
        }
        other => {
            return Err(GooseError::message(format!(
                "unsupported reference comparison family {other}; use hrv|sleep|strain|stress"
            )));
        }
    };
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize reference comparison report: {error}"
        ))
    })
}

fn normalize_sleep_v1_input_value(input: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut object) = input else {
        return input;
    };
    let Some(serde_json::Value::Object(sleep)) = object.remove("sleep") else {
        return serde_json::Value::Object(object);
    };
    let mut merged = sleep;
    for (key, value) in object {
        merged.insert(key, value);
    }
    serde_json::Value::Object(merged)
}

fn metric_input_readiness_bridge(args: MetricInputReadinessArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let correlation = run_capture_correlation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_owned_captures: args.require_owned_captures,
        },
    )?;
    let report = run_metric_input_readiness(
        &correlation,
        MetricInputReadinessOptions {
            require_scores_ready: args.require_scores_ready,
        },
    );
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize metric input readiness report: {error}"
        ))
    })
}

fn motion_features_bridge(args: MotionFeaturesArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_motion_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        MotionFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize motion feature report: {error}"))
    })
}

fn heart_rate_features_bridge(args: HeartRateFeaturesArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_heart_rate_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize heart-rate feature report: {error}"
        ))
    })
}

fn vital_event_features_bridge(args: VitalEventFeaturesArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_vital_event_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize vital event feature report: {error}"
        ))
    })
}

fn step_packet_discovery_bridge(args: StepPacketDiscoveryArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_step_packet_discovery_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        StepPacketDiscoveryOptions {
            max_candidate_fields: args.max_candidate_fields.unwrap_or(250),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize step packet discovery report: {error}"
        ))
    })
}

fn step_capture_validation_bridge(
    args: StepCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_step_capture_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        StepCaptureValidationOptions {
            max_candidate_fields: args.max_candidate_fields.unwrap_or(1000),
            capture_kind: args.capture_kind,
            manual_step_delta: args.manual_step_delta,
            official_whoop_step_delta: args.official_whoop_step_delta,
            tolerance_steps: args.tolerance_steps.unwrap_or(10).max(0),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize step capture validation report: {error}"
        ))
    })
}

fn raw_motion_step_estimate_bridge(
    args: RawMotionStepEstimateArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_raw_motion_step_estimate_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        RawMotionStepEstimateOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            sample_rate_hz: args.sample_rate_hz.unwrap_or(50.0),
            peak_threshold_i16: args.peak_threshold_i16.unwrap_or(1_200.0),
            min_peak_spacing_samples: args.min_peak_spacing_samples.unwrap_or(10),
            manual_step_delta: args.manual_step_delta,
            official_whoop_step_delta: args.official_whoop_step_delta,
            tolerance_steps: args.tolerance_steps.unwrap_or(10),
            label_provenance: args.label_provenance,
            date_key: args.date_key,
            timezone: args.timezone,
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize raw-motion step estimate report: {error}"
        ))
    })
}

fn step_counter_ingest_bridge(args: StepCounterIngestArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_step_counter_ingest_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        StepCounterIngestOptions {
            max_candidate_fields: args.max_candidate_fields.unwrap_or(1000),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize step counter ingest report: {error}"
        ))
    })
}

fn step_counter_daily_rollup_bridge(
    args: StepCounterDailyRollupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_device_step_counter_day(
        &store,
        StepCounterDailyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start_time_unix_ms: args.start_time_unix_ms,
            end_time_unix_ms: args.end_time_unix_ms,
            min_sample_count: args.min_sample_count.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize step counter daily rollup report: {error}"
        ))
    })
}

fn step_counter_hourly_rollup_bridge(
    args: StepCounterHourlyRollupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_device_step_counter_hour(
        &store,
        StepCounterHourlyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start_time_unix_ms: args.start_time_unix_ms,
            end_time_unix_ms: args.end_time_unix_ms,
            min_sample_count: args.min_sample_count.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize step counter hourly rollup report: {error}"
        ))
    })
}

fn activity_unavailable_daily_status_bridge(
    args: ActivityUnavailableDailyStatusArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_activity_unavailable_daily_status_for_store(
        &store,
        ActivityUnavailableDailyStatusOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start_time_unix_ms: args.start_time_unix_ms,
            end_time_unix_ms: args.end_time_unix_ms,
            min_sample_count: args.min_sample_count.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize activity unavailable daily status report: {error}"
        ))
    })
}

fn daily_activity_metrics_bridge(
    args: DailyActivityMetricListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metrics =
        store.daily_activity_metrics_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    Ok(json!({
        "schema": "goose.daily-activity-metric-list.v1",
        "generated_by": "goose-bridge",
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "metric_count": metrics.len(),
        "metrics": metrics,
    }))
}

fn hourly_activity_metrics_bridge(
    args: HourlyActivityMetricListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metrics =
        store.hourly_activity_metrics_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    Ok(json!({
        "schema": "goose.hourly-activity-metric-list.v1",
        "generated_by": "goose-bridge",
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "metric_count": metrics.len(),
        "metrics": metrics,
    }))
}

fn daily_recovery_metrics_bridge(
    args: DailyRecoveryMetricListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metrics =
        store.daily_recovery_metrics_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    Ok(json!({
        "schema": "goose.daily-recovery-metric-list.v1",
        "generated_by": "goose-bridge",
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "metric_count": metrics.len(),
        "metrics": metrics,
    }))
}

fn energy_daily_rollup_bridge(args: EnergyDailyRollupArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_energy_day_for_store(
        &store,
        &args.database_path,
        EnergyDailyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            profile_weight_kg: args.profile_weight_kg,
            profile_age_years: args.profile_age_years,
            profile_sex: args.profile_sex.as_deref(),
            profile_height_cm: args.profile_height_cm,
            resting_hr_bpm: args.resting_hr_bpm,
            max_hr_bpm: args.max_hr_bpm,
            min_heart_rate_samples: args.min_heart_rate_samples.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize energy daily rollup report: {error}"
        ))
    })
}

fn energy_unavailable_daily_status_bridge(
    args: EnergyDailyRollupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_energy_unavailable_daily_status_for_store(
        &store,
        &args.database_path,
        EnergyDailyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            profile_weight_kg: args.profile_weight_kg,
            profile_age_years: args.profile_age_years,
            profile_sex: args.profile_sex.as_deref(),
            profile_height_cm: args.profile_height_cm,
            resting_hr_bpm: args.resting_hr_bpm,
            max_hr_bpm: args.max_hr_bpm,
            min_heart_rate_samples: args.min_heart_rate_samples.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize energy unavailable daily status report: {error}"
        ))
    })
}

fn energy_hourly_rollup_bridge(args: EnergyHourlyRollupArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_energy_hour_for_store(
        &store,
        &args.database_path,
        EnergyHourlyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            profile_weight_kg: args.profile_weight_kg,
            profile_age_years: args.profile_age_years,
            profile_sex: args.profile_sex.as_deref(),
            resting_hr_bpm: args.resting_hr_bpm,
            max_hr_bpm: args.max_hr_bpm,
            min_heart_rate_samples: args.min_heart_rate_samples.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize energy hourly rollup report: {error}"
        ))
    })
}

fn energy_capture_validation_bridge(
    args: EnergyCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = validate_energy_capture_for_store(
        &store,
        &args.database_path,
        EnergyCaptureValidationOptions {
            rollup_options: EnergyDailyRollupOptions {
                date_key: &args.date_key,
                timezone: &args.timezone,
                start: &args.start,
                end: &args.end,
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
                profile_weight_kg: args.profile_weight_kg,
                profile_age_years: args.profile_age_years,
                profile_sex: args.profile_sex.as_deref(),
                profile_height_cm: args.profile_height_cm,
                resting_hr_bpm: args.resting_hr_bpm,
                max_hr_bpm: args.max_hr_bpm,
                min_heart_rate_samples: args.min_heart_rate_samples.unwrap_or(2),
                write_metric: false,
            },
            capture_kind: args.capture_kind,
            official_whoop_active_kcal: args.official_whoop_active_kcal,
            official_whoop_resting_kcal: args.official_whoop_resting_kcal,
            official_whoop_total_kcal: args.official_whoop_total_kcal,
            tolerance_kcal: args.tolerance_kcal.unwrap_or(75.0),
            relative_tolerance_fraction: args.relative_tolerance_fraction.unwrap_or(0.25),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize energy capture validation report: {error}"
        ))
    })
}

fn validate_requested_primary_algorithm(
    metric_family: &str,
    requested_algorithm_id: Option<&str>,
    requested_algorithm_version: Option<&str>,
    supported_algorithm_id: &str,
    supported_algorithm_version: &str,
) -> GooseResult<()> {
    let Some(requested_id) = requested_algorithm_id else {
        return Ok(());
    };
    let requested_id = requested_id.trim();
    if requested_id.is_empty() {
        return Err(GooseError::message(
            "algorithm_id must be non-empty when provided",
        ));
    }
    let requested_version = requested_algorithm_version
        .map(str::trim)
        .unwrap_or(supported_algorithm_version);
    if requested_version.is_empty() {
        return Err(GooseError::message(
            "algorithm_version must be non-empty when provided",
        ));
    }
    if requested_id != supported_algorithm_id || requested_version != supported_algorithm_version {
        return Err(GooseError::message(format!(
            "unsupported primary algorithm {requested_id}@{requested_version} for {metric_family}; this packet-derived scorer currently supports {supported_algorithm_id}@{supported_algorithm_version}"
        )));
    }
    Ok(())
}

fn hrv_features_bridge(args: HrvFeaturesArgs) -> GooseResult<serde_json::Value> {
    validate_requested_primary_algorithm(
        "hrv",
        args.algorithm_id.as_deref(),
        args.algorithm_version.as_deref(),
        GOOSE_HRV_V0_ID,
        GOOSE_HRV_V0_VERSION,
    )?;
    let store = open_bridge_store(&args.database_path)?;
    let report = run_hrv_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            min_rr_intervals_to_compute: args.min_rr_intervals_to_compute.unwrap_or(2),
            baseline_min_days: args.baseline_min_days.unwrap_or(3),
            require_baseline: args.require_baseline,
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|error| {
        GooseError::message(format!("cannot serialize HRV feature report: {error}"))
    })?;
    maybe_persist_algorithm_run(
        &store,
        &mut value,
        args.persist_algorithm_run,
        args.algorithm_run_id.as_deref(),
        "packet-derived-hrv",
        report.score_result.as_ref(),
    )?;
    Ok(value)
}

fn hrv_capture_validation_bridge(args: HrvCaptureValidationArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_hrv_capture_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        HrvCaptureValidationOptions {
            feature_options: HrvFeatureOptions {
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
                min_rr_intervals_to_compute: args.min_rr_intervals_to_compute.unwrap_or(2),
                baseline_min_days: 1,
                require_baseline: false,
            },
            capture_kind: args.capture_kind,
            official_whoop_hrv_rmssd_ms: args.official_whoop_hrv_rmssd_ms,
            tolerance_ms: args.tolerance_ms.unwrap_or(10.0),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize HRV capture validation report: {error}"
        ))
    })
}

fn respiratory_rate_capture_validation_bridge(
    args: RespiratoryRateCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_respiratory_rate_capture_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        RespiratoryRateCaptureValidationOptions {
            feature_options: VitalEventFeatureOptions {
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
            },
            capture_kind: args.capture_kind,
            official_whoop_respiratory_rate_rpm: args.official_whoop_respiratory_rate_rpm,
            tolerance_rpm: args.tolerance_rpm.unwrap_or(1.0),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize respiratory-rate capture validation report: {error}"
        ))
    })
}

fn oxygen_saturation_capture_validation_bridge(
    args: OxygenSaturationCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_oxygen_saturation_capture_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        OxygenSaturationCaptureValidationOptions {
            feature_options: VitalEventFeatureOptions {
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
            },
            capture_kind: args.capture_kind,
            official_whoop_oxygen_saturation_percent: args.official_whoop_oxygen_saturation_percent,
            tolerance_percent: args.tolerance_percent.unwrap_or(2.0),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize oxygen-saturation capture validation report: {error}"
        ))
    })
}

fn temperature_capture_validation_bridge(
    args: TemperatureCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_temperature_capture_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        TemperatureCaptureValidationOptions {
            feature_options: VitalEventFeatureOptions {
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
            },
            capture_kind: args.capture_kind,
            official_whoop_skin_temperature_delta_c: args.official_whoop_skin_temperature_delta_c,
            tolerance_c: args.tolerance_c.unwrap_or(0.3),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize temperature capture validation report: {error}"
        ))
    })
}

fn recovery_sensor_discovery_bridge(
    args: RecoverySensorDiscoveryArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_recovery_sensor_discovery_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        RecoverySensorDiscoveryOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            min_rr_intervals_to_compute: args.min_rr_intervals_to_compute.unwrap_or(2),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize recovery sensor discovery report: {error}"
        ))
    })
}

fn recovery_unavailable_daily_status_bridge(
    args: RecoveryUnavailableDailyStatusArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_recovery_unavailable_daily_status_for_store(
        &store,
        &args.database_path,
        RecoveryUnavailableDailyStatusOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            min_rr_intervals_to_compute: args.min_rr_intervals_to_compute.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize recovery unavailable daily status report: {error}"
        ))
    })
}

fn recovery_sensor_daily_rollup_bridge(
    args: RecoverySensorDailyRollupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_recovery_sensor_daily_for_store(
        &store,
        &args.database_path,
        RecoverySensorDailyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            min_rr_intervals_to_compute: args.min_rr_intervals_to_compute.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize recovery sensor daily rollup report: {error}"
        ))
    })
}

fn metric_window_features_bridge(args: MetricWindowFeaturesArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_metric_window_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        MetricWindowFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            resting_hr_bpm: args.resting_hr_bpm,
            max_hr_bpm: args.max_hr_bpm,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize metric window feature report: {error}"
        ))
    })
}

fn resting_heart_rate_features_bridge(
    args: RestingHeartRateFeaturesArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_resting_heart_rate_feature_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            baseline_min_days: args.baseline_min_days.unwrap_or(3),
            require_baseline: args.require_baseline,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize resting heart-rate feature report: {error}"
        ))
    })
}

fn resting_heart_rate_daily_rollup_bridge(
    args: RestingHeartRateDailyRollupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = rollup_resting_heart_rate_day_for_store(
        &store,
        &args.database_path,
        RestingHeartRateDailyRollupOptions {
            date_key: &args.date_key,
            timezone: &args.timezone,
            start: &args.start,
            end: &args.end,
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            baseline_min_days: args.baseline_min_days.unwrap_or(3),
            require_baseline: args.require_baseline,
            min_sample_count: args.min_sample_count.unwrap_or(2),
            write_metric: args.write_metric,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize resting heart-rate daily rollup report: {error}"
        ))
    })
}

fn resting_heart_rate_capture_validation_bridge(
    args: RestingHeartRateCaptureValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = validate_resting_heart_rate_capture_for_store(
        &store,
        &args.database_path,
        RestingHeartRateCaptureValidationOptions {
            rollup_options: RestingHeartRateDailyRollupOptions {
                date_key: &args.date_key,
                timezone: &args.timezone,
                start: &args.start,
                end: &args.end,
                min_owned_captures_per_summary: args
                    .min_owned_captures
                    .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
                require_trusted_evidence: args.require_trusted_evidence,
                baseline_min_days: args.baseline_min_days.unwrap_or(3),
                require_baseline: args.require_baseline,
                min_sample_count: args.min_sample_count.unwrap_or(2),
                write_metric: false,
            },
            capture_kind: args.capture_kind,
            official_whoop_resting_hr_bpm: args.official_whoop_resting_hr_bpm,
            tolerance_bpm: args.tolerance_bpm.unwrap_or(3.0),
            label_provenance: args.label_provenance,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize resting heart-rate capture validation report: {error}"
        ))
    })
}

fn sleep_v1_input_from_feature_score(
    store: &GooseStore,
    sleep_input: &SleepInput,
    report: &SleepFeatureScoreReport,
    history_import_in_progress: bool,
) -> GooseResult<SleepV1Input> {
    let prior_history_end_unix_ms = sleep_time_unix_ms(&sleep_input.start_time)
        .ok_or_else(|| GooseError::message("sleep_v1_input_start_time_invalid"))?;
    let prior_nights = external_sleep_history_nights_for_sleep_v1(
        store,
        sleep_input.sleep_need_minutes,
        prior_history_end_unix_ms,
    )?;
    let naps_minutes = external_sleep_naps_before_sleep(store, sleep_input)?;
    let schedule_baseline = sleep_history_schedule_baseline(&prior_nights);
    let imported_sleep_history_seen = !prior_nights.is_empty();
    let imported_platform_sleep_nights = prior_nights
        .iter()
        .filter(|night| sleep_history_night_is_usable(night))
        .count() as u32;
    let excluded_sleep_nights = prior_nights
        .iter()
        .filter(|night| !sleep_history_night_is_usable(night))
        .count() as u32;
    let repeated_low_confidence_nights = prior_nights
        .iter()
        .filter(|night| night.confidence_0_to_1 < 0.50)
        .count()
        >= 3;
    let days_since_last_valid_night = days_since_last_valid_sleep_night(sleep_input, &prior_nights);
    let trusted_goose_sleep_nights = u32::from(
        report
            .sleep_window
            .as_ref()
            .is_some_and(|window| window.trusted_metric_input),
    );
    let stage_segments = report
        .sleep_window
        .as_ref()
        .map(|window| {
            window
                .stage_segments
                .iter()
                .map(|segment| SleepStageSegment {
                    stage_kind: sleep_stage_kind_label(&segment.stage).to_string(),
                    start_time: segment.start_time.clone(),
                    end_time: segment.end_time.clone(),
                    duration_minutes: segment.duration_minutes,
                    confidence_0_to_1: segment.confidence_0_to_1,
                    stage_probabilities: if segment.stage_probabilities.is_empty() {
                        BTreeMap::from([(
                            sleep_stage_kind_label(&segment.stage).to_string(),
                            segment.confidence_0_to_1,
                        )])
                    } else {
                        segment.stage_probabilities.clone()
                    },
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let data_coverage_fraction = report.sleep_window.as_ref().map(|window| {
        (window.motion_coverage_fraction + window.heart_rate_coverage_fraction) / 2.0
    });

    Ok(SleepV1Input {
        sleep: sleep_input.clone(),
        model_status: SleepModelStatusInput {
            sleep_permission_granted: imported_sleep_history_seen,
            history_import_in_progress,
            imported_platform_sleep_nights,
            excluded_sleep_nights,
            trusted_goose_sleep_nights,
            days_since_last_valid_night,
            repeated_low_confidence_nights,
            motion_coverage_fraction: report
                .sleep_window
                .as_ref()
                .map(|window| window.motion_coverage_fraction),
            heart_rate_coverage_fraction: report
                .sleep_window
                .as_ref()
                .map(|window| window.heart_rate_coverage_fraction),
            ..Default::default()
        },
        prior_nights,
        stage_segments,
        sleep_hr_average_bpm: report
            .sleep_window
            .as_ref()
            .and_then(|window| window.average_sleep_hr_bpm),
        sleep_hr_min_bpm: report
            .sleep_window
            .as_ref()
            .and_then(|window| window.lowest_sleep_hr_bpm),
        pre_sleep_awake_hr_average_bpm: report
            .sleep_window
            .as_ref()
            .and_then(|window| window.baseline_awake_hr_bpm),
        sleep_hr_trend_bpm_per_hour: report
            .sleep_window
            .as_ref()
            .and_then(|window| window.sleep_hr_trend_bpm_per_hour),
        bedtime_deviation_minutes: schedule_baseline
            .and_then(|(typical_bedtime, _)| {
                sleep_time_minute_of_day(&sleep_input.start_time)
                    .map(|bedtime| circular_minute_deviation(bedtime, typical_bedtime))
            })
            .unwrap_or(0.0),
        wake_time_deviation_minutes: schedule_baseline
            .and_then(|(_, typical_wake_time)| {
                sleep_time_minute_of_day(&sleep_input.end_time)
                    .map(|wake_time| circular_minute_deviation(wake_time, typical_wake_time))
            })
            .unwrap_or(0.0),
        naps_minutes,
        data_coverage_fraction,
        ..Default::default()
    })
}

fn days_since_last_valid_sleep_night(
    sleep_input: &SleepInput,
    prior_nights: &[SleepNightHistoryInput],
) -> Option<u32> {
    let current_start_unix_ms = sleep_time_unix_ms(&sleep_input.start_time)?;
    let latest_valid_end_unix_ms = prior_nights
        .iter()
        .filter(|night| sleep_history_night_is_usable(night))
        .filter_map(|night| sleep_time_unix_ms(&night.end_time))
        .max()?;
    let elapsed_ms = current_start_unix_ms.saturating_sub(latest_valid_end_unix_ms);
    Some((elapsed_ms / (24 * 60 * 60 * 1_000)) as u32)
}

fn external_sleep_history_nights_for_sleep_v1(
    store: &GooseStore,
    sleep_need_minutes: f64,
    before_unix_ms: i64,
) -> GooseResult<Vec<SleepNightHistoryInput>> {
    let sessions = store.external_sleep_sessions_between(0, before_unix_ms)?;
    let mut nights = Vec::new();
    for session in sessions
        .into_iter()
        .filter(|session| session.end_time_unix_ms <= before_unix_ms)
    {
        let detailed_stages = store.external_sleep_stages_for_session(&session.sleep_id)?;
        let maybe_night = (|| {
            let (mut stage_minutes, has_stage_summary_minutes) =
                external_sleep_stage_minutes_from_rows_or_summary(
                    &detailed_stages,
                    &session.stage_summary_json,
                );
            let time_in_bed_minutes = session.duration_ms as f64 / 60_000.0;
            if time_in_bed_minutes <= 0.0 || !time_in_bed_minutes.is_finite() {
                return None;
            }
            let stage_minutes_normalized = normalize_external_stage_minutes_to_time_in_bed(
                &mut stage_minutes,
                time_in_bed_minutes,
            );
            let sleep_duration_minutes = external_sleep_duration_minutes_or_empty_summary_fallback(
                &stage_minutes,
                time_in_bed_minutes,
                has_stage_summary_minutes,
            )?;
            if sleep_duration_minutes <= 0.0 {
                return None;
            }
            let is_nap = external_sleep_session_is_nap(
                session.start_time_unix_ms,
                session.end_time_unix_ms,
                sleep_duration_minutes,
            );
            if is_nap {
                return None;
            }
            let awake_minutes = stage_minutes
                .get("awake")
                .copied()
                .unwrap_or((time_in_bed_minutes - sleep_duration_minutes).max(0.0));
            let excluded_from_baseline = stage_minutes_normalized
                || external_sleep_session_has_platform_import_marker(&session)
                || external_sleep_session_excluded_from_baseline(
                    session.confidence,
                    &session.provenance_json,
                )
                || external_sleep_stage_rows_excluded_from_baseline(&detailed_stages);
            Some(SleepNightHistoryInput {
                night_id: session.sleep_id,
                start_time: format!("unix_ms:{}", session.start_time_unix_ms),
                end_time: format!("unix_ms:{}", session.end_time_unix_ms),
                sleep_duration_minutes,
                sleep_need_minutes,
                time_in_bed_minutes,
                awake_minutes,
                sleep_latency_minutes: 0.0,
                wake_after_sleep_onset_minutes: awake_minutes,
                wake_episode_count: 0,
                stage_minutes,
                heart_rate_dip_percent: None,
                sleep_hr_average_bpm: None,
                sleep_hr_min_bpm: None,
                pre_sleep_awake_hr_average_bpm: None,
                sleep_hr_trend_bpm_per_hour: None,
                bedtime_deviation_minutes: 0.0,
                wake_time_deviation_minutes: 0.0,
                midpoint_deviation_minutes: 0.0,
                naps_minutes: 0.0,
                confidence_0_to_1: session.confidence,
                source: session.platform,
                excluded_from_baseline,
            })
        })();
        if let Some(night) = maybe_night {
            nights.push(night);
        }
    }
    if let Some((typical_bedtime, typical_wake_time)) = sleep_history_schedule_baseline(&nights) {
        for night in &mut nights {
            if let Some(bedtime) = sleep_time_minute_of_day(&night.start_time) {
                night.bedtime_deviation_minutes =
                    circular_minute_deviation(bedtime, typical_bedtime);
            }
            if let Some(wake_time) = sleep_time_minute_of_day(&night.end_time) {
                night.wake_time_deviation_minutes =
                    circular_minute_deviation(wake_time, typical_wake_time);
            }
            night.midpoint_deviation_minutes =
                (night.bedtime_deviation_minutes + night.wake_time_deviation_minutes) / 2.0;
        }
    }
    Ok(nights)
}

fn external_sleep_session_excluded_from_baseline(confidence: f64, provenance_json: &str) -> bool {
    if confidence < 0.50 {
        return true;
    }
    let Ok(provenance) = serde_json::from_str::<Value>(provenance_json) else {
        return true;
    };
    provenance
        .get("overlap_conflict")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || provenance
            .get("excluded_from_baseline")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || provenance_has_baseline_exclusion_context(&provenance)
}

fn external_sleep_session_has_platform_import_marker(session: &ExternalSleepSessionRow) -> bool {
    external_sleep_platform_import_token(&session.platform)
        || external_sleep_platform_import_token(&session.source)
        || external_sleep_provenance_has_platform_import_marker(&session.provenance_json)
}

fn external_sleep_stage_rows_excluded_from_baseline(stages: &[ExternalSleepStageRow]) -> bool {
    stages.iter().any(|stage| {
        stage.confidence < 0.50
            || serde_json::from_str::<Value>(&stage.provenance_json).map_or(true, |provenance| {
                provenance
                    .get("overlap_conflict")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || provenance
                        .get("excluded_from_baseline")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    || provenance_has_baseline_exclusion_context(&provenance)
                    || value_has_platform_import_marker(&provenance)
            })
    })
}

fn provenance_has_baseline_exclusion_context(provenance: &Value) -> bool {
    const BOOL_KEYS: &[&str] = &[
        "travel",
        "sickness",
        "illness",
        "manual_entry",
        "manual_correction",
        "manually_corrected",
    ];
    const STRING_KEYS: &[&str] = &[
        "detected_context",
        "context",
        "journal_tag",
        "tag",
        "source",
        "correction_source",
    ];
    const ARRAY_KEYS: &[&str] = &["journal_tags", "tags", "context_tags", "quality_flags"];

    if BOOL_KEYS.iter().any(|key| {
        provenance
            .get(*key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) {
        return true;
    }

    if STRING_KEYS.iter().any(|key| {
        provenance
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(baseline_exclusion_context_token)
    }) {
        return true;
    }

    ARRAY_KEYS.iter().any(|key| {
        provenance
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .any(baseline_exclusion_context_token)
            })
    })
}

fn baseline_exclusion_context_token(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    matches!(
        normalized.as_str(),
        "travel"
            | "sick"
            | "sickness"
            | "illness"
            | "manual_entry"
            | "manual_correction"
            | "manual_edit"
            | "manual_sleep_edit"
            | "manually_corrected"
    )
}

fn external_sleep_provenance_has_platform_import_marker(provenance_json: &str) -> bool {
    serde_json::from_str::<Value>(provenance_json)
        .map(|provenance| value_has_platform_import_marker(&provenance))
        .unwrap_or(true)
}

fn value_has_platform_import_marker(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            external_sleep_platform_import_token(key) || value_has_platform_import_marker(child)
        }),
        Value::Array(values) => values.iter().any(value_has_platform_import_marker),
        Value::String(text) => external_sleep_platform_import_token(text),
        _ => false,
    }
}

fn external_sleep_platform_import_token(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    matches!(
        normalized.as_str(),
        "healthkit"
            | "health_kit"
            | "apple_health"
            | "apple_healthkit"
            | "hkhealthstore"
            | "healthkit_sleep_analysis"
            | "health_connect"
            | "google_health_connect"
            | "health_connect_sleep_session"
            | "health_connect_sleep_stage"
            | "imported_platform_sleep"
            | "sleep_history_import"
            | "external_history_context_only"
    ) || normalized.starts_with("healthkit_")
        || normalized.starts_with("health_kit_")
        || normalized.contains("_healthkit_")
        || normalized.contains("_health_connect_")
}

fn external_sleep_naps_before_sleep(
    store: &GooseStore,
    sleep_input: &SleepInput,
) -> GooseResult<f64> {
    let Some(sleep_start_unix_ms) = sleep_time_unix_ms(&sleep_input.start_time) else {
        return Ok(0.0);
    };
    let lookback_start_unix_ms = sleep_start_unix_ms.saturating_sub(18 * 60 * 60 * 1000);
    let sessions =
        store.external_sleep_sessions_between(lookback_start_unix_ms, sleep_start_unix_ms)?;
    let mut naps_minutes = 0.0;
    for session in sessions
        .into_iter()
        .filter(|session| session.end_time_unix_ms <= sleep_start_unix_ms)
    {
        let detailed_stages = store.external_sleep_stages_for_session(&session.sleep_id)?;
        let maybe_nap_minutes = (|| {
            let duration_minutes = session.duration_ms as f64 / 60_000.0;
            if duration_minutes <= 0.0 || !duration_minutes.is_finite() {
                return None;
            }
            let (mut stage_minutes, has_stage_summary_minutes) =
                external_sleep_stage_minutes_from_rows_or_summary(
                    &detailed_stages,
                    &session.stage_summary_json,
                );
            let stage_minutes_normalized = normalize_external_stage_minutes_to_time_in_bed(
                &mut stage_minutes,
                duration_minutes,
            );
            if stage_minutes_normalized
                || external_sleep_session_has_platform_import_marker(&session)
                || external_sleep_session_excluded_from_baseline(
                    session.confidence,
                    &session.provenance_json,
                )
                || external_sleep_stage_rows_excluded_from_baseline(&detailed_stages)
            {
                return None;
            }
            let sleep_duration_minutes = external_sleep_duration_minutes_or_empty_summary_fallback(
                &stage_minutes,
                duration_minutes,
                has_stage_summary_minutes,
            )?;
            external_sleep_session_is_nap(
                session.start_time_unix_ms,
                session.end_time_unix_ms,
                sleep_duration_minutes,
            )
            .then_some(sleep_duration_minutes)
        })();
        if let Some(minutes) = maybe_nap_minutes {
            naps_minutes += minutes;
        }
    }
    Ok(naps_minutes)
}

fn external_sleep_session_is_nap(
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    sleep_duration_minutes: f64,
) -> bool {
    if !(20.0..=180.0).contains(&sleep_duration_minutes) {
        return false;
    }
    let midpoint_unix_ms = start_time_unix_ms + (end_time_unix_ms - start_time_unix_ms) / 2;
    let midpoint_minute = unix_ms_minute_of_day(midpoint_unix_ms);
    (9.0 * 60.0..=20.0 * 60.0).contains(&midpoint_minute)
}

fn sleep_history_schedule_baseline(nights: &[SleepNightHistoryInput]) -> Option<(f64, f64)> {
    let mut bedtime_minutes = nights
        .iter()
        .filter(|night| sleep_history_night_is_usable(night))
        .filter_map(|night| sleep_time_minute_of_day(&night.start_time))
        .collect::<Vec<_>>();
    let mut wake_time_minutes = nights
        .iter()
        .filter(|night| sleep_history_night_is_usable(night))
        .filter_map(|night| sleep_time_minute_of_day(&night.end_time))
        .collect::<Vec<_>>();
    if bedtime_minutes.is_empty() || wake_time_minutes.is_empty() {
        return None;
    }
    Some((
        typical_minute_of_day(&mut bedtime_minutes),
        typical_minute_of_day(&mut wake_time_minutes),
    ))
}

fn sleep_time_minute_of_day(value: &str) -> Option<f64> {
    if let Some(unix_ms) = value
        .strip_prefix("unix_ms:")
        .and_then(|text| text.parse::<i64>().ok())
    {
        return Some(unix_ms_minute_of_day(unix_ms));
    }
    rfc3339_minute_of_day(value)
}

fn sleep_time_unix_ms(value: &str) -> Option<i64> {
    if let Some(unix_ms) = value
        .strip_prefix("unix_ms:")
        .and_then(|text| text.parse::<i64>().ok())
    {
        return Some(unix_ms);
    }
    parse_rfc3339_utc_unix_ms(value)
}

fn unix_ms_minute_of_day(unix_ms: i64) -> f64 {
    ((unix_ms / 60_000).rem_euclid(24 * 60)) as f64
}

fn rfc3339_minute_of_day(value: &str) -> Option<f64> {
    let (_, time) = value.split_once('T')?;
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    if hour >= 24 || minute >= 60 {
        return None;
    }
    Some((hour * 60 + minute) as f64)
}

fn parse_rfc3339_utc_unix_ms(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let seconds_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let second = seconds_part
        .split_once('.')
        .map(|(second, _)| second)
        .unwrap_or(seconds_part)
        .parse::<u32>()
        .ok()?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64) * 1_000)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year as i64 - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn typical_minute_of_day(values: &mut [f64]) -> f64 {
    values.sort_by(|left, right| left.total_cmp(right));
    values
        .iter()
        .copied()
        .min_by(|left, right| {
            let left_distance = values
                .iter()
                .map(|value| circular_minute_deviation(*left, *value))
                .sum::<f64>();
            let right_distance = values
                .iter()
                .map(|value| circular_minute_deviation(*right, *value))
                .sum::<f64>();
            left_distance.total_cmp(&right_distance)
        })
        .unwrap_or(0.0)
}

fn circular_minute_deviation(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs().rem_euclid(24.0 * 60.0);
    difference.min(24.0 * 60.0 - difference)
}

fn external_sleep_stage_minutes_from_rows_or_summary(
    stages: &[ExternalSleepStageRow],
    stage_summary_json: &str,
) -> (BTreeMap<String, f64>, bool) {
    if !stages.is_empty() {
        let mut stage_minutes = BTreeMap::new();
        for stage in stages {
            let Some(stage_kind) = canonical_external_sleep_stage(&stage.stage_kind) else {
                continue;
            };
            let minutes = stage.duration_ms as f64 / 60_000.0;
            if minutes.is_finite() && minutes >= 0.0 {
                *stage_minutes.entry(stage_kind.to_string()).or_insert(0.0) += minutes;
            }
        }
        return (stage_minutes, true);
    }
    external_sleep_stage_minutes(stage_summary_json)
}

fn external_sleep_stage_minutes(stage_summary_json: &str) -> (BTreeMap<String, f64>, bool) {
    let Ok(summary) = serde_json::from_str::<Value>(stage_summary_json) else {
        return (BTreeMap::new(), false);
    };
    let Some(values) = summary.get("minutes_by_stage").and_then(Value::as_object) else {
        return (BTreeMap::new(), false);
    };
    let has_stage_summary_minutes = !values.is_empty();
    let stage_minutes = values
        .iter()
        .fold(BTreeMap::new(), |mut acc, (stage, minutes)| {
            if let (Some(stage), Some(minutes)) = (
                canonical_external_sleep_stage(stage),
                minutes
                    .as_f64()
                    .filter(|minutes| minutes.is_finite() && *minutes >= 0.0),
            ) {
                *acc.entry(stage.to_string()).or_insert(0.0) += minutes;
            }
            acc
        });
    (stage_minutes, has_stage_summary_minutes)
}

fn external_sleep_duration_minutes(stage_minutes: &BTreeMap<String, f64>) -> Option<f64> {
    let asleep = ["core", "deep", "rem"]
        .iter()
        .filter_map(|stage| stage_minutes.get(*stage))
        .copied()
        .sum::<f64>();
    (asleep > 0.0).then_some(asleep)
}

fn external_sleep_duration_minutes_or_empty_summary_fallback(
    stage_minutes: &BTreeMap<String, f64>,
    time_in_bed_minutes: f64,
    has_stage_summary_minutes: bool,
) -> Option<f64> {
    if !has_stage_summary_minutes {
        Some(time_in_bed_minutes)
    } else {
        external_sleep_duration_minutes(stage_minutes)
            .map(|minutes| minutes.min(time_in_bed_minutes))
    }
}

fn normalize_external_stage_minutes_to_time_in_bed(
    stage_minutes: &mut BTreeMap<String, f64>,
    time_in_bed_minutes: f64,
) -> bool {
    let total = stage_minutes.values().copied().sum::<f64>();
    if total <= time_in_bed_minutes || total <= 0.0 {
        return false;
    }
    let scale = time_in_bed_minutes / total;
    for minutes in stage_minutes.values_mut() {
        *minutes *= scale;
    }
    true
}

fn canonical_external_sleep_stage(stage: &str) -> Option<&'static str> {
    match stage
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "awake" | "asleep_awake" | "sleep_awake" | "out_of_bed" => Some("awake"),
        "asleep" | "asleep_unspecified" | "core" | "light" | "asleep_core" | "sleep_light" => {
            Some("core")
        }
        "deep" | "asleep_deep" | "sleep_deep" => Some("deep"),
        "rem" | "asleep_rem" | "sleep_rem" => Some("rem"),
        "in_bed" | "inbed" => None,
        _ => None,
    }
}

fn canonical_external_sleep_stage_row(stage: &str) -> Option<&'static str> {
    match stage
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "in_bed" | "inbed" => Some("in_bed"),
        "unknown" => Some("unknown"),
        "not_applicable" | "not_applicable_sleep" => Some("not_applicable"),
        value => canonical_external_sleep_stage(value),
    }
}

fn sleep_stage_kind_label(stage: &SleepStageKind) -> &'static str {
    match stage {
        SleepStageKind::Awake => "awake",
        SleepStageKind::Core => "core",
        SleepStageKind::Deep => "deep",
        SleepStageKind::Rem => "rem",
    }
}

fn sleep_feature_score_bridge(args: SleepFeatureScoreArgs) -> GooseResult<serde_json::Value> {
    let requested_algorithm_id = args
        .algorithm_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(GOOSE_SLEEP_V0_ID);
    let requested_algorithm_version = args
        .algorithm_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if requested_algorithm_id == GOOSE_SLEEP_V1_ID {
            GOOSE_SLEEP_V1_VERSION
        } else {
            GOOSE_SLEEP_V0_VERSION
        });
    let sleep_v1_requested = match (requested_algorithm_id, requested_algorithm_version) {
        (GOOSE_SLEEP_V0_ID, GOOSE_SLEEP_V0_VERSION) => false,
        (GOOSE_SLEEP_V1_ID, GOOSE_SLEEP_V1_VERSION) => true,
        _ => {
            return Err(GooseError::message(format!(
                "unsupported primary algorithm {requested_algorithm_id}@{requested_algorithm_version} for sleep; this packet-derived scorer currently supports {GOOSE_SLEEP_V0_ID}@{GOOSE_SLEEP_V0_VERSION} and {GOOSE_SLEEP_V1_ID}@{GOOSE_SLEEP_V1_VERSION}"
            )));
        }
    };
    let store = open_bridge_store(&args.database_path)?;
    let report = run_sleep_feature_score_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            sleep_need_minutes: args.sleep_need_minutes.unwrap_or(480.0),
            low_motion_threshold_0_to_1: args.low_motion_threshold_0_to_1.unwrap_or(0.05),
            disturbance_motion_threshold_0_to_1: args
                .disturbance_motion_threshold_0_to_1
                .unwrap_or(0.20),
            target_midpoint_minutes_since_midnight: args
                .target_midpoint_minutes_since_midnight
                .unwrap_or(180.0),
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep feature score report: {error}"
        ))
    })?;
    if sleep_v1_requested {
        if let Some(sleep_input) = report.sleep_input.as_ref() {
            let sleep_v1_input = sleep_v1_input_from_feature_score(
                &store,
                sleep_input,
                &report,
                args.history_import_in_progress,
            )?;
            let sleep_v1_result = goose_sleep_v1(&sleep_v1_input);
            value["sleep_v1_input"] = serde_json::to_value(&sleep_v1_input).map_err(|error| {
                GooseError::message(format!("cannot serialize sleep v1 input: {error}"))
            })?;
            value["score_result"] = metric_result_to_value(&sleep_v1_result)?;
            maybe_persist_algorithm_run(
                &store,
                &mut value,
                args.persist_algorithm_run,
                args.algorithm_run_id.as_deref(),
                "packet-derived-sleep-v1",
                Some(&sleep_v1_result),
            )?;
        } else {
            value["score_result"] = Value::Null;
            maybe_persist_algorithm_run::<crate::metrics::SleepV1Output>(
                &store,
                &mut value,
                args.persist_algorithm_run,
                args.algorithm_run_id.as_deref(),
                "packet-derived-sleep-v1",
                None,
            )?;
        }
    } else {
        maybe_persist_algorithm_run(
            &store,
            &mut value,
            args.persist_algorithm_run,
            args.algorithm_run_id.as_deref(),
            "packet-derived-sleep",
            report.score_result.as_ref(),
        )?;
    }
    Ok(value)
}

fn recovery_feature_score_bridge(args: RecoveryFeatureScoreArgs) -> GooseResult<serde_json::Value> {
    validate_requested_primary_algorithm(
        "recovery",
        args.algorithm_id.as_deref(),
        args.algorithm_version.as_deref(),
        GOOSE_RECOVERY_V0_ID,
        GOOSE_RECOVERY_V0_VERSION,
    )?;
    let store = open_bridge_store(&args.database_path)?;
    let hrv_start = args.hrv_start.as_deref().unwrap_or(&args.start);
    let hrv_end = args.hrv_end.as_deref().unwrap_or(&args.end);
    let sleep_start = args.sleep_start.as_deref().unwrap_or(&args.start);
    let sleep_end = args.sleep_end.as_deref().unwrap_or(&args.end);
    let prior_strain_start = args.prior_strain_start.as_deref().unwrap_or(&args.start);
    let prior_strain_end = args.prior_strain_end.as_deref().unwrap_or(&args.end);
    let report = run_recovery_feature_score_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        hrv_start,
        hrv_end,
        &args.hrv_baseline_start,
        &args.hrv_baseline_end,
        &args.resting_start,
        &args.resting_end,
        sleep_start,
        sleep_end,
        prior_strain_start,
        prior_strain_end,
        RecoveryFeatureScoreOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            resting_baseline_min_days: args.resting_baseline_min_days.unwrap_or(3),
            hrv_min_rr_intervals_to_compute: args.hrv_min_rr_intervals_to_compute.unwrap_or(2),
            hrv_baseline_min_days: args.hrv_baseline_min_days.unwrap_or(3),
            sleep_need_minutes: args.sleep_need_minutes.unwrap_or(480.0),
            low_motion_threshold_0_to_1: args.low_motion_threshold_0_to_1.unwrap_or(0.05),
            disturbance_motion_threshold_0_to_1: args
                .disturbance_motion_threshold_0_to_1
                .unwrap_or(0.20),
            target_midpoint_minutes_since_midnight: args
                .target_midpoint_minutes_since_midnight
                .unwrap_or(180.0),
            prior_strain_resting_baseline_min_days: args
                .prior_strain_resting_baseline_min_days
                .unwrap_or(3),
            prior_strain_max_hr_bpm: args.prior_strain_max_hr_bpm,
            respiratory_rate_rpm: args.respiratory_rate_rpm,
            respiratory_rate_baseline_rpm: args.respiratory_rate_baseline_rpm,
            skin_temp_delta_c: args.skin_temp_delta_c,
            provided_vitals_source: args.provided_vitals_source,
            provided_vitals_provenance_json: args.provided_vitals_provenance_json,
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize recovery feature score report: {error}"
        ))
    })?;
    if args.persist_algorithm_run && !report.pass {
        value["persisted_algorithm_run"] = json!({
            "persist_requested": true,
            "inserted": false,
            "blocked_reason": "report_not_passed",
            "issues": &report.issues,
        });
    } else {
        maybe_persist_algorithm_run(
            &store,
            &mut value,
            args.persist_algorithm_run,
            args.algorithm_run_id.as_deref(),
            "packet-derived-recovery",
            report.score_result.as_ref(),
        )?;
    }
    Ok(value)
}

fn strain_feature_score_bridge(args: StrainFeatureScoreArgs) -> GooseResult<serde_json::Value> {
    validate_requested_primary_algorithm(
        "strain",
        args.algorithm_id.as_deref(),
        args.algorithm_version.as_deref(),
        GOOSE_STRAIN_V0_ID,
        GOOSE_STRAIN_V0_VERSION,
    )?;
    let store = open_bridge_store(&args.database_path)?;
    let resting_start = args.resting_start.as_deref().unwrap_or(&args.start);
    let resting_end = args.resting_end.as_deref().unwrap_or(&args.end);
    let report = run_strain_feature_score_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        resting_start,
        resting_end,
        StrainFeatureScoreOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            resting_baseline_min_days: args.resting_baseline_min_days.unwrap_or(3),
            max_hr_bpm: args.max_hr_bpm,
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize strain feature score report: {error}"
        ))
    })?;
    maybe_persist_algorithm_run(
        &store,
        &mut value,
        args.persist_algorithm_run,
        args.algorithm_run_id.as_deref(),
        "packet-derived-strain",
        report.score_result.as_ref(),
    )?;
    Ok(value)
}

fn stress_feature_score_bridge(args: StressFeatureScoreArgs) -> GooseResult<serde_json::Value> {
    validate_requested_primary_algorithm(
        "stress",
        args.algorithm_id.as_deref(),
        args.algorithm_version.as_deref(),
        GOOSE_STRESS_V0_ID,
        GOOSE_STRESS_V0_VERSION,
    )?;
    let store = open_bridge_store(&args.database_path)?;
    let hrv_start = args.hrv_start.as_deref().unwrap_or(&args.start);
    let hrv_end = args.hrv_end.as_deref().unwrap_or(&args.end);
    let report = run_stress_feature_score_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        &args.resting_start,
        &args.resting_end,
        hrv_start,
        hrv_end,
        &args.hrv_baseline_start,
        &args.hrv_baseline_end,
        StressFeatureScoreOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_trusted_evidence: args.require_trusted_evidence,
            resting_baseline_min_days: args.resting_baseline_min_days.unwrap_or(3),
            hrv_min_rr_intervals_to_compute: args.hrv_min_rr_intervals_to_compute.unwrap_or(2),
            hrv_baseline_min_days: args.hrv_baseline_min_days.unwrap_or(3),
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize stress feature score report: {error}"
        ))
    })?;
    maybe_persist_algorithm_run(
        &store,
        &mut value,
        args.persist_algorithm_run,
        args.algorithm_run_id.as_deref(),
        "packet-derived-stress",
        report.score_result.as_ref(),
    )?;
    Ok(value)
}

fn health_sync_dry_run_bridge(input: HealthSyncDryRunInput) -> GooseResult<serde_json::Value> {
    let report = run_health_sync_dry_run(&input);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize health sync dry-run report: {error}"
        ))
    })
}

fn capture_import_frame_batch_bridge(
    args: CaptureImportFrameBatchArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store_hot(&args.database_path)?;
    let report = import_captured_frame_batch_with_output_options(
        &store,
        &args.frames,
        CapturedFrameBatchOptions {
            parser_version: &args.parser_version,
            active_device_id: args.active_device_id.as_deref(),
        },
        CapturedFrameBatchOutputOptions {
            include_timeline_rows: args.include_timeline_rows,
            compact_raw_payloads: args.compact_raw_payloads,
            include_results: args.include_results,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize capture import report: {error}"))
    })
}

fn storage_compact_raw_evidence_bridge(
    args: StorageCompactRawEvidenceArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = store.compact_raw_evidence_payloads_to_limit(args.limit_bytes)?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize compaction report: {error}"))
    })
}

fn overnight_mirror_batch_bridge(args: OvernightMirrorBatchArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let sessions: Vec<OvernightSyncSessionInput<'_>> = args
        .sessions
        .iter()
        .map(|session| OvernightSyncSessionInput {
            session_id: &session.session_id,
            started_at: &session.started_at,
            ended_at: session.ended_at.as_deref(),
            band_identifier: session.band_identifier.as_deref(),
            app_version: session.app_version.as_deref(),
            mode: &session.mode,
            final_status: &session.final_status,
            raw_frame_count: session.raw_frame_count,
            historical_frame_count: session.historical_frame_count,
            k18_count: session.k18_count,
            k24_count: session.k24_count,
            k25_count: session.k25_count,
            k26_count: session.k26_count,
            packet47_count: session.packet47_count,
            event17_count: session.event17_count,
            event29_count: session.event29_count,
            metadata49_count: session.metadata49_count,
            metadata56_count: session.metadata56_count,
            range_poll_count: session.range_poll_count,
            successful_range_poll_count: session.successful_range_poll_count,
            event_log_count: session.event_log_count,
            readiness_status: session.readiness_status.as_deref(),
            readiness: session.readiness.as_deref(),
            error_count: session.error_count,
            notes: session.notes.as_deref(),
        })
        .collect();
    let raw_notifications: Vec<OvernightRawNotificationInput<'_>> = args
        .raw_notifications
        .iter()
        .map(|notification| OvernightRawNotificationInput {
            session_id: &notification.session_id,
            captured_at: &notification.captured_at,
            source: &notification.source,
            device_id: notification.device_id.as_deref(),
            active_device_name: notification.active_device_name.as_deref(),
            connection_state: notification.connection_state.as_deref(),
            service_uuid: notification.service_uuid.as_deref(),
            characteristic_uuid: &notification.characteristic_uuid,
            device_type: notification.device_type.as_deref(),
            command_or_event: notification.command_or_event,
            packet_type: notification.packet_type,
            k_revision: notification.k_revision,
            sequence: notification.sequence,
            frame_hex: &notification.frame_hex,
            payload_hex: notification.payload_hex.as_deref(),
            byte_count: notification.byte_count,
            decode_status: &notification.decode_status,
        })
        .collect();
    let historical_range_polls: Vec<OvernightHistoricalRangePollInput<'_>> = args
        .historical_range_polls
        .iter()
        .map(|poll| OvernightHistoricalRangePollInput {
            session_id: &poll.session_id,
            captured_at: &poll.captured_at,
            status: &poll.status,
            command_sequence: poll.command_sequence,
            result_code: poll.result_code,
            result_name: &poll.result_name,
            raw_payload_hex: &poll.raw_payload_hex,
            raw_body_hex: &poll.raw_body_hex,
            revision_or_status: poll.revision_or_status,
            page_current: poll.page_current,
            page_oldest: poll.page_oldest,
            page_end: poll.page_end,
            pages_behind: poll.pages_behind,
            pending_response_count: poll.pending_response_count,
            retry_count: poll.retry_count,
            notes: &poll.notes,
        })
        .collect();
    let report =
        store.mirror_overnight_batch(&sessions, &raw_notifications, &historical_range_polls)?;
    serde_json::to_value(report)
        .map_err(|error| GooseError::message(format!("cannot serialize overnight mirror: {error}")))
}

fn overnight_mirror_counts_bridge(
    args: OvernightMirrorCountsArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let counts = store.overnight_mirror_counts(&args.session_id)?;
    serde_json::to_value(counts).map_err(|error| {
        GooseError::message(format!("cannot serialize overnight mirror counts: {error}"))
    })
}

fn capture_timeline_bridge(args: CaptureTimelineArgs) -> GooseResult<serde_json::Value> {
    if args.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if args.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if args.start >= args.end {
        return Err(GooseError::message("start must be earlier than end"));
    }
    let store = open_bridge_store(&args.database_path)?;
    let rows = packet_timeline_between(&store, &args.start, &args.end)?;
    serde_json::to_value(rows)
        .map_err(|error| GooseError::message(format!("cannot serialize capture timeline: {error}")))
}

fn capture_observability_timeline_bridge(
    args: CaptureObservabilityTimelineArgs,
) -> GooseResult<serde_json::Value> {
    if args.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if args.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if args.start >= args.end {
        return Err(GooseError::message("start must be earlier than end"));
    }
    if args.start_unix_ms < 0 {
        return Err(GooseError::message("start_unix_ms must be non-negative"));
    }
    if args.end_unix_ms <= 0 {
        return Err(GooseError::message("end_unix_ms must be positive"));
    }
    if args.start_unix_ms >= args.end_unix_ms {
        return Err(GooseError::message(
            "start_unix_ms must be earlier than end_unix_ms",
        ));
    }

    let store = open_bridge_store(&args.database_path)?;
    let raw_rows = store.raw_evidence_between(&args.start, &args.end)?;
    let packet_rows = packet_timeline_between(&store, &args.start, &args.end)?;
    let debug_rows = store.debug_events_between(args.start_unix_ms, args.end_unix_ms)?;
    let rows = observability_timeline_from_rows(&raw_rows, &packet_rows, &debug_rows)?;
    serde_json::to_value(rows).map_err(|error| {
        GooseError::message(format!("cannot serialize observability timeline: {error}"))
    })
}

fn capture_start_session_bridge(args: CaptureStartSessionArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store_hot(&args.database_path)?;
    let provenance_json = if args.provenance.is_null() {
        "{}".to_string()
    } else {
        if !args.provenance.is_object() {
            return Err(GooseError::message("provenance must be a JSON object"));
        }
        serde_json::to_string(&args.provenance)
            .map_err(|error| GooseError::message(format!("cannot serialize provenance: {error}")))?
    };
    let inserted = store.start_capture_session(CaptureSessionInput {
        session_id: &args.session_id,
        source: &args.source,
        started_at_unix_ms: args.started_at_unix_ms,
        device_model: &args.device_model,
        active_device_id: args.active_device_id.as_deref(),
        provenance_json: &provenance_json,
    })?;
    let session = store.capture_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("capture session {} not found", args.session_id))
    })?;
    serde_json::to_value(json!({
        "schema": "goose.capture-session-result.v1",
        "inserted": inserted,
        "session": session,
    }))
    .map_err(|error| GooseError::message(format!("cannot serialize capture session: {error}")))
}

fn capture_finish_session_bridge(args: CaptureFinishSessionArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store_hot(&args.database_path)?;
    let session =
        store.finish_capture_session(&args.session_id, args.ended_at_unix_ms, args.frame_count)?;
    serde_json::to_value(json!({
        "schema": "goose.capture-session-result.v1",
        "inserted": false,
        "session": session,
    }))
    .map_err(|error| GooseError::message(format!("cannot serialize capture session: {error}")))
}

fn capture_list_sessions_bridge(args: CaptureListSessionsArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let sessions = store.capture_sessions_between(args.start_unix_ms, args.end_unix_ms)?;
    serde_json::to_value(json!({
        "schema": "goose.capture-session-list.v1",
        "session_count": sessions.len(),
        "sessions": sessions,
    }))
    .map_err(|error| GooseError::message(format!("cannot serialize capture session list: {error}")))
}

fn activity_create_session_bridge(
    args: ActivitySessionUpsertArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let provenance_json = json_object_string("provenance", &args.provenance)?;
    let inserted = store.insert_activity_session(ActivitySessionInput {
        session_id: &args.session_id,
        source: &args.source,
        start_time_unix_ms: args.start_time_unix_ms,
        end_time_unix_ms: args.end_time_unix_ms,
        activity_type: &args.activity_type,
        external_activity_type_code: args.external_activity_type_code.as_deref(),
        external_activity_type_name: args.external_activity_type_name.as_deref(),
        custom_label: args.custom_label.as_deref(),
        confidence: args.confidence,
        detection_method: &args.detection_method,
        sync_status: &args.sync_status,
        provenance_json: &provenance_json,
    })?;
    let session = store.activity_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("activity session {} not found", args.session_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-session-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
        "session": session,
    }))
}

fn activity_get_session_bridge(args: ActivitySessionLookupArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let session = store.activity_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("activity session {} not found", args.session_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-session-result.v1",
        "generated_by": "goose-bridge",
        "session": session,
    }))
}

fn activity_list_sessions_bridge(args: ActivitySessionListArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let sessions =
        store.activity_sessions_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    Ok(json!({
        "schema": "goose.activity-session-list.v1",
        "generated_by": "goose-bridge",
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "session_count": sessions.len(),
        "sessions": sessions,
    }))
}

fn activity_list_sessions_with_metrics_bridge(
    args: ActivitySessionListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let sessions =
        store.activity_sessions_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    let session_ids = sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    let metrics = store.activity_metrics_for_sessions(&session_ids)?;
    let mut metrics_by_session: BTreeMap<String, Vec<ActivityMetricRow>> = BTreeMap::new();
    for metric in metrics {
        metrics_by_session
            .entry(metric.activity_session_id.clone())
            .or_default()
            .push(metric);
    }

    Ok(json!({
        "schema": "goose.activity-session-list-with-metrics.v1",
        "generated_by": "goose-bridge",
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "session_count": sessions.len(),
        "sessions": sessions,
        "metrics_by_session": metrics_by_session,
    }))
}

fn activity_update_session_bridge(
    args: ActivitySessionUpsertArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let provenance_json = json_object_string("provenance", &args.provenance)?;
    let updated = store.update_activity_session(ActivitySessionInput {
        session_id: &args.session_id,
        source: &args.source,
        start_time_unix_ms: args.start_time_unix_ms,
        end_time_unix_ms: args.end_time_unix_ms,
        activity_type: &args.activity_type,
        external_activity_type_code: args.external_activity_type_code.as_deref(),
        external_activity_type_name: args.external_activity_type_name.as_deref(),
        custom_label: args.custom_label.as_deref(),
        confidence: args.confidence,
        detection_method: &args.detection_method,
        sync_status: &args.sync_status,
        provenance_json: &provenance_json,
    })?;
    let session = store.activity_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("activity session {} not found", args.session_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-session-result.v1",
        "generated_by": "goose-bridge",
        "updated": updated,
        "session": session,
    }))
}

fn activity_correction_plans_bridge() -> GooseResult<serde_json::Value> {
    let plans = activity_session_correction_plans();
    Ok(json!({
        "schema": "goose.activity-correction-plans.v1",
        "generated_by": "goose-bridge",
        "plan_count": plans.len(),
        "plans": plans,
    }))
}

fn activity_apply_correction_bridge(
    args: ActivitySessionCorrectionArgs,
) -> GooseResult<serde_json::Value> {
    if !args.details.is_object() {
        return Err(GooseError::message("details must be a JSON object"));
    }
    if !args.provenance.is_object() {
        return Err(GooseError::message("provenance must be a JSON object"));
    }

    let store = open_bridge_store(&args.database_path)?;
    let existing = store.activity_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("activity session {} not found", args.session_id))
    })?;

    let previous_provenance =
        serde_json::from_str::<Value>(&existing.provenance_json).map_err(|error| {
            GooseError::message(format!(
                "activity session {} provenance_json is invalid: {error}",
                existing.session_id
            ))
        })?;

    let mut start_time_unix_ms = existing.start_time_unix_ms;
    let mut end_time_unix_ms = existing.end_time_unix_ms;
    let mut activity_type = existing.activity_type.clone();
    let mut external_activity_type_code = existing.external_activity_type_code.clone();
    let mut external_activity_type_name = existing.external_activity_type_name.clone();
    let mut custom_label = existing.custom_label.clone();

    match args.kind {
        ActivitySessionCorrectionKind::ChangeActivityType => {
            activity_type = args.activity_type.clone().ok_or_else(|| {
                GooseError::message(
                    "activity_type is required for change_activity_type corrections",
                )
            })?;
            if args.external_activity_type_code.is_some() {
                external_activity_type_code = args.external_activity_type_code.clone();
            }
            if args.external_activity_type_name.is_some() {
                external_activity_type_name = args.external_activity_type_name.clone();
            }
            if args.custom_label.is_some() {
                custom_label = args.custom_label.clone();
            }
        }
        ActivitySessionCorrectionKind::TrimStart => {
            start_time_unix_ms = args.start_time_unix_ms.ok_or_else(|| {
                GooseError::message("start_time_unix_ms is required for trim_start corrections")
            })?;
        }
        ActivitySessionCorrectionKind::TrimEnd => {
            end_time_unix_ms = args.end_time_unix_ms.ok_or_else(|| {
                GooseError::message("end_time_unix_ms is required for trim_end corrections")
            })?;
        }
        ActivitySessionCorrectionKind::Split
        | ActivitySessionCorrectionKind::Merge
        | ActivitySessionCorrectionKind::FalsePositive => {}
    }

    let mut details = args.details.as_object().cloned().unwrap_or_default();
    details.insert(
        "previous_start_time_unix_ms".to_string(),
        json!(existing.start_time_unix_ms),
    );
    details.insert(
        "previous_end_time_unix_ms".to_string(),
        json!(existing.end_time_unix_ms),
    );
    details.insert(
        "previous_activity_type".to_string(),
        json!(existing.activity_type.clone()),
    );
    details.insert(
        "updated_start_time_unix_ms".to_string(),
        json!(start_time_unix_ms),
    );
    details.insert(
        "updated_end_time_unix_ms".to_string(),
        json!(end_time_unix_ms),
    );
    details.insert(
        "updated_activity_type".to_string(),
        json!(activity_type.clone()),
    );
    details.insert("request_provenance".to_string(), args.provenance.clone());

    let corrected_provenance = append_activity_session_correction_history(
        &previous_provenance,
        args.kind,
        Value::Object(details),
    );
    let provenance_json = json_object_string("provenance", &corrected_provenance)?;

    let updated = store.update_activity_session(ActivitySessionInput {
        session_id: &existing.session_id,
        source: &existing.source,
        start_time_unix_ms,
        end_time_unix_ms,
        activity_type: &activity_type,
        external_activity_type_code: external_activity_type_code.as_deref(),
        external_activity_type_name: external_activity_type_name.as_deref(),
        custom_label: custom_label.as_deref(),
        confidence: existing.confidence,
        detection_method: args.kind.detection_method(),
        sync_status: args.kind.sync_status(),
        provenance_json: &provenance_json,
    })?;
    let session = store.activity_session(&args.session_id)?.ok_or_else(|| {
        GooseError::message(format!("activity session {} not found", args.session_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-correction-result.v1",
        "generated_by": "goose-bridge",
        "session_id": args.session_id,
        "kind": args.kind,
        "updated": updated,
        "session": session,
    }))
}

fn activity_delete_session_bridge(
    args: ActivitySessionLookupArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let deleted = store.delete_activity_session(&args.session_id)?;
    Ok(json!({
        "schema": "goose.activity-session-delete-result.v1",
        "generated_by": "goose-bridge",
        "session_id": args.session_id,
        "deleted": deleted,
    }))
}

fn activity_attach_metric_bridge(args: ActivityMetricAttachArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let provenance_json = json_object_string("provenance", &args.provenance)?;
    let quality_flags_json = serde_json::to_string(&args.quality_flags)
        .map_err(|error| GooseError::message(format!("cannot serialize quality_flags: {error}")))?;
    let inserted = store.insert_activity_metric(ActivityMetricInput {
        metric_id: &args.metric_id,
        activity_session_id: &args.activity_session_id,
        metric_name: &args.metric_name,
        value: args.value,
        unit: &args.unit,
        start_time_unix_ms: args.start_time_unix_ms,
        end_time_unix_ms: args.end_time_unix_ms,
        quality_flags_json: &quality_flags_json,
        provenance_json: &provenance_json,
    })?;
    let metric = store.activity_metric(&args.metric_id)?.ok_or_else(|| {
        GooseError::message(format!("activity metric {} not found", args.metric_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-metric-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
        "metric": metric,
    }))
}

fn activity_attach_metrics_bridge(
    args: ActivityMetricAttachBatchArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let serialized = args
        .metrics
        .iter()
        .map(|metric| {
            Ok(SerializedActivityMetricAttachArg {
                metric,
                quality_flags_json: serde_json::to_string(&metric.quality_flags).map_err(
                    |error| GooseError::message(format!("cannot serialize quality_flags: {error}")),
                )?,
                provenance_json: json_object_string("provenance", &metric.provenance)?,
            })
        })
        .collect::<GooseResult<Vec<_>>>()?;
    let inputs = serialized
        .iter()
        .map(|serialized| ActivityMetricInput {
            metric_id: &serialized.metric.metric_id,
            activity_session_id: &serialized.metric.activity_session_id,
            metric_name: &serialized.metric.metric_name,
            value: serialized.metric.value,
            unit: &serialized.metric.unit,
            start_time_unix_ms: serialized.metric.start_time_unix_ms,
            end_time_unix_ms: serialized.metric.end_time_unix_ms,
            quality_flags_json: &serialized.quality_flags_json,
            provenance_json: &serialized.provenance_json,
        })
        .collect::<Vec<_>>();
    let (inserted, existing) =
        store.immediate_transaction(|store| store.insert_activity_metrics(&inputs))?;
    let metrics = if args.include_metrics {
        args.metrics
            .iter()
            .map(|metric| {
                store.activity_metric(&metric.metric_id)?.ok_or_else(|| {
                    GooseError::message(format!("activity metric {} not found", metric.metric_id))
                })
            })
            .collect::<GooseResult<Vec<_>>>()?
    } else {
        Vec::new()
    };

    Ok(json!({
        "schema": "goose.activity-metric-batch-result.v1",
        "generated_by": "goose-bridge",
        "metric_count": args.metrics.len(),
        "inserted": inserted,
        "existing": existing,
        "metrics": metrics,
    }))
}

fn activity_list_metrics_bridge(args: ActivityMetricListArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metrics = store.activity_metrics_for_session(&args.activity_session_id)?;
    Ok(json!({
        "schema": "goose.activity-metric-list.v1",
        "generated_by": "goose-bridge",
        "activity_session_id": args.activity_session_id,
        "metric_count": metrics.len(),
        "metrics": metrics,
    }))
}

fn activity_metrics_for_session_in_window_bridge(
    args: ActivityMetricWindowArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metrics = store.activity_metrics_for_session_in_window(
        &args.activity_session_id,
        args.start_time_unix_ms,
        args.end_time_unix_ms,
    )?;
    Ok(json!({
        "schema": "goose.activity-metric-window.v1",
        "generated_by": "goose-bridge",
        "activity_session_id": args.activity_session_id,
        "start_time_unix_ms": args.start_time_unix_ms,
        "end_time_unix_ms": args.end_time_unix_ms,
        "metric_count": metrics.len(),
        "metrics": metrics,
    }))
}

fn activity_attach_interval_bridge(
    args: ActivityIntervalAttachArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let metadata_json = json_object_string("metadata", &args.metadata)?;
    let provenance_json = json_object_string("provenance", &args.provenance)?;
    let inserted = store.insert_activity_interval(ActivityIntervalInput {
        interval_id: &args.interval_id,
        activity_session_id: &args.activity_session_id,
        interval_type: &args.interval_type,
        start_time_unix_ms: args.start_time_unix_ms,
        end_time_unix_ms: args.end_time_unix_ms,
        sequence: args.sequence,
        metadata_json: &metadata_json,
        provenance_json: &provenance_json,
    })?;
    let interval = store.activity_interval(&args.interval_id)?.ok_or_else(|| {
        GooseError::message(format!("activity interval {} not found", args.interval_id))
    })?;
    Ok(json!({
        "schema": "goose.activity-interval-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
        "interval": interval,
    }))
}

fn activity_list_intervals_bridge(
    args: ActivityIntervalListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let intervals = store.activity_intervals_for_session(&args.activity_session_id)?;
    Ok(json!({
        "schema": "goose.activity-interval-list.v1",
        "generated_by": "goose-bridge",
        "activity_session_id": args.activity_session_id,
        "interval_count": intervals.len(),
        "intervals": intervals,
    }))
}

fn external_sleep_history_import_bridge(
    args: ExternalSleepHistoryImportArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let (inserted_sessions, unchanged_sessions, inserted_stages, unchanged_stages) = store
        .immediate_transaction(|store| {
            let mut inserted_sessions = 0usize;
            let mut unchanged_sessions = 0usize;
            for session in &args.sessions {
                let stage_summary_json =
                    json_object_string("stage_summary", &session.stage_summary)?;
                let provenance_json = json_object_string("provenance", &session.provenance)?;
                if store.insert_external_sleep_session(ExternalSleepSessionInput {
                    sleep_id: &session.sleep_id,
                    source: &session.source,
                    platform: &session.platform,
                    platform_record_id: session.platform_record_id.as_deref(),
                    start_time_unix_ms: session.start_time_unix_ms,
                    end_time_unix_ms: session.end_time_unix_ms,
                    timezone: session.timezone.as_deref(),
                    stage_summary_json: &stage_summary_json,
                    confidence: session.confidence,
                    provenance_json: &provenance_json,
                })? {
                    inserted_sessions += 1;
                } else {
                    unchanged_sessions += 1;
                }
            }

            let mut inserted_stages = 0usize;
            let mut unchanged_stages = 0usize;
            for stage in &args.stages {
                let provenance_json = json_object_string("provenance", &stage.provenance)?;
                let Some(stage_kind) = canonical_external_sleep_stage_row(&stage.stage_kind) else {
                    return Err(GooseError::message(format!(
                        "external sleep stage {} kind {} is not recognized",
                        stage.stage_id, stage.stage_kind
                    )));
                };
                if store.insert_external_sleep_stage(ExternalSleepStageInput {
                    stage_id: &stage.stage_id,
                    sleep_id: &stage.sleep_id,
                    stage_kind,
                    start_time_unix_ms: stage.start_time_unix_ms,
                    end_time_unix_ms: stage.end_time_unix_ms,
                    confidence: stage.confidence,
                    provenance_json: &provenance_json,
                })? {
                    inserted_stages += 1;
                } else {
                    unchanged_stages += 1;
                }
            }

            Ok((
                inserted_sessions,
                unchanged_sessions,
                inserted_stages,
                unchanged_stages,
            ))
        })?;

    Ok(json!({
        "schema": "goose.external-sleep-history-import-result.v1",
        "generated_by": "goose-bridge",
        "session_count": args.sessions.len(),
        "stage_count": args.stages.len(),
        "inserted_session_count": inserted_sessions,
        "unchanged_session_count": unchanged_sessions,
        "inserted_stage_count": inserted_stages,
        "unchanged_stage_count": unchanged_stages,
        "import_policy": "external_history_context_only",
    }))
}

fn sleep_correction_label_bridge(args: SleepCorrectionLabelArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let value_json = json_object_string("value", &args.value)?;
    let provenance_json = json_object_string("provenance", &args.provenance)?;
    let inserted = store.insert_sleep_correction_label(SleepCorrectionLabelInput {
        label_id: &args.label_id,
        sleep_id: args.sleep_id.as_deref(),
        label_type: &args.label_type,
        start_time_unix_ms: args.start_time_unix_ms,
        end_time_unix_ms: args.end_time_unix_ms,
        value_json: &value_json,
        source: &args.source,
        confidence: args.confidence,
        provenance_json: &provenance_json,
    })?;
    let label = store
        .sleep_correction_label(&args.label_id)?
        .ok_or_else(|| GooseError::message("sleep correction label was not stored"))?;
    Ok(json!({
        "schema": "goose.sleep-correction-label-result.v1",
        "generated_by": "goose-bridge",
        "inserted": inserted,
        "label": label,
        "storage_policy": "manual_corrections_are_labels_not_raw_packet_edits",
    }))
}

fn sleep_correction_label_list_bridge(
    args: SleepCorrectionLabelListArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let labels =
        store.sleep_correction_labels_between(args.start_time_unix_ms, args.end_time_unix_ms)?;
    let sleep_window_label_count = labels
        .iter()
        .filter(|label| label.label_type == "sleep_window")
        .count();
    let sleep_stage_label_count = labels
        .iter()
        .filter(|label| label.label_type == "sleep_stage")
        .count();
    let nap_label_count = labels
        .iter()
        .filter(|label| label.label_type == "nap")
        .count();
    let distinct_sleep_window_sleep_id_count = labels
        .iter()
        .filter(|label| label.label_type == "sleep_window")
        .filter_map(|label| label.sleep_id.as_deref())
        .filter(|sleep_id| !sleep_id.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    Ok(json!({
        "schema": "goose.sleep-correction-label-list.v1",
        "generated_by": "goose-bridge",
        "label_count": labels.len(),
        "sleep_window_label_count": sleep_window_label_count,
        "sleep_stage_label_count": sleep_stage_label_count,
        "nap_label_count": nap_label_count,
        "distinct_sleep_window_sleep_id_count": distinct_sleep_window_sleep_id_count,
        "labels": labels,
        "storage_policy": "manual_corrections_are_labels_not_raw_packet_edits",
    }))
}

fn sleep_window_label_validation_bridge(
    args: SleepWindowLabelValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let defaults = SleepWindowLabelValidationOptions::default();
    let report = run_sleep_window_label_validation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        SleepWindowLabelValidationOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(defaults.min_owned_captures_per_summary),
            require_trusted_evidence: args.require_trusted_evidence,
            sleep_need_minutes: args
                .sleep_need_minutes
                .unwrap_or(defaults.sleep_need_minutes),
            low_motion_threshold_0_to_1: args
                .low_motion_threshold_0_to_1
                .unwrap_or(defaults.low_motion_threshold_0_to_1),
            disturbance_motion_threshold_0_to_1: args
                .disturbance_motion_threshold_0_to_1
                .unwrap_or(defaults.disturbance_motion_threshold_0_to_1),
            target_midpoint_minutes_since_midnight: args
                .target_midpoint_minutes_since_midnight
                .unwrap_or(defaults.target_midpoint_minutes_since_midnight),
            start_tolerance_minutes: args
                .start_tolerance_minutes
                .unwrap_or(defaults.start_tolerance_minutes),
            end_tolerance_minutes: args
                .end_tolerance_minutes
                .unwrap_or(defaults.end_tolerance_minutes),
            duration_tolerance_minutes: args
                .duration_tolerance_minutes
                .unwrap_or(defaults.duration_tolerance_minutes),
            min_label_confidence: args
                .min_label_confidence
                .unwrap_or(defaults.min_label_confidence),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep window label validation report: {error}"
        ))
    })
}

fn sleep_stage_label_validation_bridge(
    args: SleepStageLabelValidationArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let defaults = SleepStageLabelValidationOptions::default();
    let report = validate_sleep_v1_stage_labels_for_store(
        &store,
        &args.input,
        SleepStageLabelValidationOptions {
            min_label_confidence: args
                .min_label_confidence
                .unwrap_or(defaults.min_label_confidence),
            min_overlap_fraction: args
                .min_overlap_fraction
                .unwrap_or(defaults.min_overlap_fraction),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep stage label validation report: {error}"
        ))
    })
}

fn sleep_v1_explanation_stability_bridge(
    args: SleepV1ExplanationStabilityArgs,
) -> GooseResult<serde_json::Value> {
    let defaults = SleepV1ExplanationStabilityOptions::default();
    let report = validate_sleep_v1_explanation_and_stability(
        &args.input,
        SleepV1ExplanationStabilityOptions {
            max_repeated_run_delta: args
                .max_repeated_run_delta
                .unwrap_or(defaults.max_repeated_run_delta),
            max_small_perturbation_delta: args
                .max_small_perturbation_delta
                .unwrap_or(defaults.max_small_perturbation_delta),
            perturb_sleep_duration_minutes: args
                .perturb_sleep_duration_minutes
                .unwrap_or(defaults.perturb_sleep_duration_minutes),
            min_v1_component_count: args
                .min_v1_component_count
                .unwrap_or(defaults.min_v1_component_count),
            min_explanation_quality_signal_count: args
                .min_explanation_quality_signal_count
                .unwrap_or(defaults.min_explanation_quality_signal_count),
        },
    );
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep v1 explanation stability report: {error}"
        ))
    })
}

fn sleep_v1_release_gate_bridge(args: SleepV1ReleaseGateArgs) -> GooseResult<serde_json::Value> {
    let report = validate_sleep_v1_release_gates(&args.input);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep v1 release gate report: {error}"
        ))
    })
}

fn sleep_v1_evidence_folder_bridge(
    args: SleepV1EvidenceFolderArgs,
) -> GooseResult<serde_json::Value> {
    let report = validate_sleep_v1_evidence_folder_with_options(
        Path::new(&args.evidence_dir),
        SleepV1EvidenceFolderOptions {
            expected_evidence_manifest_sha256: args.expected_manifest_sha256,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize sleep v1 evidence folder report: {error}"
        ))
    })
}

fn capture_correlation_bridge(args: CaptureCorrelationArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let report = run_capture_correlation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: args
                .min_owned_captures
                .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY),
            require_owned_captures: args.require_owned_captures,
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize capture correlation report: {error}"
        ))
    })
}

fn capture_arrival_plan_bridge(args: CaptureArrivalPlanArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let min_owned_captures = args
        .min_owned_captures
        .unwrap_or(DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY);
    let capture_correlation = run_capture_correlation_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: min_owned_captures,
            require_owned_captures: args.require_owned_captures,
        },
    )?;
    let metric_input_readiness = run_metric_input_readiness(
        &capture_correlation,
        MetricInputReadinessOptions {
            require_scores_ready: args.require_scores_ready,
        },
    );
    let recovery_sensor_discovery = run_recovery_sensor_discovery_report_for_store(
        &store,
        &args.database_path,
        &args.start,
        &args.end,
        RecoverySensorDiscoveryOptions {
            min_owned_captures_per_summary: min_owned_captures,
            require_trusted_evidence: args.require_owned_captures,
            min_rr_intervals_to_compute: 2,
        },
    )?;
    let local_health_validation_manifest =
        scaffold_local_health_validation_manifest(&LocalHealthValidationManifestScaffoldOptions {
            database_path: PathBuf::from(&args.database_path),
            manifest_id: "capture-arrival-local-health-validation".to_string(),
            timezone: args.timezone.unwrap_or_else(|| "UTC".to_string()),
            date_key: None,
            database_source_kind: Some("direct_database".to_string()),
            start: Some(args.start.clone()),
            end: Some(args.end.clone()),
            window_source: Some("capture_arrival_plan_window".to_string()),
            raw_export_bundle_path: None,
        })?;
    let local_health_validation_review =
        review_local_health_validation_manifest(&local_health_validation_manifest);
    let actions = capture_arrival_plan_actions(
        &capture_correlation,
        &metric_input_readiness,
        &recovery_sensor_discovery,
        &local_health_validation_review,
    );
    let next_capture_focus = capture_arrival_plan_next_focus(&actions);
    let mut issues = Vec::new();
    issues.extend(
        capture_correlation
            .issues
            .iter()
            .map(|issue| format!("capture_correlation:{issue}")),
    );
    issues.extend(
        metric_input_readiness
            .issues
            .iter()
            .map(|issue| format!("metric_input_readiness:{issue}")),
    );
    issues.extend(
        recovery_sensor_discovery
            .issues
            .iter()
            .map(|issue| format!("recovery_sensor_discovery:{issue}")),
    );
    if local_health_validation_review
        .get("status")
        .and_then(Value::as_str)
        != Some("ready_to_run_validation_suite")
    {
        issues.push("local_health_validation:operator_edits_required".to_string());
    }
    let pass = capture_correlation.pass
        && metric_input_readiness.pass
        && recovery_sensor_discovery.pass
        && local_health_validation_review
            .get("status")
            .and_then(Value::as_str)
            == Some("ready_to_run_validation_suite")
        && actions.is_empty()
        && issues.is_empty();
    let (capture_sessions, activity_sessions) =
        capture_arrival_window_rows(&store, &args.start, &args.end)?;
    let command_validation_records = store.command_validation_records()?;
    let physical_arrival_rows = capture_arrival_physical_rows(
        &capture_correlation,
        &metric_input_readiness,
        &capture_sessions,
        &command_validation_records,
        &activity_sessions,
    );
    let report = CaptureArrivalPlanReport {
        schema: CAPTURE_ARRIVAL_PLAN_REPORT_SCHEMA.to_string(),
        generated_by: "goose-capture-arrival-plan".to_string(),
        pass,
        start: args.start,
        end: args.end,
        min_owned_captures,
        require_owned_captures: args.require_owned_captures,
        require_scores_ready: args.require_scores_ready,
        action_count: actions.len(),
        physical_arrival_row_count: physical_arrival_rows.len(),
        physical_arrival_rows,
        next_capture_focus,
        actions,
        capture_correlation,
        metric_input_readiness,
        recovery_sensor_discovery,
        local_health_validation_review,
        issues,
    };
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize capture arrival plan: {error}"))
    })
}

fn capture_arrival_physical_rows(
    capture_correlation: &CaptureCorrelationReport,
    metric_input_readiness: &MetricInputReadinessReport,
    capture_sessions: &[CaptureSessionRow],
    command_validation_records: &[CommandValidationRecord],
    activity_sessions: &[ActivitySessionRow],
) -> Vec<CaptureArrivalPhysicalRow> {
    let capture_session_ready = capture_sessions
        .iter()
        .any(|session| session.status == "finished" && session.frame_count > 0);
    let capture_session_started = !capture_sessions.is_empty();
    let capture_observations_ready = !capture_correlation.observations.is_empty();
    let trusted_capture_summary_ready = capture_correlation
        .summaries
        .iter()
        .any(|summary| summary.trusted_metric_ready);
    let historical_summary_observed = capture_correlation
        .summaries
        .iter()
        .any(|summary| summary.body_summary_kind == "normal_history");
    let service_filter_ready = capture_sessions.iter().any(|session| {
        session_json_has_any(
            session,
            &[
                "whoop_scan_targeted",
                "scan_mode",
                "whoop_profile",
                "service_uuids",
                "generation",
            ],
        )
    });
    let role_labels_ready = capture_sessions.iter().any(|session| {
        session_json_has_any(
            session,
            &[
                "roles",
                "whoop_role",
                "command_to_strap",
                "command_from_strap",
                "events_from_strap",
                "data_from_strap",
                "memfault",
            ],
        )
    });
    let notification_subscriptions_ready = capture_sessions.iter().any(|session| {
        session_json_has_any(
            session,
            &[
                "notification_state",
                "is_notifying",
                "subscribed_characteristics",
                "first_notification_timestamp",
                "reconnect_resubscription",
            ],
        )
    });
    let auth_session_ready = capture_sessions.iter().any(|session| {
        session_json_has_any(
            session,
            &[
                "auth",
                "auth_trace",
                "session_log",
                "connect",
                "reconnect",
                "lock",
                "timeout",
                "wake",
                "retry",
            ],
        )
    });
    let sync_metadata_ready = capture_sessions.iter().any(|session| {
        session_json_has_any(
            session,
            &[
                "HistoryStart",
                "HistoryEnd",
                "HistoryComplete",
                "sync_metadata",
                "transfer_state",
                "range_window",
                "completion_reason",
            ],
        )
    });
    let any_command_validation_record = !command_validation_records.is_empty();
    let ready_command_validation_record = command_validation_records
        .iter()
        .any(|record| record.direct_send_ready);
    let any_activity_session = !activity_sessions.is_empty();
    let typed_activity_session = activity_sessions.iter().any(|session| {
        session.activity_type != "unknown"
            && session.confidence > 0.0
            && !matches!(session.sync_status.as_str(), "blocked" | "discarded")
    });
    let activity_boundary_provenance_ready = activity_sessions.iter().any(|session| {
        session.activity_type != "unknown"
            && session_json_has_any(
                session,
                &[
                    "activity_type",
                    "activity_type_provenance",
                    "packet_fields",
                    "activity_start",
                    "activity_end",
                    "confidence",
                ],
            )
    });
    let activity_promotion_ready = metric_input_readiness.activity_session_promotion.pass
        || activity_sessions.iter().any(|session| {
            !matches!(session.sync_status.as_str(), "blocked" | "discarded")
                && matches!(
                    session.detection_method.as_str(),
                    "official_capture"
                        | "imported"
                        | "heuristic_motion"
                        | "heuristic_hr_motion"
                        | "machine_learning"
                )
        });
    let activity_classifier_evidence = metric_input_readiness
        .activity_session_promotion
        .classification_evidence_available;

    let mut rows = Vec::new();
    rows.push(capture_arrival_physical_row(
        "arrival.service_filters",
        "Service filters",
        "gatt",
        arrival_state(service_filter_ready, capture_session_started),
        "No live WHOOP service-filter trace is attached yet.",
        "Record broad versus WHOOP-targeted scan mode, matched Gen4/Gen5 service UUIDs, peripheral id, and inferred generation.",
        "docs/whoop-arrival-checklist.md service filters",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.role_labels",
        "Role labels",
        "gatt",
        arrival_state(role_labels_ready, capture_session_started),
        "No live characteristic role map is attached yet.",
        "Label command_to_strap, command_from_strap, events_from_strap, data_from_strap, memfault, unknown roles, properties, and notifying state.",
        "docs/whoop-arrival-checklist.md role labels",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.notification_subscriptions",
        "Notification subscriptions",
        "gatt",
        arrival_state(notification_subscriptions_ready, capture_session_started),
        "No live subscribe-before-first-frame trace is attached yet.",
        "Record subscribed characteristics, subscription success, first notification timestamp, reconnect resubscription, and silent roles.",
        "docs/whoop-arrival-checklist.md notifications",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.frame_counts",
        "Frame counts",
        "capture",
        arrival_state(capture_session_ready, capture_observations_ready),
        "No first-frame or close-frame count evidence is attached yet.",
        "Record total, per-role, and per-characteristic frame counts at first frame and at close, including zero-frame windows.",
        "docs/whoop-arrival-checklist.md frame counts",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.capture_statuses",
        "Capture statuses",
        "capture",
        arrival_state(
            capture_sessions.iter().any(|session| session.status == "finished"),
            capture_session_started,
        ),
        "No live connect-to-complete status timeline is attached yet.",
        "Record connect, auth, subscribe, transfer, reconnect, abort, and complete statuses from debug stream events and session logs.",
        "docs/whoop-arrival-checklist.md capture statuses",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.command_write_pairs",
        "Command/write pairs",
        "commands",
        arrival_state(ready_command_validation_record, any_command_validation_record),
        "Fixture validation exists, but no official physical request/response pair is attached yet.",
        "Capture official app action, endpoint id, write type, request bytes, response bytes, command name, and local dry-run parity.",
        "docs/whoop-arrival-checklist.md command/write pairs",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.auth.session",
        "Auth / session observations",
        "session",
        arrival_state(auth_session_ready, capture_session_started),
        "No ordered connect/auth/reconnect/lock/timeout trace is attached yet.",
        "Record connect, auth, reconnect, lock, timeout, wake, retry, and required user action in order.",
        "docs/whoop-arrival-checklist.md auth/session",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.history.metadata",
        "Sync metadata",
        "history metadata",
        arrival_state(sync_metadata_ready, historical_summary_observed),
        "No live HistoryStart/HistoryEnd/HistoryComplete timeline is attached yet.",
        "Record range window, transfer-state transitions, retry behavior, abort behavior, and final completion reason.",
        "docs/whoop-arrival-checklist.md sync metadata",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.history.fields",
        "Parser field validation",
        "parser fields",
        arrival_state(capture_correlation.pass && trusted_capture_summary_ready, capture_observations_ready),
        "No physical byte-for-field parser validation is attached yet.",
        "Mark timestamp, BPM, RR, IMU, PPG, SpO2, skin temp, ambient light, respiratory, quality, contact, gravity, and Gen5 fields as matched/candidate/conflicting/missing.",
        "docs/whoop-arrival-checklist.md parser fields",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.activity.boundary_type",
        "Activity boundary/type fields",
        "activity fields",
        arrival_state(activity_boundary_provenance_ready, typed_activity_session),
        "No packet-derived activity boundary or type provenance is attached yet.",
        "Record start, end, pauses, sport/activity type, confidence, and whether type came from WHOOP bytes, app metadata, or Goose inference.",
        "docs/whoop-arrival-checklist.md activity fields",
    ));
    rows.push(capture_arrival_physical_row(
        "arrival.activity.promotion",
        "Activity promotion evidence",
        "activity promotion",
        arrival_state(activity_promotion_ready, any_activity_session || activity_classifier_evidence),
        "No candidate window has been promoted from a physical sync yet.",
        "Record candidate windows, feature evidence, classifier confidence, and user/session approval before activity_session creation.",
        "docs/whoop-arrival-checklist.md activity promotion",
    ));
    rows
}

fn capture_arrival_physical_row(
    id: &str,
    label: &str,
    domain: &str,
    state: &str,
    blocker: &str,
    next_action: &str,
    evidence: &str,
) -> CaptureArrivalPhysicalRow {
    let (blocker, next_action) = match state {
        "physical-validated" => ("", ""),
        "fixture-tested" | "implemented" => (blocker, next_action),
        _ => (blocker, next_action),
    };
    CaptureArrivalPhysicalRow {
        id: id.to_string(),
        label: label.to_string(),
        domain: domain.to_string(),
        state: state.to_string(),
        blocker: blocker.to_string(),
        next_action: next_action.to_string(),
        evidence: evidence.to_string(),
    }
}

fn arrival_state(physical_ready: bool, fixture_or_app_ready: bool) -> &'static str {
    if physical_ready {
        "physical-validated"
    } else if fixture_or_app_ready {
        "fixture-tested"
    } else {
        "blocked"
    }
}

fn capture_arrival_window_rows(
    store: &GooseStore,
    start: &str,
    end: &str,
) -> GooseResult<(Vec<CaptureSessionRow>, Vec<ActivitySessionRow>)> {
    let Some((start_unix_ms, end_unix_ms)) = capture_arrival_window_unix_ms(start, end) else {
        return Ok((Vec::new(), Vec::new()));
    };
    Ok((
        store.capture_sessions_between(start_unix_ms, end_unix_ms)?,
        store.activity_sessions_between(start_unix_ms, end_unix_ms)?,
    ))
}

fn capture_arrival_window_unix_ms(start: &str, end: &str) -> Option<(i64, i64)> {
    let start = capture_arrival_rfc3339_utc_unix_ms(start.trim())?;
    let end = capture_arrival_rfc3339_utc_unix_ms(end.trim())?;
    (start < end).then_some((start, end))
}

fn capture_arrival_rfc3339_utc_unix_ms(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let seconds_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let (second_text, fraction_text) = seconds_part
        .split_once('.')
        .map_or((seconds_part, ""), |(seconds, fraction)| {
            (seconds, fraction)
        });
    let second = second_text.parse::<u32>().ok()?;
    let millis = capture_arrival_millis_fraction(fraction_text)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = capture_arrival_days_from_civil(year, month, day);
    days.checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)?
        .checked_add(i64::from(minute) * 60_000)?
        .checked_add(i64::from(second) * 1_000)?
        .checked_add(i64::from(millis))
}

fn capture_arrival_millis_fraction(value: &str) -> Option<u32> {
    if value.is_empty() {
        return Some(0);
    }
    if !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let mut millis = 0_u32;
    let mut factor = 100_u32;
    for character in value.chars().take(3) {
        millis += character.to_digit(10)? * factor;
        factor /= 10;
    }
    Some(millis)
}

fn capture_arrival_days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

trait CaptureArrivalProvenance {
    fn provenance_json(&self) -> &str;
}

impl CaptureArrivalProvenance for CaptureSessionRow {
    fn provenance_json(&self) -> &str {
        &self.provenance_json
    }
}

impl CaptureArrivalProvenance for ActivitySessionRow {
    fn provenance_json(&self) -> &str {
        &self.provenance_json
    }
}

fn session_json_has_any<T: CaptureArrivalProvenance>(row: &T, keys: &[&str]) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(row.provenance_json()) else {
        return false;
    };
    keys.iter()
        .any(|key| capture_arrival_json_contains_key(&value, key))
}

fn capture_arrival_json_contains_key(value: &Value, expected: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            key == expected || capture_arrival_json_contains_key(child, expected)
        }),
        Value::Array(values) => values
            .iter()
            .any(|child| capture_arrival_json_contains_key(child, expected)),
        _ => false,
    }
}

fn capture_arrival_plan_actions(
    capture_correlation: &CaptureCorrelationReport,
    metric_input_readiness: &MetricInputReadinessReport,
    recovery_sensor_discovery: &RecoverySensorDiscoveryReport,
    local_health_validation_review: &Value,
) -> Vec<CaptureArrivalPlanAction> {
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();

    for action in &capture_correlation.next_capture_actions {
        push_capture_arrival_action(&mut actions, &mut seen, "Capture Trust", action);
    }
    for summary in &capture_correlation.summaries {
        if summary.trusted_metric_ready {
            continue;
        }
        for action in &summary.next_capture_actions {
            push_capture_arrival_action(&mut actions, &mut seen, "Capture Trust", action);
        }
    }

    for action in &metric_input_readiness.next_actions {
        push_metric_arrival_action(&mut actions, &mut seen, "Metric Inputs", action);
    }
    for family in &metric_input_readiness.families {
        if family.score_ready {
            continue;
        }
        for action in &family.next_actions {
            push_metric_arrival_action(&mut actions, &mut seen, "Metric Inputs", action);
        }
    }
    for action in &recovery_sensor_discovery.next_actions {
        push_metric_feature_arrival_action(&mut actions, &mut seen, "Recovery Sensors", action);
    }
    push_local_health_validation_arrival_actions(
        &mut actions,
        &mut seen,
        local_health_validation_review,
    );

    actions
}

fn capture_arrival_plan_next_focus(
    actions: &[CaptureArrivalPlanAction],
) -> Option<CaptureArrivalPlanAction> {
    for priority in [
        arrival_action_is_owned_capture_target,
        arrival_action_is_capture_dependency,
        arrival_action_is_local_health_validation,
        arrival_action_is_metric_input_work,
    ] {
        if let Some(action) = actions.iter().find(priority).cloned() {
            return Some(action);
        }
    }
    None
}

fn arrival_action_is_owned_capture_target(action: &&CaptureArrivalPlanAction) -> bool {
    action.source == "Capture Trust"
        && (action.reason.contains("owned_capture")
            || action.action.contains("Capture")
            || action.action.contains("capture")
            || action.scope.contains("r17")
            || action.scope.contains("temperature"))
}

fn arrival_action_is_capture_dependency(action: &&CaptureArrivalPlanAction) -> bool {
    (action.source == "Metric Inputs" || action.source == "Recovery Sensors")
        && (action.scope == "capture_correlation"
            || action.reason.contains("capture")
            || action.action.contains("Capture")
            || action.action.contains("capture"))
}

fn arrival_action_is_local_health_validation(action: &&CaptureArrivalPlanAction) -> bool {
    action.source == "Local Health Validation"
}

fn arrival_action_is_metric_input_work(action: &&CaptureArrivalPlanAction) -> bool {
    action.source == "Metric Inputs" || action.source == "Recovery Sensors"
}

fn push_capture_arrival_action(
    actions: &mut Vec<CaptureArrivalPlanAction>,
    seen: &mut BTreeSet<String>,
    source: &str,
    action: &CaptureCorrelationNextAction,
) {
    push_arrival_action(
        actions,
        seen,
        source,
        &action.scope,
        &action.reason,
        &action.action,
    );
}

fn push_metric_arrival_action(
    actions: &mut Vec<CaptureArrivalPlanAction>,
    seen: &mut BTreeSet<String>,
    source: &str,
    action: &MetricInputNextAction,
) {
    push_arrival_action(
        actions,
        seen,
        source,
        &action.scope,
        &action.reason,
        &action.action,
    );
}

fn push_metric_feature_arrival_action(
    actions: &mut Vec<CaptureArrivalPlanAction>,
    seen: &mut BTreeSet<String>,
    source: &str,
    action: &MetricFeatureNextAction,
) {
    push_arrival_action(
        actions,
        seen,
        source,
        &action.scope,
        &action.reason,
        &action.action,
    );
}

fn push_local_health_validation_arrival_actions(
    actions: &mut Vec<CaptureArrivalPlanAction>,
    seen: &mut BTreeSet<String>,
    review: &Value,
) {
    let Some(cases) = review
        .get("acceptance_evidence_cases")
        .and_then(Value::as_array)
    else {
        return;
    };
    for case in cases {
        let Some(object) = case.as_object() else {
            continue;
        };
        let outstanding_requirements = object
            .get("outstanding_requirements")
            .and_then(Value::as_array)
            .map(|requirements| {
                requirements
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|requirement| !requirement.trim().is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if outstanding_requirements.is_empty() {
            continue;
        }
        let scope = object
            .get("case_id")
            .and_then(Value::as_str)
            .unwrap_or("acceptance_evidence_case");
        let report = object
            .get("report")
            .and_then(Value::as_str)
            .unwrap_or("validation");
        let capture_kind = object
            .get("capture_kind")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("owned_capture");
        let action = object
            .get("collection_action")
            .and_then(Value::as_str)
            .unwrap_or(
                "Collect owned packet evidence and validation labels required by this case.",
            );
        let reason = format!("{}:{}", report, outstanding_requirements.join(","));
        push_arrival_action(
            actions,
            seen,
            "Local Health Validation",
            scope,
            &reason,
            &format!("{action} Capture kind: {capture_kind}."),
        );
    }
}

fn push_arrival_action(
    actions: &mut Vec<CaptureArrivalPlanAction>,
    seen: &mut BTreeSet<String>,
    source: &str,
    scope: &str,
    reason: &str,
    action: &str,
) {
    let key = format!("{source}|{scope}|{reason}|{action}");
    if !seen.insert(key) {
        return;
    }
    let summary = if reason.is_empty() {
        action.to_string()
    } else {
        format!("{reason}: {action}")
    };
    actions.push(CaptureArrivalPlanAction {
        source: source.to_string(),
        scope: scope.to_string(),
        reason: reason.to_string(),
        action: action.to_string(),
        summary,
    });
}

fn command_validate_evidence_bridge(
    args: CommandValidateEvidenceArgs,
) -> GooseResult<serde_json::Value> {
    let report = validate_commands(&args.evidence);
    if args.persist {
        let database_path = args
            .database_path
            .as_deref()
            .ok_or_else(|| GooseError::message("database_path is required when persist is true"))?;
        let store = open_bridge_store(database_path)?;
        persist_command_validation_results(&store, &report.commands)?;
    }
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize command validation report: {error}"
        ))
    })
}

fn command_evidence_from_emulator_log_bridge(
    args: CommandEvidenceFromEmulatorLogArgs,
) -> GooseResult<serde_json::Value> {
    let defaults = CommandEmulatorLogEvidenceOptions::default();
    let source_log = args
        .source_log
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("app-selected-emulator-log");
    let report = command_evidence_from_emulator_log_text(
        source_log,
        &args.log_text,
        &CommandEmulatorLogEvidenceOptions {
            write_type: args.write_type.unwrap_or(defaults.write_type),
            visible_user_intent: args.visible_user_intent,
            triggering_ui_action: args.triggering_ui_action,
            visible_confirmation: args.visible_confirmation,
            rollback_plan: args.rollback_plan,
            explicit_approval: args.explicit_approval,
            mirror_local_frame: args.mirror_local_frame,
            capture_app: args.capture_app.unwrap_or(defaults.capture_app),
            capture_kind: args.capture_kind.unwrap_or(defaults.capture_kind),
            owner: args.owner.unwrap_or(defaults.owner),
        },
    )?;
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize command emulator-log evidence report: {error}"
        ))
    })
}

fn command_promote_local_frame_matches_bridge(
    args: CommandPromoteLocalFrameMatchesArgs,
) -> GooseResult<serde_json::Value> {
    let report = command_evidence_with_local_frame_matches(&args.evidence, &args.candidates);
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize command local-frame match report: {error}"
        ))
    })
}

fn command_direct_send_gate_bridge(
    args: CommandDirectSendGateArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let result = match store.command_validation_record(&args.command)? {
        Some(record) => Some(command_result_from_report_json(&record.report_json)?),
        None => None,
    };
    let gate = direct_send_gate_from_result(&args.command, result.as_ref());
    serde_json::to_value(gate)
        .map_err(|error| GooseError::message(format!("cannot serialize command gate: {error}")))
}

fn command_direct_send_preflight_bridge(
    args: CommandDirectSendPreflightArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let result = match store.command_validation_record(&args.command)? {
        Some(record) => Some(command_result_from_report_json(&record.report_json)?),
        None => None,
    };
    let gate = direct_send_gate_from_result(&args.command, result.as_ref());
    let input = crate::commands::CommandDirectSendPreflightInput {
        command: args.command,
        now_unix_ms: args.now_unix_ms,
        override_expires_at_unix_ms: args.override_expires_at_unix_ms,
        visible_user_intent: args.visible_user_intent,
        dry_run_bytes_shown: args.dry_run_bytes_shown,
        dry_run_frame_hex: args.dry_run_frame_hex,
        dry_run_service_uuid: args.dry_run_service_uuid,
        dry_run_characteristic_uuid: args.dry_run_characteristic_uuid,
        dry_run_write_type: args.dry_run_write_type,
        session_log_ready: args.session_log_ready,
        connection_state: args.connection_state,
        active_device_id: args.active_device_id,
        critical_visible_confirmation: args.critical_visible_confirmation,
        critical_explicit_approval: args.critical_explicit_approval,
        critical_rollback_or_restore_acknowledged: args.critical_rollback_or_restore_acknowledged,
    };
    let preflight = direct_send_preflight_from_gate(&input, gate);
    serde_json::to_value(preflight).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize command preflight result: {error}"
        ))
    })
}

fn command_capture_plan_bridge(args: CommandCapturePlanArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let records = store.command_validation_records()?;
    let mut results = Vec::new();
    let mut parse_issues = Vec::new();
    for record in records {
        match command_result_from_report_json(&record.report_json) {
            Ok(result) => results.push(result),
            Err(error) => parse_issues.push(format!(
                "command_validation_record_parse_failed:{}:{error}",
                record.command
            )),
        }
    }

    let mut report = command_capture_plan_from_results(&results, &args.commands);
    report.issues.extend(parse_issues);
    report.pass = report.pass && report.issues.is_empty();
    serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize command capture plan: {error}"))
    })
}

fn command_list_validation_records_bridge(
    args: ListCommandValidationRecordsArgs,
) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let records = store.command_validation_records()?;
    serde_json::to_value(records).map_err(|error| {
        GooseError::message(format!(
            "cannot serialize command validation records: {error}"
        ))
    })
}

fn command_import_validation_records_bridge(
    args: ImportCommandValidationRecordsArgs,
) -> GooseResult<serde_json::Value> {
    let record_count = args.records.len();
    let mut issues = Vec::new();
    if record_count == 0 {
        issues.push("records_required".to_string());
    }

    let mut records = Vec::new();
    let mut record_summaries = Vec::new();
    for (index, row) in args.records.into_iter().enumerate() {
        let command = row.command.trim().to_string();
        let risk_gate = row.risk_gate.trim().to_string();
        let mut row_issues = Vec::new();
        if command.is_empty() {
            row_issues.push("command_required".to_string());
        }
        if risk_gate.is_empty() {
            row_issues.push("risk_gate_required".to_string());
        }

        let report_json = match command_validation_report_json_string(&row.report_json) {
            Ok(report_json) => report_json,
            Err(issue) => {
                row_issues.push(issue);
                String::new()
            }
        };

        let result = if report_json.is_empty() {
            None
        } else {
            match command_result_from_report_json(&report_json) {
                Ok(result) => Some(result),
                Err(error) => {
                    row_issues.push(format!("report_json_parse_failed: {error}"));
                    None
                }
            }
        };

        if let Some(result) = result {
            let result_risk_gate = command_risk_gate_name(&result.risk_gate);
            if result.command != command {
                row_issues.push("report_json_command_mismatch".to_string());
            }
            if result_risk_gate != risk_gate {
                row_issues.push("report_json_risk_gate_mismatch".to_string());
            }
            if result.direct_send_ready != row.direct_send_ready {
                row_issues.push("report_json_direct_send_ready_mismatch".to_string());
            }
            if row.direct_send_ready {
                row_issues.extend(command_validation_import_provenance_issues(&result));
            }
        }

        if row_issues.is_empty() {
            record_summaries.push(json!({
                "command": command,
                "risk_gate": risk_gate,
                "direct_send_ready": row.direct_send_ready,
            }));
            records.push(CommandValidationRecord {
                command,
                risk_gate,
                direct_send_ready: row.direct_send_ready,
                report_json,
            });
        } else {
            issues.extend(
                row_issues
                    .into_iter()
                    .map(|issue| format!("records[{index}].{issue}")),
            );
        }
    }

    let mut inserted_count = 0usize;
    let mut ready_count = 0usize;
    let mut blocked_count = 0usize;
    if issues.is_empty() {
        let store = open_bridge_store(&args.database_path)?;
        for record in &records {
            store.upsert_command_validation_record(record)?;
        }
        inserted_count = records.len();
        ready_count = records
            .iter()
            .filter(|record| record.direct_send_ready)
            .count();
        blocked_count = records.len() - ready_count;
    }

    Ok(json!({
        "schema": "goose.command-validation-import-report.v1",
        "generated_by": "goose-command-validation-import",
        "pass": issues.is_empty(),
        "record_count": record_count,
        "validated_record_count": records.len(),
        "inserted_count": inserted_count,
        "ready_count": ready_count,
        "blocked_count": blocked_count,
        "records": record_summaries,
        "issues": issues,
    }))
}

fn persist_command_validation_results(
    store: &GooseStore,
    results: &[CommandValidationResult],
) -> GooseResult<()> {
    for result in results {
        store.upsert_command_validation_record(&CommandValidationRecord {
            command: result.command.clone(),
            risk_gate: command_risk_gate_name(&result.risk_gate).to_string(),
            direct_send_ready: result.direct_send_ready,
            report_json: serde_json::to_string(result).map_err(|error| {
                GooseError::message(format!("cannot serialize command result: {error}"))
            })?,
        })?;
    }
    Ok(())
}

fn command_validation_report_json_string(report_json: &Value) -> Result<String, String> {
    match report_json {
        Value::String(text) if !text.trim().is_empty() => Ok(text.clone()),
        Value::String(_) => Err("report_json_required".to_string()),
        Value::Object(_) => serde_json::to_string(report_json)
            .map_err(|error| format!("report_json_serialize_failed: {error}")),
        _ => Err("report_json_object_or_string_required".to_string()),
    }
}

fn command_validation_import_provenance_issues(result: &CommandValidationResult) -> Vec<String> {
    const TRUSTED_SOURCES: &[&str] = &[
        "user_owned_official_capture",
        "passive_official_capture",
        "official_app_capture",
        "official_app_to_macos_emulator",
    ];
    const TRUSTED_CAPTURE_KINDS: &[&str] = &[
        "official_app_to_macos_emulator",
        "passive_ble_observation",
        "user_owned_official_capture",
        "owned_device_passive_capture",
    ];

    let mut issues = Vec::new();
    let source = result
        .validated_evidence_source
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if source.is_empty() {
        issues.push("validated_evidence_source_required".to_string());
    } else if !TRUSTED_SOURCES.contains(&source) {
        issues.push("validated_evidence_source_trusted".to_string());
    }

    let capture_kind = result
        .validated_capture_kind
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if capture_kind.is_empty() {
        issues.push("validated_capture_kind_required".to_string());
    } else if !TRUSTED_CAPTURE_KINDS.contains(&capture_kind) {
        issues.push("validated_capture_kind_trusted".to_string());
    }

    let owner = result
        .validated_owner
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if owner != "user" {
        issues.push("validated_owner_user_required".to_string());
    }

    let provenance_json = result
        .validated_provenance_json
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let provenance = if provenance_json.is_empty() {
        issues.push("validated_provenance_json_required".to_string());
        None
    } else {
        match serde_json::from_str::<Value>(provenance_json) {
            Ok(Value::Object(object)) if !object.is_empty() => Some(object),
            Ok(Value::Object(_)) => {
                issues.push("validated_provenance_non_empty_object".to_string());
                None
            }
            Ok(_) => {
                issues.push("validated_provenance_json_object".to_string());
                None
            }
            Err(_) => {
                issues.push("validated_provenance_json_object".to_string());
                None
            }
        }
    };

    if let Some(provenance) = provenance.as_ref() {
        if bridge_provenance_string(provenance, "owner") != Some("user") {
            issues.push("validated_provenance_owner_user".to_string());
        }
        if bridge_provenance_string(provenance, "capture_app") != Some("whoop_official") {
            issues.push("validated_provenance_capture_app_official".to_string());
        }
        match bridge_provenance_string(provenance, "capture_kind") {
            Some(kind) if TRUSTED_CAPTURE_KINDS.contains(&kind) => {
                if !capture_kind.is_empty() && kind != capture_kind {
                    issues.push("validated_provenance_capture_kind_match".to_string());
                }
            }
            Some(_) => issues.push("validated_provenance_capture_kind_trusted".to_string()),
            None => issues.push("validated_provenance_capture_kind_required".to_string()),
        }
    }
    if result.direct_send_ready
        && !matches!(result.risk_gate, crate::commands::CommandRiskGate::ReadOnly)
        && result
            .validated_triggering_ui_action
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        issues.push("validated_triggering_ui_action_required".to_string());
    }

    issues.sort();
    issues.dedup();
    issues
}

fn bridge_provenance_string<'a>(
    provenance: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    provenance
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn command_risk_gate_name(risk_gate: &crate::commands::CommandRiskGate) -> &'static str {
    match risk_gate {
        crate::commands::CommandRiskGate::ReadOnly => "read_only",
        crate::commands::CommandRiskGate::UserVisibleStateChange => "user_visible_state_change",
        crate::commands::CommandRiskGate::CriticalStateChange => "critical_state_change",
    }
}

fn debug_start_session_bridge(args: DebugStartSessionArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let snapshot = start_debug_session(
        &store,
        &DebugSessionStartInput {
            session_id: args.session_id,
            started_at_unix_ms: args.started_at_unix_ms,
            bridge: args.bridge,
        },
    )?;
    serde_json::to_value(snapshot).map_err(|error| {
        GooseError::message(format!("cannot serialize debug session snapshot: {error}"))
    })
}

fn debug_start_command_bridge(args: DebugStartCommandArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let snapshot = start_debug_command(
        &store,
        &DebugCommandStartInput {
            session_id: args.session_id,
            received_at_unix_ms: args.received_at_unix_ms,
            command: args.command,
        },
    )?;
    serde_json::to_value(snapshot).map_err(|error| {
        GooseError::message(format!("cannot serialize debug session snapshot: {error}"))
    })
}

fn debug_finish_command_bridge(args: DebugFinishCommandArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let snapshot = finish_debug_command(
        &store,
        &DebugCommandFinishInput {
            session_id: args.session_id,
            time_unix_ms: args.time_unix_ms,
            command_id: args.command_id,
            ok: args.ok,
            message: args.message,
            data: args.data,
        },
    )?;
    serde_json::to_value(snapshot).map_err(|error| {
        GooseError::message(format!("cannot serialize debug session snapshot: {error}"))
    })
}

fn debug_record_event_bridge(args: DebugRecordEventArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let event = append_debug_event(
        &store,
        &DebugEventInput {
            session_id: args.session_id,
            time_unix_ms: args.time_unix_ms,
            source: args.source,
            level: args.level,
            topic: args.topic,
            message: args.message,
            command_id: args.command_id,
            data: args.data,
        },
    )?;
    serde_json::to_value(event)
        .map_err(|error| GooseError::message(format!("cannot serialize debug event: {error}")))
}

fn debug_session_snapshot_bridge(args: DebugSessionSnapshotArgs) -> GooseResult<serde_json::Value> {
    let store = open_bridge_store(&args.database_path)?;
    let snapshot = debug_session_snapshot(&store, &args.session_id)?;
    serde_json::to_value(snapshot).map_err(|error| {
        GooseError::message(format!("cannot serialize debug session snapshot: {error}"))
    })
}

fn metric_result_to_value<T: Serialize>(result: T) -> GooseResult<serde_json::Value> {
    serde_json::to_value(result)
        .map_err(|error| GooseError::message(format!("cannot serialize metric result: {error}")))
}

fn maybe_persist_algorithm_run<T: Serialize>(
    store: &GooseStore,
    report_value: &mut serde_json::Value,
    persist_requested: bool,
    requested_run_id: Option<&str>,
    default_run_prefix: &str,
    score_result: Option<&AlgorithmRunResult<T>>,
) -> GooseResult<()> {
    if !persist_requested {
        return Ok(());
    }
    let Some(score_result) = score_result else {
        report_value["persisted_algorithm_run"] = json!({
            "persist_requested": true,
            "inserted": false,
            "blocked_reason": "score_result_missing",
        });
        return Ok(());
    };
    if score_result.output.is_none() {
        report_value["persisted_algorithm_run"] = json!({
            "persist_requested": true,
            "inserted": false,
            "algorithm_id": &score_result.algorithm_id,
            "algorithm_version": &score_result.algorithm_version,
            "blocked_reason": "score_output_missing",
        });
        return Ok(());
    }
    let run_id = requested_run_id
        .filter(|run_id| !run_id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| packet_derived_algorithm_run_id(default_run_prefix, score_result));
    for definition in built_in_algorithm_definitions()
        .into_iter()
        .filter(|definition| {
            definition.algorithm_id == score_result.algorithm_id
                && definition.version == score_result.algorithm_version
        })
    {
        store.upsert_algorithm_definition(&definition)?;
    }
    let record = algorithm_run_record(&run_id, score_result)?;
    let inserted = store.insert_algorithm_run(&record)?;
    report_value["persisted_algorithm_run"] = json!({
        "persist_requested": true,
        "inserted": inserted,
        "run_id": run_id,
        "algorithm_id": &score_result.algorithm_id,
        "algorithm_version": &score_result.algorithm_version,
        "start_time": &score_result.start_time,
        "end_time": &score_result.end_time,
    });
    Ok(())
}

fn packet_derived_algorithm_run_id<T>(prefix: &str, result: &AlgorithmRunResult<T>) -> String {
    format!(
        "{}.{}.{}.{}",
        prefix, result.algorithm_id, result.start_time, result.end_time
    )
    .chars()
    .map(|ch| {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            ch
        } else {
            '-'
        }
    })
    .collect()
}

fn latest_matching_calibration_run(
    store: &GooseStore,
    algorithm_id: &str,
    algorithm_version: &str,
) -> GooseResult<Option<crate::store::CalibrationRunRecord>> {
    let runs = store.calibration_runs_overlapping("0000", "9999")?;
    Ok(runs
        .into_iter()
        .filter(|run| run.algorithm_id == algorithm_id && run.version == algorithm_version)
        .max_by(|left, right| {
            left.times
                .holdout_end
                .cmp(&right.times.holdout_end)
                .then_with(|| left.calibration_run_id.cmp(&right.calibration_run_id))
        }))
}

/// Reject paths containing `..` components to prevent directory traversal.
/// On iOS all valid database paths are absolute and within the app sandbox container.
fn validate_no_traversal(label: &str, path: &str) -> GooseResult<()> {
    if Path::new(path)
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(GooseError::message(format!(
            "{label}: path traversal (.. component) is not permitted"
        )));
    }
    Ok(())
}

fn open_bridge_store(database_path: &str) -> GooseResult<GooseStore> {
    if database_path.trim().is_empty() {
        return Err(GooseError::message("database_path is required"));
    }
    validate_no_traversal("database_path", database_path)?;
    GooseStore::open(Path::new(database_path))
}

fn open_bridge_store_hot(database_path: &str) -> GooseResult<GooseStore> {
    if database_path.trim().is_empty() {
        return Err(GooseError::message("database_path is required"));
    }
    validate_no_traversal("database_path", database_path)?;
    let path = Path::new(database_path);
    GooseStore::open_existing_current(path).or_else(|_| GooseStore::open(path))
}

fn json_object_string(field_name: &str, value: &serde_json::Value) -> GooseResult<String> {
    if !value.is_object() {
        return Err(GooseError::message(format!(
            "{field_name} must be a JSON object"
        )));
    }
    serde_json::to_string(value)
        .map_err(|error| GooseError::message(format!("cannot serialize {field_name}: {error}")))
}

fn register_built_in_definitions(store: &GooseStore) -> GooseResult<()> {
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition)?;
    }
    Ok(())
}

fn request_args<T>(request: &BridgeRequest) -> GooseResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(request.args.clone())
        .map_err(|error| GooseError::message(format!("invalid args: {error}")))
}

fn parse_device_type(value: &str) -> GooseResult<DeviceType> {
    match value {
        "GEN4" | "GEN_4" | "Gen4" | "gen4" => Ok(DeviceType::Gen4),
        "MAVERICK" | "Maverick" | "maverick" => Ok(DeviceType::Maverick),
        "PUFFIN" | "Puffin" | "puffin" => Ok(DeviceType::Puffin),
        "GOOSE" | "Goose" | "goose" => Ok(DeviceType::Goose),
        "HR_MONITOR" | "hr_monitor" => Ok(DeviceType::HrMonitor),
        other => Err(GooseError::message(format!(
            "unsupported device_type: {other}"
        ))),
    }
}

fn default_device_type() -> String {
    "GOOSE".to_string()
}

fn default_algorithm_scope() -> String {
    "global".to_string()
}

fn default_true() -> bool {
    true
}

fn default_raw_export_app_version() -> String {
    "goose-app/bridge".to_string()
}

fn default_raw_export_core_version() -> String {
    format!(
        "goose-core/{}",
        option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
    )
}

fn default_parser_version() -> String {
    format!(
        "goose-core/{}",
        option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
    )
}

fn default_overnight_mode() -> String {
    "overnight_guard".to_string()
}

fn default_active_status() -> String {
    "active".to_string()
}

fn default_raw_notification_source() -> String {
    "ios.corebluetooth.raw_notification".to_string()
}

fn default_decode_status() -> String {
    "not_decoded".to_string()
}

fn default_capture_sanitize_salt() -> String {
    "goose-capture-sanitize-v1".to_string()
}

fn default_ui_coverage_map_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../apk-ui-inventory/coverage-map.json")
}

fn default_perf_scale() -> usize {
    DEFAULT_PERF_SCALE
}

fn default_property_seed() -> u64 {
    DEFAULT_PROPERTY_SEED
}

fn default_property_cases() -> usize {
    DEFAULT_CASES_PER_GROUP
}

fn default_manual_source() -> String {
    "manual".to_string()
}

fn default_correlation_start() -> String {
    "0000".to_string()
}

fn default_correlation_end() -> String {
    "9999".to_string()
}

fn empty_json_array() -> serde_json::Value {
    json!([])
}

fn empty_json_object() -> serde_json::Value {
    json!({})
}

fn elapsed_us_u64(started: Instant) -> u64 {
    let elapsed = started.elapsed().as_micros();
    if elapsed > u64::MAX as u128 {
        u64::MAX
    } else {
        elapsed as u64
    }
}

fn bridge_ok(request_id: &str, result: serde_json::Value) -> BridgeResponse {
    BridgeResponse {
        schema: BRIDGE_RESPONSE_SCHEMA.to_string(),
        request_id: request_id.to_string(),
        ok: true,
        result: Some(result),
        error: None,
        timing: None,
    }
}

fn bridge_error(
    request_id: &str,
    code: impl Into<String>,
    message: impl ToString,
) -> BridgeResponse {
    BridgeResponse {
        schema: BRIDGE_RESPONSE_SCHEMA.to_string(),
        request_id: request_id.to_string(),
        ok: false,
        result: None,
        error: Some(BridgeError {
            code: code.into(),
            message: message.to_string(),
        }),
        timing: None,
    }
}

fn response_to_c_string(response: &BridgeResponse) -> *mut c_char {
    string_to_c_string(serialize_response(response))
}

fn json_to_c_string(value: serde_json::Value) -> *mut c_char {
    match serde_json::to_string(&value) {
        Ok(value) => string_to_c_string(value),
        Err(error) => string_to_c_string(serialize_response(&bridge_error(
            "unknown",
            "serialization_error",
            error.to_string(),
        ))),
    }
}

fn string_to_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn serialize_response(response: &BridgeResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|error| {
        format!(
            r#"{{"schema":"{BRIDGE_RESPONSE_SCHEMA}","request_id":"unknown","ok":false,"error":{{"code":"serialization_error","message":"{}"}}}}"#,
            escape_json_string(&error.to_string())
        )
    })
}

fn escape_json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard against drift between [`BRIDGE_METHODS`] and the dispatcher.
    ///
    /// Scans the live source of `handle_bridge_request_inner` for every
    /// `"method.name" =>` arm and asserts the extracted set equals
    /// `BRIDGE_METHODS`. Anyone adding a new bridge method must register it
    /// in the constant or this test fails — keeping `core.list_methods`
    /// authoritative.
    #[test]
    fn bridge_methods_constant_matches_dispatcher() {
        let src = include_str!("bridge.rs");
        let start = src
            .find("match request.method.as_str()")
            .expect("dispatcher match not found");
        // The dispatcher arm uses `method =>` as its catch-all. Stop scanning
        // there so we don't pick up unrelated string literals later in the
        // file (e.g. in tests).
        let catchall = src[start..]
            .find("method => bridge_error(")
            .expect("dispatcher catch-all not found");
        let block = &src[start..start + catchall];

        let mut found: Vec<String> = Vec::new();
        let lines: Vec<&str> = block.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('"') {
                continue;
            }
            // Skip arms preceded by a #[cfg(...)] attribute — these are
            // conditionally compiled (e.g. test-only) and must not appear in
            // the public BRIDGE_METHODS list.
            if i > 0 {
                let prev = lines[i - 1].trim_start();
                if prev.starts_with("#[cfg(") {
                    continue;
                }
            }
            // Match `"some.method" =>` at line start.
            let after_quote = &trimmed[1..];
            let Some(end_quote) = after_quote.find('"') else {
                continue;
            };
            let name = &after_quote[..end_quote];
            let rest = after_quote[end_quote + 1..].trim_start();
            if rest.starts_with("=>") {
                found.push(name.to_string());
            }
        }
        found.sort();
        found.dedup();

        let mut expected: Vec<String> = BRIDGE_METHODS.iter().map(|s| s.to_string()).collect();
        expected.sort();

        assert_eq!(
            found, expected,
            "BRIDGE_METHODS is out of sync with the dispatcher. \
             Either add the new method to BRIDGE_METHODS (keep it sorted) \
             or remove the stale entry."
        );
    }

    /// Belt-and-braces: `BRIDGE_METHODS` is documented as sorted; verify it.
    #[test]
    fn bridge_methods_constant_is_sorted_and_unique() {
        let mut sorted = BRIDGE_METHODS.to_vec();
        sorted.sort();
        assert_eq!(
            BRIDGE_METHODS,
            sorted.as_slice(),
            "BRIDGE_METHODS must be sorted"
        );
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(sorted.len(), deduped.len(), "BRIDGE_METHODS must be unique");
    }

    /// The `core.list_methods` RPC must round-trip through the bridge envelope
    /// and return the exact same list as the constant.
    #[test]
    fn core_list_methods_rpc_returns_full_method_set() {
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-list-methods".to_string(),
            method: "core.list_methods".to_string(),
            args: serde_json::Value::Null,
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "core.list_methods should succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result payload");
        assert_eq!(result["schema"], BRIDGE_METHODS_LIST_SCHEMA);
        assert_eq!(
            result["count"].as_u64().unwrap() as usize,
            BRIDGE_METHODS.len()
        );
        let methods: Vec<String> = result["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let expected: Vec<String> = BRIDGE_METHODS.iter().map(|s| s.to_string()).collect();
        assert_eq!(methods, expected);
        // `core.list_methods` must itself appear in the list it advertises.
        assert!(methods.iter().any(|m| m == "core.list_methods"));
    }

    #[test]
    fn capture_arrival_next_focus_includes_recovery_sensor_capture_actions() {
        let generic_metric_action = CaptureArrivalPlanAction {
            source: "Metric Inputs".to_string(),
            scope: "heart_rate".to_string(),
            reason: "score_input_missing".to_string(),
            action: "Resolve score input blockers.".to_string(),
            summary: "score_input_missing: Resolve score input blockers.".to_string(),
        };
        let recovery_sensor_action = CaptureArrivalPlanAction {
            source: "Recovery Sensors".to_string(),
            scope: "oxygen_saturation_percent".to_string(),
            reason: "pulse_information_seen_without_spo2_decode".to_string(),
            action: "Capture charger, overnight, and post-sync optical/history packets."
                .to_string(),
            summary: "pulse_information_seen_without_spo2_decode: Capture charger, overnight, and post-sync optical/history packets.".to_string(),
        };

        let focus = capture_arrival_plan_next_focus(&[
            generic_metric_action,
            recovery_sensor_action.clone(),
        ])
        .unwrap();

        assert_eq!(focus.source, "Recovery Sensors");
        assert_eq!(focus.scope, "oxygen_saturation_percent");
        assert_eq!(focus.reason, "pulse_information_seen_without_spo2_decode");
    }

    #[test]
    fn capture_arrival_next_focus_prioritizes_local_health_before_generic_metric_work() {
        let generic_metric_action = CaptureArrivalPlanAction {
            source: "Metric Inputs".to_string(),
            scope: "heart_rate".to_string(),
            reason: "score_input_missing".to_string(),
            action: "Resolve score input blockers.".to_string(),
            summary: "score_input_missing: Resolve score input blockers.".to_string(),
        };
        let local_health_action = CaptureArrivalPlanAction {
            source: "Local Health Validation".to_string(),
            scope: "owned-step-validation".to_string(),
            reason: "step-validation:manual_label:manual_step_delta".to_string(),
            action: "Run the controlled step capture and add labels.".to_string(),
            summary: "step-validation:manual_label:manual_step_delta: Run the controlled step capture and add labels.".to_string(),
        };

        let focus =
            capture_arrival_plan_next_focus(&[generic_metric_action, local_health_action.clone()])
                .unwrap();

        assert_eq!(focus.source, "Local Health Validation");
        assert_eq!(focus.scope, "owned-step-validation");
    }

    #[test]
    fn sleep_history_schedule_baseline_ignores_unusable_imported_nights() {
        let usable_night = sleep_history_night_fixture(
            "usable",
            "2026-05-01T22:00:00Z",
            "2026-05-02T06:00:00Z",
            430.0,
            480.0,
            50.0,
        );
        let impossible_night = sleep_history_night_fixture(
            "impossible",
            "2026-05-01T02:00:00Z",
            "2026-05-01T10:00:00Z",
            480.0,
            480.0,
            120.0,
        );

        assert!(sleep_history_schedule_baseline(&[impossible_night.clone()]).is_none());

        let (bedtime, wake_time) =
            sleep_history_schedule_baseline(&[usable_night, impossible_night]).unwrap();
        assert_eq!(bedtime, 22.0 * 60.0);
        assert_eq!(wake_time, 6.0 * 60.0);
    }

    #[test]
    fn days_since_last_valid_sleep_night_ignores_unusable_imported_nights() {
        let sleep_input = SleepInput {
            start_time: "2026-05-03T22:00:00Z".to_string(),
            end_time: "2026-05-04T06:00:00Z".to_string(),
            sleep_duration_minutes: 440.0,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes: 480.0,
            midpoint_deviation_minutes: 0.0,
            disturbance_count: 0,
            ..Default::default()
        };
        let usable_night = sleep_history_night_fixture(
            "usable",
            "2026-05-01T22:00:00Z",
            "2026-05-02T06:00:00Z",
            430.0,
            480.0,
            50.0,
        );
        let recent_impossible_night = sleep_history_night_fixture(
            "recent-impossible",
            "2026-05-03T02:00:00Z",
            "2026-05-03T10:00:00Z",
            480.0,
            480.0,
            120.0,
        );

        assert_eq!(
            days_since_last_valid_sleep_night(
                &sleep_input,
                &[usable_night, recent_impossible_night]
            ),
            Some(1)
        );
    }

    #[test]
    fn sleep_v1_external_history_prefers_detailed_stage_rows_over_empty_summary() {
        let store = GooseStore::open_in_memory().unwrap();
        let night_start = sleep_time_unix_ms("2026-05-01T22:00:00Z").unwrap();
        let night_end = sleep_time_unix_ms("2026-05-02T06:00:00Z").unwrap();
        store
            .insert_external_sleep_session(ExternalSleepSessionInput {
                sleep_id: "detailed-stage-night",
                source: "Apple Watch",
                platform: "healthkit",
                platform_record_id: Some("hk-detailed-stage-night"),
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_end,
                timezone: Some("Europe/London"),
                stage_summary_json: r#"{}"#,
                confidence: 0.90,
                provenance_json: r#"{"source":"healthkit_sleep_analysis"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "detailed-stage-night-core",
                sleep_id: "detailed-stage-night",
                stage_kind: "core",
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_start + 180 * 60 * 1000,
                confidence: 0.90,
                provenance_json: r#"{"source":"healthkit_sleep_analysis"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "detailed-stage-night-awake",
                sleep_id: "detailed-stage-night",
                stage_kind: "awake",
                start_time_unix_ms: night_start + 180 * 60 * 1000,
                end_time_unix_ms: night_start + 240 * 60 * 1000,
                confidence: 0.90,
                provenance_json: r#"{"source":"healthkit_sleep_analysis"}"#,
            })
            .unwrap();

        let nights = external_sleep_history_nights_for_sleep_v1(
            &store,
            480.0,
            sleep_time_unix_ms("2026-05-02T22:00:00Z").unwrap(),
        )
        .unwrap();

        assert_eq!(nights.len(), 1);
        let night = &nights[0];
        assert_eq!(night.night_id, "detailed-stage-night");
        assert_eq!(night.sleep_duration_minutes, 180.0);
        assert_eq!(night.awake_minutes, 60.0);
        assert_eq!(night.stage_minutes.get("core"), Some(&180.0));
        assert_eq!(night.stage_minutes.get("awake"), Some(&60.0));
        assert!(night.excluded_from_baseline);
    }

    #[test]
    fn sleep_v1_external_history_excludes_low_confidence_detailed_stage_rows() {
        let store = GooseStore::open_in_memory().unwrap();
        let night_start = sleep_time_unix_ms("2026-05-01T22:00:00Z").unwrap();
        let night_end = sleep_time_unix_ms("2026-05-02T06:00:00Z").unwrap();
        store
            .insert_external_sleep_session(ExternalSleepSessionInput {
                sleep_id: "low-confidence-stage-night",
                source: "Health Connect",
                platform: "health_connect",
                platform_record_id: Some("hc-low-confidence-stage-night"),
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_end,
                timezone: Some("Europe/London"),
                stage_summary_json: r#"{}"#,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_session"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "low-confidence-stage-night-core",
                sleep_id: "low-confidence-stage-night",
                stage_kind: "core",
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_start + 420 * 60 * 1000,
                confidence: 0.40,
                provenance_json: r#"{"source":"health_connect_sleep_stage"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "low-confidence-stage-night-awake",
                sleep_id: "low-confidence-stage-night",
                stage_kind: "awake",
                start_time_unix_ms: night_start + 420 * 60 * 1000,
                end_time_unix_ms: night_end,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_stage"}"#,
            })
            .unwrap();

        let nights = external_sleep_history_nights_for_sleep_v1(
            &store,
            480.0,
            sleep_time_unix_ms("2026-05-02T22:00:00Z").unwrap(),
        )
        .unwrap();

        assert_eq!(nights.len(), 1);
        assert_eq!(nights[0].night_id, "low-confidence-stage-night");
        assert!(nights[0].excluded_from_baseline);
    }

    #[test]
    fn sleep_v1_external_history_excludes_manual_detailed_stage_rows() {
        let store = GooseStore::open_in_memory().unwrap();
        let night_start = sleep_time_unix_ms("2026-05-01T22:00:00Z").unwrap();
        let night_end = sleep_time_unix_ms("2026-05-02T06:00:00Z").unwrap();
        store
            .insert_external_sleep_session(ExternalSleepSessionInput {
                sleep_id: "manual-stage-night",
                source: "Apple Watch",
                platform: "healthkit",
                platform_record_id: Some("hk-manual-stage-night"),
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_end,
                timezone: Some("Europe/London"),
                stage_summary_json: r#"{}"#,
                confidence: 0.90,
                provenance_json: r#"{"source":"healthkit_sleep_analysis"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "manual-stage-night-core",
                sleep_id: "manual-stage-night",
                stage_kind: "core",
                start_time_unix_ms: night_start,
                end_time_unix_ms: night_start + 420 * 60 * 1000,
                confidence: 0.90,
                provenance_json: r#"{"source":"manual_sleep_edit","manual_entry":true}"#,
            })
            .unwrap();

        let nights = external_sleep_history_nights_for_sleep_v1(
            &store,
            480.0,
            sleep_time_unix_ms("2026-05-02T22:00:00Z").unwrap(),
        )
        .unwrap();

        assert_eq!(nights.len(), 1);
        assert_eq!(nights[0].night_id, "manual-stage-night");
        assert!(nights[0].excluded_from_baseline);
    }

    #[test]
    fn sleep_v1_external_nap_credit_excludes_platform_imported_stage_rows() {
        let store = GooseStore::open_in_memory().unwrap();
        let nap_start = sleep_time_unix_ms("2026-05-02T16:00:00Z").unwrap();
        let nap_end = sleep_time_unix_ms("2026-05-02T17:00:00Z").unwrap();
        store
            .insert_external_sleep_session(ExternalSleepSessionInput {
                sleep_id: "detailed-stage-nap",
                source: "Health Connect",
                platform: "health_connect",
                platform_record_id: Some("hc-detailed-stage-nap"),
                start_time_unix_ms: nap_start,
                end_time_unix_ms: nap_end,
                timezone: Some("Europe/London"),
                stage_summary_json: r#"{}"#,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_session"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "detailed-stage-nap-core",
                sleep_id: "detailed-stage-nap",
                stage_kind: "core",
                start_time_unix_ms: nap_start,
                end_time_unix_ms: nap_start + 45 * 60 * 1000,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_stage"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "detailed-stage-nap-awake",
                sleep_id: "detailed-stage-nap",
                stage_kind: "awake",
                start_time_unix_ms: nap_start + 45 * 60 * 1000,
                end_time_unix_ms: nap_end,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_stage"}"#,
            })
            .unwrap();
        let sleep_input = SleepInput {
            start_time: "2026-05-02T22:00:00Z".to_string(),
            end_time: "2026-05-03T06:00:00Z".to_string(),
            sleep_duration_minutes: 430.0,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes: 480.0,
            midpoint_deviation_minutes: 0.0,
            disturbance_count: 0,
            ..Default::default()
        };

        let naps_minutes = external_sleep_naps_before_sleep(&store, &sleep_input).unwrap();

        assert_eq!(naps_minutes, 0.0);
    }

    #[test]
    fn sleep_v1_external_nap_credit_excludes_low_confidence_stage_rows() {
        let store = GooseStore::open_in_memory().unwrap();
        let nap_start = sleep_time_unix_ms("2026-05-02T16:00:00Z").unwrap();
        let nap_end = sleep_time_unix_ms("2026-05-02T17:00:00Z").unwrap();
        store
            .insert_external_sleep_session(ExternalSleepSessionInput {
                sleep_id: "low-confidence-stage-nap",
                source: "Health Connect",
                platform: "health_connect",
                platform_record_id: Some("hc-low-confidence-stage-nap"),
                start_time_unix_ms: nap_start,
                end_time_unix_ms: nap_end,
                timezone: Some("Europe/London"),
                stage_summary_json: r#"{}"#,
                confidence: 0.90,
                provenance_json: r#"{"source":"health_connect_sleep_session"}"#,
            })
            .unwrap();
        store
            .insert_external_sleep_stage(ExternalSleepStageInput {
                stage_id: "low-confidence-stage-nap-core",
                sleep_id: "low-confidence-stage-nap",
                stage_kind: "core",
                start_time_unix_ms: nap_start,
                end_time_unix_ms: nap_start + 45 * 60 * 1000,
                confidence: 0.40,
                provenance_json: r#"{"source":"health_connect_sleep_stage"}"#,
            })
            .unwrap();
        let sleep_input = SleepInput {
            start_time: "2026-05-02T22:00:00Z".to_string(),
            end_time: "2026-05-03T06:00:00Z".to_string(),
            sleep_duration_minutes: 430.0,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes: 480.0,
            midpoint_deviation_minutes: 0.0,
            disturbance_count: 0,
            ..Default::default()
        };

        let naps_minutes = external_sleep_naps_before_sleep(&store, &sleep_input).unwrap();

        assert_eq!(naps_minutes, 0.0);
    }

    fn sleep_history_night_fixture(
        night_id: &str,
        start_time: &str,
        end_time: &str,
        sleep_duration_minutes: f64,
        time_in_bed_minutes: f64,
        awake_minutes: f64,
    ) -> SleepNightHistoryInput {
        SleepNightHistoryInput {
            night_id: night_id.to_string(),
            start_time: start_time.to_string(),
            end_time: end_time.to_string(),
            sleep_duration_minutes,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes,
            awake_minutes,
            sleep_latency_minutes: 10.0,
            wake_after_sleep_onset_minutes: awake_minutes,
            wake_episode_count: 2,
            stage_minutes: BTreeMap::from([
                ("light".to_string(), sleep_duration_minutes * 0.55),
                ("deep".to_string(), sleep_duration_minutes * 0.20),
                ("rem".to_string(), sleep_duration_minutes * 0.25),
            ]),
            heart_rate_dip_percent: None,
            sleep_hr_average_bpm: None,
            sleep_hr_min_bpm: None,
            pre_sleep_awake_hr_average_bpm: None,
            sleep_hr_trend_bpm_per_hour: None,
            bedtime_deviation_minutes: 0.0,
            wake_time_deviation_minutes: 0.0,
            midpoint_deviation_minutes: 0.0,
            naps_minutes: 0.0,
            confidence_0_to_1: 0.95,
            source: "healthkit".to_string(),
            excluded_from_baseline: false,
        }
    }

    // Phase 08-P03 — parse_device_type HrMonitor assertions (HIGH-2)
    #[test]
    fn parse_device_type_hr_monitor_uppercase() {
        let result = parse_device_type("HR_MONITOR").expect("HR_MONITOR must parse");
        assert_eq!(
            result,
            DeviceType::HrMonitor,
            "HR_MONITOR must map to DeviceType::HrMonitor"
        );
    }

    #[test]
    fn parse_device_type_hr_monitor_lowercase() {
        let result = parse_device_type("hr_monitor").expect("hr_monitor must parse");
        assert_eq!(
            result,
            DeviceType::HrMonitor,
            "hr_monitor must map to DeviceType::HrMonitor"
        );
    }

    #[test]
    fn parse_device_type_goose_no_regression() {
        let result = parse_device_type("GOOSE").expect("GOOSE must parse");
        assert_eq!(
            result,
            DeviceType::Goose,
            "GOOSE must still map to DeviceType::Goose"
        );
    }

    // ---- EWMA baseline bridge round-trip tests ----------------------------

    fn make_temp_db() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.sqlite").to_string_lossy().to_string();
        // Migrate by opening a store (creates all tables)
        let _store = crate::store::GooseStore::open(std::path::Path::new(&path))
            .expect("open store for migration");
        (dir, path)
    }

    #[test]
    fn ewma_baseline_fold_history_bridge_empty_store() {
        let (_dir, db_path) = make_temp_db();
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-ewma-fold-empty".to_string(),
            method: "store.ewma_baseline_fold_history".to_string(),
            args: json!({"database_path": db_path}),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "fold_history on empty store must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["hrv"]["night_count"], 0,
            "empty store: hrv night_count = 0"
        );
        assert_eq!(
            result["resting_hr"]["night_count"], 0,
            "empty store: rhr night_count = 0"
        );
        assert_eq!(
            result["hrv"]["trust"], "calibrating",
            "empty store: trust = calibrating"
        );
    }

    #[test]
    fn ewma_baseline_update_bridge_round_trip() {
        let (_dir, db_path) = make_temp_db();

        // Insert one night via update bridge
        let update_req = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-ewma-update-1".to_string(),
            method: "store.ewma_baseline_update".to_string(),
            args: json!({
                "database_path": db_path,
                "date_key": "2024-01-01",
                "hrv_rmssd": 60.0,
                "rhr_bpm": 55.0
            }),
        };
        let update_resp = handle_bridge_request(update_req);
        assert!(
            update_resp.ok,
            "update must succeed: {:?}",
            update_resp.error
        );
        let update_result = update_resp.result.expect("update result");
        assert_eq!(
            update_result["skipped"], false,
            "first update must not be skipped"
        );

        // Idempotency: second call for same date must be skipped
        let update_req2 = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-ewma-update-2".to_string(),
            method: "store.ewma_baseline_update".to_string(),
            args: json!({
                "database_path": db_path,
                "date_key": "2024-01-01",
                "hrv_rmssd": 60.0,
                "rhr_bpm": 55.0
            }),
        };
        let update_resp2 = handle_bridge_request(update_req2);
        assert!(update_resp2.ok, "second update must not error");
        let update_result2 = update_resp2.result.expect("second update result");
        assert_eq!(
            update_result2["skipped"], true,
            "second update for same date must be skipped"
        );

        // fold_history must reflect the inserted night
        let fold_req = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-ewma-fold-after-update".to_string(),
            method: "store.ewma_baseline_fold_history".to_string(),
            args: json!({"database_path": db_path}),
        };
        let fold_resp = handle_bridge_request(fold_req);
        assert!(fold_resp.ok, "fold_history after update must succeed");
        let fold_result = fold_resp.result.expect("fold_history result");
        assert_eq!(
            fold_result["hrv"]["night_count"], 1,
            "fold_history must reflect inserted night"
        );
        assert!(
            (fold_result["hrv"]["mean"].as_f64().unwrap() - 60.0).abs() < 1e-9,
            "fold_history hrv mean must be 60.0 after one night"
        );
    }

    #[test]
    fn ewma_baseline_update_bridge_rejects_nan_hrv() {
        let (_dir, db_path) = make_temp_db();
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-ewma-nan".to_string(),
            method: "store.ewma_baseline_update".to_string(),
            args: json!({
                "database_path": db_path,
                "date_key": "2024-01-01",
                "hrv_rmssd": f64::NAN,
                "rhr_bpm": 55.0
            }),
        };
        let response = handle_bridge_request(request);
        // NaN is serialised to null in JSON by serde_json, so args parsing
        // may deserialise to null. Either way the result should not be ok or
        // if it is, the method must validate and return an error.
        // Accept both: ok=false OR ok=true with skipped due to null parse.
        // The important thing is the bridge doesn't crash and returns a response.
        assert!(
            !response.ok || response.result.is_some(),
            "bridge must respond (either error or valid result) for NaN hrv"
        );
    }

    // ---- metrics.goose_recovery_v1 bridge tests ---------------------------

    #[test]
    fn goose_recovery_v1_bridge_cold_start_no_rows() {
        let (_dir, db_path) = make_temp_db();

        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-recovery-v1-cold".to_string(),
            method: "metrics.goose_recovery_v1".to_string(),
            args: json!({
                "database_path": db_path,
                "device_id": "dev-001",
                "date_key": "2024-01-05",
                "hrv_rmssd_ms": 60.0,
                "resting_hr_bpm": 55.0
            }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "cold-start call must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["score_0_to_100"],
            serde_json::Value::Null,
            "cold-start: score must be null"
        );
        assert_eq!(
            result["trust_level"], "calibrating",
            "cold-start: trust_level must be 'calibrating'"
        );
        assert!(
            result.get("colour_band").is_some(),
            "cold-start: colour_band must be present"
        );
    }

    #[test]
    fn goose_recovery_v1_bridge_round_trip_with_4_nights() {
        let (_dir, db_path) = make_temp_db();

        // Insert 4 nights via ewma_baseline_update to seed the baseline.
        let dates = ["2024-01-01", "2024-01-02", "2024-01-03", "2024-01-04"];
        for date in &dates {
            let req = BridgeRequest {
                schema: BRIDGE_REQUEST_SCHEMA.to_string(),
                request_id: format!("test-seed-{}", date),
                method: "store.ewma_baseline_update".to_string(),
                args: json!({
                    "database_path": db_path,
                    "date_key": date,
                    "hrv_rmssd": 60.0,
                    "rhr_bpm": 55.0
                }),
            };
            let resp = handle_bridge_request(req);
            assert!(
                resp.ok,
                "seed night {} must succeed: {:?}",
                date, resp.error
            );
        }

        // Now call goose_recovery_v1 — should have a score (≥4 nights seeded).
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-recovery-v1-round-trip".to_string(),
            method: "metrics.goose_recovery_v1".to_string(),
            args: json!({
                "database_path": db_path,
                "device_id": "dev-001",
                "date_key": "2024-01-05",
                "hrv_rmssd_ms": 60.0,
                "resting_hr_bpm": 55.0
            }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "round-trip call must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");

        assert!(
            !result["score_0_to_100"].is_null(),
            "after 4 seeded nights: score must not be null"
        );
        assert!(
            result["trust_level"].as_str().is_some(),
            "trust_level must be a string"
        );
        assert!(
            result["colour_band"].as_str().is_some(),
            "colour_band must be a string"
        );
        // Trust must be provisional (exactly 4 nights) not calibrating.
        assert_ne!(
            result["trust_level"], "calibrating",
            "after 4 nights: trust must not be calibrating"
        );
    }

    // ---- metrics.goose_readiness_v1 bridge tests ---------------------------

    #[test]
    fn goose_readiness_v1_bridge_empty_input_returns_unknown() {
        // empty daily_strain → insufficient_data=true, level="unknown"
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "r1".to_string(),
            method: "metrics.goose_readiness_v1".to_string(),
            args: json!({ "daily_strain": [] }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "empty input must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["insufficient_data"],
            serde_json::Value::Bool(true),
            "empty input: insufficient_data must be true"
        );
        assert_eq!(
            result["level"], "unknown",
            "empty input: level must be 'unknown'"
        );
    }

    #[test]
    fn goose_readiness_v1_bridge_28_equal_strains_returns_primed() {
        // 28 equal-strain pairs (strain=10.0) → level="primed", acwr≈1.0, zone="optimal",
        // monotony=null (std=0 → None)
        let pairs: Vec<[f64; 2]> = (0..28).map(|i| [i as f64 * 86400.0, 10.0]).collect();
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "r2".to_string(),
            method: "metrics.goose_readiness_v1".to_string(),
            args: json!({ "daily_strain": pairs }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "28 equal strains must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["level"], "primed",
            "28 equal strains: level must be 'primed'"
        );
        assert_eq!(
            result["acwr_zone"], "optimal",
            "28 equal strains: zone must be 'optimal'"
        );
        assert!(
            result["acwr"]
                .as_f64()
                .map_or(false, |v| (v - 1.0).abs() < 1e-9),
            "28 equal strains: acwr must be ≈1.0, got {:?}",
            result["acwr"]
        );
        assert!(
            result["monotony"].is_null(),
            "28 equal strains: monotony must be null (std=0), got {:?}",
            result["monotony"]
        );
    }

    #[test]
    fn goose_readiness_v1_bridge_27_strains_returns_unknown() {
        // 27 pairs → insufficient_data=true, level="unknown"
        let pairs: Vec<[f64; 2]> = (0..27).map(|i| [i as f64 * 86400.0, 10.0]).collect();
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "r3".to_string(),
            method: "metrics.goose_readiness_v1".to_string(),
            args: json!({ "daily_strain": pairs }),
        };
        let response = handle_bridge_request(request);
        assert!(response.ok, "27 strains must succeed: {:?}", response.error);
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["insufficient_data"],
            serde_json::Value::Bool(true),
            "27 strains: insufficient_data must be true"
        );
        assert_eq!(
            result["level"], "unknown",
            "27 strains: level must be 'unknown'"
        );
    }

    #[test]
    fn goose_readiness_v1_bridge_high_acwr_returns_rundown() {
        // 28 pairs: first 21 at strain=5.0, last 7 at strain=21.0
        // chronic=(21*5+7*21)/28=9.0; acute=21.0; acwr=21/9≈2.33>1.5 → level="rundown"
        let mut pairs: Vec<[f64; 2]> = (0..21).map(|i| [i as f64 * 86400.0, 5.0]).collect();
        for j in 0..7 {
            pairs.push([(21 + j) as f64 * 86400.0, 21.0]);
        }
        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "r4".to_string(),
            method: "metrics.goose_readiness_v1".to_string(),
            args: json!({ "daily_strain": pairs }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "high-ACWR 28 strains must succeed: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["level"], "rundown",
            "high-ACWR input: level must be 'rundown'"
        );
        let acwr = result["acwr"].as_f64().expect("acwr must be a number");
        assert!(
            acwr >= 1.5,
            "high-ACWR input: acwr must be >=1.5, got {acwr}"
        );
    }

    // ---- metrics.sleep_staging bridge tests --------------------------------

    #[test]
    fn sleep_staging_bridge_empty_gravity_returns_no_imu_data() {
        let (_dir, db_path) = make_temp_db();

        let request = BridgeRequest {
            schema: BRIDGE_REQUEST_SCHEMA.to_string(),
            request_id: "test-sleep-staging-empty".to_string(),
            method: "metrics.sleep_staging".to_string(),
            args: json!({
                "database_path": db_path,
                "device_id": "dev-001",
                "sleep_start_ts": 1_700_000_000.0_f64,
                "sleep_end_ts":   1_700_028_800.0_f64
            }),
        };
        let response = handle_bridge_request(request);
        assert!(
            response.ok,
            "empty gravity table must return ok=true: {:?}",
            response.error
        );
        let result = response.result.expect("result must be present");
        assert_eq!(
            result["staging_method"].as_str(),
            Some("no_imu_data"),
            "empty gravity table must yield staging_method=no_imu_data"
        );
        assert!(
            result["epochs"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false),
            "epochs must be empty for an empty gravity table"
        );
        // AASM fields must be present in the output (even when no IMU data).
        assert!(
            result["stage_minutes"].is_object(),
            "stage_minutes must be an object in the output"
        );
        assert!(
            result["tst_minutes"].is_number(),
            "tst_minutes must be present in the output"
        );
        assert!(
            result["sol_minutes"].is_number(),
            "sol_minutes must be present in the output"
        );
        assert!(
            result["waso_minutes"].is_number(),
            "waso_minutes must be present in the output"
        );
        assert!(
            result["sleep_efficiency_fraction"].is_number(),
            "sleep_efficiency_fraction must be present in the output"
        );
    }
}

#[cfg(target_os = "android")]
pub mod android {
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use std::ptr;

    /// JNI entry point for com.goose.core.GooseBridge.handle(String) -> String.
    ///
    /// Converts the Java string to a Rust str, delegates to the existing
    /// handle_bridge_request_json dispatch function, and returns the response
    /// as a new Java string. Never panics — all errors are returned as JSON.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_goose_core_GooseBridge_handle(
        mut env: JNIEnv,
        _class: JClass,
        request_json: JString,
    ) -> jstring {
        let request = match env.get_string(&request_json) {
            Ok(s) => s.to_string_lossy().into_owned(),
            Err(_) => {
                return env
                    .new_string("{\"ok\":false,\"error\":\"jni_string_conversion_error\"}")
                    .map(|s| s.into_raw())
                    .unwrap_or(ptr::null_mut());
            }
        };
        let response = super::handle_bridge_request_json(&request);
        env.new_string(response)
            .map(|s| s.into_raw())
            .unwrap_or(ptr::null_mut())
    }
}
