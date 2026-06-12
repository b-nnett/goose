use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    GooseError, GooseResult,
    metric_features::{
        HeartRateFeatureOptions, HrvFeatureOptions, MetricWindowFeatureOptions,
        MotionFeatureOptions, RestingHeartRateFeatureOptions, SleepFeatureScoreOptions,
        VitalEventFeatureOptions, run_heart_rate_feature_report_for_store,
        run_hrv_feature_report_for_store, run_metric_window_feature_report_for_store,
        run_motion_feature_report_for_store, run_resting_heart_rate_feature_report_for_store,
        run_sleep_feature_score_report_for_store, run_vital_event_feature_report_for_store,
    },
    protocol::{
        DataPacketBodySummary, I16SeriesSummary, ParsedPayload, decode_hex_with_whitespace,
    },
    store::{
        ActivityIntervalRow, ActivityLabelRow, ActivityMetricRow, ActivitySessionRow,
        AlgorithmRunRecord, CalibrationLabelRow, CalibrationRunRecord, CommandValidationRecord,
        DailyActivityMetricRow, DailyRecoveryMetricRow, DebugCommandRow, DebugEventRow,
        DebugSessionRow, DecodedFrameRow, GooseStore, HourlyActivityMetricRow, MetricProvenanceRow,
        RawEvidenceRow,
    },
    timeline::{PacketTimelineRow, packet_timeline_from_decoded_frames},
};

const ALLOWED_ACTIVITY_SYNC_STATUSES: &[&str] = &[
    "candidate",
    "verified",
    "user_confirmed",
    "synced",
    "blocked",
    "discarded",
];

const ALLOWED_ACTIVITY_TYPES: &[&str] = &[
    "unknown",
    "running",
    "walking",
    "cycling",
    "jogging",
    "strength",
    "weightlifting",
    "powerlifting",
    "swimming",
    "rowing",
    "hiit",
    "hiking",
    "hiking_rucking",
    "functional_fitness",
    "machine_workout",
    "martial_arts",
    "boxing",
    "kickboxing",
    "rock_climbing",
    "climber",
    "pilates",
    "yoga",
    "hot_yoga",
    "restorative_yoga",
    "meditation",
    "breathwork",
    "non_sleep_deep_rest",
    "ice_bath",
    "sauna",
    "manual",
    "manual_labor",
    "commuting",
    "cleaning",
    "cooking",
    "driving",
    "dog_walking",
    "stroller_walking",
    "stroller_jogging",
    "race_walking",
    "spinning",
    "elliptical",
    "team_sport",
    "padel",
    "barre",
    "barre3",
    "other",
    "other_recovery",
    "nap",
];

const ALLOWED_ACTIVITY_DETECTION_METHODS: &[&str] = &[
    "user_assigned",
    "heuristic_motion",
    "heuristic_hr_motion",
    "machine_learning",
    "official_capture",
    "imported",
    "manual_split",
    "manual_merge",
    "manual_annotation",
];

const ALLOWED_ACTIVITY_INTERVAL_TYPES: &[&str] =
    &["lap", "pause", "work", "rest", "window", "split"];

const ALLOWED_ACTIVITY_LABEL_TYPES: &[&str] = &[
    "user",
    "official_app_comparison",
    "calibration",
    "candidate",
];

const ALLOWED_ACTIVITY_METRIC_UNITS: &[&str] = &[
    "raw", "bpm", "ms", "hz", "count", "steps", "m", "km", "mi", "kcal", "m/s", "km/h", "min", "s",
    "percent", "ratio", "load", "joule", "w", "kg", "m/s2", "c", "f", "degrees", "n/a",
];

const ALLOWED_METRIC_SOURCE_KINDS: &[&str] = &[
    "device_counter",
    "device_sensor",
    "local_estimate",
    "unavailable",
];

const ALLOWED_METRIC_PROVENANCE_SCOPES: &[&str] =
    &["daily_activity", "daily_recovery", "hourly_activity"];

pub const RAW_EXPORT_RAW_EVIDENCE_FAMILY: &str = "raw_evidence";
pub const RAW_EXPORT_DECODED_FRAMES_FAMILY: &str = "decoded_frames";
pub const RAW_EXPORT_PACKET_TIMELINE_FAMILY: &str = "packet_timeline";
pub const RAW_EXPORT_SENSOR_SAMPLES_FAMILY: &str = "sensor_samples";
pub const RAW_EXPORT_METRIC_FEATURES_FAMILY: &str = "metric_features";
pub const RAW_EXPORT_METRIC_OUTPUTS_FAMILY: &str = "metric_outputs";
pub const RAW_EXPORT_ALGORITHM_RUNS_FAMILY: &str = "algorithm_runs";
pub const RAW_EXPORT_CALIBRATION_LABELS_FAMILY: &str = "calibration_labels";
pub const RAW_EXPORT_CALIBRATION_RUNS_FAMILY: &str = "calibration_runs";
pub const RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY: &str = "activity_sessions";
pub const RAW_EXPORT_ACTIVITY_METRICS_FAMILY: &str = "activity_metrics";
pub const RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY: &str = "activity_intervals";
pub const RAW_EXPORT_ACTIVITY_LABELS_FAMILY: &str = "activity_labels";
pub const RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY: &str = "local_health_metrics";
pub const RAW_EXPORT_DEBUG_SESSIONS_FAMILY: &str = "debug_sessions";
pub const RAW_EXPORT_DEBUG_COMMANDS_FAMILY: &str = "debug_commands";
pub const RAW_EXPORT_DEBUG_EVENTS_FAMILY: &str = "debug_events";
pub const RAW_EXPORT_COMMAND_VALIDATION_FAMILY: &str = "command_validation";
pub const RAW_EXPORT_SQLITE_FAMILY: &str = "sqlite";

const RAW_EXPORT_DATA_FAMILIES: &[&str] = &[
    RAW_EXPORT_RAW_EVIDENCE_FAMILY,
    RAW_EXPORT_DECODED_FRAMES_FAMILY,
    RAW_EXPORT_PACKET_TIMELINE_FAMILY,
    RAW_EXPORT_SENSOR_SAMPLES_FAMILY,
    RAW_EXPORT_METRIC_FEATURES_FAMILY,
    RAW_EXPORT_METRIC_OUTPUTS_FAMILY,
    RAW_EXPORT_ALGORITHM_RUNS_FAMILY,
    RAW_EXPORT_CALIBRATION_LABELS_FAMILY,
    RAW_EXPORT_CALIBRATION_RUNS_FAMILY,
    RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY,
    RAW_EXPORT_ACTIVITY_METRICS_FAMILY,
    RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY,
    RAW_EXPORT_ACTIVITY_LABELS_FAMILY,
    RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
    RAW_EXPORT_DEBUG_SESSIONS_FAMILY,
    RAW_EXPORT_DEBUG_COMMANDS_FAMILY,
    RAW_EXPORT_DEBUG_EVENTS_FAMILY,
    RAW_EXPORT_COMMAND_VALIDATION_FAMILY,
    RAW_EXPORT_SQLITE_FAMILY,
];

