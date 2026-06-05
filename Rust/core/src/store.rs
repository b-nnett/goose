use std::{collections::BTreeSet, path::Path};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    GooseError, GooseResult,
    protocol::{DeviceType, ParsedFrame},
    validation_labels::OFFICIAL_WHOOP_LABEL_POLICY,
};

pub const CURRENT_SCHEMA_VERSION: i64 = 14;
pub const DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES: i64 = 512 * 1024 * 1024;

const ALLOWED_METRIC_SOURCE_KINDS: [&str; 4] = [
    "device_counter",
    "device_sensor",
    "local_estimate",
    "unavailable",
];

const ALLOWED_METRIC_PROVENANCE_SCOPES: [&str; 3] =
    ["daily_activity", "daily_recovery", "hourly_activity"];

const ALLOWED_ACTIVITY_SYNC_STATUSES: [&str; 6] = [
    "candidate",
    "verified",
    "user_confirmed",
    "synced",
    "blocked",
    "discarded",
];

const ALLOWED_ACTIVITY_TYPES: [&str; 48] = [
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

const ALLOWED_ACTIVITY_DETECTION_METHODS: [&str; 9] = [
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

const ALLOWED_ACTIVITY_INTERVAL_TYPES: [&str; 6] =
    ["lap", "pause", "work", "rest", "window", "split"];

const ALLOWED_ACTIVITY_LABEL_TYPES: [&str; 4] = [
    "user",
    "official_app_comparison",
    "calibration",
    "candidate",
];

const ALLOWED_ACTIVITY_METRIC_UNITS: [&str; 25] = [
    "raw", "bpm", "ms", "hz", "count", "steps", "m", "km", "mi", "kcal", "m/s", "km/h", "min", "s",
    "percent", "ratio", "load", "joule", "w", "kg", "m/s2", "c", "f", "degrees", "n/a",
];

const ALLOWED_EXTERNAL_SLEEP_PLATFORMS: [&str; 4] =
    ["healthkit", "health_connect", "manual", "import"];

const ALLOWED_EXTERNAL_SLEEP_STAGE_KINDS: [&str; 8] = [
    "in_bed",
    "asleep",
    "awake",
    "core",
    "deep",
    "rem",
    "unknown",
    "not_applicable",
];

const ALLOWED_EXTERNAL_SLEEP_STAGE_SUMMARY_KEYS: [&str; 21] = [
    "in_bed",
    "inbed",
    "unknown",
    "not_applicable",
    "not_applicable_sleep",
    "awake",
    "asleep_awake",
    "sleep_awake",
    "out_of_bed",
    "asleep",
    "asleep_unspecified",
    "core",
    "light",
    "asleep_core",
    "sleep_light",
    "deep",
    "asleep_deep",
    "sleep_deep",
    "rem",
    "asleep_rem",
    "sleep_rem",
];

const ALLOWED_SLEEP_CORRECTION_LABEL_TYPES: [&str; 5] = [
    "sleep_start",
    "sleep_end",
    "sleep_window",
    "sleep_stage",
    "nap",
];

#[derive(Debug)]
pub struct GooseStore {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct RawEvidenceInput<'a> {
    pub evidence_id: &'a str,
    pub source: &'a str,
    pub captured_at: &'a str,
    pub device_model: &'a str,
    pub payload: &'a [u8],
    pub sensitivity: &'a str,
    pub capture_session_id: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawEvidenceRow {
    pub evidence_id: String,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub payload_hex: String,
    pub sha256: String,
    pub sensitivity: String,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawEvidencePayloadRetentionReport {
    pub limit_bytes: i64,
    pub before_bytes: i64,
    pub after_bytes: i64,
    pub compacted_rows: i64,
    pub freed_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedFrameRow {
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub device_type: String,
    pub raw_len: i64,
    pub header_len: i64,
    pub declared_len: i64,
    pub payload_hex: String,
    pub payload_crc_hex: String,
    pub header_crc_valid: bool,
    pub payload_crc_valid: bool,
    pub packet_type: Option<i64>,
    pub packet_type_name: Option<String>,
    pub sequence: Option<i64>,
    pub command_or_event: Option<i64>,
    pub parsed_payload_json: String,
    pub parser_version: String,
    pub warnings_json: String,
}

#[derive(Debug, Clone)]
pub struct DecodedFrameInput<'a> {
    pub frame_id: &'a str,
    pub evidence_id: &'a str,
    pub parsed: &'a ParsedFrame,
    pub parser_version: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureSessionInput<'a> {
    pub session_id: &'a str,
    pub source: &'a str,
    pub started_at_unix_ms: i64,
    pub device_model: &'a str,
    pub active_device_id: Option<&'a str>,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureSessionRow {
    pub session_id: String,
    pub source: String,
    pub started_at_unix_ms: i64,
    pub ended_at_unix_ms: Option<i64>,
    pub device_model: String,
    pub active_device_id: Option<String>,
    pub status: String,
    pub frame_count: i64,
    pub provenance_json: String,
}

#[derive(Debug, Clone)]
pub struct OvernightSyncSessionInput<'a> {
    pub session_id: &'a str,
    pub started_at: &'a str,
    pub ended_at: Option<&'a str>,
    pub band_identifier: Option<&'a str>,
    pub app_version: Option<&'a str>,
    pub mode: &'a str,
    pub final_status: &'a str,
    pub raw_frame_count: i64,
    pub historical_frame_count: i64,
    pub k18_count: i64,
    pub k24_count: i64,
    pub k25_count: i64,
    pub k26_count: i64,
    pub packet47_count: i64,
    pub event17_count: i64,
    pub event29_count: i64,
    pub metadata49_count: i64,
    pub metadata56_count: i64,
    pub range_poll_count: i64,
    pub successful_range_poll_count: i64,
    pub event_log_count: i64,
    pub readiness_status: Option<&'a str>,
    pub readiness: Option<&'a str>,
    pub error_count: i64,
    pub notes: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct OvernightRawNotificationInput<'a> {
    pub session_id: &'a str,
    pub captured_at: &'a str,
    pub source: &'a str,
    pub device_id: Option<&'a str>,
    pub active_device_name: Option<&'a str>,
    pub connection_state: Option<&'a str>,
    pub service_uuid: Option<&'a str>,
    pub characteristic_uuid: &'a str,
    pub device_type: Option<&'a str>,
    pub command_or_event: Option<i64>,
    pub packet_type: Option<i64>,
    pub k_revision: Option<i64>,
    pub sequence: Option<i64>,
    pub frame_hex: &'a str,
    pub payload_hex: Option<&'a str>,
    pub byte_count: i64,
    pub decode_status: &'a str,
}

#[derive(Debug, Clone)]
pub struct OvernightHistoricalRangePollInput<'a> {
    pub session_id: &'a str,
    pub captured_at: &'a str,
    pub status: &'a str,
    pub command_sequence: i64,
    pub result_code: i64,
    pub result_name: &'a str,
    pub raw_payload_hex: &'a str,
    pub raw_body_hex: &'a str,
    pub revision_or_status: Option<i64>,
    pub page_current: Option<i64>,
    pub page_oldest: Option<i64>,
    pub page_end: Option<i64>,
    pub pages_behind: Option<i64>,
    pub pending_response_count: i64,
    pub retry_count: i64,
    pub notes: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OvernightMirrorReport {
    pub schema: String,
    pub session_upserted: usize,
    pub raw_inserted: usize,
    pub raw_existing: usize,
    pub historical_range_inserted: usize,
    pub historical_range_existing: usize,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OvernightMirrorCounts {
    pub schema: String,
    pub session_id: String,
    pub session_exists: bool,
    pub raw_notification_count: i64,
    pub historical_range_poll_count: i64,
    pub successful_historical_range_poll_count: i64,
}

#[derive(Debug, Clone)]
pub struct ActivitySessionInput<'a> {
    pub session_id: &'a str,
    pub source: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub activity_type: &'a str,
    pub external_activity_type_code: Option<&'a str>,
    pub external_activity_type_name: Option<&'a str>,
    pub custom_label: Option<&'a str>,
    pub confidence: f64,
    pub detection_method: &'a str,
    pub sync_status: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySessionRow {
    pub session_id: String,
    pub source: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub duration_ms: i64,
    pub activity_type: String,
    pub external_activity_type_code: Option<String>,
    pub external_activity_type_name: Option<String>,
    pub custom_label: Option<String>,
    pub confidence: f64,
    pub detection_method: String,
    pub sync_status: String,
    pub provenance_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ActivityMetricInput<'a> {
    pub metric_id: &'a str,
    pub activity_session_id: &'a str,
    pub metric_name: &'a str,
    pub value: f64,
    pub unit: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityMetricRow {
    pub metric_id: String,
    pub activity_session_id: String,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct DailyActivityMetricInput<'a> {
    pub daily_metric_id: &'a str,
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub steps: Option<i64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub average_cadence_spm: Option<f64>,
    pub source_kind: &'a str,
    pub confidence: f64,
    pub inputs_json: &'a str,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyActivityMetricRow {
    pub daily_metric_id: String,
    pub date_key: String,
    pub timezone: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub steps: Option<i64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub average_cadence_spm: Option<f64>,
    pub source_kind: String,
    pub confidence: f64,
    pub inputs_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct HourlyActivityMetricInput<'a> {
    pub hourly_metric_id: &'a str,
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub steps: Option<i64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub average_cadence_spm: Option<f64>,
    pub source_kind: &'a str,
    pub confidence: f64,
    pub inputs_json: &'a str,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HourlyActivityMetricRow {
    pub hourly_metric_id: String,
    pub date_key: String,
    pub timezone: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub steps: Option<i64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub average_cadence_spm: Option<f64>,
    pub source_kind: String,
    pub confidence: f64,
    pub inputs_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct DailyRecoveryMetricInput<'a> {
    pub daily_metric_id: &'a str,
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub resting_hr_bpm: Option<f64>,
    pub hrv_rmssd_ms: Option<f64>,
    pub respiratory_rate_rpm: Option<f64>,
    pub oxygen_saturation_percent: Option<f64>,
    pub skin_temperature_delta_c: Option<f64>,
    pub source_kind: &'a str,
    pub confidence: f64,
    pub inputs_json: &'a str,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyRecoveryMetricRow {
    pub daily_metric_id: String,
    pub date_key: String,
    pub timezone: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub resting_hr_bpm: Option<f64>,
    pub hrv_rmssd_ms: Option<f64>,
    pub respiratory_rate_rpm: Option<f64>,
    pub oxygen_saturation_percent: Option<f64>,
    pub skin_temperature_delta_c: Option<f64>,
    pub source_kind: String,
    pub confidence: f64,
    pub inputs_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct MetricProvenanceInput<'a> {
    pub provenance_id: &'a str,
    pub metric_scope: &'a str,
    pub metric_id: &'a str,
    pub source_kind: &'a str,
    pub source_detail: &'a str,
    pub confidence: Option<f64>,
    pub inputs_json: &'a str,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricProvenanceRow {
    pub provenance_id: String,
    pub metric_scope: String,
    pub metric_id: String,
    pub source_kind: String,
    pub source_detail: String,
    pub confidence: Option<f64>,
    pub inputs_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct MetricDebugFeatureInput<'a> {
    pub feature_id: &'a str,
    pub metric_family: &'a str,
    pub feature_name: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub source_kind: &'a str,
    pub confidence: Option<f64>,
    pub feature_json: &'a str,
    pub inputs_json: &'a str,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricDebugFeatureRow {
    pub feature_id: String,
    pub metric_family: String,
    pub feature_name: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub source_kind: String,
    pub confidence: Option<f64>,
    pub feature_json: String,
    pub inputs_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct StepCounterSampleInput<'a> {
    pub sample_id: &'a str,
    pub sample_time_unix_ms: i64,
    pub counter_value: i64,
    pub cadence_spm: Option<f64>,
    pub activity_state: Option<&'a str>,
    pub source_kind: &'a str,
    pub packet_family: &'a str,
    pub json_path: &'a str,
    pub frame_id: Option<&'a str>,
    pub evidence_id: Option<&'a str>,
    pub capture_session_id: Option<&'a str>,
    pub quality_flags_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepCounterSampleRow {
    pub sample_id: String,
    pub sample_time_unix_ms: i64,
    pub counter_value: i64,
    pub cadence_spm: Option<f64>,
    pub activity_state: Option<String>,
    pub source_kind: String,
    pub packet_family: String,
    pub json_path: String,
    pub frame_id: Option<String>,
    pub evidence_id: Option<String>,
    pub capture_session_id: Option<String>,
    pub quality_flags_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ActivityIntervalInput<'a> {
    pub interval_id: &'a str,
    pub activity_session_id: &'a str,
    pub interval_type: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub sequence: i64,
    pub metadata_json: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityIntervalRow {
    pub interval_id: String,
    pub activity_session_id: String,
    pub interval_type: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub duration_ms: i64,
    pub sequence: i64,
    pub metadata_json: String,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ActivityLabelInput<'a> {
    pub label_id: &'a str,
    pub activity_session_id: &'a str,
    pub label_type: &'a str,
    pub value: &'a str,
    pub source: &'a str,
    pub confidence: Option<f64>,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityLabelRow {
    pub label_id: String,
    pub activity_session_id: String,
    pub label_type: String,
    pub value: String,
    pub source: String,
    pub confidence: Option<f64>,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ExternalSleepSessionInput<'a> {
    pub sleep_id: &'a str,
    pub source: &'a str,
    pub platform: &'a str,
    pub platform_record_id: Option<&'a str>,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub timezone: Option<&'a str>,
    pub stage_summary_json: &'a str,
    pub confidence: f64,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalSleepSessionRow {
    pub sleep_id: String,
    pub source: String,
    pub platform: String,
    pub platform_record_id: Option<String>,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub duration_ms: i64,
    pub timezone: Option<String>,
    pub stage_summary_json: String,
    pub confidence: f64,
    pub provenance_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ExternalSleepStageInput<'a> {
    pub stage_id: &'a str,
    pub sleep_id: &'a str,
    pub stage_kind: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub confidence: f64,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalSleepStageRow {
    pub stage_id: String,
    pub sleep_id: String,
    pub stage_kind: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub duration_ms: i64,
    pub confidence: f64,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SleepCorrectionLabelInput<'a> {
    pub label_id: &'a str,
    pub sleep_id: Option<&'a str>,
    pub label_type: &'a str,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub value_json: &'a str,
    pub source: &'a str,
    pub confidence: Option<f64>,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepCorrectionLabelRow {
    pub label_id: String,
    pub sleep_id: Option<String>,
    pub label_type: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub value_json: String,
    pub source: String,
    pub confidence: Option<f64>,
    pub provenance_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlgorithmDefinitionRecord {
    pub algorithm_id: String,
    pub version: String,
    pub metric_family: String,
    pub display_name: String,
    pub implementation: String,
    pub license: String,
    pub input_schema: String,
    pub output_schema: String,
    pub input_requirements_json: String,
    pub params_json: String,
    pub quality_gates_json: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlgorithmRunRecord {
    pub run_id: String,
    pub algorithm_id: String,
    pub version: String,
    pub start_time: String,
    pub end_time: String,
    pub output_json: String,
    pub quality_flags_json: String,
    pub provenance_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricValueRecord {
    pub metric_value_id: String,
    pub run_id: String,
    pub metric_family: String,
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub start_time: String,
    pub end_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricComponentRecord {
    pub metric_component_id: String,
    pub run_id: String,
    pub component_name: String,
    pub value: f64,
    pub unit: String,
    pub contribution_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlgorithmPreferenceRecord {
    pub scope: String,
    pub metric_family: String,
    pub algorithm_id: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationRunTimes {
    pub train_start: String,
    pub train_end: String,
    pub holdout_start: String,
    pub holdout_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationRunRecord {
    pub calibration_run_id: String,
    pub algorithm_id: String,
    pub version: String,
    pub times: CalibrationRunTimes,
    pub metrics_json: String,
    pub params_json: String,
}

#[derive(Debug, Clone)]
pub struct CalibrationLabelInput<'a> {
    pub label_id: &'a str,
    pub metric_family: &'a str,
    pub label_source: &'a str,
    pub captured_at: &'a str,
    pub value: f64,
    pub unit: &'a str,
    pub provenance_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationLabelRow {
    pub label_id: String,
    pub metric_family: String,
    pub label_source: String,
    pub captured_at: String,
    pub value: f64,
    pub unit: String,
    pub provenance_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandValidationRecord {
    pub command: String,
    pub risk_gate: String,
    pub direct_send_ready: bool,
    pub report_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugSessionRow {
    pub session_id: String,
    pub started_at_unix_ms: i64,
    pub bridge_url: String,
    pub bind_host: String,
    pub token_required: bool,
    pub token_present: bool,
    pub remote_bind_enabled: bool,
    pub visible_remote_bind_toggle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugCommandRow {
    pub command_id: String,
    pub session_id: String,
    pub schema: String,
    pub command: String,
    pub args_json: String,
    pub dry_run: bool,
    pub received_at_unix_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugEventRow {
    pub session_id: String,
    pub sequence: i64,
    pub schema: String,
    pub time_unix_ms: i64,
    pub source: String,
    pub level: String,
    pub topic: String,
    pub message: String,
    pub command_id: Option<String>,
    pub data_json: String,
}

impl GooseStore {
    pub fn open(path: &Path) -> GooseResult<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_read_only(path: &Path) -> GooseResult<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> GooseResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn immediate_transaction<F, T>(&self, operation: F) -> GooseResult<T>
    where
        F: FnOnce(&GooseStore) -> GooseResult<T>,
    {
        self.conn.execute_batch("BEGIN IMMEDIATE TRANSACTION")?;
        match operation(self) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn migrate(&self) -> GooseResult<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS goose_schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS raw_evidence (
                evidence_id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                device_model TEXT NOT NULL,
                payload_hex TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                sensitivity TEXT NOT NULL,
                capture_session_id TEXT REFERENCES capture_sessions(session_id) ON DELETE SET NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS decoded_frames (
                frame_id TEXT PRIMARY KEY,
                evidence_id TEXT NOT NULL REFERENCES raw_evidence(evidence_id) ON DELETE CASCADE,
                device_type TEXT NOT NULL,
                raw_len INTEGER NOT NULL,
                header_len INTEGER NOT NULL,
                declared_len INTEGER NOT NULL,
                payload_hex TEXT NOT NULL,
                payload_crc_hex TEXT NOT NULL,
                header_crc_valid INTEGER NOT NULL,
                payload_crc_valid INTEGER NOT NULL,
                packet_type INTEGER,
                packet_type_name TEXT,
                sequence INTEGER,
                command_or_event INTEGER,
                parsed_payload_json TEXT NOT NULL DEFAULT 'null',
                parser_version TEXT NOT NULL,
                warnings_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS algorithm_definitions (
                algorithm_id TEXT NOT NULL,
                version TEXT NOT NULL,
                metric_family TEXT NOT NULL,
                display_name TEXT NOT NULL DEFAULT '',
                implementation TEXT NOT NULL DEFAULT '',
                license TEXT NOT NULL DEFAULT '',
                input_schema TEXT NOT NULL,
                output_schema TEXT NOT NULL,
                input_requirements_json TEXT NOT NULL DEFAULT '{}',
                params_json TEXT NOT NULL,
                quality_gates_json TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'experimental',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY (algorithm_id, version)
            );

            CREATE TABLE IF NOT EXISTS algorithm_runs (
                run_id TEXT PRIMARY KEY,
                algorithm_id TEXT NOT NULL,
                version TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                output_json TEXT NOT NULL,
                quality_flags_json TEXT NOT NULL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                FOREIGN KEY (algorithm_id, version)
                    REFERENCES algorithm_definitions(algorithm_id, version)
            );

            CREATE TABLE IF NOT EXISTS command_validation_records (
                command TEXT PRIMARY KEY,
                risk_gate TEXT NOT NULL,
                direct_send_ready INTEGER NOT NULL,
                report_json TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS capture_sessions (
                session_id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at_unix_ms INTEGER NOT NULL,
                ended_at_unix_ms INTEGER,
                device_model TEXT NOT NULL,
                active_device_id TEXT,
                status TEXT NOT NULL,
                frame_count INTEGER NOT NULL DEFAULT 0,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS activity_sessions (
                session_id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                activity_type TEXT NOT NULL,
                external_activity_type_code TEXT,
                external_activity_type_name TEXT,
                custom_label TEXT,
                confidence REAL NOT NULL,
                detection_method TEXT NOT NULL,
                sync_status TEXT NOT NULL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_activity_sessions_by_window
                ON activity_sessions(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_activity_sessions_by_type
                ON activity_sessions(activity_type);
            CREATE INDEX IF NOT EXISTS idx_activity_sessions_by_source
                ON activity_sessions(source);
            CREATE INDEX IF NOT EXISTS idx_activity_sessions_by_sync_status
                ON activity_sessions(sync_status);

            CREATE TABLE IF NOT EXISTS activity_metrics (
                metric_id TEXT PRIMARY KEY,
                activity_session_id TEXT NOT NULL REFERENCES activity_sessions(session_id) ON DELETE CASCADE,
                metric_name TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_activity_metrics_by_session
                ON activity_metrics(activity_session_id);
            CREATE INDEX IF NOT EXISTS idx_activity_metrics_by_name
                ON activity_metrics(metric_name);

            CREATE TABLE IF NOT EXISTS daily_activity_metrics (
                daily_metric_id TEXT PRIMARY KEY,
                date_key TEXT NOT NULL,
                timezone TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                steps INTEGER,
                active_kcal REAL,
                resting_kcal REAL,
                total_kcal REAL,
                average_cadence_spm REAL,
                source_kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_date
                ON daily_activity_metrics(date_key);
            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_window
                ON daily_activity_metrics(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_source_kind
                ON daily_activity_metrics(source_kind);

            CREATE TABLE IF NOT EXISTS hourly_activity_metrics (
                hourly_metric_id TEXT PRIMARY KEY,
                date_key TEXT NOT NULL,
                timezone TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                steps INTEGER,
                active_kcal REAL,
                resting_kcal REAL,
                total_kcal REAL,
                average_cadence_spm REAL,
                source_kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_hourly_activity_metrics_by_date
                ON hourly_activity_metrics(date_key);
            CREATE INDEX IF NOT EXISTS idx_hourly_activity_metrics_by_window
                ON hourly_activity_metrics(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_hourly_activity_metrics_by_source_kind
                ON hourly_activity_metrics(source_kind);

            CREATE TABLE IF NOT EXISTS daily_recovery_metrics (
                daily_metric_id TEXT PRIMARY KEY,
                date_key TEXT NOT NULL,
                timezone TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                resting_hr_bpm REAL,
                hrv_rmssd_ms REAL,
                respiratory_rate_rpm REAL,
                oxygen_saturation_percent REAL,
                skin_temperature_delta_c REAL,
                source_kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_date
                ON daily_recovery_metrics(date_key);
            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_window
                ON daily_recovery_metrics(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_source_kind
                ON daily_recovery_metrics(source_kind);

            CREATE TABLE IF NOT EXISTS metric_provenance (
                provenance_id TEXT PRIMARY KEY,
                metric_scope TEXT NOT NULL,
                metric_id TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_detail TEXT NOT NULL DEFAULT '',
                confidence REAL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_metric_provenance_by_metric
                ON metric_provenance(metric_scope, metric_id);
            CREATE INDEX IF NOT EXISTS idx_metric_provenance_by_source_kind
                ON metric_provenance(source_kind);

            CREATE TABLE IF NOT EXISTS metric_debug_features (
                feature_id TEXT PRIMARY KEY,
                metric_family TEXT NOT NULL,
                feature_name TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                source_kind TEXT NOT NULL,
                confidence REAL,
                feature_json TEXT NOT NULL DEFAULT '{}',
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_metric_debug_features_by_family
                ON metric_debug_features(metric_family, feature_name);
            CREATE INDEX IF NOT EXISTS idx_metric_debug_features_by_window
                ON metric_debug_features(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_metric_debug_features_by_source_kind
                ON metric_debug_features(source_kind);

            CREATE TABLE IF NOT EXISTS step_counter_samples (
                sample_id TEXT PRIMARY KEY,
                sample_time_unix_ms INTEGER NOT NULL,
                counter_value INTEGER NOT NULL,
                cadence_spm REAL,
                activity_state TEXT,
                source_kind TEXT NOT NULL,
                packet_family TEXT NOT NULL DEFAULT '',
                json_path TEXT NOT NULL DEFAULT '',
                frame_id TEXT,
                evidence_id TEXT,
                capture_session_id TEXT,
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_step_counter_samples_by_time
                ON step_counter_samples(sample_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_step_counter_samples_by_field
                ON step_counter_samples(packet_family, json_path, sample_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_step_counter_samples_by_source_kind
                ON step_counter_samples(source_kind);

            CREATE TABLE IF NOT EXISTS activity_intervals (
                interval_id TEXT PRIMARY KEY,
                activity_session_id TEXT NOT NULL REFERENCES activity_sessions(session_id) ON DELETE CASCADE,
                interval_type TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                sequence INTEGER NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_activity_intervals_by_session
                ON activity_intervals(activity_session_id);
            CREATE INDEX IF NOT EXISTS idx_activity_intervals_by_type
                ON activity_intervals(interval_type);

            CREATE TABLE IF NOT EXISTS activity_labels (
                label_id TEXT PRIMARY KEY,
                activity_session_id TEXT NOT NULL REFERENCES activity_sessions(session_id) ON DELETE CASCADE,
                label_type TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_activity_labels_by_session
                ON activity_labels(activity_session_id);
            CREATE INDEX IF NOT EXISTS idx_activity_labels_by_type
                ON activity_labels(label_type);

            CREATE TABLE IF NOT EXISTS external_sleep_sessions (
                sleep_id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                platform TEXT NOT NULL,
                platform_record_id TEXT,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                timezone TEXT,
                stage_summary_json TEXT NOT NULL DEFAULT '{}',
                confidence REAL NOT NULL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                UNIQUE(platform, platform_record_id)
            );

            CREATE INDEX IF NOT EXISTS idx_external_sleep_sessions_by_window
                ON external_sleep_sessions(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_external_sleep_sessions_by_platform
                ON external_sleep_sessions(platform);
            CREATE INDEX IF NOT EXISTS idx_external_sleep_sessions_by_source
                ON external_sleep_sessions(source);

            CREATE TABLE IF NOT EXISTS external_sleep_stages (
                stage_id TEXT PRIMARY KEY,
                sleep_id TEXT NOT NULL REFERENCES external_sleep_sessions(sleep_id) ON DELETE CASCADE,
                stage_kind TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                confidence REAL NOT NULL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_external_sleep_stages_by_sleep
                ON external_sleep_stages(sleep_id);
            CREATE INDEX IF NOT EXISTS idx_external_sleep_stages_by_window
                ON external_sleep_stages(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_external_sleep_stages_by_kind
                ON external_sleep_stages(stage_kind);

            CREATE TABLE IF NOT EXISTS sleep_correction_labels (
                label_id TEXT PRIMARY KEY,
                sleep_id TEXT,
                label_type TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                value_json TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_sleep_correction_labels_by_sleep
                ON sleep_correction_labels(sleep_id);
            CREATE INDEX IF NOT EXISTS idx_sleep_correction_labels_by_type
                ON sleep_correction_labels(label_type);
            CREATE INDEX IF NOT EXISTS idx_sleep_correction_labels_by_window
                ON sleep_correction_labels(start_time_unix_ms, end_time_unix_ms);

            CREATE TABLE IF NOT EXISTS metric_values (
                metric_value_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES algorithm_runs(run_id) ON DELETE CASCADE,
                metric_family TEXT NOT NULL,
                name TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS metric_components (
                metric_component_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES algorithm_runs(run_id) ON DELETE CASCADE,
                component_name TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                contribution_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS calibration_labels (
                label_id TEXT PRIMARY KEY,
                metric_family TEXT NOT NULL,
                label_source TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT NOT NULL,
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS calibration_runs (
                calibration_run_id TEXT PRIMARY KEY,
                algorithm_id TEXT NOT NULL,
                version TEXT NOT NULL,
                train_start TEXT NOT NULL,
                train_end TEXT NOT NULL,
                holdout_start TEXT NOT NULL,
                holdout_end TEXT NOT NULL,
                metrics_json TEXT NOT NULL,
                params_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                FOREIGN KEY (algorithm_id, version)
                    REFERENCES algorithm_definitions(algorithm_id, version)
            );

            CREATE TABLE IF NOT EXISTS algorithm_preferences (
                scope TEXT NOT NULL,
                metric_family TEXT NOT NULL,
                algorithm_id TEXT NOT NULL,
                version TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY (scope, metric_family),
                FOREIGN KEY (algorithm_id, version)
                    REFERENCES algorithm_definitions(algorithm_id, version)
            );

            CREATE TABLE IF NOT EXISTS debug_sessions (
                session_id TEXT PRIMARY KEY,
                started_at_unix_ms INTEGER NOT NULL,
                bridge_url TEXT NOT NULL,
                bind_host TEXT NOT NULL,
                token_required INTEGER NOT NULL,
                token_present INTEGER NOT NULL,
                remote_bind_enabled INTEGER NOT NULL,
                visible_remote_bind_toggle INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS debug_commands (
                command_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES debug_sessions(session_id) ON DELETE CASCADE,
                schema TEXT NOT NULL,
                command TEXT NOT NULL,
                args_json TEXT NOT NULL,
                dry_run INTEGER NOT NULL,
                received_at_unix_ms INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS debug_events (
                session_id TEXT NOT NULL REFERENCES debug_sessions(session_id) ON DELETE CASCADE,
                sequence INTEGER NOT NULL,
                schema TEXT NOT NULL,
                time_unix_ms INTEGER NOT NULL,
                source TEXT NOT NULL,
                level TEXT NOT NULL,
                topic TEXT NOT NULL,
                message TEXT NOT NULL,
                command_id TEXT REFERENCES debug_commands(command_id),
                data_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY (session_id, sequence)
            );

            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (1);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (2);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (3);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (4);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (5);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (6);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (7);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (8);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (9);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (10);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (11);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (12);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (13);
            INSERT OR IGNORE INTO goose_schema_migrations(version) VALUES (14);
            PRAGMA user_version = 14;
            "#,
        )?;
        self.ensure_raw_evidence_columns()?;
        self.ensure_decoded_frame_columns()?;
        self.ensure_algorithm_definition_columns()?;
        self.ensure_daily_activity_metric_multi_row_source_kind()?;
        self.ensure_daily_recovery_metric_multi_row_source_kind()?;
        self.ensure_step_counter_sample_columns()?;
        Ok(())
    }

    pub fn schema_version(&self) -> GooseResult<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn mirror_overnight_batch(
        &self,
        sessions: &[OvernightSyncSessionInput<'_>],
        raw_notifications: &[OvernightRawNotificationInput<'_>],
        historical_range_polls: &[OvernightHistoricalRangePollInput<'_>],
    ) -> GooseResult<OvernightMirrorReport> {
        self.immediate_transaction(|store| {
            store.ensure_overnight_mirror_tables()?;
            let mut report = OvernightMirrorReport {
                schema: "goose.overnight-mirror-report.v1".to_string(),
                session_upserted: 0,
                raw_inserted: 0,
                raw_existing: 0,
                historical_range_inserted: 0,
                historical_range_existing: 0,
                issues: Vec::new(),
            };

            for session in sessions {
                match store.upsert_overnight_sync_session(session) {
                    Ok(true) => report.session_upserted += 1,
                    Ok(false) => {}
                    Err(error) => report
                        .issues
                        .push(format!("session {} failed: {error}", session.session_id)),
                }
            }

            for notification in raw_notifications {
                match store.insert_overnight_raw_notification(notification) {
                    Ok(true) => report.raw_inserted += 1,
                    Ok(false) => report.raw_existing += 1,
                    Err(error) => report.issues.push(format!(
                        "raw notification {} {} failed: {error}",
                        notification.session_id, notification.captured_at
                    )),
                }
            }

            for poll in historical_range_polls {
                match store.insert_overnight_historical_range_poll(poll) {
                    Ok(true) => report.historical_range_inserted += 1,
                    Ok(false) => report.historical_range_existing += 1,
                    Err(error) => report.issues.push(format!(
                        "historical range {} {} seq {} failed: {error}",
                        poll.session_id, poll.captured_at, poll.command_sequence
                    )),
                }
            }

            Ok(report)
        })
    }

    pub fn overnight_mirror_counts(&self, session_id: &str) -> GooseResult<OvernightMirrorCounts> {
        validate_required("session_id", session_id)?;
        self.ensure_overnight_mirror_tables()?;
        let session_exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM overnight_sync_sessions WHERE session_id = ?1)",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )? != 0;
        let raw_notification_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM ble_raw_notifications WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        let historical_range_poll_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM historical_range_polls WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        let successful_historical_range_poll_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM historical_range_polls WHERE session_id = ?1 AND status = 'success'",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(OvernightMirrorCounts {
            schema: "goose.overnight-mirror-counts.v1".to_string(),
            session_id: session_id.to_string(),
            session_exists,
            raw_notification_count,
            historical_range_poll_count,
            successful_historical_range_poll_count,
        })
    }

    fn ensure_overnight_mirror_tables(&self) -> GooseResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS overnight_sync_sessions (
                session_id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                band_identifier TEXT,
                app_version TEXT,
                mode TEXT NOT NULL,
                final_status TEXT NOT NULL,
                raw_frame_count INTEGER NOT NULL DEFAULT 0,
                historical_frame_count INTEGER NOT NULL DEFAULT 0,
                k18_count INTEGER NOT NULL DEFAULT 0,
                k24_count INTEGER NOT NULL DEFAULT 0,
                k25_count INTEGER NOT NULL DEFAULT 0,
                k26_count INTEGER NOT NULL DEFAULT 0,
                packet47_count INTEGER NOT NULL DEFAULT 0,
                event17_count INTEGER NOT NULL DEFAULT 0,
                event29_count INTEGER NOT NULL DEFAULT 0,
                metadata49_count INTEGER NOT NULL DEFAULT 0,
                metadata56_count INTEGER NOT NULL DEFAULT 0,
                range_poll_count INTEGER NOT NULL DEFAULT 0,
                successful_range_poll_count INTEGER NOT NULL DEFAULT 0,
                event_log_count INTEGER NOT NULL DEFAULT 0,
                readiness_status TEXT,
                readiness TEXT,
                error_count INTEGER NOT NULL DEFAULT 0,
                notes TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS ble_raw_notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                source TEXT NOT NULL,
                device_id TEXT,
                active_device_name TEXT,
                connection_state TEXT,
                service_uuid TEXT,
                characteristic_uuid TEXT NOT NULL,
                device_type TEXT,
                command_or_event INTEGER,
                packet_type INTEGER,
                k_revision INTEGER,
                sequence INTEGER,
                frame_hex TEXT NOT NULL,
                payload_hex TEXT,
                byte_count INTEGER NOT NULL,
                sha256 TEXT NOT NULL,
                decode_status TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                UNIQUE(session_id, captured_at, characteristic_uuid, sha256)
            );

            CREATE INDEX IF NOT EXISTS idx_ble_raw_notifications_session_time
                ON ble_raw_notifications(session_id, captured_at);
            CREATE INDEX IF NOT EXISTS idx_ble_raw_notifications_packet_type
                ON ble_raw_notifications(packet_type);

            CREATE TABLE IF NOT EXISTS historical_range_polls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                status TEXT NOT NULL,
                command_sequence INTEGER NOT NULL,
                result_code INTEGER NOT NULL,
                result_name TEXT NOT NULL,
                raw_payload_hex TEXT NOT NULL,
                raw_body_hex TEXT NOT NULL,
                revision_or_status INTEGER,
                page_current INTEGER,
                page_oldest INTEGER,
                page_end INTEGER,
                pages_behind INTEGER,
                pending_response_count INTEGER NOT NULL DEFAULT 0,
                retry_count INTEGER NOT NULL DEFAULT 0,
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                UNIQUE(session_id, captured_at, command_sequence, status, result_code, raw_body_hex)
            );

            CREATE INDEX IF NOT EXISTS idx_historical_range_polls_session_time
                ON historical_range_polls(session_id, captured_at);
            CREATE INDEX IF NOT EXISTS idx_historical_range_polls_status
                ON historical_range_polls(status);
            "#,
        )?;
        Ok(())
    }

    fn upsert_overnight_sync_session(
        &self,
        input: &OvernightSyncSessionInput<'_>,
    ) -> GooseResult<bool> {
        validate_required("session_id", input.session_id)?;
        validate_required("started_at", input.started_at)?;
        validate_required("mode", input.mode)?;
        validate_required("final_status", input.final_status)?;
        validate_non_negative("raw_frame_count", input.raw_frame_count)?;
        validate_non_negative("historical_frame_count", input.historical_frame_count)?;
        validate_non_negative("range_poll_count", input.range_poll_count)?;
        validate_non_negative(
            "successful_range_poll_count",
            input.successful_range_poll_count,
        )?;
        validate_non_negative("event_log_count", input.event_log_count)?;

        let changed = self.conn.execute(
            r#"
            INSERT INTO overnight_sync_sessions (
                session_id,
                started_at,
                ended_at,
                band_identifier,
                app_version,
                mode,
                final_status,
                raw_frame_count,
                historical_frame_count,
                k18_count,
                k24_count,
                k25_count,
                k26_count,
                packet47_count,
                event17_count,
                event29_count,
                metadata49_count,
                metadata56_count,
                range_poll_count,
                successful_range_poll_count,
                event_log_count,
                readiness_status,
                readiness,
                error_count,
                notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
            ON CONFLICT(session_id) DO UPDATE SET
                ended_at = excluded.ended_at,
                band_identifier = excluded.band_identifier,
                app_version = excluded.app_version,
                mode = excluded.mode,
                final_status = excluded.final_status,
                raw_frame_count = excluded.raw_frame_count,
                historical_frame_count = excluded.historical_frame_count,
                k18_count = excluded.k18_count,
                k24_count = excluded.k24_count,
                k25_count = excluded.k25_count,
                k26_count = excluded.k26_count,
                packet47_count = excluded.packet47_count,
                event17_count = excluded.event17_count,
                event29_count = excluded.event29_count,
                metadata49_count = excluded.metadata49_count,
                metadata56_count = excluded.metadata56_count,
                range_poll_count = excluded.range_poll_count,
                successful_range_poll_count = excluded.successful_range_poll_count,
                event_log_count = excluded.event_log_count,
                readiness_status = excluded.readiness_status,
                readiness = excluded.readiness,
                error_count = excluded.error_count,
                notes = excluded.notes,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
            params![
                input.session_id,
                input.started_at,
                input.ended_at,
                input.band_identifier,
                input.app_version,
                input.mode,
                input.final_status,
                input.raw_frame_count,
                input.historical_frame_count,
                input.k18_count,
                input.k24_count,
                input.k25_count,
                input.k26_count,
                input.packet47_count,
                input.event17_count,
                input.event29_count,
                input.metadata49_count,
                input.metadata56_count,
                input.range_poll_count,
                input.successful_range_poll_count,
                input.event_log_count,
                input.readiness_status,
                input.readiness,
                input.error_count,
                input.notes,
            ],
        )?;
        Ok(changed > 0)
    }

    fn insert_overnight_raw_notification(
        &self,
        input: &OvernightRawNotificationInput<'_>,
    ) -> GooseResult<bool> {
        validate_required("session_id", input.session_id)?;
        validate_required("captured_at", input.captured_at)?;
        validate_required("source", input.source)?;
        validate_required("characteristic_uuid", input.characteristic_uuid)?;
        validate_required("frame_hex", input.frame_hex)?;
        validate_required("decode_status", input.decode_status)?;
        validate_non_negative("byte_count", input.byte_count)?;

        let payload = hex::decode(input.frame_hex).map_err(|error| {
            GooseError::message(format!("frame_hex is not valid hexadecimal: {error}"))
        })?;
        let sha256 = sha256_hex(&payload);

        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO ble_raw_notifications (
                session_id,
                captured_at,
                source,
                device_id,
                active_device_name,
                connection_state,
                service_uuid,
                characteristic_uuid,
                device_type,
                command_or_event,
                packet_type,
                k_revision,
                sequence,
                frame_hex,
                payload_hex,
                byte_count,
                sha256,
                decode_status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
            params![
                input.session_id,
                input.captured_at,
                input.source,
                input.device_id,
                input.active_device_name,
                input.connection_state,
                input.service_uuid,
                input.characteristic_uuid,
                input.device_type,
                input.command_or_event,
                input.packet_type,
                input.k_revision,
                input.sequence,
                input.frame_hex,
                input.payload_hex,
                input.byte_count,
                sha256,
                input.decode_status,
            ],
        )?;
        Ok(changed > 0)
    }

    fn insert_overnight_historical_range_poll(
        &self,
        input: &OvernightHistoricalRangePollInput<'_>,
    ) -> GooseResult<bool> {
        validate_required("session_id", input.session_id)?;
        validate_required("captured_at", input.captured_at)?;
        validate_required("status", input.status)?;
        validate_required("result_name", input.result_name)?;
        validate_required("raw_payload_hex", input.raw_payload_hex)?;
        validate_required("raw_body_hex", input.raw_body_hex)?;
        validate_non_negative("command_sequence", input.command_sequence)?;
        validate_non_negative("result_code", input.result_code)?;
        validate_non_negative("pending_response_count", input.pending_response_count)?;
        validate_non_negative("retry_count", input.retry_count)?;

        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO historical_range_polls (
                session_id,
                captured_at,
                status,
                command_sequence,
                result_code,
                result_name,
                raw_payload_hex,
                raw_body_hex,
                revision_or_status,
                page_current,
                page_oldest,
                page_end,
                pages_behind,
                pending_response_count,
                retry_count,
                notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
            params![
                input.session_id,
                input.captured_at,
                input.status,
                input.command_sequence,
                input.result_code,
                input.result_name,
                input.raw_payload_hex,
                input.raw_body_hex,
                input.revision_or_status,
                input.page_current,
                input.page_oldest,
                input.page_end,
                input.pages_behind,
                input.pending_response_count,
                input.retry_count,
                input.notes,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn insert_raw_evidence(&self, input: RawEvidenceInput<'_>) -> GooseResult<bool> {
        validate_required("evidence_id", input.evidence_id)?;
        validate_required("source", input.source)?;
        validate_required("captured_at", input.captured_at)?;
        validate_required("device_model", input.device_model)?;
        validate_required("sensitivity", input.sensitivity)?;
        if let Some(capture_session_id) = input.capture_session_id {
            validate_required("capture_session_id", capture_session_id)?;
        }

        let payload_hex = hex::encode(input.payload);
        let sha256 = sha256_hex(input.payload);

        let mut statement = self.conn.prepare_cached(
            r#"
            INSERT OR IGNORE INTO raw_evidence (
                evidence_id,
                source,
                captured_at,
                device_model,
                payload_hex,
                sha256,
                sensitivity,
                capture_session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )?;
        let changed = statement.execute(params![
            input.evidence_id,
            input.source,
            input.captured_at,
            input.device_model,
            payload_hex,
            sha256,
            input.sensitivity,
            input.capture_session_id
        ])?;
        if changed > 0 {
            return Ok(true);
        }

        let mut statement = self
            .conn
            .prepare_cached("SELECT sha256 FROM raw_evidence WHERE evidence_id = ?1")?;
        let existing_sha256: Option<String> = statement
            .query_row(params![input.evidence_id], |row| row.get(0))
            .optional()?;
        match existing_sha256 {
            Some(existing_sha256) if existing_sha256 == sha256 => Ok(false),
            Some(_) => Err(GooseError::message(format!(
                "raw evidence id {} already exists with a different checksum",
                input.evidence_id
            ))),
            None => Err(GooseError::message(format!(
                "raw evidence id {} insert was ignored but no existing row was found",
                input.evidence_id
            ))),
        }
    }

    pub fn insert_decoded_frame(&self, input: DecodedFrameInput<'_>) -> GooseResult<bool> {
        validate_required("frame_id", input.frame_id)?;
        validate_required("evidence_id", input.evidence_id)?;
        validate_required("parser_version", input.parser_version)?;

        let parsed_payload_json = serde_json::to_string(&input.parsed.parsed_payload)
            .map_err(|error| GooseError::message(error.to_string()))?;
        let warnings_json = serde_json::to_string(&input.parsed.warnings)
            .map_err(|error| GooseError::message(error.to_string()))?;

        let mut statement = self.conn.prepare_cached(
            r#"
            INSERT OR IGNORE INTO decoded_frames (
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            "#,
        )?;
        let changed = statement.execute(params![
            input.frame_id,
            input.evidence_id,
            device_type_name(input.parsed.device_type),
            input.parsed.raw_len as i64,
            input.parsed.header_len as i64,
            input.parsed.declared_len as i64,
            input.parsed.payload_hex,
            input.parsed.payload_crc_hex,
            bool_to_i64(input.parsed.header_crc_valid),
            bool_to_i64(input.parsed.payload_crc_valid),
            input.parsed.packet_type.map(i64::from),
            input.parsed.packet_type_name,
            input.parsed.sequence.map(i64::from),
            input.parsed.command_or_event.map(i64::from),
            parsed_payload_json,
            input.parser_version,
            warnings_json
        ])?;
        Ok(changed > 0)
    }

    pub fn start_capture_session(&self, input: CaptureSessionInput<'_>) -> GooseResult<bool> {
        validate_required("session_id", input.session_id)?;
        validate_required("source", input.source)?;
        validate_required("device_model", input.device_model)?;
        validate_json_object("provenance_json", input.provenance_json)?;
        validate_non_negative("started_at_unix_ms", input.started_at_unix_ms)?;

        if let Some(existing) = self.capture_session(input.session_id)? {
            let expected = CaptureSessionRow {
                session_id: input.session_id.to_string(),
                source: input.source.to_string(),
                started_at_unix_ms: input.started_at_unix_ms,
                ended_at_unix_ms: None,
                device_model: input.device_model.to_string(),
                active_device_id: input.active_device_id.map(str::to_string),
                status: "active".to_string(),
                frame_count: 0,
                provenance_json: input.provenance_json.to_string(),
            };
            if existing == expected {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "capture session {} already exists with different metadata",
                input.session_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO capture_sessions (
                session_id,
                source,
                started_at_unix_ms,
                ended_at_unix_ms,
                device_model,
                active_device_id,
                status,
                frame_count,
                provenance_json
            ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'active', 0, ?6)
            "#,
            params![
                input.session_id,
                input.source,
                input.started_at_unix_ms,
                input.device_model,
                input.active_device_id,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn finish_capture_session(
        &self,
        session_id: &str,
        ended_at_unix_ms: i64,
        frame_count: i64,
    ) -> GooseResult<CaptureSessionRow> {
        validate_required("session_id", session_id)?;
        validate_non_negative("ended_at_unix_ms", ended_at_unix_ms)?;
        validate_non_negative("frame_count", frame_count)?;
        let Some(existing) = self.capture_session(session_id)? else {
            return Err(GooseError::message(format!(
                "capture session {session_id} not found"
            )));
        };
        if ended_at_unix_ms < existing.started_at_unix_ms {
            return Err(GooseError::message(
                "ended_at_unix_ms must be greater than or equal to started_at_unix_ms",
            ));
        }

        self.conn.execute(
            r#"
            UPDATE capture_sessions
            SET ended_at_unix_ms = ?2,
                status = 'finished',
                frame_count = ?3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE session_id = ?1
            "#,
            params![session_id, ended_at_unix_ms, frame_count],
        )?;
        self.capture_session(session_id)?
            .ok_or_else(|| GooseError::message(format!("capture session {session_id} not found")))
    }

    pub fn capture_session(&self, session_id: &str) -> GooseResult<Option<CaptureSessionRow>> {
        validate_required("session_id", session_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    session_id,
                    source,
                    started_at_unix_ms,
                    ended_at_unix_ms,
                    device_model,
                    active_device_id,
                    status,
                    frame_count,
                    provenance_json
                FROM capture_sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                capture_session_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn capture_sessions_between(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> GooseResult<Vec<CaptureSessionRow>> {
        validate_non_negative("start_unix_ms", start_unix_ms)?;
        validate_non_negative("end_unix_ms", end_unix_ms)?;
        if end_unix_ms < start_unix_ms {
            return Err(GooseError::message(
                "end_unix_ms must be greater than or equal to start_unix_ms",
            ));
        }
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                started_at_unix_ms,
                ended_at_unix_ms,
                device_model,
                active_device_id,
                status,
                frame_count,
                provenance_json
            FROM capture_sessions
            WHERE started_at_unix_ms < ?2
              AND COALESCE(ended_at_unix_ms, started_at_unix_ms) >= ?1
            ORDER BY started_at_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_unix_ms, end_unix_ms],
            capture_session_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_activity_session(&self, input: ActivitySessionInput<'_>) -> GooseResult<bool> {
        validate_activity_session_input(self, &input)?;

        if let Some(existing) = self.activity_session(input.session_id)? {
            let same = existing.session_id == input.session_id
                && existing.source == input.source
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.activity_type == input.activity_type
                && existing.external_activity_type_code
                    == input.external_activity_type_code.map(str::to_string)
                && existing.external_activity_type_name
                    == input.external_activity_type_name.map(str::to_string)
                && existing.custom_label == input.custom_label.map(str::to_string)
                && existing.confidence == input.confidence
                && existing.detection_method == input.detection_method
                && existing.sync_status == input.sync_status
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "activity session {} already exists with different metadata",
                input.session_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO activity_sessions (
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                input.session_id,
                input.source,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.end_time_unix_ms - input.start_time_unix_ms,
                input.activity_type,
                input.external_activity_type_code,
                input.external_activity_type_name,
                input.custom_label,
                input.confidence,
                input.detection_method,
                input.sync_status,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn update_activity_session(&self, input: ActivitySessionInput<'_>) -> GooseResult<bool> {
        validate_activity_session_input(self, &input)?;
        let Some(existing) = self.activity_session(input.session_id)? else {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                input.session_id
            )));
        };

        let same = existing.session_id == input.session_id
            && existing.source == input.source
            && existing.start_time_unix_ms == input.start_time_unix_ms
            && existing.end_time_unix_ms == input.end_time_unix_ms
            && existing.activity_type == input.activity_type
            && existing.external_activity_type_code
                == input.external_activity_type_code.map(str::to_string)
            && existing.external_activity_type_name
                == input.external_activity_type_name.map(str::to_string)
            && existing.custom_label == input.custom_label.map(str::to_string)
            && existing.confidence == input.confidence
            && existing.detection_method == input.detection_method
            && existing.sync_status == input.sync_status
            && existing.provenance_json == input.provenance_json;
        if same {
            return Ok(false);
        }

        let changed = self.conn.execute(
            r#"
            UPDATE activity_sessions
            SET source = ?2,
                start_time_unix_ms = ?3,
                end_time_unix_ms = ?4,
                duration_ms = ?5,
                activity_type = ?6,
                external_activity_type_code = ?7,
                external_activity_type_name = ?8,
                custom_label = ?9,
                confidence = ?10,
                detection_method = ?11,
                sync_status = ?12,
                provenance_json = ?13,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE session_id = ?1
            "#,
            params![
                input.session_id,
                input.source,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.end_time_unix_ms - input.start_time_unix_ms,
                input.activity_type,
                input.external_activity_type_code,
                input.external_activity_type_name,
                input.custom_label,
                input.confidence,
                input.detection_method,
                input.sync_status,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_activity_session(&self, session_id: &str) -> GooseResult<bool> {
        validate_required("session_id", session_id)?;
        let changed = self.conn.execute(
            "DELETE FROM activity_sessions WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(changed > 0)
    }

    pub fn activity_session(&self, session_id: &str) -> GooseResult<Option<ActivitySessionRow>> {
        validate_required("session_id", session_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    session_id,
                    source,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    duration_ms,
                    activity_type,
                    external_activity_type_code,
                    external_activity_type_name,
                    custom_label,
                    confidence,
                    detection_method,
                    sync_status,
                    provenance_json,
                    created_at,
                    updated_at
                FROM activity_sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                activity_session_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            activity_session_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_type(
        &self,
        activity_type: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("activity_type", activity_type)?;
        validate_activity_type(activity_type)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE activity_type = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(params![activity_type], activity_session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_source(
        &self,
        source: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("source", source)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE source = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(params![source], activity_session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_sync_status(
        &self,
        sync_status: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("sync_status", sync_status)?;
        validate_sync_status(sync_status)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE sync_status = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(params![sync_status], activity_session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_custom_label(
        &self,
        custom_label: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("custom_label", custom_label)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE custom_label = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(params![custom_label], activity_session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_external_activity_type_code(
        &self,
        external_activity_type_code: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("external_activity_type_code", external_activity_type_code)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE external_activity_type_code = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(
            params![external_activity_type_code],
            activity_session_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_sessions_by_external_activity_type_name(
        &self,
        external_activity_type_name: &str,
    ) -> GooseResult<Vec<ActivitySessionRow>> {
        validate_required("external_activity_type_name", external_activity_type_name)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                source,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                activity_type,
                external_activity_type_code,
                external_activity_type_name,
                custom_label,
                confidence,
                detection_method,
                sync_status,
                provenance_json,
                created_at,
                updated_at
            FROM activity_sessions
            WHERE external_activity_type_name = ?1
            ORDER BY start_time_unix_ms, session_id
            "#,
        )?;
        let rows = statement.query_map(
            params![external_activity_type_name],
            activity_session_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_activity_metric(&self, input: ActivityMetricInput<'_>) -> GooseResult<bool> {
        validate_activity_metric_input(self, &input)?;
        if self.activity_session(input.activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                input.activity_session_id
            )));
        }
        self.insert_activity_metric_without_session_check(&input)
    }

    pub fn insert_activity_metrics(
        &self,
        inputs: &[ActivityMetricInput<'_>],
    ) -> GooseResult<(usize, usize)> {
        let mut session_ids = BTreeSet::new();
        for input in inputs {
            validate_activity_metric_input(self, input)?;
            session_ids.insert(input.activity_session_id);
        }

        for session_id in session_ids {
            if self.activity_session(session_id)?.is_none() {
                return Err(GooseError::message(format!(
                    "activity session {} not found",
                    session_id
                )));
            }
        }

        let mut inserted = 0;
        let mut existing = 0;
        for input in inputs {
            if self.insert_activity_metric_without_session_check(input)? {
                inserted += 1;
            } else {
                existing += 1;
            }
        }
        Ok((inserted, existing))
    }

    fn insert_activity_metric_without_session_check(
        &self,
        input: &ActivityMetricInput<'_>,
    ) -> GooseResult<bool> {
        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO activity_metrics (
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                input.metric_id,
                input.activity_session_id,
                input.metric_name,
                input.value,
                input.unit,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        if changed > 0 {
            return Ok(true);
        }

        if let Some(existing) = self.activity_metric(input.metric_id)? {
            if existing.activity_session_id == input.activity_session_id
                && existing.metric_name == input.metric_name
                && existing.value == input.value
                && existing.unit == input.unit
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json
            {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "activity metric {} already exists with different metadata",
                input.metric_id
            )));
        }

        Err(GooseError::message(format!(
            "activity metric {} insert was ignored but no existing row was found",
            input.metric_id
        )))
    }

    pub fn activity_metric(&self, metric_id: &str) -> GooseResult<Option<ActivityMetricRow>> {
        validate_required("metric_id", metric_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    metric_id,
                    activity_session_id,
                    metric_name,
                    value,
                    unit,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    quality_flags_json,
                    provenance_json,
                    created_at
                FROM activity_metrics
                WHERE metric_id = ?1
                "#,
                params![metric_id],
                activity_metric_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn activity_metrics_for_session(
        &self,
        activity_session_id: &str,
    ) -> GooseResult<Vec<ActivityMetricRow>> {
        validate_required("activity_session_id", activity_session_id)?;
        if self.activity_session(activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                activity_session_id
            )));
        }
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json,
                created_at
            FROM activity_metrics
            WHERE activity_session_id = ?1
            ORDER BY start_time_unix_ms, metric_id
            "#,
        )?;
        let rows = statement.query_map(params![activity_session_id], activity_metric_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_metrics_for_sessions(
        &self,
        activity_session_ids: &[String],
    ) -> GooseResult<Vec<ActivityMetricRow>> {
        if activity_session_ids.is_empty() {
            return Ok(Vec::new());
        }
        for activity_session_id in activity_session_ids {
            validate_required("activity_session_id", activity_session_id)?;
        }

        let placeholders = (0..activity_session_ids.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json,
                created_at
            FROM activity_metrics
            WHERE activity_session_id IN ({placeholders})
            ORDER BY activity_session_id, start_time_unix_ms, metric_id
            "#
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            params_from_iter(activity_session_ids.iter().map(String::as_str)),
            activity_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_metrics_by_name(
        &self,
        metric_name: &str,
    ) -> GooseResult<Vec<ActivityMetricRow>> {
        validate_required("metric_name", metric_name)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json,
                created_at
            FROM activity_metrics
            WHERE metric_name = ?1
            ORDER BY start_time_unix_ms, metric_id
            "#,
        )?;
        let rows = statement.query_map(params![metric_name], activity_metric_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_metrics_for_session_in_window(
        &self,
        activity_session_id: &str,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<ActivityMetricRow>> {
        validate_required("activity_session_id", activity_session_id)?;
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        if self.activity_session(activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                activity_session_id
            )));
        }
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json,
                created_at
            FROM activity_metrics
            WHERE activity_session_id = ?1
              AND start_time_unix_ms < ?3
              AND end_time_unix_ms > ?2
            ORDER BY start_time_unix_ms, metric_id
            "#,
        )?;
        let rows = statement.query_map(
            params![activity_session_id, start_time_unix_ms, end_time_unix_ms],
            activity_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_metrics_in_window(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<ActivityMetricRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_id,
                activity_session_id,
                metric_name,
                value,
                unit,
                start_time_unix_ms,
                end_time_unix_ms,
                quality_flags_json,
                provenance_json,
                created_at
            FROM activity_metrics
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, metric_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            activity_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_daily_activity_metric(
        &self,
        input: DailyActivityMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_daily_activity_metric_input(&input)?;
        if let Some(existing) = self.daily_activity_metric(input.daily_metric_id)? {
            let same = existing.date_key == input.date_key
                && existing.timezone == input.timezone
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.steps == input.steps
                && existing.active_kcal == input.active_kcal
                && existing.resting_kcal == input.resting_kcal
                && existing.total_kcal == input.total_kcal
                && existing.average_cadence_spm == input.average_cadence_spm
                && existing.source_kind == input.source_kind
                && existing.confidence == input.confidence
                && existing.inputs_json == input.inputs_json
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "daily activity metric {} already exists with different metadata",
                input.daily_metric_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO daily_activity_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            "#,
            params![
                input.daily_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.steps,
                input.active_kcal,
                input.resting_kcal,
                input.total_kcal,
                input.average_cadence_spm,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn upsert_daily_activity_metric(
        &self,
        input: DailyActivityMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_daily_activity_metric_input(&input)?;
        let changed = self.conn.execute(
            r#"
            INSERT INTO daily_activity_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(daily_metric_id) DO UPDATE SET
                date_key = excluded.date_key,
                timezone = excluded.timezone,
                start_time_unix_ms = excluded.start_time_unix_ms,
                end_time_unix_ms = excluded.end_time_unix_ms,
                steps = excluded.steps,
                active_kcal = excluded.active_kcal,
                resting_kcal = excluded.resting_kcal,
                total_kcal = excluded.total_kcal,
                average_cadence_spm = excluded.average_cadence_spm,
                source_kind = excluded.source_kind,
                confidence = excluded.confidence,
                inputs_json = excluded.inputs_json,
                quality_flags_json = excluded.quality_flags_json,
                provenance_json = excluded.provenance_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE daily_activity_metrics.date_key IS NOT excluded.date_key
               OR daily_activity_metrics.timezone IS NOT excluded.timezone
               OR daily_activity_metrics.start_time_unix_ms IS NOT excluded.start_time_unix_ms
               OR daily_activity_metrics.end_time_unix_ms IS NOT excluded.end_time_unix_ms
               OR daily_activity_metrics.steps IS NOT excluded.steps
               OR daily_activity_metrics.active_kcal IS NOT excluded.active_kcal
               OR daily_activity_metrics.resting_kcal IS NOT excluded.resting_kcal
               OR daily_activity_metrics.total_kcal IS NOT excluded.total_kcal
               OR daily_activity_metrics.average_cadence_spm IS NOT excluded.average_cadence_spm
               OR daily_activity_metrics.source_kind IS NOT excluded.source_kind
               OR daily_activity_metrics.confidence IS NOT excluded.confidence
               OR daily_activity_metrics.inputs_json IS NOT excluded.inputs_json
               OR daily_activity_metrics.quality_flags_json IS NOT excluded.quality_flags_json
               OR daily_activity_metrics.provenance_json IS NOT excluded.provenance_json
            "#,
            params![
                input.daily_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.steps,
                input.active_kcal,
                input.resting_kcal,
                input.total_kcal,
                input.average_cadence_spm,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn daily_activity_metric(
        &self,
        daily_metric_id: &str,
    ) -> GooseResult<Option<DailyActivityMetricRow>> {
        validate_required("daily_metric_id", daily_metric_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    daily_metric_id,
                    date_key,
                    timezone,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    steps,
                    active_kcal,
                    resting_kcal,
                    total_kcal,
                    average_cadence_spm,
                    source_kind,
                    confidence,
                    inputs_json,
                    quality_flags_json,
                    provenance_json,
                    created_at,
                    updated_at
                FROM daily_activity_metrics
                WHERE daily_metric_id = ?1
                "#,
                params![daily_metric_id],
                daily_activity_metric_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn daily_activity_metrics_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<DailyActivityMetricRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            FROM daily_activity_metrics
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, daily_metric_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            daily_activity_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_hourly_activity_metric(
        &self,
        input: HourlyActivityMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_hourly_activity_metric_input(&input)?;
        if let Some(existing) = self.hourly_activity_metric(input.hourly_metric_id)? {
            let same = existing.date_key == input.date_key
                && existing.timezone == input.timezone
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.steps == input.steps
                && existing.active_kcal == input.active_kcal
                && existing.resting_kcal == input.resting_kcal
                && existing.total_kcal == input.total_kcal
                && existing.average_cadence_spm == input.average_cadence_spm
                && existing.source_kind == input.source_kind
                && existing.confidence == input.confidence
                && existing.inputs_json == input.inputs_json
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "hourly activity metric {} already exists with different metadata",
                input.hourly_metric_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO hourly_activity_metrics (
                hourly_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            "#,
            params![
                input.hourly_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.steps,
                input.active_kcal,
                input.resting_kcal,
                input.total_kcal,
                input.average_cadence_spm,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn upsert_hourly_activity_metric(
        &self,
        input: HourlyActivityMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_hourly_activity_metric_input(&input)?;
        let changed = self.conn.execute(
            r#"
            INSERT INTO hourly_activity_metrics (
                hourly_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(hourly_metric_id) DO UPDATE SET
                date_key = excluded.date_key,
                timezone = excluded.timezone,
                start_time_unix_ms = excluded.start_time_unix_ms,
                end_time_unix_ms = excluded.end_time_unix_ms,
                steps = excluded.steps,
                active_kcal = excluded.active_kcal,
                resting_kcal = excluded.resting_kcal,
                total_kcal = excluded.total_kcal,
                average_cadence_spm = excluded.average_cadence_spm,
                source_kind = excluded.source_kind,
                confidence = excluded.confidence,
                inputs_json = excluded.inputs_json,
                quality_flags_json = excluded.quality_flags_json,
                provenance_json = excluded.provenance_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE hourly_activity_metrics.date_key IS NOT excluded.date_key
               OR hourly_activity_metrics.timezone IS NOT excluded.timezone
               OR hourly_activity_metrics.start_time_unix_ms IS NOT excluded.start_time_unix_ms
               OR hourly_activity_metrics.end_time_unix_ms IS NOT excluded.end_time_unix_ms
               OR hourly_activity_metrics.steps IS NOT excluded.steps
               OR hourly_activity_metrics.active_kcal IS NOT excluded.active_kcal
               OR hourly_activity_metrics.resting_kcal IS NOT excluded.resting_kcal
               OR hourly_activity_metrics.total_kcal IS NOT excluded.total_kcal
               OR hourly_activity_metrics.average_cadence_spm IS NOT excluded.average_cadence_spm
               OR hourly_activity_metrics.source_kind IS NOT excluded.source_kind
               OR hourly_activity_metrics.confidence IS NOT excluded.confidence
               OR hourly_activity_metrics.inputs_json IS NOT excluded.inputs_json
               OR hourly_activity_metrics.quality_flags_json IS NOT excluded.quality_flags_json
               OR hourly_activity_metrics.provenance_json IS NOT excluded.provenance_json
            "#,
            params![
                input.hourly_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.steps,
                input.active_kcal,
                input.resting_kcal,
                input.total_kcal,
                input.average_cadence_spm,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn hourly_activity_metric(
        &self,
        hourly_metric_id: &str,
    ) -> GooseResult<Option<HourlyActivityMetricRow>> {
        validate_required("hourly_metric_id", hourly_metric_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    hourly_metric_id,
                    date_key,
                    timezone,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    steps,
                    active_kcal,
                    resting_kcal,
                    total_kcal,
                    average_cadence_spm,
                    source_kind,
                    confidence,
                    inputs_json,
                    quality_flags_json,
                    provenance_json,
                    created_at,
                    updated_at
                FROM hourly_activity_metrics
                WHERE hourly_metric_id = ?1
                "#,
                params![hourly_metric_id],
                hourly_activity_metric_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn hourly_activity_metrics_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<HourlyActivityMetricRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                hourly_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            FROM hourly_activity_metrics
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, hourly_metric_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            hourly_activity_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_daily_recovery_metric(
        &self,
        input: DailyRecoveryMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_daily_recovery_metric_input(&input)?;
        if let Some(existing) = self.daily_recovery_metric(input.daily_metric_id)? {
            let same = existing.date_key == input.date_key
                && existing.timezone == input.timezone
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.resting_hr_bpm == input.resting_hr_bpm
                && existing.hrv_rmssd_ms == input.hrv_rmssd_ms
                && existing.respiratory_rate_rpm == input.respiratory_rate_rpm
                && existing.oxygen_saturation_percent == input.oxygen_saturation_percent
                && existing.skin_temperature_delta_c == input.skin_temperature_delta_c
                && existing.source_kind == input.source_kind
                && existing.confidence == input.confidence
                && existing.inputs_json == input.inputs_json
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "daily recovery metric {} already exists with different metadata",
                input.daily_metric_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO daily_recovery_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                resting_hr_bpm,
                hrv_rmssd_ms,
                respiratory_rate_rpm,
                oxygen_saturation_percent,
                skin_temperature_delta_c,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            "#,
            params![
                input.daily_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.resting_hr_bpm,
                input.hrv_rmssd_ms,
                input.respiratory_rate_rpm,
                input.oxygen_saturation_percent,
                input.skin_temperature_delta_c,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn upsert_daily_recovery_metric(
        &self,
        input: DailyRecoveryMetricInput<'_>,
    ) -> GooseResult<bool> {
        validate_daily_recovery_metric_input(&input)?;
        let changed = self.conn.execute(
            r#"
            INSERT INTO daily_recovery_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                resting_hr_bpm,
                hrv_rmssd_ms,
                respiratory_rate_rpm,
                oxygen_saturation_percent,
                skin_temperature_delta_c,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(daily_metric_id) DO UPDATE SET
                date_key = excluded.date_key,
                timezone = excluded.timezone,
                start_time_unix_ms = excluded.start_time_unix_ms,
                end_time_unix_ms = excluded.end_time_unix_ms,
                resting_hr_bpm = excluded.resting_hr_bpm,
                hrv_rmssd_ms = excluded.hrv_rmssd_ms,
                respiratory_rate_rpm = excluded.respiratory_rate_rpm,
                oxygen_saturation_percent = excluded.oxygen_saturation_percent,
                skin_temperature_delta_c = excluded.skin_temperature_delta_c,
                source_kind = excluded.source_kind,
                confidence = excluded.confidence,
                inputs_json = excluded.inputs_json,
                quality_flags_json = excluded.quality_flags_json,
                provenance_json = excluded.provenance_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE daily_recovery_metrics.date_key IS NOT excluded.date_key
               OR daily_recovery_metrics.timezone IS NOT excluded.timezone
               OR daily_recovery_metrics.start_time_unix_ms IS NOT excluded.start_time_unix_ms
               OR daily_recovery_metrics.end_time_unix_ms IS NOT excluded.end_time_unix_ms
               OR daily_recovery_metrics.resting_hr_bpm IS NOT excluded.resting_hr_bpm
               OR daily_recovery_metrics.hrv_rmssd_ms IS NOT excluded.hrv_rmssd_ms
               OR daily_recovery_metrics.respiratory_rate_rpm IS NOT excluded.respiratory_rate_rpm
               OR daily_recovery_metrics.oxygen_saturation_percent IS NOT excluded.oxygen_saturation_percent
               OR daily_recovery_metrics.skin_temperature_delta_c IS NOT excluded.skin_temperature_delta_c
               OR daily_recovery_metrics.source_kind IS NOT excluded.source_kind
               OR daily_recovery_metrics.confidence IS NOT excluded.confidence
               OR daily_recovery_metrics.inputs_json IS NOT excluded.inputs_json
               OR daily_recovery_metrics.quality_flags_json IS NOT excluded.quality_flags_json
               OR daily_recovery_metrics.provenance_json IS NOT excluded.provenance_json
            "#,
            params![
                input.daily_metric_id,
                input.date_key,
                input.timezone,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.resting_hr_bpm,
                input.hrv_rmssd_ms,
                input.respiratory_rate_rpm,
                input.oxygen_saturation_percent,
                input.skin_temperature_delta_c,
                input.source_kind,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn daily_recovery_metric(
        &self,
        daily_metric_id: &str,
    ) -> GooseResult<Option<DailyRecoveryMetricRow>> {
        validate_required("daily_metric_id", daily_metric_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    daily_metric_id,
                    date_key,
                    timezone,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    resting_hr_bpm,
                    hrv_rmssd_ms,
                    respiratory_rate_rpm,
                    oxygen_saturation_percent,
                    skin_temperature_delta_c,
                    source_kind,
                    confidence,
                    inputs_json,
                    quality_flags_json,
                    provenance_json,
                    created_at,
                    updated_at
                FROM daily_recovery_metrics
                WHERE daily_metric_id = ?1
                "#,
                params![daily_metric_id],
                daily_recovery_metric_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn daily_recovery_metrics_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<DailyRecoveryMetricRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                resting_hr_bpm,
                hrv_rmssd_ms,
                respiratory_rate_rpm,
                oxygen_saturation_percent,
                skin_temperature_delta_c,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            FROM daily_recovery_metrics
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, daily_metric_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            daily_recovery_metric_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_metric_provenance(&self, input: MetricProvenanceInput<'_>) -> GooseResult<bool> {
        validate_metric_provenance_input(self, &input)?;
        if let Some(existing) = self.metric_provenance(input.provenance_id)? {
            let same = existing.metric_scope == input.metric_scope
                && existing.metric_id == input.metric_id
                && existing.source_kind == input.source_kind
                && existing.source_detail == input.source_detail
                && existing.confidence == input.confidence
                && existing.inputs_json == input.inputs_json
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "metric provenance {} already exists with different metadata",
                input.provenance_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO metric_provenance (
                provenance_id,
                metric_scope,
                metric_id,
                source_kind,
                source_detail,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                input.provenance_id,
                input.metric_scope,
                input.metric_id,
                input.source_kind,
                input.source_detail,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn upsert_metric_provenance(&self, input: MetricProvenanceInput<'_>) -> GooseResult<bool> {
        validate_metric_provenance_input(self, &input)?;
        let changed = self.conn.execute(
            r#"
            INSERT INTO metric_provenance (
                provenance_id,
                metric_scope,
                metric_id,
                source_kind,
                source_detail,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(provenance_id) DO UPDATE SET
                metric_scope = excluded.metric_scope,
                metric_id = excluded.metric_id,
                source_kind = excluded.source_kind,
                source_detail = excluded.source_detail,
                confidence = excluded.confidence,
                inputs_json = excluded.inputs_json,
                quality_flags_json = excluded.quality_flags_json,
                provenance_json = excluded.provenance_json
            WHERE metric_provenance.metric_scope IS NOT excluded.metric_scope
               OR metric_provenance.metric_id IS NOT excluded.metric_id
               OR metric_provenance.source_kind IS NOT excluded.source_kind
               OR metric_provenance.source_detail IS NOT excluded.source_detail
               OR metric_provenance.confidence IS NOT excluded.confidence
               OR metric_provenance.inputs_json IS NOT excluded.inputs_json
               OR metric_provenance.quality_flags_json IS NOT excluded.quality_flags_json
               OR metric_provenance.provenance_json IS NOT excluded.provenance_json
            "#,
            params![
                input.provenance_id,
                input.metric_scope,
                input.metric_id,
                input.source_kind,
                input.source_detail,
                input.confidence,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn metric_provenance(
        &self,
        provenance_id: &str,
    ) -> GooseResult<Option<MetricProvenanceRow>> {
        validate_required("provenance_id", provenance_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    provenance_id,
                    metric_scope,
                    metric_id,
                    source_kind,
                    source_detail,
                    confidence,
                    inputs_json,
                    quality_flags_json,
                    provenance_json,
                    created_at
                FROM metric_provenance
                WHERE provenance_id = ?1
                "#,
                params![provenance_id],
                metric_provenance_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn metric_provenance_for_metric(
        &self,
        metric_scope: &str,
        metric_id: &str,
    ) -> GooseResult<Vec<MetricProvenanceRow>> {
        validate_required("metric_scope", metric_scope)?;
        validate_required("metric_id", metric_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                provenance_id,
                metric_scope,
                metric_id,
                source_kind,
                source_detail,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at
            FROM metric_provenance
            WHERE metric_scope = ?1
              AND metric_id = ?2
            ORDER BY provenance_id
            "#,
        )?;
        let rows =
            statement.query_map(params![metric_scope, metric_id], metric_provenance_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_metric_debug_feature(
        &self,
        input: MetricDebugFeatureInput<'_>,
    ) -> GooseResult<bool> {
        validate_metric_debug_feature_input(&input)?;
        if let Some(existing) = self.metric_debug_feature(input.feature_id)? {
            let same = existing.metric_family == input.metric_family
                && existing.feature_name == input.feature_name
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.source_kind == input.source_kind
                && existing.confidence == input.confidence
                && existing.feature_json == input.feature_json
                && existing.inputs_json == input.inputs_json
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "metric debug feature {} already exists with different metadata",
                input.feature_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO metric_debug_features (
                feature_id,
                metric_family,
                feature_name,
                start_time_unix_ms,
                end_time_unix_ms,
                source_kind,
                confidence,
                feature_json,
                inputs_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                input.feature_id,
                input.metric_family,
                input.feature_name,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.source_kind,
                input.confidence,
                input.feature_json,
                input.inputs_json,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn metric_debug_feature(
        &self,
        feature_id: &str,
    ) -> GooseResult<Option<MetricDebugFeatureRow>> {
        validate_required("feature_id", feature_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    feature_id,
                    metric_family,
                    feature_name,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    source_kind,
                    confidence,
                    feature_json,
                    inputs_json,
                    quality_flags_json,
                    provenance_json,
                    created_at
                FROM metric_debug_features
                WHERE feature_id = ?1
                "#,
                params![feature_id],
                metric_debug_feature_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn metric_debug_features_between(
        &self,
        metric_family: &str,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<MetricDebugFeatureRow>> {
        validate_required("metric_family", metric_family)?;
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                feature_id,
                metric_family,
                feature_name,
                start_time_unix_ms,
                end_time_unix_ms,
                source_kind,
                confidence,
                feature_json,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at
            FROM metric_debug_features
            WHERE metric_family = ?1
              AND start_time_unix_ms < ?3
              AND end_time_unix_ms > ?2
            ORDER BY start_time_unix_ms, feature_id
            "#,
        )?;
        let rows = statement.query_map(
            params![metric_family, start_time_unix_ms, end_time_unix_ms],
            metric_debug_feature_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_step_counter_sample(
        &self,
        input: StepCounterSampleInput<'_>,
    ) -> GooseResult<bool> {
        validate_step_counter_sample_input(&input)?;
        if let Some(existing) = self.step_counter_sample(input.sample_id)? {
            let same = existing.sample_time_unix_ms == input.sample_time_unix_ms
                && existing.counter_value == input.counter_value
                && existing.cadence_spm == input.cadence_spm
                && existing.activity_state.as_deref() == input.activity_state
                && existing.source_kind == input.source_kind
                && existing.packet_family == input.packet_family
                && existing.json_path == input.json_path
                && existing.frame_id.as_deref() == input.frame_id
                && existing.evidence_id.as_deref() == input.evidence_id
                && existing.capture_session_id.as_deref() == input.capture_session_id
                && existing.quality_flags_json == input.quality_flags_json
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "step counter sample {} already exists with different metadata",
                input.sample_id
            )));
        }

        let changed = self.conn.execute(
            r#"
            INSERT INTO step_counter_samples (
                sample_id,
                sample_time_unix_ms,
                counter_value,
                cadence_spm,
                activity_state,
                source_kind,
                packet_family,
                json_path,
                frame_id,
                evidence_id,
                capture_session_id,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                input.sample_id,
                input.sample_time_unix_ms,
                input.counter_value,
                input.cadence_spm,
                input.activity_state,
                input.source_kind,
                input.packet_family,
                input.json_path,
                input.frame_id,
                input.evidence_id,
                input.capture_session_id,
                input.quality_flags_json,
                input.provenance_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn step_counter_sample(
        &self,
        sample_id: &str,
    ) -> GooseResult<Option<StepCounterSampleRow>> {
        validate_required("sample_id", sample_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    sample_id,
                    sample_time_unix_ms,
                    counter_value,
                    cadence_spm,
                    activity_state,
                    source_kind,
                    packet_family,
                    json_path,
                    frame_id,
                    evidence_id,
                    capture_session_id,
                    quality_flags_json,
                    provenance_json,
                    created_at
                FROM step_counter_samples
                WHERE sample_id = ?1
                "#,
                params![sample_id],
                step_counter_sample_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn step_counter_samples_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<StepCounterSampleRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                    sample_id,
                    sample_time_unix_ms,
                    counter_value,
                    cadence_spm,
                    activity_state,
                    source_kind,
                    packet_family,
                json_path,
                frame_id,
                evidence_id,
                capture_session_id,
                quality_flags_json,
                provenance_json,
                created_at
            FROM step_counter_samples
            WHERE sample_time_unix_ms >= ?1
              AND sample_time_unix_ms < ?2
            ORDER BY sample_time_unix_ms, sample_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            step_counter_sample_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_activity_interval(&self, input: ActivityIntervalInput<'_>) -> GooseResult<bool> {
        validate_activity_interval_input(self, &input)?;
        if self.activity_session(input.activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                input.activity_session_id
            )));
        }
        if let Some(existing) = self.activity_interval(input.interval_id)? {
            if existing.activity_session_id == input.activity_session_id
                && existing.interval_type == input.interval_type
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.sequence == input.sequence
                && existing.metadata_json == input.metadata_json
                && existing.provenance_json == input.provenance_json
            {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "activity interval {} already exists with different metadata",
                input.interval_id
            )));
        }
        let duration_ms = input.end_time_unix_ms - input.start_time_unix_ms;
        self.conn.execute(
            r#"
            INSERT INTO activity_intervals (
                interval_id,
                activity_session_id,
                interval_type,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                sequence,
                metadata_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                input.interval_id,
                input.activity_session_id,
                input.interval_type,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                duration_ms,
                input.sequence,
                input.metadata_json,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn activity_interval(&self, interval_id: &str) -> GooseResult<Option<ActivityIntervalRow>> {
        validate_required("interval_id", interval_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    interval_id,
                    activity_session_id,
                    interval_type,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    duration_ms,
                    sequence,
                    metadata_json,
                    provenance_json,
                    created_at
                FROM activity_intervals
                WHERE interval_id = ?1
                "#,
                params![interval_id],
                activity_interval_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn activity_intervals_for_session(
        &self,
        activity_session_id: &str,
    ) -> GooseResult<Vec<ActivityIntervalRow>> {
        validate_required("activity_session_id", activity_session_id)?;
        if self.activity_session(activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                activity_session_id
            )));
        }
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                interval_id,
                activity_session_id,
                interval_type,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                sequence,
                metadata_json,
                provenance_json,
                created_at
            FROM activity_intervals
            WHERE activity_session_id = ?1
            ORDER BY start_time_unix_ms, sequence, interval_id
            "#,
        )?;
        let rows = statement.query_map(params![activity_session_id], activity_interval_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_intervals_in_window(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<ActivityIntervalRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                interval_id,
                activity_session_id,
                interval_type,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                sequence,
                metadata_json,
                provenance_json,
                created_at
            FROM activity_intervals
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, sequence, interval_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            activity_interval_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_activity_label(&self, input: ActivityLabelInput<'_>) -> GooseResult<bool> {
        validate_activity_label_input(self, &input)?;
        if self.activity_session(input.activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                input.activity_session_id
            )));
        }
        if let Some(existing) = self.activity_label(input.label_id)? {
            if existing.activity_session_id == input.activity_session_id
                && existing.label_type == input.label_type
                && existing.value == input.value
                && existing.source == input.source
                && existing.confidence == input.confidence
                && existing.provenance_json == input.provenance_json
            {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "activity label {} already exists with different metadata",
                input.label_id
            )));
        }
        self.conn.execute(
            r#"
            INSERT INTO activity_labels (
                label_id,
                activity_session_id,
                label_type,
                value,
                source,
                confidence,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                input.label_id,
                input.activity_session_id,
                input.label_type,
                input.value,
                input.source,
                input.confidence,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn insert_external_sleep_session(
        &self,
        input: ExternalSleepSessionInput<'_>,
    ) -> GooseResult<bool> {
        validate_external_sleep_session_input(&input)?;

        if let Some(existing) = self.external_sleep_session(input.sleep_id)? {
            let same = existing.sleep_id == input.sleep_id
                && existing.source == input.source
                && existing.platform == input.platform
                && existing.platform_record_id == input.platform_record_id.map(str::to_string)
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.timezone == input.timezone.map(str::to_string)
                && existing.stage_summary_json == input.stage_summary_json
                && existing.confidence == input.confidence
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "external sleep session {} already exists with different metadata",
                input.sleep_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO external_sleep_sessions (
                sleep_id,
                source,
                platform,
                platform_record_id,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                timezone,
                stage_summary_json,
                confidence,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                input.sleep_id,
                input.source,
                input.platform,
                input.platform_record_id,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.end_time_unix_ms - input.start_time_unix_ms,
                input.timezone,
                input.stage_summary_json,
                input.confidence,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn external_sleep_session(
        &self,
        sleep_id: &str,
    ) -> GooseResult<Option<ExternalSleepSessionRow>> {
        validate_required("sleep_id", sleep_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    sleep_id,
                    source,
                    platform,
                    platform_record_id,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    duration_ms,
                    timezone,
                    stage_summary_json,
                    confidence,
                    provenance_json,
                    created_at,
                    updated_at
                FROM external_sleep_sessions
                WHERE sleep_id = ?1
                "#,
                params![sleep_id],
                external_sleep_session_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn external_sleep_sessions_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<ExternalSleepSessionRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                sleep_id,
                source,
                platform,
                platform_record_id,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                timezone,
                stage_summary_json,
                confidence,
                provenance_json,
                created_at,
                updated_at
            FROM external_sleep_sessions
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, sleep_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            external_sleep_session_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_external_sleep_stage(
        &self,
        input: ExternalSleepStageInput<'_>,
    ) -> GooseResult<bool> {
        validate_external_sleep_stage_input(self, &input)?;

        if let Some(existing) = self.external_sleep_stage(input.stage_id)? {
            let same = existing.stage_id == input.stage_id
                && existing.sleep_id == input.sleep_id
                && existing.stage_kind == input.stage_kind
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.confidence == input.confidence
                && existing.provenance_json == input.provenance_json;
            if same {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "external sleep stage {} already exists with different metadata",
                input.stage_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO external_sleep_stages (
                stage_id,
                sleep_id,
                stage_kind,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                confidence,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                input.stage_id,
                input.sleep_id,
                input.stage_kind,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.end_time_unix_ms - input.start_time_unix_ms,
                input.confidence,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn external_sleep_stage(
        &self,
        stage_id: &str,
    ) -> GooseResult<Option<ExternalSleepStageRow>> {
        validate_required("stage_id", stage_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    stage_id,
                    sleep_id,
                    stage_kind,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    duration_ms,
                    confidence,
                    provenance_json,
                    created_at
                FROM external_sleep_stages
                WHERE stage_id = ?1
                "#,
                params![stage_id],
                external_sleep_stage_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn external_sleep_stages_for_session(
        &self,
        sleep_id: &str,
    ) -> GooseResult<Vec<ExternalSleepStageRow>> {
        validate_required("sleep_id", sleep_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                stage_id,
                sleep_id,
                stage_kind,
                start_time_unix_ms,
                end_time_unix_ms,
                duration_ms,
                confidence,
                provenance_json,
                created_at
            FROM external_sleep_stages
            WHERE sleep_id = ?1
            ORDER BY start_time_unix_ms, stage_id
            "#,
        )?;
        let rows = statement.query_map(params![sleep_id], external_sleep_stage_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_sleep_correction_label(
        &self,
        input: SleepCorrectionLabelInput<'_>,
    ) -> GooseResult<bool> {
        validate_sleep_correction_label_input(&input)?;
        if let Some(existing) = self.sleep_correction_label(input.label_id)? {
            if existing.sleep_id == input.sleep_id.map(str::to_string)
                && existing.label_type == input.label_type
                && existing.start_time_unix_ms == input.start_time_unix_ms
                && existing.end_time_unix_ms == input.end_time_unix_ms
                && existing.value_json == input.value_json
                && existing.source == input.source
                && existing.confidence == input.confidence
                && existing.provenance_json == input.provenance_json
            {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "sleep correction label {} already exists with different metadata",
                input.label_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO sleep_correction_labels (
                label_id,
                sleep_id,
                label_type,
                start_time_unix_ms,
                end_time_unix_ms,
                value_json,
                source,
                confidence,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                input.label_id,
                input.sleep_id,
                input.label_type,
                input.start_time_unix_ms,
                input.end_time_unix_ms,
                input.value_json,
                input.source,
                input.confidence,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn sleep_correction_label(
        &self,
        label_id: &str,
    ) -> GooseResult<Option<SleepCorrectionLabelRow>> {
        validate_required("label_id", label_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    label_id,
                    sleep_id,
                    label_type,
                    start_time_unix_ms,
                    end_time_unix_ms,
                    value_json,
                    source,
                    confidence,
                    provenance_json,
                    created_at
                FROM sleep_correction_labels
                WHERE label_id = ?1
                "#,
                params![label_id],
                sleep_correction_label_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn sleep_correction_labels_between(
        &self,
        start_time_unix_ms: i64,
        end_time_unix_ms: i64,
    ) -> GooseResult<Vec<SleepCorrectionLabelRow>> {
        validate_non_negative("start_time_unix_ms", start_time_unix_ms)?;
        validate_non_negative("end_time_unix_ms", end_time_unix_ms)?;
        validate_window_order(start_time_unix_ms, end_time_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                label_id,
                sleep_id,
                label_type,
                start_time_unix_ms,
                end_time_unix_ms,
                value_json,
                source,
                confidence,
                provenance_json,
                created_at
            FROM sleep_correction_labels
            WHERE start_time_unix_ms < ?2
              AND end_time_unix_ms > ?1
            ORDER BY start_time_unix_ms, label_type, label_id
            "#,
        )?;
        let rows = statement.query_map(
            params![start_time_unix_ms, end_time_unix_ms],
            sleep_correction_label_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_label(&self, label_id: &str) -> GooseResult<Option<ActivityLabelRow>> {
        validate_required("label_id", label_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    label_id,
                    activity_session_id,
                    label_type,
                    value,
                    source,
                    confidence,
                    provenance_json,
                    created_at
                FROM activity_labels
                WHERE label_id = ?1
                "#,
                params![label_id],
                activity_label_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn activity_labels_for_session(
        &self,
        activity_session_id: &str,
    ) -> GooseResult<Vec<ActivityLabelRow>> {
        validate_required("activity_session_id", activity_session_id)?;
        if self.activity_session(activity_session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "activity session {} not found",
                activity_session_id
            )));
        }
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                label_id,
                activity_session_id,
                label_type,
                value,
                source,
                confidence,
                provenance_json,
                created_at
            FROM activity_labels
            WHERE activity_session_id = ?1
            ORDER BY label_type, created_at, label_id
            "#,
        )?;
        let rows = statement.query_map(params![activity_session_id], activity_label_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn activity_labels_by_type(&self, label_type: &str) -> GooseResult<Vec<ActivityLabelRow>> {
        validate_required("label_type", label_type)?;
        validate_activity_label_type(label_type)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                label_id,
                activity_session_id,
                label_type,
                value,
                source,
                confidence,
                provenance_json,
                created_at
            FROM activity_labels
            WHERE label_type = ?1
            ORDER BY created_at, label_id
            "#,
        )?;
        let rows = statement.query_map(params![label_type], activity_label_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn raw_evidence(&self, evidence_id: &str) -> GooseResult<Option<RawEvidenceRow>> {
        self.conn
            .query_row(
                r#"
                SELECT
                    evidence_id,
                    source,
                    captured_at,
                    device_model,
                    payload_hex,
                    sha256,
                    sensitivity,
                    capture_session_id
                FROM raw_evidence
                WHERE evidence_id = ?1
                "#,
                params![evidence_id],
                |row| {
                    Ok(RawEvidenceRow {
                        evidence_id: row.get(0)?,
                        source: row.get(1)?,
                        captured_at: row.get(2)?,
                        device_model: row.get(3)?,
                        payload_hex: row.get(4)?,
                        sha256: row.get(5)?,
                        sensitivity: row.get(6)?,
                        capture_session_id: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn raw_evidence_between(&self, start: &str, end: &str) -> GooseResult<Vec<RawEvidenceRow>> {
        validate_required("start", start)?;
        validate_required("end", end)?;

        let mut statement = self.conn.prepare(
            r#"
            SELECT
                evidence_id,
                source,
                captured_at,
                device_model,
                payload_hex,
                sha256,
                sensitivity,
                capture_session_id
            FROM raw_evidence
            WHERE captured_at >= ?1 AND captured_at < ?2
            ORDER BY captured_at, evidence_id
            "#,
        )?;
        let rows = statement.query_map(params![start, end], |row| {
            Ok(RawEvidenceRow {
                evidence_id: row.get(0)?,
                source: row.get(1)?,
                captured_at: row.get(2)?,
                device_model: row.get(3)?,
                payload_hex: row.get(4)?,
                sha256: row.get(5)?,
                sensitivity: row.get(6)?,
                capture_session_id: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn raw_evidence_payload_bytes(&self) -> GooseResult<i64> {
        Ok(self.conn.query_row(
            r#"
            SELECT COALESCE(SUM(LENGTH(payload_hex) / 2), 0)
            FROM raw_evidence
            WHERE payload_hex != ''
            "#,
            [],
            |row| row.get(0),
        )?)
    }

    pub fn compact_raw_evidence_payloads_to_limit(
        &self,
        limit_bytes: i64,
    ) -> GooseResult<RawEvidencePayloadRetentionReport> {
        validate_non_negative("limit_bytes", limit_bytes)?;
        let before_bytes = self.raw_evidence_payload_bytes()?;
        if before_bytes <= limit_bytes {
            return Ok(RawEvidencePayloadRetentionReport {
                limit_bytes,
                before_bytes,
                after_bytes: before_bytes,
                compacted_rows: 0,
                freed_bytes: 0,
            });
        }

        let mut bytes_to_free = before_bytes - limit_bytes;
        let mut statement = self.conn.prepare(
            r#"
            SELECT evidence_id, LENGTH(payload_hex) / 2
            FROM raw_evidence
            WHERE payload_hex != ''
            ORDER BY captured_at, evidence_id
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut compacted_ids = Vec::new();
        for row in rows {
            let (evidence_id, payload_bytes) = row?;
            if bytes_to_free <= 0 {
                break;
            }
            bytes_to_free -= payload_bytes;
            compacted_ids.push(evidence_id);
        }

        let mut compacted_rows = 0;
        for evidence_id in compacted_ids {
            compacted_rows += self.conn.execute(
                "UPDATE raw_evidence SET payload_hex = '' WHERE evidence_id = ?1",
                params![evidence_id],
            )? as i64;
        }

        let after_bytes = self.raw_evidence_payload_bytes()?;
        Ok(RawEvidencePayloadRetentionReport {
            limit_bytes,
            before_bytes,
            after_bytes,
            compacted_rows,
            freed_bytes: before_bytes - after_bytes,
        })
    }

    pub fn decoded_frames_between(
        &self,
        start: &str,
        end: &str,
    ) -> GooseResult<Vec<DecodedFrameRow>> {
        validate_required("start", start)?;
        validate_required("end", end)?;

        let mut statement = self.conn.prepare(
            r#"
            SELECT
                decoded_frames.frame_id,
                decoded_frames.evidence_id,
                raw_evidence.captured_at,
                decoded_frames.device_type,
                decoded_frames.raw_len,
                decoded_frames.header_len,
                decoded_frames.declared_len,
                decoded_frames.payload_hex,
                decoded_frames.payload_crc_hex,
                decoded_frames.header_crc_valid,
                decoded_frames.payload_crc_valid,
                decoded_frames.packet_type,
                decoded_frames.packet_type_name,
                decoded_frames.sequence,
                decoded_frames.command_or_event,
                decoded_frames.parsed_payload_json,
                decoded_frames.parser_version,
                decoded_frames.warnings_json
            FROM decoded_frames
            INNER JOIN raw_evidence
                ON raw_evidence.evidence_id = decoded_frames.evidence_id
            WHERE raw_evidence.captured_at >= ?1 AND raw_evidence.captured_at < ?2
            ORDER BY raw_evidence.captured_at, decoded_frames.frame_id
            "#,
        )?;
        let rows = statement.query_map(params![start, end], decoded_frame_from_row)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn decoded_frame(&self, frame_id: &str) -> GooseResult<Option<DecodedFrameRow>> {
        validate_required("frame_id", frame_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    decoded_frames.frame_id,
                    decoded_frames.evidence_id,
                    raw_evidence.captured_at,
                    decoded_frames.device_type,
                    decoded_frames.raw_len,
                    decoded_frames.header_len,
                    decoded_frames.declared_len,
                    decoded_frames.payload_hex,
                    decoded_frames.payload_crc_hex,
                    decoded_frames.header_crc_valid,
                    decoded_frames.payload_crc_valid,
                    decoded_frames.packet_type,
                    decoded_frames.packet_type_name,
                    decoded_frames.sequence,
                    decoded_frames.command_or_event,
                    decoded_frames.parsed_payload_json,
                    decoded_frames.parser_version,
                    decoded_frames.warnings_json
                FROM decoded_frames
                INNER JOIN raw_evidence
                    ON raw_evidence.evidence_id = decoded_frames.evidence_id
                WHERE decoded_frames.frame_id = ?1
                "#,
                params![frame_id],
                decoded_frame_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn upsert_algorithm_definition(
        &self,
        definition: &AlgorithmDefinitionRecord,
    ) -> GooseResult<()> {
        validate_required("algorithm_id", &definition.algorithm_id)?;
        validate_required("version", &definition.version)?;
        validate_required("metric_family", &definition.metric_family)?;
        validate_required("display_name", &definition.display_name)?;
        validate_required("implementation", &definition.implementation)?;
        validate_required("license", &definition.license)?;
        validate_required("input_schema", &definition.input_schema)?;
        validate_required("output_schema", &definition.output_schema)?;
        validate_json(
            "input_requirements_json",
            &definition.input_requirements_json,
        )?;
        validate_json("params_json", &definition.params_json)?;
        validate_json("quality_gates_json", &definition.quality_gates_json)?;
        validate_required("status", &definition.status)?;

        self.conn.execute(
            r#"
            INSERT INTO algorithm_definitions (
                algorithm_id,
                version,
                metric_family,
                display_name,
                implementation,
                license,
                input_schema,
                output_schema,
                input_requirements_json,
                params_json,
                quality_gates_json,
                status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(algorithm_id, version) DO UPDATE SET
                metric_family = excluded.metric_family,
                display_name = excluded.display_name,
                implementation = excluded.implementation,
                license = excluded.license,
                input_schema = excluded.input_schema,
                output_schema = excluded.output_schema,
                input_requirements_json = excluded.input_requirements_json,
                params_json = excluded.params_json,
                quality_gates_json = excluded.quality_gates_json,
                status = excluded.status
            "#,
            params![
                definition.algorithm_id,
                definition.version,
                definition.metric_family,
                definition.display_name,
                definition.implementation,
                definition.license,
                definition.input_schema,
                definition.output_schema,
                definition.input_requirements_json,
                definition.params_json,
                definition.quality_gates_json,
                definition.status,
            ],
        )?;
        Ok(())
    }

    pub fn algorithm_definition(
        &self,
        algorithm_id: &str,
        version: &str,
    ) -> GooseResult<Option<AlgorithmDefinitionRecord>> {
        self.conn
            .query_row(
                r#"
                SELECT
                    algorithm_id,
                    version,
                    metric_family,
                    display_name,
                    implementation,
                    license,
                    input_schema,
                    output_schema,
                    input_requirements_json,
                    params_json,
                    quality_gates_json,
                    status
                FROM algorithm_definitions
                WHERE algorithm_id = ?1 AND version = ?2
                "#,
                params![algorithm_id, version],
                |row| {
                    Ok(AlgorithmDefinitionRecord {
                        algorithm_id: row.get(0)?,
                        version: row.get(1)?,
                        metric_family: row.get(2)?,
                        display_name: row.get(3)?,
                        implementation: row.get(4)?,
                        license: row.get(5)?,
                        input_schema: row.get(6)?,
                        output_schema: row.get(7)?,
                        input_requirements_json: row.get(8)?,
                        params_json: row.get(9)?,
                        quality_gates_json: row.get(10)?,
                        status: row.get(11)?,
                    })
                },
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn set_algorithm_preference(
        &self,
        preference: &AlgorithmPreferenceRecord,
    ) -> GooseResult<()> {
        validate_required("scope", &preference.scope)?;
        validate_required("metric_family", &preference.metric_family)?;
        validate_required("algorithm_id", &preference.algorithm_id)?;
        validate_required("version", &preference.version)?;

        let Some(definition) =
            self.algorithm_definition(&preference.algorithm_id, &preference.version)?
        else {
            return Err(GooseError::message(format!(
                "algorithm definition {}@{} must exist before it can be selected",
                preference.algorithm_id, preference.version
            )));
        };
        if definition.metric_family != preference.metric_family {
            return Err(GooseError::message(format!(
                "algorithm {}@{} belongs to metric family {}, not {}",
                preference.algorithm_id,
                preference.version,
                definition.metric_family,
                preference.metric_family
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO algorithm_preferences (
                scope,
                metric_family,
                algorithm_id,
                version
            ) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(scope, metric_family) DO UPDATE SET
                algorithm_id = excluded.algorithm_id,
                version = excluded.version,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
            params![
                preference.scope,
                preference.metric_family,
                preference.algorithm_id,
                preference.version,
            ],
        )?;
        Ok(())
    }

    pub fn algorithm_preference(
        &self,
        scope: &str,
        metric_family: &str,
    ) -> GooseResult<Option<AlgorithmPreferenceRecord>> {
        validate_required("scope", scope)?;
        validate_required("metric_family", metric_family)?;

        self.conn
            .query_row(
                r#"
                SELECT scope, metric_family, algorithm_id, version
                FROM algorithm_preferences
                WHERE scope = ?1 AND metric_family = ?2
                "#,
                params![scope, metric_family],
                |row| {
                    Ok(AlgorithmPreferenceRecord {
                        scope: row.get(0)?,
                        metric_family: row.get(1)?,
                        algorithm_id: row.get(2)?,
                        version: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn algorithm_preferences(
        &self,
        scope: Option<&str>,
    ) -> GooseResult<Vec<AlgorithmPreferenceRecord>> {
        if let Some(scope) = scope {
            validate_required("scope", scope)?;
            let mut statement = self.conn.prepare(
                r#"
                SELECT scope, metric_family, algorithm_id, version
                FROM algorithm_preferences
                WHERE scope = ?1
                ORDER BY metric_family
                "#,
            )?;
            let rows = statement.query_map(params![scope], algorithm_preference_from_row)?;
            return rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(GooseError::from);
        }

        let mut statement = self.conn.prepare(
            r#"
            SELECT scope, metric_family, algorithm_id, version
            FROM algorithm_preferences
            ORDER BY scope, metric_family
            "#,
        )?;
        let rows = statement.query_map([], algorithm_preference_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_algorithm_run(&self, run: &AlgorithmRunRecord) -> GooseResult<bool> {
        validate_required("run_id", &run.run_id)?;
        validate_required("algorithm_id", &run.algorithm_id)?;
        validate_required("version", &run.version)?;
        validate_required("start_time", &run.start_time)?;
        validate_required("end_time", &run.end_time)?;
        validate_json("output_json", &run.output_json)?;
        validate_json("quality_flags_json", &run.quality_flags_json)?;
        validate_json("provenance_json", &run.provenance_json)?;

        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO algorithm_runs (
                run_id,
                algorithm_id,
                version,
                start_time,
                end_time,
                output_json,
                quality_flags_json,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                run.run_id,
                run.algorithm_id,
                run.version,
                run.start_time,
                run.end_time,
                run.output_json,
                run.quality_flags_json,
                run.provenance_json,
            ],
        )?;
        if changed > 0 {
            self.insert_metric_rows_for_algorithm_run(run)?;
        }
        Ok(changed > 0)
    }

    fn insert_metric_rows_for_algorithm_run(&self, run: &AlgorithmRunRecord) -> GooseResult<()> {
        let definition = self
            .algorithm_definition(&run.algorithm_id, &run.version)?
            .ok_or_else(|| {
                GooseError::message(format!(
                    "missing algorithm definition {} {}",
                    run.algorithm_id, run.version
                ))
            })?;
        let output: Value = serde_json::from_str(&run.output_json).map_err(|error| {
            GooseError::message(format!("output_json is not valid JSON: {error}"))
        })?;
        let Some(output_object) = output.as_object() else {
            return Ok(());
        };

        for (name, value) in output_object {
            if name == "algorithm_id" || name == "algorithm_version" || name == "components" {
                continue;
            }
            let Some(value) = finite_json_number(value) else {
                continue;
            };
            self.conn.execute(
                r#"
                INSERT OR IGNORE INTO metric_values (
                    metric_value_id,
                    run_id,
                    metric_family,
                    name,
                    value,
                    unit,
                    start_time,
                    end_time
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![
                    format!("{}.{}", run.run_id, name),
                    run.run_id,
                    definition.metric_family,
                    name,
                    value,
                    metric_output_unit(name),
                    run.start_time,
                    run.end_time,
                ],
            )?;
        }

        if let Some(components) = output_object.get("components").and_then(Value::as_array) {
            for (index, component) in components.iter().enumerate() {
                let component_name = component
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed_component");
                let Some(value) = component.get("value").and_then(finite_json_number) else {
                    continue;
                };
                let unit = component
                    .get("unit")
                    .and_then(Value::as_str)
                    .unwrap_or("raw");
                let contribution_json = serde_json::json!({
                    "score_0_to_100": component.get("score_0_to_100").cloned().unwrap_or(Value::Null),
                    "weight": component.get("weight").cloned().unwrap_or(Value::Null),
                    "contribution": component.get("contribution").cloned().unwrap_or(Value::Null),
                })
                .to_string();
                self.conn.execute(
                    r#"
                    INSERT OR IGNORE INTO metric_components (
                        metric_component_id,
                        run_id,
                        component_name,
                        value,
                        unit,
                        contribution_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        format!("{}.component.{}.{}", run.run_id, index, component_name),
                        run.run_id,
                        component_name,
                        value,
                        unit,
                        contribution_json,
                    ],
                )?;
            }
        }

        Ok(())
    }

    pub fn algorithm_run(&self, run_id: &str) -> GooseResult<Option<AlgorithmRunRecord>> {
        self.conn
            .query_row(
                r#"
                SELECT
                    run_id,
                    algorithm_id,
                    version,
                    start_time,
                    end_time,
                    output_json,
                    quality_flags_json,
                    provenance_json
                FROM algorithm_runs
                WHERE run_id = ?1
                "#,
                params![run_id],
                |row| {
                    Ok(AlgorithmRunRecord {
                        run_id: row.get(0)?,
                        algorithm_id: row.get(1)?,
                        version: row.get(2)?,
                        start_time: row.get(3)?,
                        end_time: row.get(4)?,
                        output_json: row.get(5)?,
                        quality_flags_json: row.get(6)?,
                        provenance_json: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn algorithm_runs_overlapping(
        &self,
        start: &str,
        end: &str,
    ) -> GooseResult<Vec<AlgorithmRunRecord>> {
        validate_required("start", start)?;
        validate_required("end", end)?;

        let mut statement = self.conn.prepare(
            r#"
            SELECT
                run_id,
                algorithm_id,
                version,
                start_time,
                end_time,
                output_json,
                quality_flags_json,
                provenance_json
            FROM algorithm_runs
            WHERE start_time < ?2 AND end_time > ?1
            ORDER BY start_time, run_id
            "#,
        )?;
        let rows = statement.query_map(params![start, end], |row| {
            Ok(AlgorithmRunRecord {
                run_id: row.get(0)?,
                algorithm_id: row.get(1)?,
                version: row.get(2)?,
                start_time: row.get(3)?,
                end_time: row.get(4)?,
                output_json: row.get(5)?,
                quality_flags_json: row.get(6)?,
                provenance_json: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn metric_values_for_run(&self, run_id: &str) -> GooseResult<Vec<MetricValueRecord>> {
        validate_required("run_id", run_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_value_id,
                run_id,
                metric_family,
                name,
                value,
                unit,
                start_time,
                end_time
            FROM metric_values
            WHERE run_id = ?1
            ORDER BY name
            "#,
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok(MetricValueRecord {
                metric_value_id: row.get(0)?,
                run_id: row.get(1)?,
                metric_family: row.get(2)?,
                name: row.get(3)?,
                value: row.get(4)?,
                unit: row.get(5)?,
                start_time: row.get(6)?,
                end_time: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn metric_components_for_run(
        &self,
        run_id: &str,
    ) -> GooseResult<Vec<MetricComponentRecord>> {
        validate_required("run_id", run_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                metric_component_id,
                run_id,
                component_name,
                value,
                unit,
                contribution_json
            FROM metric_components
            WHERE run_id = ?1
            ORDER BY metric_component_id
            "#,
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok(MetricComponentRecord {
                metric_component_id: row.get(0)?,
                run_id: row.get(1)?,
                component_name: row.get(2)?,
                value: row.get(3)?,
                unit: row.get(4)?,
                contribution_json: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_calibration_run(&self, run: &CalibrationRunRecord) -> GooseResult<bool> {
        validate_required("calibration_run_id", &run.calibration_run_id)?;
        validate_required("algorithm_id", &run.algorithm_id)?;
        validate_required("version", &run.version)?;
        validate_required("train_start", &run.times.train_start)?;
        validate_required("train_end", &run.times.train_end)?;
        validate_required("holdout_start", &run.times.holdout_start)?;
        validate_required("holdout_end", &run.times.holdout_end)?;
        validate_json("metrics_json", &run.metrics_json)?;
        validate_json("params_json", &run.params_json)?;

        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO calibration_runs (
                calibration_run_id,
                algorithm_id,
                version,
                train_start,
                train_end,
                holdout_start,
                holdout_end,
                metrics_json,
                params_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                run.calibration_run_id,
                run.algorithm_id,
                run.version,
                run.times.train_start,
                run.times.train_end,
                run.times.holdout_start,
                run.times.holdout_end,
                run.metrics_json,
                run.params_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn calibration_run(
        &self,
        calibration_run_id: &str,
    ) -> GooseResult<Option<CalibrationRunRecord>> {
        self.conn
            .query_row(
                r#"
                SELECT
                    calibration_run_id,
                    algorithm_id,
                    version,
                    train_start,
                    train_end,
                    holdout_start,
                    holdout_end,
                    metrics_json,
                    params_json
                FROM calibration_runs
                WHERE calibration_run_id = ?1
                "#,
                params![calibration_run_id],
                |row| {
                    Ok(CalibrationRunRecord {
                        calibration_run_id: row.get(0)?,
                        algorithm_id: row.get(1)?,
                        version: row.get(2)?,
                        times: CalibrationRunTimes {
                            train_start: row.get(3)?,
                            train_end: row.get(4)?,
                            holdout_start: row.get(5)?,
                            holdout_end: row.get(6)?,
                        },
                        metrics_json: row.get(7)?,
                        params_json: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn calibration_runs_overlapping(
        &self,
        start: &str,
        end: &str,
    ) -> GooseResult<Vec<CalibrationRunRecord>> {
        validate_required("start", start)?;
        validate_required("end", end)?;

        let mut statement = self.conn.prepare(
            r#"
            SELECT
                calibration_run_id,
                algorithm_id,
                version,
                train_start,
                train_end,
                holdout_start,
                holdout_end,
                metrics_json,
                params_json
            FROM calibration_runs
            WHERE holdout_start < ?2 AND holdout_end > ?1
            ORDER BY holdout_start, calibration_run_id
            "#,
        )?;
        let rows = statement.query_map(params![start, end], |row| {
            Ok(CalibrationRunRecord {
                calibration_run_id: row.get(0)?,
                algorithm_id: row.get(1)?,
                version: row.get(2)?,
                times: CalibrationRunTimes {
                    train_start: row.get(3)?,
                    train_end: row.get(4)?,
                    holdout_start: row.get(5)?,
                    holdout_end: row.get(6)?,
                },
                metrics_json: row.get(7)?,
                params_json: row.get(8)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_calibration_label(&self, input: CalibrationLabelInput<'_>) -> GooseResult<bool> {
        validate_required("label_id", input.label_id)?;
        validate_required("metric_family", input.metric_family)?;
        validate_required("label_source", input.label_source)?;
        validate_required("captured_at", input.captured_at)?;
        validate_required("unit", input.unit)?;
        validate_json_object("provenance_json", input.provenance_json)?;
        if !input.value.is_finite() {
            return Err(GooseError::message("value must be finite"));
        }
        if !is_allowed_calibration_label_source(input.label_source) {
            return Err(GooseError::message(format!(
                "unsupported label_source {}",
                input.label_source
            )));
        }
        let parsed_provenance: serde_json::Value = serde_json::from_str(input.provenance_json)
            .map_err(|error| {
                GooseError::message(format!("provenance_json must be valid JSON: {error}"))
            })?;
        if parsed_provenance == serde_json::json!({}) {
            return Err(GooseError::message("provenance_json must not be empty"));
        }

        if let Some(existing) = self.calibration_label(input.label_id)? {
            let new_row = CalibrationLabelRow {
                label_id: input.label_id.to_string(),
                metric_family: input.metric_family.to_string(),
                label_source: input.label_source.to_string(),
                captured_at: input.captured_at.to_string(),
                value: input.value,
                unit: input.unit.to_string(),
                provenance_json: input.provenance_json.to_string(),
            };
            if existing == new_row {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "calibration label {} already exists with different metadata",
                input.label_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO calibration_labels (
                label_id,
                metric_family,
                label_source,
                captured_at,
                value,
                unit,
                provenance_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                input.label_id,
                input.metric_family,
                input.label_source,
                input.captured_at,
                input.value,
                input.unit,
                input.provenance_json,
            ],
        )?;
        Ok(true)
    }

    pub fn calibration_label(&self, label_id: &str) -> GooseResult<Option<CalibrationLabelRow>> {
        validate_required("label_id", label_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    label_id,
                    metric_family,
                    label_source,
                    captured_at,
                    value,
                    unit,
                    provenance_json
                FROM calibration_labels
                WHERE label_id = ?1
                "#,
                params![label_id],
                calibration_label_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn calibration_labels_between(
        &self,
        start: &str,
        end: &str,
    ) -> GooseResult<Vec<CalibrationLabelRow>> {
        validate_required("start", start)?;
        validate_required("end", end)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                label_id,
                metric_family,
                label_source,
                captured_at,
                value,
                unit,
                provenance_json
            FROM calibration_labels
            WHERE captured_at >= ?1 AND captured_at < ?2
            ORDER BY captured_at, label_id
            "#,
        )?;
        let rows = statement.query_map(params![start, end], calibration_label_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn upsert_command_validation_record(
        &self,
        record: &CommandValidationRecord,
    ) -> GooseResult<()> {
        validate_required("command", &record.command)?;
        validate_required("risk_gate", &record.risk_gate)?;
        validate_command_report_json(record)?;
        self.conn.execute(
            r#"
            INSERT INTO command_validation_records (
                command,
                risk_gate,
                direct_send_ready,
                report_json
            ) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(command) DO UPDATE SET
                risk_gate = excluded.risk_gate,
                direct_send_ready = excluded.direct_send_ready,
                report_json = excluded.report_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
            params![
                record.command,
                record.risk_gate,
                bool_to_i64(record.direct_send_ready),
                record.report_json,
            ],
        )?;
        Ok(())
    }

    pub fn command_validation_record(
        &self,
        command: &str,
    ) -> GooseResult<Option<CommandValidationRecord>> {
        validate_required("command", command)?;
        self.conn
            .query_row(
                r#"
                SELECT command, risk_gate, direct_send_ready, report_json
                FROM command_validation_records
                WHERE command = ?1
                "#,
                params![command],
                command_validation_record_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn command_validation_records(&self) -> GooseResult<Vec<CommandValidationRecord>> {
        let mut statement = self.conn.prepare(
            r#"
            SELECT command, risk_gate, direct_send_ready, report_json
            FROM command_validation_records
            ORDER BY command
            "#,
        )?;
        let rows = statement.query_map([], command_validation_record_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_debug_session(&self, session: &DebugSessionRow) -> GooseResult<bool> {
        validate_required("session_id", &session.session_id)?;
        validate_required("bridge_url", &session.bridge_url)?;
        validate_required("bind_host", &session.bind_host)?;
        validate_non_negative("started_at_unix_ms", session.started_at_unix_ms)?;

        if let Some(existing) = self.debug_session(&session.session_id)? {
            if existing == *session {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "debug session {} already exists with different metadata",
                session.session_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO debug_sessions (
                session_id,
                started_at_unix_ms,
                bridge_url,
                bind_host,
                token_required,
                token_present,
                remote_bind_enabled,
                visible_remote_bind_toggle
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                session.session_id,
                session.started_at_unix_ms,
                session.bridge_url,
                session.bind_host,
                bool_to_i64(session.token_required),
                bool_to_i64(session.token_present),
                bool_to_i64(session.remote_bind_enabled),
                bool_to_i64(session.visible_remote_bind_toggle),
            ],
        )?;
        Ok(true)
    }

    pub fn debug_session(&self, session_id: &str) -> GooseResult<Option<DebugSessionRow>> {
        validate_required("session_id", session_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    session_id,
                    started_at_unix_ms,
                    bridge_url,
                    bind_host,
                    token_required,
                    token_present,
                    remote_bind_enabled,
                    visible_remote_bind_toggle
                FROM debug_sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                debug_session_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn debug_sessions_between(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> GooseResult<Vec<DebugSessionRow>> {
        validate_non_negative("start_unix_ms", start_unix_ms)?;
        validate_positive("end_unix_ms", end_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                started_at_unix_ms,
                bridge_url,
                bind_host,
                token_required,
                token_present,
                remote_bind_enabled,
                visible_remote_bind_toggle
            FROM debug_sessions
            WHERE started_at_unix_ms >= ?1 AND started_at_unix_ms < ?2
            ORDER BY started_at_unix_ms, session_id
            "#,
        )?;
        let rows =
            statement.query_map(params![start_unix_ms, end_unix_ms], debug_session_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn insert_debug_command(&self, command: &DebugCommandRow) -> GooseResult<bool> {
        validate_required("command_id", &command.command_id)?;
        validate_required("session_id", &command.session_id)?;
        validate_required("schema", &command.schema)?;
        validate_required("command", &command.command)?;
        validate_json_object("args_json", &command.args_json)?;
        validate_non_negative("received_at_unix_ms", command.received_at_unix_ms)?;

        if let Some(existing) = self.debug_command(&command.command_id)? {
            if existing == *command {
                return Ok(false);
            }
            return Err(GooseError::message(format!(
                "debug command {} already exists with different metadata",
                command.command_id
            )));
        }

        self.conn.execute(
            r#"
            INSERT INTO debug_commands (
                command_id,
                session_id,
                schema,
                command,
                args_json,
                dry_run,
                received_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                command.command_id,
                command.session_id,
                command.schema,
                command.command,
                command.args_json,
                bool_to_i64(command.dry_run),
                command.received_at_unix_ms,
            ],
        )?;
        Ok(true)
    }

    pub fn debug_command(&self, command_id: &str) -> GooseResult<Option<DebugCommandRow>> {
        validate_required("command_id", command_id)?;
        self.conn
            .query_row(
                r#"
                SELECT
                    command_id,
                    session_id,
                    schema,
                    command,
                    args_json,
                    dry_run,
                    received_at_unix_ms
                FROM debug_commands
                WHERE command_id = ?1
                "#,
                params![command_id],
                debug_command_from_row,
            )
            .optional()
            .map_err(GooseError::from)
    }

    pub fn debug_commands_for_session(
        &self,
        session_id: &str,
    ) -> GooseResult<Vec<DebugCommandRow>> {
        validate_required("session_id", session_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                command_id,
                session_id,
                schema,
                command,
                args_json,
                dry_run,
                received_at_unix_ms
            FROM debug_commands
            WHERE session_id = ?1
            ORDER BY received_at_unix_ms, command_id
            "#,
        )?;
        let rows = statement.query_map(params![session_id], debug_command_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn debug_commands_between(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> GooseResult<Vec<DebugCommandRow>> {
        validate_non_negative("start_unix_ms", start_unix_ms)?;
        validate_positive("end_unix_ms", end_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                command_id,
                session_id,
                schema,
                command,
                args_json,
                dry_run,
                received_at_unix_ms
            FROM debug_commands
            WHERE received_at_unix_ms >= ?1 AND received_at_unix_ms < ?2
            ORDER BY received_at_unix_ms, command_id
            "#,
        )?;
        let rows =
            statement.query_map(params![start_unix_ms, end_unix_ms], debug_command_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn next_debug_event_sequence(&self, session_id: &str) -> GooseResult<i64> {
        validate_required("session_id", session_id)?;
        if self.debug_session(session_id)?.is_none() {
            return Err(GooseError::message(format!(
                "debug session {session_id} not found"
            )));
        }
        let max_sequence: Option<i64> = self.conn.query_row(
            "SELECT MAX(sequence) FROM debug_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(max_sequence.unwrap_or(0) + 1)
    }

    pub fn insert_debug_event(&self, event: &DebugEventRow) -> GooseResult<bool> {
        validate_required("session_id", &event.session_id)?;
        validate_required("schema", &event.schema)?;
        validate_required("source", &event.source)?;
        validate_required("level", &event.level)?;
        validate_required("topic", &event.topic)?;
        validate_required("message", &event.message)?;
        validate_json_object("data_json", &event.data_json)?;
        validate_positive("sequence", event.sequence)?;
        validate_non_negative("time_unix_ms", event.time_unix_ms)?;

        let previous: Option<(i64, i64)> = self
            .conn
            .query_row(
                r#"
                SELECT sequence, time_unix_ms
                FROM debug_events
                WHERE session_id = ?1
                ORDER BY sequence DESC
                LIMIT 1
                "#,
                params![event.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((previous_sequence, previous_time)) = previous {
            if event.sequence <= previous_sequence {
                return Err(GooseError::message(format!(
                    "debug event sequence {} is not after previous sequence {}",
                    event.sequence, previous_sequence
                )));
            }
            if event.time_unix_ms < previous_time {
                return Err(GooseError::message(format!(
                    "debug event time {} is before previous event time {}",
                    event.time_unix_ms, previous_time
                )));
            }
        }

        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO debug_events (
                session_id,
                sequence,
                schema,
                time_unix_ms,
                source,
                level,
                topic,
                message,
                command_id,
                data_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                event.session_id,
                event.sequence,
                event.schema,
                event.time_unix_ms,
                event.source,
                event.level,
                event.topic,
                event.message,
                event.command_id,
                event.data_json,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn debug_events_for_session(&self, session_id: &str) -> GooseResult<Vec<DebugEventRow>> {
        validate_required("session_id", session_id)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                sequence,
                schema,
                time_unix_ms,
                source,
                level,
                topic,
                message,
                command_id,
                data_json
            FROM debug_events
            WHERE session_id = ?1
            ORDER BY sequence
            "#,
        )?;
        let rows = statement.query_map(params![session_id], debug_event_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn debug_events_between(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> GooseResult<Vec<DebugEventRow>> {
        validate_non_negative("start_unix_ms", start_unix_ms)?;
        validate_positive("end_unix_ms", end_unix_ms)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                sequence,
                schema,
                time_unix_ms,
                source,
                level,
                topic,
                message,
                command_id,
                data_json
            FROM debug_events
            WHERE time_unix_ms >= ?1 AND time_unix_ms < ?2
            ORDER BY time_unix_ms, session_id, sequence
            "#,
        )?;
        let rows =
            statement.query_map(params![start_unix_ms, end_unix_ms], debug_event_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn debug_events_after_sequence(
        &self,
        session_id: &str,
        after_sequence: i64,
        limit: Option<usize>,
    ) -> GooseResult<Vec<DebugEventRow>> {
        validate_required("session_id", session_id)?;
        validate_non_negative("after_sequence", after_sequence)?;
        let limit = i64::try_from(limit.unwrap_or(1000))
            .map_err(|_| GooseError::message("limit is too large"))?;
        validate_positive("limit", limit)?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT
                session_id,
                sequence,
                schema,
                time_unix_ms,
                source,
                level,
                topic,
                message,
                command_id,
                data_json
            FROM debug_events
            WHERE session_id = ?1 AND sequence > ?2
            ORDER BY sequence
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(
            params![session_id, after_sequence, limit],
            debug_event_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }

    pub fn table_count(&self, table: &str) -> GooseResult<i64> {
        if !is_known_table(table) {
            return Err(GooseError::message(format!("unknown table: {table}")));
        }
        let query = format!("SELECT COUNT(*) FROM {table}");
        Ok(self.conn.query_row(&query, [], |row| row.get(0))?)
    }

    pub fn table_columns(&self, table: &str) -> GooseResult<BTreeSet<String>> {
        if !is_known_table(table) {
            return Err(GooseError::message(format!("unknown table: {table}")));
        }
        self.table_columns_unchecked(table)
    }

    pub fn foreign_keys_enabled(&self) -> GooseResult<bool> {
        let enabled: i64 = self
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        Ok(enabled != 0)
    }

    pub fn integrity_check(&self) -> GooseResult<String> {
        self.conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(GooseError::from)
    }
}

impl GooseStore {
    fn ensure_raw_evidence_columns(&self) -> GooseResult<()> {
        let columns = self.table_columns_unchecked("raw_evidence")?;
        for (column, ddl) in [(
            "capture_session_id",
            "capture_session_id TEXT REFERENCES capture_sessions(session_id) ON DELETE SET NULL",
        )] {
            if !columns.contains(column) {
                self.conn
                    .execute(&format!("ALTER TABLE raw_evidence ADD COLUMN {ddl}"), [])?;
            }
        }
        Ok(())
    }

    fn ensure_decoded_frame_columns(&self) -> GooseResult<()> {
        let columns = self.table_columns_unchecked("decoded_frames")?;
        for (column, ddl) in [
            ("packet_type_name", "packet_type_name TEXT"),
            (
                "parsed_payload_json",
                "parsed_payload_json TEXT NOT NULL DEFAULT 'null'",
            ),
        ] {
            if !columns.contains(column) {
                self.conn
                    .execute(&format!("ALTER TABLE decoded_frames ADD COLUMN {ddl}"), [])?;
            }
        }
        Ok(())
    }

    fn ensure_algorithm_definition_columns(&self) -> GooseResult<()> {
        let columns = self.table_columns_unchecked("algorithm_definitions")?;
        for (column, ddl) in [
            ("display_name", "display_name TEXT NOT NULL DEFAULT ''"),
            ("implementation", "implementation TEXT NOT NULL DEFAULT ''"),
            ("license", "license TEXT NOT NULL DEFAULT ''"),
            (
                "input_requirements_json",
                "input_requirements_json TEXT NOT NULL DEFAULT '{}'",
            ),
            (
                "quality_gates_json",
                "quality_gates_json TEXT NOT NULL DEFAULT '[]'",
            ),
            ("status", "status TEXT NOT NULL DEFAULT 'experimental'"),
        ] {
            if !columns.contains(column) {
                self.conn.execute(
                    &format!("ALTER TABLE algorithm_definitions ADD COLUMN {ddl}"),
                    [],
                )?;
            }
        }
        Ok(())
    }

    fn ensure_daily_activity_metric_multi_row_source_kind(&self) -> GooseResult<()> {
        if !self.daily_activity_metrics_has_source_kind_unique_constraint()? {
            return Ok(());
        }

        self.conn.execute_batch(
            r#"
            ALTER TABLE daily_activity_metrics RENAME TO daily_activity_metrics_v12_source_unique;

            CREATE TABLE daily_activity_metrics (
                daily_metric_id TEXT PRIMARY KEY,
                date_key TEXT NOT NULL,
                timezone TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                steps INTEGER,
                active_kcal REAL,
                resting_kcal REAL,
                total_kcal REAL,
                average_cadence_spm REAL,
                source_kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            INSERT INTO daily_activity_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            )
            SELECT
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                steps,
                active_kcal,
                resting_kcal,
                total_kcal,
                average_cadence_spm,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            FROM daily_activity_metrics_v12_source_unique;

            DROP TABLE daily_activity_metrics_v12_source_unique;

            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_date
                ON daily_activity_metrics(date_key);
            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_window
                ON daily_activity_metrics(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_daily_activity_metrics_by_source_kind
                ON daily_activity_metrics(source_kind);
            "#,
        )?;
        Ok(())
    }

    fn daily_activity_metrics_has_source_kind_unique_constraint(&self) -> GooseResult<bool> {
        self.table_has_source_kind_unique_constraint("daily_activity_metrics")
    }

    fn ensure_daily_recovery_metric_multi_row_source_kind(&self) -> GooseResult<()> {
        if !self.daily_recovery_metrics_has_source_kind_unique_constraint()? {
            return Ok(());
        }

        self.conn.execute_batch(
            r#"
            ALTER TABLE daily_recovery_metrics RENAME TO daily_recovery_metrics_source_unique;

            CREATE TABLE daily_recovery_metrics (
                daily_metric_id TEXT PRIMARY KEY,
                date_key TEXT NOT NULL,
                timezone TEXT NOT NULL,
                start_time_unix_ms INTEGER NOT NULL,
                end_time_unix_ms INTEGER NOT NULL,
                resting_hr_bpm REAL,
                hrv_rmssd_ms REAL,
                respiratory_rate_rpm REAL,
                oxygen_saturation_percent REAL,
                skin_temperature_delta_c REAL,
                source_kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                inputs_json TEXT NOT NULL DEFAULT '{}',
                quality_flags_json TEXT NOT NULL DEFAULT '[]',
                provenance_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            INSERT INTO daily_recovery_metrics (
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                resting_hr_bpm,
                hrv_rmssd_ms,
                respiratory_rate_rpm,
                oxygen_saturation_percent,
                skin_temperature_delta_c,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            )
            SELECT
                daily_metric_id,
                date_key,
                timezone,
                start_time_unix_ms,
                end_time_unix_ms,
                resting_hr_bpm,
                hrv_rmssd_ms,
                respiratory_rate_rpm,
                oxygen_saturation_percent,
                skin_temperature_delta_c,
                source_kind,
                confidence,
                inputs_json,
                quality_flags_json,
                provenance_json,
                created_at,
                updated_at
            FROM daily_recovery_metrics_source_unique;

            DROP TABLE daily_recovery_metrics_source_unique;

            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_date
                ON daily_recovery_metrics(date_key);
            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_window
                ON daily_recovery_metrics(start_time_unix_ms, end_time_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_daily_recovery_metrics_by_source_kind
                ON daily_recovery_metrics(source_kind);
            "#,
        )?;
        Ok(())
    }

    fn daily_recovery_metrics_has_source_kind_unique_constraint(&self) -> GooseResult<bool> {
        self.table_has_source_kind_unique_constraint("daily_recovery_metrics")
    }

    fn table_has_source_kind_unique_constraint(&self, table: &str) -> GooseResult<bool> {
        let mut statement = self.conn.prepare(&format!("PRAGMA index_list({table})"))?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
        })?;
        for row in rows {
            let (index_name, unique) = row?;
            if !unique {
                continue;
            }
            let columns = self.index_columns_unchecked(&index_name)?;
            let column_names = columns.iter().map(String::as_str).collect::<Vec<_>>();
            if column_names == ["date_key", "timezone", "source_kind"] {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ensure_step_counter_sample_columns(&self) -> GooseResult<()> {
        let columns = self.table_columns_unchecked("step_counter_samples")?;
        for (column, ddl) in [
            ("cadence_spm", "cadence_spm REAL"),
            ("activity_state", "activity_state TEXT"),
        ] {
            if !columns.contains(column) {
                self.conn.execute(
                    &format!("ALTER TABLE step_counter_samples ADD COLUMN {ddl}"),
                    [],
                )?;
            }
        }
        Ok(())
    }

    fn table_columns_unchecked(&self, table: &str) -> GooseResult<BTreeSet<String>> {
        let mut statement = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        rows.collect::<Result<BTreeSet<_>, _>>()
            .map_err(GooseError::from)
    }

    fn index_columns_unchecked(&self, index_name: &str) -> GooseResult<Vec<String>> {
        let escaped = index_name.replace('\'', "''");
        let mut statement = self
            .conn
            .prepare(&format!("PRAGMA index_info('{escaped}')"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(2))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(GooseError::from)
    }
}

fn finite_json_number(value: &Value) -> Option<f64> {
    let value = value.as_f64()?;
    value.is_finite().then_some(value)
}

fn metric_output_unit(name: &str) -> &'static str {
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
}

fn validate_required(name: &str, value: &str) -> GooseResult<()> {
    if value.trim().is_empty() {
        Err(GooseError::message(format!("{name} is required")))
    } else {
        Ok(())
    }
}

fn validate_optional_required(name: &str, value: Option<&str>) -> GooseResult<()> {
    if let Some(value) = value {
        validate_required(name, value)?;
    }
    Ok(())
}

fn validate_json(name: &str, value: &str) -> GooseResult<()> {
    serde_json::from_str::<serde_json::Value>(value)
        .map_err(|error| GooseError::message(format!("{name} must be valid JSON: {error}")))?;
    Ok(())
}

fn validate_command_report_json(record: &CommandValidationRecord) -> GooseResult<()> {
    let parsed = serde_json::from_str::<serde_json::Value>(&record.report_json)
        .map_err(|error| GooseError::message(format!("report_json must be valid JSON: {error}")))?;
    let Some(report_command) = parsed.get("command").and_then(serde_json::Value::as_str) else {
        return Err(GooseError::message("report_json must contain command"));
    };
    if report_command != record.command {
        return Err(GooseError::message(format!(
            "report_json command {report_command} does not match record command {}",
            record.command
        )));
    }

    let Some(report_risk_gate) = parsed.get("risk_gate").and_then(serde_json::Value::as_str) else {
        return Err(GooseError::message("report_json must contain risk_gate"));
    };
    if report_risk_gate != record.risk_gate {
        return Err(GooseError::message(format!(
            "report_json risk_gate {report_risk_gate} does not match record risk_gate {}",
            record.risk_gate
        )));
    }

    let Some(report_ready) = parsed
        .get("direct_send_ready")
        .and_then(serde_json::Value::as_bool)
    else {
        return Err(GooseError::message(
            "report_json must contain direct_send_ready",
        ));
    };
    if report_ready != record.direct_send_ready {
        return Err(GooseError::message(format!(
            "report_json direct_send_ready {report_ready} does not match record direct_send_ready {}",
            record.direct_send_ready
        )));
    }
    Ok(())
}

fn validate_json_object(name: &str, value: &str) -> GooseResult<()> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)
        .map_err(|error| GooseError::message(format!("{name} must be valid JSON: {error}")))?;
    if !parsed.is_object() {
        return Err(GooseError::message(format!("{name} must be a JSON object")));
    }
    Ok(())
}

fn validate_no_official_whoop_label_marker(name: &str, value: &str) -> GooseResult<()> {
    let parsed = serde_json::from_str::<Value>(value)
        .map_err(|error| GooseError::message(format!("{name} must be valid JSON: {error}")))?;
    if value_contains_official_whoop_label_marker(&parsed) {
        return Err(GooseError::message(format!(
            "{name} must not contain official WHOOP label markers for formatted local metrics",
        )));
    }
    Ok(())
}

fn validate_no_official_whoop_label_text(name: &str, value: &str) -> GooseResult<()> {
    if is_official_whoop_label_token(value) {
        return Err(GooseError::message(format!(
            "{name} must not identify official WHOOP labels as a formatted metric source",
        )));
    }
    Ok(())
}

fn validate_no_platform_metric_source_marker(name: &str, value: &str) -> GooseResult<()> {
    let parsed = serde_json::from_str::<Value>(value)
        .map_err(|error| GooseError::message(format!("{name} must be valid JSON: {error}")))?;
    if value_contains_platform_metric_source_marker(&parsed, None) {
        return Err(GooseError::message(format!(
            "{name} must not contain HealthKit, Health Connect, Apple Health, or platform-import markers as formatted metric sources",
        )));
    }
    Ok(())
}

fn validate_no_platform_metric_source_text(name: &str, value: &str) -> GooseResult<()> {
    if is_platform_metric_source_token(value, None) {
        return Err(GooseError::message(format!(
            "{name} must not identify HealthKit, Health Connect, Apple Health, or platform imports as a formatted metric source",
        )));
    }
    Ok(())
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
    // The official-label compliance policy declaration explicitly documents that
    // official WHOOP values are validation labels, never metric inputs. It is
    // compliance metadata, not a source-identity claim, so it must not trip the
    // marker guard even though it shares the `official_whoop_` prefix.
    if normalized == normalized_marker(OFFICIAL_WHOOP_LABEL_POLICY) {
        return false;
    }
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

fn validate_external_sleep_stage_summary_json(value: &str) -> GooseResult<()> {
    let parsed = serde_json::from_str::<serde_json::Value>(value).map_err(|error| {
        GooseError::message(format!("stage_summary_json must be valid JSON: {error}"))
    })?;
    let Some(object) = parsed.as_object() else {
        return Err(GooseError::message(
            "stage_summary_json must be a JSON object",
        ));
    };
    if object.is_empty() {
        return Ok(());
    }
    let Some(minutes_by_stage) = object
        .get("minutes_by_stage")
        .and_then(serde_json::Value::as_object)
    else {
        return Err(GooseError::message(
            "stage_summary_json must contain minutes_by_stage object",
        ));
    };
    if minutes_by_stage.is_empty() {
        return Err(GooseError::message(
            "stage_summary_json minutes_by_stage must not be empty",
        ));
    }
    for (stage, minutes) in minutes_by_stage {
        if stage.trim().is_empty() {
            return Err(GooseError::message(
                "stage_summary_json stage names must not be empty",
            ));
        }
        validate_external_sleep_stage_summary_key(stage)?;
        let Some(minutes) = minutes.as_f64() else {
            return Err(GooseError::message(format!(
                "stage_summary_json minutes_by_stage.{stage} must be a number",
            )));
        };
        if !minutes.is_finite() || minutes < 0.0 {
            return Err(GooseError::message(format!(
                "stage_summary_json minutes_by_stage.{stage} must be finite and non-negative",
            )));
        }
    }
    Ok(())
}

fn validate_external_sleep_stage_summary_key(stage: &str) -> GooseResult<()> {
    let normalized = stage.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if ALLOWED_EXTERNAL_SLEEP_STAGE_SUMMARY_KEYS.contains(&normalized.as_str()) {
        Ok(())
    } else {
        Err(GooseError::message(format!(
            "stage_summary_json minutes_by_stage.{stage} stage must be recognized"
        )))
    }
}

fn validate_non_negative(name: &str, value: i64) -> GooseResult<()> {
    if value < 0 {
        Err(GooseError::message(format!("{name} must be non-negative")))
    } else {
        Ok(())
    }
}

fn validate_optional_non_negative_i64(name: &str, value: Option<i64>) -> GooseResult<()> {
    if let Some(value) = value {
        validate_non_negative(name, value)?;
    }
    Ok(())
}

fn validate_optional_finite_f64(name: &str, value: Option<f64>) -> GooseResult<()> {
    if let Some(value) = value {
        if !value.is_finite() {
            return Err(GooseError::message(format!("{name} must be finite")));
        }
    }
    Ok(())
}

fn validate_optional_non_negative_f64(name: &str, value: Option<f64>) -> GooseResult<()> {
    if let Some(value) = value {
        if !value.is_finite() || value < 0.0 {
            return Err(GooseError::message(format!(
                "{name} must be finite and non-negative",
            )));
        }
    }
    Ok(())
}

fn validate_positive(name: &str, value: i64) -> GooseResult<()> {
    if value <= 0 {
        Err(GooseError::message(format!("{name} must be positive")))
    } else {
        Ok(())
    }
}

fn validate_window_order(start_time_unix_ms: i64, end_time_unix_ms: i64) -> GooseResult<()> {
    if end_time_unix_ms <= start_time_unix_ms {
        Err(GooseError::message(
            "end_time_unix_ms must be greater than start_time_unix_ms",
        ))
    } else {
        Ok(())
    }
}

fn validate_allowed(name: &str, value: &str, allowed: &[&str]) -> GooseResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(GooseError::message(format!(
            "{name} must be one of: {}",
            allowed.join(", ")
        )))
    }
}

fn validate_activity_type(activity_type: &str) -> GooseResult<()> {
    validate_allowed("activity_type", activity_type, &ALLOWED_ACTIVITY_TYPES)
}

fn validate_sync_status(sync_status: &str) -> GooseResult<()> {
    validate_allowed("sync_status", sync_status, &ALLOWED_ACTIVITY_SYNC_STATUSES)
}

fn validate_activity_detection_method(detection_method: &str) -> GooseResult<()> {
    validate_allowed(
        "detection_method",
        detection_method,
        &ALLOWED_ACTIVITY_DETECTION_METHODS,
    )
}

fn validate_activity_interval_type(interval_type: &str) -> GooseResult<()> {
    validate_allowed(
        "interval_type",
        interval_type,
        &ALLOWED_ACTIVITY_INTERVAL_TYPES,
    )
}

fn validate_activity_label_type(label_type: &str) -> GooseResult<()> {
    validate_allowed("label_type", label_type, &ALLOWED_ACTIVITY_LABEL_TYPES)
}

fn validate_activity_metric_unit(unit: &str) -> GooseResult<()> {
    validate_allowed("unit", unit, &ALLOWED_ACTIVITY_METRIC_UNITS)
}

fn validate_metric_source_kind(source_kind: &str) -> GooseResult<()> {
    validate_allowed("source_kind", source_kind, &ALLOWED_METRIC_SOURCE_KINDS)
}

fn validate_metric_provenance_scope(metric_scope: &str) -> GooseResult<()> {
    validate_allowed(
        "metric_scope",
        metric_scope,
        &ALLOWED_METRIC_PROVENANCE_SCOPES,
    )
}

fn validate_external_sleep_platform(platform: &str) -> GooseResult<()> {
    validate_allowed("platform", platform, &ALLOWED_EXTERNAL_SLEEP_PLATFORMS)
}

fn validate_external_sleep_stage_kind(stage_kind: &str) -> GooseResult<()> {
    validate_allowed(
        "stage_kind",
        stage_kind,
        &ALLOWED_EXTERNAL_SLEEP_STAGE_KINDS,
    )
}

fn validate_sleep_correction_label_type(label_type: &str) -> GooseResult<()> {
    validate_allowed(
        "label_type",
        label_type,
        &ALLOWED_SLEEP_CORRECTION_LABEL_TYPES,
    )
}

fn validate_confidence(name: &str, confidence: f64) -> GooseResult<()> {
    if !confidence.is_finite() {
        return Err(GooseError::message(format!("{name} must be finite")));
    }
    if !(0.0..=1.0).contains(&confidence) {
        return Err(GooseError::message(format!(
            "{name} must be between 0.0 and 1.0",
        )));
    }
    Ok(())
}

fn validate_unavailable_metric_confidence(source_kind: &str, confidence: f64) -> GooseResult<()> {
    if source_kind == "unavailable" && confidence != 0.0 {
        return Err(GooseError::message(
            "unavailable formatted metrics must have confidence 0.0",
        ));
    }
    Ok(())
}

fn validate_unavailable_metric_provenance_confidence(
    source_kind: &str,
    confidence: Option<f64>,
) -> GooseResult<()> {
    if source_kind == "unavailable" && confidence.unwrap_or(0.0) != 0.0 {
        return Err(GooseError::message(
            "unavailable metric provenance must have confidence 0.0",
        ));
    }
    Ok(())
}

fn validate_activity_session_input(
    _store: &GooseStore,
    input: &ActivitySessionInput<'_>,
) -> GooseResult<()> {
    validate_required("session_id", input.session_id)?;
    validate_required("source", input.source)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_required("activity_type", input.activity_type)?;
    validate_activity_type(input.activity_type)?;
    validate_optional_required(
        "external_activity_type_code",
        input.external_activity_type_code,
    )?;
    validate_optional_required(
        "external_activity_type_name",
        input.external_activity_type_name,
    )?;
    validate_optional_required("custom_label", input.custom_label)?;
    validate_confidence("confidence", input.confidence)?;
    validate_required("detection_method", input.detection_method)?;
    validate_activity_detection_method(input.detection_method)?;
    validate_required("sync_status", input.sync_status)?;
    validate_sync_status(input.sync_status)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_activity_metric_input(
    _store: &GooseStore,
    input: &ActivityMetricInput<'_>,
) -> GooseResult<()> {
    validate_required("metric_id", input.metric_id)?;
    validate_required("activity_session_id", input.activity_session_id)?;
    validate_required("metric_name", input.metric_name)?;
    if !input.value.is_finite() {
        return Err(GooseError::message("value must be finite"));
    }
    validate_required("unit", input.unit)?;
    validate_activity_metric_unit(input.unit)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_daily_activity_metric_input(input: &DailyActivityMetricInput<'_>) -> GooseResult<()> {
    validate_required("daily_metric_id", input.daily_metric_id)?;
    validate_required("date_key", input.date_key)?;
    validate_required("timezone", input.timezone)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_optional_non_negative_i64("steps", input.steps)?;
    validate_optional_non_negative_f64("active_kcal", input.active_kcal)?;
    validate_optional_non_negative_f64("resting_kcal", input.resting_kcal)?;
    validate_optional_non_negative_f64("total_kcal", input.total_kcal)?;
    validate_optional_non_negative_f64("average_cadence_spm", input.average_cadence_spm)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    validate_confidence("confidence", input.confidence)?;
    validate_unavailable_metric_confidence(input.source_kind, input.confidence)?;
    validate_activity_formatted_metric_values(
        input.source_kind,
        input.steps,
        input.active_kcal,
        input.resting_kcal,
        input.total_kcal,
        input.average_cadence_spm,
    )?;
    validate_json_object("inputs_json", input.inputs_json)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    validate_no_official_whoop_label_marker("inputs_json", input.inputs_json)?;
    validate_no_official_whoop_label_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_official_whoop_label_marker("provenance_json", input.provenance_json)?;
    validate_no_platform_metric_source_marker("inputs_json", input.inputs_json)?;
    validate_no_platform_metric_source_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_platform_metric_source_marker("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_hourly_activity_metric_input(input: &HourlyActivityMetricInput<'_>) -> GooseResult<()> {
    validate_required("hourly_metric_id", input.hourly_metric_id)?;
    validate_required("date_key", input.date_key)?;
    validate_required("timezone", input.timezone)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_optional_non_negative_i64("steps", input.steps)?;
    validate_optional_non_negative_f64("active_kcal", input.active_kcal)?;
    validate_optional_non_negative_f64("resting_kcal", input.resting_kcal)?;
    validate_optional_non_negative_f64("total_kcal", input.total_kcal)?;
    validate_optional_non_negative_f64("average_cadence_spm", input.average_cadence_spm)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    validate_confidence("confidence", input.confidence)?;
    validate_unavailable_metric_confidence(input.source_kind, input.confidence)?;
    validate_activity_formatted_metric_values(
        input.source_kind,
        input.steps,
        input.active_kcal,
        input.resting_kcal,
        input.total_kcal,
        input.average_cadence_spm,
    )?;
    validate_json_object("inputs_json", input.inputs_json)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    validate_no_official_whoop_label_marker("inputs_json", input.inputs_json)?;
    validate_no_official_whoop_label_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_official_whoop_label_marker("provenance_json", input.provenance_json)?;
    validate_no_platform_metric_source_marker("inputs_json", input.inputs_json)?;
    validate_no_platform_metric_source_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_platform_metric_source_marker("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_daily_recovery_metric_input(input: &DailyRecoveryMetricInput<'_>) -> GooseResult<()> {
    validate_required("daily_metric_id", input.daily_metric_id)?;
    validate_required("date_key", input.date_key)?;
    validate_required("timezone", input.timezone)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_optional_non_negative_f64("resting_hr_bpm", input.resting_hr_bpm)?;
    validate_optional_non_negative_f64("hrv_rmssd_ms", input.hrv_rmssd_ms)?;
    validate_optional_non_negative_f64("respiratory_rate_rpm", input.respiratory_rate_rpm)?;
    validate_optional_non_negative_f64(
        "oxygen_saturation_percent",
        input.oxygen_saturation_percent,
    )?;
    validate_optional_finite_f64("skin_temperature_delta_c", input.skin_temperature_delta_c)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    validate_confidence("confidence", input.confidence)?;
    validate_unavailable_metric_confidence(input.source_kind, input.confidence)?;
    validate_recovery_formatted_metric_values(
        input.source_kind,
        input.resting_hr_bpm,
        input.hrv_rmssd_ms,
        input.respiratory_rate_rpm,
        input.oxygen_saturation_percent,
        input.skin_temperature_delta_c,
    )?;
    validate_json_object("inputs_json", input.inputs_json)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    validate_no_official_whoop_label_marker("inputs_json", input.inputs_json)?;
    validate_no_official_whoop_label_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_official_whoop_label_marker("provenance_json", input.provenance_json)?;
    validate_no_platform_metric_source_marker("inputs_json", input.inputs_json)?;
    validate_no_platform_metric_source_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_platform_metric_source_marker("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_activity_formatted_metric_values(
    source_kind: &str,
    steps: Option<i64>,
    active_kcal: Option<f64>,
    resting_kcal: Option<f64>,
    total_kcal: Option<f64>,
    average_cadence_spm: Option<f64>,
) -> GooseResult<()> {
    let has_metric_value =
        steps.is_some() || active_kcal.is_some() || resting_kcal.is_some() || total_kcal.is_some();
    let has_any_value = has_metric_value || average_cadence_spm.is_some();
    if source_kind == "unavailable" {
        if has_any_value {
            return Err(GooseError::message(
                "unavailable activity metrics must not carry metric values",
            ));
        }
    } else if !has_metric_value {
        return Err(GooseError::message(
            "available activity metrics must include steps or calorie values",
        ));
    }
    Ok(())
}

fn validate_recovery_formatted_metric_values(
    source_kind: &str,
    resting_hr_bpm: Option<f64>,
    hrv_rmssd_ms: Option<f64>,
    respiratory_rate_rpm: Option<f64>,
    oxygen_saturation_percent: Option<f64>,
    skin_temperature_delta_c: Option<f64>,
) -> GooseResult<()> {
    let has_metric_value = resting_hr_bpm.is_some()
        || hrv_rmssd_ms.is_some()
        || respiratory_rate_rpm.is_some()
        || oxygen_saturation_percent.is_some()
        || skin_temperature_delta_c.is_some();
    if source_kind == "unavailable" {
        if has_metric_value {
            return Err(GooseError::message(
                "unavailable recovery metrics must not carry metric values",
            ));
        }
    } else if !has_metric_value {
        return Err(GooseError::message(
            "available recovery metrics must include at least one recovery value",
        ));
    }
    Ok(())
}

fn validate_metric_provenance_input(
    store: &GooseStore,
    input: &MetricProvenanceInput<'_>,
) -> GooseResult<()> {
    validate_required("provenance_id", input.provenance_id)?;
    validate_required("metric_scope", input.metric_scope)?;
    validate_metric_provenance_scope(input.metric_scope)?;
    validate_required("metric_id", input.metric_id)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    validate_required("source_detail", input.source_detail)?;
    validate_no_official_whoop_label_text("source_detail", input.source_detail)?;
    validate_no_platform_metric_source_text("source_detail", input.source_detail)?;
    if let Some(confidence) = input.confidence {
        validate_confidence("confidence", confidence)?;
    }
    validate_unavailable_metric_provenance_confidence(input.source_kind, input.confidence)?;
    validate_json_object("inputs_json", input.inputs_json)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    validate_no_official_whoop_label_marker("inputs_json", input.inputs_json)?;
    validate_no_official_whoop_label_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_official_whoop_label_marker("provenance_json", input.provenance_json)?;
    validate_no_platform_metric_source_marker("inputs_json", input.inputs_json)?;
    validate_no_platform_metric_source_marker("quality_flags_json", input.quality_flags_json)?;
    validate_no_platform_metric_source_marker("provenance_json", input.provenance_json)?;
    validate_metric_provenance_target(store, input)?;
    Ok(())
}

fn validate_metric_provenance_target(
    store: &GooseStore,
    input: &MetricProvenanceInput<'_>,
) -> GooseResult<()> {
    let metric_source_kind = match input.metric_scope {
        "daily_activity" => store
            .daily_activity_metric(input.metric_id)?
            .map(|metric| metric.source_kind)
            .ok_or_else(|| {
                GooseError::message(
                    "metric_provenance metric_id must reference existing daily_activity metric",
                )
            })?,
        "daily_recovery" => store
            .daily_recovery_metric(input.metric_id)?
            .map(|metric| metric.source_kind)
            .ok_or_else(|| {
                GooseError::message(
                    "metric_provenance metric_id must reference existing daily_recovery metric",
                )
            })?,
        "hourly_activity" => store
            .hourly_activity_metric(input.metric_id)?
            .map(|metric| metric.source_kind)
            .ok_or_else(|| {
                GooseError::message(
                    "metric_provenance metric_id must reference existing hourly_activity metric",
                )
            })?,
        _ => unreachable!("metric_scope was validated before target lookup"),
    };
    if metric_source_kind != input.source_kind {
        return Err(GooseError::message(format!(
            "metric_provenance source_kind must match {} metric source_kind",
            input.metric_scope
        )));
    }
    Ok(())
}

fn validate_metric_debug_feature_input(input: &MetricDebugFeatureInput<'_>) -> GooseResult<()> {
    validate_required("feature_id", input.feature_id)?;
    validate_required("metric_family", input.metric_family)?;
    validate_required("feature_name", input.feature_name)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    if let Some(confidence) = input.confidence {
        validate_confidence("confidence", confidence)?;
    }
    validate_json_object("feature_json", input.feature_json)?;
    validate_json_object("inputs_json", input.inputs_json)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_step_counter_sample_input(input: &StepCounterSampleInput<'_>) -> GooseResult<()> {
    validate_required("sample_id", input.sample_id)?;
    validate_non_negative("sample_time_unix_ms", input.sample_time_unix_ms)?;
    validate_non_negative("counter_value", input.counter_value)?;
    validate_optional_non_negative_f64("cadence_spm", input.cadence_spm)?;
    validate_optional_required("activity_state", input.activity_state)?;
    validate_required("source_kind", input.source_kind)?;
    validate_metric_source_kind(input.source_kind)?;
    if input.source_kind != "device_counter" {
        return Err(GooseError::message(
            "source_kind for step_counter_samples must be device_counter",
        ));
    }
    validate_required("packet_family", input.packet_family)?;
    validate_required("json_path", input.json_path)?;
    validate_optional_required("frame_id", input.frame_id)?;
    validate_optional_required("evidence_id", input.evidence_id)?;
    validate_optional_required("capture_session_id", input.capture_session_id)?;
    validate_json("quality_flags_json", input.quality_flags_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_activity_interval_input(
    _store: &GooseStore,
    input: &ActivityIntervalInput<'_>,
) -> GooseResult<()> {
    validate_required("interval_id", input.interval_id)?;
    validate_required("activity_session_id", input.activity_session_id)?;
    validate_required("interval_type", input.interval_type)?;
    validate_activity_interval_type(input.interval_type)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_non_negative("sequence", input.sequence)?;
    validate_json_object("metadata_json", input.metadata_json)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_activity_label_input(
    _store: &GooseStore,
    input: &ActivityLabelInput<'_>,
) -> GooseResult<()> {
    validate_required("label_id", input.label_id)?;
    validate_required("activity_session_id", input.activity_session_id)?;
    validate_required("label_type", input.label_type)?;
    validate_activity_label_type(input.label_type)?;
    validate_required("value", input.value)?;
    validate_required("source", input.source)?;
    if let Some(confidence) = input.confidence {
        validate_confidence("confidence", confidence)?;
    }
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_external_sleep_session_input(input: &ExternalSleepSessionInput<'_>) -> GooseResult<()> {
    validate_required("sleep_id", input.sleep_id)?;
    validate_required("source", input.source)?;
    validate_required("platform", input.platform)?;
    validate_external_sleep_platform(input.platform)?;
    validate_optional_required("platform_record_id", input.platform_record_id)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_optional_required("timezone", input.timezone)?;
    validate_external_sleep_stage_summary_json(input.stage_summary_json)?;
    validate_confidence("confidence", input.confidence)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_external_sleep_stage_input(
    store: &GooseStore,
    input: &ExternalSleepStageInput<'_>,
) -> GooseResult<()> {
    validate_required("stage_id", input.stage_id)?;
    validate_required("sleep_id", input.sleep_id)?;
    let Some(session) = store.external_sleep_session(input.sleep_id)? else {
        return Err(GooseError::message(format!(
            "external sleep session {} not found",
            input.sleep_id
        )));
    };
    validate_required("stage_kind", input.stage_kind)?;
    validate_external_sleep_stage_kind(input.stage_kind)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    if input.start_time_unix_ms < session.start_time_unix_ms
        || input.end_time_unix_ms > session.end_time_unix_ms
    {
        return Err(GooseError::message(format!(
            "external sleep stage {} must be within parent sleep session {}",
            input.stage_id, input.sleep_id
        )));
    }
    validate_confidence("confidence", input.confidence)?;
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn validate_sleep_correction_label_input(input: &SleepCorrectionLabelInput<'_>) -> GooseResult<()> {
    validate_required("label_id", input.label_id)?;
    validate_optional_required("sleep_id", input.sleep_id)?;
    validate_required("label_type", input.label_type)?;
    validate_sleep_correction_label_type(input.label_type)?;
    validate_non_negative("start_time_unix_ms", input.start_time_unix_ms)?;
    validate_non_negative("end_time_unix_ms", input.end_time_unix_ms)?;
    validate_window_order(input.start_time_unix_ms, input.end_time_unix_ms)?;
    validate_json_object("value_json", input.value_json)?;
    validate_required("source", input.source)?;
    if let Some(confidence) = input.confidence {
        validate_confidence("confidence", confidence)?;
    }
    validate_json_object("provenance_json", input.provenance_json)?;
    Ok(())
}

fn is_allowed_calibration_label_source(source: &str) -> bool {
    matches!(
        source,
        "manual" | "passive_official_capture" | "user_export" | "screenshot_import" | "synthetic"
    )
}

fn algorithm_preference_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AlgorithmPreferenceRecord> {
    Ok(AlgorithmPreferenceRecord {
        scope: row.get(0)?,
        metric_family: row.get(1)?,
        algorithm_id: row.get(2)?,
        version: row.get(3)?,
    })
}

fn decoded_frame_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecodedFrameRow> {
    Ok(DecodedFrameRow {
        frame_id: row.get(0)?,
        evidence_id: row.get(1)?,
        captured_at: row.get(2)?,
        device_type: row.get(3)?,
        raw_len: row.get(4)?,
        header_len: row.get(5)?,
        declared_len: row.get(6)?,
        payload_hex: row.get(7)?,
        payload_crc_hex: row.get(8)?,
        header_crc_valid: row.get::<_, i64>(9)? != 0,
        payload_crc_valid: row.get::<_, i64>(10)? != 0,
        packet_type: row.get(11)?,
        packet_type_name: row.get(12)?,
        sequence: row.get(13)?,
        command_or_event: row.get(14)?,
        parsed_payload_json: row.get(15)?,
        parser_version: row.get(16)?,
        warnings_json: row.get(17)?,
    })
}

fn command_validation_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<CommandValidationRecord> {
    Ok(CommandValidationRecord {
        command: row.get(0)?,
        risk_gate: row.get(1)?,
        direct_send_ready: i64_to_bool(row.get(2)?),
        report_json: row.get(3)?,
    })
}

fn calibration_label_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CalibrationLabelRow> {
    Ok(CalibrationLabelRow {
        label_id: row.get(0)?,
        metric_family: row.get(1)?,
        label_source: row.get(2)?,
        captured_at: row.get(3)?,
        value: row.get(4)?,
        unit: row.get(5)?,
        provenance_json: row.get(6)?,
    })
}

fn capture_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureSessionRow> {
    Ok(CaptureSessionRow {
        session_id: row.get(0)?,
        source: row.get(1)?,
        started_at_unix_ms: row.get(2)?,
        ended_at_unix_ms: row.get(3)?,
        device_model: row.get(4)?,
        active_device_id: row.get(5)?,
        status: row.get(6)?,
        frame_count: row.get(7)?,
        provenance_json: row.get(8)?,
    })
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn activity_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivitySessionRow> {
    Ok(ActivitySessionRow {
        session_id: row.get(0)?,
        source: row.get(1)?,
        start_time_unix_ms: row.get(2)?,
        end_time_unix_ms: row.get(3)?,
        duration_ms: row.get(4)?,
        activity_type: row.get(5)?,
        external_activity_type_code: row.get(6)?,
        external_activity_type_name: row.get(7)?,
        custom_label: row.get(8)?,
        confidence: row.get(9)?,
        detection_method: row.get(10)?,
        sync_status: row.get(11)?,
        provenance_json: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn activity_metric_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityMetricRow> {
    Ok(ActivityMetricRow {
        metric_id: row.get(0)?,
        activity_session_id: row.get(1)?,
        metric_name: row.get(2)?,
        value: row.get(3)?,
        unit: row.get(4)?,
        start_time_unix_ms: row.get(5)?,
        end_time_unix_ms: row.get(6)?,
        quality_flags_json: row.get(7)?,
        provenance_json: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn daily_activity_metric_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<DailyActivityMetricRow> {
    Ok(DailyActivityMetricRow {
        daily_metric_id: row.get(0)?,
        date_key: row.get(1)?,
        timezone: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        steps: row.get(5)?,
        active_kcal: row.get(6)?,
        resting_kcal: row.get(7)?,
        total_kcal: row.get(8)?,
        average_cadence_spm: row.get(9)?,
        source_kind: row.get(10)?,
        confidence: row.get(11)?,
        inputs_json: row.get(12)?,
        quality_flags_json: row.get(13)?,
        provenance_json: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn hourly_activity_metric_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<HourlyActivityMetricRow> {
    Ok(HourlyActivityMetricRow {
        hourly_metric_id: row.get(0)?,
        date_key: row.get(1)?,
        timezone: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        steps: row.get(5)?,
        active_kcal: row.get(6)?,
        resting_kcal: row.get(7)?,
        total_kcal: row.get(8)?,
        average_cadence_spm: row.get(9)?,
        source_kind: row.get(10)?,
        confidence: row.get(11)?,
        inputs_json: row.get(12)?,
        quality_flags_json: row.get(13)?,
        provenance_json: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn daily_recovery_metric_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<DailyRecoveryMetricRow> {
    Ok(DailyRecoveryMetricRow {
        daily_metric_id: row.get(0)?,
        date_key: row.get(1)?,
        timezone: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        resting_hr_bpm: row.get(5)?,
        hrv_rmssd_ms: row.get(6)?,
        respiratory_rate_rpm: row.get(7)?,
        oxygen_saturation_percent: row.get(8)?,
        skin_temperature_delta_c: row.get(9)?,
        source_kind: row.get(10)?,
        confidence: row.get(11)?,
        inputs_json: row.get(12)?,
        quality_flags_json: row.get(13)?,
        provenance_json: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn metric_provenance_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MetricProvenanceRow> {
    Ok(MetricProvenanceRow {
        provenance_id: row.get(0)?,
        metric_scope: row.get(1)?,
        metric_id: row.get(2)?,
        source_kind: row.get(3)?,
        source_detail: row.get(4)?,
        confidence: row.get(5)?,
        inputs_json: row.get(6)?,
        quality_flags_json: row.get(7)?,
        provenance_json: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn metric_debug_feature_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MetricDebugFeatureRow> {
    Ok(MetricDebugFeatureRow {
        feature_id: row.get(0)?,
        metric_family: row.get(1)?,
        feature_name: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        source_kind: row.get(5)?,
        confidence: row.get(6)?,
        feature_json: row.get(7)?,
        inputs_json: row.get(8)?,
        quality_flags_json: row.get(9)?,
        provenance_json: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn step_counter_sample_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepCounterSampleRow> {
    Ok(StepCounterSampleRow {
        sample_id: row.get(0)?,
        sample_time_unix_ms: row.get(1)?,
        counter_value: row.get(2)?,
        cadence_spm: row.get(3)?,
        activity_state: row.get(4)?,
        source_kind: row.get(5)?,
        packet_family: row.get(6)?,
        json_path: row.get(7)?,
        frame_id: row.get(8)?,
        evidence_id: row.get(9)?,
        capture_session_id: row.get(10)?,
        quality_flags_json: row.get(11)?,
        provenance_json: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn activity_interval_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityIntervalRow> {
    Ok(ActivityIntervalRow {
        interval_id: row.get(0)?,
        activity_session_id: row.get(1)?,
        interval_type: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        duration_ms: row.get(5)?,
        sequence: row.get(6)?,
        metadata_json: row.get(7)?,
        provenance_json: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn activity_label_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityLabelRow> {
    Ok(ActivityLabelRow {
        label_id: row.get(0)?,
        activity_session_id: row.get(1)?,
        label_type: row.get(2)?,
        value: row.get(3)?,
        source: row.get(4)?,
        confidence: row.get(5)?,
        provenance_json: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn external_sleep_session_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ExternalSleepSessionRow> {
    Ok(ExternalSleepSessionRow {
        sleep_id: row.get(0)?,
        source: row.get(1)?,
        platform: row.get(2)?,
        platform_record_id: row.get(3)?,
        start_time_unix_ms: row.get(4)?,
        end_time_unix_ms: row.get(5)?,
        duration_ms: row.get(6)?,
        timezone: row.get(7)?,
        stage_summary_json: row.get(8)?,
        confidence: row.get(9)?,
        provenance_json: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn external_sleep_stage_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ExternalSleepStageRow> {
    Ok(ExternalSleepStageRow {
        stage_id: row.get(0)?,
        sleep_id: row.get(1)?,
        stage_kind: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        duration_ms: row.get(5)?,
        confidence: row.get(6)?,
        provenance_json: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn sleep_correction_label_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SleepCorrectionLabelRow> {
    Ok(SleepCorrectionLabelRow {
        label_id: row.get(0)?,
        sleep_id: row.get(1)?,
        label_type: row.get(2)?,
        start_time_unix_ms: row.get(3)?,
        end_time_unix_ms: row.get(4)?,
        value_json: row.get(5)?,
        source: row.get(6)?,
        confidence: row.get(7)?,
        provenance_json: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn debug_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DebugSessionRow> {
    Ok(DebugSessionRow {
        session_id: row.get(0)?,
        started_at_unix_ms: row.get(1)?,
        bridge_url: row.get(2)?,
        bind_host: row.get(3)?,
        token_required: i64_to_bool(row.get(4)?),
        token_present: i64_to_bool(row.get(5)?),
        remote_bind_enabled: i64_to_bool(row.get(6)?),
        visible_remote_bind_toggle: i64_to_bool(row.get(7)?),
    })
}

fn debug_command_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DebugCommandRow> {
    Ok(DebugCommandRow {
        command_id: row.get(0)?,
        session_id: row.get(1)?,
        schema: row.get(2)?,
        command: row.get(3)?,
        args_json: row.get(4)?,
        dry_run: i64_to_bool(row.get(5)?),
        received_at_unix_ms: row.get(6)?,
    })
}

fn debug_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DebugEventRow> {
    Ok(DebugEventRow {
        session_id: row.get(0)?,
        sequence: row.get(1)?,
        schema: row.get(2)?,
        time_unix_ms: row.get(3)?,
        source: row.get(4)?,
        level: row.get(5)?,
        topic: row.get(6)?,
        message: row.get(7)?,
        command_id: row.get(8)?,
        data_json: row.get(9)?,
    })
}

fn device_type_name(device_type: DeviceType) -> &'static str {
    match device_type {
        DeviceType::Gen4 => "GEN_4",
        DeviceType::Maverick => "MAVERICK",
        DeviceType::Puffin => "PUFFIN",
        DeviceType::Goose => "GOOSE",
    }
}

fn is_known_table(table: &str) -> bool {
    known_tables().contains(&table)
}

pub fn known_tables() -> &'static [&'static str] {
    &[
        "goose_schema_migrations",
        "raw_evidence",
        "decoded_frames",
        "algorithm_definitions",
        "algorithm_runs",
        "metric_values",
        "metric_components",
        "calibration_labels",
        "calibration_runs",
        "algorithm_preferences",
        "command_validation_records",
        "capture_sessions",
        "activity_sessions",
        "activity_metrics",
        "daily_activity_metrics",
        "hourly_activity_metrics",
        "daily_recovery_metrics",
        "metric_provenance",
        "metric_debug_features",
        "step_counter_samples",
        "activity_intervals",
        "activity_labels",
        "external_sleep_sessions",
        "external_sleep_stages",
        "sleep_correction_labels",
        "debug_sessions",
        "debug_commands",
        "debug_events",
    ]
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