const RAW_EXPORT_DEFAULT_RECORD_FAMILIES: &[&str] = &[
    RAW_EXPORT_RAW_EVIDENCE_FAMILY,
    RAW_EXPORT_DECODED_FRAMES_FAMILY,
    RAW_EXPORT_PACKET_TIMELINE_FAMILY,
    RAW_EXPORT_SENSOR_SAMPLES_FAMILY,
    RAW_EXPORT_METRIC_FEATURES_FAMILY,
    RAW_EXPORT_METRIC_OUTPUTS_FAMILY,
    RAW_EXPORT_ALGORITHM_RUNS_FAMILY,
    RAW_EXPORT_CALIBRATION_LABELS_FAMILY,
    RAW_EXPORT_CALIBRATION_RUNS_FAMILY,
    RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY,
    RAW_EXPORT_ACTIVITY_METRICS_FAMILY,
    RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY,
    RAW_EXPORT_ACTIVITY_LABELS_FAMILY,
    RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
    RAW_EXPORT_DEBUG_SESSIONS_FAMILY,
    RAW_EXPORT_DEBUG_COMMANDS_FAMILY,
    RAW_EXPORT_DEBUG_EVENTS_FAMILY,
    RAW_EXPORT_COMMAND_VALIDATION_FAMILY,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportManifest {
    pub schema_version: String,
    pub app_version: String,
    pub core_version: String,
    pub time_window: ExportTimeWindow,
    pub data_families: Vec<String>,
    #[serde(default)]
    pub filters: RawExportFilters,
    pub files: Vec<ExportFileManifest>,
    #[serde(default)]
    pub official_labels_are_labels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportTimeWindow {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFileManifest {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub row_count: Option<u64>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub bundle_path: String,
    pub manifest_valid: bool,
    pub files_valid: bool,
    pub content_valid: bool,
    pub pass: bool,
    pub files: Vec<ExportFileValidation>,
    pub content: ExportContentValidation,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ExportValidationNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFileValidation {
    pub path: String,
    pub expected_sha256: String,
    pub actual_sha256: Option<String>,
    pub pass: bool,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ExportValidationNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExportValidationNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExportContentValidation {
    pub pass: bool,
    #[serde(default)]
    pub csv_valid: bool,
    #[serde(default)]
    pub csv_row_count_checks: usize,
    pub raw_evidence_rows: usize,
    pub decoded_frame_rows: usize,
    pub packet_timeline_rows: usize,
    pub sensor_sample_rows: usize,
    pub metric_feature_report_rows: usize,
    pub metric_value_rows: usize,
    pub metric_component_rows: usize,
    #[serde(default)]
    pub algorithm_run_rows: usize,
    pub calibration_label_rows: usize,
    #[serde(default)]
    pub calibration_run_rows: usize,
    pub activity_session_rows: usize,
    pub activity_metric_rows: usize,
    pub activity_interval_rows: usize,
    pub activity_label_rows: usize,
    #[serde(default)]
    pub daily_activity_metric_rows: usize,
    #[serde(default)]
    pub hourly_activity_metric_rows: usize,
    #[serde(default)]
    pub daily_recovery_metric_rows: usize,
    #[serde(default)]
    pub metric_provenance_rows: usize,
    pub command_validation_rows: usize,
    #[serde(default)]
    pub debug_session_rows: usize,
    #[serde(default)]
    pub debug_command_rows: usize,
    #[serde(default)]
    pub debug_event_rows: usize,
    pub reimported_evidence_ids: usize,
    pub reimported_frame_ids: usize,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ExportValidationNextAction>,
}

#[derive(Debug, Clone)]
pub struct RawExportOptions<'a> {
    pub output_dir: &'a Path,
    pub start: &'a str,
    pub end: &'a str,
    pub app_version: &'a str,
    pub core_version: &'a str,
    pub data_families: Vec<String>,
    pub filters: RawExportFilters,
    pub sqlite_source_path: Option<&'a Path>,
    pub zip_output_path: Option<&'a Path>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawExportFilters {
    #[serde(default = "default_include_raw_bytes")]
    pub include_raw_bytes: bool,
    #[serde(default)]
    pub capture_session_ids: Vec<String>,
    #[serde(default)]
    pub packet_type_names: Vec<String>,
    #[serde(default)]
    pub sensor_source_signals: Vec<String>,
    #[serde(default)]
    pub metric_families: Vec<String>,
    #[serde(default)]
    pub algorithm_ids: Vec<String>,
    #[serde(default)]
    pub algorithm_versions: Vec<String>,
}

impl Default for RawExportFilters {
    fn default() -> Self {
        Self {
            include_raw_bytes: true,
            capture_session_ids: Vec::new(),
            packet_type_names: Vec::new(),
            sensor_source_signals: Vec::new(),
            metric_families: Vec::new(),
            algorithm_ids: Vec::new(),
            algorithm_versions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawExportReport {
    pub schema: String,
    pub generated_by: String,
    pub output_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zip_path: Option<String>,
    pub pass: bool,
    pub input_valid: bool,
    pub data_families_valid: bool,
    pub filters_valid: bool,
    pub time_window_valid: bool,
    pub version_fields_valid: bool,
    pub sqlite_policy_valid: bool,
    pub manifest_ready: bool,
    pub files_written: bool,
    pub zip_ready: bool,
    pub export_ready: bool,
    pub raw_rows: usize,
    pub decoded_frame_rows: usize,
    pub packet_timeline_rows: usize,
    pub sensor_sample_rows: usize,
    pub metric_feature_report_rows: usize,
    pub metric_value_rows: usize,
    pub metric_component_rows: usize,
    pub algorithm_run_rows: usize,
    pub calibration_label_rows: usize,
    pub calibration_run_rows: usize,
    pub activity_session_rows: usize,
    pub activity_metric_rows: usize,
    pub activity_interval_rows: usize,
    pub activity_label_rows: usize,
    pub daily_activity_metric_rows: usize,
    pub hourly_activity_metric_rows: usize,
    pub daily_recovery_metric_rows: usize,
    pub metric_provenance_rows: usize,
    pub debug_session_rows: usize,
    pub debug_command_rows: usize,
    pub debug_event_rows: usize,
    pub command_validation_rows: usize,
    pub manifest: ExportManifest,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExportDebugSessionRow {
    session_id: String,
    started_at_unix_ms: i64,
    bridge_url: String,
    bind_host: String,
    token_required: bool,
    token_present: bool,
    remote_bind_enabled: bool,
    visible_remote_bind_toggle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExportDebugCommandRow {
    command_id: String,
    session_id: String,
    schema: String,
    command: String,
    args_json: String,
    dry_run: bool,
    received_at_unix_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExportDebugEventRow {
    session_id: String,
    sequence: i64,
    schema: String,
    time_unix_ms: i64,
    source: String,
    level: String,
    topic: String,
    message: String,
    command_id: Option<String>,
    data_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportCommandValidationRow {
    command: String,
    command_number: Option<u16>,
    family: String,
    risk_gate: String,
    direct_send_ready: bool,
    missing_requirements: Value,
    warnings: Value,
    next_capture_actions: Value,
    validated_service_uuid: Option<String>,
    validated_characteristic_uuid: Option<String>,
    validated_write_type: Option<String>,
    validated_evidence_source: Option<String>,
    validated_capture_kind: Option<String>,
    validated_owner: Option<String>,
    validated_provenance_json: Option<String>,
    validated_triggering_ui_action: Option<String>,
    report_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportCalibrationLabelRow {
    label_id: String,
    metric_family: String,
    label_source: String,
    captured_at: String,
    value: f64,
    unit: String,
    provenance_json: String,
    official_labels_are_labels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportMetricFeatureReportRow {
    report_kind: String,
    schema: String,
    start_time: String,
    end_time: String,
    pass: bool,
    feature_count: usize,
    trusted_feature_count: usize,
    issue_count: usize,
    issues_json: String,
    report_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportSensorSampleRow {
    sample_id: String,
    frame_id: String,
    evidence_id: String,
    captured_at: String,
    sample_time: String,
    sample_time_unix_ms: Option<i64>,
    sample_time_source: String,
    source_signal: String,
    packet_type_name: Option<String>,
    packet_k: Option<u8>,
    domain: Option<String>,
    series_name: String,
    sample_index: usize,
    payload_offset: usize,
    raw_i16: Option<i16>,
    raw_u8: Option<u8>,
    sample_value: i64,
    unit: String,
    device_timestamp_seconds: Option<u32>,
    device_timestamp_subseconds: Option<u16>,
    parser_version: String,
    quality_flags: Vec<String>,
    provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportMetricValueRow {
    metric_value_id: String,
    run_id: String,
    algorithm_id: String,
    version: String,
    metric_family: String,
    name: String,
    value: f64,
    unit: String,
    start_time: String,
    end_time: String,
    quality_flags: Vec<String>,
    provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExportMetricComponentRow {
    metric_component_id: String,
    run_id: String,
    algorithm_id: String,
    version: String,
    metric_family: String,
    component_name: String,
    value: f64,
    unit: String,
    score_0_to_100: Option<f64>,
    weight: Option<f64>,
    contribution: Option<f64>,
    contribution_json: Value,
    start_time: String,
    end_time: String,
    quality_flags: Vec<String>,
    provenance: Value,
}

pub fn validate_export_bundle(path: &Path) -> GooseResult<ExportValidationReport> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "zip")
    {
        return validate_zipped_export_bundle(path);
    }

    let (manifest_path, base_dir) = if path.is_dir() {
        (path.join("manifest.json"), path.to_path_buf())
    } else {
        (
            path.to_path_buf(),
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        )
    };

    let manifest_raw = fs::read_to_string(&manifest_path)
        .map_err(|source| GooseError::io(&manifest_path, source))?;
    let manifest: ExportManifest = serde_json::from_str(&manifest_raw)
        .map_err(|source| GooseError::json(&manifest_path, source))?;

    let mut manifest_issues = Vec::new();
    validate_manifest_shape(&manifest, &mut manifest_issues);
    let manifest_valid = manifest_issues.is_empty();
    let mut issues = manifest_issues;

    let file_results = manifest
        .files
        .iter()
        .map(|file| validate_manifest_file(&base_dir, file))
        .collect::<Vec<_>>();

    for result in &file_results {
        if !result.pass {
            issues.push(format!("{} failed file validation", result.path));
        }
    }
    let content = validate_export_contents(&manifest, |relative_path| {
        fs::read_to_string(base_dir.join(relative_path))
            .map_err(|source| GooseError::io(base_dir.join(relative_path), source))
    });
    issues.extend(content.issues.iter().cloned());

    Ok(report(path, manifest_valid, file_results, content, issues))
}

pub fn default_raw_export_data_families(include_sqlite: bool) -> Vec<String> {
    let mut families = RAW_EXPORT_DEFAULT_RECORD_FAMILIES
        .iter()
        .map(|family| (*family).to_string())
        .collect::<Vec<_>>();
    if include_sqlite {
        families.push(RAW_EXPORT_SQLITE_FAMILY.to_string());
    }
    families
}

pub fn export_raw_timeframe(
    store: &GooseStore,
    options: RawExportOptions<'_>,
) -> GooseResult<RawExportReport> {
    let mut issues = Vec::new();
    let family_issue_start = issues.len();
    let selected_data_families = normalize_raw_export_data_families(
        &options.data_families,
        options.sqlite_source_path.is_some(),
        &mut issues,
    );
    let data_families_valid = issues.len() == family_issue_start;
    let filter_issue_start = issues.len();
    let filters = normalize_raw_export_filters(&options.filters, &mut issues);
    let filters_valid = issues.len() == filter_issue_start;
    let mut time_window_valid = options.start < options.end;
    if !time_window_valid {
        issues.push("start must be earlier than end".to_string());
    }
    let version_fields_valid =
        !options.app_version.trim().is_empty() && !options.core_version.trim().is_empty();
    if options.app_version.trim().is_empty() {
        issues.push("app_version is required".to_string());
    }
    if options.core_version.trim().is_empty() {
        issues.push("core_version is required".to_string());
    }
    let exports_debug =
        raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_SESSIONS_FAMILY)
            || raw_export_family_selected(
                &selected_data_families,
                RAW_EXPORT_DEBUG_COMMANDS_FAMILY,
            )
            || raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_EVENTS_FAMILY);
    let debug_window = if exports_debug {
        let debug_window = parse_debug_time_window(options.start, options.end);
        if let Err(error) = &debug_window {
            time_window_valid = false;
            issues.push(error.clone());
        }
        Some(debug_window)
    } else {
        None
    };
    let exports_activity_sessions =
        raw_export_family_selected(&selected_data_families, RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY);
    let exports_activity_metrics =
        raw_export_family_selected(&selected_data_families, RAW_EXPORT_ACTIVITY_METRICS_FAMILY);
    let exports_activity_intervals = raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY,
    );
    let exports_activity_labels =
        raw_export_family_selected(&selected_data_families, RAW_EXPORT_ACTIVITY_LABELS_FAMILY);
    let exports_activity = exports_activity_sessions
        || exports_activity_metrics
        || exports_activity_intervals
        || exports_activity_labels;
    let exports_local_health_metrics = raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
    );
    let activity_window = if exports_activity || exports_local_health_metrics {
        let activity_window = parse_activity_time_window(options.start, options.end);
        if let Err(error) = &activity_window {
            time_window_valid = false;
            issues.push(error.clone());
        }
        Some(activity_window)
    } else {
        None
    };
    let sqlite_policy_issue_start = issues.len();
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_SQLITE_FAMILY)
        && options.sqlite_source_path.is_none()
    {
        issues.push("sqlite data family requires sqlite_source_path".to_string());
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_SQLITE_FAMILY)
        && !filters.include_raw_bytes
    {
        issues.push(
            "sqlite data family cannot be exported when include_raw_bytes is false".to_string(),
        );
    }
    let sqlite_policy_valid = issues.len() == sqlite_policy_issue_start;
    if !issues.is_empty() {
        let zip_ready = options.zip_output_path.is_none();
        return raw_export_report(
            options,
            selected_data_families,
            filters,
            RawExportReadinessInput {
                data_families_valid,
                filters_valid,
                time_window_valid,
                version_fields_valid,
                sqlite_policy_valid,
                manifest_ready: false,
                files_written: false,
                zip_ready,
            },
            issues,
        );
    }
    let debug_window = debug_window
        .transpose()
        .map_err(GooseError::message)?
        .unwrap_or((0, 0));
    let (debug_start_unix_ms, debug_end_unix_ms) = debug_window;
    let activity_window = activity_window
        .transpose()
        .map_err(GooseError::message)?
        .unwrap_or((0, 0));
    let (activity_start_unix_ms, activity_end_unix_ms) = activity_window;

    fs::create_dir_all(options.output_dir)
        .map_err(|source| GooseError::io(options.output_dir, source))?;
    fs::create_dir_all(options.output_dir.join("data"))
        .map_err(|source| GooseError::io(options.output_dir.join("data"), source))?;

    let raw_rows =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_RAW_EVIDENCE_FAMILY) {
            raw_evidence_rows_between_for_raw_export(store, options.start, options.end, &filters)?
        } else {
            Vec::new()
        };
    let decoded_rows =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DECODED_FRAMES_FAMILY) {
            decoded_rows_between_for_raw_export(store, options.start, options.end, &filters)?
        } else {
            Vec::new()
        };
    let packet_timeline_rows =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_PACKET_TIMELINE_FAMILY) {
            let timeline_source_rows = if decoded_rows.is_empty()
                && !raw_export_family_selected(
                    &selected_data_families,
                    RAW_EXPORT_DECODED_FRAMES_FAMILY,
                ) {
                decoded_rows_between_for_raw_export(store, options.start, options.end, &filters)?
            } else {
                decoded_rows.clone()
            };
            packet_timeline_from_decoded_frames(&timeline_source_rows)?
        } else {
            Vec::new()
        };
    let sensor_sample_rows =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_SENSOR_SAMPLES_FAMILY) {
            let sensor_source_rows = if decoded_rows.is_empty()
                && !raw_export_family_selected(
                    &selected_data_families,
                    RAW_EXPORT_DECODED_FRAMES_FAMILY,
                ) {
                decoded_rows_between_for_raw_export(store, options.start, options.end, &filters)?
            } else {
                decoded_rows.clone()
            };
            filter_sensor_sample_rows(export_sensor_samples(&sensor_source_rows)?, &filters)
        } else {
            Vec::new()
        };
    let metric_feature_reports =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_METRIC_FEATURES_FAMILY) {
            let evidence_scope = options
                .sqlite_source_path
                .unwrap_or(options.output_dir)
                .display()
                .to_string();
            let reports =
                export_metric_feature_reports(store, &evidence_scope, options.start, options.end)?;
            filter_metric_feature_reports(reports, &filters)
        } else {
            Vec::new()
        };
    let needs_algorithm_run_source =
        raw_export_family_selected(&selected_data_families, RAW_EXPORT_ALGORITHM_RUNS_FAMILY)
            || raw_export_family_selected(
                &selected_data_families,
                RAW_EXPORT_METRIC_OUTPUTS_FAMILY,
            );
    let algorithm_runs = if needs_algorithm_run_source {
        filter_algorithm_runs(
            store.algorithm_runs_overlapping(options.start, options.end)?,
            &filters,
        )
    } else {
        Vec::new()
    };
    let algorithm_run_report_rows =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_ALGORITHM_RUNS_FAMILY) {
            algorithm_runs.len()
        } else {
            0
        };
    let (metric_value_rows, metric_component_rows) =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_METRIC_OUTPUTS_FAMILY) {
            export_metric_outputs(&algorithm_runs)?
        } else {
            (Vec::new(), Vec::new())
        };
    let calibration_labels = if raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_CALIBRATION_LABELS_FAMILY,
    ) {
        filter_calibration_labels(
            export_calibration_labels(
                store.calibration_labels_between(options.start, options.end)?,
            ),
            &filters,
        )
    } else {
        Vec::new()
    };
    let calibration_runs = if raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_CALIBRATION_RUNS_FAMILY,
    ) {
        filter_calibration_runs(
            store.calibration_runs_overlapping(options.start, options.end)?,
            &filters,
        )
    } else {
        Vec::new()
    };
    let activity_session_rows = if exports_activity_sessions || exports_activity_labels {
        store.activity_sessions_between(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let activity_metric_rows = if exports_activity_metrics {
        store.activity_metrics_in_window(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let activity_interval_rows = if exports_activity_intervals {
        store.activity_intervals_in_window(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let activity_label_rows = if exports_activity_labels {
        export_activity_labels(store, &activity_session_rows)?
    } else {
        Vec::new()
    };
    let daily_activity_metric_rows = if exports_local_health_metrics {
        store.daily_activity_metrics_between(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let hourly_activity_metric_rows = if exports_local_health_metrics {
        store.hourly_activity_metrics_between(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let daily_recovery_metric_rows = if exports_local_health_metrics {
        store.daily_recovery_metrics_between(activity_start_unix_ms, activity_end_unix_ms)?
    } else {
        Vec::new()
    };
    let metric_provenance_rows = if exports_local_health_metrics {
        export_metric_provenance_rows(
            store,
            &daily_activity_metric_rows,
            &hourly_activity_metric_rows,
            &daily_recovery_metric_rows,
        )?
    } else {
        Vec::new()
    };
    let debug_sessions =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_SESSIONS_FAMILY) {
            export_debug_sessions(
                store.debug_sessions_between(debug_start_unix_ms, debug_end_unix_ms)?,
            )
        } else {
            Vec::new()
        };
    let debug_commands =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_COMMANDS_FAMILY) {
            export_debug_commands(
                store.debug_commands_between(debug_start_unix_ms, debug_end_unix_ms)?,
            )
        } else {
            Vec::new()
        };
    let debug_events =
        if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_EVENTS_FAMILY) {
            export_debug_events(store.debug_events_between(debug_start_unix_ms, debug_end_unix_ms)?)
        } else {
            Vec::new()
        };
    let command_validation_rows = if raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_COMMAND_VALIDATION_FAMILY,
    ) {
        export_command_validation_records(store.command_validation_records()?)?
    } else {
        Vec::new()
    };

    let mut manifest_files = Vec::new();
    let raw_rows_for_export = raw_evidence_rows_for_raw_byte_policy(&raw_rows, &filters);
    let decoded_rows_for_export = decoded_rows_for_raw_byte_policy(&decoded_rows, &filters)?;
    let packet_timeline_rows_for_export =
        packet_timeline_rows_for_raw_byte_policy(&packet_timeline_rows, &filters);
    let activity_session_rows_for_export =
        activity_session_rows_for_raw_byte_policy(&activity_session_rows, &filters)?;
    let activity_metric_rows_for_export =
        activity_metric_rows_for_raw_byte_policy(&activity_metric_rows, &filters)?;
    let activity_interval_rows_for_export =
        activity_interval_rows_for_raw_byte_policy(&activity_interval_rows, &filters)?;
    let activity_label_rows_for_export =
        activity_label_rows_for_raw_byte_policy(&activity_label_rows, &filters)?;
    let daily_activity_metric_rows_for_export =
        daily_activity_metric_rows_for_raw_byte_policy(&daily_activity_metric_rows, &filters)?;
    let hourly_activity_metric_rows_for_export =
        hourly_activity_metric_rows_for_raw_byte_policy(&hourly_activity_metric_rows, &filters)?;
    let daily_recovery_metric_rows_for_export =
        daily_recovery_metric_rows_for_raw_byte_policy(&daily_recovery_metric_rows, &filters)?;
    let metric_provenance_rows_for_export =
        metric_provenance_rows_for_raw_byte_policy(&metric_provenance_rows, &filters)?;
    let debug_commands_for_export = debug_commands_for_raw_byte_policy(&debug_commands, &filters)?;
    let debug_events_for_export = debug_events_for_raw_byte_policy(&debug_events, &filters)?;
    let command_validation_rows_for_export =
        command_validation_rows_for_raw_byte_policy(&command_validation_rows, &filters);

    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_RAW_EVIDENCE_FAMILY) {
        let raw_jsonl = write_jsonl(
            &options.output_dir.join("data/raw_evidence.jsonl"),
            &raw_rows_for_export,
        )?;
        let raw_csv = write_raw_csv(
            &options.output_dir.join("data/raw_evidence.csv"),
            &raw_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/raw_evidence.jsonl",
            &raw_jsonl,
            Some(raw_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/raw_evidence.csv",
            &raw_csv,
            Some(raw_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DECODED_FRAMES_FAMILY) {
        let decoded_jsonl = write_jsonl(
            &options.output_dir.join("data/decoded_frames.jsonl"),
            &decoded_rows_for_export,
        )?;
        let decoded_csv = write_decoded_csv(
            &options.output_dir.join("data/decoded_frames.csv"),
            &decoded_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/decoded_frames.jsonl",
            &decoded_jsonl,
            Some(decoded_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/decoded_frames.csv",
            &decoded_csv,
            Some(decoded_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_PACKET_TIMELINE_FAMILY) {
        let timeline_jsonl = write_jsonl(
            &options.output_dir.join("data/packet_timeline.jsonl"),
            &packet_timeline_rows_for_export,
        )?;
        let timeline_csv = write_packet_timeline_csv(
            &options.output_dir.join("data/packet_timeline.csv"),
            &packet_timeline_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/packet_timeline.jsonl",
            &timeline_jsonl,
            Some(packet_timeline_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/packet_timeline.csv",
            &timeline_csv,
            Some(packet_timeline_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_SENSOR_SAMPLES_FAMILY) {
        let sensor_samples_jsonl = write_jsonl(
            &options.output_dir.join("data/sensor_samples.jsonl"),
            &sensor_sample_rows,
        )?;
        let sensor_samples_csv = write_sensor_samples_csv(
            &options.output_dir.join("data/sensor_samples.csv"),
            &sensor_sample_rows,
        )?;
        manifest_files.push(manifest_file(
            "data/sensor_samples.jsonl",
            &sensor_samples_jsonl,
            Some(sensor_sample_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/sensor_samples.csv",
            &sensor_samples_csv,
            Some(sensor_sample_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_METRIC_FEATURES_FAMILY) {
        let metric_features_jsonl = write_jsonl(
            &options.output_dir.join("data/metric_features.jsonl"),
            &metric_feature_reports,
        )?;
        let metric_features_csv = write_metric_feature_reports_csv(
            &options.output_dir.join("data/metric_features.csv"),
            &metric_feature_reports,
        )?;
        manifest_files.push(manifest_file(
            "data/metric_features.jsonl",
            &metric_features_jsonl,
            Some(metric_feature_reports.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/metric_features.csv",
            &metric_features_csv,
            Some(metric_feature_reports.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_METRIC_OUTPUTS_FAMILY) {
        let metric_values_jsonl = write_jsonl(
            &options.output_dir.join("data/metric_values.jsonl"),
            &metric_value_rows,
        )?;
        let metric_values_csv = write_metric_values_csv(
            &options.output_dir.join("data/metric_values.csv"),
            &metric_value_rows,
        )?;
        manifest_files.push(manifest_file(
            "data/metric_values.jsonl",
            &metric_values_jsonl,
            Some(metric_value_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/metric_values.csv",
            &metric_values_csv,
            Some(metric_value_rows.len() as u64),
            "csv",
        ));

        let metric_components_jsonl = write_jsonl(
            &options.output_dir.join("data/metric_components.jsonl"),
            &metric_component_rows,
        )?;
        let metric_components_csv = write_metric_components_csv(
            &options.output_dir.join("data/metric_components.csv"),
            &metric_component_rows,
        )?;
        manifest_files.push(manifest_file(
            "data/metric_components.jsonl",
            &metric_components_jsonl,
            Some(metric_component_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/metric_components.csv",
            &metric_components_csv,
            Some(metric_component_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_ALGORITHM_RUNS_FAMILY) {
        let algorithm_runs_jsonl = write_jsonl(
            &options.output_dir.join("data/algorithm_runs.jsonl"),
            &algorithm_runs,
        )?;
        let algorithm_runs_csv = write_algorithm_runs_csv(
            &options.output_dir.join("data/algorithm_runs.csv"),
            &algorithm_runs,
        )?;
        manifest_files.push(manifest_file(
            "data/algorithm_runs.jsonl",
            &algorithm_runs_jsonl,
            Some(algorithm_runs.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/algorithm_runs.csv",
            &algorithm_runs_csv,
            Some(algorithm_runs.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_CALIBRATION_LABELS_FAMILY,
    ) {
        let calibration_labels_jsonl = write_jsonl(
            &options.output_dir.join("data/calibration_labels.jsonl"),
            &calibration_labels,
        )?;
        let calibration_labels_csv = write_calibration_labels_csv(
            &options.output_dir.join("data/calibration_labels.csv"),
            &calibration_labels,
        )?;
        manifest_files.push(manifest_file(
            "data/calibration_labels.jsonl",
            &calibration_labels_jsonl,
            Some(calibration_labels.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/calibration_labels.csv",
            &calibration_labels_csv,
            Some(calibration_labels.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_CALIBRATION_RUNS_FAMILY) {
        let calibration_runs_jsonl = write_jsonl(
            &options.output_dir.join("data/calibration_runs.jsonl"),
            &calibration_runs,
        )?;
        let calibration_runs_csv = write_calibration_runs_csv(
            &options.output_dir.join("data/calibration_runs.csv"),
            &calibration_runs,
        )?;
        manifest_files.push(manifest_file(
            "data/calibration_runs.jsonl",
            &calibration_runs_jsonl,
            Some(calibration_runs.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/calibration_runs.csv",
            &calibration_runs_csv,
            Some(calibration_runs.len() as u64),
            "csv",
        ));
    }
    if exports_activity_sessions {
        let activity_sessions_jsonl = write_jsonl(
            &options.output_dir.join("data/activity_sessions.jsonl"),
            &activity_session_rows_for_export,
        )?;
        let activity_sessions_csv = write_activity_sessions_csv(
            &options.output_dir.join("data/activity_sessions.csv"),
            &activity_session_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/activity_sessions.jsonl",
            &activity_sessions_jsonl,
            Some(activity_session_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/activity_sessions.csv",
            &activity_sessions_csv,
            Some(activity_session_rows.len() as u64),
            "csv",
        ));
    }
    if exports_activity_metrics {
        let activity_metrics_jsonl = write_jsonl(
            &options.output_dir.join("data/activity_metrics.jsonl"),
            &activity_metric_rows_for_export,
        )?;
        let activity_metrics_csv = write_activity_metrics_csv(
            &options.output_dir.join("data/activity_metrics.csv"),
            &activity_metric_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/activity_metrics.jsonl",
            &activity_metrics_jsonl,
            Some(activity_metric_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/activity_metrics.csv",
            &activity_metrics_csv,
            Some(activity_metric_rows.len() as u64),
            "csv",
        ));
    }
    if exports_activity_intervals {
        let activity_intervals_jsonl = write_jsonl(
            &options.output_dir.join("data/activity_intervals.jsonl"),
            &activity_interval_rows_for_export,
        )?;
        let activity_intervals_csv = write_activity_intervals_csv(
            &options.output_dir.join("data/activity_intervals.csv"),
            &activity_interval_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/activity_intervals.jsonl",
            &activity_intervals_jsonl,
            Some(activity_interval_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/activity_intervals.csv",
            &activity_intervals_csv,
            Some(activity_interval_rows.len() as u64),
            "csv",
        ));
    }
    if exports_activity_labels {
        let activity_labels_jsonl = write_jsonl(
            &options.output_dir.join("data/activity_labels.jsonl"),
            &activity_label_rows_for_export,
        )?;
        let activity_labels_csv = write_activity_labels_csv(
            &options.output_dir.join("data/activity_labels.csv"),
            &activity_label_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/activity_labels.jsonl",
            &activity_labels_jsonl,
            Some(activity_label_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/activity_labels.csv",
            &activity_labels_csv,
            Some(activity_label_rows.len() as u64),
            "csv",
        ));
    }
    if exports_local_health_metrics {
        let daily_activity_jsonl = write_jsonl(
            &options
                .output_dir
                .join("data/local_health_daily_activity_metrics.jsonl"),
            &daily_activity_metric_rows_for_export,
        )?;
        let daily_activity_csv = write_daily_activity_metrics_csv(
            &options
                .output_dir
                .join("data/local_health_daily_activity_metrics.csv"),
            &daily_activity_metric_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/local_health_daily_activity_metrics.jsonl",
            &daily_activity_jsonl,
            Some(daily_activity_metric_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/local_health_daily_activity_metrics.csv",
            &daily_activity_csv,
            Some(daily_activity_metric_rows.len() as u64),
            "csv",
        ));

        let hourly_activity_jsonl = write_jsonl(
            &options
                .output_dir
                .join("data/local_health_hourly_activity_metrics.jsonl"),
            &hourly_activity_metric_rows_for_export,
        )?;
        let hourly_activity_csv = write_hourly_activity_metrics_csv(
            &options
                .output_dir
                .join("data/local_health_hourly_activity_metrics.csv"),
            &hourly_activity_metric_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/local_health_hourly_activity_metrics.jsonl",
            &hourly_activity_jsonl,
            Some(hourly_activity_metric_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/local_health_hourly_activity_metrics.csv",
            &hourly_activity_csv,
            Some(hourly_activity_metric_rows.len() as u64),
            "csv",
        ));

        let daily_recovery_jsonl = write_jsonl(
            &options
                .output_dir
                .join("data/local_health_daily_recovery_metrics.jsonl"),
            &daily_recovery_metric_rows_for_export,
        )?;
        let daily_recovery_csv = write_daily_recovery_metrics_csv(
            &options
                .output_dir
                .join("data/local_health_daily_recovery_metrics.csv"),
            &daily_recovery_metric_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/local_health_daily_recovery_metrics.jsonl",
            &daily_recovery_jsonl,
            Some(daily_recovery_metric_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/local_health_daily_recovery_metrics.csv",
            &daily_recovery_csv,
            Some(daily_recovery_metric_rows.len() as u64),
            "csv",
        ));

        let metric_provenance_jsonl = write_jsonl(
            &options
                .output_dir
                .join("data/local_health_metric_provenance.jsonl"),
            &metric_provenance_rows_for_export,
        )?;
        let metric_provenance_csv = write_metric_provenance_csv(
            &options
                .output_dir
                .join("data/local_health_metric_provenance.csv"),
            &metric_provenance_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/local_health_metric_provenance.jsonl",
            &metric_provenance_jsonl,
            Some(metric_provenance_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/local_health_metric_provenance.csv",
            &metric_provenance_csv,
            Some(metric_provenance_rows.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_SESSIONS_FAMILY) {
        let debug_sessions_jsonl = write_jsonl(
            &options.output_dir.join("data/debug_sessions.jsonl"),
            &debug_sessions,
        )?;
        let debug_sessions_csv = write_debug_sessions_csv(
            &options.output_dir.join("data/debug_sessions.csv"),
            &debug_sessions,
        )?;
        manifest_files.push(manifest_file(
            "data/debug_sessions.jsonl",
            &debug_sessions_jsonl,
            Some(debug_sessions.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/debug_sessions.csv",
            &debug_sessions_csv,
            Some(debug_sessions.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_COMMANDS_FAMILY) {
        let debug_commands_jsonl = write_jsonl(
            &options.output_dir.join("data/debug_commands.jsonl"),
            &debug_commands_for_export,
        )?;
        let debug_commands_csv = write_debug_commands_csv(
            &options.output_dir.join("data/debug_commands.csv"),
            &debug_commands_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/debug_commands.jsonl",
            &debug_commands_jsonl,
            Some(debug_commands.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/debug_commands.csv",
            &debug_commands_csv,
            Some(debug_commands.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_DEBUG_EVENTS_FAMILY) {
        let debug_events_jsonl = write_jsonl(
            &options.output_dir.join("data/debug_events.jsonl"),
            &debug_events_for_export,
        )?;
        let debug_events_csv = write_debug_events_csv(
            &options.output_dir.join("data/debug_events.csv"),
            &debug_events_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/debug_events.jsonl",
            &debug_events_jsonl,
            Some(debug_events.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/debug_events.csv",
            &debug_events_csv,
            Some(debug_events.len() as u64),
            "csv",
        ));
    }
    if raw_export_family_selected(
        &selected_data_families,
        RAW_EXPORT_COMMAND_VALIDATION_FAMILY,
    ) {
        let command_validation_jsonl = write_jsonl(
            &options.output_dir.join("data/command_validation.jsonl"),
            &command_validation_rows_for_export,
        )?;
        let command_validation_csv = write_command_validation_csv(
            &options.output_dir.join("data/command_validation.csv"),
            &command_validation_rows_for_export,
        )?;
        manifest_files.push(manifest_file(
            "data/command_validation.jsonl",
            &command_validation_jsonl,
            Some(command_validation_rows.len() as u64),
            "jsonl",
        ));
        manifest_files.push(manifest_file(
            "data/command_validation.csv",
            &command_validation_csv,
            Some(command_validation_rows.len() as u64),
            "csv",
        ));
    }

    if raw_export_family_selected(&selected_data_families, RAW_EXPORT_SQLITE_FAMILY) {
        let sqlite_source_path = options
            .sqlite_source_path
            .ok_or_else(|| GooseError::message("sqlite data family requires sqlite_source_path"))?;
        let sqlite_target_path = options.output_dir.join("data/goose.sqlite");
        snapshot_sqlite_database(sqlite_source_path, &sqlite_target_path)?;
        let bytes = fs::read(&sqlite_target_path)
            .map_err(|source| GooseError::io(&sqlite_target_path, source))?;
        manifest_files.push(manifest_file("data/goose.sqlite", &bytes, None, "sqlite"));
    }

    let manifest = ExportManifest {
        schema_version: "goose.export.v1".to_string(),
        app_version: options.app_version.to_string(),
        core_version: options.core_version.to_string(),
        time_window: ExportTimeWindow {
            start: options.start.to_string(),
            end: options.end.to_string(),
        },
        data_families: selected_data_families.clone(),
        filters: filters.clone(),
        files: manifest_files,
        official_labels_are_labels: true,
    };

    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| GooseError::message(format!("cannot serialize manifest: {error}")))?;
    fs::write(options.output_dir.join("manifest.json"), manifest_json)
        .map_err(|source| GooseError::io(options.output_dir.join("manifest.json"), source))?;

    if let Some(zip_output_path) = options.zip_output_path {
        write_export_zip(options.output_dir, zip_output_path, &manifest)?;
    }

    let manifest_ready = !manifest.files.is_empty();
    let files_written = !manifest.files.is_empty();
    let zip_ready = options
        .zip_output_path
        .map(|path| path.exists())
        .unwrap_or(true);
    let input_valid = data_families_valid
        && filters_valid
        && time_window_valid
        && version_fields_valid
        && sqlite_policy_valid;
    let export_ready =
        input_valid && manifest_ready && files_written && zip_ready && issues.is_empty();
    Ok(RawExportReport {
        schema: "goose.raw-export-report.v1".to_string(),
        generated_by: "goose-raw-export".to_string(),
        output_dir: options.output_dir.display().to_string(),
        zip_path: options
            .zip_output_path
            .map(|path| path.display().to_string()),
        pass: export_ready,
        input_valid,
        data_families_valid,
        filters_valid,
        time_window_valid,
        version_fields_valid,
        sqlite_policy_valid,
        manifest_ready,
        files_written,
        zip_ready,
        export_ready,
        raw_rows: raw_rows.len(),
        decoded_frame_rows: decoded_rows.len(),
        packet_timeline_rows: packet_timeline_rows.len(),
        sensor_sample_rows: sensor_sample_rows.len(),
        metric_feature_report_rows: metric_feature_reports.len(),
        metric_value_rows: metric_value_rows.len(),
        metric_component_rows: metric_component_rows.len(),
        algorithm_run_rows: algorithm_run_report_rows,
        calibration_label_rows: calibration_labels.len(),
        calibration_run_rows: calibration_runs.len(),
        activity_session_rows: activity_session_rows.len(),
        activity_metric_rows: activity_metric_rows.len(),
        activity_interval_rows: activity_interval_rows.len(),
        activity_label_rows: activity_label_rows.len(),
        daily_activity_metric_rows: daily_activity_metric_rows.len(),
        hourly_activity_metric_rows: hourly_activity_metric_rows.len(),
        daily_recovery_metric_rows: daily_recovery_metric_rows.len(),
        metric_provenance_rows: metric_provenance_rows.len(),
        debug_session_rows: debug_sessions.len(),
        debug_command_rows: debug_commands.len(),
        debug_event_rows: debug_events.len(),
        command_validation_rows: command_validation_rows.len(),
        manifest,
        issues,
    })
}

fn validate_manifest_shape(manifest: &ExportManifest, issues: &mut Vec<String>) {
    if manifest.schema_version.trim().is_empty() {
        issues.push("manifest.schema_version is required".to_string());
    }
    if manifest.app_version.trim().is_empty() {
        issues.push("manifest.app_version is required".to_string());
    }
    if manifest.core_version.trim().is_empty() {
        issues.push("manifest.core_version is required".to_string());
    }
    if manifest.time_window.start.trim().is_empty() || manifest.time_window.end.trim().is_empty() {
        issues.push("manifest.time_window start/end are required".to_string());
    }
    if manifest.data_families.is_empty() {
        issues.push("manifest.data_families must list at least one family".to_string());
    }
    let mut seen_families = BTreeSet::new();
    for family in &manifest.data_families {
        if family.trim().is_empty() {
            issues.push("manifest.data_families must not include empty family".to_string());
            continue;
        }
        if !RAW_EXPORT_DATA_FAMILIES.contains(&family.as_str()) {
            issues.push(format!(
                "manifest.data_families contains unknown family {family}"
            ));
        }
        if !seen_families.insert(family.as_str()) {
            issues.push(format!(
                "manifest.data_families contains duplicate family {family}"
            ));
        }
    }
    if manifest.files.is_empty() {
        issues.push("manifest.files must list at least one file".to_string());
    }
    let mut seen_file_paths = BTreeSet::new();
    for file in &manifest.files {
        if !seen_file_paths.insert(file.path.as_str()) {
            issues.push(format!(
                "manifest.files contains duplicate path {}",
                file.path
            ));
        }
        match export_manifest_file_family(&file.path) {
            Some(family) if !family_is_listed(manifest, family) => issues.push(format!(
                "{} belongs to unselected data family {family}",
                file.path
            )),
            Some(_) => {}
            None => issues.push(format!(
                "{} is not a recognized Goose export artifact path",
                file.path
            )),
        }
    }
    for family in &manifest.data_families {
        for required_path in required_export_artifact_paths_for_family(family) {
            if !manifest_lists_path(manifest, required_path) {
                issues.push(format!("data family {family} requires {required_path}"));
            }
        }
    }
    if family_is_listed(manifest, "calibration_labels") && !manifest.official_labels_are_labels {
        issues.push(
            "manifest.official_labels_are_labels must be true when calibration_labels are exported"
                .to_string(),
        );
    }
    if family_is_listed(manifest, RAW_EXPORT_SQLITE_FAMILY) && !manifest.filters.include_raw_bytes {
        issues.push(
            "sqlite data family cannot be exported when include_raw_bytes is false".to_string(),
        );
    }
}

struct RawExportReadinessInput {
    data_families_valid: bool,
    filters_valid: bool,
    time_window_valid: bool,
    version_fields_valid: bool,
    sqlite_policy_valid: bool,
    manifest_ready: bool,
    files_written: bool,
    zip_ready: bool,
}

fn snapshot_sqlite_database(source_path: &Path, target_path: &Path) -> GooseResult<()> {
    fs::metadata(source_path).map_err(|source| GooseError::io(source_path, source))?;
    if target_path.exists() {
        fs::remove_file(target_path).map_err(|source| GooseError::io(target_path, source))?;
    }
    let connection = Connection::open(source_path).map_err(|error| {
        GooseError::message(format!(
            "cannot open SQLite source for raw export snapshot: {error}"
        ))
    })?;
    let target_literal = sql_string_literal(&target_path.display().to_string());
    connection
        .execute_batch(&format!("VACUUM INTO {target_literal};"))
        .map_err(|error| {
            GooseError::message(format!(
                "cannot snapshot SQLite source for raw export: {error}"
            ))
        })?;
    Ok(())
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn raw_export_report(
    options: RawExportOptions<'_>,
    data_families: Vec<String>,
    filters: RawExportFilters,
    readiness: RawExportReadinessInput,
    issues: Vec<String>,
) -> GooseResult<RawExportReport> {
    let input_valid = readiness.data_families_valid
        && readiness.filters_valid
        && readiness.time_window_valid
        && readiness.version_fields_valid
        && readiness.sqlite_policy_valid;
    let export_ready = input_valid
        && readiness.manifest_ready
        && readiness.files_written
        && readiness.zip_ready
        && issues.is_empty();
    Ok(RawExportReport {
        schema: "goose.raw-export-report.v1".to_string(),
        generated_by: "goose-raw-export".to_string(),
        output_dir: options.output_dir.display().to_string(),
        zip_path: options
            .zip_output_path
            .map(|path| path.display().to_string()),
        pass: export_ready,
        input_valid,
        data_families_valid: readiness.data_families_valid,
        filters_valid: readiness.filters_valid,
        time_window_valid: readiness.time_window_valid,
        version_fields_valid: readiness.version_fields_valid,
        sqlite_policy_valid: readiness.sqlite_policy_valid,
        manifest_ready: readiness.manifest_ready,
        files_written: readiness.files_written,
        zip_ready: readiness.zip_ready,
        export_ready,
        raw_rows: 0,
        decoded_frame_rows: 0,
        packet_timeline_rows: 0,
        sensor_sample_rows: 0,
        metric_feature_report_rows: 0,
        metric_value_rows: 0,
        metric_component_rows: 0,
        algorithm_run_rows: 0,
        calibration_label_rows: 0,
        calibration_run_rows: 0,
        activity_session_rows: 0,
        activity_metric_rows: 0,
        activity_interval_rows: 0,
        activity_label_rows: 0,
        daily_activity_metric_rows: 0,
        hourly_activity_metric_rows: 0,
        daily_recovery_metric_rows: 0,
        metric_provenance_rows: 0,
        debug_session_rows: 0,
        debug_command_rows: 0,
        debug_event_rows: 0,
        command_validation_rows: 0,
        manifest: ExportManifest {
            schema_version: "goose.export.v1".to_string(),
            app_version: options.app_version.to_string(),
            core_version: options.core_version.to_string(),
            time_window: ExportTimeWindow {
                start: options.start.to_string(),
                end: options.end.to_string(),
            },
            data_families,
            filters,
            files: Vec::new(),
            official_labels_are_labels: true,
        },
        issues,
    })
}

fn normalize_raw_export_data_families(
    requested: &[String],
    default_include_sqlite: bool,
    issues: &mut Vec<String>,
) -> Vec<String> {
    if requested.is_empty() {
        return default_raw_export_data_families(default_include_sqlite);
    }

    let mut requested_set = BTreeSet::new();
    for family in requested {
        let family = family.trim();
        if family.is_empty() {
            issues.push("data_families must not include empty family".to_string());
            continue;
        }
        if !RAW_EXPORT_DATA_FAMILIES.contains(&family) {
            issues.push(format!("unknown data family: {family}"));
            continue;
        }
        requested_set.insert(family.to_string());
    }

    let selected = RAW_EXPORT_DATA_FAMILIES
        .iter()
        .filter(|family| requested_set.contains::<str>(*family))
        .map(|family| (*family).to_string())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        issues.push("at least one data family must be selected".to_string());
    }
    selected
}

fn normalize_raw_export_filters(
    requested: &RawExportFilters,
    issues: &mut Vec<String>,
) -> RawExportFilters {
    RawExportFilters {
        include_raw_bytes: requested.include_raw_bytes,
        capture_session_ids: normalize_raw_export_filter_values(
            "filters.capture_session_ids",
            &requested.capture_session_ids,
            issues,
        ),
        packet_type_names: normalize_raw_export_filter_values(
            "filters.packet_type_names",
            &requested.packet_type_names,
            issues,
        ),
        sensor_source_signals: normalize_raw_export_filter_values(
            "filters.sensor_source_signals",
            &requested.sensor_source_signals,
            issues,
        ),
        metric_families: normalize_raw_export_filter_values(
            "filters.metric_families",
            &requested.metric_families,
            issues,
        ),
        algorithm_ids: normalize_raw_export_filter_values(
            "filters.algorithm_ids",
            &requested.algorithm_ids,
            issues,
        ),
        algorithm_versions: normalize_raw_export_filter_values(
            "filters.algorithm_versions",
            &requested.algorithm_versions,
            issues,
        ),
    }
}

fn normalize_raw_export_filter_values(
    field: &str,
    requested: &[String],
    issues: &mut Vec<String>,
) -> Vec<String> {
    let mut values = BTreeSet::new();
    for value in requested {
        let value = value.trim();
        if value.is_empty() {
            issues.push(format!("{field} must not include empty values"));
        } else {
            values.insert(value.to_string());
        }
    }
    values.into_iter().collect()
}

fn default_include_raw_bytes() -> bool {
    true
}

fn raw_export_family_selected(data_families: &[String], family: &str) -> bool {
    data_families
        .iter()
        .any(|data_family| data_family == family)
}

fn raw_evidence_rows_between_for_raw_export(
    store: &GooseStore,
    start: &str,
    end: &str,
    filters: &RawExportFilters,
) -> GooseResult<Vec<RawEvidenceRow>> {
    let rows = raw_evidence_rows_between_capture_scoped(store, start, end, filters)?;
    if !decoded_scope_filters_active(filters) {
        return Ok(rows);
    }

    let decoded_rows = decoded_rows_between_for_raw_export(store, start, end, filters)?;
    let evidence_ids = decoded_evidence_id_set(&decoded_rows);
    Ok(filter_raw_rows_by_evidence_ids(rows, &evidence_ids))
}

fn decoded_rows_between_for_raw_export(
    store: &GooseStore,
    start: &str,
    end: &str,
    filters: &RawExportFilters,
) -> GooseResult<Vec<DecodedFrameRow>> {
    let mut rows = store.decoded_frames_between(start, end)?;

    if !filters.capture_session_ids.is_empty() {
        let raw_rows = raw_evidence_rows_between_capture_scoped(store, start, end, filters)?;
        let evidence_ids = raw_evidence_id_set(&raw_rows);
        rows = filter_decoded_rows_by_evidence_ids(rows, &evidence_ids);
    }

    rows = filter_decoded_rows_by_packet_type(rows, filters);
    filter_decoded_rows_by_sensor_source(rows, filters)
}

fn raw_evidence_rows_between_capture_scoped(
    store: &GooseStore,
    start: &str,
    end: &str,
    filters: &RawExportFilters,
) -> GooseResult<Vec<RawEvidenceRow>> {
    let rows = store.raw_evidence_between(start, end)?;
    Ok(filter_raw_evidence_rows_by_capture_session(rows, filters))
}

fn filter_raw_evidence_rows_by_capture_session(
    rows: Vec<RawEvidenceRow>,
    filters: &RawExportFilters,
) -> Vec<RawEvidenceRow> {
    if filters.capture_session_ids.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| {
            row.capture_session_id.as_deref().is_some_and(|session_id| {
                filter_value_matches(&filters.capture_session_ids, session_id)
            })
        })
        .collect()
}

fn filter_raw_rows_by_evidence_ids(
    rows: Vec<RawEvidenceRow>,
    evidence_ids: &BTreeSet<String>,
) -> Vec<RawEvidenceRow> {
    rows.into_iter()
        .filter(|row| evidence_ids.contains(&row.evidence_id))
        .collect()
}

fn raw_evidence_id_set(rows: &[RawEvidenceRow]) -> BTreeSet<String> {
    rows.iter()
        .map(|row| row.evidence_id.clone())
        .collect::<BTreeSet<_>>()
}

fn decoded_evidence_id_set(rows: &[DecodedFrameRow]) -> BTreeSet<String> {
    rows.iter()
        .map(|row| row.evidence_id.clone())
        .collect::<BTreeSet<_>>()
}

fn filter_decoded_rows_by_evidence_ids(
    rows: Vec<DecodedFrameRow>,
    evidence_ids: &BTreeSet<String>,
) -> Vec<DecodedFrameRow> {
    rows.into_iter()
        .filter(|row| evidence_ids.contains(&row.evidence_id))
        .collect()
}

fn filter_decoded_rows_by_packet_type(
    rows: Vec<DecodedFrameRow>,
    filters: &RawExportFilters,
) -> Vec<DecodedFrameRow> {
    if filters.packet_type_names.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| {
            row.packet_type_name
                .as_deref()
                .is_some_and(|name| filter_value_matches(&filters.packet_type_names, name))
        })
        .collect()
}

fn filter_decoded_rows_by_sensor_source(
    rows: Vec<DecodedFrameRow>,
    filters: &RawExportFilters,
) -> GooseResult<Vec<DecodedFrameRow>> {
    if filters.sensor_source_signals.is_empty() {
        return Ok(rows);
    }
    let matching_frame_ids = filter_sensor_sample_rows(export_sensor_samples(&rows)?, filters)
        .into_iter()
        .map(|row| row.frame_id)
        .collect::<BTreeSet<_>>();
    Ok(rows
        .into_iter()
        .filter(|row| matching_frame_ids.contains(&row.frame_id))
        .collect())
}

fn filter_sensor_sample_rows(
    rows: Vec<ExportSensorSampleRow>,
    filters: &RawExportFilters,
) -> Vec<ExportSensorSampleRow> {
    if filters.sensor_source_signals.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| filter_value_matches(&filters.sensor_source_signals, &row.source_signal))
        .collect()
}

fn decoded_scope_filters_active(filters: &RawExportFilters) -> bool {
    !filters.packet_type_names.is_empty() || !filters.sensor_source_signals.is_empty()
}

fn raw_evidence_rows_for_raw_byte_policy(
    rows: &[RawEvidenceRow],
    filters: &RawExportFilters,
) -> Vec<RawEvidenceRow> {
    if filters.include_raw_bytes {
        return rows.to_vec();
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.payload_hex.clear();
            row
        })
        .collect()
}

fn decoded_rows_for_raw_byte_policy(
    rows: &[DecodedFrameRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<DecodedFrameRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.payload_hex.clear();
            row.parsed_payload_json = raw_byte_policy_json_text(&row.parsed_payload_json)?;
            Ok(row)
        })
        .collect()
}

fn packet_timeline_rows_for_raw_byte_policy(
    rows: &[PacketTimelineRow],
    filters: &RawExportFilters,
) -> Vec<PacketTimelineRow> {
    if filters.include_raw_bytes {
        return rows.to_vec();
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.body_hex = None;
            row.summary = raw_byte_policy_json_value(row.summary);
            row
        })
        .collect()
}

fn debug_commands_for_raw_byte_policy(
    rows: &[ExportDebugCommandRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ExportDebugCommandRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.args_json = raw_byte_policy_json_text_if_valid(&row.args_json)?;
            Ok(row)
        })
        .collect()
}

fn debug_events_for_raw_byte_policy(
    rows: &[ExportDebugEventRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ExportDebugEventRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.data_json = raw_byte_policy_json_text_if_valid(&row.data_json)?;
            Ok(row)
        })
        .collect()
}

fn command_validation_rows_for_raw_byte_policy(
    rows: &[ExportCommandValidationRow],
    filters: &RawExportFilters,
) -> Vec<ExportCommandValidationRow> {
    if filters.include_raw_bytes {
        return rows.to_vec();
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.report_json = raw_byte_policy_json_value(row.report_json);
            row
        })
        .collect()
}

fn raw_byte_policy_json_text(text: &str) -> GooseResult<String> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        GooseError::message(format!("cannot parse JSON for raw-byte policy: {error}"))
    })?;
    serde_json::to_string(&raw_byte_policy_json_value(value)).map_err(|error| {
        GooseError::message(format!("cannot serialize raw-byte policy JSON: {error}"))
    })
}

fn raw_byte_policy_json_text_if_valid(text: &str) -> GooseResult<String> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => serde_json::to_string(&raw_byte_policy_json_value(value)).map_err(|error| {
            GooseError::message(format!("cannot serialize raw-byte policy JSON: {error}"))
        }),
        Err(_) => Ok(text.to_string()),
    }
}

fn raw_byte_policy_json_value(mut value: Value) -> Value {
    redact_raw_byte_json_value(&mut value);
    value
}

fn redact_raw_byte_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_raw_byte_json_key(key) {
                    *value = Value::String(String::new());
                } else {
                    redact_raw_byte_json_value(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_raw_byte_json_value(value);
            }
        }
        _ => {}
    }
}

fn is_raw_byte_json_key(key: &str) -> bool {
    key.ends_with("_hex")
        || key.ends_with("_bytes")
        || key == "frame_hex"
        || key == "payload_hex"
        || key == "body_hex"
        || key == "data_hex"
}

fn write_jsonl<T: Serialize>(path: &Path, rows: &[T]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut bytes, row)
            .map_err(|error| GooseError::message(format!("cannot serialize JSONL row: {error}")))?;
        bytes.push(b'\n');
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_raw_csv(path: &Path, rows: &[RawEvidenceRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "evidence_id,source,captured_at,device_model,payload_hex,sha256,sensitivity,capture_session_id"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.evidence_id,
                &row.source,
                &row.captured_at,
                &row.device_model,
                &row.payload_hex,
                &row.sha256,
                &row.sensitivity,
                row.capture_session_id.as_deref().unwrap_or_default(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_decoded_csv(path: &Path, rows: &[DecodedFrameRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "frame_id,evidence_id,captured_at,device_type,raw_len,header_len,declared_len,payload_hex,payload_crc_hex,header_crc_valid,payload_crc_valid,packet_type,packet_type_name,sequence,command_or_event,parsed_payload_json,parser_version,warnings_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.frame_id,
                &row.evidence_id,
                &row.captured_at,
                &row.device_type,
                &row.raw_len.to_string(),
                &row.header_len.to_string(),
                &row.declared_len.to_string(),
                &row.payload_hex,
                &row.payload_crc_hex,
                &row.header_crc_valid.to_string(),
                &row.payload_crc_valid.to_string(),
                &row.packet_type
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.packet_type_name.as_deref().unwrap_or_default(),
                &row.sequence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.command_or_event
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.parsed_payload_json,
                &row.parser_version,
                &row.warnings_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_packet_timeline_csv(path: &Path, rows: &[PacketTimelineRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "timeline_id,frame_id,evidence_id,captured_at,category,title,packet_type_name,sequence,command_or_event,device_timestamp_seconds,device_timestamp_subseconds,body_hex,summary,warnings"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.timeline_id,
                &row.frame_id,
                &row.evidence_id,
                &row.captured_at,
                &row.category,
                &row.title,
                row.packet_type_name.as_deref().unwrap_or_default(),
                &row.sequence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.command_or_event
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.device_timestamp_seconds
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.device_timestamp_subseconds
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.body_hex.as_deref().unwrap_or_default(),
                &row.summary.to_string(),
                &serde_json::to_string(&row.warnings)
                    .map_err(|error| GooseError::message(error.to_string()))?,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_sensor_samples_csv(path: &Path, rows: &[ExportSensorSampleRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "sample_id,frame_id,evidence_id,captured_at,sample_time,sample_time_unix_ms,sample_time_source,source_signal,packet_type_name,packet_k,domain,series_name,sample_index,payload_offset,raw_i16,raw_u8,sample_value,unit,device_timestamp_seconds,device_timestamp_subseconds,parser_version,quality_flags_json,provenance_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let quality_flags_json = serde_json::to_string(&row.quality_flags)
            .map_err(|error| GooseError::message(error.to_string()))?;
        write_csv_row(
            &mut bytes,
            &[
                &row.sample_id,
                &row.frame_id,
                &row.evidence_id,
                &row.captured_at,
                &row.sample_time,
                &row.sample_time_unix_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.sample_time_source,
                &row.source_signal,
                row.packet_type_name.as_deref().unwrap_or_default(),
                &row.packet_k
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.domain.as_deref().unwrap_or_default(),
                &row.series_name,
                &row.sample_index.to_string(),
                &row.payload_offset.to_string(),
                &row.raw_i16
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.raw_u8
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.sample_value.to_string(),
                &row.unit,
                &row.device_timestamp_seconds
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.device_timestamp_subseconds
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.parser_version,
                &quality_flags_json,
                &row.provenance.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn export_sensor_samples(
    decoded_rows: &[DecodedFrameRow],
) -> GooseResult<Vec<ExportSensorSampleRow>> {
    let mut rows = Vec::new();
    for row in decoded_rows {
        let parsed_payload: ParsedPayload = serde_json::from_str(&row.parsed_payload_json)
            .map_err(|error| {
                GooseError::message(format!(
                    "{} parsed_payload_json invalid for sensor export: {error}",
                    row.frame_id
                ))
            })?;
        let ParsedPayload::DataPacket {
            packet_k,
            domain,
            timestamp_seconds,
            timestamp_subseconds,
            hr_marker_offset,
            hr_present_marker,
            body_summary,
            ..
        } = parsed_payload
        else {
            continue;
        };
        let Some(body_summary) = body_summary else {
            continue;
        };
        let payload = decode_hex_with_whitespace(&row.payload_hex)?;
        let context = SensorSampleContext {
            row,
            packet_k,
            domain: domain.as_deref(),
            timestamp_seconds,
            timestamp_subseconds,
        };
        match body_summary {
            DataPacketBodySummary::NormalHistory {
                hr_present,
                marker_offset,
                marker_value,
            } => {
                if hr_present.unwrap_or(false)
                    && let (Some(offset), Some(value)) = (
                        marker_offset.or(hr_marker_offset),
                        marker_value.or(hr_present_marker),
                    )
                {
                    rows.push(sensor_u8_sample(
                        &context,
                        "normal_history_hr_marker",
                        "heart_rate_marker",
                        0,
                        offset,
                        value,
                        "bpm_candidate",
                        vec!["raw_history_marker".to_string()],
                    ));
                }
            }
            DataPacketBodySummary::R17OpticalOrLabradorFiltered {
                samples, warnings, ..
            } => {
                if let Some(samples) = samples {
                    push_i16_sensor_series(
                        &mut rows,
                        &context,
                        &payload,
                        "r17_optical_or_labrador_filtered",
                        &samples,
                        &warnings,
                    )?;
                }
            }
            DataPacketBodySummary::RawMotionK10 {
                heart_rate,
                axes,
                warnings,
            } => {
                if let Some(value) = heart_rate.filter(|value| *value > 0) {
                    rows.push(sensor_u8_sample(
                        &context,
                        "raw_motion_k10_heart_rate",
                        "raw_motion_embedded_heart_rate",
                        0,
                        17,
                        value,
                        "bpm_candidate",
                        vec!["raw_motion_embedded_hr".to_string()],
                    ));
                }
                for axis in axes {
                    push_i16_sensor_series(
                        &mut rows,
                        &context,
                        &payload,
                        "raw_motion_k10",
                        &axis,
                        &warnings,
                    )?;
                }
            }
            DataPacketBodySummary::RawMotionK21 { axes, warnings, .. } => {
                for axis in axes {
                    push_i16_sensor_series(
                        &mut rows,
                        &context,
                        &payload,
                        "raw_motion_k21",
                        &axis,
                        &warnings,
                    )?;
                }
            }
            DataPacketBodySummary::V24History { .. } => {
                // V24 biometric data is exported via dedicated v24 tables (Phase 27).
                // Sensor sample export path does not handle V24 — skip gracefully.
            }
            DataPacketBodySummary::R22Whoop5Hr { .. } => {
                // R22 WHOOP 5.0 realtime data is surfaced via the upload pipeline.
                // Sensor sample export path does not export R22 frames — skip gracefully.
            }
            DataPacketBodySummary::V18History { .. } => {
                // V18 WHOOP 5.0 historical data is persisted via the upload pipeline.
                // Sensor sample export path does not export V18 frames — skip gracefully.
            }
        }
    }
    Ok(rows)
}

struct SensorSampleContext<'a> {
    row: &'a DecodedFrameRow,
    packet_k: Option<u8>,
    domain: Option<&'a str>,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
}

#[derive(Debug, Clone)]
struct NormalizedSensorSampleTime {
    time: String,
    unix_ms: Option<i64>,
    source: String,
}

fn push_i16_sensor_series(
    rows: &mut Vec<ExportSensorSampleRow>,
    context: &SensorSampleContext<'_>,
    payload: &[u8],
    source_signal: &str,
    series: &I16SeriesSummary,
    summary_warnings: &[String],
) -> GooseResult<()> {
    if series.parsed_count < series.expected_count
        && !summary_warnings
            .iter()
            .any(|warning| warning == &format!("{}_truncated", series.name))
    {
        return Err(GooseError::message(format!(
            "{} summary for {} is truncated without a warning",
            context.row.frame_id, series.name
        )));
    }
    for sample_index in 0..series.parsed_count {
        let payload_offset = series.offset + sample_index * 2;
        let value = read_i16_le_at(payload, payload_offset).ok_or_else(|| {
            GooseError::message(format!(
                "{} {} sample {} is outside payload",
                context.row.frame_id, series.name, sample_index
            ))
        })?;
        let mut quality_flags = Vec::new();
        if series.parsed_count < series.expected_count {
            quality_flags.push(format!("{}_truncated", series.name));
        }
        rows.push(sensor_i16_sample(
            context,
            source_signal,
            &series.name,
            sample_index,
            payload_offset,
            value,
            quality_flags,
        ));
    }
    Ok(())
}

fn sensor_i16_sample(
    context: &SensorSampleContext<'_>,
    source_signal: &str,
    series_name: &str,
    sample_index: usize,
    payload_offset: usize,
    value: i16,
    mut quality_flags: Vec<String>,
) -> ExportSensorSampleRow {
    let sample_time = normalized_sensor_sample_time(
        context.row,
        context.timestamp_seconds,
        context.timestamp_subseconds,
        &mut quality_flags,
    );
    ExportSensorSampleRow {
        sample_id: sensor_sample_id(&context.row.frame_id, series_name, sample_index),
        frame_id: context.row.frame_id.clone(),
        evidence_id: context.row.evidence_id.clone(),
        captured_at: context.row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        source_signal: source_signal.to_string(),
        packet_type_name: context.row.packet_type_name.clone(),
        packet_k: context.packet_k,
        domain: context.domain.map(ToOwned::to_owned),
        series_name: series_name.to_string(),
        sample_index,
        payload_offset,
        raw_i16: Some(value),
        raw_u8: None,
        sample_value: i64::from(value),
        unit: "raw_i16".to_string(),
        device_timestamp_seconds: context.timestamp_seconds,
        device_timestamp_subseconds: context.timestamp_subseconds,
        parser_version: context.row.parser_version.clone(),
        quality_flags,
        provenance: json!({
            "input_source": "decoded_frame_payload",
            "frame_id": context.row.frame_id,
            "evidence_id": context.row.evidence_id,
            "parser_version": context.row.parser_version,
            "source_signal": source_signal,
            "series_name": series_name,
            "sample_index": sample_index,
            "payload_offset": payload_offset,
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": context.timestamp_seconds,
            "device_timestamp_subseconds": context.timestamp_subseconds,
            "unit_policy": "raw_signed_i16_no_physiological_unit_claim",
        }),
    }
}

fn sensor_u8_sample(
    context: &SensorSampleContext<'_>,
    source_signal: &str,
    series_name: &str,
    sample_index: usize,
    payload_offset: usize,
    value: u8,
    unit: &str,
    mut quality_flags: Vec<String>,
) -> ExportSensorSampleRow {
    let sample_time = normalized_sensor_sample_time(
        context.row,
        context.timestamp_seconds,
        context.timestamp_subseconds,
        &mut quality_flags,
    );
    ExportSensorSampleRow {
        sample_id: sensor_sample_id(&context.row.frame_id, series_name, sample_index),
        frame_id: context.row.frame_id.clone(),
        evidence_id: context.row.evidence_id.clone(),
        captured_at: context.row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        source_signal: source_signal.to_string(),
        packet_type_name: context.row.packet_type_name.clone(),
        packet_k: context.packet_k,
        domain: context.domain.map(ToOwned::to_owned),
        series_name: series_name.to_string(),
        sample_index,
        payload_offset,
        raw_i16: None,
        raw_u8: Some(value),
        sample_value: i64::from(value),
        unit: unit.to_string(),
        device_timestamp_seconds: context.timestamp_seconds,
        device_timestamp_subseconds: context.timestamp_subseconds,
        parser_version: context.row.parser_version.clone(),
        quality_flags,
        provenance: json!({
            "input_source": "decoded_frame_payload",
            "frame_id": context.row.frame_id,
            "evidence_id": context.row.evidence_id,
            "parser_version": context.row.parser_version,
            "source_signal": source_signal,
            "series_name": series_name,
            "sample_index": sample_index,
            "payload_offset": payload_offset,
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": context.timestamp_seconds,
            "device_timestamp_subseconds": context.timestamp_subseconds,
            "unit_policy": unit,
        }),
    }
}

fn sensor_sample_id(frame_id: &str, series_name: &str, sample_index: usize) -> String {
    format!("{frame_id}.{series_name}.{sample_index}")
}

fn read_i16_le_at(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn normalized_sensor_sample_time(
    row: &DecodedFrameRow,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
    quality_flags: &mut Vec<String>,
) -> NormalizedSensorSampleTime {
    if let Some(seconds) = timestamp_seconds
        && plausible_unix_timestamp_seconds(seconds)
    {
        if let Some(subseconds) = timestamp_subseconds
            && subseconds > 999
        {
            push_quality_flag_once(quality_flags, "device_timestamp_subseconds_out_of_range");
            push_quality_flag_once(quality_flags, "sample_time_from_capture_time");
            return NormalizedSensorSampleTime {
                time: row.captured_at.clone(),
                unix_ms: parse_rfc3339_utc_unix_ms(&row.captured_at),
                source: "captured_at".to_string(),
            };
        }
        push_quality_flag_once(quality_flags, "sample_time_from_device_timestamp");
        let millis = timestamp_subseconds.map_or(0, i64::from);
        let unix_ms = i64::from(seconds) * 1_000 + millis;
        return NormalizedSensorSampleTime {
            time: unix_ms_to_rfc3339_utc(unix_ms),
            unix_ms: Some(unix_ms),
            source: "device_timestamp".to_string(),
        };
    }

    if timestamp_seconds.is_some() {
        push_quality_flag_once(
            quality_flags,
            "device_timestamp_outside_plausible_unix_range",
        );
    } else {
        push_quality_flag_once(quality_flags, "device_timestamp_missing");
    }
    push_quality_flag_once(quality_flags, "sample_time_from_capture_time");
    NormalizedSensorSampleTime {
        time: row.captured_at.clone(),
        unix_ms: parse_rfc3339_utc_unix_ms(&row.captured_at),
        source: "captured_at".to_string(),
    }
}

fn push_quality_flag_once(quality_flags: &mut Vec<String>, flag: &str) {
    if !quality_flags.iter().any(|existing| existing == flag) {
        quality_flags.push(flag.to_string());
    }
}

fn plausible_unix_timestamp_seconds(seconds: u32) -> bool {
    (946_684_800..=4_102_444_800).contains(&seconds)
}

fn write_metric_feature_reports_csv(
    path: &Path,
    rows: &[ExportMetricFeatureReportRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "report_kind,schema,start_time,end_time,pass,feature_count,trusted_feature_count,issue_count,issues_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.report_kind,
                &row.schema,
                &row.start_time,
                &row.end_time,
                &row.pass.to_string(),
                &row.feature_count.to_string(),
                &row.trusted_feature_count.to_string(),
                &row.issue_count.to_string(),
                &row.issues_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn export_metric_feature_reports(
    store: &GooseStore,
    evidence_scope: &str,
    start: &str,
    end: &str,
) -> GooseResult<Vec<ExportMetricFeatureReportRow>> {
    let mut rows = Vec::new();
    push_metric_feature_report(
        &mut rows,
        "motion",
        start,
        end,
        run_motion_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            MotionFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "heart_rate",
        start,
        end,
        run_heart_rate_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            HeartRateFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "vital_event",
        start,
        end,
        run_vital_event_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            VitalEventFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "hrv",
        start,
        end,
        run_hrv_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            HrvFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "metric_window",
        start,
        end,
        run_metric_window_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            MetricWindowFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "resting_heart_rate",
        start,
        end,
        run_resting_heart_rate_feature_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            RestingHeartRateFeatureOptions::default(),
        )?,
    )?;
    push_metric_feature_report(
        &mut rows,
        "sleep_score_from_features",
        start,
        end,
        run_sleep_feature_score_report_for_store(
            store,
            evidence_scope,
            start,
            end,
            SleepFeatureScoreOptions::default(),
        )?,
    )?;
    Ok(rows)
}

fn push_metric_feature_report(
    rows: &mut Vec<ExportMetricFeatureReportRow>,
    report_kind: &str,
    start: &str,
    end: &str,
    report: impl Serialize,
) -> GooseResult<()> {
    let report_json = serde_json::to_value(report).map_err(|error| {
        GooseError::message(format!("cannot serialize metric feature report: {error}"))
    })?;
    let schema = string_field(&report_json, "schema").unwrap_or_default();
    let issues_json = report_json
        .get("issues")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let issues_json = serde_json::to_string(&issues_json)
        .map_err(|error| GooseError::message(format!("cannot serialize report issues: {error}")))?;
    rows.push(ExportMetricFeatureReportRow {
        report_kind: report_kind.to_string(),
        schema,
        start_time: string_field(&report_json, "start_time").unwrap_or_else(|| start.to_string()),
        end_time: string_field(&report_json, "end_time").unwrap_or_else(|| end.to_string()),
        pass: report_json
            .get("pass")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        feature_count: metric_report_feature_count(report_kind, &report_json),
        trusted_feature_count: metric_report_trusted_feature_count(report_kind, &report_json),
        issue_count: report_json
            .get("issues")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        issues_json,
        report_json,
    });
    Ok(())
}

fn metric_report_feature_count(report_kind: &str, report: &Value) -> usize {
    if let Some(count) = usize_field(report, "feature_count") {
        return count;
    }
    match report_kind {
        "metric_window" => usize::from(report.get("window").is_some_and(|value| !value.is_null())),
        "resting_heart_rate" => {
            usize::from(report.get("resting").is_some_and(|value| !value.is_null()))
                + usize::from(report.get("baseline").is_some_and(|value| !value.is_null()))
                + usize_field(report, "daily_count").unwrap_or_default()
        }
        "sleep_score_from_features" => usize::from(
            report
                .get("sleep_window")
                .is_some_and(|value| !value.is_null()),
        ),
        _ => 0,
    }
}

fn metric_report_trusted_feature_count(report_kind: &str, report: &Value) -> usize {
    if let Some(count) = usize_field(report, "trusted_feature_count") {
        return count;
    }
    match report_kind {
        "metric_window" => report
            .get("window")
            .and_then(|window| window.get("trusted_metric_input"))
            .and_then(Value::as_bool)
            .map_or(0, usize::from),
        "resting_heart_rate" => {
            trusted_feature_object_count(report.get("resting"))
                + trusted_feature_object_count(report.get("baseline"))
                + report
                    .get("daily")
                    .and_then(Value::as_array)
                    .map(|daily| {
                        daily
                            .iter()
                            .filter(|value| {
                                value
                                    .get("trusted_metric_input")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false)
                            })
                            .count()
                    })
                    .unwrap_or_default()
        }
        "sleep_score_from_features" => report
            .get("sleep_window")
            .and_then(|window| window.get("trusted_metric_input"))
            .and_then(Value::as_bool)
            .map_or(0, usize::from),
        _ => 0,
    }
}

fn trusted_feature_object_count(value: Option<&Value>) -> usize {
    value
        .and_then(|value| value.get("trusted_metric_input"))
        .and_then(Value::as_bool)
        .map_or(0, usize::from)
}

fn filter_metric_feature_reports(
    rows: Vec<ExportMetricFeatureReportRow>,
    filters: &RawExportFilters,
) -> Vec<ExportMetricFeatureReportRow> {
    if filters.metric_families.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| {
            metric_feature_report_families(&row.report_kind)
                .iter()
                .any(|family| {
                    filters
                        .metric_families
                        .iter()
                        .any(|filter| filter == family)
                })
        })
        .collect()
}

fn metric_feature_report_families(report_kind: &str) -> &'static [&'static str] {
    match report_kind {
        "motion" => &["motion"],
        "heart_rate" => &["heart_rate"],
        "vital_event" => &["vital_event"],
        "hrv" => &["hrv"],
        "metric_window" => &["heart_rate", "motion", "strain", "stress"],
        "resting_heart_rate" => &["resting_heart_rate", "recovery", "stress"],
        "sleep_score_from_features" => &["sleep", "recovery"],
        _ => &[],
    }
}

fn filter_algorithm_runs(
    rows: Vec<AlgorithmRunRecord>,
    filters: &RawExportFilters,
) -> Vec<AlgorithmRunRecord> {
    rows.into_iter()
        .filter(|row| algorithm_run_matches_filters(row, filters))
        .collect()
}

fn algorithm_run_matches_filters(row: &AlgorithmRunRecord, filters: &RawExportFilters) -> bool {
    filter_value_matches(&filters.algorithm_ids, &row.algorithm_id)
        && filter_value_matches(&filters.algorithm_versions, &row.version)
        && filter_value_matches(
            &filters.metric_families,
            &metric_family_for_algorithm(&row.algorithm_id),
        )
}

fn filter_calibration_labels(
    rows: Vec<ExportCalibrationLabelRow>,
    filters: &RawExportFilters,
) -> Vec<ExportCalibrationLabelRow> {
    rows.into_iter()
        .filter(|row| filter_value_matches(&filters.metric_families, &row.metric_family))
        .collect()
}

fn filter_calibration_runs(
    rows: Vec<CalibrationRunRecord>,
    filters: &RawExportFilters,
) -> Vec<CalibrationRunRecord> {
    rows.into_iter()
        .filter(|row| {
            filter_value_matches(&filters.algorithm_ids, &row.algorithm_id)
                && filter_value_matches(&filters.algorithm_versions, &row.version)
                && filter_value_matches(
                    &filters.metric_families,
                    &metric_family_for_algorithm(&row.algorithm_id),
                )
        })
        .collect()
}

fn filter_value_matches(filters: &[String], value: &str) -> bool {
    filters.is_empty() || filters.iter().any(|filter| filter == value)
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn usize_field(value: &Value, field: &str) -> Option<usize> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
}

fn write_metric_values_csv(path: &Path, rows: &[ExportMetricValueRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "metric_value_id,run_id,algorithm_id,version,metric_family,name,value,unit,start_time,end_time,quality_flags_json,provenance_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let quality_flags_json = serde_json::to_string(&row.quality_flags)
            .map_err(|error| GooseError::message(error.to_string()))?;
        write_csv_row(
            &mut bytes,
            &[
                &row.metric_value_id,
                &row.run_id,
                &row.algorithm_id,
                &row.version,
                &row.metric_family,
                &row.name,
                &row.value.to_string(),
                &row.unit,
                &row.start_time,
                &row.end_time,
                &quality_flags_json,
                &row.provenance.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_metric_components_csv(
    path: &Path,
    rows: &[ExportMetricComponentRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "metric_component_id,run_id,algorithm_id,version,metric_family,component_name,value,unit,score_0_to_100,weight,contribution,contribution_json,start_time,end_time,quality_flags_json,provenance_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let quality_flags_json = serde_json::to_string(&row.quality_flags)
            .map_err(|error| GooseError::message(error.to_string()))?;
        write_csv_row(
            &mut bytes,
            &[
                &row.metric_component_id,
                &row.run_id,
                &row.algorithm_id,
                &row.version,
                &row.metric_family,
                &row.component_name,
                &row.value.to_string(),
                &row.unit,
                &row.score_0_to_100
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.weight
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.contribution
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.contribution_json.to_string(),
                &row.start_time,
                &row.end_time,
                &quality_flags_json,
                &row.provenance.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn export_metric_outputs(
    algorithm_runs: &[AlgorithmRunRecord],
) -> GooseResult<(Vec<ExportMetricValueRow>, Vec<ExportMetricComponentRow>)> {
    let mut values = Vec::new();
    let mut components = Vec::new();
    for run in algorithm_runs {
        let output: Value = serde_json::from_str(&run.output_json).map_err(|error| {
            GooseError::message(format!(
                "{} output_json invalid for metric output export: {error}",
                run.run_id
            ))
        })?;
        let Some(output_object) = output.as_object() else {
            continue;
        };
        let metric_family = metric_family_for_algorithm(&run.algorithm_id);
        let quality_flags = parse_quality_flags_for_metric_output(run)?;
        for (name, value) in output_object {
            if name == "algorithm_id" || name == "algorithm_version" || name == "components" {
                continue;
            }
            let Some(value) = numeric_json_value(value) else {
                continue;
            };
            values.push(ExportMetricValueRow {
                metric_value_id: format!("{}.{}", run.run_id, name),
                run_id: run.run_id.clone(),
                algorithm_id: run.algorithm_id.clone(),
                version: run.version.clone(),
                metric_family: metric_family.clone(),
                name: name.clone(),
                value,
                unit: unit_for_metric_value(name),
                start_time: run.start_time.clone(),
                end_time: run.end_time.clone(),
                quality_flags: quality_flags.clone(),
                provenance: json!({
                    "input_source": "algorithm_run.output_json",
                    "run_id": run.run_id,
                    "algorithm_id": run.algorithm_id,
                    "version": run.version,
                    "field": name,
                }),
            });
        }
        if let Some(component_values) = output_object.get("components").and_then(Value::as_array) {
            for (index, component) in component_values.iter().enumerate() {
                let component_name = component
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed_component");
                let Some(value) = component.get("value").and_then(numeric_json_value) else {
                    continue;
                };
                let unit = component
                    .get("unit")
                    .and_then(Value::as_str)
                    .unwrap_or("raw")
                    .to_string();
                let score_0_to_100 = component.get("score_0_to_100").and_then(numeric_json_value);
                let weight = component.get("weight").and_then(numeric_json_value);
                let contribution = component.get("contribution").and_then(numeric_json_value);
                components.push(ExportMetricComponentRow {
                    metric_component_id: format!(
                        "{}.component.{}.{}",
                        run.run_id, index, component_name
                    ),
                    run_id: run.run_id.clone(),
                    algorithm_id: run.algorithm_id.clone(),
                    version: run.version.clone(),
                    metric_family: metric_family.clone(),
                    component_name: component_name.to_string(),
                    value,
                    unit,
                    score_0_to_100,
                    weight,
                    contribution,
                    contribution_json: json!({
                        "score_0_to_100": score_0_to_100,
                        "weight": weight,
                        "contribution": contribution,
                    }),
                    start_time: run.start_time.clone(),
                    end_time: run.end_time.clone(),
                    quality_flags: quality_flags.clone(),
                    provenance: json!({
                        "input_source": "algorithm_run.output_json.components",
                        "run_id": run.run_id,
                        "algorithm_id": run.algorithm_id,
                        "version": run.version,
                        "component_index": index,
                        "component_name": component_name,
                    }),
                });
            }
        }
    }
    Ok((values, components))
}

fn parse_quality_flags_for_metric_output(run: &AlgorithmRunRecord) -> GooseResult<Vec<String>> {
    let quality_flags: Vec<String> =
        serde_json::from_str(&run.quality_flags_json).map_err(|error| {
            GooseError::message(format!(
                "{} quality_flags_json invalid for metric output export: {error}",
                run.run_id
            ))
        })?;
    Ok(quality_flags)
}

fn numeric_json_value(value: &Value) -> Option<f64> {
    let value = value.as_f64()?;
    value.is_finite().then_some(value)
}

fn metric_family_for_algorithm(algorithm_id: &str) -> String {
    for family in ["recovery", "strain", "sleep", "hrv", "stress"] {
        if algorithm_id.contains(&format!(".{family}."))
            || algorithm_id.ends_with(&format!(".{family}"))
        {
            return family.to_string();
        }
    }
    "unknown".to_string()
}

fn unit_for_metric_value(name: &str) -> String {
    if name.ends_with("_0_to_100") {
        "score_0_to_100"
    } else if name.ends_with("_0_to_21") {
        "score_0_to_21"
    } else if name.ends_with("_ms") {
        "ms"
    } else if name.ends_with("_minutes") {
        "minutes"
    } else if name.ends_with("_bpm") {
        "bpm"
    } else if name.ends_with("_rpm") {
        "rpm"
    } else if name.ends_with("_c") {
        "celsius"
    } else if name.ends_with("_fraction") {
        "fraction"
    } else if name.ends_with("_count") || name == "interval_count" || name == "disturbance_count" {
        "count"
    } else if name.ends_with("_per_hour") {
        "per_hour"
    } else if name.contains("load") {
        "load"
    } else {
        "raw"
    }
    .to_string()
}

fn write_algorithm_runs_csv(path: &Path, rows: &[AlgorithmRunRecord]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "run_id,algorithm_id,version,start_time,end_time,output_json,quality_flags_json,provenance_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.run_id,
                &row.algorithm_id,
                &row.version,
                &row.start_time,
                &row.end_time,
                &row.output_json,
                &row.quality_flags_json,
                &row.provenance_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_calibration_labels_csv(
    path: &Path,
    rows: &[ExportCalibrationLabelRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "label_id,metric_family,label_source,captured_at,value,unit,provenance_json,official_labels_are_labels"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.label_id,
                &row.metric_family,
                &row.label_source,
                &row.captured_at,
                &row.value.to_string(),
                &row.unit,
                &row.provenance_json,
                &row.official_labels_are_labels.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn export_calibration_labels(rows: Vec<CalibrationLabelRow>) -> Vec<ExportCalibrationLabelRow> {
    rows.into_iter()
        .map(|row| ExportCalibrationLabelRow {
            label_id: row.label_id,
            metric_family: row.metric_family,
            label_source: row.label_source,
            captured_at: row.captured_at,
            value: row.value,
            unit: row.unit,
            provenance_json: row.provenance_json,
            official_labels_are_labels: true,
        })
        .collect()
}

fn export_activity_labels(
    store: &GooseStore,
    session_rows: &[ActivitySessionRow],
) -> GooseResult<Vec<ActivityLabelRow>> {
    let mut rows = Vec::new();
    for session in session_rows {
        rows.extend(store.activity_labels_for_session(&session.session_id)?);
    }
    Ok(rows)
}

fn export_metric_provenance_rows(
    store: &GooseStore,
    daily_activity_rows: &[DailyActivityMetricRow],
    hourly_activity_rows: &[HourlyActivityMetricRow],
    daily_recovery_rows: &[DailyRecoveryMetricRow],
) -> GooseResult<Vec<MetricProvenanceRow>> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for row in daily_activity_rows {
        append_metric_provenance_rows(
            store,
            "daily_activity",
            &row.daily_metric_id,
            &mut seen,
            &mut rows,
        )?;
    }
    for row in hourly_activity_rows {
        append_metric_provenance_rows(
            store,
            "hourly_activity",
            &row.hourly_metric_id,
            &mut seen,
            &mut rows,
        )?;
    }
    for row in daily_recovery_rows {
        append_metric_provenance_rows(
            store,
            "daily_recovery",
            &row.daily_metric_id,
            &mut seen,
            &mut rows,
        )?;
    }
    Ok(rows)
}

fn append_metric_provenance_rows(
    store: &GooseStore,
    metric_scope: &str,
    metric_id: &str,
    seen: &mut BTreeSet<String>,
    rows: &mut Vec<MetricProvenanceRow>,
) -> GooseResult<()> {
    for row in store.metric_provenance_for_metric(metric_scope, metric_id)? {
        if seen.insert(row.provenance_id.clone()) {
            rows.push(row);
        }
    }
    Ok(())
}

fn activity_session_rows_for_raw_byte_policy(
    rows: &[ActivitySessionRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ActivitySessionRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn activity_metric_rows_for_raw_byte_policy(
    rows: &[ActivityMetricRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ActivityMetricRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn activity_interval_rows_for_raw_byte_policy(
    rows: &[ActivityIntervalRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ActivityIntervalRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.metadata_json = raw_byte_policy_json_text(&row.metadata_json)?;
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn activity_label_rows_for_raw_byte_policy(
    rows: &[ActivityLabelRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<ActivityLabelRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn daily_activity_metric_rows_for_raw_byte_policy(
    rows: &[DailyActivityMetricRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<DailyActivityMetricRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.inputs_json = raw_byte_policy_json_text(&row.inputs_json)?;
            row.quality_flags_json = raw_byte_policy_json_text(&row.quality_flags_json)?;
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn hourly_activity_metric_rows_for_raw_byte_policy(
    rows: &[HourlyActivityMetricRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<HourlyActivityMetricRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.inputs_json = raw_byte_policy_json_text(&row.inputs_json)?;
            row.quality_flags_json = raw_byte_policy_json_text(&row.quality_flags_json)?;
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn daily_recovery_metric_rows_for_raw_byte_policy(
    rows: &[DailyRecoveryMetricRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<DailyRecoveryMetricRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.inputs_json = raw_byte_policy_json_text(&row.inputs_json)?;
            row.quality_flags_json = raw_byte_policy_json_text(&row.quality_flags_json)?;
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn metric_provenance_rows_for_raw_byte_policy(
    rows: &[MetricProvenanceRow],
    filters: &RawExportFilters,
) -> GooseResult<Vec<MetricProvenanceRow>> {
    if filters.include_raw_bytes {
        return Ok(rows.to_vec());
    }
    rows.iter()
        .cloned()
        .map(|mut row| {
            row.inputs_json = raw_byte_policy_json_text(&row.inputs_json)?;
            row.quality_flags_json = raw_byte_policy_json_text(&row.quality_flags_json)?;
            row.provenance_json = raw_byte_policy_json_text(&row.provenance_json)?;
            Ok(row)
        })
        .collect()
}

fn write_activity_sessions_csv(path: &Path, rows: &[ActivitySessionRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "session_id,source,start_time_unix_ms,end_time_unix_ms,duration_ms,activity_type,external_activity_type_code,external_activity_type_name,custom_label,confidence,detection_method,sync_status,provenance_json,created_at,updated_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.session_id,
                &row.source,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &row.duration_ms.to_string(),
                &row.activity_type,
                row.external_activity_type_code
                    .as_deref()
                    .unwrap_or_default(),
                row.external_activity_type_name
                    .as_deref()
                    .unwrap_or_default(),
                row.custom_label.as_deref().unwrap_or_default(),
                &row.confidence.to_string(),
                &row.detection_method,
                &row.sync_status,
                &row.provenance_json,
                &row.created_at,
                &row.updated_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_activity_metrics_csv(path: &Path, rows: &[ActivityMetricRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "metric_id,activity_session_id,metric_name,value,unit,start_time_unix_ms,end_time_unix_ms,quality_flags_json,provenance_json,created_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.metric_id,
                &row.activity_session_id,
                &row.metric_name,
                &row.value.to_string(),
                &row.unit,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &row.quality_flags_json,
                &row.provenance_json,
                &row.created_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_activity_intervals_csv(path: &Path, rows: &[ActivityIntervalRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "interval_id,activity_session_id,interval_type,start_time_unix_ms,end_time_unix_ms,duration_ms,sequence,metadata_json,provenance_json,created_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.interval_id,
                &row.activity_session_id,
                &row.interval_type,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &row.duration_ms.to_string(),
                &row.sequence.to_string(),
                &row.metadata_json,
                &row.provenance_json,
                &row.created_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_activity_labels_csv(path: &Path, rows: &[ActivityLabelRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "label_id,activity_session_id,label_type,value,source,confidence,provenance_json,created_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.label_id,
                &row.activity_session_id,
                &row.label_type,
                &row.value,
                &row.source,
                &row.confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                &row.provenance_json,
                &row.created_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_daily_activity_metrics_csv(
    path: &Path,
    rows: &[DailyActivityMetricRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "daily_metric_id,date_key,timezone,start_time_unix_ms,end_time_unix_ms,steps,active_kcal,resting_kcal,total_kcal,average_cadence_spm,source_kind,confidence,inputs_json,quality_flags_json,provenance_json,created_at,updated_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let steps = csv_optional_i64(row.steps);
        let active_kcal = csv_optional_f64(row.active_kcal);
        let resting_kcal = csv_optional_f64(row.resting_kcal);
        let total_kcal = csv_optional_f64(row.total_kcal);
        let average_cadence_spm = csv_optional_f64(row.average_cadence_spm);
        write_csv_row(
            &mut bytes,
            &[
                &row.daily_metric_id,
                &row.date_key,
                &row.timezone,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &steps,
                &active_kcal,
                &resting_kcal,
                &total_kcal,
                &average_cadence_spm,
                &row.source_kind,
                &row.confidence.to_string(),
                &row.inputs_json,
                &row.quality_flags_json,
                &row.provenance_json,
                &row.created_at,
                &row.updated_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_hourly_activity_metrics_csv(
    path: &Path,
    rows: &[HourlyActivityMetricRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "hourly_metric_id,date_key,timezone,start_time_unix_ms,end_time_unix_ms,steps,active_kcal,resting_kcal,total_kcal,average_cadence_spm,source_kind,confidence,inputs_json,quality_flags_json,provenance_json,created_at,updated_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let steps = csv_optional_i64(row.steps);
        let active_kcal = csv_optional_f64(row.active_kcal);
        let resting_kcal = csv_optional_f64(row.resting_kcal);
        let total_kcal = csv_optional_f64(row.total_kcal);
        let average_cadence_spm = csv_optional_f64(row.average_cadence_spm);
        write_csv_row(
            &mut bytes,
            &[
                &row.hourly_metric_id,
                &row.date_key,
                &row.timezone,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &steps,
                &active_kcal,
                &resting_kcal,
                &total_kcal,
                &average_cadence_spm,
                &row.source_kind,
                &row.confidence.to_string(),
                &row.inputs_json,
                &row.quality_flags_json,
                &row.provenance_json,
                &row.created_at,
                &row.updated_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_daily_recovery_metrics_csv(
    path: &Path,
    rows: &[DailyRecoveryMetricRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "daily_metric_id,date_key,timezone,start_time_unix_ms,end_time_unix_ms,resting_hr_bpm,hrv_rmssd_ms,respiratory_rate_rpm,oxygen_saturation_percent,skin_temperature_delta_c,source_kind,confidence,inputs_json,quality_flags_json,provenance_json,created_at,updated_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let resting_hr_bpm = csv_optional_f64(row.resting_hr_bpm);
        let hrv_rmssd_ms = csv_optional_f64(row.hrv_rmssd_ms);
        let respiratory_rate_rpm = csv_optional_f64(row.respiratory_rate_rpm);
        let oxygen_saturation_percent = csv_optional_f64(row.oxygen_saturation_percent);
        let skin_temperature_delta_c = csv_optional_f64(row.skin_temperature_delta_c);
        write_csv_row(
            &mut bytes,
            &[
                &row.daily_metric_id,
                &row.date_key,
                &row.timezone,
                &row.start_time_unix_ms.to_string(),
                &row.end_time_unix_ms.to_string(),
                &resting_hr_bpm,
                &hrv_rmssd_ms,
                &respiratory_rate_rpm,
                &oxygen_saturation_percent,
                &skin_temperature_delta_c,
                &row.source_kind,
                &row.confidence.to_string(),
                &row.inputs_json,
                &row.quality_flags_json,
                &row.provenance_json,
                &row.created_at,
                &row.updated_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_metric_provenance_csv(path: &Path, rows: &[MetricProvenanceRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "provenance_id,metric_scope,metric_id,source_kind,source_detail,confidence,inputs_json,quality_flags_json,provenance_json,created_at"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let confidence = csv_optional_f64(row.confidence);
        write_csv_row(
            &mut bytes,
            &[
                &row.provenance_id,
                &row.metric_scope,
                &row.metric_id,
                &row.source_kind,
                &row.source_detail,
                &confidence,
                &row.inputs_json,
                &row.quality_flags_json,
                &row.provenance_json,
                &row.created_at,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_calibration_runs_csv(path: &Path, rows: &[CalibrationRunRecord]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "calibration_run_id,algorithm_id,version,train_start,train_end,holdout_start,holdout_end,metrics_json,params_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.calibration_run_id,
                &row.algorithm_id,
                &row.version,
                &row.times.train_start,
                &row.times.train_end,
                &row.times.holdout_start,
                &row.times.holdout_end,
                &row.metrics_json,
                &row.params_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_debug_sessions_csv(path: &Path, rows: &[ExportDebugSessionRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "session_id,started_at_unix_ms,bridge_url,bind_host,token_required,token_present,remote_bind_enabled,visible_remote_bind_toggle"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.session_id,
                &row.started_at_unix_ms.to_string(),
                &row.bridge_url,
                &row.bind_host,
                &row.token_required.to_string(),
                &row.token_present.to_string(),
                &row.remote_bind_enabled.to_string(),
                &row.visible_remote_bind_toggle.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_debug_commands_csv(path: &Path, rows: &[ExportDebugCommandRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "command_id,session_id,schema,command,args_json,dry_run,received_at_unix_ms"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.command_id,
                &row.session_id,
                &row.schema,
                &row.command,
                &row.args_json,
                &row.dry_run.to_string(),
                &row.received_at_unix_ms.to_string(),
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_debug_events_csv(path: &Path, rows: &[ExportDebugEventRow]) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "session_id,sequence,schema,time_unix_ms,source,level,topic,message,command_id,data_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        write_csv_row(
            &mut bytes,
            &[
                &row.session_id,
                &row.sequence.to_string(),
                &row.schema,
                &row.time_unix_ms.to_string(),
                &row.source,
                &row.level,
                &row.topic,
                &row.message,
                row.command_id.as_deref().unwrap_or_default(),
                &row.data_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn write_command_validation_csv(
    path: &Path,
    rows: &[ExportCommandValidationRow],
) -> GooseResult<Vec<u8>> {
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "command,command_number,family,risk_gate,direct_send_ready,missing_requirements_json,warnings_json,next_capture_actions_json,validated_service_uuid,validated_characteristic_uuid,validated_write_type,validated_evidence_source,validated_capture_kind,validated_owner,validated_provenance_json,validated_triggering_ui_action,report_json"
    )
    .map_err(|error| GooseError::message(format!("cannot write CSV header: {error}")))?;
    for row in rows {
        let command_number = row
            .command_number
            .map(|command_number| command_number.to_string())
            .unwrap_or_default();
        let direct_send_ready = row.direct_send_ready.to_string();
        let missing_requirements = row.missing_requirements.to_string();
        let warnings = row.warnings.to_string();
        let next_capture_actions = row.next_capture_actions.to_string();
        let report_json = row.report_json.to_string();
        write_csv_row(
            &mut bytes,
            &[
                &row.command,
                &command_number,
                &row.family,
                &row.risk_gate,
                &direct_send_ready,
                &missing_requirements,
                &warnings,
                &next_capture_actions,
                row.validated_service_uuid.as_deref().unwrap_or_default(),
                row.validated_characteristic_uuid
                    .as_deref()
                    .unwrap_or_default(),
                row.validated_write_type.as_deref().unwrap_or_default(),
                row.validated_evidence_source.as_deref().unwrap_or_default(),
                row.validated_capture_kind.as_deref().unwrap_or_default(),
                row.validated_owner.as_deref().unwrap_or_default(),
                row.validated_provenance_json.as_deref().unwrap_or_default(),
                row.validated_triggering_ui_action
                    .as_deref()
                    .unwrap_or_default(),
                &report_json,
            ],
        )?;
    }
    fs::write(path, &bytes).map_err(|source| GooseError::io(path, source))?;
    Ok(bytes)
}

fn csv_optional_i64(value: Option<i64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn csv_optional_f64(value: Option<f64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn write_csv_row(output: &mut Vec<u8>, fields: &[&str]) -> GooseResult<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            output.push(b',');
        }
        write!(output, "{}", csv_escape(field))
            .map_err(|error| GooseError::message(format!("cannot write CSV field: {error}")))?;
    }
    output.push(b'\n');
    Ok(())
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn export_debug_sessions(rows: Vec<DebugSessionRow>) -> Vec<ExportDebugSessionRow> {
    rows.into_iter()
        .map(|row| ExportDebugSessionRow {
            session_id: row.session_id,
            started_at_unix_ms: row.started_at_unix_ms,
            bridge_url: redact_debug_text(&row.bridge_url),
            bind_host: row.bind_host,
            token_required: row.token_required,
            token_present: row.token_present,
            remote_bind_enabled: row.remote_bind_enabled,
            visible_remote_bind_toggle: row.visible_remote_bind_toggle,
        })
        .collect()
}

fn export_debug_commands(rows: Vec<DebugCommandRow>) -> Vec<ExportDebugCommandRow> {
    rows.into_iter()
        .map(|row| ExportDebugCommandRow {
            command_id: row.command_id,
            session_id: row.session_id,
            schema: row.schema,
            command: row.command,
            args_json: redact_debug_json_text_if_valid(&row.args_json),
            dry_run: row.dry_run,
            received_at_unix_ms: row.received_at_unix_ms,
        })
        .collect()
}

fn export_debug_events(rows: Vec<DebugEventRow>) -> Vec<ExportDebugEventRow> {
    rows.into_iter()
        .map(|row| ExportDebugEventRow {
            session_id: row.session_id,
            sequence: row.sequence,
            schema: row.schema,
            time_unix_ms: row.time_unix_ms,
            source: row.source,
            level: row.level,
            topic: row.topic,
            message: redact_debug_text(&row.message),
            command_id: row.command_id,
            data_json: redact_debug_json_text_if_valid(&row.data_json),
        })
        .collect()
}

fn export_command_validation_records(
    rows: Vec<CommandValidationRecord>,
) -> GooseResult<Vec<ExportCommandValidationRow>> {
    rows.into_iter()
        .map(|row| {
            let report_json: Value = serde_json::from_str(&row.report_json).map_err(|error| {
                GooseError::message(format!(
                    "command validation {} report_json invalid: {error}",
                    row.command
                ))
            })?;
            if !report_json.is_object() {
                return Err(GooseError::message(format!(
                    "command validation {} report_json must be an object",
                    row.command
                )));
            }
            let command_number = report_json
                .get("command_number")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok());
            let family = report_json
                .get("family")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(ExportCommandValidationRow {
                command: row.command,
                command_number,
                family,
                risk_gate: row.risk_gate,
                direct_send_ready: row.direct_send_ready,
                missing_requirements: report_json
                    .get("missing_requirements")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                warnings: report_json
                    .get("warnings")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                next_capture_actions: report_json
                    .get("next_capture_actions")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                validated_service_uuid: report_json
                    .get("validated_service_uuid")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_characteristic_uuid: report_json
                    .get("validated_characteristic_uuid")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_write_type: report_json
                    .get("validated_write_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_evidence_source: report_json
                    .get("validated_evidence_source")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_capture_kind: report_json
                    .get("validated_capture_kind")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_owner: report_json
                    .get("validated_owner")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_provenance_json: report_json
                    .get("validated_provenance_json")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                validated_triggering_ui_action: report_json
                    .get("validated_triggering_ui_action")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                report_json,
            })
        })
        .collect()
}

fn redact_debug_text(value: &str) -> String {
    redact_query_value(value, "token")
}

fn redact_debug_json_text_if_valid(value: &str) -> String {
    match serde_json::from_str::<Value>(value) {
        Ok(mut json) => {
            redact_debug_json_value(&mut json);
            serde_json::to_string(&json).unwrap_or_else(|_| redact_debug_text(value))
        }
        Err(_) => redact_debug_text(value),
    }
}

fn redact_debug_json_value(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = redact_debug_text(text);
        }
        Value::Array(values) => {
            for value in values {
                redact_debug_json_value(value);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_debug_json_value(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_query_value(value: &str, key: &str) -> String {
    let needle = format!("{key}=");
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        let Some(index) = rest.find(&needle) else {
            output.push_str(rest);
            break;
        };
        let (before, after_before) = rest.split_at(index);
        output.push_str(before);
        output.push_str(&needle);
        output.push_str("<redacted>");
        let value_start = needle.len();
        let after_value_start = &after_before[value_start..];
        let value_end = after_value_start
            .find('&')
            .unwrap_or(after_value_start.len());
        rest = &after_value_start[value_end..];
    }
    output
}

fn parse_debug_time_window(start: &str, end: &str) -> Result<(i64, i64), String> {
    parse_labeled_time_window("debug", start, end)
}

fn parse_activity_time_window(start: &str, end: &str) -> Result<(i64, i64), String> {
    parse_labeled_time_window("activity", start, end)
}

fn parse_labeled_time_window(label: &str, start: &str, end: &str) -> Result<(i64, i64), String> {
    let start_unix_ms = parse_export_bound_unix_ms(label, start, true)?;
    let end_unix_ms = parse_export_bound_unix_ms(label, end, false)?;
    if start_unix_ms >= end_unix_ms {
        return Err(format!(
            "{label} time window start must be earlier than end"
        ));
    }
    Ok((start_unix_ms, end_unix_ms))
}

fn parse_export_bound_unix_ms(label: &str, value: &str, is_start: bool) -> Result<i64, String> {
    let value = value.trim();
    if is_start && value == "0000" {
        return Ok(0);
    }
    if !is_start && value == "9999" {
        return Ok(i64::MAX);
    }
    parse_rfc3339_utc_unix_ms(value)
        .ok_or_else(|| format!("{label} time window bound is not supported: {value}"))
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
    let (second_text, fraction_text) = seconds_part
        .split_once('.')
        .map_or((seconds_part, ""), |(seconds, fraction)| {
            (seconds, fraction)
        });
    let second = second_text.parse::<u32>().ok()?;
    let millis = parse_millis_fraction(fraction_text)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    days.checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)?
        .checked_add(i64::from(minute) * 60_000)?
        .checked_add(i64::from(second) * 1_000)?
        .checked_add(i64::from(millis))
}

fn parse_millis_fraction(value: &str) -> Option<u32> {
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

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

fn unix_ms_to_rfc3339_utc(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1_000);
    let millis = unix_ms.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    if millis == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
    }
}

fn manifest_file(
    path: &str,
    bytes: &[u8],
    row_count: Option<u64>,
    kind: &str,
) -> ExportFileManifest {
    ExportFileManifest {
        path: path.to_string(),
        sha256: sha256_hex(bytes),
        row_count,
        kind: Some(kind.to_string()),
    }
}

fn validate_zipped_export_bundle(path: &Path) -> GooseResult<ExportValidationReport> {
    let zip_file = File::open(path).map_err(|source| GooseError::io(path, source))?;
    let mut archive = ZipArchive::new(zip_file)
        .map_err(|error| GooseError::message(format!("cannot open zip bundle: {error}")))?;
    let manifest_raw = read_zip_entry_to_string(&mut archive, "manifest.json")?;
    let manifest: ExportManifest =
        serde_json::from_str(&manifest_raw).map_err(|source| GooseError::json(path, source))?;

    let mut manifest_issues = Vec::new();
    validate_manifest_shape(&manifest, &mut manifest_issues);
    let manifest_valid = manifest_issues.is_empty();
    let mut issues = manifest_issues;

    let file_results = manifest
        .files
        .iter()
        .map(|file| validate_zip_manifest_file(&mut archive, file))
        .collect::<Vec<_>>();

    for result in &file_results {
        if !result.pass {
            issues.push(format!("{} failed file validation", result.path));
        }
    }
    let content = validate_export_contents(&manifest, |relative_path| {
        read_zip_entry_to_string(&mut archive, relative_path)
    });
    issues.extend(content.issues.iter().cloned());

    Ok(report(path, manifest_valid, file_results, content, issues))
}

fn validate_export_contents(
    manifest: &ExportManifest,
    mut read_file: impl FnMut(&str) -> GooseResult<String>,
) -> ExportContentValidation {
    let mut issues = Vec::new();
    let mut csv_valid = true;
    let mut csv_row_count_checks = 0_usize;
    for file in &manifest.files {
        match file.kind.as_deref() {
            Some("jsonl") => match read_file(&file.path) {
                Ok(text) => validate_jsonl_file(&file.path, &text, file.row_count, &mut issues),
                Err(error) => issues.push(format!("{} cannot be inspected: {error}", file.path)),
            },
            Some("csv") => {
                csv_row_count_checks += 1;
                match read_file(&file.path) {
                    Ok(text) => {
                        if !validate_csv_file(&file.path, &text, file.row_count, &mut issues) {
                            csv_valid = false;
                        }
                    }
                    Err(error) => {
                        csv_valid = false;
                        issues.push(format!("{} cannot be inspected: {error}", file.path));
                    }
                }
            }
            _ => {}
        }
    }

    let mut raw_evidence_rows = read_typed_export_rows::<RawEvidenceRow>(
        &mut read_file,
        manifest,
        "raw_evidence",
        "data/raw_evidence.jsonl",
        &mut issues,
    );
    let mut decoded_frame_rows = read_typed_export_rows::<DecodedFrameRow>(
        &mut read_file,
        manifest,
        "decoded_frames",
        "data/decoded_frames.jsonl",
        &mut issues,
    );
    let mut packet_timeline_rows = read_typed_export_rows::<PacketTimelineRow>(
        &mut read_file,
        manifest,
        "packet_timeline",
        "data/packet_timeline.jsonl",
        &mut issues,
    );
    let sensor_sample_rows = read_typed_export_rows::<ExportSensorSampleRow>(
        &mut read_file,
        manifest,
        "sensor_samples",
        "data/sensor_samples.jsonl",
        &mut issues,
    );
    let metric_feature_report_rows = read_typed_export_rows::<ExportMetricFeatureReportRow>(
        &mut read_file,
        manifest,
        "metric_features",
        "data/metric_features.jsonl",
        &mut issues,
    );
    let metric_value_rows = read_typed_export_rows::<ExportMetricValueRow>(
        &mut read_file,
        manifest,
        "metric_outputs",
        "data/metric_values.jsonl",
        &mut issues,
    );
    let metric_component_rows = read_typed_export_rows::<ExportMetricComponentRow>(
        &mut read_file,
        manifest,
        "metric_outputs",
        "data/metric_components.jsonl",
        &mut issues,
    );
    let algorithm_run_rows = read_typed_export_rows::<AlgorithmRunRecord>(
        &mut read_file,
        manifest,
        "algorithm_runs",
        "data/algorithm_runs.jsonl",
        &mut issues,
    );
    let calibration_label_rows = read_typed_export_rows::<ExportCalibrationLabelRow>(
        &mut read_file,
        manifest,
        "calibration_labels",
        "data/calibration_labels.jsonl",
        &mut issues,
    );
    let calibration_run_rows = read_typed_export_rows::<CalibrationRunRecord>(
        &mut read_file,
        manifest,
        "calibration_runs",
        "data/calibration_runs.jsonl",
        &mut issues,
    );
    let activity_session_rows = read_typed_export_rows::<ActivitySessionRow>(
        &mut read_file,
        manifest,
        "activity_sessions",
        "data/activity_sessions.jsonl",
        &mut issues,
    );
    let activity_metric_rows = read_typed_export_rows::<ActivityMetricRow>(
        &mut read_file,
        manifest,
        "activity_metrics",
        "data/activity_metrics.jsonl",
        &mut issues,
    );
    let activity_interval_rows = read_typed_export_rows::<ActivityIntervalRow>(
        &mut read_file,
        manifest,
        "activity_intervals",
        "data/activity_intervals.jsonl",
        &mut issues,
    );
    let activity_label_rows = read_typed_export_rows::<ActivityLabelRow>(
        &mut read_file,
        manifest,
        "activity_labels",
        "data/activity_labels.jsonl",
        &mut issues,
    );
    let daily_activity_metric_rows = read_typed_export_rows::<DailyActivityMetricRow>(
        &mut read_file,
        manifest,
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
        "data/local_health_daily_activity_metrics.jsonl",
        &mut issues,
    );
    let hourly_activity_metric_rows = read_typed_export_rows::<HourlyActivityMetricRow>(
        &mut read_file,
        manifest,
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
        "data/local_health_hourly_activity_metrics.jsonl",
        &mut issues,
    );
    let daily_recovery_metric_rows = read_typed_export_rows::<DailyRecoveryMetricRow>(
        &mut read_file,
        manifest,
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
        "data/local_health_daily_recovery_metrics.jsonl",
        &mut issues,
    );
    let metric_provenance_rows = read_typed_export_rows::<MetricProvenanceRow>(
        &mut read_file,
        manifest,
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY,
        "data/local_health_metric_provenance.jsonl",
        &mut issues,
    );
    let debug_session_rows = read_typed_export_rows::<ExportDebugSessionRow>(
        &mut read_file,
        manifest,
        "debug_sessions",
        "data/debug_sessions.jsonl",
        &mut issues,
    );
    let debug_command_rows = read_typed_export_rows::<ExportDebugCommandRow>(
        &mut read_file,
        manifest,
        "debug_commands",
        "data/debug_commands.jsonl",
        &mut issues,
    );
    let debug_event_rows = read_typed_export_rows::<ExportDebugEventRow>(
        &mut read_file,
        manifest,
        "debug_events",
        "data/debug_events.jsonl",
        &mut issues,
    );
    let command_validation_rows = read_typed_export_rows::<ExportCommandValidationRow>(
        &mut read_file,
        manifest,
        "command_validation",
        "data/command_validation.jsonl",
        &mut issues,
    );

    let raw_evidence_ids = validate_raw_evidence_reimport(
        raw_evidence_rows.as_deref().unwrap_or_default(),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    let decoded_frame_ids = validate_decoded_frame_reimport(
        decoded_frame_rows.as_deref().unwrap_or_default(),
        raw_evidence_rows.as_ref().map(|_| &raw_evidence_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_packet_timeline_reimport(
        packet_timeline_rows.as_deref().unwrap_or_default(),
        raw_evidence_rows.as_ref().map(|_| &raw_evidence_ids),
        decoded_frame_rows.as_ref().map(|_| &decoded_frame_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_sensor_sample_reimport(
        sensor_sample_rows.as_deref().unwrap_or_default(),
        raw_evidence_rows.as_ref().map(|_| &raw_evidence_ids),
        decoded_frame_rows.as_ref().map(|_| &decoded_frame_ids),
        &mut issues,
    );
    validate_metric_feature_report_reimport(
        metric_feature_report_rows.as_deref().unwrap_or_default(),
        &mut issues,
    );
    let algorithm_run_ids = validate_algorithm_run_reimport(
        algorithm_run_rows.as_deref().unwrap_or_default(),
        &mut issues,
    );

    let raw_evidence_ids_from_fields = read_jsonl_string_field_set(
        &mut read_file,
        "data/raw_evidence.jsonl",
        "evidence_id",
        &mut issues,
    );
    let decoded_evidence_ids_from_fields = read_jsonl_string_field_set(
        &mut read_file,
        "data/decoded_frames.jsonl",
        "evidence_id",
        &mut issues,
    );
    let timeline_evidence_ids_from_fields = read_jsonl_string_field_set(
        &mut read_file,
        "data/packet_timeline.jsonl",
        "evidence_id",
        &mut issues,
    );

    if let (Some(raw_evidence_ids), Some(decoded_evidence_ids)) = (
        &raw_evidence_ids_from_fields,
        &decoded_evidence_ids_from_fields,
    ) {
        for evidence_id in decoded_evidence_ids.difference(raw_evidence_ids) {
            issues.push(format!(
                "decoded frame evidence_id {evidence_id} is missing from raw evidence export"
            ));
        }
    }
    if let (Some(raw_evidence_ids), Some(timeline_evidence_ids)) = (
        &raw_evidence_ids_from_fields,
        &timeline_evidence_ids_from_fields,
    ) {
        for evidence_id in timeline_evidence_ids.difference(raw_evidence_ids) {
            issues.push(format!(
                "timeline evidence_id {evidence_id} is missing from raw evidence export"
            ));
        }
    }

    let decoded_frame_ids_from_fields = read_jsonl_string_field_set(
        &mut read_file,
        "data/decoded_frames.jsonl",
        "frame_id",
        &mut issues,
    );
    let timeline_frame_ids_from_fields = read_jsonl_string_field_set(
        &mut read_file,
        "data/packet_timeline.jsonl",
        "frame_id",
        &mut issues,
    );
    if let (Some(decoded_frame_ids), Some(timeline_frame_ids)) = (
        &decoded_frame_ids_from_fields,
        &timeline_frame_ids_from_fields,
    ) {
        for frame_id in timeline_frame_ids.difference(decoded_frame_ids) {
            issues.push(format!(
                "timeline frame_id {frame_id} is missing from decoded frame export"
            ));
        }
    }
    validate_metric_output_reimport(
        metric_value_rows.as_deref().unwrap_or_default(),
        metric_component_rows.as_deref().unwrap_or_default(),
        algorithm_run_rows.as_ref().map(|_| &algorithm_run_ids),
        &mut issues,
    );
    validate_calibration_label_reimport(
        calibration_label_rows.as_deref().unwrap_or_default(),
        &mut issues,
    );
    validate_calibration_run_reimport(
        calibration_run_rows.as_deref().unwrap_or_default(),
        &mut issues,
    );
    let activity_session_ids = validate_activity_session_reimport(
        activity_session_rows.as_deref().unwrap_or_default(),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    let _activity_metric_ids = validate_activity_metric_reimport(
        activity_metric_rows.as_deref().unwrap_or_default(),
        activity_session_rows
            .as_ref()
            .map(|_| &activity_session_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    let _activity_interval_ids = validate_activity_interval_reimport(
        activity_interval_rows.as_deref().unwrap_or_default(),
        activity_session_rows
            .as_ref()
            .map(|_| &activity_session_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_activity_label_reimport(
        activity_label_rows.as_deref().unwrap_or_default(),
        activity_session_rows
            .as_ref()
            .map(|_| &activity_session_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    let local_health_metric_refs = validate_local_health_metric_reimport(
        daily_activity_metric_rows.as_deref().unwrap_or_default(),
        hourly_activity_metric_rows.as_deref().unwrap_or_default(),
        daily_recovery_metric_rows.as_deref().unwrap_or_default(),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_metric_provenance_reimport(
        metric_provenance_rows.as_deref().unwrap_or_default(),
        &local_health_metric_refs,
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    let debug_session_ids = validate_debug_session_reimport(
        debug_session_rows.as_deref().unwrap_or_default(),
        &mut issues,
    );
    let debug_command_ids = validate_debug_command_reimport(
        debug_command_rows.as_deref().unwrap_or_default(),
        debug_session_rows.as_ref().map(|_| &debug_session_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_debug_event_reimport(
        debug_event_rows.as_deref().unwrap_or_default(),
        debug_session_rows.as_ref().map(|_| &debug_session_ids),
        debug_command_rows.as_ref().map(|_| &debug_command_ids),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );
    validate_command_validation_reimport(
        command_validation_rows.as_deref().unwrap_or_default(),
        manifest.filters.include_raw_bytes,
        &mut issues,
    );

    let next_actions = export_validation_issue_actions("content", &issues);
    ExportContentValidation {
        pass: issues.is_empty(),
        csv_valid,
        csv_row_count_checks,
        raw_evidence_rows: raw_evidence_rows.take().map_or(0, |rows| rows.len()),
        decoded_frame_rows: decoded_frame_rows.take().map_or(0, |rows| rows.len()),
        packet_timeline_rows: packet_timeline_rows.take().map_or(0, |rows| rows.len()),
        sensor_sample_rows: sensor_sample_rows.map_or(0, |rows| rows.len()),
        metric_feature_report_rows: metric_feature_report_rows.map_or(0, |rows| rows.len()),
        metric_value_rows: metric_value_rows.map_or(0, |rows| rows.len()),
        metric_component_rows: metric_component_rows.map_or(0, |rows| rows.len()),
        algorithm_run_rows: algorithm_run_rows.map_or(0, |rows| rows.len()),
        calibration_label_rows: calibration_label_rows.map_or(0, |rows| rows.len()),
        calibration_run_rows: calibration_run_rows.map_or(0, |rows| rows.len()),
        activity_session_rows: activity_session_rows.map_or(0, |rows| rows.len()),
        activity_metric_rows: activity_metric_rows.map_or(0, |rows| rows.len()),
        activity_interval_rows: activity_interval_rows.map_or(0, |rows| rows.len()),
        activity_label_rows: activity_label_rows.map_or(0, |rows| rows.len()),
        daily_activity_metric_rows: daily_activity_metric_rows.map_or(0, |rows| rows.len()),
        hourly_activity_metric_rows: hourly_activity_metric_rows.map_or(0, |rows| rows.len()),
        daily_recovery_metric_rows: daily_recovery_metric_rows.map_or(0, |rows| rows.len()),
        metric_provenance_rows: metric_provenance_rows.map_or(0, |rows| rows.len()),
        command_validation_rows: command_validation_rows.map_or(0, |rows| rows.len()),
        debug_session_rows: debug_session_rows.map_or(0, |rows| rows.len()),
        debug_command_rows: debug_command_rows.map_or(0, |rows| rows.len()),
        debug_event_rows: debug_event_rows.map_or(0, |rows| rows.len()),
        reimported_evidence_ids: raw_evidence_ids.len(),
        reimported_frame_ids: decoded_frame_ids.len(),
        issues,
        next_actions,
    }
}

fn read_typed_export_rows<T: DeserializeOwned>(
    read_file: &mut impl FnMut(&str) -> GooseResult<String>,
    manifest: &ExportManifest,
    family: &str,
    path: &str,
    issues: &mut Vec<String>,
) -> Option<Vec<T>> {
    if !family_is_listed(manifest, family) {
        return None;
    }
    if !manifest_lists_path(manifest, path) {
        issues.push(format!("data family {family} requires {path}"));
        return None;
    }

    let text = match read_file(path) {
        Ok(text) => text,
        Err(error) => {
            issues.push(format!("{path} cannot be re-imported: {error}"));
            return None;
        }
    };
    Some(parse_typed_jsonl(path, &text, issues))
}

fn parse_typed_jsonl<T: DeserializeOwned>(
    path: &str,
    text: &str,
    issues: &mut Vec<String>,
) -> Vec<T> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(row) => rows.push(row),
            Err(error) => issues.push(format!(
                "{path} line {} cannot be re-imported as Goose data: {error}",
                index + 1
            )),
        }
    }
    rows
}

fn validate_raw_evidence_reimport(
    rows: &[RawEvidenceRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut evidence_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.evidence_id, "raw_evidence.evidence_id", issues);
        validate_non_empty(&row.source, "raw_evidence.source", issues);
        validate_non_empty(&row.captured_at, "raw_evidence.captured_at", issues);
        validate_non_empty(&row.device_model, "raw_evidence.device_model", issues);
        validate_non_empty(&row.sensitivity, "raw_evidence.sensitivity", issues);
        if !evidence_ids.insert(row.evidence_id.clone()) {
            issues.push(format!(
                "raw evidence evidence_id {} cannot be re-imported twice",
                row.evidence_id
            ));
        }
        validate_sha256_hex_field(
            &row.sha256,
            &format!("raw evidence {} sha256", row.evidence_id),
            issues,
        );
        if include_raw_bytes {
            match hex::decode(&row.payload_hex) {
                Ok(bytes) => {
                    let actual_sha256 = sha256_hex(&bytes);
                    if row.sha256 != actual_sha256 {
                        issues.push(format!(
                            "raw evidence {} sha256 does not match payload_hex",
                            row.evidence_id
                        ));
                    }
                }
                Err(error) => issues.push(format!(
                    "raw evidence {} payload_hex is not valid hex: {error}",
                    row.evidence_id
                )),
            }
        } else if !row.payload_hex.is_empty() {
            issues.push(format!(
                "raw evidence {} payload_hex must be empty when include_raw_bytes is false",
                row.evidence_id
            ));
        }
    }
    evidence_ids
}

fn validate_decoded_frame_reimport(
    rows: &[DecodedFrameRow],
    raw_evidence_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut frame_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.frame_id, "decoded_frames.frame_id", issues);
        validate_non_empty(&row.evidence_id, "decoded_frames.evidence_id", issues);
        validate_non_empty(&row.captured_at, "decoded_frames.captured_at", issues);
        validate_non_empty(&row.device_type, "decoded_frames.device_type", issues);
        validate_non_empty(&row.parser_version, "decoded_frames.parser_version", issues);
        if !frame_ids.insert(row.frame_id.clone()) {
            issues.push(format!(
                "decoded frame frame_id {} cannot be re-imported twice",
                row.frame_id
            ));
        }
        if let Some(raw_evidence_ids) = raw_evidence_ids
            && !raw_evidence_ids.contains(&row.evidence_id)
        {
            issues.push(format!(
                "decoded frame evidence_id {} is missing from typed raw evidence re-import",
                row.evidence_id
            ));
        }
        if include_raw_bytes {
            validate_hex_field(
                &row.payload_hex,
                &format!("decoded frame {} payload_hex", row.frame_id),
                issues,
            );
        } else if !row.payload_hex.is_empty() {
            issues.push(format!(
                "decoded frame {} payload_hex must be empty when include_raw_bytes is false",
                row.frame_id
            ));
        }
        if !row.payload_crc_hex.is_empty() {
            validate_hex_field(
                &row.payload_crc_hex,
                &format!("decoded frame {} payload_crc_hex", row.frame_id),
                issues,
            );
        }
        validate_json_text(
            &row.parsed_payload_json,
            &format!("decoded frame {} parsed_payload_json", row.frame_id),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.parsed_payload_json,
            include_raw_bytes,
            &format!("decoded frame {} parsed_payload_json", row.frame_id),
            issues,
        );
        validate_json_text(
            &row.warnings_json,
            &format!("decoded frame {} warnings_json", row.frame_id),
            issues,
        );
    }
    frame_ids
}

fn validate_packet_timeline_reimport(
    rows: &[PacketTimelineRow],
    raw_evidence_ids: Option<&BTreeSet<String>>,
    decoded_frame_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) {
    let mut timeline_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.timeline_id, "packet_timeline.timeline_id", issues);
        validate_non_empty(&row.frame_id, "packet_timeline.frame_id", issues);
        validate_non_empty(&row.evidence_id, "packet_timeline.evidence_id", issues);
        validate_non_empty(&row.captured_at, "packet_timeline.captured_at", issues);
        validate_non_empty(&row.category, "packet_timeline.category", issues);
        validate_non_empty(&row.title, "packet_timeline.title", issues);
        if !timeline_ids.insert(row.timeline_id.clone()) {
            issues.push(format!(
                "packet timeline timeline_id {} cannot be re-imported twice",
                row.timeline_id
            ));
        }
        if let Some(raw_evidence_ids) = raw_evidence_ids
            && !raw_evidence_ids.contains(&row.evidence_id)
        {
            issues.push(format!(
                "timeline evidence_id {} is missing from typed raw evidence re-import",
                row.evidence_id
            ));
        }
        if let Some(decoded_frame_ids) = decoded_frame_ids
            && !decoded_frame_ids.contains(&row.frame_id)
        {
            issues.push(format!(
                "timeline frame_id {} is missing from typed decoded frame re-import",
                row.frame_id
            ));
        }
        if let Some(body_hex) = &row.body_hex {
            if !include_raw_bytes && !body_hex.is_empty() {
                issues.push(format!(
                    "packet timeline {} body_hex must be empty when include_raw_bytes is false",
                    row.timeline_id
                ));
            }
            validate_hex_field(
                body_hex,
                &format!("packet timeline {} body_hex", row.timeline_id),
                issues,
            );
        }
        validate_raw_byte_json_value_policy(
            &row.summary,
            include_raw_bytes,
            &format!("packet timeline {} summary", row.timeline_id),
            issues,
        );
    }
}

fn validate_sensor_sample_reimport(
    rows: &[ExportSensorSampleRow],
    raw_evidence_ids: Option<&BTreeSet<String>>,
    decoded_frame_ids: Option<&BTreeSet<String>>,
    issues: &mut Vec<String>,
) {
    let mut sample_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.sample_id, "sensor_samples.sample_id", issues);
        validate_non_empty(&row.frame_id, "sensor_samples.frame_id", issues);
        validate_non_empty(&row.evidence_id, "sensor_samples.evidence_id", issues);
        validate_non_empty(&row.captured_at, "sensor_samples.captured_at", issues);
        validate_non_empty(&row.sample_time, "sensor_samples.sample_time", issues);
        validate_non_empty(
            &row.sample_time_source,
            "sensor_samples.sample_time_source",
            issues,
        );
        validate_non_empty(&row.source_signal, "sensor_samples.source_signal", issues);
        validate_non_empty(&row.series_name, "sensor_samples.series_name", issues);
        validate_non_empty(&row.unit, "sensor_samples.unit", issues);
        validate_non_empty(&row.parser_version, "sensor_samples.parser_version", issues);
        if !sample_ids.insert(row.sample_id.clone()) {
            issues.push(format!(
                "sensor sample sample_id {} cannot be re-imported twice",
                row.sample_id
            ));
        }
        if let Some(raw_evidence_ids) = raw_evidence_ids
            && !raw_evidence_ids.contains(&row.evidence_id)
        {
            issues.push(format!(
                "sensor sample evidence_id {} is missing from typed raw evidence re-import",
                row.evidence_id
            ));
        }
        if let Some(decoded_frame_ids) = decoded_frame_ids
            && !decoded_frame_ids.contains(&row.frame_id)
        {
            issues.push(format!(
                "sensor sample frame_id {} is missing from typed decoded frame re-import",
                row.frame_id
            ));
        }
        match (row.raw_i16, row.raw_u8) {
            (Some(raw_i16), None) if row.sample_value == i64::from(raw_i16) => {}
            (None, Some(raw_u8)) if row.sample_value == i64::from(raw_u8) => {}
            (Some(_), Some(_)) => issues.push(format!(
                "sensor sample {} must not set raw_i16 and raw_u8 together",
                row.sample_id
            )),
            (None, None) => issues.push(format!(
                "sensor sample {} must set one raw value",
                row.sample_id
            )),
            _ => issues.push(format!(
                "sensor sample {} sample_value must match raw value",
                row.sample_id
            )),
        }
        if row.unit == "raw_i16" && row.raw_i16.is_none() {
            issues.push(format!(
                "sensor sample {} raw_i16 unit requires raw_i16",
                row.sample_id
            ));
        }
        if row.unit.ends_with("_candidate") && row.raw_u8.is_none() {
            issues.push(format!(
                "sensor sample {} candidate unit requires raw_u8",
                row.sample_id
            ));
        }
        if !row.provenance.is_object() {
            issues.push(format!(
                "sensor sample {} provenance must be an object",
                row.sample_id
            ));
        }
    }
}

fn validate_metric_feature_report_reimport(
    rows: &[ExportMetricFeatureReportRow],
    issues: &mut Vec<String>,
) {
    let mut report_kinds = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.report_kind, "metric_features.report_kind", issues);
        validate_non_empty(&row.schema, "metric_features.schema", issues);
        validate_non_empty(&row.start_time, "metric_features.start_time", issues);
        validate_non_empty(&row.end_time, "metric_features.end_time", issues);
        if !report_kinds.insert(row.report_kind.clone()) {
            issues.push(format!(
                "metric feature report {} cannot be exported twice",
                row.report_kind
            ));
        }
        if row.feature_count < row.trusted_feature_count {
            issues.push(format!(
                "metric feature report {} trusted_feature_count exceeds feature_count",
                row.report_kind
            ));
        }
        match serde_json::from_str::<Value>(&row.issues_json) {
            Ok(value) if value.is_array() => {}
            Ok(_) => issues.push(format!(
                "metric feature report {} issues_json must be an array",
                row.report_kind
            )),
            Err(error) => issues.push(format!(
                "metric feature report {} issues_json invalid: {error}",
                row.report_kind
            )),
        }
        let report_schema = row
            .report_json
            .get("schema")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if report_schema != row.schema {
            issues.push(format!(
                "metric feature report {} schema does not match report_json",
                row.report_kind
            ));
        }
        let report_pass = row
            .report_json
            .get("pass")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if report_pass != row.pass {
            issues.push(format!(
                "metric feature report {} pass does not match report_json",
                row.report_kind
            ));
        }
    }
}

fn validate_metric_output_reimport(
    value_rows: &[ExportMetricValueRow],
    component_rows: &[ExportMetricComponentRow],
    algorithm_run_ids: Option<&BTreeSet<String>>,
    issues: &mut Vec<String>,
) {
    let mut metric_value_ids = BTreeSet::new();
    for row in value_rows {
        validate_non_empty(
            &row.metric_value_id,
            "metric_outputs.metric_value_id",
            issues,
        );
        validate_non_empty(&row.run_id, "metric_outputs.run_id", issues);
        validate_non_empty(&row.algorithm_id, "metric_outputs.algorithm_id", issues);
        validate_non_empty(&row.version, "metric_outputs.version", issues);
        validate_non_empty(&row.metric_family, "metric_outputs.metric_family", issues);
        validate_non_empty(&row.name, "metric_outputs.name", issues);
        validate_non_empty(&row.unit, "metric_outputs.unit", issues);
        validate_non_empty(&row.start_time, "metric_outputs.start_time", issues);
        validate_non_empty(&row.end_time, "metric_outputs.end_time", issues);
        if !metric_value_ids.insert(row.metric_value_id.clone()) {
            issues.push(format!(
                "metric value {} cannot be re-imported twice",
                row.metric_value_id
            ));
        }
        if !row.value.is_finite() {
            issues.push(format!(
                "metric value {} value must be finite",
                row.metric_value_id
            ));
        }
        validate_metric_output_run_reference(
            "metric value",
            &row.run_id,
            algorithm_run_ids,
            issues,
        );
        validate_string_array_items(
            &row.quality_flags,
            &format!("metric value {} quality_flags", row.metric_value_id),
            issues,
        );
        if !row.provenance.is_object() {
            issues.push(format!(
                "metric value {} provenance must be an object",
                row.metric_value_id
            ));
        }
    }

    let mut metric_component_ids = BTreeSet::new();
    for row in component_rows {
        validate_non_empty(
            &row.metric_component_id,
            "metric_outputs.metric_component_id",
            issues,
        );
        validate_non_empty(&row.run_id, "metric_outputs.run_id", issues);
        validate_non_empty(&row.algorithm_id, "metric_outputs.algorithm_id", issues);
        validate_non_empty(&row.version, "metric_outputs.version", issues);
        validate_non_empty(&row.metric_family, "metric_outputs.metric_family", issues);
        validate_non_empty(&row.component_name, "metric_outputs.component_name", issues);
        validate_non_empty(&row.unit, "metric_outputs.unit", issues);
        validate_non_empty(&row.start_time, "metric_outputs.start_time", issues);
        validate_non_empty(&row.end_time, "metric_outputs.end_time", issues);
        if !metric_component_ids.insert(row.metric_component_id.clone()) {
            issues.push(format!(
                "metric component {} cannot be re-imported twice",
                row.metric_component_id
            ));
        }
        if !row.value.is_finite() {
            issues.push(format!(
                "metric component {} value must be finite",
                row.metric_component_id
            ));
        }
        validate_optional_finite(
            row.score_0_to_100,
            &format!(
                "metric component {} score_0_to_100",
                row.metric_component_id
            ),
            issues,
        );
        validate_optional_finite(
            row.weight,
            &format!("metric component {} weight", row.metric_component_id),
            issues,
        );
        validate_optional_finite(
            row.contribution,
            &format!("metric component {} contribution", row.metric_component_id),
            issues,
        );
        validate_metric_output_run_reference(
            "metric component",
            &row.run_id,
            algorithm_run_ids,
            issues,
        );
        validate_string_array_items(
            &row.quality_flags,
            &format!("metric component {} quality_flags", row.metric_component_id),
            issues,
        );
        if !row.contribution_json.is_object() {
            issues.push(format!(
                "metric component {} contribution_json must be an object",
                row.metric_component_id
            ));
        }
        if !row.provenance.is_object() {
            issues.push(format!(
                "metric component {} provenance must be an object",
                row.metric_component_id
            ));
        }
    }
}

fn validate_algorithm_run_reimport(
    rows: &[AlgorithmRunRecord],
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut run_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.run_id, "algorithm_runs.run_id", issues);
        validate_non_empty(&row.algorithm_id, "algorithm_runs.algorithm_id", issues);
        validate_non_empty(&row.version, "algorithm_runs.version", issues);
        validate_non_empty(&row.start_time, "algorithm_runs.start_time", issues);
        validate_non_empty(&row.end_time, "algorithm_runs.end_time", issues);
        if !run_ids.insert(row.run_id.clone()) {
            issues.push(format!(
                "algorithm run run_id {} cannot be re-imported twice",
                row.run_id
            ));
        }
        validate_algorithm_run_output_json(row, issues);
        validate_algorithm_run_quality_flags_json(row, issues);
        validate_algorithm_run_provenance_json(row, issues);
    }
    run_ids
}

fn validate_algorithm_run_output_json(row: &AlgorithmRunRecord, issues: &mut Vec<String>) {
    match serde_json::from_str::<Value>(&row.output_json) {
        Ok(Value::Object(object)) if !object.is_empty() => {}
        Ok(_) => issues.push(format!(
            "algorithm run {} output_json must be a non-empty JSON object",
            row.run_id
        )),
        Err(error) => issues.push(format!(
            "algorithm run {} output_json is not valid JSON: {error}",
            row.run_id
        )),
    }
}

fn validate_algorithm_run_quality_flags_json(row: &AlgorithmRunRecord, issues: &mut Vec<String>) {
    match serde_json::from_str::<Value>(&row.quality_flags_json) {
        Ok(Value::Array(values)) if values.iter().all(Value::is_string) => {}
        Ok(Value::Array(_)) => issues.push(format!(
            "algorithm run {} quality_flags_json must be an array of strings",
            row.run_id
        )),
        Ok(_) => issues.push(format!(
            "algorithm run {} quality_flags_json must be an array",
            row.run_id
        )),
        Err(error) => issues.push(format!(
            "algorithm run {} quality_flags_json is not valid JSON: {error}",
            row.run_id
        )),
    }
}

fn validate_algorithm_run_provenance_json(row: &AlgorithmRunRecord, issues: &mut Vec<String>) {
    let provenance_json = match serde_json::from_str::<Value>(&row.provenance_json) {
        Ok(Value::Object(object)) if !object.is_empty() => Value::Object(object),
        Ok(_) => {
            issues.push(format!(
                "algorithm run {} provenance_json must be a non-empty JSON object",
                row.run_id
            ));
            return;
        }
        Err(error) => {
            issues.push(format!(
                "algorithm run {} provenance_json is not valid JSON: {error}",
                row.run_id
            ));
            return;
        }
    };
    let Some(run_provenance) = provenance_json.get("provenance") else {
        issues.push(format!(
            "algorithm run {} provenance_json.provenance is required",
            row.run_id
        ));
        return;
    };
    let Some(run_provenance_object) = run_provenance.as_object() else {
        issues.push(format!(
            "algorithm run {} provenance_json.provenance must be an object",
            row.run_id
        ));
        return;
    };
    if run_provenance_object.is_empty() {
        issues.push(format!(
            "algorithm run {} provenance_json.provenance must be non-empty",
            row.run_id
        ));
    }
    match provenance_json.get("errors") {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => {}
        Some(Value::Array(_)) => issues.push(format!(
            "algorithm run {} provenance_json.errors must be an array of strings",
            row.run_id
        )),
        Some(_) => issues.push(format!(
            "algorithm run {} provenance_json.errors must be an array",
            row.run_id
        )),
        None => issues.push(format!(
            "algorithm run {} provenance_json.errors is required",
            row.run_id
        )),
    }
    if let Some(provided_vitals) = run_provenance_object.get("provided_vitals") {
        validate_algorithm_run_provided_vitals(row, provided_vitals, issues);
    }
}

fn validate_algorithm_run_provided_vitals(
    row: &AlgorithmRunRecord,
    provided_vitals: &Value,
    issues: &mut Vec<String>,
) {
    let Some(provided_vitals) = provided_vitals.as_object() else {
        issues.push(format!(
            "algorithm run {} provided_vitals must be an object",
            row.run_id
        ));
        return;
    };
    if provided_vitals
        .get("source")
        .and_then(Value::as_str)
        .is_none_or(|source| source.trim().is_empty())
    {
        issues.push(format!(
            "algorithm run {} provided_vitals.source is required",
            row.run_id
        ));
    }
    if provided_vitals
        .get("trusted_metric_input")
        .and_then(Value::as_bool)
        != Some(true)
    {
        issues.push(format!(
            "algorithm run {} provided_vitals.trusted_metric_input must be true",
            row.run_id
        ));
    }
    match provided_vitals.get("quality_flags") {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
            if values
                .iter()
                .any(|value| value.as_str() == Some("provided_resp_temp_inputs_not_packet_derived"))
            {
                issues.push(format!(
                    "algorithm run {} provided_vitals quality_flags must not include provided_resp_temp_inputs_not_packet_derived",
                    row.run_id
                ));
            }
            if values
                .iter()
                .any(|value| value.as_str() == Some("provided_resp_temp_provenance_untrusted"))
            {
                issues.push(format!(
                    "algorithm run {} provided_vitals quality_flags must not include provided_resp_temp_provenance_untrusted",
                    row.run_id
                ));
            }
        }
        Some(Value::Array(_)) => issues.push(format!(
            "algorithm run {} provided_vitals.quality_flags must be an array of strings",
            row.run_id
        )),
        Some(_) => issues.push(format!(
            "algorithm run {} provided_vitals.quality_flags must be an array",
            row.run_id
        )),
        None => issues.push(format!(
            "algorithm run {} provided_vitals.quality_flags is required",
            row.run_id
        )),
    }
    let Some(provenance) = provided_vitals.get("provenance").and_then(Value::as_object) else {
        issues.push(format!(
            "algorithm run {} provided_vitals.provenance must be an object",
            row.run_id
        ));
        return;
    };
    match provenance.get("provided_vitals_provenance") {
        Some(Value::Object(object)) if !object.is_empty() => {}
        Some(_) => issues.push(format!(
            "algorithm run {} provided_vitals.provenance.provided_vitals_provenance must be a non-empty object",
            row.run_id
        )),
        None => issues.push(format!(
            "algorithm run {} provided_vitals.provenance.provided_vitals_provenance is required",
            row.run_id
        )),
    }
}

fn validate_metric_output_run_reference(
    row_kind: &str,
    run_id: &str,
    algorithm_run_ids: Option<&BTreeSet<String>>,
    issues: &mut Vec<String>,
) {
    if let Some(algorithm_run_ids) = algorithm_run_ids
        && !algorithm_run_ids.contains(run_id)
    {
        issues.push(format!(
            "{row_kind} run_id {run_id} is missing from algorithm run export"
        ));
    }
}

fn validate_optional_finite(value: Option<f64>, field: &str, issues: &mut Vec<String>) {
    if value.is_some_and(|value| !value.is_finite()) {
        issues.push(format!("{field} must be finite"));
    }
}

fn validate_optional_non_negative_i64(value: Option<i64>, field: &str, issues: &mut Vec<String>) {
    if value.is_some_and(|value| value < 0) {
        issues.push(format!("{field} must be non-negative"));
    }
}

fn validate_optional_non_negative_f64(value: Option<f64>, field: &str, issues: &mut Vec<String>) {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        issues.push(format!("{field} must be finite and non-negative"));
    }
}

fn validate_allowed_value(field: &str, value: &str, allowed: &[&str], issues: &mut Vec<String>) {
    if !allowed.contains(&value) {
        issues.push(format!("{field} must be one of: {}", allowed.join(", ")));
    }
}

fn validate_confidence_fraction(value: f64, field: &str, issues: &mut Vec<String>) {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        issues.push(format!("{field} must be a finite fraction between 0 and 1"));
    }
}

fn validate_optional_confidence_fraction(
    value: Option<f64>,
    field: &str,
    issues: &mut Vec<String>,
) {
    if let Some(value) = value {
        validate_confidence_fraction(value, field, issues);
    }
}

fn validate_rfc3339_utc_timestamp(value: &str, field: &str, issues: &mut Vec<String>) {
    if parse_rfc3339_utc_unix_ms(value).is_none() {
        issues.push(format!("{field} is not a supported UTC timestamp"));
    }
}

fn validate_string_array_items(values: &[String], field: &str, issues: &mut Vec<String>) {
    if values.iter().any(|value| value.trim().is_empty()) {
        issues.push(format!("{field} must not contain empty values"));
    }
}

fn validate_activity_session_reimport(
    rows: &[ActivitySessionRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut session_ids = BTreeSet::new();
    for row in rows {
        let row_name = format!("activity session {}", row.session_id);
        validate_non_empty(&row.session_id, "activity_sessions.session_id", issues);
        validate_non_empty(&row.source, "activity_sessions.source", issues);
        validate_non_empty(
            &row.activity_type,
            "activity_sessions.activity_type",
            issues,
        );
        validate_non_empty(
            &row.detection_method,
            "activity_sessions.detection_method",
            issues,
        );
        validate_non_empty(&row.sync_status, "activity_sessions.sync_status", issues);
        if !session_ids.insert(row.session_id.clone()) {
            issues.push(format!(
                "activity session session_id {} cannot be re-imported twice",
                row.session_id
            ));
        }
        if row.start_time_unix_ms < 0 {
            issues.push(format!(
                "{} start_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms < 0 {
            issues.push(format!(
                "{} end_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms <= row.start_time_unix_ms {
            issues.push(format!(
                "{} end_time_unix_ms must be greater than start_time_unix_ms",
                row_name
            ));
        }
        if row.duration_ms != row.end_time_unix_ms - row.start_time_unix_ms {
            issues.push(format!(
                "{} duration_ms does not match end_time_unix_ms - start_time_unix_ms",
                row_name
            ));
        }
        validate_allowed_value(
            &format!("{} activity_type", row_name),
            &row.activity_type,
            ALLOWED_ACTIVITY_TYPES,
            issues,
        );
        if let Some(value) = row.external_activity_type_code.as_deref() {
            validate_non_empty(
                value,
                "activity_sessions.external_activity_type_code",
                issues,
            );
        }
        if let Some(value) = row.external_activity_type_name.as_deref() {
            validate_non_empty(
                value,
                "activity_sessions.external_activity_type_name",
                issues,
            );
        }
        if let Some(value) = row.custom_label.as_deref() {
            validate_non_empty(value, "activity_sessions.custom_label", issues);
        }
        validate_confidence_fraction(row.confidence, &format!("{} confidence", row_name), issues);
        validate_allowed_value(
            &format!("{} detection_method", row_name),
            &row.detection_method,
            ALLOWED_ACTIVITY_DETECTION_METHODS,
            issues,
        );
        validate_allowed_value(
            &format!("{} sync_status", row_name),
            &row.sync_status,
            ALLOWED_ACTIVITY_SYNC_STATUSES,
            issues,
        );
        validate_json_object_text(
            &row.provenance_json,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.provenance_json,
            include_raw_bytes,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_rfc3339_utc_timestamp(
            &row.created_at,
            &format!("{} created_at", row_name),
            issues,
        );
        validate_rfc3339_utc_timestamp(
            &row.updated_at,
            &format!("{} updated_at", row_name),
            issues,
        );
    }
    session_ids
}

fn validate_activity_metric_reimport(
    rows: &[ActivityMetricRow],
    session_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut metric_ids = BTreeSet::new();
    for row in rows {
        let row_name = format!("activity metric {}", row.metric_id);
        validate_non_empty(&row.metric_id, "activity_metrics.metric_id", issues);
        validate_non_empty(
            &row.activity_session_id,
            "activity_metrics.activity_session_id",
            issues,
        );
        validate_non_empty(&row.metric_name, "activity_metrics.metric_name", issues);
        validate_non_empty(&row.unit, "activity_metrics.unit", issues);
        if !metric_ids.insert(row.metric_id.clone()) {
            issues.push(format!(
                "activity metric metric_id {} cannot be re-imported twice",
                row.metric_id
            ));
        }
        if !row.value.is_finite() {
            issues.push(format!("{} value must be finite", row_name));
        }
        if row.start_time_unix_ms < 0 {
            issues.push(format!(
                "{} start_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms < 0 {
            issues.push(format!(
                "{} end_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms <= row.start_time_unix_ms {
            issues.push(format!(
                "{} end_time_unix_ms must be greater than start_time_unix_ms",
                row_name
            ));
        }
        validate_allowed_value(
            &format!("{} unit", row_name),
            &row.unit,
            ALLOWED_ACTIVITY_METRIC_UNITS,
            issues,
        );
        if let Some(session_ids) = session_ids
            && !session_ids.contains(&row.activity_session_id)
        {
            issues.push(format!(
                "{} activity_session_id {} is missing from activity session export",
                row_name, row.activity_session_id
            ));
        }
        match serde_json::from_str::<Vec<String>>(&row.quality_flags_json) {
            Ok(values) => validate_string_array_items(
                &values,
                &format!("{} quality_flags_json", row_name),
                issues,
            ),
            Err(error) => issues.push(format!(
                "{} quality_flags_json is not valid JSON array of strings: {error}",
                row_name
            )),
        }
        validate_json_object_text(
            &row.provenance_json,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.provenance_json,
            include_raw_bytes,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_rfc3339_utc_timestamp(
            &row.created_at,
            &format!("{} created_at", row_name),
            issues,
        );
    }
    metric_ids
}

fn validate_activity_interval_reimport(
    rows: &[ActivityIntervalRow],
    session_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut interval_ids = BTreeSet::new();
    for row in rows {
        let row_name = format!("activity interval {}", row.interval_id);
        validate_non_empty(&row.interval_id, "activity_intervals.interval_id", issues);
        validate_non_empty(
            &row.activity_session_id,
            "activity_intervals.activity_session_id",
            issues,
        );
        validate_non_empty(
            &row.interval_type,
            "activity_intervals.interval_type",
            issues,
        );
        if !interval_ids.insert(row.interval_id.clone()) {
            issues.push(format!(
                "activity interval interval_id {} cannot be re-imported twice",
                row.interval_id
            ));
        }
        if row.start_time_unix_ms < 0 {
            issues.push(format!(
                "{} start_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms < 0 {
            issues.push(format!(
                "{} end_time_unix_ms must be non-negative",
                row_name
            ));
        }
        if row.end_time_unix_ms <= row.start_time_unix_ms {
            issues.push(format!(
                "{} end_time_unix_ms must be greater than start_time_unix_ms",
                row_name
            ));
        }
        if row.duration_ms != row.end_time_unix_ms - row.start_time_unix_ms {
            issues.push(format!(
                "{} duration_ms does not match end_time_unix_ms - start_time_unix_ms",
                row_name
            ));
        }
        if row.sequence < 0 {
            issues.push(format!("{} sequence must be non-negative", row_name));
        }
        validate_allowed_value(
            &format!("{} interval_type", row_name),
            &row.interval_type,
            ALLOWED_ACTIVITY_INTERVAL_TYPES,
            issues,
        );
        if let Some(session_ids) = session_ids
            && !session_ids.contains(&row.activity_session_id)
        {
            issues.push(format!(
                "{} activity_session_id {} is missing from activity session export",
                row_name, row.activity_session_id
            ));
        }
        validate_json_object_text(
            &row.metadata_json,
            &format!("{} metadata_json", row_name),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.metadata_json,
            include_raw_bytes,
            &format!("{} metadata_json", row_name),
            issues,
        );
        validate_json_object_text(
            &row.provenance_json,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.provenance_json,
            include_raw_bytes,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_rfc3339_utc_timestamp(
            &row.created_at,
            &format!("{} created_at", row_name),
            issues,
        );
    }
    interval_ids
}

fn validate_activity_label_reimport(
    rows: &[ActivityLabelRow],
    session_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut label_ids = BTreeSet::new();
    for row in rows {
        let row_name = format!("activity label {}", row.label_id);
        validate_non_empty(&row.label_id, "activity_labels.label_id", issues);
        validate_non_empty(
            &row.activity_session_id,
            "activity_labels.activity_session_id",
            issues,
        );
        validate_non_empty(&row.label_type, "activity_labels.label_type", issues);
        validate_non_empty(&row.value, "activity_labels.value", issues);
        validate_non_empty(&row.source, "activity_labels.source", issues);
        if !label_ids.insert(row.label_id.clone()) {
            issues.push(format!(
                "activity label label_id {} cannot be re-imported twice",
                row.label_id
            ));
        }
        validate_allowed_value(
            &format!("{} label_type", row_name),
            &row.label_type,
            ALLOWED_ACTIVITY_LABEL_TYPES,
            issues,
        );
        validate_optional_confidence_fraction(
            row.confidence,
            &format!("{} confidence", row_name),
            issues,
        );
        if let Some(session_ids) = session_ids
            && !session_ids.contains(&row.activity_session_id)
        {
            issues.push(format!(
                "{} activity_session_id {} is missing from activity session export",
                row_name, row.activity_session_id
            ));
        }
        validate_json_object_text(
            &row.provenance_json,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.provenance_json,
            include_raw_bytes,
            &format!("{} provenance_json", row_name),
            issues,
        );
        validate_rfc3339_utc_timestamp(
            &row.created_at,
            &format!("{} created_at", row_name),
            issues,
        );
    }
    label_ids
}

#[derive(Debug, Default)]
struct LocalHealthMetricReferences {
    daily_activity: BTreeMap<String, String>,
    hourly_activity: BTreeMap<String, String>,
    daily_recovery: BTreeMap<String, String>,
}

fn validate_local_health_metric_reimport(
    daily_activity_rows: &[DailyActivityMetricRow],
    hourly_activity_rows: &[HourlyActivityMetricRow],
    daily_recovery_rows: &[DailyRecoveryMetricRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> LocalHealthMetricReferences {
    LocalHealthMetricReferences {
        daily_activity: validate_daily_activity_metric_reimport(
            daily_activity_rows,
            include_raw_bytes,
            issues,
        ),
        hourly_activity: validate_hourly_activity_metric_reimport(
            hourly_activity_rows,
            include_raw_bytes,
            issues,
        ),
        daily_recovery: validate_daily_recovery_metric_reimport(
            daily_recovery_rows,
            include_raw_bytes,
            issues,
        ),
    }
}

fn validate_daily_activity_metric_reimport(
    rows: &[DailyActivityMetricRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut metric_ids = BTreeMap::new();
    for row in rows {
        let row_name = format!("daily activity metric {}", row.daily_metric_id);
        validate_non_empty(
            &row.daily_metric_id,
            "local_health_daily_activity_metrics.daily_metric_id",
            issues,
        );
        validate_non_empty(
            &row.date_key,
            "local_health_daily_activity_metrics.date_key",
            issues,
        );
        validate_non_empty(
            &row.timezone,
            "local_health_daily_activity_metrics.timezone",
            issues,
        );
        validate_metric_window(
            &row_name,
            row.start_time_unix_ms,
            row.end_time_unix_ms,
            issues,
        );
        validate_optional_non_negative_i64(row.steps, &format!("{row_name} steps"), issues);
        validate_optional_non_negative_f64(
            row.active_kcal,
            &format!("{row_name} active_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.resting_kcal,
            &format!("{row_name} resting_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.total_kcal,
            &format!("{row_name} total_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.average_cadence_spm,
            &format!("{row_name} average_cadence_spm"),
            issues,
        );
        validate_metric_source_kind(&row_name, &row.source_kind, issues);
        validate_confidence_fraction(row.confidence, &format!("{row_name} confidence"), issues);
        validate_activity_formatted_metric_value_policy(
            &row_name,
            &row.source_kind,
            row.steps,
            row.active_kcal,
            row.resting_kcal,
            row.total_kcal,
            row.average_cadence_spm,
            row.confidence,
            issues,
        );
        validate_metric_json_fields(
            &row.inputs_json,
            &row.quality_flags_json,
            &row.provenance_json,
            &row_name,
            include_raw_bytes,
            issues,
        );
        validate_rfc3339_utc_timestamp(&row.created_at, &format!("{row_name} created_at"), issues);
        validate_rfc3339_utc_timestamp(&row.updated_at, &format!("{row_name} updated_at"), issues);
        if metric_ids
            .insert(row.daily_metric_id.clone(), row.source_kind.clone())
            .is_some()
        {
            issues.push(format!(
                "daily activity metric daily_metric_id {} cannot be re-imported twice",
                row.daily_metric_id
            ));
        }
    }
    metric_ids
}

fn validate_hourly_activity_metric_reimport(
    rows: &[HourlyActivityMetricRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut metric_ids = BTreeMap::new();
    for row in rows {
        let row_name = format!("hourly activity metric {}", row.hourly_metric_id);
        validate_non_empty(
            &row.hourly_metric_id,
            "local_health_hourly_activity_metrics.hourly_metric_id",
            issues,
        );
        validate_non_empty(
            &row.date_key,
            "local_health_hourly_activity_metrics.date_key",
            issues,
        );
        validate_non_empty(
            &row.timezone,
            "local_health_hourly_activity_metrics.timezone",
            issues,
        );
        validate_metric_window(
            &row_name,
            row.start_time_unix_ms,
            row.end_time_unix_ms,
            issues,
        );
        validate_optional_non_negative_i64(row.steps, &format!("{row_name} steps"), issues);
        validate_optional_non_negative_f64(
            row.active_kcal,
            &format!("{row_name} active_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.resting_kcal,
            &format!("{row_name} resting_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.total_kcal,
            &format!("{row_name} total_kcal"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.average_cadence_spm,
            &format!("{row_name} average_cadence_spm"),
            issues,
        );
        validate_metric_source_kind(&row_name, &row.source_kind, issues);
        validate_confidence_fraction(row.confidence, &format!("{row_name} confidence"), issues);
        validate_activity_formatted_metric_value_policy(
            &row_name,
            &row.source_kind,
            row.steps,
            row.active_kcal,
            row.resting_kcal,
            row.total_kcal,
            row.average_cadence_spm,
            row.confidence,
            issues,
        );
        validate_metric_json_fields(
            &row.inputs_json,
            &row.quality_flags_json,
            &row.provenance_json,
            &row_name,
            include_raw_bytes,
            issues,
        );
        validate_rfc3339_utc_timestamp(&row.created_at, &format!("{row_name} created_at"), issues);
        validate_rfc3339_utc_timestamp(&row.updated_at, &format!("{row_name} updated_at"), issues);
        if metric_ids
            .insert(row.hourly_metric_id.clone(), row.source_kind.clone())
            .is_some()
        {
            issues.push(format!(
                "hourly activity metric hourly_metric_id {} cannot be re-imported twice",
                row.hourly_metric_id
            ));
        }
    }
    metric_ids
}

fn validate_daily_recovery_metric_reimport(
    rows: &[DailyRecoveryMetricRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut metric_ids = BTreeMap::new();
    for row in rows {
        let row_name = format!("daily recovery metric {}", row.daily_metric_id);
        validate_non_empty(
            &row.daily_metric_id,
            "local_health_daily_recovery_metrics.daily_metric_id",
            issues,
        );
        validate_non_empty(
            &row.date_key,
            "local_health_daily_recovery_metrics.date_key",
            issues,
        );
        validate_non_empty(
            &row.timezone,
            "local_health_daily_recovery_metrics.timezone",
            issues,
        );
        validate_metric_window(
            &row_name,
            row.start_time_unix_ms,
            row.end_time_unix_ms,
            issues,
        );
        validate_optional_non_negative_f64(
            row.resting_hr_bpm,
            &format!("{row_name} resting_hr_bpm"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.hrv_rmssd_ms,
            &format!("{row_name} hrv_rmssd_ms"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.respiratory_rate_rpm,
            &format!("{row_name} respiratory_rate_rpm"),
            issues,
        );
        validate_optional_non_negative_f64(
            row.oxygen_saturation_percent,
            &format!("{row_name} oxygen_saturation_percent"),
            issues,
        );
        validate_optional_finite(
            row.skin_temperature_delta_c,
            &format!("{row_name} skin_temperature_delta_c"),
            issues,
        );
        validate_metric_source_kind(&row_name, &row.source_kind, issues);
        validate_confidence_fraction(row.confidence, &format!("{row_name} confidence"), issues);
        validate_recovery_formatted_metric_value_policy(
            &row_name,
            &row.source_kind,
            row.resting_hr_bpm,
            row.hrv_rmssd_ms,
            row.respiratory_rate_rpm,
            row.oxygen_saturation_percent,
            row.skin_temperature_delta_c,
            row.confidence,
            issues,
        );
        validate_metric_json_fields(
            &row.inputs_json,
            &row.quality_flags_json,
            &row.provenance_json,
            &row_name,
            include_raw_bytes,
            issues,
        );
        validate_rfc3339_utc_timestamp(&row.created_at, &format!("{row_name} created_at"), issues);
        validate_rfc3339_utc_timestamp(&row.updated_at, &format!("{row_name} updated_at"), issues);
        if metric_ids
            .insert(row.daily_metric_id.clone(), row.source_kind.clone())
            .is_some()
        {
            issues.push(format!(
                "daily recovery metric daily_metric_id {} cannot be re-imported twice",
                row.daily_metric_id
            ));
        }
    }
    metric_ids
}

fn validate_metric_provenance_reimport(
    rows: &[MetricProvenanceRow],
    metric_refs: &LocalHealthMetricReferences,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut provenance_ids = BTreeSet::new();
    for row in rows {
        let row_name = format!("metric provenance {}", row.provenance_id);
        validate_non_empty(
            &row.provenance_id,
            "local_health_metric_provenance.provenance_id",
            issues,
        );
        validate_allowed_value(
            &format!("{row_name} metric_scope"),
            &row.metric_scope,
            ALLOWED_METRIC_PROVENANCE_SCOPES,
            issues,
        );
        validate_non_empty(
            &row.metric_id,
            "local_health_metric_provenance.metric_id",
            issues,
        );
        validate_metric_source_kind(&row_name, &row.source_kind, issues);
        validate_non_empty(
            &row.source_detail,
            "local_health_metric_provenance.source_detail",
            issues,
        );
        validate_no_official_whoop_label_text(
            &row.source_detail,
            &format!("{row_name} source_detail"),
            issues,
        );
        validate_no_platform_metric_source_text(
            &row.source_detail,
            &format!("{row_name} source_detail"),
            issues,
        );
        validate_optional_confidence_fraction(
            row.confidence,
            &format!("{row_name} confidence"),
            issues,
        );
        validate_unavailable_metric_provenance_confidence(
            &row_name,
            &row.source_kind,
            row.confidence,
            issues,
        );
        validate_metric_json_fields(
            &row.inputs_json,
            &row.quality_flags_json,
            &row.provenance_json,
            &row_name,
            include_raw_bytes,
            issues,
        );
        validate_rfc3339_utc_timestamp(&row.created_at, &format!("{row_name} created_at"), issues);
        if !provenance_ids.insert(row.provenance_id.clone()) {
            issues.push(format!(
                "metric provenance provenance_id {} cannot be re-imported twice",
                row.provenance_id
            ));
        }
        validate_metric_provenance_target_reference(row, metric_refs, issues);
    }
    provenance_ids
}

fn validate_metric_provenance_target_reference(
    row: &MetricProvenanceRow,
    metric_refs: &LocalHealthMetricReferences,
    issues: &mut Vec<String>,
) {
    let expected_source_kind = match row.metric_scope.as_str() {
        "daily_activity" => metric_refs.daily_activity.get(&row.metric_id),
        "hourly_activity" => metric_refs.hourly_activity.get(&row.metric_id),
        "daily_recovery" => metric_refs.daily_recovery.get(&row.metric_id),
        _ => return,
    };
    let Some(expected_source_kind) = expected_source_kind else {
        issues.push(format!(
            "metric provenance {} references missing {} metric_id {}",
            row.provenance_id, row.metric_scope, row.metric_id
        ));
        return;
    };
    if expected_source_kind != &row.source_kind {
        issues.push(format!(
            "metric provenance {} source_kind must match {} metric source_kind",
            row.provenance_id, row.metric_scope
        ));
    }
}

fn validate_metric_window(
    row_name: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    issues: &mut Vec<String>,
) {
    if start_time_unix_ms < 0 {
        issues.push(format!(
            "{row_name} start_time_unix_ms must be non-negative"
        ));
    }
    if end_time_unix_ms < 0 {
        issues.push(format!("{row_name} end_time_unix_ms must be non-negative"));
    }
    if end_time_unix_ms <= start_time_unix_ms {
        issues.push(format!(
            "{row_name} end_time_unix_ms must be greater than start_time_unix_ms"
        ));
    }
}

fn validate_metric_source_kind(row_name: &str, source_kind: &str, issues: &mut Vec<String>) {
    validate_allowed_value(
        &format!("{row_name} source_kind"),
        source_kind,
        ALLOWED_METRIC_SOURCE_KINDS,
        issues,
    );
}

fn validate_activity_formatted_metric_value_policy(
    row_name: &str,
    source_kind: &str,
    steps: Option<i64>,
    active_kcal: Option<f64>,
    resting_kcal: Option<f64>,
    total_kcal: Option<f64>,
    average_cadence_spm: Option<f64>,
    confidence: f64,
    issues: &mut Vec<String>,
) {
    let has_metric_value =
        steps.is_some() || active_kcal.is_some() || resting_kcal.is_some() || total_kcal.is_some();
    let has_any_value = has_metric_value || average_cadence_spm.is_some();
    if source_kind == "unavailable" {
        if has_any_value {
            issues.push(format!(
                "{row_name} unavailable activity metric must not carry metric values"
            ));
        }
        validate_unavailable_formatted_metric_confidence(row_name, confidence, issues);
    } else if !has_metric_value {
        issues.push(format!(
            "{row_name} available activity metric must include steps or calorie values"
        ));
    }
}

fn validate_recovery_formatted_metric_value_policy(
    row_name: &str,
    source_kind: &str,
    resting_hr_bpm: Option<f64>,
    hrv_rmssd_ms: Option<f64>,
    respiratory_rate_rpm: Option<f64>,
    oxygen_saturation_percent: Option<f64>,
    skin_temperature_delta_c: Option<f64>,
    confidence: f64,
    issues: &mut Vec<String>,
) {
    let has_metric_value = resting_hr_bpm.is_some()
        || hrv_rmssd_ms.is_some()
        || respiratory_rate_rpm.is_some()
        || oxygen_saturation_percent.is_some()
        || skin_temperature_delta_c.is_some();
    if source_kind == "unavailable" {
        if has_metric_value {
            issues.push(format!(
                "{row_name} unavailable recovery metric must not carry metric values"
            ));
        }
        validate_unavailable_formatted_metric_confidence(row_name, confidence, issues);
    } else if !has_metric_value {
        issues.push(format!(
            "{row_name} available recovery metric must include at least one recovery value"
        ));
    }
}

fn validate_unavailable_formatted_metric_confidence(
    row_name: &str,
    confidence: f64,
    issues: &mut Vec<String>,
) {
    if confidence != 0.0 {
        issues.push(format!(
            "{row_name} unavailable formatted metric must have confidence 0.0"
        ));
    }
}

fn validate_unavailable_metric_provenance_confidence(
    row_name: &str,
    source_kind: &str,
    confidence: Option<f64>,
    issues: &mut Vec<String>,
) {
    if source_kind == "unavailable" && confidence.unwrap_or(0.0) != 0.0 {
        issues.push(format!(
            "{row_name} unavailable metric provenance must have confidence 0.0"
        ));
    }
}

fn validate_metric_json_fields(
    inputs_json: &str,
    quality_flags_json: &str,
    provenance_json: &str,
    row_name: &str,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) {
    validate_json_object_text(inputs_json, &format!("{row_name} inputs_json"), issues);
    validate_raw_byte_json_policy(
        inputs_json,
        include_raw_bytes,
        &format!("{row_name} inputs_json"),
        issues,
    );
    validate_quality_flags_json(
        quality_flags_json,
        &format!("{row_name} quality_flags_json"),
        include_raw_bytes,
        issues,
    );
    validate_json_object_text(
        provenance_json,
        &format!("{row_name} provenance_json"),
        issues,
    );
    validate_raw_byte_json_policy(
        provenance_json,
        include_raw_bytes,
        &format!("{row_name} provenance_json"),
        issues,
    );
    validate_no_official_whoop_label_json(inputs_json, &format!("{row_name} inputs_json"), issues);
    validate_no_official_whoop_label_json(
        quality_flags_json,
        &format!("{row_name} quality_flags_json"),
        issues,
    );
    validate_no_official_whoop_label_json(
        provenance_json,
        &format!("{row_name} provenance_json"),
        issues,
    );
    validate_no_platform_metric_source_json(
        inputs_json,
        &format!("{row_name} inputs_json"),
        issues,
    );
    validate_no_platform_metric_source_json(
        quality_flags_json,
        &format!("{row_name} quality_flags_json"),
        issues,
    );
    validate_no_platform_metric_source_json(
        provenance_json,
        &format!("{row_name} provenance_json"),
        issues,
    );
}

fn validate_quality_flags_json(
    value: &str,
    field: &str,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(values) => validate_string_array_items(&values, field, issues),
        Err(error) => issues.push(format!(
            "{field} is not valid JSON array of strings: {error}"
        )),
    }
    validate_raw_byte_json_policy(value, include_raw_bytes, field, issues);
}

fn validate_calibration_label_reimport(
    rows: &[ExportCalibrationLabelRow],
    issues: &mut Vec<String>,
) {
    let mut label_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.label_id, "calibration_labels.label_id", issues);
        validate_non_empty(
            &row.metric_family,
            "calibration_labels.metric_family",
            issues,
        );
        validate_non_empty(&row.label_source, "calibration_labels.label_source", issues);
        validate_non_empty(&row.captured_at, "calibration_labels.captured_at", issues);
        validate_non_empty(&row.unit, "calibration_labels.unit", issues);
        if !label_ids.insert(row.label_id.clone()) {
            issues.push(format!(
                "calibration label label_id {} cannot be re-imported twice",
                row.label_id
            ));
        }
        if !row.value.is_finite() {
            issues.push(format!(
                "calibration label {} value must be finite",
                row.label_id
            ));
        }
        if !row.official_labels_are_labels {
            issues.push(format!(
                "calibration label {} must keep official_labels_are_labels=true",
                row.label_id
            ));
        }
        match serde_json::from_str::<serde_json::Value>(&row.provenance_json) {
            Ok(serde_json::Value::Object(object)) if !object.is_empty() => {}
            Ok(_) => issues.push(format!(
                "calibration label {} provenance_json must be a non-empty JSON object",
                row.label_id
            )),
            Err(error) => issues.push(format!(
                "calibration label {} provenance_json is not valid JSON: {error}",
                row.label_id
            )),
        }
    }
}

fn validate_calibration_run_reimport(rows: &[CalibrationRunRecord], issues: &mut Vec<String>) {
    let mut run_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(
            &row.calibration_run_id,
            "calibration_runs.calibration_run_id",
            issues,
        );
        validate_non_empty(&row.algorithm_id, "calibration_runs.algorithm_id", issues);
        validate_non_empty(&row.version, "calibration_runs.version", issues);
        validate_non_empty(
            &row.times.train_start,
            "calibration_runs.train_start",
            issues,
        );
        validate_non_empty(&row.times.train_end, "calibration_runs.train_end", issues);
        validate_non_empty(
            &row.times.holdout_start,
            "calibration_runs.holdout_start",
            issues,
        );
        validate_non_empty(
            &row.times.holdout_end,
            "calibration_runs.holdout_end",
            issues,
        );
        if !run_ids.insert(row.calibration_run_id.clone()) {
            issues.push(format!(
                "calibration run calibration_run_id {} cannot be re-imported twice",
                row.calibration_run_id
            ));
        }
        validate_calibration_run_times(row, issues);
        let metrics =
            validate_calibration_run_json_object(row, "metrics_json", &row.metrics_json, issues);
        let params =
            validate_calibration_run_json_object(row, "params_json", &row.params_json, issues);
        if let Some(metrics) = metrics.as_ref() {
            validate_calibration_run_metrics_json(row, metrics, issues);
        }
        if let Some(params) = params.as_ref() {
            validate_calibration_run_params_json(row, params, issues);
        }
        if let (Some(metrics), Some(params)) = (metrics.as_ref(), params.as_ref()) {
            validate_calibration_run_metric_param_consistency(row, metrics, params, issues);
        }
    }
}

fn validate_calibration_run_times(row: &CalibrationRunRecord, issues: &mut Vec<String>) {
    let train_start =
        parse_calibration_run_time(row, "train_start", &row.times.train_start, issues);
    let train_end = parse_calibration_run_time(row, "train_end", &row.times.train_end, issues);
    let holdout_start =
        parse_calibration_run_time(row, "holdout_start", &row.times.holdout_start, issues);
    let holdout_end =
        parse_calibration_run_time(row, "holdout_end", &row.times.holdout_end, issues);
    if let (Some(start), Some(end)) = (train_start, train_end)
        && start >= end
    {
        issues.push(format!(
            "calibration run {} train_start must be before train_end",
            row.calibration_run_id
        ));
    }
    if let (Some(start), Some(end)) = (holdout_start, holdout_end)
        && start >= end
    {
        issues.push(format!(
            "calibration run {} holdout_start must be before holdout_end",
            row.calibration_run_id
        ));
    }
    if let (Some(train_end), Some(holdout_start)) = (train_end, holdout_start)
        && train_end > holdout_start
    {
        issues.push(format!(
            "calibration run {} train_end must not be after holdout_start",
            row.calibration_run_id
        ));
    }
}

fn parse_calibration_run_time(
    row: &CalibrationRunRecord,
    field: &str,
    value: &str,
    issues: &mut Vec<String>,
) -> Option<i64> {
    let parsed = parse_rfc3339_utc_unix_ms(value);
    if parsed.is_none() && !value.trim().is_empty() {
        issues.push(format!(
            "calibration run {} {field} is not a supported UTC timestamp",
            row.calibration_run_id
        ));
    }
    parsed
}

fn validate_calibration_run_json_object(
    row: &CalibrationRunRecord,
    field: &str,
    value: &str,
    issues: &mut Vec<String>,
) -> Option<Value> {
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Object(object)) if !object.is_empty() => Some(Value::Object(object)),
        Ok(_) => {
            issues.push(format!(
                "calibration run {} {field} must be a non-empty JSON object",
                row.calibration_run_id
            ));
            None
        }
        Err(error) => {
            issues.push(format!(
                "calibration run {} {field} is not valid JSON: {error}",
                row.calibration_run_id
            ));
            None
        }
    }
}

fn validate_calibration_run_metrics_json(
    row: &CalibrationRunRecord,
    metrics: &Value,
    issues: &mut Vec<String>,
) {
    for key in [
        "dataset_valid",
        "labels_valid",
        "split_valid",
        "model_fit_ready",
        "train_metrics_ready",
        "holdout_metrics_ready",
        "holdout_improvement_valid",
        "calibration_ready",
    ] {
        if metrics.get(key).and_then(Value::as_bool) != Some(true) {
            issues.push(format!(
                "calibration run {} metrics_json.{key} must be true",
                row.calibration_run_id
            ));
        }
    }
    validate_calibration_run_string_array(
        row,
        metrics.get("issues"),
        "metrics_json.issues",
        issues,
    );
    match metrics.get("next_actions") {
        Some(Value::Array(values)) if values.iter().all(Value::is_object) => {}
        Some(Value::Array(_)) => issues.push(format!(
            "calibration run {} metrics_json.next_actions must be an array of objects",
            row.calibration_run_id
        )),
        Some(_) => issues.push(format!(
            "calibration run {} metrics_json.next_actions must be an array",
            row.calibration_run_id
        )),
        None => issues.push(format!(
            "calibration run {} metrics_json.next_actions is required",
            row.calibration_run_id
        )),
    }
}

fn validate_calibration_run_params_json(
    row: &CalibrationRunRecord,
    params: &Value,
    issues: &mut Vec<String>,
) {
    for key in [
        "dataset_valid",
        "labels_valid",
        "split_valid",
        "model_fit_ready",
        "holdout_improvement_valid",
        "calibration_ready",
        "pass",
    ] {
        if params.get(key).and_then(Value::as_bool) != Some(true) {
            issues.push(format!(
                "calibration run {} params_json.{key} must be true",
                row.calibration_run_id
            ));
        }
    }
    match params.get("model") {
        Some(Value::Object(object)) if !object.is_empty() => {
            for key in ["slope", "intercept"] {
                if !params
                    .get("model")
                    .and_then(|model| model.get(key))
                    .and_then(Value::as_f64)
                    .is_some_and(f64::is_finite)
                {
                    issues.push(format!(
                        "calibration run {} params_json.model.{key} must be finite",
                        row.calibration_run_id
                    ));
                }
            }
        }
        Some(_) => issues.push(format!(
            "calibration run {} params_json.model must be a non-empty object",
            row.calibration_run_id
        )),
        None => issues.push(format!(
            "calibration run {} params_json.model is required",
            row.calibration_run_id
        )),
    }
    if params
        .get("split_policy")
        .and_then(Value::as_str)
        .is_none_or(|policy| policy.trim().is_empty())
    {
        issues.push(format!(
            "calibration run {} params_json.split_policy is required",
            row.calibration_run_id
        ));
    }
}

fn validate_calibration_run_metric_param_consistency(
    row: &CalibrationRunRecord,
    metrics: &Value,
    params: &Value,
    issues: &mut Vec<String>,
) {
    for key in [
        "dataset_valid",
        "labels_valid",
        "split_valid",
        "model_fit_ready",
        "holdout_improvement_valid",
        "calibration_ready",
    ] {
        if metrics.get(key).and_then(Value::as_bool) != params.get(key).and_then(Value::as_bool) {
            issues.push(format!(
                "calibration run {} {key} differs between metrics_json and params_json",
                row.calibration_run_id
            ));
        }
    }
}

fn validate_calibration_run_string_array(
    row: &CalibrationRunRecord,
    value: Option<&Value>,
    field: &str,
    issues: &mut Vec<String>,
) {
    match value {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => {}
        Some(Value::Array(_)) => issues.push(format!(
            "calibration run {} {field} must be an array of strings",
            row.calibration_run_id
        )),
        Some(_) => issues.push(format!(
            "calibration run {} {field} must be an array",
            row.calibration_run_id
        )),
        None => issues.push(format!(
            "calibration run {} {field} is required",
            row.calibration_run_id
        )),
    }
}

fn validate_debug_session_reimport(
    rows: &[ExportDebugSessionRow],
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut session_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.session_id, "debug_sessions.session_id", issues);
        validate_non_empty(&row.bridge_url, "debug_sessions.bridge_url", issues);
        validate_non_empty(&row.bind_host, "debug_sessions.bind_host", issues);
        if row.started_at_unix_ms < 0 {
            issues.push(format!(
                "debug session {} started_at_unix_ms must be non-negative",
                row.session_id
            ));
        }
        if !session_ids.insert(row.session_id.clone()) {
            issues.push(format!(
                "debug session session_id {} cannot be re-imported twice",
                row.session_id
            ));
        }
        if !is_loopback_bind_host(&row.bind_host) {
            issues.push(format!(
                "debug session {} bind_host must be loopback",
                row.session_id
            ));
        }
        if row.remote_bind_enabled {
            issues.push(format!(
                "debug session {} remote_bind_enabled must be false",
                row.session_id
            ));
        }
        if !row.token_required {
            issues.push(format!(
                "debug session {} token_required must be true",
                row.session_id
            ));
        }
        if !row.token_present {
            issues.push(format!(
                "debug session {} token_present must be true",
                row.session_id
            ));
        }
        if has_unredacted_debug_token(&row.bridge_url) {
            issues.push(format!(
                "debug session {} bridge_url contains an unredacted token query parameter",
                row.session_id
            ));
        }
        if row.token_present && !row.bridge_url.contains("token=<redacted>") {
            issues.push(format!(
                "debug session {} bridge_url must preserve a redacted token marker",
                row.session_id
            ));
        }
    }
    session_ids
}

fn validate_debug_command_reimport(
    rows: &[ExportDebugCommandRow],
    session_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut command_ids = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.command_id, "debug_commands.command_id", issues);
        validate_non_empty(&row.session_id, "debug_commands.session_id", issues);
        validate_non_empty(&row.schema, "debug_commands.schema", issues);
        validate_non_empty(&row.command, "debug_commands.command", issues);
        if row.received_at_unix_ms < 0 {
            issues.push(format!(
                "debug command {} received_at_unix_ms must be non-negative",
                row.command_id
            ));
        }
        if !command_ids.insert(row.command_id.clone()) {
            issues.push(format!(
                "debug command command_id {} cannot be re-imported twice",
                row.command_id
            ));
        }
        if row.schema != "goose.debug.command.v1" {
            issues.push(format!(
                "debug command {} schema must be goose.debug.command.v1",
                row.command_id
            ));
        }
        if let Some(session_ids) = session_ids
            && !session_ids.contains(&row.session_id)
        {
            issues.push(format!(
                "debug command {} session_id {} is missing from debug session export",
                row.command_id, row.session_id
            ));
        }
        validate_json_object_text(
            &row.args_json,
            &format!("debug command {} args_json", row.command_id),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.args_json,
            include_raw_bytes,
            &format!("debug command {} args_json", row.command_id),
            issues,
        );
        if has_unredacted_debug_token(&row.args_json) {
            issues.push(format!(
                "debug command {} args_json contains an unredacted token query parameter",
                row.command_id
            ));
        }
    }
    command_ids
}

fn validate_debug_event_reimport(
    rows: &[ExportDebugEventRow],
    session_ids: Option<&BTreeSet<String>>,
    command_ids: Option<&BTreeSet<String>>,
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) {
    let mut event_ids = BTreeSet::new();
    let mut latest_by_session = BTreeMap::<String, (i64, i64)>::new();
    for row in rows {
        validate_non_empty(&row.session_id, "debug_events.session_id", issues);
        validate_non_empty(&row.schema, "debug_events.schema", issues);
        validate_non_empty(&row.source, "debug_events.source", issues);
        validate_non_empty(&row.level, "debug_events.level", issues);
        validate_non_empty(&row.topic, "debug_events.topic", issues);
        validate_non_empty(&row.message, "debug_events.message", issues);
        if row.sequence <= 0 {
            issues.push(format!(
                "debug event {}:{} sequence must be positive",
                row.session_id, row.sequence
            ));
        }
        if row.time_unix_ms < 0 {
            issues.push(format!(
                "debug event {}:{} time_unix_ms must be non-negative",
                row.session_id, row.sequence
            ));
        }
        let event_id = (row.session_id.clone(), row.sequence);
        if !event_ids.insert(event_id) {
            issues.push(format!(
                "debug event {}:{} cannot be re-imported twice",
                row.session_id, row.sequence
            ));
        }
        if row.schema != "goose.debug.event.v1" {
            issues.push(format!(
                "debug event {}:{} schema must be goose.debug.event.v1",
                row.session_id, row.sequence
            ));
        }
        if let Some(session_ids) = session_ids
            && !session_ids.contains(&row.session_id)
        {
            issues.push(format!(
                "debug event {}:{} session_id is missing from debug session export",
                row.session_id, row.sequence
            ));
        }
        if let (Some(command_id), Some(command_ids)) = (row.command_id.as_deref(), command_ids)
            && !command_ids.contains(command_id)
        {
            issues.push(format!(
                "debug event {}:{} command_id {} is missing from debug command export",
                row.session_id, row.sequence, command_id
            ));
        }
        if let Some((previous_sequence, previous_time)) =
            latest_by_session.get(row.session_id.as_str()).copied()
        {
            if row.sequence <= previous_sequence {
                issues.push(format!(
                    "debug event {}:{} sequence must be after previous sequence {}",
                    row.session_id, row.sequence, previous_sequence
                ));
            }
            if row.time_unix_ms < previous_time {
                issues.push(format!(
                    "debug event {}:{} time_unix_ms must not move backwards",
                    row.session_id, row.sequence
                ));
            }
        }
        latest_by_session.insert(row.session_id.clone(), (row.sequence, row.time_unix_ms));
        validate_json_object_text(
            &row.data_json,
            &format!("debug event {}:{} data_json", row.session_id, row.sequence),
            issues,
        );
        validate_raw_byte_json_policy(
            &row.data_json,
            include_raw_bytes,
            &format!("debug event {}:{} data_json", row.session_id, row.sequence),
            issues,
        );
        if has_unredacted_debug_token(&row.message) || has_unredacted_debug_token(&row.data_json) {
            issues.push(format!(
                "debug event {}:{} contains an unredacted token query parameter",
                row.session_id, row.sequence
            ));
        }
    }
}

fn validate_json_object_text(value: &str, field: &str, issues: &mut Vec<String>) {
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Object(_)) => {}
        Ok(_) => issues.push(format!("{field} must be a JSON object")),
        Err(error) => issues.push(format!("{field} is not valid JSON: {error}")),
    }
}

fn validate_no_official_whoop_label_json(value: &str, field: &str, issues: &mut Vec<String>) {
    match serde_json::from_str::<Value>(value) {
        Ok(parsed) => {
            if value_contains_official_whoop_label_marker(&parsed) {
                issues.push(format!(
                    "{field} must not contain official WHOOP label markers for formatted local metrics",
                ));
            }
        }
        Err(error) => issues.push(format!("{field} is not valid JSON: {error}")),
    }
}

fn validate_no_official_whoop_label_text(value: &str, field: &str, issues: &mut Vec<String>) {
    if is_official_whoop_label_token(value) {
        issues.push(format!(
            "{field} must not identify official WHOOP labels as a formatted metric source",
        ));
    }
}

fn validate_no_platform_metric_source_json(value: &str, field: &str, issues: &mut Vec<String>) {
    match serde_json::from_str::<Value>(value) {
        Ok(parsed) => {
            if value_contains_platform_metric_source_marker(&parsed, None) {
                issues.push(format!(
                    "{field} must not contain HealthKit, Health Connect, Apple Health, or platform-import markers as formatted metric sources",
                ));
            }
        }
        Err(error) => issues.push(format!("{field} is not valid JSON: {error}")),
    }
}

fn validate_no_platform_metric_source_text(value: &str, field: &str, issues: &mut Vec<String>) {
    if is_platform_metric_source_token(value, None) {
        issues.push(format!(
            "{field} must not identify HealthKit, Health Connect, Apple Health, or platform imports as a formatted metric source",
        ));
    }
}

fn value_contains_official_whoop_label_marker(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            if matches!(
                normalized_marker(key).as_str(),
                "official_whoop_label" | "whoop_label"
            ) && child.as_bool().unwrap_or(true)
            {
                return true;
            }
            value_contains_official_whoop_label_marker(child)
        }),
        Value::Array(values) => values
            .iter()
            .any(value_contains_official_whoop_label_marker),
        Value::String(text) => is_official_whoop_label_token(text),
        _ => false,
    }
}

fn is_official_whoop_label_token(value: &str) -> bool {
    let normalized = normalized_marker(value);
    matches!(
        normalized.as_str(),
        "whoop"
            | "whoop_app"
            | "whoop_backend"
            | "official_whoop"
            | "official_whoop_label"
            | "official_whoop_app"
            | "official_whoop_backend"
            | "official_whoop_value"
            | "official_whoop_values"
            | "validation_label_from_whoop"
    ) || normalized.starts_with("official_whoop_")
        || normalized.starts_with("whoop_backend_")
}

fn value_contains_platform_metric_source_marker(value: &Value, parent_key: Option<&str>) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            if is_platform_metric_source_token(key, None) {
                return true;
            }
            value_contains_platform_metric_source_marker(child, Some(key))
        }),
        Value::Array(values) => values
            .iter()
            .any(|child| value_contains_platform_metric_source_marker(child, parent_key)),
        Value::String(text) => is_platform_metric_source_token(text, parent_key),
        _ => false,
    }
}

fn is_platform_metric_source_token(value: &str, parent_key: Option<&str>) -> bool {
    let normalized = normalized_marker(value);
    if !contains_platform_metric_source_marker(&normalized) {
        return false;
    }
    let parent_context = parent_key.map(normalized_marker);
    if parent_context
        .as_deref()
        .is_some_and(is_allowed_profile_platform_context)
        || is_allowed_profile_platform_context(&normalized)
    {
        return false;
    }
    true
}

fn contains_platform_metric_source_marker(normalized: &str) -> bool {
    normalized.contains("healthkit")
        || normalized.contains("health_kit")
        || normalized.contains("apple_health")
        || normalized.contains("applehealth")
        || normalized.contains("health_connect")
        || normalized.contains("healthconnect")
        || normalized.contains("android_health")
        || normalized.contains("platform_import")
        || normalized.contains("platform_imported")
        || normalized.contains("imported_platform")
        || normalized.contains("external_history_context_only")
        || normalized.contains("hkquantitytypeidentifier")
        || normalized.contains("hksample")
}

fn is_allowed_profile_platform_context(normalized: &str) -> bool {
    normalized.contains("profile")
        || normalized.contains("weight")
        || normalized.contains("body_mass")
        || normalized.contains("bodymass")
}

fn normalized_marker(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '.', ':'], "_")
}

fn is_loopback_bind_host(value: &str) -> bool {
    matches!(value.trim(), "127.0.0.1" | "localhost" | "::1")
}

fn has_unredacted_debug_token(value: &str) -> bool {
    let mut rest = value;
    while let Some(index) = rest.find("token=") {
        let token_value = &rest[index + "token=".len()..];
        if !token_value.starts_with("<redacted>") {
            return true;
        }
        rest = &token_value["<redacted>".len()..];
    }
    false
}

fn validate_command_validation_reimport(
    rows: &[ExportCommandValidationRow],
    include_raw_bytes: bool,
    issues: &mut Vec<String>,
) {
    let mut commands = BTreeSet::new();
    for row in rows {
        validate_non_empty(&row.command, "command_validation.command", issues);
        validate_non_empty(&row.family, "command_validation.family", issues);
        validate_non_empty(&row.risk_gate, "command_validation.risk_gate", issues);
        if !commands.insert(row.command.clone()) {
            issues.push(format!(
                "command validation {} cannot be re-imported twice",
                row.command
            ));
        }
        validate_json_string_array_value(
            &row.missing_requirements,
            &format!("command validation {} missing_requirements", row.command),
            issues,
        );
        validate_json_string_array_value(
            &row.warnings,
            &format!("command validation {} warnings", row.command),
            issues,
        );
        validate_command_next_actions_value(
            &row.next_capture_actions,
            &format!("command validation {} next_capture_actions", row.command),
            issues,
        );
        if !row.report_json.is_object() {
            issues.push(format!(
                "command validation {} report_json must be an object",
                row.command
            ));
            continue;
        }
        validate_report_string_match(
            &row.report_json,
            "command",
            &row.command,
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_string_match(
            &row.report_json,
            "risk_gate",
            &row.risk_gate,
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_string_match(
            &row.report_json,
            "family",
            &row.family,
            &format!("command validation {}", row.command),
            issues,
        );
        match row
            .report_json
            .get("direct_send_ready")
            .and_then(Value::as_bool)
        {
            Some(report_ready) if report_ready == row.direct_send_ready => {}
            Some(_) => issues.push(format!(
                "command validation {} direct_send_ready does not match report_json",
                row.command
            )),
            None => issues.push(format!(
                "command validation {} report_json.direct_send_ready is required",
                row.command
            )),
        }
        let report_command_number = row
            .report_json
            .get("command_number")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok());
        if report_command_number != row.command_number {
            issues.push(format!(
                "command validation {} command_number does not match report_json",
                row.command
            ));
        }
        validate_report_value_match(
            &row.report_json,
            "missing_requirements",
            &row.missing_requirements,
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_value_match(
            &row.report_json,
            "warnings",
            &row.warnings,
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_value_match(
            &row.report_json,
            "next_capture_actions",
            &row.next_capture_actions,
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_service_uuid",
            row.validated_service_uuid.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_characteristic_uuid",
            row.validated_characteristic_uuid.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_write_type",
            row.validated_write_type.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_evidence_source",
            row.validated_evidence_source.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_capture_kind",
            row.validated_capture_kind.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_owner",
            row.validated_owner.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_provenance_json",
            row.validated_provenance_json.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        validate_report_optional_string_match(
            &row.report_json,
            "validated_triggering_ui_action",
            row.validated_triggering_ui_action.as_deref(),
            &format!("command validation {}", row.command),
            issues,
        );
        if row.direct_send_ready
            && (row
                .validated_service_uuid
                .as_deref()
                .unwrap_or_default()
                .is_empty()
                || row
                    .validated_characteristic_uuid
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
                || row
                    .validated_write_type
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty())
        {
            issues.push(format!(
                "command validation {} direct_send_ready requires endpoint and write type",
                row.command
            ));
        }
        if row.direct_send_ready
            && (row
                .validated_evidence_source
                .as_deref()
                .unwrap_or_default()
                .is_empty()
                || row
                    .validated_provenance_json
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty())
        {
            issues.push(format!(
                "command validation {} direct_send_ready requires trusted evidence provenance",
                row.command
            ));
        }
        if row.direct_send_ready
            && row.risk_gate != "read_only"
            && row
                .validated_triggering_ui_action
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            issues.push(format!(
                "command validation {} direct_send_ready requires triggering UI action",
                row.command
            ));
        }
        if let Some(provenance_json) = row.validated_provenance_json.as_deref() {
            match serde_json::from_str::<Value>(provenance_json) {
                Ok(Value::Object(object)) if !object.is_empty() => {}
                Ok(_) => issues.push(format!(
                    "command validation {} validated_provenance_json must be a non-empty object",
                    row.command
                )),
                Err(error) => issues.push(format!(
                    "command validation {} validated_provenance_json is not valid JSON: {error}",
                    row.command
                )),
            }
        }
        validate_raw_byte_json_value_policy(
            &row.report_json,
            include_raw_bytes,
            &format!("command validation {} report_json", row.command),
            issues,
        );
    }
}

fn validate_json_string_array_value(value: &Value, field: &str, issues: &mut Vec<String>) {
    match value.as_array() {
        Some(values) if values.iter().all(Value::is_string) => {}
        Some(_) => issues.push(format!("{field} must be an array of strings")),
        None => issues.push(format!("{field} must be an array")),
    }
}

fn validate_command_next_actions_value(value: &Value, field: &str, issues: &mut Vec<String>) {
    let Some(values) = value.as_array() else {
        issues.push(format!("{field} must be an array"));
        return;
    };
    for (index, value) in values.iter().enumerate() {
        let Some(object) = value.as_object() else {
            issues.push(format!("{field}[{index}] must be an object"));
            continue;
        };
        if object
            .get("requirement")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            issues.push(format!("{field}[{index}].requirement is required"));
        }
        if object
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            issues.push(format!("{field}[{index}].action is required"));
        }
    }
}

fn validate_report_string_match(
    report_json: &Value,
    field: &str,
    expected: &str,
    row_name: &str,
    issues: &mut Vec<String>,
) {
    match report_json.get(field).and_then(Value::as_str) {
        Some(value) if value == expected => {}
        Some(_) => issues.push(format!("{row_name} {field} does not match report_json")),
        None => issues.push(format!("{row_name} report_json.{field} is required")),
    }
}

fn validate_report_optional_string_match(
    report_json: &Value,
    field: &str,
    expected: Option<&str>,
    row_name: &str,
    issues: &mut Vec<String>,
) {
    let actual = report_json.get(field).and_then(Value::as_str);
    if actual != expected {
        issues.push(format!("{row_name} {field} does not match report_json"));
    }
}

fn validate_report_value_match(
    report_json: &Value,
    field: &str,
    expected: &Value,
    row_name: &str,
    issues: &mut Vec<String>,
) {
    match report_json.get(field) {
        Some(value) if value == expected => {}
        Some(_) => issues.push(format!("{row_name} {field} does not match report_json")),
        None => issues.push(format!("{row_name} report_json.{field} is required")),
    }
}

fn validate_non_empty(value: &str, field: &str, issues: &mut Vec<String>) {
    if value.trim().is_empty() {
        issues.push(format!("{field} is required for re-import"));
    }
}

fn validate_hex_field(value: &str, field: &str, issues: &mut Vec<String>) {
    if let Err(error) = hex::decode(value) {
        issues.push(format!("{field} is not valid hex: {error}"));
    }
}

fn validate_sha256_hex_field(value: &str, field: &str, issues: &mut Vec<String>) {
    if value.len() != 64 {
        issues.push(format!("{field} must be a 64-character sha256 hex digest"));
        return;
    }
    validate_hex_field(value, field, issues);
}

fn validate_json_text(value: &str, field: &str, issues: &mut Vec<String>) {
    if let Err(error) = serde_json::from_str::<serde_json::Value>(value) {
        issues.push(format!("{field} is not valid JSON: {error}"));
    }
}

fn validate_raw_byte_json_policy(
    value: &str,
    include_raw_bytes: bool,
    field: &str,
    issues: &mut Vec<String>,
) {
    if include_raw_bytes {
        return;
    }
    if let Ok(value) = serde_json::from_str::<Value>(value) {
        validate_raw_byte_json_value_policy(&value, false, field, issues)
    }
}

fn validate_raw_byte_json_value_policy(
    value: &Value,
    include_raw_bytes: bool,
    field: &str,
    issues: &mut Vec<String>,
) {
    if include_raw_bytes {
        return;
    }
    validate_no_raw_byte_json_values(value, field, issues);
}

fn validate_no_raw_byte_json_values(value: &Value, field: &str, issues: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let child_field = format!("{field}.{key}");
                if is_raw_byte_json_key(key) {
                    let has_raw_value = match value {
                        Value::String(text) => !text.is_empty(),
                        Value::Null => false,
                        _ => true,
                    };
                    if has_raw_value {
                        issues.push(format!(
                            "{child_field} must be empty when include_raw_bytes is false"
                        ));
                    }
                } else {
                    validate_no_raw_byte_json_values(value, &child_field, issues);
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_no_raw_byte_json_values(value, &format!("{field}[{index}]"), issues);
            }
        }
        _ => {}
    }
}

fn manifest_lists_path(manifest: &ExportManifest, path: &str) -> bool {
    manifest.files.iter().any(|file| file.path == path)
}

fn export_manifest_file_family(path: &str) -> Option<&'static str> {
    match path {
        "data/raw_evidence.jsonl" | "data/raw_evidence.csv" => Some(RAW_EXPORT_RAW_EVIDENCE_FAMILY),
        "data/decoded_frames.jsonl" | "data/decoded_frames.csv" => {
            Some(RAW_EXPORT_DECODED_FRAMES_FAMILY)
        }
        "data/packet_timeline.jsonl" | "data/packet_timeline.csv" => {
            Some(RAW_EXPORT_PACKET_TIMELINE_FAMILY)
        }
        "data/sensor_samples.jsonl" | "data/sensor_samples.csv" => {
            Some(RAW_EXPORT_SENSOR_SAMPLES_FAMILY)
        }
        "data/metric_features.jsonl" | "data/metric_features.csv" => {
            Some(RAW_EXPORT_METRIC_FEATURES_FAMILY)
        }
        "data/metric_values.jsonl"
        | "data/metric_values.csv"
        | "data/metric_components.jsonl"
        | "data/metric_components.csv" => Some(RAW_EXPORT_METRIC_OUTPUTS_FAMILY),
        "data/algorithm_runs.jsonl" | "data/algorithm_runs.csv" => {
            Some(RAW_EXPORT_ALGORITHM_RUNS_FAMILY)
        }
        "data/calibration_labels.jsonl" | "data/calibration_labels.csv" => {
            Some(RAW_EXPORT_CALIBRATION_LABELS_FAMILY)
        }
        "data/calibration_runs.jsonl" | "data/calibration_runs.csv" => {
            Some(RAW_EXPORT_CALIBRATION_RUNS_FAMILY)
        }
        "data/activity_sessions.jsonl" | "data/activity_sessions.csv" => {
            Some(RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY)
        }
        "data/activity_metrics.jsonl" | "data/activity_metrics.csv" => {
            Some(RAW_EXPORT_ACTIVITY_METRICS_FAMILY)
        }
        "data/activity_intervals.jsonl" | "data/activity_intervals.csv" => {
            Some(RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY)
        }
        "data/activity_labels.jsonl" | "data/activity_labels.csv" => {
            Some(RAW_EXPORT_ACTIVITY_LABELS_FAMILY)
        }
        "data/local_health_daily_activity_metrics.jsonl"
        | "data/local_health_daily_activity_metrics.csv"
        | "data/local_health_hourly_activity_metrics.jsonl"
        | "data/local_health_hourly_activity_metrics.csv"
        | "data/local_health_daily_recovery_metrics.jsonl"
        | "data/local_health_daily_recovery_metrics.csv"
        | "data/local_health_metric_provenance.jsonl"
        | "data/local_health_metric_provenance.csv" => Some(RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY),
        "data/debug_sessions.jsonl" | "data/debug_sessions.csv" => {
            Some(RAW_EXPORT_DEBUG_SESSIONS_FAMILY)
        }
        "data/debug_commands.jsonl" | "data/debug_commands.csv" => {
            Some(RAW_EXPORT_DEBUG_COMMANDS_FAMILY)
        }
        "data/debug_events.jsonl" | "data/debug_events.csv" => Some(RAW_EXPORT_DEBUG_EVENTS_FAMILY),
        "data/command_validation.jsonl" | "data/command_validation.csv" => {
            Some(RAW_EXPORT_COMMAND_VALIDATION_FAMILY)
        }
        "data/goose.sqlite" => Some(RAW_EXPORT_SQLITE_FAMILY),
        _ => None,
    }
}

fn required_export_artifact_paths_for_family(family: &str) -> &'static [&'static str] {
    match family {
        RAW_EXPORT_RAW_EVIDENCE_FAMILY => &["data/raw_evidence.jsonl", "data/raw_evidence.csv"],
        RAW_EXPORT_DECODED_FRAMES_FAMILY => {
            &["data/decoded_frames.jsonl", "data/decoded_frames.csv"]
        }
        RAW_EXPORT_PACKET_TIMELINE_FAMILY => {
            &["data/packet_timeline.jsonl", "data/packet_timeline.csv"]
        }
        RAW_EXPORT_SENSOR_SAMPLES_FAMILY => {
            &["data/sensor_samples.jsonl", "data/sensor_samples.csv"]
        }
        RAW_EXPORT_METRIC_FEATURES_FAMILY => {
            &["data/metric_features.jsonl", "data/metric_features.csv"]
        }
        RAW_EXPORT_METRIC_OUTPUTS_FAMILY => &[
            "data/metric_values.jsonl",
            "data/metric_values.csv",
            "data/metric_components.jsonl",
            "data/metric_components.csv",
        ],
        RAW_EXPORT_ALGORITHM_RUNS_FAMILY => {
            &["data/algorithm_runs.jsonl", "data/algorithm_runs.csv"]
        }
        RAW_EXPORT_CALIBRATION_LABELS_FAMILY => &[
            "data/calibration_labels.jsonl",
            "data/calibration_labels.csv",
        ],
        RAW_EXPORT_CALIBRATION_RUNS_FAMILY => {
            &["data/calibration_runs.jsonl", "data/calibration_runs.csv"]
        }
        RAW_EXPORT_ACTIVITY_SESSIONS_FAMILY => {
            &["data/activity_sessions.jsonl", "data/activity_sessions.csv"]
        }
        RAW_EXPORT_ACTIVITY_METRICS_FAMILY => {
            &["data/activity_metrics.jsonl", "data/activity_metrics.csv"]
        }
        RAW_EXPORT_ACTIVITY_INTERVALS_FAMILY => &[
            "data/activity_intervals.jsonl",
            "data/activity_intervals.csv",
        ],
        RAW_EXPORT_ACTIVITY_LABELS_FAMILY => {
            &["data/activity_labels.jsonl", "data/activity_labels.csv"]
        }
        RAW_EXPORT_LOCAL_HEALTH_METRICS_FAMILY => &[
            "data/local_health_daily_activity_metrics.jsonl",
            "data/local_health_daily_activity_metrics.csv",
            "data/local_health_hourly_activity_metrics.jsonl",
            "data/local_health_hourly_activity_metrics.csv",
            "data/local_health_daily_recovery_metrics.jsonl",
            "data/local_health_daily_recovery_metrics.csv",
            "data/local_health_metric_provenance.jsonl",
            "data/local_health_metric_provenance.csv",
        ],
        RAW_EXPORT_DEBUG_SESSIONS_FAMILY => {
            &["data/debug_sessions.jsonl", "data/debug_sessions.csv"]
        }
        RAW_EXPORT_DEBUG_COMMANDS_FAMILY => {
            &["data/debug_commands.jsonl", "data/debug_commands.csv"]
        }
        RAW_EXPORT_DEBUG_EVENTS_FAMILY => &["data/debug_events.jsonl", "data/debug_events.csv"],
        RAW_EXPORT_COMMAND_VALIDATION_FAMILY => &[
            "data/command_validation.jsonl",
            "data/command_validation.csv",
        ],
        RAW_EXPORT_SQLITE_FAMILY => &["data/goose.sqlite"],
        _ => &[],
    }
}

fn family_is_listed(manifest: &ExportManifest, family: &str) -> bool {
    manifest
        .data_families
        .iter()
        .any(|data_family| data_family == family)
}

fn validate_jsonl_file(
    path: &str,
    text: &str,
    expected_row_count: Option<u64>,
    issues: &mut Vec<String>,
) {
    let mut row_count = 0_u64;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        row_count += 1;
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(serde_json::Value::Object(_)) => {}
            Ok(_) => issues.push(format!("{path} line {} must be a JSON object", index + 1)),
            Err(error) => issues.push(format!(
                "{path} line {} is not valid JSONL: {error}",
                index + 1
            )),
        }
    }
    if let Some(expected_row_count) = expected_row_count
        && row_count != expected_row_count
    {
        issues.push(format!(
            "{path} row_count mismatch: manifest {expected_row_count}, actual {row_count}"
        ));
    }
}

fn validate_csv_file(
    path: &str,
    text: &str,
    expected_row_count: Option<u64>,
    issues: &mut Vec<String>,
) -> bool {
    let issue_count = issues.len();
    let record_count = match count_csv_records(text) {
        Ok(record_count) => record_count,
        Err(error) => {
            issues.push(format!("{path} is not valid CSV: {error}"));
            return false;
        }
    };
    if record_count == 0 {
        issues.push(format!("{path} must include a header row"));
    }
    if let Some(expected_row_count) = expected_row_count {
        let data_row_count = record_count.saturating_sub(1) as u64;
        if data_row_count != expected_row_count {
            issues.push(format!(
                "{path} row_count mismatch: manifest {expected_row_count}, actual {data_row_count}"
            ));
        }
    }
    issues.len() == issue_count
}

fn count_csv_records(text: &str) -> Result<usize, String> {
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut record_count = 0_usize;
    let mut saw_record_content = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    saw_record_content = true;
                    at_field_start = false;
                } else {
                    in_quotes = false;
                    saw_record_content = true;
                    at_field_start = false;
                }
            }
            '"' if at_field_start => {
                in_quotes = true;
                saw_record_content = true;
                at_field_start = false;
            }
            '"' => return Err("unexpected quote in unquoted field".to_string()),
            ',' if !in_quotes => {
                saw_record_content = true;
                at_field_start = true;
            }
            '\n' if !in_quotes => {
                record_count += 1;
                saw_record_content = false;
                at_field_start = true;
            }
            '\r' if !in_quotes => {
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                record_count += 1;
                saw_record_content = false;
                at_field_start = true;
            }
            _ => {
                saw_record_content = true;
                at_field_start = false;
            }
        }
    }

    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    if saw_record_content {
        record_count += 1;
    }
    Ok(record_count)
}

fn read_jsonl_string_field_set(
    read_file: &mut impl FnMut(&str) -> GooseResult<String>,
    path: &str,
    field: &str,
    issues: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    let text = match read_file(path) {
        Ok(text) => text,
        Err(_error) => return None,
    };
    let mut values = BTreeSet::new();
    let mut present = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        let Some(field_value) = object.get(field) else {
            continue;
        };
        let Some(field_text) = field_value.as_str() else {
            issues.push(format!(
                "{path} line {} field {field} must be a string",
                index + 1
            ));
            continue;
        };
        present = true;
        if !values.insert(field_text.to_string()) {
            issues.push(format!("{path} duplicate {field}: {field_text}"));
        }
    }
    if present { Some(values) } else { None }
}

fn write_export_zip(
    bundle_dir: &Path,
    zip_output_path: &Path,
    manifest: &ExportManifest,
) -> GooseResult<()> {
    if let Some(parent) = zip_output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| GooseError::io(parent, source))?;
    }

    let zip_file =
        File::create(zip_output_path).map_err(|source| GooseError::io(zip_output_path, source))?;
    let mut writer = ZipWriter::new(zip_file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    write_zip_file(
        &mut writer,
        "manifest.json",
        &bundle_dir.join("manifest.json"),
        options,
    )?;
    for file in &manifest.files {
        validate_safe_relative_path(&file.path)?;
        write_zip_file(
            &mut writer,
            &file.path,
            &bundle_dir.join(&file.path),
            options,
        )?;
    }
    writer
        .finish()
        .map_err(|error| GooseError::message(format!("cannot finish zip bundle: {error}")))?;
    Ok(())
}

fn write_zip_file(
    writer: &mut ZipWriter<File>,
    archive_path: &str,
    source_path: &Path,
    options: SimpleFileOptions,
) -> GooseResult<()> {
    validate_safe_relative_path(archive_path)?;
    let bytes = fs::read(source_path).map_err(|source| GooseError::io(source_path, source))?;
    writer.start_file(archive_path, options).map_err(|error| {
        GooseError::message(format!("cannot add {archive_path} to zip: {error}"))
    })?;
    writer.write_all(&bytes).map_err(|error| {
        GooseError::message(format!("cannot write {archive_path} to zip: {error}"))
    })?;
    Ok(())
}

fn validate_manifest_file(base_dir: &Path, file: &ExportFileManifest) -> ExportFileValidation {
    let mut issues = Vec::new();

    if let Err(_error) = validate_safe_relative_path(&file.path) {
        issues.push("file path must be a safe relative path".to_string());
        let next_actions = export_validation_issue_actions(&file.path, &issues);
        return ExportFileValidation {
            path: file.path.clone(),
            expected_sha256: file.sha256.clone(),
            actual_sha256: None,
            pass: false,
            issues,
            next_actions,
        };
    }
    let relative = PathBuf::from(&file.path);

    let full_path = base_dir.join(relative);
    let actual_sha256 = match fs::read(&full_path) {
        Ok(bytes) => Some(sha256_hex(&bytes)),
        Err(source) => {
            issues.push(format!("cannot read file: {source}"));
            None
        }
    };

    if file.sha256.trim().is_empty() {
        issues.push("sha256 is required".to_string());
    } else if actual_sha256.as_deref() != Some(file.sha256.as_str()) {
        issues.push("sha256 mismatch".to_string());
    }

    let next_actions = export_validation_issue_actions(&file.path, &issues);
    ExportFileValidation {
        path: file.path.clone(),
        expected_sha256: file.sha256.clone(),
        actual_sha256,
        pass: issues.is_empty(),
        issues,
        next_actions,
    }
}

fn validate_zip_manifest_file(
    archive: &mut ZipArchive<File>,
    file: &ExportFileManifest,
) -> ExportFileValidation {
    let mut issues = Vec::new();
    if let Err(_error) = validate_safe_relative_path(&file.path) {
        issues.push("file path must be a safe relative path".to_string());
        let next_actions = export_validation_issue_actions(&file.path, &issues);
        return ExportFileValidation {
            path: file.path.clone(),
            expected_sha256: file.sha256.clone(),
            actual_sha256: None,
            pass: false,
            issues,
            next_actions,
        };
    }

    let actual_sha256 = match read_zip_entry(archive, &file.path) {
        Ok(bytes) => Some(sha256_hex(&bytes)),
        Err(error) => {
            issues.push(format!("cannot read file: {error}"));
            None
        }
    };

    if file.sha256.trim().is_empty() {
        issues.push("sha256 is required".to_string());
    } else if actual_sha256.as_deref() != Some(file.sha256.as_str()) {
        issues.push("sha256 mismatch".to_string());
    }

    let next_actions = export_validation_issue_actions(&file.path, &issues);
    ExportFileValidation {
        path: file.path.clone(),
        expected_sha256: file.sha256.clone(),
        actual_sha256,
        pass: issues.is_empty(),
        issues,
        next_actions,
    }
}

fn read_zip_entry_to_string(archive: &mut ZipArchive<File>, path: &str) -> GooseResult<String> {
    let bytes = read_zip_entry(archive, path)?;
    String::from_utf8(bytes)
        .map_err(|error| GooseError::message(format!("{path} is not valid UTF-8: {error}")))
}

fn read_zip_entry(archive: &mut ZipArchive<File>, path: &str) -> GooseResult<Vec<u8>> {
    validate_safe_relative_path(path)?;
    let mut entry = archive
        .by_name(path)
        .map_err(|error| GooseError::message(format!("{path}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| GooseError::message(format!("cannot read {path}: {error}")))?;
    Ok(bytes)
}

fn validate_safe_relative_path(path: &str) -> GooseResult<()> {
    let relative = PathBuf::from(path);
    if path.trim().is_empty()
        || relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(GooseError::message(
            "file path must be a safe relative path",
        ))
    } else {
        Ok(())
    }
}

fn export_validation_report_actions(
    issues: &[String],
    files: &[ExportFileValidation],
    content: &ExportContentValidation,
) -> Vec<ExportValidationNextAction> {
    let mut actions = BTreeSet::new();
    for file in files {
        actions.extend(file.next_actions.iter().cloned());
    }
    actions.extend(content.next_actions.iter().cloned());
    for issue in issues {
        if issue.ends_with("failed file validation") {
            continue;
        }
        actions.insert(export_validation_issue_action("report", issue));
    }
    actions.into_iter().collect()
}

fn export_validation_issue_actions(
    default_scope: &str,
    issues: &[String],
) -> Vec<ExportValidationNextAction> {
    issues
        .iter()
        .map(|issue| export_validation_issue_action(default_scope, issue))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn export_validation_issue_action(default_scope: &str, issue: &str) -> ExportValidationNextAction {
    let scope = export_validation_issue_scope(default_scope, issue);
    let issue_lower = issue.to_ascii_lowercase();
    let (reason, action) = if issue_lower.contains("sha256 mismatch")
        || issue_lower.contains("sha256 is required")
    {
        (
            "checksum_mismatch",
            "Regenerate the bundle or restore the file bytes that match the manifest checksum, then rerun export validation.",
        )
    } else if issue_lower.contains("row_count mismatch") {
        (
            "row_count_mismatch",
            "Regenerate the export after recounting JSONL rows for the selected timeframe, filters, and data families.",
        )
    } else if issue_lower.contains("must be a json object")
        || issue_lower.contains("not valid jsonl")
    {
        (
            "jsonl_shape",
            "Fix or regenerate the JSONL file so each non-empty line is one JSON object.",
        )
    } else if issue_lower.contains("cannot be re-imported as goose data") {
        (
            "typed_reimport_failed",
            "Regenerate the typed export row from Goose SQLite or update the typed parser/schema before revalidating.",
        )
    } else if issue_lower.contains("must be empty when include_raw_bytes is false") {
        (
            "raw_byte_redaction",
            "Re-export with raw-byte redaction applied, and keep the SQLite copy disabled for redacted bundles.",
        )
    } else if issue_lower
        .contains("sqlite data family cannot be exported when include_raw_bytes is false")
    {
        (
            "raw_byte_sqlite_policy",
            "Disable SQLite export or enable raw bytes; redacted exports cannot include a database copy.",
        )
    } else if issue_lower.contains("sqlite data family requires sqlite_source_path") {
        (
            "sqlite_source_path",
            "Provide the Goose SQLite source path or deselect the sqlite family before exporting.",
        )
    } else if issue_lower.contains("unknown family")
        || issue_lower.contains("empty family")
        || issue_lower.contains("duplicate family")
    {
        (
            "manifest_data_family",
            "Regenerate the export manifest with only supported Goose data families selected once.",
        )
    } else if issue_lower.contains("unselected data family") {
        (
            "unselected_data_family_artifact",
            "Regenerate the export so files and row counts exist only for selected Goose data families.",
        )
    } else if issue_lower.contains("not a recognized goose export artifact path") {
        (
            "unknown_manifest_file",
            "Regenerate the export so the manifest lists only canonical Goose artifact paths.",
        )
    } else if issue_lower.contains("official_labels_are_labels")
        || issue_lower.contains("official labels")
    {
        (
            "official_label_policy",
            "Mark official WHOOP comparison values as labels, not Goose outputs, and re-export calibration labels.",
        )
    } else if issue_lower.contains("missing from raw evidence")
        || issue_lower.contains("missing from decoded frame")
        || issue_lower.contains("missing from typed raw evidence")
        || issue_lower.contains("missing from typed decoded frame")
        || issue_lower.contains("missing from algorithm run export")
        || issue_lower.contains("missing from activity session export")
        || issue_lower.contains("missing from debug session export")
        || issue_lower.contains("missing from debug command export")
    {
        (
            "broken_export_reference",
            "Regenerate the export with linked raw evidence, decoded frames, timeline rows, samples, activity session rows, algorithm runs, and debug session/command rows for the same filters.",
        )
    } else if issue_lower.contains("cannot read file")
        || issue_lower.contains("cannot be inspected")
        || issue_lower.contains("cannot be re-imported:")
        || (issue_lower.contains("data family ") && issue_lower.contains(" requires "))
    {
        (
            "missing_export_file",
            "Regenerate the export so every selected data family has its required manifest entry and artifact file.",
        )
    } else if issue_lower.contains("safe relative path") {
        (
            "unsafe_manifest_path",
            "Regenerate the bundle with manifest file paths relative to the bundle root and without absolute or parent-directory segments.",
        )
    } else if issue_lower.contains("cannot be re-imported twice")
        || issue_lower.contains("duplicate ")
    {
        (
            "duplicate_identifier",
            "Deduplicate source rows or adjust stable ids before re-exporting.",
        )
    } else if issue_lower.contains("sha256 does not match payload_hex") {
        (
            "payload_checksum",
            "Regenerate raw evidence rows from stored payload bytes so payload_hex and sha256 agree.",
        )
    } else if issue_lower.contains("not valid hex") || issue_lower.contains("64-character sha256") {
        (
            "invalid_hex",
            "Regenerate the row with valid hex and checksum fields from Goose storage.",
        )
    } else if issue_lower.contains("is required for re-import") {
        (
            "required_field",
            "Regenerate the export from normalized Goose rows so required fields are populated.",
        )
    } else if issue_lower.contains("is not valid json")
        || issue_lower.contains("json invalid")
        || issue_lower.contains("json must be")
    {
        (
            "json_field",
            "Regenerate the row with valid JSON field content from Goose storage.",
        )
    } else if issue_lower.contains("sensor sample") {
        (
            "sensor_sample_shape",
            "Regenerate sensor sample rows from decoded packet summaries so raw value, unit, and provenance fields agree.",
        )
    } else if issue_lower.contains("algorithm run") {
        (
            "algorithm_run_shape",
            "Regenerate algorithm-run rows with structured output, quality flags, errors, and trusted score provenance before revalidating.",
        )
    } else if issue_lower.contains("metric value")
        || issue_lower.contains("metric component")
        || issue_lower.contains("metric feature report")
    {
        (
            "metric_export_shape",
            "Regenerate metric export rows from stored algorithm runs and feature reports before revalidating.",
        )
    } else if issue_lower.contains("calibration label") {
        (
            "calibration_label_shape",
            "Regenerate calibration label rows with finite values, non-empty provenance, and label-only official comparison policy.",
        )
    } else if issue_lower.contains("calibration run") {
        (
            "calibration_run_shape",
            "Regenerate calibration-run rows from passed date-split calibration reports with valid model parameters and readiness metrics.",
        )
    } else if issue_lower.contains("activity session")
        || issue_lower.contains("activity metric")
        || issue_lower.contains("activity interval")
        || issue_lower.contains("activity label")
    {
        (
            "activity_export_shape",
            "Regenerate activity session, metric, interval, and label rows from Goose storage so timestamps, enums, linkage, and provenance fields stay consistent.",
        )
    } else if issue_lower.contains("debug session")
        || issue_lower.contains("debug command")
        || issue_lower.contains("debug event")
    {
        (
            "debug_export_shape",
            "Regenerate debug export rows from the local loopback debug stream with redacted tokens, valid JSON payloads, and linked session/command references.",
        )
    } else if issue_lower.contains("command validation") {
        (
            "command_validation_shape",
            "Regenerate command-validation rows from Goose storage after importing trusted official-app command evidence.",
        )
    } else if issue_lower.contains("manifest.") {
        (
            "manifest_shape",
            "Regenerate the export manifest with required schema, version, timeframe, family, and file metadata.",
        )
    } else {
        (
            "export_validation_issue",
            "Resolve the export validation issue, regenerate the bundle if needed, and rerun export validation.",
        )
    };

    ExportValidationNextAction {
        scope,
        reason: reason.to_string(),
        action: action.to_string(),
    }
}

fn export_validation_issue_scope(default_scope: &str, issue: &str) -> String {
    let first = issue.split_whitespace().next().unwrap_or(default_scope);
    if first.starts_with("manifest.") {
        return "manifest".to_string();
    }
    if first.starts_with("data/") || first.ends_with(".jsonl") || first.ends_with(".csv") {
        return first.trim_end_matches(':').to_string();
    }
    default_scope.to_string()
}

fn report(
    path: &Path,
    manifest_valid: bool,
    files: Vec<ExportFileValidation>,
    content: ExportContentValidation,
    issues: Vec<String>,
) -> ExportValidationReport {
    let files_valid = files.iter().all(|file| file.pass);
    let content_valid = content.pass;
    let pass = manifest_valid && files_valid && content_valid && issues.is_empty();
    let next_actions = export_validation_report_actions(&issues, &files, &content);
    ExportValidationReport {
        schema: "goose.export-validation-report.v1".to_string(),
        generated_by: "goose-export-validator".to_string(),
        bundle_path: path.display().to_string(),
        manifest_valid,
        files_valid,
        content_valid,
        pass,
        files,
        content,
        issues,
        next_actions,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
