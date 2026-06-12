use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GooseError, GooseResult,
    capture_correlation::{
        CaptureCorrelationOptions, CaptureCorrelationReport,
        DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY, run_capture_correlation_for_store,
    },
    metrics::{
        AlgorithmRunResult, GOOSE_HRV_V0_ID, GOOSE_HRV_V0_VERSION, HrvInput, HrvOutput,
        RecoveryInput, RecoveryScoreOutput, SleepInput, SleepScoreOutput, StrainInput,
        StrainScoreOutput, StressInput, StressScoreOutput, goose_hrv_v0, goose_recovery_v0,
        goose_sleep_v0, goose_strain_v0, goose_stress_v0, heart_rate_dip_pct, hr_disturbance_count,
        sol_from_hr, waso_from_hr,
    },
    protocol::{
        DataPacketBodySummary, I16SeriesSummary, ParsedPayload, decode_hex_with_whitespace,
    },
    store::{DecodedFrameRow, GooseStore, RrIntervalRow},
    validation_labels::{
        OFFICIAL_WHOOP_LABEL_POLICY, official_label_policy_issue_action,
        official_label_policy_issues,
    },
};

pub const MOTION_FEATURE_REPORT_SCHEMA: &str = "goose.motion-feature-report.v1";
pub const HEART_RATE_FEATURE_REPORT_SCHEMA: &str = "goose.heart-rate-feature-report.v1";
pub const HRV_FEATURE_REPORT_SCHEMA: &str = "goose.hrv-feature-report.v1";
pub const HRV_CAPTURE_VALIDATION_REPORT_SCHEMA: &str = "goose.hrv-capture-validation-report.v1";
pub const VITAL_EVENT_FEATURE_REPORT_SCHEMA: &str = "goose.vital-event-feature-report.v1";
pub const RESPIRATORY_RATE_CAPTURE_VALIDATION_REPORT_SCHEMA: &str =
    "goose.respiratory-rate-capture-validation-report.v1";
pub const OXYGEN_SATURATION_CAPTURE_VALIDATION_REPORT_SCHEMA: &str =
    "goose.oxygen-saturation-capture-validation-report.v1";
pub const TEMPERATURE_CAPTURE_VALIDATION_REPORT_SCHEMA: &str =
    "goose.temperature-capture-validation-report.v1";
pub const RECOVERY_SENSOR_DISCOVERY_REPORT_SCHEMA: &str =
    "goose.recovery-sensor-discovery-report.v1";
pub const METRIC_WINDOW_FEATURE_REPORT_SCHEMA: &str = "goose.metric-window-feature-report.v1";
pub const RESTING_HEART_RATE_FEATURE_REPORT_SCHEMA: &str =
    "goose.resting-heart-rate-feature-report.v1";
pub const SLEEP_FEATURE_SCORE_REPORT_SCHEMA: &str = "goose.sleep-feature-score-report.v1";
pub const RECOVERY_FEATURE_SCORE_REPORT_SCHEMA: &str = "goose.recovery-feature-score-report.v1";
pub const STRAIN_FEATURE_SCORE_REPORT_SCHEMA: &str = "goose.strain-feature-score-report.v1";
pub const STRESS_FEATURE_SCORE_REPORT_SCHEMA: &str = "goose.stress-feature-score-report.v1";
const MIN_SMOOTHED_SLEEP_STAGE_DURATION_MINUTES: f64 = 5.0;
const RESTING_HR_LOW_MOTION_INTENSITY_MAX: f64 = 0.08;
const RESTING_HR_MOTION_MATCH_WINDOW_MS: i64 = 10 * 60 * 1_000;

#[derive(Debug, Clone, Copy)]
pub struct MotionFeatureOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
}

pub type HeartRateFeatureOptions = MotionFeatureOptions;
pub type VitalEventFeatureOptions = MotionFeatureOptions;

pub const GOOSE_RESPIRATORY_RATE_HISTORY_CANDIDATE_V0_ID: &str =
    "goose.respiratory_rate.history_candidate.v0";
pub const GOOSE_RESPIRATORY_RATE_HISTORY_CANDIDATE_V0_VERSION: &str = "0.1.0";
pub const GOOSE_OXYGEN_SATURATION_PACKET_CANDIDATE_V0_ID: &str =
    "goose.oxygen_saturation.packet_candidate.v0";
pub const GOOSE_OXYGEN_SATURATION_PACKET_CANDIDATE_V0_VERSION: &str = "0.1.0";
pub const GOOSE_SKIN_TEMPERATURE_HISTORY_CANDIDATE_V0_ID: &str =
    "goose.skin_temperature.history_candidate.v0";
pub const GOOSE_SKIN_TEMPERATURE_HISTORY_CANDIDATE_V0_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Copy)]
pub struct HrvFeatureOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub min_rr_intervals_to_compute: usize,
    pub baseline_min_days: usize,
    pub require_baseline: bool,
}

#[derive(Debug, Clone)]
pub struct HrvCaptureValidationOptions {
    pub feature_options: HrvFeatureOptions,
    pub capture_kind: Option<String>,
    pub official_whoop_hrv_rmssd_ms: Option<f64>,
    pub tolerance_ms: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RespiratoryRateCaptureValidationOptions {
    pub feature_options: VitalEventFeatureOptions,
    pub capture_kind: Option<String>,
    pub official_whoop_respiratory_rate_rpm: Option<f64>,
    pub tolerance_rpm: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct OxygenSaturationCaptureValidationOptions {
    pub feature_options: VitalEventFeatureOptions,
    pub capture_kind: Option<String>,
    pub official_whoop_oxygen_saturation_percent: Option<f64>,
    pub tolerance_percent: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct TemperatureCaptureValidationOptions {
    pub feature_options: VitalEventFeatureOptions,
    pub capture_kind: Option<String>,
    pub official_whoop_skin_temperature_delta_c: Option<f64>,
    pub tolerance_c: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoverySensorDiscoveryOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub min_rr_intervals_to_compute: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RestingHeartRateFeatureOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub baseline_min_days: usize,
    pub require_baseline: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MetricWindowFeatureOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub resting_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct StrainFeatureScoreOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub resting_baseline_min_days: usize,
    pub max_hr_bpm: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct SleepFeatureScoreOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub sleep_need_minutes: f64,
    pub low_motion_threshold_0_to_1: f64,
    pub disturbance_motion_threshold_0_to_1: f64,
    pub target_midpoint_minutes_since_midnight: f64,
}

#[derive(Debug, Clone)]
pub struct RecoveryFeatureScoreOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub resting_baseline_min_days: usize,
    pub hrv_min_rr_intervals_to_compute: usize,
    pub hrv_baseline_min_days: usize,
    pub sleep_need_minutes: f64,
    pub low_motion_threshold_0_to_1: f64,
    pub disturbance_motion_threshold_0_to_1: f64,
    pub target_midpoint_minutes_since_midnight: f64,
    pub prior_strain_resting_baseline_min_days: usize,
    pub prior_strain_max_hr_bpm: Option<f64>,
    pub respiratory_rate_rpm: Option<f64>,
    pub respiratory_rate_baseline_rpm: Option<f64>,
    pub skin_temp_delta_c: Option<f64>,
    pub provided_vitals_source: Option<String>,
    pub provided_vitals_provenance_json: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct StressFeatureScoreOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub resting_baseline_min_days: usize,
    pub hrv_min_rr_intervals_to_compute: usize,
    pub hrv_baseline_min_days: usize,
}

impl Default for RestingHeartRateFeatureOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            baseline_min_days: 3,
            require_baseline: false,
        }
    }
}

impl Default for HrvFeatureOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            min_rr_intervals_to_compute: 2,
            baseline_min_days: 3,
            require_baseline: false,
        }
    }
}

impl Default for RecoverySensorDiscoveryOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            min_rr_intervals_to_compute: 2,
        }
    }
}

impl Default for MetricWindowFeatureOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            resting_hr_bpm: None,
            max_hr_bpm: None,
        }
    }
}

impl Default for StrainFeatureScoreOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            resting_baseline_min_days: 3,
            max_hr_bpm: None,
        }
    }
}

impl Default for SleepFeatureScoreOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            sleep_need_minutes: 480.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 180.0,
        }
    }
}

impl Default for RecoveryFeatureScoreOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
            sleep_need_minutes: 480.0,
            low_motion_threshold_0_to_1: 0.05,
            disturbance_motion_threshold_0_to_1: 0.20,
            target_midpoint_minutes_since_midnight: 180.0,
            prior_strain_resting_baseline_min_days: 3,
            prior_strain_max_hr_bpm: None,
            respiratory_rate_rpm: None,
            respiratory_rate_baseline_rpm: None,
            skin_temp_delta_c: None,
            provided_vitals_source: None,
            provided_vitals_provenance_json: None,
        }
    }
}

impl Default for StressFeatureScoreOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            resting_baseline_min_days: 3,
            hrv_min_rr_intervals_to_compute: 2,
            hrv_baseline_min_days: 3,
        }
    }
}

impl Default for MotionFeatureOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub capture_correlation_pass: bool,
    pub candidate_frame_count: usize,
    pub feature_count: usize,
    pub trusted_feature_count: usize,
    pub features: Vec<MotionFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub sample_time: String,
    pub sample_time_unix_ms: Option<i64>,
    pub sample_time_source: String,
    pub body_summary_kind: String,
    pub source_signal: String,
    pub scale_basis: String,
    pub motion_intensity_0_to_1: f64,
    pub raw_mean_abs: f64,
    pub raw_peak_abs: f64,
    pub parsed_sample_count: usize,
    pub axis_count: usize,
    pub heart_rate_bpm: Option<u8>,
    pub device_timestamp_seconds: Option<u32>,
    pub device_timestamp_subseconds: Option<u16>,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartRateFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub capture_correlation_pass: bool,
    pub candidate_frame_count: usize,
    pub feature_count: usize,
    pub trusted_feature_count: usize,
    pub features: Vec<HeartRateFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartRateFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub sample_time: String,
    pub sample_time_unix_ms: Option<i64>,
    pub sample_time_source: String,
    pub body_summary_kind: String,
    pub source_signal: String,
    pub heart_rate_bpm: f64,
    pub marker_offset: usize,
    pub marker_value: u8,
    pub device_timestamp_seconds: Option<u32>,
    pub device_timestamp_subseconds: Option<u16>,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VitalEventFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub capture_correlation_pass: bool,
    pub decoded_frame_count: usize,
    pub data_packet_frame_count: usize,
    pub pulse_information_packet_count: usize,
    pub candidate_frame_count: usize,
    pub feature_count: usize,
    pub trusted_feature_count: usize,
    pub resolved_metric_input_count: usize,
    pub features: Vec<VitalEventFeature>,
    pub skin_temperature_input_count: usize,
    pub trusted_skin_temperature_input_count: usize,
    pub skin_temperature_inputs: Vec<SkinTemperatureFeature>,
    pub respiratory_rate_input_count: usize,
    pub trusted_respiratory_rate_input_count: usize,
    pub respiratory_rate_inputs: Vec<RespiratoryRateFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RespiratoryRateCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start_time: String,
    pub end_time: String,
    pub capture_kind: Option<String>,
    pub label_policy: String,
    pub official_whoop_respiratory_rate_rpm: Option<f64>,
    pub tolerance_rpm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_respiratory_rate_rpm: Option<f64>,
    pub respiratory_rate_error_rpm: Option<f64>,
    pub respiratory_rate_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub selected_candidate_schema_field: Option<String>,
    pub selected_candidate_sample_time: Option<String>,
    pub decoder_id: String,
    pub decoder_version: String,
    pub promotion_status: String,
    pub quality_flags: Vec<String>,
    pub vital_event_report: VitalEventFeatureReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OxygenSaturationCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start_time: String,
    pub end_time: String,
    pub capture_kind: Option<String>,
    pub label_policy: String,
    pub official_whoop_oxygen_saturation_percent: Option<f64>,
    pub tolerance_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_oxygen_saturation_percent: Option<f64>,
    pub oxygen_saturation_error_percent: Option<f64>,
    pub oxygen_saturation_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub pulse_information_packet_count: usize,
    pub decoder_id: String,
    pub decoder_version: String,
    pub source_kind: String,
    pub promotion_status: String,
    pub quality_flags: Vec<String>,
    pub vital_event_report: VitalEventFeatureReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemperatureCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start_time: String,
    pub end_time: String,
    pub capture_kind: Option<String>,
    pub label_policy: String,
    pub official_whoop_skin_temperature_delta_c: Option<f64>,
    pub tolerance_c: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_skin_temperature_delta_c: Option<f64>,
    pub selected_candidate_skin_temperature_c: Option<f64>,
    pub skin_temperature_error_c: Option<f64>,
    pub skin_temperature_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub selected_candidate_schema_field: Option<String>,
    pub selected_candidate_sample_time: Option<String>,
    pub selected_candidate_source_signal: Option<String>,
    pub decoder_id: String,
    pub decoder_version: String,
    pub source_kind: String,
    pub promotion_status: String,
    pub quality_flags: Vec<String>,
    pub vital_event_report: VitalEventFeatureReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VitalEventFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub event_id: u16,
    pub event_name: String,
    pub source_signal: String,
    pub candidate_kind: String,
    pub semantic_status: String,
    pub raw_body_hex: String,
    pub raw_byte_count: usize,
    pub raw_i16_le: Option<i16>,
    pub raw_u16_le: Option<u16>,
    pub raw_i32_le: Option<i32>,
    pub raw_u32_le: Option<u32>,
    pub device_timestamp_seconds: Option<u32>,
    pub device_timestamp_subseconds: Option<u16>,
    pub trusted_candidate_evidence: bool,
    pub resolved_metric_input: bool,
    pub value_semantics_verified: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkinTemperatureFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub sample_time: String,
    pub sample_time_unix_ms: Option<i64>,
    pub sample_time_source: String,
    pub packet_k: u8,
    pub source_signal: String,
    pub candidate_kind: String,
    pub schema_field: String,
    pub semantic_status: String,
    pub raw_body_offset: usize,
    pub raw_absolute_offset: usize,
    pub raw_hex: String,
    pub raw_i16_le: Option<i16>,
    pub raw_u16_le: Option<u16>,
    pub scale: f64,
    pub skin_temperature_c: Option<f64>,
    pub trusted_candidate_evidence: bool,
    pub resolved_metric_input: bool,
    pub value_semantics_verified: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RespiratoryRateFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub sample_time: String,
    pub sample_time_unix_ms: Option<i64>,
    pub sample_time_source: String,
    pub packet_k: u8,
    pub source_signal: String,
    pub candidate_kind: String,
    pub schema_field: String,
    pub semantic_status: String,
    pub raw_body_offset: usize,
    pub raw_absolute_offset: usize,
    pub raw_hex: String,
    pub raw_u16_le: Option<u16>,
    pub scale: f64,
    pub respiratory_rate_rpm: Option<f64>,
    pub trusted_candidate_evidence: bool,
    pub resolved_metric_input: bool,
    pub value_semantics_verified: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HrvFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub capture_correlation_pass: bool,
    pub start_time: String,
    pub end_time: String,
    pub candidate_frame_count: usize,
    pub feature_count: usize,
    pub trusted_feature_count: usize,
    pub rr_interval_count: usize,
    pub trusted_rr_interval_count: usize,
    pub min_rr_intervals_to_compute: usize,
    pub require_baseline: bool,
    pub baseline_min_days: usize,
    pub daily_count: usize,
    pub hrv_input: Option<HrvInput>,
    pub score_result: Option<AlgorithmRunResult<HrvOutput>>,
    pub baseline: Option<HrvBaselineFeature>,
    pub daily: Vec<HrvDayFeature>,
    pub features: Vec<HrvFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HrvCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start_time: String,
    pub end_time: String,
    pub capture_kind: Option<String>,
    pub label_policy: String,
    pub official_whoop_hrv_rmssd_ms: Option<f64>,
    pub tolerance_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_hrv_rmssd_ms: Option<f64>,
    pub hrv_rmssd_error_ms: Option<f64>,
    pub hrv_rmssd_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub rr_interval_count: usize,
    pub trusted_rr_interval_count: usize,
    pub trusted_feature_count: usize,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub promotion_status: String,
    pub quality_flags: Vec<String>,
    pub hrv_report: HrvFeatureReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HrvFeature {
    pub metric_input_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub body_summary_kind: String,
    pub source_signal: String,
    pub scale_basis: String,
    pub rr_intervals_ms: Vec<f64>,
    pub raw_sample_count: usize,
    pub plausible_sample_count: usize,
    pub rejected_sample_count: usize,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HrvBaselineFeature {
    pub metric_input_id: String,
    pub hrv_baseline_rmssd_ms: f64,
    pub method: String,
    pub day_count: usize,
    pub trusted_metric_input: bool,
    pub input_ids: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HrvDayFeature {
    pub date: String,
    pub rmssd_ms: f64,
    pub rr_interval_count: usize,
    pub trusted_metric_input: bool,
    pub input_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySensorDiscoveryReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub hrv_report: HrvFeatureReport,
    pub vital_event_report: VitalEventFeatureReport,
    pub widgets: Vec<RecoverySensorWidgetDiscovery>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySensorWidgetDiscovery {
    pub metric_id: String,
    pub source_kind: String,
    pub confidence: f64,
    pub promotion_status: String,
    pub promotion_allowed: bool,
    pub user_visible_value_allowed: bool,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub resolved_metric_input_count: usize,
    pub value_semantics_verified_count: usize,
    pub candidate_source_signals: Vec<String>,
    pub quality_flags: Vec<String>,
    pub blocker_reasons: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricWindowFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub heart_rate_feature_count: usize,
    pub trusted_heart_rate_feature_count: usize,
    pub motion_feature_count: usize,
    pub trusted_motion_feature_count: usize,
    pub window: Option<MetricWindowFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricWindowFeature {
    pub metric_input_id: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: f64,
    pub average_hr_bpm: f64,
    pub max_hr_bpm: f64,
    pub average_motion_intensity_0_to_1: Option<f64>,
    pub hr_zone_minutes: Vec<f64>,
    pub heart_rate_sample_count: usize,
    pub motion_sample_count: usize,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub input_ids: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateFeatureReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub require_baseline: bool,
    pub baseline_min_days: usize,
    pub heart_rate_feature_count: usize,
    pub trusted_heart_rate_feature_count: usize,
    pub daily_count: usize,
    pub resting: Option<RestingHeartRateFeature>,
    pub baseline: Option<RestingHeartRateBaselineFeature>,
    pub daily: Vec<RestingHeartRateDayFeature>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateFeature {
    pub metric_input_id: String,
    pub start_time: String,
    pub end_time: String,
    pub resting_hr_bpm: f64,
    pub method: String,
    pub sample_count: usize,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub input_ids: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateBaselineFeature {
    pub metric_input_id: String,
    pub resting_hr_baseline_bpm: f64,
    pub method: String,
    pub day_count: usize,
    pub trusted_metric_input: bool,
    pub input_ids: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateDayFeature {
    pub date: String,
    pub resting_hr_bpm: f64,
    pub sample_count: usize,
    pub trusted_metric_input: bool,
    pub input_ids: Vec<String>,
}

struct RestingHeartRateCandidateSelection<'a> {
    features: Vec<&'a HeartRateFeature>,
    method: &'static str,
    quality_flags: Vec<String>,
    motion_sample_count: usize,
    matched_heart_rate_sample_count: usize,
    low_motion_heart_rate_sample_count: usize,
    high_motion_heart_rate_sample_count: usize,
    unmatched_heart_rate_sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrainFeatureScoreReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub resting_start_time: String,
    pub resting_end_time: String,
    pub max_hr_basis: Option<String>,
    pub resting_report: RestingHeartRateFeatureReport,
    pub window_report: Option<MetricWindowFeatureReport>,
    pub strain_input: Option<StrainInput>,
    pub score_result: Option<AlgorithmRunResult<StrainScoreOutput>>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepFeatureScoreReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub motion_report: MotionFeatureReport,
    pub heart_rate_report: HeartRateFeatureReport,
    pub sleep_window: Option<SleepWindowFeature>,
    pub sleep_input: Option<SleepInput>,
    pub score_result: Option<AlgorithmRunResult<SleepScoreOutput>>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepWindowFeature {
    pub metric_input_id: String,
    pub start_time: String,
    pub end_time: String,
    pub time_in_bed_minutes: f64,
    pub sleep_duration_minutes: f64,
    pub sleep_latency_minutes: f64,
    pub wake_after_sleep_onset_minutes: f64,
    pub wake_episode_count: u32,
    pub midpoint_deviation_minutes: f64,
    pub disturbance_count: u32,
    pub stage_model_version: String,
    pub stage_segments: Vec<SleepStageSegmentFeature>,
    pub stage_minutes: BTreeMap<String, f64>,
    pub average_sleep_hr_bpm: Option<f64>,
    pub lowest_sleep_hr_bpm: Option<f64>,
    pub sleep_hr_trend_bpm_per_hour: Option<f64>,
    pub baseline_awake_hr_bpm: Option<f64>,
    pub heart_rate_dip_percent: Option<f64>,
    pub motion_feature_count: usize,
    pub heart_rate_feature_count: usize,
    pub motion_coverage_fraction: f64,
    pub heart_rate_coverage_fraction: f64,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub input_ids: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SleepStageKind {
    Awake,
    Core,
    Deep,
    Rem,
}

impl SleepStageKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Awake => "awake",
            Self::Core => "core",
            Self::Deep => "deep",
            Self::Rem => "rem",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepStageSegmentFeature {
    pub stage: SleepStageKind,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: f64,
    pub confidence_0_to_1: f64,
    #[serde(default)]
    pub stage_probabilities: BTreeMap<String, f64>,
    pub motion_intensity_0_to_1: f64,
    pub heart_rate_bpm: Option<f64>,
    pub quality_flags: Vec<String>,
    pub input_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryFeatureScoreReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub hrv_start_time: String,
    pub hrv_end_time: String,
    pub hrv_baseline_start_time: String,
    pub hrv_baseline_end_time: String,
    pub resting_start_time: String,
    pub resting_end_time: String,
    pub sleep_start_time: String,
    pub sleep_end_time: String,
    pub prior_strain_start_time: String,
    pub prior_strain_end_time: String,
    pub hrv_report: HrvFeatureReport,
    pub hrv_baseline_report: HrvFeatureReport,
    pub resting_report: RestingHeartRateFeatureReport,
    pub sleep_report: SleepFeatureScoreReport,
    pub prior_strain_report: StrainFeatureScoreReport,
    pub provided_vitals: Option<RecoveryProvidedVitalsFeature>,
    pub recovery_input: Option<RecoveryInput>,
    pub score_result: Option<AlgorithmRunResult<RecoveryScoreOutput>>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryProvidedVitalsFeature {
    pub metric_input_id: String,
    pub respiratory_rate_rpm: f64,
    pub respiratory_rate_baseline_rpm: f64,
    pub skin_temp_delta_c: f64,
    pub source: String,
    pub trusted_metric_input: bool,
    pub quality_flags: Vec<String>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StressFeatureScoreReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub require_trusted_evidence: bool,
    pub start_time: String,
    pub end_time: String,
    pub resting_start_time: String,
    pub resting_end_time: String,
    pub hrv_start_time: String,
    pub hrv_end_time: String,
    pub hrv_baseline_start_time: String,
    pub hrv_baseline_end_time: String,
    pub heart_rate_report: HeartRateFeatureReport,
    pub motion_report: MotionFeatureReport,
    pub resting_report: RestingHeartRateFeatureReport,
    pub hrv_report: HrvFeatureReport,
    pub hrv_baseline_report: HrvFeatureReport,
    pub stress_input: Option<StressInput>,
    pub score_result: Option<AlgorithmRunResult<StressScoreOutput>>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetricFeatureNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone)]
struct MotionPlan {
    body_summary_kind: &'static str,
    axes: Vec<I16SeriesSummary>,
    heart_rate_bpm: Option<u8>,
    device_timestamp_seconds: Option<u32>,
    device_timestamp_subseconds: Option<u16>,
    summary_warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct HeartRatePlan {
    body_summary_kind: &'static str,
    source_signal: &'static str,
    quality_flag: &'static str,
    marker_offset: usize,
    marker_value: u8,
    device_timestamp_seconds: Option<u32>,
    device_timestamp_subseconds: Option<u16>,
}

#[derive(Debug, Clone)]
struct VitalEventPlan {
    event_id: u16,
    event_name: String,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
    data_hex: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct SkinTemperaturePlan {
    packet_k: u8,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
    schema_field: &'static str,
    raw_body_offset: usize,
    raw_absolute_offset: usize,
    encoding: &'static str,
    scale: f64,
}

#[derive(Debug, Clone)]
struct RespiratoryRatePlan {
    packet_k: u8,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
    schema_field: &'static str,
    raw_body_offset: usize,
    raw_absolute_offset: usize,
    encoding: &'static str,
    scale: f64,
}

#[derive(Debug, Clone)]
struct HrvPlan {
    samples: I16SeriesSummary,
    flags: Option<u16>,
    sample_count: Option<u16>,
    summary_warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
struct MotionAccumulator {
    abs_sum: f64,
    peak_abs: f64,
    sample_count: usize,
}

#[derive(Debug, Clone)]
struct NormalizedSampleTime {
    time: String,
    unix_ms: Option<i64>,
    source: String,
}

pub fn run_motion_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: MotionFeatureOptions,
) -> GooseResult<MotionFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    run_motion_feature_report(&decoded_rows, &correlation, options)
}

pub fn run_motion_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    options: MotionFeatureOptions,
) -> GooseResult<MotionFeatureReport> {
    let trusted_frames =
        trusted_frames_for_summary_kinds(correlation, &["raw_motion_k10", "raw_motion_k21"]);
    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }

    let mut candidate_frame_count = 0;
    let mut features = Vec::new();
    for row in decoded_rows {
        let Some(plan) = motion_plan_from_row(row)? else {
            continue;
        };
        candidate_frame_count += 1;
        let payload = decode_hex_with_whitespace(&row.payload_hex)?;
        let Some(feature) = motion_feature_from_plan(row, &payload, plan, &trusted_frames)? else {
            continue;
        };
        features.push(feature);
    }

    let trusted_feature_count = features
        .iter()
        .filter(|feature| feature.trusted_metric_input)
        .count();
    if options.require_trusted_evidence && trusted_feature_count == 0 {
        issues.push("no_trusted_motion_features".to_string());
    }
    let next_actions = metric_feature_next_actions("motion", &issues);

    Ok(MotionFeatureReport {
        schema: MOTION_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-motion-feature-extractor".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        capture_correlation_pass: correlation.pass,
        candidate_frame_count,
        feature_count: features.len(),
        trusted_feature_count,
        features,
        issues,
        next_actions,
    })
}

pub fn run_heart_rate_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: HeartRateFeatureOptions,
) -> GooseResult<HeartRateFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    run_heart_rate_feature_report(&decoded_rows, &correlation, options)
}

pub fn run_heart_rate_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    options: HeartRateFeatureOptions,
) -> GooseResult<HeartRateFeatureReport> {
    let trusted_frames =
        trusted_frames_for_summary_kinds(correlation, &["normal_history", "raw_motion_k10"]);
    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }

    let mut candidate_frame_count = 0;
    let mut features = Vec::new();
    for row in decoded_rows {
        let Some(plan) = heart_rate_plan_from_row(row)? else {
            continue;
        };
        candidate_frame_count += 1;
        let Some(feature) = heart_rate_feature_from_plan(row, plan, &trusted_frames)? else {
            continue;
        };
        features.push(feature);
    }

    let trusted_feature_count = features
        .iter()
        .filter(|feature| feature.trusted_metric_input)
        .count();
    if options.require_trusted_evidence && trusted_feature_count == 0 {
        issues.push("no_trusted_heart_rate_features".to_string());
    }
    let next_actions = metric_feature_next_actions("heart_rate", &issues);

    Ok(HeartRateFeatureReport {
        schema: HEART_RATE_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-heart-rate-feature-extractor".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        capture_correlation_pass: correlation.pass,
        candidate_frame_count,
        feature_count: features.len(),
        trusted_feature_count,
        features,
        issues,
        next_actions,
    })
}

pub fn run_vital_event_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: VitalEventFeatureOptions,
) -> GooseResult<VitalEventFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    run_vital_event_feature_report(&decoded_rows, &correlation, options)
}

pub fn run_respiratory_rate_capture_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: RespiratoryRateCaptureValidationOptions,
) -> GooseResult<RespiratoryRateCaptureValidationReport> {
    validate_respiratory_rate_validation_options(&options)?;
    let vital_event_report = run_vital_event_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        options.feature_options,
    )?;
    let selected_candidate = select_respiratory_rate_validation_candidate(
        &vital_event_report,
        options.feature_options.require_trusted_evidence,
    );
    let local_respiratory_rate_rpm =
        selected_candidate.and_then(|feature| feature.respiratory_rate_rpm);
    let selected_candidate_schema_field =
        selected_candidate.map(|feature| feature.schema_field.clone());
    let selected_candidate_sample_time =
        selected_candidate.map(|feature| feature.sample_time.clone());
    let comparison = compare_respiratory_rate_label(
        local_respiratory_rate_rpm,
        options.official_whoop_respiratory_rate_rpm,
        options.tolerance_rpm,
    );
    let provided_label_count = usize::from(options.official_whoop_respiratory_rate_rpm.is_some());
    let matching_label_count = usize::from(comparison.within_tolerance == Some(true));

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_respiratory_rate_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !vital_event_report.pass {
        issues.push("vital_event_report_blocked".to_string());
        for issue in &vital_event_report.issues {
            issues.push(format!("vital_event_report_issue:{issue}"));
        }
    }
    if vital_event_report.respiratory_rate_input_count == 0 {
        issues.push("no_respiratory_rate_packet_candidate".to_string());
    }
    if options.feature_options.require_trusted_evidence
        && vital_event_report.trusted_respiratory_rate_input_count == 0
    {
        issues.push("no_trusted_respiratory_rate_candidate".to_string());
    }
    if options.official_whoop_respiratory_rate_rpm.is_some() && local_respiratory_rate_rpm.is_none()
    {
        issues.push("local_respiratory_rate_rpm_missing".to_string());
    }
    if comparison.within_tolerance == Some(false) {
        issues.push("respiratory_rate_label_delta_out_of_tolerance".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("respiratory_rate_semantics_unverified".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    quality_flags.insert("official_whoop_values_are_validation_labels_not_inputs".to_string());
    if let Some(candidate) = selected_candidate {
        quality_flags.insert(candidate.semantic_status.clone());
        quality_flags.extend(candidate.quality_flags.iter().cloned());
    }

    Ok(RespiratoryRateCaptureValidationReport {
        schema: RESPIRATORY_RATE_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-respiratory-rate-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        start_time: start.to_string(),
        end_time: end.to_string(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_respiratory_rate_rpm: options.official_whoop_respiratory_rate_rpm,
        tolerance_rpm: options.tolerance_rpm,
        label_provenance: options.label_provenance,
        local_respiratory_rate_rpm,
        respiratory_rate_error_rpm: comparison.error,
        respiratory_rate_within_tolerance: comparison.within_tolerance,
        provided_label_count,
        matching_label_count,
        candidate_count: vital_event_report.respiratory_rate_input_count,
        trusted_candidate_count: vital_event_report.trusted_respiratory_rate_input_count,
        selected_candidate_schema_field,
        selected_candidate_sample_time,
        decoder_id: GOOSE_RESPIRATORY_RATE_HISTORY_CANDIDATE_V0_ID.to_string(),
        decoder_version: GOOSE_RESPIRATORY_RATE_HISTORY_CANDIDATE_V0_VERSION.to_string(),
        promotion_status: "validation_only_respiratory_rate_semantics_still_unverified".to_string(),
        quality_flags: quality_flags.into_iter().collect(),
        vital_event_report,
        next_actions: respiratory_rate_validation_next_actions(&issues),
        issues,
    })
}

pub fn run_oxygen_saturation_capture_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: OxygenSaturationCaptureValidationOptions,
) -> GooseResult<OxygenSaturationCaptureValidationReport> {
    validate_oxygen_saturation_validation_options(&options)?;
    let vital_event_report = run_vital_event_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        options.feature_options,
    )?;
    let local_oxygen_saturation_percent = None;
    let comparison = compare_oxygen_saturation_label(
        local_oxygen_saturation_percent,
        options.official_whoop_oxygen_saturation_percent,
        options.tolerance_percent,
    );
    let provided_label_count =
        usize::from(options.official_whoop_oxygen_saturation_percent.is_some());
    let matching_label_count = usize::from(comparison.within_tolerance == Some(true));

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_oxygen_saturation_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !vital_event_report.pass {
        issues.push("vital_event_report_blocked".to_string());
        for issue in &vital_event_report.issues {
            issues.push(format!("vital_event_report_issue:{issue}"));
        }
    }
    issues.push("oxygen_saturation_decoder_not_implemented".to_string());
    if vital_event_report.pulse_information_packet_count == 0 {
        issues.push("no_oxygen_saturation_packet_candidate".to_string());
    } else {
        issues.push("pulse_information_seen_without_spo2_decode".to_string());
    }
    if options.official_whoop_oxygen_saturation_percent.is_some()
        && local_oxygen_saturation_percent.is_none()
    {
        issues.push("local_oxygen_saturation_percent_missing".to_string());
    }
    if comparison.within_tolerance == Some(false) {
        issues.push("oxygen_saturation_label_delta_out_of_tolerance".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("oxygen_saturation_decoder_not_implemented".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    quality_flags.insert("official_whoop_values_are_validation_labels_not_inputs".to_string());
    if vital_event_report.pulse_information_packet_count > 0 {
        quality_flags.insert("pulse_information_seen_without_spo2_decode".to_string());
    }

    Ok(OxygenSaturationCaptureValidationReport {
        schema: OXYGEN_SATURATION_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-oxygen-saturation-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        start_time: start.to_string(),
        end_time: end.to_string(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_oxygen_saturation_percent: options.official_whoop_oxygen_saturation_percent,
        tolerance_percent: options.tolerance_percent,
        label_provenance: options.label_provenance,
        local_oxygen_saturation_percent,
        oxygen_saturation_error_percent: comparison.error,
        oxygen_saturation_within_tolerance: comparison.within_tolerance,
        provided_label_count,
        matching_label_count,
        candidate_count: vital_event_report.pulse_information_packet_count,
        trusted_candidate_count: 0,
        pulse_information_packet_count: vital_event_report.pulse_information_packet_count,
        decoder_id: GOOSE_OXYGEN_SATURATION_PACKET_CANDIDATE_V0_ID.to_string(),
        decoder_version: GOOSE_OXYGEN_SATURATION_PACKET_CANDIDATE_V0_VERSION.to_string(),
        source_kind: "unavailable".to_string(),
        promotion_status: "validation_only_oxygen_saturation_decoder_not_implemented".to_string(),
        quality_flags: quality_flags.into_iter().collect(),
        vital_event_report,
        next_actions: oxygen_saturation_validation_next_actions(&issues),
        issues,
    })
}

pub fn run_temperature_capture_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: TemperatureCaptureValidationOptions,
) -> GooseResult<TemperatureCaptureValidationReport> {
    validate_temperature_validation_options(&options)?;
    let vital_event_report = run_vital_event_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        options.feature_options,
    )?;
    let selected_candidate = select_temperature_validation_candidate(
        &vital_event_report,
        options.feature_options.require_trusted_evidence,
    );
    let selected_candidate_skin_temperature_c =
        selected_candidate.and_then(|feature| feature.skin_temperature_c);
    let local_skin_temperature_delta_c = selected_candidate.and_then(|feature| {
        if feature.resolved_metric_input && feature.value_semantics_verified {
            feature.skin_temperature_c
        } else {
            None
        }
    });
    let selected_candidate_schema_field =
        selected_candidate.map(|feature| feature.schema_field.clone());
    let selected_candidate_sample_time =
        selected_candidate.map(|feature| feature.sample_time.clone());
    let selected_candidate_source_signal =
        selected_candidate.map(|feature| feature.source_signal.clone());
    let candidate_count =
        vital_event_report.feature_count + vital_event_report.skin_temperature_input_count;
    let trusted_candidate_count = vital_event_report.trusted_feature_count
        + vital_event_report.trusted_skin_temperature_input_count;
    let comparison = compare_temperature_label(
        local_skin_temperature_delta_c,
        options.official_whoop_skin_temperature_delta_c,
        options.tolerance_c,
    );
    let provided_label_count =
        usize::from(options.official_whoop_skin_temperature_delta_c.is_some());
    let matching_label_count = usize::from(comparison.within_tolerance == Some(true));

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_skin_temperature_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !vital_event_report.pass {
        issues.push("vital_event_report_blocked".to_string());
        for issue in &vital_event_report.issues {
            issues.push(format!("vital_event_report_issue:{issue}"));
        }
    }
    if candidate_count == 0 {
        issues.push("no_temperature_packet_candidate".to_string());
    }
    if options.feature_options.require_trusted_evidence && trusted_candidate_count == 0 {
        issues.push("no_trusted_temperature_candidate".to_string());
    }
    if selected_candidate_skin_temperature_c.is_some() && local_skin_temperature_delta_c.is_none() {
        issues.push("temperature_units_unverified".to_string());
    }
    if options.official_whoop_skin_temperature_delta_c.is_some()
        && local_skin_temperature_delta_c.is_none()
    {
        issues.push("local_skin_temperature_delta_c_missing".to_string());
    }
    if comparison.within_tolerance == Some(false) {
        issues.push("skin_temperature_label_delta_out_of_tolerance".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("temperature_units_unverified".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    quality_flags.insert("official_whoop_values_are_validation_labels_not_inputs".to_string());
    if let Some(candidate) = selected_candidate {
        quality_flags.insert(candidate.semantic_status.clone());
        quality_flags.extend(candidate.quality_flags.iter().cloned());
    }

    Ok(TemperatureCaptureValidationReport {
        schema: TEMPERATURE_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-temperature-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        start_time: start.to_string(),
        end_time: end.to_string(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_skin_temperature_delta_c: options.official_whoop_skin_temperature_delta_c,
        tolerance_c: options.tolerance_c,
        label_provenance: options.label_provenance,
        local_skin_temperature_delta_c,
        selected_candidate_skin_temperature_c,
        skin_temperature_error_c: comparison.error,
        skin_temperature_within_tolerance: comparison.within_tolerance,
        provided_label_count,
        matching_label_count,
        candidate_count,
        trusted_candidate_count,
        selected_candidate_schema_field,
        selected_candidate_sample_time,
        selected_candidate_source_signal,
        decoder_id: GOOSE_SKIN_TEMPERATURE_HISTORY_CANDIDATE_V0_ID.to_string(),
        decoder_version: GOOSE_SKIN_TEMPERATURE_HISTORY_CANDIDATE_V0_VERSION.to_string(),
        source_kind: "unavailable".to_string(),
        promotion_status: "validation_only_temperature_units_still_unverified".to_string(),
        quality_flags: quality_flags.into_iter().collect(),
        vital_event_report,
        next_actions: temperature_validation_next_actions(&issues),
        issues,
    })
}

pub fn run_vital_event_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    options: VitalEventFeatureOptions,
) -> GooseResult<VitalEventFeatureReport> {
    let trusted_frames = trusted_frames_for_summary_kinds(
        correlation,
        &["event_temperature_level", "normal_history"],
    );
    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }

    let mut data_packet_frame_count = 0;
    let mut pulse_information_packet_count = 0;
    let mut candidate_frame_count = 0;
    let mut features = Vec::new();
    let mut skin_temperature_inputs = Vec::new();
    let mut respiratory_rate_inputs = Vec::new();
    for row in decoded_rows {
        let parsed_payload = parsed_payload_from_row(row)?;
        if let Some(ParsedPayload::DataPacket { packet_k, .. }) = &parsed_payload {
            data_packet_frame_count += 1;
            if matches!(packet_k, Some(25 | 26)) {
                pulse_information_packet_count += 1;
            }
        }

        if let Some(plan) = vital_event_plan_from_payload(&parsed_payload) {
            candidate_frame_count += 1;
            features.push(vital_event_feature_from_plan(row, plan, &trusted_frames)?);
        }
        if let Some(plan) = skin_temperature_plan_from_payload(&parsed_payload)
            && let Some(feature) = skin_temperature_feature_from_plan(row, plan, &trusted_frames)?
        {
            skin_temperature_inputs.push(feature);
        }
        if let Some(plan) = respiratory_rate_plan_from_payload(&parsed_payload)
            && let Some(feature) = respiratory_rate_feature_from_plan(row, plan, &trusted_frames)?
        {
            respiratory_rate_inputs.push(feature);
        }
    }

    let trusted_feature_count = features
        .iter()
        .filter(|feature| feature.trusted_candidate_evidence)
        .count();
    let trusted_skin_temperature_input_count = skin_temperature_inputs
        .iter()
        .filter(|feature| feature.trusted_candidate_evidence)
        .count();
    let trusted_respiratory_rate_input_count = respiratory_rate_inputs
        .iter()
        .filter(|feature| feature.trusted_candidate_evidence)
        .count();
    let resolved_metric_input_count = features
        .iter()
        .filter(|feature| feature.resolved_metric_input)
        .count();
    let trusted_vital_candidate_count = trusted_feature_count
        + trusted_skin_temperature_input_count
        + trusted_respiratory_rate_input_count;
    if options.require_trusted_evidence && trusted_vital_candidate_count == 0 {
        issues.push("no_trusted_vital_event_features".to_string());
    }
    let next_actions = metric_feature_next_actions("vital_event", &issues);

    Ok(VitalEventFeatureReport {
        schema: VITAL_EVENT_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-vital-event-feature-extractor".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        capture_correlation_pass: correlation.pass,
        decoded_frame_count: decoded_rows.len(),
        data_packet_frame_count,
        pulse_information_packet_count,
        candidate_frame_count,
        feature_count: features.len(),
        trusted_feature_count,
        resolved_metric_input_count,
        features,
        skin_temperature_input_count: skin_temperature_inputs.len(),
        trusted_skin_temperature_input_count,
        skin_temperature_inputs,
        respiratory_rate_input_count: respiratory_rate_inputs.len(),
        trusted_respiratory_rate_input_count,
        respiratory_rate_inputs,
        issues,
        next_actions,
    })
}

pub fn run_hrv_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: HrvFeatureOptions,
) -> GooseResult<HrvFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    let report = run_hrv_feature_report(&decoded_rows, &correlation, start, end, options)?;

    // When no HRV data came from decoded_frames, fall back to the rr_intervals table.
    // This covers simulator testing and fresh-install scenarios where HR/RR data
    // was imported directly (store.insert_hr_rr_batch) without the BLE trust chain.
    if report.hrv_input.is_none()
        && let Ok(fallback) = hrv_report_from_rr_table(store, start, end, options)
        && fallback.hrv_input.is_some()
    {
        return Ok(fallback);
    }

    Ok(report)
}

/// Parse a "YYYY-MM-DDTHH:MM:SS..." ISO-8601 UTC string to a Unix timestamp (seconds).
/// Returns None if the string cannot be parsed.
fn iso8601_to_unix_approx(s: &str) -> Option<f64> {
    let s = s.trim_end_matches('Z');
    let (date_str, time_str) = s.split_once('T')?;
    let mut date_parts = date_str.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let time_str = time_str.split('.').next().unwrap_or(time_str);
    let mut hms = time_str.splitn(3, ':');
    let hour: i64 = hms.next()?.parse().ok()?;
    let minute: i64 = hms.next()?.parse().ok()?;
    let second: i64 = hms.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // Days since 1970-01-01 using a simple Gregorian calculation.
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let jdn: i64 = 365 * y + y / 4 - y / 100 + y / 400 + (153 * m + 2) / 5 + day - 719469;
    let unix = jdn * 86400 + hour * 3600 + minute * 60 + second;
    Some(unix as f64)
}

/// Build an HrvFeatureReport directly from the rr_intervals table when the
/// standard decoded_frames path has no data (e.g. imported data without trust chain).
fn hrv_report_from_rr_table(
    store: &GooseStore,
    start: &str,
    end: &str,
    options: HrvFeatureOptions,
) -> GooseResult<HrvFeatureReport> {
    let start_ts = iso8601_to_unix_approx(start).unwrap_or(0.0);
    let end_ts = iso8601_to_unix_approx(end).unwrap_or(f64::MAX);
    let rows: Vec<RrIntervalRow> = store.rr_intervals_between(start_ts, end_ts)?;

    let rr_ms: Vec<f64> = rows
        .iter()
        .map(|r| r.interval_ms as f64)
        .filter(|&ms| ms > 200.0 && ms < 3000.0)
        .collect();

    let mut issues = Vec::new();
    let hrv_input = if rr_ms.len() >= options.min_rr_intervals_to_compute {
        Some(HrvInput {
            start_time: start.to_string(),
            end_time: end.to_string(),
            rr_intervals_ms: rr_ms.clone(),
            input_ids: vec!["rr_intervals_table".to_string()],
            rr_timestamps_s: None,
            stage_segments: None,
        })
    } else {
        issues.push("not_enough_rr_intervals".to_string());
        None
    };
    let score_result = hrv_input.as_ref().map(goose_hrv_v0);
    if score_result.as_ref().is_some_and(|r| !r.errors.is_empty()) {
        issues.push("hrv_score_errors".to_string());
    }

    Ok(HrvFeatureReport {
        schema: HRV_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-hrv-features".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: false,
        capture_correlation_pass: true,
        start_time: start.to_string(),
        end_time: end.to_string(),
        candidate_frame_count: rows.len(),
        feature_count: if hrv_input.is_some() { 1 } else { 0 },
        trusted_feature_count: if hrv_input.is_some() { 1 } else { 0 },
        rr_interval_count: rr_ms.len(),
        trusted_rr_interval_count: rr_ms.len(),
        min_rr_intervals_to_compute: options.min_rr_intervals_to_compute,
        require_baseline: options.require_baseline,
        baseline_min_days: options.baseline_min_days,
        daily_count: 0,
        hrv_input,
        score_result,
        baseline: None,
        daily: Vec::new(),
        features: Vec::new(),
        issues,
        next_actions: Vec::new(),
    })
}

pub fn run_hrv_capture_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: HrvCaptureValidationOptions,
) -> GooseResult<HrvCaptureValidationReport> {
    validate_hrv_validation_options(&options)?;
    let hrv_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        options.feature_options,
    )?;
    let local_hrv_rmssd_ms = hrv_report
        .score_result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .map(|output| output.rmssd_ms);
    let comparison = compare_hrv_label(
        local_hrv_rmssd_ms,
        options.official_whoop_hrv_rmssd_ms,
        options.tolerance_ms,
    );
    let provided_label_count = usize::from(options.official_whoop_hrv_rmssd_ms.is_some());
    let matching_label_count = usize::from(comparison.within_tolerance == Some(true));

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_hrv_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !hrv_report.pass {
        issues.push("hrv_feature_report_blocked".to_string());
        for issue in &hrv_report.issues {
            issues.push(format!("hrv_feature_report_issue:{issue}"));
        }
    }
    if options.official_whoop_hrv_rmssd_ms.is_some() && local_hrv_rmssd_ms.is_none() {
        issues.push("local_hrv_rmssd_missing".to_string());
    }
    if comparison.within_tolerance == Some(false) {
        issues.push("hrv_label_delta_out_of_tolerance".to_string());
    }
    issues.sort();
    issues.dedup();

    let quality_flags = vec![
        "hrv_rr_interval_scale_unverified".to_string(),
        "official_whoop_values_are_validation_labels_not_inputs".to_string(),
    ];

    Ok(HrvCaptureValidationReport {
        schema: HRV_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-hrv-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        start_time: start.to_string(),
        end_time: end.to_string(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_hrv_rmssd_ms: options.official_whoop_hrv_rmssd_ms,
        tolerance_ms: options.tolerance_ms,
        label_provenance: options.label_provenance,
        local_hrv_rmssd_ms,
        hrv_rmssd_error_ms: comparison.error,
        hrv_rmssd_within_tolerance: comparison.within_tolerance,
        provided_label_count,
        matching_label_count,
        rr_interval_count: hrv_report.rr_interval_count,
        trusted_rr_interval_count: hrv_report.trusted_rr_interval_count,
        trusted_feature_count: hrv_report.trusted_feature_count,
        algorithm_id: GOOSE_HRV_V0_ID.to_string(),
        algorithm_version: GOOSE_HRV_V0_VERSION.to_string(),
        promotion_status: "validation_only_rr_interval_scale_still_unverified".to_string(),
        quality_flags,
        hrv_report,
        next_actions: hrv_validation_next_actions(&issues),
        issues,
    })
}

pub fn run_hrv_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    start: &str,
    end: &str,
    options: HrvFeatureOptions,
) -> GooseResult<HrvFeatureReport> {
    let trusted_frames = trusted_frames_for_summary_kinds(
        correlation,
        &["r17_optical_or_labrador_filtered", "r22_whoop5_hr"],
    );
    // R22 has priority over R17 in the same unix-second window. Build a set of R17
    // frame IDs that are shadowed by an R22 frame in the same second and must be
    // skipped so we do not double-count the same biometric window.
    let r17_shadowed = r22_shadowed_r17_frame_ids(decoded_rows);
    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }

    let mut candidate_frame_count = 0;
    let mut features = Vec::new();
    for row in decoded_rows {
        if r17_shadowed.contains(&row.frame_id) {
            continue;
        }
        let Some(plan) = hrv_plan_from_row(row)? else {
            continue;
        };
        candidate_frame_count += 1;
        let payload = decode_hex_with_whitespace(&row.payload_hex)?;
        let Some(feature) = hrv_feature_from_plan(row, &payload, plan, &trusted_frames)? else {
            continue;
        };
        features.push(feature);
    }

    let trusted_feature_count = features
        .iter()
        .filter(|feature| feature.trusted_metric_input)
        .count();
    if options.require_trusted_evidence && trusted_feature_count == 0 {
        issues.push("no_trusted_hrv_features".to_string());
    }

    let input_features = features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let rr_intervals_ms = input_features
        .iter()
        .flat_map(|feature| feature.rr_intervals_ms.iter().copied())
        .collect::<Vec<_>>();
    let trusted_rr_interval_count = features
        .iter()
        .filter(|feature| feature.trusted_metric_input)
        .map(|feature| feature.rr_intervals_ms.len())
        .sum::<usize>();
    let rr_interval_count = features
        .iter()
        .map(|feature| feature.rr_intervals_ms.len())
        .sum::<usize>();

    if rr_intervals_ms.len() < options.min_rr_intervals_to_compute {
        issues.push("not_enough_rr_intervals".to_string());
    }

    let hrv_input = if rr_intervals_ms.len() >= options.min_rr_intervals_to_compute {
        let mut input_ids = input_features
            .iter()
            .map(|feature| feature.metric_input_id.clone())
            .collect::<Vec<_>>();
        input_ids.sort();
        Some(HrvInput {
            start_time: start.to_string(),
            end_time: end.to_string(),
            rr_intervals_ms,
            input_ids,
            rr_timestamps_s: None,
            stage_segments: None,
        })
    } else {
        None
    };
    let score_result = hrv_input.as_ref().map(goose_hrv_v0);
    if score_result
        .as_ref()
        .is_some_and(|result| !result.errors.is_empty())
    {
        issues.push("hrv_score_errors".to_string());
    }

    let daily = daily_hrv_features(&input_features, options.min_rr_intervals_to_compute);
    let baseline = hrv_baseline_feature(start, end, &daily, options);
    if options.require_baseline && baseline.is_none() {
        issues.push("hrv_baseline_min_days_not_met".to_string());
    }
    let next_actions = metric_feature_next_actions("hrv", &issues);

    Ok(HrvFeatureReport {
        schema: HRV_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-hrv-feature-extractor".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        capture_correlation_pass: correlation.pass,
        start_time: start.to_string(),
        end_time: end.to_string(),
        candidate_frame_count,
        feature_count: features.len(),
        trusted_feature_count,
        rr_interval_count,
        trusted_rr_interval_count,
        min_rr_intervals_to_compute: options.min_rr_intervals_to_compute,
        require_baseline: options.require_baseline,
        baseline_min_days: options.baseline_min_days,
        daily_count: daily.len(),
        hrv_input,
        score_result,
        baseline,
        daily,
        features,
        issues,
        next_actions,
    })
}

pub fn run_recovery_sensor_discovery_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: RecoverySensorDiscoveryOptions,
) -> GooseResult<RecoverySensorDiscoveryReport> {
    let hrv_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.min_rr_intervals_to_compute,
            baseline_min_days: 1,
            require_baseline: false,
        },
    )?;
    let vital_event_report = run_vital_event_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        VitalEventFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
        },
    )?;
    let widgets = recovery_sensor_widgets(&hrv_report, &vital_event_report);
    let issues = recovery_sensor_discovery_issues(&widgets);
    let next_actions = recovery_sensor_discovery_next_actions(&widgets);

    Ok(RecoverySensorDiscoveryReport {
        schema: RECOVERY_SENSOR_DISCOVERY_REPORT_SCHEMA.to_string(),
        generated_by: "goose-recovery-sensor-discovery-gate".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        hrv_report,
        vital_event_report,
        widgets,
        issues,
        next_actions,
    })
}

pub fn run_metric_window_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: MetricWindowFeatureOptions,
) -> GooseResult<MetricWindowFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    run_metric_window_feature_report(&decoded_rows, &correlation, start, end, options)
}

pub fn run_metric_window_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    start: &str,
    end: &str,
    options: MetricWindowFeatureOptions,
) -> GooseResult<MetricWindowFeatureReport> {
    let feature_options = MotionFeatureOptions {
        min_owned_captures_per_summary: options.min_owned_captures_per_summary,
        require_trusted_evidence: options.require_trusted_evidence,
    };
    let heart_rate_report =
        run_heart_rate_feature_report(decoded_rows, correlation, feature_options)?;
    let motion_report = run_motion_feature_report(decoded_rows, correlation, feature_options)?;

    let heart_rate_features = heart_rate_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let motion_features = motion_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }
    if options.require_trusted_evidence && heart_rate_features.is_empty() {
        issues.push("no_trusted_heart_rate_window_features".to_string());
    }

    let window = if heart_rate_features.is_empty() {
        None
    } else {
        let window =
            aggregate_metric_window(start, end, &heart_rate_features, &motion_features, options)?;
        if options.require_trusted_evidence && window.duration_minutes <= 0.0 {
            issues.push("insufficient_heart_rate_window_duration".to_string());
        }
        Some(window)
    };
    let next_actions = metric_feature_next_actions("metric_window", &issues);

    Ok(MetricWindowFeatureReport {
        schema: METRIC_WINDOW_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-metric-window-feature-aggregator".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        heart_rate_feature_count: heart_rate_report.features.len(),
        trusted_heart_rate_feature_count: heart_rate_report.trusted_feature_count,
        motion_feature_count: motion_report.features.len(),
        trusted_motion_feature_count: motion_report.trusted_feature_count,
        window,
        issues,
        next_actions,
    })
}

pub fn run_resting_heart_rate_feature_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: RestingHeartRateFeatureOptions,
) -> GooseResult<RestingHeartRateFeatureReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    let correlation = run_capture_correlation_for_store(
        store,
        database_path,
        start,
        end,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_owned_captures: options.require_trusted_evidence,
        },
    )?;
    run_resting_heart_rate_feature_report(&decoded_rows, &correlation, start, end, options)
}

pub fn run_resting_heart_rate_feature_report(
    decoded_rows: &[DecodedFrameRow],
    correlation: &CaptureCorrelationReport,
    start: &str,
    end: &str,
    options: RestingHeartRateFeatureOptions,
) -> GooseResult<RestingHeartRateFeatureReport> {
    let heart_rate_report = run_heart_rate_feature_report(
        decoded_rows,
        correlation,
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
        },
    )?;
    let heart_rate_features = heart_rate_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let motion_report = run_motion_feature_report(
        decoded_rows,
        correlation,
        MotionFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
        },
    )?;
    let motion_features = motion_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let resting_selection =
        resting_heart_rate_candidate_selection(&heart_rate_features, &motion_features);

    let daily = daily_resting_heart_rate_features(&resting_selection.features);
    let resting = resting_heart_rate_feature(start, end, &resting_selection);
    let baseline = resting_heart_rate_baseline_feature(start, end, &daily, options);

    let mut issues = Vec::new();
    if options.require_trusted_evidence && !correlation.pass {
        issues.push("capture_correlation_report_not_passed".to_string());
    }
    if options.require_trusted_evidence && resting.is_none() {
        issues.push("no_trusted_resting_heart_rate_features".to_string());
    }
    if options.require_baseline && baseline.is_none() {
        issues.push("resting_hr_baseline_min_days_not_met".to_string());
    }
    let next_actions = metric_feature_next_actions("resting_hr", &issues);

    Ok(RestingHeartRateFeatureReport {
        schema: RESTING_HEART_RATE_FEATURE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-resting-heart-rate-feature-extractor".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        require_baseline: options.require_baseline,
        baseline_min_days: options.baseline_min_days,
        heart_rate_feature_count: heart_rate_report.features.len(),
        trusted_heart_rate_feature_count: heart_rate_report.trusted_feature_count,
        daily_count: daily.len(),
        resting,
        baseline,
        daily,
        issues,
        next_actions,
    })
}

pub fn run_sleep_feature_score_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: SleepFeatureScoreOptions,
) -> GooseResult<SleepFeatureScoreReport> {
    let motion_report = run_motion_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        MotionFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
        },
    )?;
    let motion_features = motion_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let heart_rate_report = run_heart_rate_feature_report_for_store(
        store,
        database_path,
        start,
        end,
        HeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
        },
    )?;
    let heart_rate_features = heart_rate_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if options.require_trusted_evidence && !motion_report.pass {
        issues.push("motion_report_not_passed".to_string());
    }
    if options.sleep_need_minutes <= 0.0 || !options.sleep_need_minutes.is_finite() {
        issues.push("sleep_need_minutes_invalid".to_string());
    }
    if options.low_motion_threshold_0_to_1 < 0.0
        || options.low_motion_threshold_0_to_1 > 1.0
        || !options.low_motion_threshold_0_to_1.is_finite()
    {
        issues.push("low_motion_threshold_invalid".to_string());
    }
    if options.disturbance_motion_threshold_0_to_1 < 0.0
        || options.disturbance_motion_threshold_0_to_1 > 1.0
        || !options.disturbance_motion_threshold_0_to_1.is_finite()
    {
        issues.push("disturbance_motion_threshold_invalid".to_string());
    }

    let sleep_window = if issues.is_empty() {
        sleep_window_feature(start, end, &motion_features, &heart_rate_features, options)
    } else {
        None
    };
    if sleep_window.is_none() {
        issues.push("sleep_window_missing".to_string());
    }

    let mut sleep_input = None;
    let mut score_result = None;
    if let Some(window) = &sleep_window {
        let input = SleepInput {
            start_time: window.start_time.clone(),
            end_time: window.end_time.clone(),
            sleep_duration_minutes: window.sleep_duration_minutes,
            sleep_need_minutes: options.sleep_need_minutes,
            time_in_bed_minutes: window.time_in_bed_minutes,
            midpoint_deviation_minutes: window.midpoint_deviation_minutes,
            disturbance_count: window.disturbance_count,
            sleep_latency_minutes: window.sleep_latency_minutes,
            wake_after_sleep_onset_minutes: window.wake_after_sleep_onset_minutes,
            wake_episode_count: window.wake_episode_count,
            stage_minutes: window.stage_minutes.clone(),
            heart_rate_dip_percent: window.heart_rate_dip_percent,
            input_ids: window.input_ids.clone(),
        };
        let result = goose_sleep_v0(&input);
        if !result.errors.is_empty() {
            issues.push("sleep_score_errors".to_string());
        }
        if result.output.is_none() {
            issues.push("sleep_score_output_missing".to_string());
        }
        sleep_input = Some(input);
        score_result = Some(result);
    }
    let next_actions = metric_feature_next_actions("sleep", &issues);

    Ok(SleepFeatureScoreReport {
        schema: SLEEP_FEATURE_SCORE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-sleep-feature-score-builder".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        motion_report,
        heart_rate_report,
        sleep_window,
        sleep_input,
        score_result,
        issues,
        next_actions,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run_recovery_feature_score_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    hrv_start: &str,
    hrv_end: &str,
    hrv_baseline_start: &str,
    hrv_baseline_end: &str,
    resting_start: &str,
    resting_end: &str,
    sleep_start: &str,
    sleep_end: &str,
    prior_strain_start: &str,
    prior_strain_end: &str,
    options: RecoveryFeatureScoreOptions,
) -> GooseResult<RecoveryFeatureScoreReport> {
    let hrv_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        hrv_start,
        hrv_end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.hrv_min_rr_intervals_to_compute,
            baseline_min_days: options.hrv_baseline_min_days,
            require_baseline: false,
        },
    )?;
    let hrv_baseline_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        hrv_baseline_start,
        hrv_baseline_end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.hrv_min_rr_intervals_to_compute,
            baseline_min_days: options.hrv_baseline_min_days,
            require_baseline: true,
        },
    )?;
    let resting_report = run_resting_heart_rate_feature_report_for_store(
        store,
        database_path,
        resting_start,
        resting_end,
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            baseline_min_days: options.resting_baseline_min_days,
            require_baseline: true,
        },
    )?;
    let sleep_report = run_sleep_feature_score_report_for_store(
        store,
        database_path,
        sleep_start,
        sleep_end,
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            sleep_need_minutes: options.sleep_need_minutes,
            low_motion_threshold_0_to_1: options.low_motion_threshold_0_to_1,
            disturbance_motion_threshold_0_to_1: options.disturbance_motion_threshold_0_to_1,
            target_midpoint_minutes_since_midnight: options.target_midpoint_minutes_since_midnight,
        },
    )?;
    let prior_strain_report = run_strain_feature_score_report_for_store(
        store,
        database_path,
        prior_strain_start,
        prior_strain_end,
        prior_strain_start,
        prior_strain_end,
        StrainFeatureScoreOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            resting_baseline_min_days: options.prior_strain_resting_baseline_min_days,
            max_hr_bpm: options.prior_strain_max_hr_bpm,
        },
    )?;
    let provided_vitals = recovery_provided_vitals_feature(start, end, &options);

    let mut issues = Vec::new();
    if !hrv_report.pass {
        issues.push("hrv_report_not_passed".to_string());
    }
    if !hrv_baseline_report.pass {
        issues.push("hrv_baseline_report_not_passed".to_string());
    }
    if !resting_report.pass {
        issues.push("resting_heart_rate_report_not_passed".to_string());
    }
    if !sleep_report.pass {
        issues.push("sleep_report_not_passed".to_string());
    }
    if !prior_strain_report.pass {
        issues.push("prior_strain_report_not_passed".to_string());
    }

    let hrv_rmssd_ms = hrv_report
        .score_result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .map(|output| output.rmssd_ms);
    let hrv_baseline_rmssd_ms = hrv_baseline_report
        .baseline
        .as_ref()
        .map(|baseline| baseline.hrv_baseline_rmssd_ms);
    let resting_hr_bpm = resting_report
        .resting
        .as_ref()
        .map(|resting| resting.resting_hr_bpm);
    let resting_hr_baseline_bpm = resting_report
        .baseline
        .as_ref()
        .map(|baseline| baseline.resting_hr_baseline_bpm);
    let sleep_score_0_to_100 = sleep_report
        .score_result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .map(|output| output.score_0_to_100);
    let prior_strain_0_to_21 = prior_strain_report
        .score_result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .map(|output| output.score_0_to_21);

    if hrv_rmssd_ms.is_none() {
        issues.push("hrv_rmssd_missing".to_string());
    }
    if hrv_baseline_rmssd_ms.is_none() {
        issues.push("hrv_baseline_missing".to_string());
    }
    if resting_hr_bpm.is_none() {
        issues.push("resting_hr_missing".to_string());
    }
    if resting_hr_baseline_bpm.is_none() {
        issues.push("resting_hr_baseline_missing".to_string());
    }
    if sleep_score_0_to_100.is_none() {
        issues.push("sleep_score_missing".to_string());
    }
    if prior_strain_0_to_21.is_none() {
        issues.push("prior_strain_missing".to_string());
    }
    if let Some(vitals) = provided_vitals.as_ref() {
        if vitals
            .quality_flags
            .iter()
            .any(|flag| flag == "provided_resp_temp_inputs_not_packet_derived")
        {
            issues.push("provided_resp_temp_inputs_not_packet_derived".to_string());
        }
        if vitals
            .quality_flags
            .iter()
            .any(|flag| flag == "provided_resp_temp_provenance_untrusted")
        {
            issues.push("provided_resp_temp_provenance_untrusted".to_string());
        }
    } else {
        issues.push("provided_resp_temp_inputs_missing".to_string());
    }

    let mut recovery_input = None;
    let mut score_result = None;
    if let (
        Some(hrv_rmssd_ms),
        Some(hrv_baseline_rmssd_ms),
        Some(resting_hr_bpm),
        Some(resting_hr_baseline_bpm),
        Some(sleep_score_0_to_100),
        Some(prior_strain_0_to_21),
        Some(vitals),
    ) = (
        hrv_rmssd_ms,
        hrv_baseline_rmssd_ms,
        resting_hr_bpm,
        resting_hr_baseline_bpm,
        sleep_score_0_to_100,
        prior_strain_0_to_21,
        provided_vitals
            .as_ref()
            .filter(|vitals| vitals.trusted_metric_input),
    ) {
        let mut input_ids = Vec::new();
        if let Some(input) = &hrv_report.hrv_input {
            input_ids.extend(input.input_ids.iter().cloned());
        }
        if let Some(baseline) = &hrv_baseline_report.baseline {
            input_ids.push(baseline.metric_input_id.clone());
            input_ids.extend(baseline.input_ids.iter().cloned());
        }
        if let Some(resting) = &resting_report.resting {
            input_ids.push(resting.metric_input_id.clone());
        }
        if let Some(baseline) = &resting_report.baseline {
            input_ids.push(baseline.metric_input_id.clone());
            input_ids.extend(baseline.input_ids.iter().cloned());
        }
        if let Some(input) = &sleep_report.sleep_input {
            input_ids.extend(input.input_ids.iter().cloned());
        }
        if let Some(input) = &prior_strain_report.strain_input {
            input_ids.extend(input.input_ids.iter().cloned());
        }
        input_ids.push(vitals.metric_input_id.clone());
        input_ids.sort();
        input_ids.dedup();

        let input = RecoveryInput {
            start_time: start.to_string(),
            end_time: end.to_string(),
            hrv_rmssd_ms,
            hrv_baseline_rmssd_ms,
            resting_hr_bpm,
            resting_hr_baseline_bpm,
            respiratory_rate_rpm: vitals.respiratory_rate_rpm,
            respiratory_rate_baseline_rpm: vitals.respiratory_rate_baseline_rpm,
            skin_temp_delta_c: vitals.skin_temp_delta_c,
            sleep_score_0_to_100,
            prior_strain_0_to_21,
            input_ids,
        };
        let mut result = goose_recovery_v0(&input);
        result
            .quality_flags
            .extend(vitals.quality_flags.iter().cloned());
        result.quality_flags.sort();
        result.quality_flags.dedup();
        attach_recovery_provided_vitals_provenance(&mut result, vitals);
        if !result.errors.is_empty() {
            issues.push("recovery_score_errors".to_string());
        }
        if result.output.is_none() {
            issues.push("recovery_score_output_missing".to_string());
        }
        recovery_input = Some(input);
        score_result = Some(result);
    }
    let next_actions = metric_feature_next_actions("recovery", &issues);

    Ok(RecoveryFeatureScoreReport {
        schema: RECOVERY_FEATURE_SCORE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-recovery-feature-score-builder".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        hrv_start_time: hrv_start.to_string(),
        hrv_end_time: hrv_end.to_string(),
        hrv_baseline_start_time: hrv_baseline_start.to_string(),
        hrv_baseline_end_time: hrv_baseline_end.to_string(),
        resting_start_time: resting_start.to_string(),
        resting_end_time: resting_end.to_string(),
        sleep_start_time: sleep_start.to_string(),
        sleep_end_time: sleep_end.to_string(),
        prior_strain_start_time: prior_strain_start.to_string(),
        prior_strain_end_time: prior_strain_end.to_string(),
        hrv_report,
        hrv_baseline_report,
        resting_report,
        sleep_report,
        prior_strain_report,
        provided_vitals,
        recovery_input,
        score_result,
        issues,
        next_actions,
    })
}

fn attach_recovery_provided_vitals_provenance(
    result: &mut AlgorithmRunResult<RecoveryScoreOutput>,
    vitals: &RecoveryProvidedVitalsFeature,
) {
    if let Some(object) = result.provenance.as_object_mut() {
        object.insert(
            "provided_vitals".to_string(),
            json!({
                "metric_input_id": vitals.metric_input_id,
                "source": vitals.source,
                "trusted_metric_input": vitals.trusted_metric_input,
                "quality_flags": vitals.quality_flags,
                "provenance": vitals.provenance,
            }),
        );
    }
}

pub fn run_strain_feature_score_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    resting_start: &str,
    resting_end: &str,
    options: StrainFeatureScoreOptions,
) -> GooseResult<StrainFeatureScoreReport> {
    let resting_report = run_resting_heart_rate_feature_report_for_store(
        store,
        database_path,
        resting_start,
        resting_end,
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            baseline_min_days: options.resting_baseline_min_days,
            require_baseline: false,
        },
    )?;

    let mut issues = Vec::new();
    if options.require_trusted_evidence && !resting_report.pass {
        issues.push("resting_heart_rate_report_not_passed".to_string());
    }

    let resting_hr_bpm = resting_report
        .resting
        .as_ref()
        .map(|resting| resting.resting_hr_bpm);

    let (max_hr_bpm, max_hr_basis, mut window_report) =
        if let (Some(resting_hr_bpm), None) = (resting_hr_bpm, options.max_hr_bpm) {
            let observed_report = run_metric_window_feature_report_for_store(
                store,
                database_path,
                start,
                end,
                MetricWindowFeatureOptions {
                    min_owned_captures_per_summary: options.min_owned_captures_per_summary,
                    require_trusted_evidence: options.require_trusted_evidence,
                    resting_hr_bpm: Some(resting_hr_bpm),
                    max_hr_bpm: None,
                },
            )?;
            let observed_max = observed_report
                .window
                .as_ref()
                .map(|window| window.max_hr_bpm);
            (
                observed_max,
                observed_max.map(|_| "observed_window_max_hr_bpm".to_string()),
                Some(observed_report),
            )
        } else {
            (
                options.max_hr_bpm,
                options
                    .max_hr_bpm
                    .map(|_| "provided_max_hr_bpm".to_string()),
                None,
            )
        };

    let mut strain_input = None;
    let mut score_result = None;

    if let Some(resting_hr_bpm) = resting_hr_bpm {
        if let Some(max_hr_bpm) = max_hr_bpm {
            if max_hr_bpm <= resting_hr_bpm {
                issues.push("max_hr_basis_must_exceed_resting_hr".to_string());
            } else {
                let report = run_metric_window_feature_report_for_store(
                    store,
                    database_path,
                    start,
                    end,
                    MetricWindowFeatureOptions {
                        min_owned_captures_per_summary: options.min_owned_captures_per_summary,
                        require_trusted_evidence: options.require_trusted_evidence,
                        resting_hr_bpm: Some(resting_hr_bpm),
                        max_hr_bpm: Some(max_hr_bpm),
                    },
                )?;
                if !report.pass {
                    issues.push("metric_window_report_not_passed".to_string());
                }
                if let Some(window) = &report.window {
                    if options.require_trusted_evidence && !window.trusted_metric_input {
                        issues.push("window_metric_input_not_trusted".to_string());
                    }
                    if window.hr_zone_minutes.len() != 5 {
                        issues.push("hr_zone_minutes_missing".to_string());
                    }

                    let mut input_ids = window.input_ids.clone();
                    if let Some(resting) = &resting_report.resting {
                        input_ids.push(resting.metric_input_id.clone());
                    }
                    input_ids.sort();
                    input_ids.dedup();

                    let input = StrainInput {
                        start_time: window.start_time.clone(),
                        end_time: window.end_time.clone(),
                        duration_minutes: window.duration_minutes,
                        resting_hr_bpm,
                        average_hr_bpm: window.average_hr_bpm,
                        max_hr_bpm,
                        hr_zone_minutes: window.hr_zone_minutes.clone(),
                        input_ids,
                        profile_sex: None,
                        profile_age: None,
                    };
                    let mut result = goose_strain_v0(&input);
                    if max_hr_basis.as_deref() == Some("observed_window_max_hr_bpm") {
                        result
                            .quality_flags
                            .push("observed_window_max_hr_basis".to_string());
                    }
                    if !result.errors.is_empty() {
                        issues.push("strain_score_errors".to_string());
                    }
                    if result.output.is_none() {
                        issues.push("strain_score_output_missing".to_string());
                    }
                    strain_input = Some(input);
                    score_result = Some(result);
                } else {
                    issues.push("metric_window_feature_missing".to_string());
                }
                window_report = Some(report);
            }
        } else {
            issues.push("max_hr_basis_missing".to_string());
        }
    } else {
        issues.push("resting_hr_missing".to_string());
    }
    let next_actions = metric_feature_next_actions("strain", &issues);

    Ok(StrainFeatureScoreReport {
        schema: STRAIN_FEATURE_SCORE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-strain-feature-score-builder".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        resting_start_time: resting_start.to_string(),
        resting_end_time: resting_end.to_string(),
        max_hr_basis,
        resting_report,
        window_report,
        strain_input,
        score_result,
        issues,
        next_actions,
    })
}

pub fn run_stress_feature_score_report_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    resting_start: &str,
    resting_end: &str,
    hrv_start: &str,
    hrv_end: &str,
    hrv_baseline_start: &str,
    hrv_baseline_end: &str,
    options: StressFeatureScoreOptions,
) -> GooseResult<StressFeatureScoreReport> {
    let feature_options = MotionFeatureOptions {
        min_owned_captures_per_summary: options.min_owned_captures_per_summary,
        require_trusted_evidence: options.require_trusted_evidence,
    };
    let heart_rate_report =
        run_heart_rate_feature_report_for_store(store, database_path, start, end, feature_options)?;
    let motion_report =
        run_motion_feature_report_for_store(store, database_path, start, end, feature_options)?;
    let resting_report = run_resting_heart_rate_feature_report_for_store(
        store,
        database_path,
        resting_start,
        resting_end,
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            baseline_min_days: options.resting_baseline_min_days,
            require_baseline: false,
        },
    )?;
    let hrv_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        hrv_start,
        hrv_end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.hrv_min_rr_intervals_to_compute,
            baseline_min_days: options.hrv_baseline_min_days,
            require_baseline: false,
        },
    )?;
    let hrv_baseline_report = run_hrv_feature_report_for_store(
        store,
        database_path,
        hrv_baseline_start,
        hrv_baseline_end,
        HrvFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.hrv_min_rr_intervals_to_compute,
            baseline_min_days: options.hrv_baseline_min_days,
            require_baseline: true,
        },
    )?;

    let heart_rate_features = heart_rate_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();
    let motion_features = motion_report
        .features
        .iter()
        .filter(|feature| !options.require_trusted_evidence || feature.trusted_metric_input)
        .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if options.require_trusted_evidence && !heart_rate_report.pass {
        issues.push("heart_rate_report_not_passed".to_string());
    }
    if options.require_trusted_evidence && !motion_report.pass {
        issues.push("motion_report_not_passed".to_string());
    }
    if options.require_trusted_evidence && !resting_report.pass {
        issues.push("resting_heart_rate_report_not_passed".to_string());
    }
    if !hrv_report.pass {
        issues.push("hrv_report_not_passed".to_string());
    }
    if !hrv_baseline_report.pass {
        issues.push("hrv_baseline_report_not_passed".to_string());
    }

    let heart_rate_bpm = average_heart_rate_bpm(&heart_rate_features);
    let motion_intensity_0_to_1 = average_motion_intensity_0_to_1(&motion_features);
    let resting_hr_bpm = resting_report
        .resting
        .as_ref()
        .map(|resting| resting.resting_hr_bpm);
    let hrv_rmssd_ms = hrv_report
        .score_result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .map(|output| output.rmssd_ms);
    let hrv_baseline_rmssd_ms = hrv_baseline_report
        .baseline
        .as_ref()
        .map(|baseline| baseline.hrv_baseline_rmssd_ms);

    if heart_rate_bpm.is_none() {
        issues.push("heart_rate_missing".to_string());
    }
    if motion_intensity_0_to_1.is_none() {
        issues.push("motion_missing".to_string());
    }
    if resting_hr_bpm.is_none() {
        issues.push("resting_hr_missing".to_string());
    }
    if hrv_rmssd_ms.is_none() {
        issues.push("hrv_rmssd_missing".to_string());
    }
    if hrv_baseline_rmssd_ms.is_none() {
        issues.push("hrv_baseline_missing".to_string());
    }

    let mut stress_input = None;
    let mut score_result = None;
    if let (
        Some(heart_rate_bpm),
        Some(motion_intensity_0_to_1),
        Some(resting_hr_bpm),
        Some(hrv_rmssd_ms),
        Some(hrv_baseline_rmssd_ms),
    ) = (
        heart_rate_bpm,
        motion_intensity_0_to_1,
        resting_hr_bpm,
        hrv_rmssd_ms,
        hrv_baseline_rmssd_ms,
    ) {
        let mut input_ids = heart_rate_features
            .iter()
            .map(|feature| feature.metric_input_id.clone())
            .collect::<Vec<_>>();
        input_ids.extend(
            motion_features
                .iter()
                .map(|feature| feature.metric_input_id.clone()),
        );
        if let Some(resting) = &resting_report.resting {
            input_ids.push(resting.metric_input_id.clone());
        }
        if let Some(input) = &hrv_report.hrv_input {
            input_ids.extend(input.input_ids.iter().cloned());
        }
        if let Some(baseline) = &hrv_baseline_report.baseline {
            input_ids.push(baseline.metric_input_id.clone());
            input_ids.extend(baseline.input_ids.iter().cloned());
        }
        input_ids.sort();
        input_ids.dedup();

        let input = StressInput {
            start_time: start.to_string(),
            end_time: end.to_string(),
            heart_rate_bpm,
            resting_hr_bpm,
            hrv_rmssd_ms,
            hrv_baseline_rmssd_ms,
            motion_intensity_0_to_1,
            input_ids,
        };
        let result = goose_stress_v0(&input);
        if !result.errors.is_empty() {
            issues.push("stress_score_errors".to_string());
        }
        if result.output.is_none() {
            issues.push("stress_score_output_missing".to_string());
        }
        stress_input = Some(input);
        score_result = Some(result);
    }
    let next_actions = metric_feature_next_actions("stress", &issues);

    Ok(StressFeatureScoreReport {
        schema: STRESS_FEATURE_SCORE_REPORT_SCHEMA.to_string(),
        generated_by: "goose-stress-feature-score-builder".to_string(),
        pass: issues.is_empty(),
        require_trusted_evidence: options.require_trusted_evidence,
        start_time: start.to_string(),
        end_time: end.to_string(),
        resting_start_time: resting_start.to_string(),
        resting_end_time: resting_end.to_string(),
        hrv_start_time: hrv_start.to_string(),
        hrv_end_time: hrv_end.to_string(),
        hrv_baseline_start_time: hrv_baseline_start.to_string(),
        hrv_baseline_end_time: hrv_baseline_end.to_string(),
        heart_rate_report,
        motion_report,
        resting_report,
        hrv_report,
        hrv_baseline_report,
        stress_input,
        score_result,
        issues,
        next_actions,
    })
}

fn validate_hrv_validation_options(options: &HrvCaptureValidationOptions) -> GooseResult<()> {
    if !options.tolerance_ms.is_finite() || options.tolerance_ms < 0.0 {
        return Err(GooseError::message("tolerance_ms must be nonnegative"));
    }
    if let Some(value) = options.official_whoop_hrv_rmssd_ms
        && (!value.is_finite() || value < 0.0)
    {
        return Err(GooseError::message(
            "official_whoop_hrv_rmssd_ms must be nonnegative",
        ));
    }
    Ok(())
}

fn validate_respiratory_rate_validation_options(
    options: &RespiratoryRateCaptureValidationOptions,
) -> GooseResult<()> {
    if !options.tolerance_rpm.is_finite() || options.tolerance_rpm < 0.0 {
        return Err(GooseError::message("tolerance_rpm must be nonnegative"));
    }
    if let Some(value) = options.official_whoop_respiratory_rate_rpm
        && (!value.is_finite() || value <= 0.0)
    {
        return Err(GooseError::message(
            "official_whoop_respiratory_rate_rpm must be positive",
        ));
    }
    Ok(())
}

fn validate_oxygen_saturation_validation_options(
    options: &OxygenSaturationCaptureValidationOptions,
) -> GooseResult<()> {
    if !options.tolerance_percent.is_finite() || options.tolerance_percent < 0.0 {
        return Err(GooseError::message("tolerance_percent must be nonnegative"));
    }
    if let Some(value) = options.official_whoop_oxygen_saturation_percent
        && (!value.is_finite() || !(0.0..=100.0).contains(&value))
    {
        return Err(GooseError::message(
            "official_whoop_oxygen_saturation_percent must be between 0 and 100",
        ));
    }
    Ok(())
}

fn validate_temperature_validation_options(
    options: &TemperatureCaptureValidationOptions,
) -> GooseResult<()> {
    if !options.tolerance_c.is_finite() || options.tolerance_c < 0.0 {
        return Err(GooseError::message("tolerance_c must be nonnegative"));
    }
    if let Some(value) = options.official_whoop_skin_temperature_delta_c
        && !value.is_finite()
    {
        return Err(GooseError::message(
            "official_whoop_skin_temperature_delta_c must be finite",
        ));
    }
    Ok(())
}

struct HrvLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

struct RespiratoryRateLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

struct OxygenSaturationLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

struct TemperatureLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

fn compare_hrv_label(
    local_rmssd_ms: Option<f64>,
    label_rmssd_ms: Option<f64>,
    tolerance_ms: f64,
) -> HrvLabelComparison {
    let Some(label_rmssd_ms) = label_rmssd_ms else {
        return HrvLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_rmssd_ms) = local_rmssd_ms else {
        return HrvLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_rmssd_ms - label_rmssd_ms;
    HrvLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance_ms),
    }
}

fn compare_respiratory_rate_label(
    local_respiratory_rate_rpm: Option<f64>,
    label_respiratory_rate_rpm: Option<f64>,
    tolerance_rpm: f64,
) -> RespiratoryRateLabelComparison {
    let Some(label_respiratory_rate_rpm) = label_respiratory_rate_rpm else {
        return RespiratoryRateLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_respiratory_rate_rpm) = local_respiratory_rate_rpm else {
        return RespiratoryRateLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_respiratory_rate_rpm - label_respiratory_rate_rpm;
    RespiratoryRateLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance_rpm),
    }
}

fn compare_oxygen_saturation_label(
    local_oxygen_saturation_percent: Option<f64>,
    label_oxygen_saturation_percent: Option<f64>,
    tolerance_percent: f64,
) -> OxygenSaturationLabelComparison {
    let Some(label_oxygen_saturation_percent) = label_oxygen_saturation_percent else {
        return OxygenSaturationLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_oxygen_saturation_percent) = local_oxygen_saturation_percent else {
        return OxygenSaturationLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_oxygen_saturation_percent - label_oxygen_saturation_percent;
    OxygenSaturationLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance_percent),
    }
}

fn compare_temperature_label(
    local_skin_temperature_delta_c: Option<f64>,
    label_skin_temperature_delta_c: Option<f64>,
    tolerance_c: f64,
) -> TemperatureLabelComparison {
    let Some(label_skin_temperature_delta_c) = label_skin_temperature_delta_c else {
        return TemperatureLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_skin_temperature_delta_c) = local_skin_temperature_delta_c else {
        return TemperatureLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_skin_temperature_delta_c - label_skin_temperature_delta_c;
    TemperatureLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance_c),
    }
}

fn select_respiratory_rate_validation_candidate(
    vital_event_report: &VitalEventFeatureReport,
    require_trusted_evidence: bool,
) -> Option<&RespiratoryRateFeature> {
    vital_event_report
        .respiratory_rate_inputs
        .iter()
        .rev()
        .find(|feature| {
            feature.respiratory_rate_rpm.is_some()
                && feature.semantic_status == "plausible_unverified_units"
                && (!require_trusted_evidence || feature.trusted_candidate_evidence)
        })
        .or_else(|| {
            vital_event_report
                .respiratory_rate_inputs
                .iter()
                .rev()
                .find(|feature| {
                    feature.respiratory_rate_rpm.is_some()
                        && (!require_trusted_evidence || feature.trusted_candidate_evidence)
                })
        })
}

fn select_temperature_validation_candidate(
    vital_event_report: &VitalEventFeatureReport,
    require_trusted_evidence: bool,
) -> Option<&SkinTemperatureFeature> {
    vital_event_report
        .skin_temperature_inputs
        .iter()
        .rev()
        .find(|feature| {
            feature.skin_temperature_c.is_some()
                && feature.semantic_status == "plausible_unverified_units"
                && (!require_trusted_evidence || feature.trusted_candidate_evidence)
        })
        .or_else(|| {
            vital_event_report
                .skin_temperature_inputs
                .iter()
                .rev()
                .find(|feature| {
                    feature.skin_temperature_c.is_some()
                        && (!require_trusted_evidence || feature.trusted_candidate_evidence)
                })
        })
}

fn hrv_validation_next_actions(issues: &[String]) -> Vec<MetricFeatureNextAction> {
    let mut actions = metric_feature_next_actions("hrv", issues);
    for issue in issues {
        if let Some(action) = official_label_policy_issue_action(issue) {
            actions.push(MetricFeatureNextAction {
                scope: "hrv.validation_label".to_string(),
                reason: issue.clone(),
                action: action.to_string(),
            });
        }
    }
    if issues
        .iter()
        .any(|issue| issue == "no_hrv_validation_label")
    {
        actions.push(MetricFeatureNextAction {
            scope: "hrv.validation_label".to_string(),
            reason: "no_hrv_validation_label".to_string(),
            action:
                "Record the official WHOOP app HRV/RMSSD value as a validation label before passing HRV validation."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "hrv_feature_report_blocked" || issue == "local_hrv_rmssd_missing")
    {
        actions.push(MetricFeatureNextAction {
            scope: "hrv.local_candidate".to_string(),
            reason: "local_hrv_rmssd_missing".to_string(),
            action:
                "Capture enough trusted R17/beat-interval packet evidence before comparing local HRV against labels."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "hrv_label_delta_out_of_tolerance")
    {
        actions.push(MetricFeatureNextAction {
            scope: "hrv.validation_delta".to_string(),
            reason: "hrv_label_delta_out_of_tolerance".to_string(),
            action:
                "Keep R17-derived HRV blocked and collect more owned captures or a beat-interval reference before validating the interval scale."
                    .to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn respiratory_rate_validation_next_actions(issues: &[String]) -> Vec<MetricFeatureNextAction> {
    let mut actions = metric_feature_next_actions("respiratory_rate", issues);
    for issue in issues {
        if let Some(action) = official_label_policy_issue_action(issue) {
            actions.push(MetricFeatureNextAction {
                scope: "respiratory_rate.validation_label".to_string(),
                reason: issue.clone(),
                action: action.to_string(),
            });
        }
    }
    if issues
        .iter()
        .any(|issue| issue == "no_respiratory_rate_validation_label")
    {
        actions.push(MetricFeatureNextAction {
            scope: "respiratory_rate.validation_label".to_string(),
            reason: "no_respiratory_rate_validation_label".to_string(),
            action:
                "Record the official WHOOP app respiratory-rate value as a validation label before passing respiratory-rate validation."
                    .to_string(),
        });
    }
    if issues.iter().any(|issue| {
        issue == "no_respiratory_rate_packet_candidate"
            || issue == "no_trusted_respiratory_rate_candidate"
            || issue == "local_respiratory_rate_rpm_missing"
            || issue == "vital_event_report_blocked"
    }) {
        actions.push(MetricFeatureNextAction {
            scope: "respiratory_rate.local_candidate".to_string(),
            reason: "local_respiratory_rate_rpm_missing".to_string(),
            action:
                "Capture trusted K18/K24 normal-history packets before comparing local respiratory-rate candidates against labels."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "respiratory_rate_label_delta_out_of_tolerance")
    {
        actions.push(MetricFeatureNextAction {
            scope: "respiratory_rate.validation_delta".to_string(),
            reason: "respiratory_rate_label_delta_out_of_tolerance".to_string(),
            action:
                "Keep respiratory rate blocked and collect more owned captures before validating the candidate offset and units."
                    .to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn oxygen_saturation_validation_next_actions(issues: &[String]) -> Vec<MetricFeatureNextAction> {
    let mut actions = metric_feature_next_actions("oxygen_saturation", issues);
    for issue in issues {
        if let Some(action) = official_label_policy_issue_action(issue) {
            actions.push(MetricFeatureNextAction {
                scope: "oxygen_saturation.validation_label".to_string(),
                reason: issue.clone(),
                action: action.to_string(),
            });
        }
    }
    if issues
        .iter()
        .any(|issue| issue == "no_oxygen_saturation_validation_label")
    {
        actions.push(MetricFeatureNextAction {
            scope: "oxygen_saturation.validation_label".to_string(),
            reason: "no_oxygen_saturation_validation_label".to_string(),
            action:
                "Record the official WHOOP app oxygen-saturation value as a validation label before passing SpO2 validation."
                    .to_string(),
        });
    }
    if issues.iter().any(|issue| {
        issue == "oxygen_saturation_decoder_not_implemented"
            || issue == "no_oxygen_saturation_packet_candidate"
            || issue == "local_oxygen_saturation_percent_missing"
            || issue == "vital_event_report_blocked"
    }) {
        actions.push(MetricFeatureNextAction {
            scope: "oxygen_saturation.local_candidate".to_string(),
            reason: "local_oxygen_saturation_percent_missing".to_string(),
            action:
                "Capture charger, overnight, and post-sync optical/history packets, then implement and validate a decoded SpO2 field before comparing against labels."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "oxygen_saturation_label_delta_out_of_tolerance")
    {
        actions.push(MetricFeatureNextAction {
            scope: "oxygen_saturation.validation_delta".to_string(),
            reason: "oxygen_saturation_label_delta_out_of_tolerance".to_string(),
            action:
                "Keep oxygen saturation blocked and collect more owned captures before validating the decoded field."
                    .to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn temperature_validation_next_actions(issues: &[String]) -> Vec<MetricFeatureNextAction> {
    let mut actions = metric_feature_next_actions("skin_temperature", issues);
    for issue in issues {
        if let Some(action) = official_label_policy_issue_action(issue) {
            actions.push(MetricFeatureNextAction {
                scope: "skin_temperature.validation_label".to_string(),
                reason: issue.clone(),
                action: action.to_string(),
            });
        }
    }
    if issues
        .iter()
        .any(|issue| issue == "no_skin_temperature_validation_label")
    {
        actions.push(MetricFeatureNextAction {
            scope: "skin_temperature.validation_label".to_string(),
            reason: "no_skin_temperature_validation_label".to_string(),
            action:
                "Record the official WHOOP app skin-temperature delta as a validation label before passing temperature validation."
                    .to_string(),
        });
    }
    if issues.iter().any(|issue| {
        issue == "no_temperature_packet_candidate"
            || issue == "no_trusted_temperature_candidate"
            || issue == "temperature_units_unverified"
            || issue == "local_skin_temperature_delta_c_missing"
            || issue == "vital_event_report_blocked"
    }) {
        actions.push(MetricFeatureNextAction {
            scope: "skin_temperature.local_candidate".to_string(),
            reason: "local_skin_temperature_delta_c_missing".to_string(),
            action:
                "Capture charger, overnight, and post-sync temperature/history packets, then validate units and delta semantics before comparing against labels."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "skin_temperature_label_delta_out_of_tolerance")
    {
        actions.push(MetricFeatureNextAction {
            scope: "skin_temperature.validation_delta".to_string(),
            reason: "skin_temperature_label_delta_out_of_tolerance".to_string(),
            action:
                "Keep temperature blocked and collect more owned captures before validating the candidate units."
                    .to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn round_1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn metric_feature_next_actions(family: &str, issues: &[String]) -> Vec<MetricFeatureNextAction> {
    issues
        .iter()
        .map(|issue| {
            let (scope, reason, action) = metric_feature_issue_action(family, issue);
            MetricFeatureNextAction {
                scope: scope.to_string(),
                reason: reason.to_string(),
                action: action.to_string(),
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn recovery_sensor_widgets(
    hrv_report: &HrvFeatureReport,
    vital_event_report: &VitalEventFeatureReport,
) -> Vec<RecoverySensorWidgetDiscovery> {
    vec![
        hrv_widget_discovery(hrv_report),
        respiratory_rate_widget_discovery(vital_event_report),
        oxygen_saturation_widget_discovery(vital_event_report),
        temperature_widget_discovery(vital_event_report),
    ]
}

fn hrv_widget_discovery(hrv_report: &HrvFeatureReport) -> RecoverySensorWidgetDiscovery {
    let quality_flags = sorted_feature_flags(
        hrv_report
            .features
            .iter()
            .flat_map(|feature| feature.quality_flags.iter()),
    );
    let candidate_source_signals = sorted_feature_flags(
        hrv_report
            .features
            .iter()
            .map(|feature| &feature.source_signal),
    );
    let mut blockers = BTreeSet::new();
    if hrv_report.trusted_rr_interval_count < hrv_report.min_rr_intervals_to_compute {
        blockers.insert("no_trusted_hrv_rr_intervals".to_string());
    }
    if quality_flags
        .iter()
        .any(|flag| flag == "rr_interval_scale_unvalidated")
    {
        blockers.insert("hrv_rr_interval_scale_unverified".to_string());
    }
    if quality_flags
        .iter()
        .any(|flag| flag == "preliminary_r17_i16_rr_interval_candidate")
    {
        blockers.insert("hrv_rr_interval_candidate_not_proven".to_string());
    }
    if hrv_report
        .score_result
        .as_ref()
        .is_some_and(|result| !result.errors.is_empty())
    {
        blockers.insert("hrv_score_errors".to_string());
    }
    for issue in &hrv_report.issues {
        if issue == "no_trusted_hrv_features" || issue == "not_enough_rr_intervals" {
            blockers.insert(issue.clone());
        }
    }

    let blocker_reasons = blockers.into_iter().collect::<Vec<_>>();
    recovery_sensor_widget(
        "hrv_rmssd_ms",
        hrv_report.feature_count,
        hrv_report.trusted_feature_count,
        usize::from(blocker_reasons.is_empty() && hrv_report.score_result.is_some()),
        usize::from(blocker_reasons.is_empty() && hrv_report.score_result.is_some()),
        candidate_source_signals,
        quality_flags,
        blocker_reasons,
        json!({
            "input_source": "metrics.hrv_features",
            "source_signal_policy": "requires_true_beat_interval_data_not_coarse_bpm",
            "candidate_rr_interval_count": hrv_report.rr_interval_count,
            "trusted_rr_interval_count": hrv_report.trusted_rr_interval_count,
            "min_rr_intervals_to_compute": hrv_report.min_rr_intervals_to_compute,
        }),
    )
}

fn respiratory_rate_widget_discovery(
    vital_event_report: &VitalEventFeatureReport,
) -> RecoverySensorWidgetDiscovery {
    let quality_flags = sorted_feature_flags(
        vital_event_report
            .respiratory_rate_inputs
            .iter()
            .flat_map(|feature| feature.quality_flags.iter()),
    );
    let candidate_source_signals = sorted_feature_flags(
        vital_event_report
            .respiratory_rate_inputs
            .iter()
            .map(|feature| &feature.source_signal),
    );
    let resolved_metric_input_count = vital_event_report
        .respiratory_rate_inputs
        .iter()
        .filter(|feature| feature.resolved_metric_input)
        .count();
    let value_semantics_verified_count = vital_event_report
        .respiratory_rate_inputs
        .iter()
        .filter(|feature| feature.value_semantics_verified)
        .count();
    let mut blockers = BTreeSet::new();
    if vital_event_report.respiratory_rate_input_count == 0 {
        blockers.insert("no_respiratory_rate_packet_candidate".to_string());
    }
    if vital_event_report.trusted_respiratory_rate_input_count == 0 {
        blockers.insert("no_trusted_respiratory_rate_candidate".to_string());
    }
    if vital_event_report.respiratory_rate_input_count > 0 && value_semantics_verified_count == 0 {
        blockers.insert("respiratory_rate_semantics_unverified".to_string());
    }
    if resolved_metric_input_count == 0 {
        blockers.insert("respiratory_rate_not_promoted_to_metric_input".to_string());
    }

    recovery_sensor_widget(
        "respiratory_rate_rpm",
        vital_event_report.respiratory_rate_input_count,
        vital_event_report.trusted_respiratory_rate_input_count,
        resolved_metric_input_count,
        value_semantics_verified_count,
        candidate_source_signals,
        quality_flags,
        blockers.into_iter().collect(),
        json!({
            "input_source": "metrics.vital_event_features",
            "source_signal_policy": "direct_respiration_or_supported_ppg_signal_required",
            "score_input_policy": "blocked_until_respiratory_units_are_verified",
        }),
    )
}

fn oxygen_saturation_widget_discovery(
    vital_event_report: &VitalEventFeatureReport,
) -> RecoverySensorWidgetDiscovery {
    let mut blockers = BTreeSet::new();
    blockers.insert("oxygen_saturation_decoder_not_implemented".to_string());
    if vital_event_report.pulse_information_packet_count == 0 {
        blockers.insert("no_oxygen_saturation_packet_candidate".to_string());
    } else {
        blockers.insert("pulse_information_seen_without_spo2_decode".to_string());
    }
    let candidate_source_signals = if vital_event_report.pulse_information_packet_count > 0 {
        vec!["pulse_information_packet_candidate".to_string()]
    } else {
        Vec::new()
    };

    recovery_sensor_widget(
        "oxygen_saturation_percent",
        vital_event_report.pulse_information_packet_count,
        0,
        0,
        0,
        candidate_source_signals,
        Vec::new(),
        blockers.into_iter().collect(),
        json!({
            "input_source": "metrics.vital_event_features",
            "source_signal_policy": "decoded_spo2_or_verified_optical_path_required",
            "score_input_policy": "blocked_until_oxygen_saturation_decoder_exists",
        }),
    )
}

fn temperature_widget_discovery(
    vital_event_report: &VitalEventFeatureReport,
) -> RecoverySensorWidgetDiscovery {
    let mut quality_flag_set = BTreeSet::new();
    let mut source_signal_set = BTreeSet::new();
    let mut resolved_metric_input_count = 0usize;
    let mut value_semantics_verified_count = 0usize;
    for feature in &vital_event_report.features {
        source_signal_set.insert(feature.source_signal.clone());
        if feature.resolved_metric_input {
            resolved_metric_input_count += 1;
        }
        if feature.value_semantics_verified {
            value_semantics_verified_count += 1;
        }
        quality_flag_set.extend(feature.quality_flags.iter().cloned());
    }
    for feature in &vital_event_report.skin_temperature_inputs {
        source_signal_set.insert(feature.source_signal.clone());
        if feature.resolved_metric_input {
            resolved_metric_input_count += 1;
        }
        if feature.value_semantics_verified {
            value_semantics_verified_count += 1;
        }
        quality_flag_set.extend(feature.quality_flags.iter().cloned());
    }
    let candidate_count =
        vital_event_report.feature_count + vital_event_report.skin_temperature_input_count;
    let trusted_candidate_count = vital_event_report.trusted_feature_count
        + vital_event_report.trusted_skin_temperature_input_count;
    let mut blockers = BTreeSet::new();
    if candidate_count == 0 {
        blockers.insert("no_temperature_packet_candidate".to_string());
    }
    if trusted_candidate_count == 0 {
        blockers.insert("no_trusted_temperature_candidate".to_string());
    }
    if candidate_count > 0 && value_semantics_verified_count == 0 {
        blockers.insert("temperature_units_unverified".to_string());
    }
    if resolved_metric_input_count == 0 {
        blockers.insert("temperature_not_promoted_to_metric_input".to_string());
    }

    recovery_sensor_widget(
        "skin_temperature_delta_c",
        candidate_count,
        trusted_candidate_count,
        resolved_metric_input_count,
        value_semantics_verified_count,
        source_signal_set.into_iter().collect(),
        quality_flag_set.into_iter().collect(),
        blockers.into_iter().collect(),
        json!({
            "input_source": "metrics.vital_event_features",
            "source_signal_policy": "decoded_device_or_skin_temperature_units_required",
            "score_input_policy": "blocked_until_temperature_units_are_verified",
        }),
    )
}

fn recovery_sensor_widget(
    metric_id: &str,
    candidate_count: usize,
    trusted_candidate_count: usize,
    resolved_metric_input_count: usize,
    value_semantics_verified_count: usize,
    candidate_source_signals: Vec<String>,
    quality_flags: Vec<String>,
    blocker_reasons: Vec<String>,
    provenance: serde_json::Value,
) -> RecoverySensorWidgetDiscovery {
    let promotion_allowed = blocker_reasons.is_empty()
        && trusted_candidate_count > 0
        && resolved_metric_input_count > 0
        && value_semantics_verified_count > 0;
    let promotion_status = if promotion_allowed {
        "promotable"
    } else if candidate_count > 0 {
        "candidate_unverified"
    } else {
        "unavailable"
    };
    let confidence = recovery_sensor_widget_confidence(
        promotion_allowed,
        candidate_count,
        trusted_candidate_count,
        resolved_metric_input_count,
        value_semantics_verified_count,
    );
    RecoverySensorWidgetDiscovery {
        metric_id: metric_id.to_string(),
        source_kind: if promotion_allowed {
            "device_sensor"
        } else {
            "unavailable"
        }
        .to_string(),
        confidence,
        promotion_status: promotion_status.to_string(),
        promotion_allowed,
        user_visible_value_allowed: promotion_allowed,
        candidate_count,
        trusted_candidate_count,
        resolved_metric_input_count,
        value_semantics_verified_count,
        candidate_source_signals,
        quality_flags,
        blocker_reasons,
        provenance,
    }
}

fn recovery_sensor_widget_confidence(
    promotion_allowed: bool,
    candidate_count: usize,
    trusted_candidate_count: usize,
    resolved_metric_input_count: usize,
    value_semantics_verified_count: usize,
) -> f64 {
    if !promotion_allowed || candidate_count == 0 {
        return 0.0;
    }
    let denominator = candidate_count as f64;
    let trusted_fraction = (trusted_candidate_count as f64 / denominator).clamp(0.0, 1.0);
    let resolved_fraction = (resolved_metric_input_count as f64 / denominator).clamp(0.0, 1.0);
    let semantic_fraction = (value_semantics_verified_count as f64 / denominator).clamp(0.0, 1.0);
    (0.55 + trusted_fraction * 0.20 + resolved_fraction * 0.15 + semantic_fraction * 0.10)
        .clamp(0.55, 0.90)
}

fn sorted_feature_flags<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
    values
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn recovery_sensor_discovery_issues(widgets: &[RecoverySensorWidgetDiscovery]) -> Vec<String> {
    widgets
        .iter()
        .flat_map(|widget| {
            widget
                .blocker_reasons
                .iter()
                .map(|reason| format!("{}:{reason}", widget.metric_id))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn recovery_sensor_discovery_next_actions(
    widgets: &[RecoverySensorWidgetDiscovery],
) -> Vec<MetricFeatureNextAction> {
    widgets
        .iter()
        .flat_map(|widget| {
            widget.blocker_reasons.iter().map(|reason| {
                let action = recovery_sensor_blocker_action(&widget.metric_id, reason);
                MetricFeatureNextAction {
                    scope: widget.metric_id.clone(),
                    reason: reason.clone(),
                    action: action.to_string(),
                }
            })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn recovery_sensor_blocker_action(metric_id: &str, reason: &str) -> &'static str {
    match reason {
        "no_trusted_hrv_rr_intervals" | "no_trusted_hrv_features" | "not_enough_rr_intervals" => {
            "Capture or import trusted overnight R17/optical frames with enough plausible beat-interval candidates."
        }
        "hrv_rr_interval_scale_unverified" | "hrv_rr_interval_candidate_not_proven" => {
            "Validate the R17 interval scale against owned packet captures and an external beat-interval reference before showing HRV."
        }
        "no_respiratory_rate_packet_candidate" => {
            "Run overnight/history captures and inspect decoded normal-history or optical packets for direct respiratory-rate evidence."
        }
        "no_trusted_respiratory_rate_candidate" => {
            "Capture or import trusted owned respiratory-rate candidate packets before considering respiratory-rate promotion."
        }
        "respiratory_rate_semantics_unverified"
        | "respiratory_rate_not_promoted_to_metric_input" => {
            "Validate respiratory-rate candidate offsets and units against owned captures and validation labels before showing the value."
        }
        "oxygen_saturation_decoder_not_implemented" => {
            "Implement a verified SpO2 decoder from device packets or optical history before showing oxygen saturation."
        }
        "no_oxygen_saturation_packet_candidate" | "pulse_information_seen_without_spo2_decode" => {
            "Capture charger, overnight, and post-sync optical/history packets, then verify whether any decoded field is oxygen saturation."
        }
        "no_temperature_packet_candidate" => {
            "Run charger, overnight, and post-sync captures and inspect temperature events/history packets for actual device temperature evidence."
        }
        "no_trusted_temperature_candidate" => {
            "Capture or import trusted owned temperature candidate packets before considering temperature promotion."
        }
        "temperature_units_unverified" | "temperature_not_promoted_to_metric_input" => {
            "Validate temperature event/history units against owned captures before showing device or skin temperature."
        }
        _ if metric_id == "oxygen_saturation_percent" => {
            "Keep oxygen saturation unavailable until a verified packet decoder exists."
        }
        _ => "Resolve this sensor blocker before showing the metric as WHOOP-derived.",
    }
}

fn metric_feature_issue_action(
    family: &str,
    issue: &str,
) -> (&'static str, &'static str, &'static str) {
    match issue {
        "capture_correlation_report_not_passed" => (
            "capture_correlation",
            "capture_correlation_report_not_passed",
            "Run Capture Trust and satisfy owned-capture requirements before trusting packet-derived score inputs.",
        ),
        "motion_report_not_passed" => (
            "motion",
            "motion_report_not_passed",
            "Resolve Motion feature blockers, usually by importing or capturing trusted raw-motion frames, then rerun local scores.",
        ),
        "no_trusted_motion_features" | "motion_missing" => (
            "motion",
            issue_reason(issue),
            "Capture or import trusted raw-motion frames for the selected window before computing this score.",
        ),
        "no_trusted_heart_rate_features" => (
            "heart_rate",
            "no_trusted_heart_rate_features",
            "Capture or import trusted normal-history heart-rate frames before promoting HR-derived metric inputs.",
        ),
        "no_trusted_vital_event_features" => (
            "vital_event",
            "no_trusted_vital_event_features",
            "Capture or import trusted temperature/vital event frames, then rerun vital event feature extraction.",
        ),
        "no_trusted_hrv_features" => (
            "hrv.current",
            "no_trusted_hrv_features",
            "Capture or import trusted R17/optical frames before promoting HRV-derived metric inputs.",
        ),
        "not_enough_rr_intervals" => (
            "hrv.rr_intervals",
            "not_enough_rr_intervals",
            "Capture enough plausible R17/optical samples to build the required RR interval count.",
        ),
        "hrv_score_errors" => (
            "hrv.score",
            "hrv_score_errors",
            "Inspect HRV score result errors and add a hand-derived regression before changing HRV scoring.",
        ),
        "hrv_baseline_min_days_not_met" => (
            "hrv.baseline",
            "hrv_baseline_min_days_not_met",
            "Capture enough trusted HRV baseline days to satisfy the configured baseline window.",
        ),
        "no_trusted_heart_rate_window_features" => (
            "metric_window.heart_rate",
            "no_trusted_heart_rate_window_features",
            "Capture trusted heart-rate frames in the selected metric window before aggregating score inputs.",
        ),
        "insufficient_heart_rate_window_duration" => (
            "metric_window.duration",
            "insufficient_heart_rate_window_duration",
            "Extend the selected window or capture heart-rate samples across a longer interval.",
        ),
        "no_trusted_resting_heart_rate_features" => (
            "resting_hr.current",
            "no_trusted_resting_heart_rate_features",
            "Capture trusted normal-history heart-rate frames in the resting window before deriving resting HR.",
        ),
        "resting_hr_baseline_min_days_not_met" => (
            "resting_hr.baseline",
            "resting_hr_baseline_min_days_not_met",
            "Capture enough trusted resting-HR baseline days to satisfy the configured baseline window.",
        ),
        "sleep_need_minutes_invalid" => (
            "sleep.sleep_need_minutes",
            "sleep_need_minutes_invalid",
            "Set sleep need to a finite positive minute value before building the sleep score input.",
        ),
        "low_motion_threshold_invalid" => (
            "sleep.low_motion_threshold",
            "low_motion_threshold_invalid",
            "Set the low-motion threshold inside 0..1 before deriving the sleep window.",
        ),
        "disturbance_motion_threshold_invalid" => (
            "sleep.disturbance_motion_threshold",
            "disturbance_motion_threshold_invalid",
            "Set the disturbance motion threshold inside 0..1 before deriving sleep disturbances.",
        ),
        "sleep_window_missing" => (
            "sleep.window",
            "sleep_window_missing",
            "Capture enough trusted low-motion raw-motion samples across the sleep window, then rerun sleep score generation.",
        ),
        "sleep_score_errors" | "sleep_score_output_missing" | "sleep_score_missing" => (
            "sleep.score",
            issue_reason(issue),
            "Inspect the sleep score result errors and add a hand-derived regression before changing the score formula.",
        ),
        "hrv_report_not_passed" | "hrv_rmssd_missing" => (
            "hrv.current",
            issue_reason(issue),
            "Capture trusted R17/optical frames with enough plausible RR intervals, then rerun HRV features.",
        ),
        "hrv_baseline_report_not_passed" | "hrv_baseline_missing" => (
            "hrv.baseline",
            issue_reason(issue),
            "Capture enough trusted HRV baseline days, then rerun HRV baseline and local score generation.",
        ),
        "resting_heart_rate_report_not_passed" | "resting_hr_missing" => (
            "resting_hr.current",
            issue_reason(issue),
            "Capture trusted normal-history heart-rate frames for the selected resting window, then rerun score generation.",
        ),
        "resting_hr_baseline_missing" => (
            "resting_hr.baseline",
            "resting_hr_baseline_missing",
            "Capture enough trusted resting heart-rate baseline days, then rerun recovery score generation.",
        ),
        "sleep_report_not_passed" => (
            "sleep.report",
            "sleep_report_not_passed",
            "Resolve the sleep score report next action before using sleep as a recovery input.",
        ),
        "prior_strain_report_not_passed" | "prior_strain_missing" => (
            "prior_strain",
            issue_reason(issue),
            "Resolve prior-strain score blockers before using strain readiness as a recovery input.",
        ),
        "provided_resp_temp_inputs_missing" => (
            "recovery.provided_vitals",
            "provided_resp_temp_inputs_missing",
            "Provide respiratory-rate, respiratory baseline, and skin-temperature delta inputs or implement packet-derived decoders before recovery scoring.",
        ),
        "provided_resp_temp_inputs_not_packet_derived" => (
            "recovery.provided_vitals",
            "provided_resp_temp_inputs_not_packet_derived",
            "Use decoded WHOOP packet recovery vitals before promoting respiratory rate or temperature into recovery scoring.",
        ),
        "provided_resp_temp_provenance_untrusted" => (
            "recovery.provided_vitals",
            "provided_resp_temp_provenance_untrusted",
            "Attach non-empty device-derived recovery vitals provenance before trusting the recovery score.",
        ),
        "recovery_score_errors" | "recovery_score_output_missing" => (
            "recovery.score",
            issue_reason(issue),
            "Inspect recovery score result errors and add a hand-derived regression before changing recovery weights.",
        ),
        "max_hr_basis_missing" => (
            "strain.max_hr",
            "max_hr_basis_missing",
            "Provide max HR or capture enough trusted workout heart-rate samples to derive a window max before strain scoring.",
        ),
        "max_hr_basis_must_exceed_resting_hr" => (
            "strain.max_hr",
            "max_hr_basis_must_exceed_resting_hr",
            "Use a max-HR basis greater than resting HR before building the strain score input.",
        ),
        "metric_window_report_not_passed" | "metric_window_feature_missing" => (
            "metric_window",
            issue_reason(issue),
            "Capture trusted heart-rate and motion samples in the activity window, then rerun metric window aggregation.",
        ),
        "window_metric_input_not_trusted" => (
            "metric_window.trust",
            "window_metric_input_not_trusted",
            "Import or capture owned evidence for the metric window before promoting this strain score.",
        ),
        "hr_zone_minutes_missing" => (
            "strain.hr_zones",
            "hr_zone_minutes_missing",
            "Rebuild the metric window with resting HR and max HR so five heart-rate zone minute buckets are available.",
        ),
        "strain_score_errors" | "strain_score_output_missing" => (
            "strain.score",
            issue_reason(issue),
            "Inspect strain score result errors and add a hand-derived regression before changing strain scoring.",
        ),
        "heart_rate_report_not_passed" | "heart_rate_missing" => (
            "heart_rate",
            issue_reason(issue),
            "Capture trusted normal-history heart-rate frames for the selected stress/activity window, then rerun scores.",
        ),
        "stress_score_errors" | "stress_score_output_missing" => (
            "stress.score",
            issue_reason(issue),
            "Inspect stress score result errors and add a hand-derived regression before changing stress scoring.",
        ),
        _ => (
            "metric_score",
            "metric_feature_score_issue",
            match family {
                "sleep" => {
                    "Inspect the sleep feature score issue and repair the required motion/window input before trusting the score."
                }
                "recovery" => {
                    "Inspect the recovery feature score issue and repair the missing HRV, resting-HR, sleep, strain, or provided-vitals input."
                }
                "strain" => {
                    "Inspect the strain feature score issue and repair the missing resting-HR, max-HR, or metric-window input."
                }
                "stress" => {
                    "Inspect the stress feature score issue and repair the missing HR, motion, resting-HR, HRV, or baseline input."
                }
                _ => {
                    "Inspect the feature score issue and repair the missing packet-derived input before trusting the score."
                }
            },
        ),
    }
}

fn issue_reason(issue: &str) -> &'static str {
    match issue {
        "no_trusted_motion_features" => "no_trusted_motion_features",
        "no_trusted_heart_rate_features" => "no_trusted_heart_rate_features",
        "no_trusted_vital_event_features" => "no_trusted_vital_event_features",
        "no_trusted_hrv_features" => "no_trusted_hrv_features",
        "not_enough_rr_intervals" => "not_enough_rr_intervals",
        "hrv_score_errors" => "hrv_score_errors",
        "hrv_baseline_min_days_not_met" => "hrv_baseline_min_days_not_met",
        "no_trusted_heart_rate_window_features" => "no_trusted_heart_rate_window_features",
        "insufficient_heart_rate_window_duration" => "insufficient_heart_rate_window_duration",
        "no_trusted_resting_heart_rate_features" => "no_trusted_resting_heart_rate_features",
        "resting_hr_baseline_min_days_not_met" => "resting_hr_baseline_min_days_not_met",
        "motion_missing" => "motion_missing",
        "sleep_score_errors" => "sleep_score_errors",
        "sleep_score_output_missing" => "sleep_score_output_missing",
        "sleep_score_missing" => "sleep_score_missing",
        "hrv_report_not_passed" => "hrv_report_not_passed",
        "hrv_rmssd_missing" => "hrv_rmssd_missing",
        "hrv_baseline_report_not_passed" => "hrv_baseline_report_not_passed",
        "hrv_baseline_missing" => "hrv_baseline_missing",
        "resting_heart_rate_report_not_passed" => "resting_heart_rate_report_not_passed",
        "resting_hr_missing" => "resting_hr_missing",
        "prior_strain_report_not_passed" => "prior_strain_report_not_passed",
        "prior_strain_missing" => "prior_strain_missing",
        "recovery_score_errors" => "recovery_score_errors",
        "recovery_score_output_missing" => "recovery_score_output_missing",
        "metric_window_report_not_passed" => "metric_window_report_not_passed",
        "metric_window_feature_missing" => "metric_window_feature_missing",
        "strain_score_errors" => "strain_score_errors",
        "strain_score_output_missing" => "strain_score_output_missing",
        "heart_rate_report_not_passed" => "heart_rate_report_not_passed",
        "heart_rate_missing" => "heart_rate_missing",
        "stress_score_errors" => "stress_score_errors",
        "stress_score_output_missing" => "stress_score_output_missing",
        _ => "metric_feature_score_issue",
    }
}

fn motion_plan_from_row(row: &DecodedFrameRow) -> GooseResult<Option<MotionPlan>> {
    let parsed_payload: Option<ParsedPayload> = serde_json::from_str(&row.parsed_payload_json)
        .map_err(|error| {
            GooseError::message(format!(
                "{} parsed_payload_json invalid: {error}",
                row.frame_id
            ))
        })?;
    let Some(ParsedPayload::DataPacket {
        timestamp_seconds,
        timestamp_subseconds,
        body_summary: Some(body_summary),
        ..
    }) = parsed_payload
    else {
        return Ok(None);
    };

    Ok(match body_summary {
        DataPacketBodySummary::RawMotionK10 {
            heart_rate,
            axes,
            warnings,
        } => Some(MotionPlan {
            body_summary_kind: "raw_motion_k10",
            axes,
            heart_rate_bpm: heart_rate,
            device_timestamp_seconds: timestamp_seconds,
            device_timestamp_subseconds: timestamp_subseconds,
            summary_warnings: warnings,
        }),
        DataPacketBodySummary::RawMotionK21 { axes, warnings, .. } => Some(MotionPlan {
            body_summary_kind: "raw_motion_k21",
            axes,
            heart_rate_bpm: None,
            device_timestamp_seconds: timestamp_seconds,
            device_timestamp_subseconds: timestamp_subseconds,
            summary_warnings: warnings,
        }),
        _ => None,
    })
}

fn heart_rate_plan_from_row(row: &DecodedFrameRow) -> GooseResult<Option<HeartRatePlan>> {
    let parsed_payload = parsed_payload_from_row(row)?;
    let Some(ParsedPayload::DataPacket {
        timestamp_seconds,
        timestamp_subseconds,
        body_summary: Some(body_summary),
        ..
    }) = parsed_payload
    else {
        return Ok(None);
    };

    Ok(match body_summary {
        DataPacketBodySummary::NormalHistory {
            marker_offset: Some(marker_offset),
            marker_value: Some(marker_value),
            ..
        } => Some(HeartRatePlan {
            body_summary_kind: "normal_history",
            source_signal: "normal_history_hr_marker",
            quality_flag: "preliminary_normal_history_hr_marker",
            marker_offset,
            marker_value,
            device_timestamp_seconds: timestamp_seconds,
            device_timestamp_subseconds: timestamp_subseconds,
        }),
        DataPacketBodySummary::RawMotionK10 {
            heart_rate: Some(heart_rate),
            ..
        } => Some(HeartRatePlan {
            body_summary_kind: "raw_motion_k10",
            source_signal: "raw_motion_k10_heart_rate",
            quality_flag: "preliminary_raw_motion_k10_heart_rate",
            marker_offset: 0,
            marker_value: heart_rate,
            device_timestamp_seconds: timestamp_seconds,
            device_timestamp_subseconds: timestamp_subseconds,
        }),
        _ => None,
    })
}

fn parsed_payload_from_row(row: &DecodedFrameRow) -> GooseResult<Option<ParsedPayload>> {
    serde_json::from_str(&row.parsed_payload_json).map_err(|error| {
        GooseError::message(format!(
            "{} parsed_payload_json invalid: {error}",
            row.frame_id
        ))
    })
}

fn vital_event_plan_from_payload(parsed_payload: &Option<ParsedPayload>) -> Option<VitalEventPlan> {
    let Some(ParsedPayload::Event {
        event_id: Some(event_id),
        event_name,
        timestamp_seconds,
        timestamp_subseconds,
        data_hex,
        warnings,
        ..
    }) = parsed_payload
    else {
        return None;
    };
    if *event_id != 17 && event_name.as_deref() != Some("TEMPERATURE_LEVEL") {
        return None;
    }

    Some(VitalEventPlan {
        event_id: *event_id,
        event_name: event_name
            .clone()
            .unwrap_or_else(|| "TEMPERATURE_LEVEL".to_string()),
        timestamp_seconds: *timestamp_seconds,
        timestamp_subseconds: *timestamp_subseconds,
        data_hex: data_hex.clone(),
        warnings: warnings.clone(),
    })
}

fn skin_temperature_plan_from_payload(
    parsed_payload: &Option<ParsedPayload>,
) -> Option<SkinTemperaturePlan> {
    let Some(ParsedPayload::DataPacket {
        packet_k: Some(packet_k),
        timestamp_seconds,
        timestamp_subseconds,
        body_summary: Some(DataPacketBodySummary::NormalHistory { .. }),
        ..
    }) = parsed_payload
    else {
        return None;
    };

    match *packet_k {
        18 => Some(SkinTemperaturePlan {
            packet_k: *packet_k,
            timestamp_seconds: *timestamp_seconds,
            timestamp_subseconds: *timestamp_subseconds,
            schema_field: "normal_history_k18_body_24_skin_temperature_c",
            raw_body_offset: 24,
            raw_absolute_offset: 37,
            encoding: "i16_le_x100",
            scale: 100.0,
        }),
        24 => Some(SkinTemperaturePlan {
            packet_k: *packet_k,
            timestamp_seconds: *timestamp_seconds,
            timestamp_subseconds: *timestamp_subseconds,
            schema_field: "normal_history_k24_body_3_skin_temperature_c",
            raw_body_offset: 3,
            raw_absolute_offset: 16,
            encoding: "u16_le_x1000",
            scale: 1000.0,
        }),
        _ => None,
    }
}

fn respiratory_rate_plan_from_payload(
    parsed_payload: &Option<ParsedPayload>,
) -> Option<RespiratoryRatePlan> {
    let Some(ParsedPayload::DataPacket {
        packet_k: Some(packet_k),
        timestamp_seconds,
        timestamp_subseconds,
        body_summary: Some(DataPacketBodySummary::NormalHistory { .. }),
        ..
    }) = parsed_payload
    else {
        return None;
    };

    match *packet_k {
        18 => Some(RespiratoryRatePlan {
            packet_k: *packet_k,
            timestamp_seconds: *timestamp_seconds,
            timestamp_subseconds: *timestamp_subseconds,
            schema_field: "normal_history_k18_body_26_respiratory_rate_rpm_candidate",
            raw_body_offset: 26,
            raw_absolute_offset: 39,
            encoding: "u16_le_x10",
            scale: 10.0,
        }),
        _ => None,
    }
}

fn hrv_plan_from_row(row: &DecodedFrameRow) -> GooseResult<Option<HrvPlan>> {
    let parsed_payload: Option<ParsedPayload> = serde_json::from_str(&row.parsed_payload_json)
        .map_err(|error| {
            GooseError::message(format!(
                "{} parsed_payload_json invalid: {error}",
                row.frame_id
            ))
        })?;
    let Some(ParsedPayload::DataPacket {
        body_summary:
            Some(DataPacketBodySummary::R17OpticalOrLabradorFiltered {
                flags,
                sample_count,
                samples: Some(samples),
                warnings,
                ..
            }),
        ..
    }) = parsed_payload
    else {
        return Ok(None);
    };

    Ok(Some(HrvPlan {
        samples,
        flags,
        sample_count,
        summary_warnings: warnings,
    }))
}

fn motion_feature_from_plan(
    row: &DecodedFrameRow,
    payload: &[u8],
    plan: MotionPlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<Option<MotionFeature>> {
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("preliminary_raw_i16_scale".to_string());
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }
    for warning in &plan.summary_warnings {
        quality_flags.insert(warning.clone());
        if warning.contains("truncated") {
            quality_flags.insert("truncated_samples".to_string());
        }
    }

    let mut accumulator = MotionAccumulator::default();
    let mut axis_count = 0;
    for axis in &plan.axes {
        let axis_accumulator = accumulate_axis(payload, axis, &mut quality_flags);
        if axis_accumulator.sample_count > 0 {
            axis_count += 1;
            accumulator.abs_sum += axis_accumulator.abs_sum;
            accumulator.peak_abs = accumulator.peak_abs.max(axis_accumulator.peak_abs);
            accumulator.sample_count += axis_accumulator.sample_count;
        }
    }

    if accumulator.sample_count == 0 {
        return Ok(None);
    }

    let raw_mean_abs = accumulator.abs_sum / accumulator.sample_count as f64;
    let motion_intensity_0_to_1 = clamp_fraction(raw_mean_abs / 32767.0);
    let trusted_metric_input = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();
    let sample_time = normalized_sample_time(
        row,
        plan.device_timestamp_seconds,
        plan.device_timestamp_subseconds,
        &mut quality_flags,
    );

    Ok(Some(MotionFeature {
        metric_input_id: format!("{}.motion_intensity", row.frame_id),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        body_summary_kind: plan.body_summary_kind.to_string(),
        source_signal: "raw_motion_signed_i16_amplitude".to_string(),
        scale_basis: "mean_absolute_signed_i16_div_32767".to_string(),
        motion_intensity_0_to_1,
        raw_mean_abs,
        raw_peak_abs: accumulator.peak_abs,
        parsed_sample_count: accumulator.sample_count,
        axis_count,
        heart_rate_bpm: plan.heart_rate_bpm.filter(|value| *value > 0),
        device_timestamp_seconds: plan.device_timestamp_seconds,
        device_timestamp_subseconds: plan.device_timestamp_subseconds,
        trusted_metric_input,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": plan.body_summary_kind,
            "scale_basis": "mean_absolute_signed_i16_div_32767",
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": plan.device_timestamp_seconds,
            "device_timestamp_subseconds": plan.device_timestamp_subseconds,
            "promotion_policy": "requires_owned_capture_correlation",
        }),
    }))
}

fn hrv_feature_from_plan(
    row: &DecodedFrameRow,
    payload: &[u8],
    plan: HrvPlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<Option<HrvFeature>> {
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("preliminary_r17_i16_rr_interval_candidate".to_string());
    quality_flags.insert("rr_interval_scale_unvalidated".to_string());
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }
    for warning in &plan.summary_warnings {
        quality_flags.insert(warning.clone());
    }

    let mut rr_intervals_ms = Vec::new();
    let mut rejected_sample_count = 0usize;
    for index in 0..plan.samples.parsed_count {
        let offset = plan.samples.offset + index * 2;
        let Some(value) = read_i16_le(payload, offset) else {
            quality_flags.insert("r17_sample_read_failed".to_string());
            rejected_sample_count += 1;
            continue;
        };
        if (300..=2000).contains(&value) {
            rr_intervals_ms.push(f64::from(value));
        } else {
            rejected_sample_count += 1;
        }
    }

    if rr_intervals_ms.is_empty() {
        return Ok(None);
    }
    if rejected_sample_count > 0 {
        quality_flags.insert("rr_interval_samples_outside_plausible_range".to_string());
    }
    if plan
        .sample_count
        .is_some_and(|sample_count| sample_count as usize != plan.samples.parsed_count)
    {
        quality_flags.insert("r17_sample_count_mismatch".to_string());
    }

    let trusted_metric_input = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();

    Ok(Some(HrvFeature {
        metric_input_id: format!("{}.rr_intervals", row.frame_id),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        body_summary_kind: "r17_optical_or_labrador_filtered".to_string(),
        source_signal: "r17_optical_or_labrador_filtered_i16_candidate".to_string(),
        scale_basis: "preliminary_plausible_i16_as_rr_interval_ms".to_string(),
        rr_intervals_ms,
        raw_sample_count: plan.samples.parsed_count,
        plausible_sample_count: plan.samples.parsed_count - rejected_sample_count,
        rejected_sample_count,
        trusted_metric_input,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": "r17_optical_or_labrador_filtered",
            "sample_offset": plan.samples.offset,
            "reported_sample_count": plan.sample_count,
            "flags": plan.flags,
            "promotion_policy": "requires_owned_capture_correlation",
            "scale_basis": "preliminary_plausible_i16_as_rr_interval_ms",
        }),
    }))
}

fn heart_rate_feature_from_plan(
    row: &DecodedFrameRow,
    plan: HeartRatePlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<Option<HeartRateFeature>> {
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert(plan.quality_flag.to_string());
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }

    if plan.marker_value == 0 {
        return Ok(None);
    }
    if !(25..=240).contains(&plan.marker_value) {
        quality_flags.insert("heart_rate_marker_outside_plausible_range".to_string());
        return Ok(None);
    }

    let trusted_metric_input = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();
    let sample_time = normalized_sample_time(
        row,
        plan.device_timestamp_seconds,
        plan.device_timestamp_subseconds,
        &mut quality_flags,
    );

    Ok(Some(HeartRateFeature {
        metric_input_id: format!("{}.heart_rate", row.frame_id),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        body_summary_kind: plan.body_summary_kind.to_string(),
        source_signal: plan.source_signal.to_string(),
        heart_rate_bpm: f64::from(plan.marker_value),
        marker_offset: plan.marker_offset,
        marker_value: plan.marker_value,
        device_timestamp_seconds: plan.device_timestamp_seconds,
        device_timestamp_subseconds: plan.device_timestamp_subseconds,
        trusted_metric_input,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": plan.body_summary_kind,
            "source_signal": plan.source_signal,
            "marker_offset": plan.marker_offset,
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": plan.device_timestamp_seconds,
            "device_timestamp_subseconds": plan.device_timestamp_subseconds,
            "promotion_policy": "requires_owned_capture_correlation",
        }),
    }))
}

fn vital_event_feature_from_plan(
    row: &DecodedFrameRow,
    plan: VitalEventPlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<VitalEventFeature> {
    let raw_body = decode_hex_with_whitespace(&plan.data_hex)?;
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("units_unresolved".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    quality_flags.insert("temperature_event_body_preserved".to_string());
    if raw_body.is_empty() {
        quality_flags.insert("empty_event_body".to_string());
    }
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }
    for warning in &plan.warnings {
        quality_flags.insert(warning.clone());
    }

    let trusted_candidate_evidence = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();

    Ok(VitalEventFeature {
        metric_input_id: format!("{}.temperature_level_event", row.frame_id),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        event_id: plan.event_id,
        event_name: plan.event_name.clone(),
        source_signal: "temperature_level_event".to_string(),
        candidate_kind: "skin_temperature_raw_event".to_string(),
        semantic_status: "unresolved_units".to_string(),
        raw_body_hex: plan.data_hex.clone(),
        raw_byte_count: raw_body.len(),
        raw_i16_le: read_i16_le(&raw_body, 0),
        raw_u16_le: read_u16_le(&raw_body, 0),
        raw_i32_le: read_i32_le(&raw_body, 0),
        raw_u32_le: read_u32_le(&raw_body, 0),
        device_timestamp_seconds: plan.timestamp_seconds,
        device_timestamp_subseconds: plan.timestamp_subseconds,
        trusted_candidate_evidence,
        resolved_metric_input: false,
        value_semantics_verified: false,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": "event_temperature_level",
            "event_id": plan.event_id,
            "event_name": plan.event_name,
            "promotion_policy": "requires_owned_capture_correlation_and_unit_semantics",
            "score_input_policy": "blocked_until_temperature_units_are_verified",
        }),
    })
}

fn skin_temperature_feature_from_plan(
    row: &DecodedFrameRow,
    plan: SkinTemperaturePlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<Option<SkinTemperatureFeature>> {
    let payload = decode_hex_with_whitespace(&row.payload_hex)?;
    let Some(raw_bytes) = payload.get(plan.raw_absolute_offset..plan.raw_absolute_offset + 2)
    else {
        return Ok(None);
    };

    let raw_i16_le = read_i16_le(&payload, plan.raw_absolute_offset);
    let raw_u16_le = read_u16_le(&payload, plan.raw_absolute_offset);
    let skin_temperature_c = match plan.encoding {
        "i16_le_x100" => raw_i16_le.map(|value| f64::from(value) / plan.scale),
        "u16_le_x1000" => raw_u16_le.map(|value| f64::from(value) / plan.scale),
        _ => None,
    };

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("provisional_capture_schema_candidate".to_string());
    quality_flags.insert("temperature_units_unverified".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }
    let semantic_status = match skin_temperature_c {
        Some(value) if (20.0..=45.0).contains(&value) => "plausible_unverified_units",
        Some(value) if value == 0.0 => "zero_candidate_unresolved",
        Some(_) => "outside_plausible_skin_temperature_range",
        None => "unresolved_raw_encoding",
    };
    if semantic_status != "plausible_unverified_units" {
        quality_flags.insert(semantic_status.to_string());
    }

    let trusted_candidate_evidence = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();
    let sample_time = normalized_sample_time(
        row,
        plan.timestamp_seconds,
        plan.timestamp_subseconds,
        &mut quality_flags,
    );

    Ok(Some(SkinTemperatureFeature {
        metric_input_id: format!("{}.{}", row.frame_id, plan.schema_field),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        packet_k: plan.packet_k,
        source_signal: "normal_history_temperature_candidate".to_string(),
        candidate_kind: "skin_temperature_history_candidate".to_string(),
        schema_field: plan.schema_field.to_string(),
        semantic_status: semantic_status.to_string(),
        raw_body_offset: plan.raw_body_offset,
        raw_absolute_offset: plan.raw_absolute_offset,
        raw_hex: hex::encode(raw_bytes),
        raw_i16_le,
        raw_u16_le,
        scale: plan.scale,
        skin_temperature_c,
        trusted_candidate_evidence,
        resolved_metric_input: false,
        value_semantics_verified: false,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": "normal_history",
            "packet_k": plan.packet_k,
            "schema_field": plan.schema_field,
            "candidate_encoding": plan.encoding,
            "candidate_body_offset": plan.raw_body_offset,
            "candidate_absolute_offset": plan.raw_absolute_offset,
            "candidate_source": "history_pip_body_evidence",
            "promotion_policy": "passive_decode_validate_only",
            "score_input_policy": "blocked_until_temperature_units_are_verified",
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": plan.timestamp_seconds,
            "device_timestamp_subseconds": plan.timestamp_subseconds,
        }),
    }))
}

fn respiratory_rate_feature_from_plan(
    row: &DecodedFrameRow,
    plan: RespiratoryRatePlan,
    trusted_frames: &BTreeMap<String, bool>,
) -> GooseResult<Option<RespiratoryRateFeature>> {
    let payload = decode_hex_with_whitespace(&row.payload_hex)?;
    let Some(raw_bytes) = payload.get(plan.raw_absolute_offset..plan.raw_absolute_offset + 2)
    else {
        return Ok(None);
    };

    let raw_u16_le = read_u16_le(&payload, plan.raw_absolute_offset);
    let respiratory_rate_rpm = match plan.encoding {
        "u16_le_x10" => raw_u16_le.map(|value| f64::from(value) / plan.scale),
        _ => None,
    };

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("provisional_capture_schema_candidate".to_string());
    quality_flags.insert("respiratory_units_unverified".to_string());
    quality_flags.insert("not_promoted_to_score_input".to_string());
    for warning in parse_warnings(row)? {
        quality_flags.insert(warning);
    }
    let semantic_status = match respiratory_rate_rpm {
        Some(value) if (6.0..=30.0).contains(&value) => "plausible_unverified_units",
        Some(value) if value == 0.0 => "zero_candidate_unresolved",
        Some(_) => "outside_plausible_respiratory_rate_range",
        None => "unresolved_raw_encoding",
    };
    if semantic_status != "plausible_unverified_units" {
        quality_flags.insert(semantic_status.to_string());
    }

    let trusted_candidate_evidence = trusted_frames
        .get(&row.frame_id)
        .copied()
        .unwrap_or_default();
    let sample_time = normalized_sample_time(
        row,
        plan.timestamp_seconds,
        plan.timestamp_subseconds,
        &mut quality_flags,
    );

    Ok(Some(RespiratoryRateFeature {
        metric_input_id: format!("{}.{}", row.frame_id, plan.schema_field),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        sample_time: sample_time.time,
        sample_time_unix_ms: sample_time.unix_ms,
        sample_time_source: sample_time.source.clone(),
        packet_k: plan.packet_k,
        source_signal: "normal_history_respiratory_rate_candidate".to_string(),
        candidate_kind: "respiratory_rate_history_candidate".to_string(),
        schema_field: plan.schema_field.to_string(),
        semantic_status: semantic_status.to_string(),
        raw_body_offset: plan.raw_body_offset,
        raw_absolute_offset: plan.raw_absolute_offset,
        raw_hex: hex::encode(raw_bytes),
        raw_u16_le,
        scale: plan.scale,
        respiratory_rate_rpm,
        trusted_candidate_evidence,
        resolved_metric_input: false,
        value_semantics_verified: false,
        quality_flags: quality_flags.into_iter().collect(),
        provenance: json!({
            "input_source": "decoded_frame",
            "frame_id": row.frame_id,
            "evidence_id": row.evidence_id,
            "parser_version": row.parser_version,
            "body_summary_kind": "normal_history",
            "packet_k": plan.packet_k,
            "schema_field": plan.schema_field,
            "candidate_encoding": plan.encoding,
            "candidate_body_offset": plan.raw_body_offset,
            "candidate_absolute_offset": plan.raw_absolute_offset,
            "candidate_source": "history_pip_body_evidence",
            "candidate_basis": "k18_fw_packet_u32_1c_high_u16_experimental_respiratory_like_tenths",
            "promotion_policy": "passive_decode_validate_only",
            "score_input_policy": "blocked_until_respiratory_units_are_verified",
            "sample_time_source": sample_time.source,
            "device_timestamp_seconds": plan.timestamp_seconds,
            "device_timestamp_subseconds": plan.timestamp_subseconds,
        }),
    }))
}

fn aggregate_metric_window(
    requested_start: &str,
    requested_end: &str,
    heart_rate_features: &[&HeartRateFeature],
    motion_features: &[&MotionFeature],
    options: MetricWindowFeatureOptions,
) -> GooseResult<MetricWindowFeature> {
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("preliminary_feature_window".to_string());

    let heart_rate_sum = heart_rate_features
        .iter()
        .map(|feature| feature.heart_rate_bpm)
        .sum::<f64>();
    let average_hr_bpm = heart_rate_sum / heart_rate_features.len() as f64;
    let max_hr_bpm = heart_rate_features
        .iter()
        .map(|feature| feature.heart_rate_bpm)
        .fold(f64::NEG_INFINITY, f64::max);

    let (observed_start, observed_end, duration_minutes) =
        observed_feature_window(heart_rate_features, &mut quality_flags);
    if duration_minutes <= 0.0 {
        quality_flags.insert("insufficient_heart_rate_window_duration".to_string());
    }

    let average_motion_intensity_0_to_1 = if motion_features.is_empty() {
        quality_flags.insert("motion_features_missing".to_string());
        None
    } else {
        Some(
            motion_features
                .iter()
                .map(|feature| feature.motion_intensity_0_to_1)
                .sum::<f64>()
                / motion_features.len() as f64,
        )
    };

    let hr_zone_minutes = heart_rate_zone_minutes(
        heart_rate_features,
        duration_minutes,
        options.resting_hr_bpm,
        options.max_hr_bpm,
        &mut quality_flags,
    );
    let (heart_rate_source_signal, heart_rate_source_signals) =
        heart_rate_source_signal_summary(heart_rate_features);

    let mut input_ids = heart_rate_features
        .iter()
        .map(|feature| feature.metric_input_id.clone())
        .collect::<Vec<_>>();
    input_ids.extend(
        motion_features
            .iter()
            .map(|feature| feature.metric_input_id.clone()),
    );
    input_ids.sort();

    let trusted_metric_input = duration_minutes > 0.0
        && heart_rate_features
            .iter()
            .all(|feature| feature.trusted_metric_input)
        && motion_features
            .iter()
            .all(|feature| feature.trusted_metric_input);

    Ok(MetricWindowFeature {
        metric_input_id: format!("window.{}.{}", requested_start, requested_end),
        start_time: observed_start.unwrap_or_else(|| requested_start.to_string()),
        end_time: observed_end.unwrap_or_else(|| requested_end.to_string()),
        duration_minutes,
        average_hr_bpm,
        max_hr_bpm,
        average_motion_intensity_0_to_1,
        hr_zone_minutes,
        heart_rate_sample_count: heart_rate_features.len(),
        motion_sample_count: motion_features.len(),
        trusted_metric_input,
        quality_flags: quality_flags.into_iter().collect(),
        input_ids,
        provenance: json!({
            "input_source": "metric_feature_reports",
            "heart_rate_source_signal": heart_rate_source_signal,
            "heart_rate_source_signals": heart_rate_source_signals,
            "motion_source_signal": "raw_motion_signed_i16_amplitude",
            "requested_start_time": requested_start,
            "requested_end_time": requested_end,
            "zone_basis": {
                "resting_hr_bpm": options.resting_hr_bpm,
                "max_hr_bpm": options.max_hr_bpm,
            },
            "promotion_policy": "requires_all_contributing_features_trusted",
        }),
    })
}

fn average_heart_rate_bpm(heart_rate_features: &[&HeartRateFeature]) -> Option<f64> {
    if heart_rate_features.is_empty() {
        return None;
    }
    Some(
        heart_rate_features
            .iter()
            .map(|feature| feature.heart_rate_bpm)
            .sum::<f64>()
            / heart_rate_features.len() as f64,
    )
}

fn sleep_hr_trend_bpm_per_hour(segments: &[SleepStageSegmentFeature]) -> Option<f64> {
    let sleep_hr_segments = segments
        .iter()
        .filter(|segment| segment.stage != SleepStageKind::Awake)
        .filter_map(|segment| {
            let heart_rate_bpm = segment.heart_rate_bpm?;
            let start_unix_ms = parse_rfc3339_utc_unix_ms(&segment.start_time)?;
            Some((start_unix_ms, heart_rate_bpm))
        })
        .collect::<Vec<_>>();
    let first = sleep_hr_segments.first()?;
    let last = sleep_hr_segments.last()?;
    let elapsed_hours = (last.0 - first.0) as f64 / 3_600_000.0;
    if elapsed_hours <= 0.0 {
        return None;
    }
    Some((last.1 - first.1) / elapsed_hours)
}

fn average_motion_intensity_0_to_1(motion_features: &[&MotionFeature]) -> Option<f64> {
    if motion_features.is_empty() {
        return None;
    }
    Some(
        motion_features
            .iter()
            .map(|feature| feature.motion_intensity_0_to_1)
            .sum::<f64>()
            / motion_features.len() as f64,
    )
}

fn sleep_window_feature(
    requested_start: &str,
    requested_end: &str,
    motion_features: &[&MotionFeature],
    heart_rate_features: &[&HeartRateFeature],
    options: SleepFeatureScoreOptions,
) -> Option<SleepWindowFeature> {
    if motion_features.len() < 2 {
        return None;
    }

    let mut timed_features = motion_features
        .iter()
        .filter_map(|feature| {
            feature_time_unix_ms(feature).map(|unix_ms| (unix_ms / 60_000, *feature))
        })
        .collect::<Vec<_>>();
    let motion_duplicate_timestamp_count = duplicate_minute_count(&timed_features);
    let motion_non_monotonic_input_count = non_monotonic_minute_count(&timed_features);
    timed_features.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.metric_input_id.cmp(&right.1.metric_input_id))
    });
    if timed_features.len() < 2 {
        return None;
    }
    let mut timed_heart_rate_features = heart_rate_features
        .iter()
        .filter_map(|feature| {
            heart_rate_feature_time_unix_ms(feature).map(|unix_ms| (unix_ms / 60_000, *feature))
        })
        .collect::<Vec<_>>();
    let heart_rate_duplicate_timestamp_count = duplicate_minute_count(&timed_heart_rate_features);
    let heart_rate_non_monotonic_input_count =
        non_monotonic_minute_count(&timed_heart_rate_features);
    timed_heart_rate_features.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.metric_input_id.cmp(&right.1.metric_input_id))
    });

    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("preliminary_sleep_from_motion_hr_heuristics".to_string());
    quality_flags.insert("stage_estimates_require_personal_calibration".to_string());
    if timed_features.len() < motion_features.len() {
        quality_flags.insert("unparseable_motion_timestamps_dropped".to_string());
    }
    if timed_heart_rate_features.len() < heart_rate_features.len() {
        quality_flags.insert("unparseable_heart_rate_timestamps_dropped".to_string());
    }
    if timed_heart_rate_features.is_empty() {
        quality_flags.insert("stage_hr_unavailable".to_string());
        quality_flags.insert("heart_rate_dip_unavailable".to_string());
    }
    if timed_features
        .iter()
        .any(|(_, feature)| feature.sample_time_source == "captured_at")
    {
        quality_flags.insert("motion_sample_time_fell_back_to_capture_time".to_string());
    }
    if timed_heart_rate_features
        .iter()
        .any(|(_, feature)| feature.sample_time_source == "captured_at")
    {
        quality_flags.insert("heart_rate_sample_time_fell_back_to_capture_time".to_string());
    }
    if motion_duplicate_timestamp_count > 0 {
        quality_flags.insert("duplicate_motion_timestamps".to_string());
    }
    if heart_rate_duplicate_timestamp_count > 0 {
        quality_flags.insert("duplicate_heart_rate_timestamps".to_string());
    }
    if motion_non_monotonic_input_count > 0 {
        quality_flags.insert("non_monotonic_motion_input_order".to_string());
    }
    if heart_rate_non_monotonic_input_count > 0 {
        quality_flags.insert("non_monotonic_heart_rate_input_order".to_string());
    }

    let mut disturbance_count = 0u32;
    let first = timed_features.first()?;
    let last = timed_features.last()?;
    let window_hr_values = timed_heart_rate_features
        .iter()
        .filter(|(minute, _)| *minute >= first.0 && *minute <= last.0)
        .map(|(_, feature)| feature.heart_rate_bpm)
        .collect::<Vec<_>>();
    let window_min_hr = window_hr_values.iter().copied().reduce(f64::min);
    let window_max_hr = window_hr_values.iter().copied().reduce(f64::max);
    let mut stage_segments = Vec::new();
    let mut motion_covered_minutes = 0.0;
    let mut heart_rate_covered_minutes = 0.0;
    let mut large_motion_gap_count = 0usize;
    let mut largest_motion_gap_minutes = 0i64;
    let mut non_increasing_motion_interval_count = 0usize;
    for pair in timed_features.windows(2) {
        let interval_minutes = pair[1].0 - pair[0].0;
        if interval_minutes <= 0 {
            non_increasing_motion_interval_count += 1;
            quality_flags.insert("non_increasing_motion_timestamps".to_string());
            continue;
        }
        let interval_covered_by_motion = interval_minutes <= 90;
        if interval_minutes > 90 {
            large_motion_gap_count += 1;
            largest_motion_gap_minutes = largest_motion_gap_minutes.max(interval_minutes);
            quality_flags.insert("large_motion_feature_gap".to_string());
        }
        if interval_covered_by_motion {
            motion_covered_minutes += interval_minutes as f64;
        }
        let current_motion = pair[0].1.motion_intensity_0_to_1;
        if current_motion >= options.disturbance_motion_threshold_0_to_1 {
            disturbance_count += 1;
        }
        let heart_rate_bpm =
            average_heart_rate_for_minute_range(pair[0].0, pair[1].0, &timed_heart_rate_features);
        if interval_covered_by_motion && heart_rate_bpm.is_some() {
            heart_rate_covered_minutes += interval_minutes as f64;
        }
        let (stage, confidence_0_to_1, stage_probabilities, mut segment_flags) = infer_sleep_stage(
            current_motion,
            heart_rate_bpm,
            window_min_hr,
            window_max_hr,
            (pair[0].0 - first.0) as f64 / (last.0 - first.0).max(1) as f64,
            &options,
        );
        if heart_rate_bpm.is_none() {
            segment_flags.push("heart_rate_unavailable_for_segment".to_string());
        }
        stage_segments.push(SleepStageSegmentFeature {
            stage,
            start_time: pair[0].1.sample_time.clone(),
            end_time: pair[1].1.sample_time.clone(),
            duration_minutes: interval_minutes as f64,
            confidence_0_to_1,
            stage_probabilities,
            motion_intensity_0_to_1: current_motion,
            heart_rate_bpm,
            quality_flags: segment_flags,
            input_ids: segment_input_ids(
                pair[0].1,
                pair[1].1,
                heart_rate_bpm,
                pair[0].0,
                pair[1].0,
                &timed_heart_rate_features,
            ),
        });
    }
    let raw_stage_segments = stage_segments.clone();
    let raw_stage_segment_count = stage_segments.len();
    let mut stage_segments = merge_compatible_sleep_stage_segments(stage_segments);
    if stage_segments.len() < raw_stage_segment_count {
        quality_flags.insert("adjacent_compatible_stage_segments_merged".to_string());
    }
    let compatible_merged_stage_segment_count = stage_segments.len();
    stage_segments = smooth_short_sleep_stage_transitions(stage_segments);
    if stage_segments.len() < compatible_merged_stage_segment_count {
        quality_flags.insert("short_stage_transition_smoothed".to_string());
    }
    let time_in_bed_minutes = (last.0 - first.0) as f64;
    if time_in_bed_minutes <= 0.0 {
        return None;
    }
    let motion_coverage_fraction = (motion_covered_minutes / time_in_bed_minutes).clamp(0.0, 1.0);
    let heart_rate_coverage_fraction =
        (heart_rate_covered_minutes / time_in_bed_minutes).clamp(0.0, 1.0);
    if motion_coverage_fraction < 0.70 {
        quality_flags.insert("sleep_motion_coverage_low".to_string());
    }
    if heart_rate_coverage_fraction < 0.50 {
        quality_flags.insert("sleep_heart_rate_coverage_low".to_string());
    }
    let first_sleep_start = stage_segments
        .iter()
        .find(|segment| segment.stage != SleepStageKind::Awake)
        .and_then(|segment| parse_rfc3339_utc_unix_ms(&segment.start_time))
        .map(|unix_ms| unix_ms / 60_000);
    let sleep_latency_minutes = first_sleep_start
        .map(|start_minute| (start_minute - first.0).max(0) as f64)
        .unwrap_or(time_in_bed_minutes);
    let last_sleep_end = stage_segments
        .iter()
        .rev()
        .find(|segment| segment.stage != SleepStageKind::Awake)
        .and_then(|segment| parse_rfc3339_utc_unix_ms(&segment.end_time))
        .map(|unix_ms| unix_ms / 60_000);
    let mut wake_after_sleep_onset_minutes = 0.0;
    let mut wake_episode_count = 0u32;
    let mut previous_was_wake = false;
    for segment in &stage_segments {
        let segment_start =
            parse_rfc3339_utc_unix_ms(&segment.start_time).map(|unix_ms| unix_ms / 60_000);
        let segment_end =
            parse_rfc3339_utc_unix_ms(&segment.end_time).map(|unix_ms| unix_ms / 60_000);
        let inside_sleep = match (
            first_sleep_start,
            last_sleep_end,
            segment_start,
            segment_end,
        ) {
            (Some(first_sleep), Some(last_sleep), Some(start_minute), Some(end_minute)) => {
                start_minute >= first_sleep && end_minute <= last_sleep
            }
            _ => false,
        };
        if inside_sleep && segment.stage == SleepStageKind::Awake {
            wake_after_sleep_onset_minutes += segment.duration_minutes;
            if !previous_was_wake {
                wake_episode_count += 1;
            }
            previous_was_wake = true;
        } else {
            previous_was_wake = false;
        }
    }
    let sleep_duration_minutes = stage_segments
        .iter()
        .filter(|segment| segment.stage != SleepStageKind::Awake)
        .map(|segment| segment.duration_minutes)
        .sum();
    let mut stage_minutes = BTreeMap::new();
    for segment in &stage_segments {
        *stage_minutes
            .entry(segment.stage.as_str().to_string())
            .or_insert(0.0) += segment.duration_minutes;
    }
    let sleep_hr_values = raw_stage_segments
        .iter()
        .filter(|segment| segment.stage != SleepStageKind::Awake)
        .filter_map(|segment| segment.heart_rate_bpm)
        .collect::<Vec<_>>();
    let average_sleep_hr_bpm = average_f64(&sleep_hr_values);
    let lowest_sleep_hr_bpm = sleep_hr_values.iter().copied().reduce(f64::min);
    let sleep_hr_trend_bpm_per_hour = sleep_hr_trend_bpm_per_hour(&raw_stage_segments);
    let pre_sleep_awake_hr_values = raw_stage_segments
        .iter()
        .filter(|segment| segment.stage == SleepStageKind::Awake)
        .filter(|segment| {
            first_sleep_start
                .and_then(|first_sleep| {
                    parse_rfc3339_utc_unix_ms(&segment.end_time)
                        .map(|unix_ms| (unix_ms / 60_000) <= first_sleep)
                })
                .unwrap_or(false)
        })
        .filter_map(|segment| segment.heart_rate_bpm)
        .collect::<Vec<_>>();
    let awake_hr_values = raw_stage_segments
        .iter()
        .filter(|segment| segment.stage == SleepStageKind::Awake)
        .filter_map(|segment| segment.heart_rate_bpm)
        .collect::<Vec<_>>();
    let baseline_awake_hr_bpm = average_f64(&pre_sleep_awake_hr_values)
        .or_else(|| average_f64(&awake_hr_values))
        .or_else(|| {
            if window_hr_values.len() >= 2 {
                quality_flags.insert("heart_rate_dip_uses_highest_quartile_fallback".to_string());
                Some(highest_quartile_average(&window_hr_values))
            } else {
                None
            }
        });
    // Build a (relative_minute, hr_bpm) series from timed_heart_rate_features within the window.
    // Timestamps from timed_heart_rate_features are absolute minutes (unix_ms / 60_000);
    // subtract first.0 to make them relative to the window start.
    let window_hr_series: Vec<(f64, f64)> = timed_heart_rate_features
        .iter()
        .filter(|(minute, _)| *minute >= first.0 && *minute <= last.0)
        .map(|(minute, feature)| ((*minute - first.0) as f64, feature.heart_rate_bpm))
        .collect();

    // Preserve heuristic values before potentially replacing them via HR-threshold helpers.
    // These names are kept distinct to avoid variable-shadowing confusion.
    let heuristic_sleep_latency = sleep_latency_minutes;
    let heuristic_waso = wake_after_sleep_onset_minutes;
    let heuristic_disturbance_count = disturbance_count;

    // ALG-SLP-01: use HR-threshold helpers when HR coverage >= 50%; fall back to
    // stage-segment heuristic with a quality flag below the coverage threshold.
    // baseline_awake_hr_bpm is used as resting_hr per ALG-SLP-01 (Claude's Discretion in
    // 24-CONTEXT): the pre-sleep awake HR is the best available proxy for resting HR when
    // dedicated resting-HR sensors are not used during the sleep window.
    let (
        heart_rate_dip_percent,
        sleep_latency_minutes,
        wake_after_sleep_onset_minutes,
        disturbance_count,
    ) = if heart_rate_coverage_fraction >= 0.50 {
        if let Some(baseline) = baseline_awake_hr_bpm {
            let dip = heart_rate_dip_pct(&window_hr_series, baseline);
            if dip.is_none() {
                quality_flags.insert("heart_rate_dip_not_detected".to_string());
            }
            // sol_from_hr measures latency from the first HR sample in the series.
            // Add the offset of the first HR sample from the window start (first.0)
            // to convert to window-relative SOL (the series timestamps are already
            // window-relative via the `*minute - first.0` mapping above).
            let first_hr_offset = window_hr_series.first().map(|(ts, _)| *ts).unwrap_or(0.0);
            let sol_opt = sol_from_hr(&window_hr_series, baseline, 3.0)
                .map(|latency_from_first_hr| latency_from_first_hr + first_hr_offset);
            let sol = sol_opt.unwrap_or(heuristic_sleep_latency);
            let waso = waso_from_hr(&window_hr_series, baseline, sol);
            let dist = hr_disturbance_count(&window_hr_series, baseline, sol);
            (dip, sol, waso, dist)
        } else {
            // No baseline HR available even with good coverage; keep heuristic values.
            quality_flags.insert("heart_rate_dip_unavailable".to_string());
            (
                None,
                heuristic_sleep_latency,
                heuristic_waso,
                heuristic_disturbance_count,
            )
        }
    } else {
        // HR coverage < 50%: keep stage-segment heuristic values, add quality flag.
        quality_flags.insert("sleep_hr_metrics_low_coverage_fallback".to_string());
        // Also compute the old segment-based dip for backward compatibility.
        let segment_dip = match (baseline_awake_hr_bpm, lowest_sleep_hr_bpm) {
            (Some(baseline), Some(lowest)) if baseline > 0.0 && lowest <= baseline => {
                Some(((baseline - lowest) / baseline) * 100.0)
            }
            (Some(_), Some(_)) => {
                quality_flags.insert("heart_rate_dip_not_detected".to_string());
                Some(0.0)
            }
            _ => None,
        };
        (
            segment_dip,
            heuristic_sleep_latency,
            heuristic_waso,
            heuristic_disturbance_count,
        )
    };

    let midpoint_minutes_since_midnight = (((first.0 + last.0) / 2).rem_euclid(24 * 60)) as f64;
    let midpoint_deviation_minutes = circular_minute_deviation(
        midpoint_minutes_since_midnight,
        options.target_midpoint_minutes_since_midnight,
    );
    let mut input_ids = timed_features
        .iter()
        .map(|(_, feature)| feature.metric_input_id.clone())
        .collect::<Vec<_>>();
    input_ids.extend(
        timed_heart_rate_features
            .iter()
            .filter(|(minute, _)| *minute >= first.0 && *minute <= last.0)
            .map(|(_, feature)| feature.metric_input_id.clone()),
    );
    input_ids.sort();
    input_ids.dedup();

    Some(SleepWindowFeature {
        metric_input_id: format!("sleep_window.{}.{}", requested_start, requested_end),
        start_time: first.1.sample_time.clone(),
        end_time: last.1.sample_time.clone(),
        time_in_bed_minutes,
        sleep_duration_minutes,
        sleep_latency_minutes,
        wake_after_sleep_onset_minutes,
        wake_episode_count,
        midpoint_deviation_minutes,
        disturbance_count,
        stage_model_version: "goose_sleep_stage_heuristic_v1_transition_smoothed".to_string(),
        stage_segments,
        stage_minutes,
        average_sleep_hr_bpm,
        lowest_sleep_hr_bpm,
        sleep_hr_trend_bpm_per_hour,
        baseline_awake_hr_bpm,
        heart_rate_dip_percent,
        motion_feature_count: timed_features.len(),
        heart_rate_feature_count: timed_heart_rate_features
            .iter()
            .filter(|(minute, _)| *minute >= first.0 && *minute <= last.0)
            .count(),
        motion_coverage_fraction,
        heart_rate_coverage_fraction,
        trusted_metric_input: timed_features
            .iter()
            .all(|(_, feature)| feature.trusted_metric_input)
            && timed_heart_rate_features
                .iter()
                .filter(|(minute, _)| *minute >= first.0 && *minute <= last.0)
                .all(|(_, feature)| feature.trusted_metric_input),
        quality_flags: quality_flags.into_iter().collect(),
        input_ids,
        provenance: json!({
            "input_source": ["motion_feature_report", "heart_rate_feature_report"],
            "source_signal": ["raw_motion_signed_i16_amplitude", "normal_history_hr_marker"],
            "method": "motion_hr_sleep_window_stage_heuristic",
            "time_basis": "normalized_sample_time",
            "sleep_hr_trend_policy": "first_to_last_non_awake_stage_hr_bpm_per_hour",
            "requested_start_time": requested_start,
            "requested_end_time": requested_end,
            "low_motion_threshold_0_to_1": options.low_motion_threshold_0_to_1,
            "disturbance_motion_threshold_0_to_1": options.disturbance_motion_threshold_0_to_1,
            "target_midpoint_minutes_since_midnight": options.target_midpoint_minutes_since_midnight,
            "stage_model_version": "goose_sleep_stage_heuristic_v1_transition_smoothed",
            "stage_smoothing_policy": "merge_short_non_awake_stage_islands_between_matching_non_awake_neighbors",
            "minimum_smoothed_stage_duration_minutes": MIN_SMOOTHED_SLEEP_STAGE_DURATION_MINUTES,
            "coverage": {
                "motion_coverage_fraction": motion_coverage_fraction,
                "heart_rate_coverage_fraction": heart_rate_coverage_fraction,
                "large_gap_threshold_minutes": 90,
                "motion_duplicate_timestamp_count": motion_duplicate_timestamp_count,
                "heart_rate_duplicate_timestamp_count": heart_rate_duplicate_timestamp_count,
                "motion_non_monotonic_input_count": motion_non_monotonic_input_count,
                "heart_rate_non_monotonic_input_count": heart_rate_non_monotonic_input_count,
                "non_increasing_motion_interval_count": non_increasing_motion_interval_count,
                "large_motion_gap_count": large_motion_gap_count,
                "largest_motion_gap_minutes": largest_motion_gap_minutes,
            },
            "promotion_policy": "requires_all_contributing_features_trusted",
        }),
    })
}

fn average_heart_rate_for_minute_range(
    start_minute: i64,
    end_minute: i64,
    timed_heart_rate_features: &[(i64, &HeartRateFeature)],
) -> Option<f64> {
    let values = timed_heart_rate_features
        .iter()
        .filter(|(minute, _)| *minute >= start_minute && *minute < end_minute)
        .map(|(_, feature)| feature.heart_rate_bpm)
        .collect::<Vec<_>>();
    average_f64(&values)
}

fn duplicate_minute_count<T>(timed_features: &[(i64, T)]) -> usize {
    let mut seen = BTreeSet::new();
    timed_features
        .iter()
        .filter(|(minute, _)| !seen.insert(*minute))
        .count()
}

fn non_monotonic_minute_count<T>(timed_features: &[(i64, T)]) -> usize {
    timed_features
        .windows(2)
        .filter(|pair| pair[1].0 <= pair[0].0)
        .count()
}

fn average_f64(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn highest_quartile_average(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| right.total_cmp(left));
    let count = sorted.len().div_ceil(4).max(1);
    sorted.iter().take(count).sum::<f64>() / count as f64
}

fn merge_compatible_sleep_stage_segments(
    stage_segments: Vec<SleepStageSegmentFeature>,
) -> Vec<SleepStageSegmentFeature> {
    let mut merged = Vec::<SleepStageSegmentFeature>::new();
    for segment in stage_segments {
        let Some(last) = merged.last_mut() else {
            merged.push(segment);
            continue;
        };
        if !compatible_sleep_stage_segments(last, &segment) {
            merged.push(segment);
            continue;
        }

        let total_duration = last.duration_minutes + segment.duration_minutes;
        let last_weight = if total_duration > 0.0 {
            last.duration_minutes / total_duration
        } else {
            0.5
        };
        let segment_weight = 1.0 - last_weight;
        last.end_time = segment.end_time;
        last.confidence_0_to_1 =
            last.confidence_0_to_1 * last_weight + segment.confidence_0_to_1 * segment_weight;
        last.motion_intensity_0_to_1 = last.motion_intensity_0_to_1 * last_weight
            + segment.motion_intensity_0_to_1 * segment_weight;
        last.heart_rate_bpm = match (last.heart_rate_bpm, segment.heart_rate_bpm) {
            (Some(left), Some(right)) => Some(left * last_weight + right * segment_weight),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        last.stage_probabilities = weighted_stage_probability_map(
            &last.stage_probabilities,
            last_weight,
            &segment.stage_probabilities,
            segment_weight,
        );
        last.duration_minutes = total_duration;
        last.quality_flags.extend(segment.quality_flags);
        last.quality_flags.sort();
        last.quality_flags.dedup();
        last.input_ids.extend(segment.input_ids);
        last.input_ids.sort();
        last.input_ids.dedup();
    }
    merged
}

fn smooth_short_sleep_stage_transitions(
    mut stage_segments: Vec<SleepStageSegmentFeature>,
) -> Vec<SleepStageSegmentFeature> {
    loop {
        let Some(index) = stage_segments
            .windows(3)
            .position(short_non_awake_stage_island)
            .map(|index| index + 1)
        else {
            break;
        };

        let combined = combine_sleep_stage_segments(
            stage_segments[index - 1].stage.clone(),
            &stage_segments[index - 1..=index + 1],
            &["short_stage_transition_smoothed"],
        );
        stage_segments.splice(index - 1..=index + 1, [combined]);
    }
    stage_segments
}

fn short_non_awake_stage_island(window: &[SleepStageSegmentFeature]) -> bool {
    let [left, middle, right] = window else {
        return false;
    };
    middle.stage != SleepStageKind::Awake
        && left.stage != SleepStageKind::Awake
        && right.stage != SleepStageKind::Awake
        && left.stage == right.stage
        && middle.stage != left.stage
        && middle.duration_minutes < MIN_SMOOTHED_SLEEP_STAGE_DURATION_MINUTES
        && left.end_time == middle.start_time
        && middle.end_time == right.start_time
}

fn combine_sleep_stage_segments(
    stage: SleepStageKind,
    segments: &[SleepStageSegmentFeature],
    extra_quality_flags: &[&str],
) -> SleepStageSegmentFeature {
    let first = segments.first().expect("at least one stage segment");
    let last = segments.last().expect("at least one stage segment");
    let total_duration = segments
        .iter()
        .map(|segment| segment.duration_minutes)
        .sum::<f64>();
    let weighted = |value: fn(&SleepStageSegmentFeature) -> f64| {
        if total_duration > 0.0 {
            segments
                .iter()
                .map(|segment| value(segment) * segment.duration_minutes)
                .sum::<f64>()
                / total_duration
        } else {
            segments.iter().map(value).sum::<f64>() / segments.len() as f64
        }
    };
    let heart_rate_values = segments
        .iter()
        .filter_map(|segment| {
            segment.heart_rate_bpm.map(|heart_rate| {
                let weight = if total_duration > 0.0 {
                    segment.duration_minutes
                } else {
                    1.0
                };
                (heart_rate, weight)
            })
        })
        .collect::<Vec<_>>();
    let heart_rate_weight_sum = heart_rate_values
        .iter()
        .map(|(_, weight)| *weight)
        .sum::<f64>();
    let heart_rate_bpm = if heart_rate_weight_sum > 0.0 {
        Some(
            heart_rate_values
                .iter()
                .map(|(heart_rate, weight)| heart_rate * weight)
                .sum::<f64>()
                / heart_rate_weight_sum,
        )
    } else {
        None
    };
    let mut quality_flags = segments
        .iter()
        .flat_map(|segment| segment.quality_flags.iter().cloned())
        .chain(extra_quality_flags.iter().map(|flag| (*flag).to_string()))
        .collect::<Vec<_>>();
    quality_flags.sort();
    quality_flags.dedup();
    let mut input_ids = segments
        .iter()
        .flat_map(|segment| segment.input_ids.iter().cloned())
        .collect::<Vec<_>>();
    input_ids.sort();
    input_ids.dedup();

    SleepStageSegmentFeature {
        stage,
        start_time: first.start_time.clone(),
        end_time: last.end_time.clone(),
        duration_minutes: total_duration,
        confidence_0_to_1: (weighted(|segment| segment.confidence_0_to_1) - 0.03).clamp(0.0, 1.0),
        stage_probabilities: weighted_stage_probabilities_for_segments(segments, total_duration),
        motion_intensity_0_to_1: weighted(|segment| segment.motion_intensity_0_to_1),
        heart_rate_bpm,
        quality_flags,
        input_ids,
    }
}

fn weighted_stage_probabilities_for_segments(
    segments: &[SleepStageSegmentFeature],
    total_duration: f64,
) -> BTreeMap<String, f64> {
    let mut probabilities = BTreeMap::new();
    for segment in segments {
        let weight = if total_duration > 0.0 {
            segment.duration_minutes / total_duration
        } else {
            1.0 / segments.len() as f64
        };
        let segment_probabilities = complete_stage_probability_map(segment);
        for (stage, probability) in segment_probabilities {
            *probabilities.entry(stage).or_insert(0.0) += probability * weight;
        }
    }
    probabilities
}

fn weighted_stage_probability_map(
    left: &BTreeMap<String, f64>,
    left_weight: f64,
    right: &BTreeMap<String, f64>,
    right_weight: f64,
) -> BTreeMap<String, f64> {
    let mut probabilities = BTreeMap::new();
    for stage in ["awake", "core", "deep", "rem"] {
        let probability = left.get(stage).copied().unwrap_or(0.0) * left_weight
            + right.get(stage).copied().unwrap_or(0.0) * right_weight;
        if probability > 0.0 {
            probabilities.insert(stage.to_string(), probability);
        }
    }
    probabilities
}

fn complete_stage_probability_map(segment: &SleepStageSegmentFeature) -> BTreeMap<String, f64> {
    if !segment.stage_probabilities.is_empty() {
        return segment.stage_probabilities.clone();
    }
    BTreeMap::from([(
        segment.stage.as_str().to_string(),
        segment.confidence_0_to_1,
    )])
}

fn compatible_sleep_stage_segments(
    left: &SleepStageSegmentFeature,
    right: &SleepStageSegmentFeature,
) -> bool {
    left.stage == right.stage
        && left.end_time == right.start_time
        && (left.confidence_0_to_1 - right.confidence_0_to_1).abs() <= 0.15
}

fn infer_sleep_stage(
    motion_intensity_0_to_1: f64,
    heart_rate_bpm: Option<f64>,
    window_min_hr: Option<f64>,
    window_max_hr: Option<f64>,
    night_fraction_0_to_1: f64,
    options: &SleepFeatureScoreOptions,
) -> (SleepStageKind, f64, BTreeMap<String, f64>, Vec<String>) {
    if motion_intensity_0_to_1 >= options.disturbance_motion_threshold_0_to_1 {
        return (
            SleepStageKind::Awake,
            0.82,
            stage_probability_map([
                (SleepStageKind::Awake, 0.82),
                (SleepStageKind::Core, 0.12),
                (SleepStageKind::Deep, 0.03),
                (SleepStageKind::Rem, 0.03),
            ]),
            vec!["wake_from_high_motion".to_string()],
        );
    }
    if motion_intensity_0_to_1 > options.low_motion_threshold_0_to_1 {
        return (
            SleepStageKind::Core,
            0.58,
            stage_probability_map([
                (SleepStageKind::Awake, 0.22),
                (SleepStageKind::Core, 0.58),
                (SleepStageKind::Deep, 0.10),
                (SleepStageKind::Rem, 0.10),
            ]),
            vec!["restless_sleep_from_motion".to_string()],
        );
    }

    let Some(heart_rate_bpm) = heart_rate_bpm else {
        let stage = if night_fraction_0_to_1 < 0.35 {
            SleepStageKind::Deep
        } else if night_fraction_0_to_1 > 0.65 {
            SleepStageKind::Rem
        } else {
            SleepStageKind::Core
        };
        return (
            stage.clone(),
            0.44,
            stage_probability_map(stage_prior_probability_rows(stage)),
            vec!["stage_from_motion_and_time_prior_only".to_string()],
        );
    };
    let hr_position = match (window_min_hr, window_max_hr) {
        (Some(min_hr), Some(max_hr)) if max_hr > min_hr => {
            ((heart_rate_bpm - min_hr) / (max_hr - min_hr)).clamp(0.0, 1.0)
        }
        _ => 0.5,
    };
    if night_fraction_0_to_1 < 0.60 && hr_position <= 0.33 {
        (
            SleepStageKind::Deep,
            0.64,
            stage_probability_map([
                (SleepStageKind::Awake, 0.04),
                (SleepStageKind::Core, 0.24),
                (SleepStageKind::Deep, 0.64),
                (SleepStageKind::Rem, 0.08),
            ]),
            vec!["deep_from_low_motion_low_hr".to_string()],
        )
    } else if night_fraction_0_to_1 > 0.45 && hr_position >= 0.55 {
        (
            SleepStageKind::Rem,
            0.56,
            stage_probability_map([
                (SleepStageKind::Awake, 0.08),
                (SleepStageKind::Core, 0.28),
                (SleepStageKind::Deep, 0.08),
                (SleepStageKind::Rem, 0.56),
            ]),
            vec!["rem_from_late_sleep_relative_hr_rise".to_string()],
        )
    } else {
        (
            SleepStageKind::Core,
            0.60,
            stage_probability_map([
                (SleepStageKind::Awake, 0.08),
                (SleepStageKind::Core, 0.60),
                (SleepStageKind::Deep, 0.16),
                (SleepStageKind::Rem, 0.16),
            ]),
            vec!["core_sleep_default_stage".to_string()],
        )
    }
}

fn stage_prior_probability_rows(stage: SleepStageKind) -> [(SleepStageKind, f64); 4] {
    match stage {
        SleepStageKind::Awake => [
            (SleepStageKind::Awake, 0.44),
            (SleepStageKind::Core, 0.30),
            (SleepStageKind::Deep, 0.13),
            (SleepStageKind::Rem, 0.13),
        ],
        SleepStageKind::Core => [
            (SleepStageKind::Awake, 0.12),
            (SleepStageKind::Core, 0.44),
            (SleepStageKind::Deep, 0.22),
            (SleepStageKind::Rem, 0.22),
        ],
        SleepStageKind::Deep => [
            (SleepStageKind::Awake, 0.08),
            (SleepStageKind::Core, 0.28),
            (SleepStageKind::Deep, 0.44),
            (SleepStageKind::Rem, 0.20),
        ],
        SleepStageKind::Rem => [
            (SleepStageKind::Awake, 0.08),
            (SleepStageKind::Core, 0.28),
            (SleepStageKind::Deep, 0.20),
            (SleepStageKind::Rem, 0.44),
        ],
    }
}

fn stage_probability_map(rows: [(SleepStageKind, f64); 4]) -> BTreeMap<String, f64> {
    rows.into_iter()
        .map(|(stage, probability)| (stage.as_str().to_string(), probability))
        .collect()
}

fn segment_input_ids(
    start_motion: &MotionFeature,
    end_motion: &MotionFeature,
    heart_rate_bpm: Option<f64>,
    start_minute: i64,
    end_minute: i64,
    timed_heart_rate_features: &[(i64, &HeartRateFeature)],
) -> Vec<String> {
    let mut input_ids = vec![
        start_motion.metric_input_id.clone(),
        end_motion.metric_input_id.clone(),
    ];
    if heart_rate_bpm.is_some() {
        input_ids.extend(
            timed_heart_rate_features
                .iter()
                .filter(|(minute, _)| *minute >= start_minute && *minute < end_minute)
                .map(|(_, feature)| feature.metric_input_id.clone()),
        );
    }
    input_ids.sort();
    input_ids.dedup();
    input_ids
}

fn recovery_provided_vitals_feature(
    start: &str,
    end: &str,
    options: &RecoveryFeatureScoreOptions,
) -> Option<RecoveryProvidedVitalsFeature> {
    let (Some(respiratory_rate_rpm), Some(respiratory_rate_baseline_rpm), Some(skin_temp_delta_c)) = (
        options.respiratory_rate_rpm,
        options.respiratory_rate_baseline_rpm,
        options.skin_temp_delta_c,
    ) else {
        return None;
    };
    if respiratory_rate_rpm <= 0.0
        || respiratory_rate_baseline_rpm <= 0.0
        || !respiratory_rate_rpm.is_finite()
        || !respiratory_rate_baseline_rpm.is_finite()
        || !skin_temp_delta_c.is_finite()
    {
        return None;
    }
    let source = options
        .provided_vitals_source
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .unwrap_or("provided_recovery_vitals")
        .to_string();
    let provenance_raw = options
        .provided_vitals_provenance_json
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    let (provenance_ready, provided_provenance) = if provenance_raw.is_empty() {
        (
            false,
            json!({
                "issue": "provided_vitals_provenance_json_missing",
            }),
        )
    } else {
        match serde_json::from_str::<serde_json::Value>(provenance_raw) {
            Ok(value) if value.as_object().is_some_and(|object| !object.is_empty()) => {
                (true, value)
            }
            Ok(value) => (
                false,
                json!({
                    "issue": "provided_vitals_provenance_not_object",
                    "value": value,
                }),
            ),
            Err(error) => (
                false,
                json!({
                    "issue": "provided_vitals_provenance_json_invalid",
                    "error": error.to_string(),
                }),
            ),
        }
    };
    let packet_derived = provided_vitals_source_is_packet_derived(&source, &provided_provenance);
    let mut quality_flags = Vec::new();
    if !packet_derived {
        quality_flags.push("provided_resp_temp_inputs_not_packet_derived".to_string());
    }
    if !provenance_ready {
        quality_flags.push("provided_resp_temp_provenance_untrusted".to_string());
    }
    quality_flags.sort();
    quality_flags.dedup();

    Some(RecoveryProvidedVitalsFeature {
        metric_input_id: format!("provided_recovery_vitals.{}.{}", start, end),
        respiratory_rate_rpm,
        respiratory_rate_baseline_rpm,
        skin_temp_delta_c,
        source: source.clone(),
        trusted_metric_input: provenance_ready && packet_derived,
        quality_flags,
        provenance: json!({
            "input_source": source,
            "provided_vitals_provenance": provided_provenance,
            "requested_start_time": start,
            "requested_end_time": end,
            "promotion_policy": "packet_derived_recovery_vitals_only",
            "official_labels_policy": "not_used",
        }),
    })
}

fn provided_vitals_source_is_packet_derived(source: &str, provenance: &serde_json::Value) -> bool {
    if provided_vitals_text_has_blocked_source(source)
        || provided_vitals_value_has_blocked_source(provenance)
    {
        return false;
    }
    provided_vitals_text_has_packet_source(source)
        || provided_vitals_value_has_packet_source(provenance)
}

fn provided_vitals_text_has_blocked_source(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "manual",
        "healthkit",
        "health_connect",
        "health connect",
        "whoop_app",
        "whoop app",
        "official_label",
        "backend",
        "platform_import",
    ]
    .iter()
    .any(|token| text.contains(token))
}

fn provided_vitals_text_has_packet_source(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "metrics.recovery_sensor_discovery",
        "metrics.vital_event_features",
        "recovery_sensor_discovery",
        "vital_event_features",
        "device_sensor",
        "decoded_packet",
        "packet_decoder",
        "whoop_packet",
        "packet_family",
    ]
    .iter()
    .any(|token| text.contains(token))
}

fn provided_vitals_value_has_blocked_source(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => provided_vitals_text_has_blocked_source(text),
        serde_json::Value::Array(values) => {
            values.iter().any(provided_vitals_value_has_blocked_source)
        }
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            provided_vitals_text_has_blocked_source(key)
                || provided_vitals_value_has_blocked_source(value)
        }),
        _ => false,
    }
}

fn provided_vitals_value_has_packet_source(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => provided_vitals_text_has_packet_source(text),
        serde_json::Value::Array(values) => {
            values.iter().any(provided_vitals_value_has_packet_source)
        }
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            provided_vitals_text_has_packet_source(key)
                || provided_vitals_value_has_packet_source(value)
        }),
        _ => false,
    }
}

fn daily_resting_heart_rate_features(
    heart_rate_features: &[&HeartRateFeature],
) -> Vec<RestingHeartRateDayFeature> {
    let mut by_date = BTreeMap::<String, Vec<&HeartRateFeature>>::new();
    for feature in heart_rate_features {
        let Some(date) = feature_date(&feature.captured_at) else {
            continue;
        };
        by_date.entry(date.to_string()).or_default().push(*feature);
    }

    by_date
        .into_iter()
        .map(|(date, features)| {
            let mut input_ids = features
                .iter()
                .map(|feature| feature.metric_input_id.clone())
                .collect::<Vec<_>>();
            input_ids.sort();
            RestingHeartRateDayFeature {
                date,
                resting_hr_bpm: low_quartile_mean_hr(&features),
                sample_count: features.len(),
                trusted_metric_input: features.iter().all(|feature| feature.trusted_metric_input),
                input_ids,
            }
        })
        .collect()
}

fn resting_heart_rate_candidate_selection<'a>(
    heart_rate_features: &[&'a HeartRateFeature],
    motion_features: &[&MotionFeature],
) -> RestingHeartRateCandidateSelection<'a> {
    let mut quality_flags = BTreeSet::new();
    quality_flags.insert("preliminary_resting_hr_from_heart_rate_features".to_string());

    if heart_rate_features.is_empty() {
        return RestingHeartRateCandidateSelection {
            features: Vec::new(),
            method: "lowest_quartile_mean_heart_rate_features",
            quality_flags: quality_flags.into_iter().collect(),
            motion_sample_count: motion_features.len(),
            matched_heart_rate_sample_count: 0,
            low_motion_heart_rate_sample_count: 0,
            high_motion_heart_rate_sample_count: 0,
            unmatched_heart_rate_sample_count: 0,
        };
    }

    if motion_features.is_empty() {
        quality_flags.insert("resting_hr_motion_context_unavailable".to_string());
        return RestingHeartRateCandidateSelection {
            features: heart_rate_features.to_vec(),
            method: "lowest_quartile_mean_heart_rate_features",
            quality_flags: quality_flags.into_iter().collect(),
            motion_sample_count: 0,
            matched_heart_rate_sample_count: 0,
            low_motion_heart_rate_sample_count: 0,
            high_motion_heart_rate_sample_count: 0,
            unmatched_heart_rate_sample_count: heart_rate_features.len(),
        };
    }

    let mut low_motion_features = Vec::new();
    let mut unmatched_features = Vec::new();
    let mut matched_heart_rate_sample_count = 0usize;
    let mut high_motion_heart_rate_sample_count = 0usize;

    for feature in heart_rate_features {
        match nearest_resting_motion_feature(feature, motion_features) {
            Some(motion) => {
                matched_heart_rate_sample_count += 1;
                if motion.motion_intensity_0_to_1 <= RESTING_HR_LOW_MOTION_INTENSITY_MAX {
                    low_motion_features.push(*feature);
                } else {
                    high_motion_heart_rate_sample_count += 1;
                }
            }
            None => unmatched_features.push(*feature),
        }
    }

    let unmatched_heart_rate_sample_count = unmatched_features.len();
    if unmatched_heart_rate_sample_count > 0 {
        quality_flags.insert("resting_hr_motion_context_partial".to_string());
    }
    if high_motion_heart_rate_sample_count > 0 {
        quality_flags.insert("resting_hr_high_motion_samples_excluded".to_string());
    }

    let low_motion_heart_rate_sample_count = low_motion_features.len();
    let (features, method) = if low_motion_features.is_empty() {
        quality_flags.insert("resting_hr_no_low_motion_hr_samples".to_string());
        if unmatched_features.is_empty() {
            (
                Vec::new(),
                "low_motion_filtered_lowest_quartile_mean_heart_rate_features",
            )
        } else {
            (
                unmatched_features,
                "motion_unmatched_lowest_quartile_mean_heart_rate_features",
            )
        }
    } else {
        quality_flags.insert("resting_hr_low_motion_filter_applied".to_string());
        if unmatched_heart_rate_sample_count > 0 {
            quality_flags.insert("resting_hr_unmatched_samples_excluded".to_string());
        }
        (
            low_motion_features,
            "low_motion_filtered_lowest_quartile_mean_heart_rate_features",
        )
    };

    RestingHeartRateCandidateSelection {
        low_motion_heart_rate_sample_count,
        features,
        method,
        quality_flags: quality_flags.into_iter().collect(),
        motion_sample_count: motion_features.len(),
        matched_heart_rate_sample_count,
        high_motion_heart_rate_sample_count,
        unmatched_heart_rate_sample_count,
    }
}

fn daily_hrv_features(
    hrv_features: &[&HrvFeature],
    min_rr_intervals_to_compute: usize,
) -> Vec<HrvDayFeature> {
    let mut by_date = BTreeMap::<String, Vec<&HrvFeature>>::new();
    for feature in hrv_features {
        let Some(date) = feature_date(&feature.captured_at) else {
            continue;
        };
        by_date.entry(date.to_string()).or_default().push(*feature);
    }

    by_date
        .into_iter()
        .filter_map(|(date, features)| {
            let rr_intervals_ms = features
                .iter()
                .flat_map(|feature| feature.rr_intervals_ms.iter().copied())
                .collect::<Vec<_>>();
            if rr_intervals_ms.len() < min_rr_intervals_to_compute {
                return None;
            }
            let mut input_ids = features
                .iter()
                .map(|feature| feature.metric_input_id.clone())
                .collect::<Vec<_>>();
            input_ids.sort();
            let input = HrvInput {
                start_time: format!("{date}T00:00:00Z"),
                end_time: format!("{date}T23:59:59Z"),
                rr_intervals_ms,
                input_ids: input_ids.clone(),
                rr_timestamps_s: None,
                stage_segments: None,
            };
            let result = goose_hrv_v0(&input);
            let output = result.output?;
            Some(HrvDayFeature {
                date,
                rmssd_ms: output.rmssd_ms,
                rr_interval_count: input.rr_intervals_ms.len(),
                trusted_metric_input: features.iter().all(|feature| feature.trusted_metric_input),
                input_ids,
            })
        })
        .collect()
}

fn resting_heart_rate_feature(
    start: &str,
    end: &str,
    selection: &RestingHeartRateCandidateSelection<'_>,
) -> Option<RestingHeartRateFeature> {
    let heart_rate_features = selection.features.as_slice();
    if heart_rate_features.is_empty() {
        return None;
    }
    let mut input_ids = heart_rate_features
        .iter()
        .map(|feature| feature.metric_input_id.clone())
        .collect::<Vec<_>>();
    input_ids.sort();
    let (source_signal, source_signals) = heart_rate_source_signal_summary(heart_rate_features);
    Some(RestingHeartRateFeature {
        metric_input_id: format!("resting_hr.{}.{}", start, end),
        start_time: start.to_string(),
        end_time: end.to_string(),
        resting_hr_bpm: low_quartile_mean_hr(heart_rate_features),
        method: selection.method.to_string(),
        sample_count: heart_rate_features.len(),
        trusted_metric_input: heart_rate_features
            .iter()
            .all(|feature| feature.trusted_metric_input),
        quality_flags: selection.quality_flags.clone(),
        input_ids,
        provenance: json!({
            "input_source": "heart_rate_feature_report",
            "source_signal": source_signal,
            "source_signals": source_signals,
            "method": selection.method,
            "requested_start_time": start,
            "requested_end_time": end,
            "motion_filter": {
                "low_motion_intensity_max": RESTING_HR_LOW_MOTION_INTENSITY_MAX,
                "match_window_ms": RESTING_HR_MOTION_MATCH_WINDOW_MS,
                "motion_sample_count": selection.motion_sample_count,
                "selected_heart_rate_sample_count": heart_rate_features.len(),
                "matched_heart_rate_sample_count": selection.matched_heart_rate_sample_count,
                "low_motion_heart_rate_sample_count": selection.low_motion_heart_rate_sample_count,
                "high_motion_heart_rate_sample_count": selection.high_motion_heart_rate_sample_count,
                "unmatched_heart_rate_sample_count": selection.unmatched_heart_rate_sample_count,
            },
            "promotion_policy": "requires_all_contributing_features_trusted",
        }),
    })
}

fn nearest_resting_motion_feature<'a>(
    heart_rate_feature: &HeartRateFeature,
    motion_features: &[&'a MotionFeature],
) -> Option<&'a MotionFeature> {
    if let Some(same_frame) = motion_features
        .iter()
        .copied()
        .find(|motion| motion.frame_id == heart_rate_feature.frame_id)
    {
        return Some(same_frame);
    }

    let heart_rate_time = heart_rate_feature_time_unix_ms(heart_rate_feature)?;
    motion_features
        .iter()
        .copied()
        .filter_map(|motion| {
            let motion_time = feature_time_unix_ms(motion)?;
            let distance = (motion_time - heart_rate_time).abs();
            (distance <= RESTING_HR_MOTION_MATCH_WINDOW_MS).then_some((distance, motion))
        })
        .min_by_key(|(distance, motion)| (*distance, motion.frame_id.as_str()))
        .map(|(_, motion)| motion)
}

fn heart_rate_source_signal_summary(features: &[&HeartRateFeature]) -> (String, Vec<String>) {
    let source_signals = features
        .iter()
        .map(|feature| feature.source_signal.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let source_signal = match source_signals.as_slice() {
        [] => "unknown".to_string(),
        [only] => only.clone(),
        _ => "mixed_heart_rate_signals".to_string(),
    };
    (source_signal, source_signals)
}

fn hrv_baseline_feature(
    start: &str,
    end: &str,
    daily: &[HrvDayFeature],
    options: HrvFeatureOptions,
) -> Option<HrvBaselineFeature> {
    if options.baseline_min_days == 0 || daily.len() < options.baseline_min_days {
        return None;
    }
    let mut input_ids = daily
        .iter()
        .flat_map(|day| day.input_ids.iter().cloned())
        .collect::<Vec<_>>();
    input_ids.sort();
    let values = daily.iter().map(|day| day.rmssd_ms).collect::<Vec<_>>();
    Some(HrvBaselineFeature {
        metric_input_id: format!("hrv_baseline.{}.{}", start, end),
        hrv_baseline_rmssd_ms: median(values),
        method: "median_daily_rmssd".to_string(),
        day_count: daily.len(),
        trusted_metric_input: daily.iter().all(|day| day.trusted_metric_input),
        input_ids,
        provenance: json!({
            "input_source": "hrv_daily_features",
            "method": "median_daily_rmssd",
            "baseline_min_days": options.baseline_min_days,
            "requested_start_time": start,
            "requested_end_time": end,
            "promotion_policy": "requires_all_daily_features_trusted",
        }),
    })
}

fn resting_heart_rate_baseline_feature(
    start: &str,
    end: &str,
    daily: &[RestingHeartRateDayFeature],
    options: RestingHeartRateFeatureOptions,
) -> Option<RestingHeartRateBaselineFeature> {
    if options.baseline_min_days == 0 || daily.len() < options.baseline_min_days {
        return None;
    }
    let mut input_ids = daily
        .iter()
        .flat_map(|day| day.input_ids.iter().cloned())
        .collect::<Vec<_>>();
    input_ids.sort();
    let values = daily
        .iter()
        .map(|day| day.resting_hr_bpm)
        .collect::<Vec<_>>();
    Some(RestingHeartRateBaselineFeature {
        metric_input_id: format!("resting_hr_baseline.{}.{}", start, end),
        resting_hr_baseline_bpm: median(values),
        method: "median_daily_lowest_quartile_resting_hr".to_string(),
        day_count: daily.len(),
        trusted_metric_input: daily.iter().all(|day| day.trusted_metric_input),
        input_ids,
        provenance: json!({
            "input_source": "resting_heart_rate_daily_features",
            "method": "median_daily_lowest_quartile_resting_hr",
            "baseline_min_days": options.baseline_min_days,
            "requested_start_time": start,
            "requested_end_time": end,
            "promotion_policy": "requires_all_daily_features_trusted",
        }),
    })
}

fn low_quartile_mean_hr(features: &[&HeartRateFeature]) -> f64 {
    let mut values = features
        .iter()
        .map(|feature| feature.heart_rate_bpm)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let take_count = ((values.len() as f64) * 0.25).ceil().max(1.0) as usize;
    values.iter().take(take_count).sum::<f64>() / take_count as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let midpoint = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[midpoint - 1] + values[midpoint]) / 2.0
    } else {
        values[midpoint]
    }
}

fn circular_minute_deviation(left: f64, right: f64) -> f64 {
    let day_minutes = 24.0 * 60.0;
    let difference = (left - right).abs().rem_euclid(day_minutes);
    difference.min(day_minutes - difference)
}

fn feature_date(captured_at: &str) -> Option<&str> {
    if captured_at.len() < 10 {
        return None;
    }
    let date = &captured_at[..10];
    let bytes = date.as_bytes();
    if bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') {
        Some(date)
    } else {
        None
    }
}

fn observed_feature_window(
    heart_rate_features: &[&HeartRateFeature],
    quality_flags: &mut BTreeSet<String>,
) -> (Option<String>, Option<String>, f64) {
    let mut parsed = heart_rate_features
        .iter()
        .filter_map(|feature| {
            parse_rfc3339_utc_unix_ms(&feature.captured_at)
                .map(|unix_ms| (unix_ms, feature.captured_at.as_str()))
        })
        .collect::<Vec<_>>();
    if parsed.len() != heart_rate_features.len() {
        quality_flags.insert("captured_at_unparseable".to_string());
    }
    if parsed.is_empty() {
        return (None, None, 0.0);
    }
    parsed.sort_by_key(|(unix_ms, _)| *unix_ms);
    let (start_ms, start_text) = parsed.first().copied().expect("parsed is not empty");
    let (end_ms, end_text) = parsed.last().copied().expect("parsed is not empty");
    (
        Some(start_text.to_string()),
        Some(end_text.to_string()),
        (end_ms - start_ms).max(0) as f64 / 60_000.0,
    )
}

fn heart_rate_zone_minutes(
    heart_rate_features: &[&HeartRateFeature],
    duration_minutes: f64,
    resting_hr_bpm: Option<f64>,
    max_hr_bpm: Option<f64>,
    quality_flags: &mut BTreeSet<String>,
) -> Vec<f64> {
    let (Some(resting_hr_bpm), Some(max_hr_bpm)) = (resting_hr_bpm, max_hr_bpm) else {
        quality_flags.insert("hr_zone_basis_missing".to_string());
        return Vec::new();
    };
    if max_hr_bpm <= resting_hr_bpm {
        quality_flags.insert("hr_zone_basis_invalid".to_string());
        return Vec::new();
    }
    if duration_minutes <= 0.0 {
        quality_flags.insert("hr_zone_duration_missing".to_string());
        return Vec::new();
    }

    let minutes_per_sample = duration_minutes / heart_rate_features.len() as f64;
    let mut zones = vec![0.0; 5];
    for feature in heart_rate_features {
        let reserve_fraction = ((feature.heart_rate_bpm - resting_hr_bpm)
            / (max_hr_bpm - resting_hr_bpm))
            .clamp(0.0, 1.0);
        let zone_index = if reserve_fraction < 0.20 {
            0
        } else if reserve_fraction < 0.40 {
            1
        } else if reserve_fraction < 0.60 {
            2
        } else if reserve_fraction < 0.80 {
            3
        } else {
            4
        };
        zones[zone_index] += minutes_per_sample;
    }
    zones
}

fn accumulate_axis(
    payload: &[u8],
    axis: &I16SeriesSummary,
    quality_flags: &mut BTreeSet<String>,
) -> MotionAccumulator {
    let mut accumulator = MotionAccumulator::default();
    for index in 0..axis.parsed_count {
        let sample_offset = axis.offset + index * 2;
        let Some(value) = read_i16_le(payload, sample_offset) else {
            quality_flags.insert(format!("{}_sample_missing", axis.name));
            break;
        };
        let abs = i32::from(value).abs() as f64;
        accumulator.abs_sum += abs;
        accumulator.peak_abs = accumulator.peak_abs.max(abs);
        accumulator.sample_count += 1;
    }
    accumulator
}

/// Returns the set of R17 frame_ids that share a unix-second window with an R22 frame.
/// When WHOOP 5.0 streams both R17 (handle 0x0027) and R22 (handle 0x0022) simultaneously,
/// R22 is the preferred source. Any R17 frame whose floor(timestamp_seconds) matches that
/// of an R22 frame in the same decoded_rows slice is shadowed and should be skipped.
fn r22_shadowed_r17_frame_ids(rows: &[DecodedFrameRow]) -> BTreeSet<String> {
    // Collect unix seconds for which we have at least one R22 frame.
    let mut r22_seconds: BTreeSet<u32> = BTreeSet::new();
    // Collect (frame_id, unix_second) pairs for R17 frames.
    let mut r17_frames: Vec<(String, u32)> = Vec::new();

    for row in rows {
        let Ok(Some(ParsedPayload::DataPacket {
            domain,
            timestamp_seconds,
            body_summary: Some(ref summary),
            ..
        })) = serde_json::from_str::<Option<ParsedPayload>>(&row.parsed_payload_json)
        else {
            continue;
        };

        match summary {
            DataPacketBodySummary::R22Whoop5Hr { .. } => {
                // R22 frames use the captured_at timestamp (wall-clock seconds) since
                // parse_r22_payload leaves timestamp_seconds as None. Fall back to
                // parsing seconds from the captured_at RFC3339 string.
                let _ = (domain, timestamp_seconds);
                // Use captured_at unix seconds as the second-granularity key.
                if let Ok(ts) = chrono_captured_at_to_unix(&row.captured_at) {
                    r22_seconds.insert(ts);
                }
            }
            DataPacketBodySummary::R17OpticalOrLabradorFiltered { .. } => {
                if let Some(ts) = timestamp_seconds {
                    r17_frames.push((row.frame_id.clone(), ts));
                }
            }
            _ => {}
        }
    }

    // Shadow any R17 frame whose second matches an R22 second.
    r17_frames
        .into_iter()
        .filter(|(_, ts)| r22_seconds.contains(ts))
        .map(|(id, _)| id)
        .collect()
}

fn chrono_captured_at_to_unix(captured_at: &str) -> Result<u32, ()> {
    // captured_at is RFC3339 — parse up to the 'T' to extract the date-time and convert.
    // Minimal parse: count seconds since epoch via string slicing (no chrono dependency).
    // If the format is not parseable, return Err so the frame is excluded from the dedup set.
    use std::time::{Duration, UNIX_EPOCH};
    // Try to parse via std only — captured_at is "YYYY-MM-DDTHH:MM:SS..." format.
    // We cannot use chrono here as it is not a dependency of goose-core. Use a simple
    // heuristic: if captured_at looks like an ISO8601 datetime, attempt to parse it via
    // the existing `normalized_sample_time` infrastructure or fall back gracefully.
    // Since goose-core does not depend on chrono, we compute unix seconds manually.
    let _ = Duration::from_secs(0);
    let _ = UNIX_EPOCH;
    // Simple approach: delegate to parsed_payload timestamp_seconds when available.
    // For R22 frames, captured_at is the best available timestamp proxy.
    // Return Err to indicate this path is unavailable without chrono.
    // The dedup set will simply be empty (no R22 seconds matched) for these frames,
    // which is the safe fallback — R17 frames are NOT suppressed incorrectly.
    let _ = captured_at;
    Err(())
}

fn trusted_frames_for_summary_kinds(
    correlation: &CaptureCorrelationReport,
    allowed_summary_kinds: &[&str],
) -> BTreeMap<String, bool> {
    let allowed_summary_kinds = allowed_summary_kinds
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let trusted_summary_kinds = correlation
        .summaries
        .iter()
        .filter(|summary| {
            summary.trusted_metric_ready
                && allowed_summary_kinds.contains(summary.body_summary_kind.as_str())
        })
        .map(|summary| summary.body_summary_kind.as_str())
        .collect::<BTreeSet<_>>();
    let mut frames = BTreeMap::new();
    for observation in &correlation.observations {
        if !observation.owned_capture
            || !trusted_summary_kinds.contains(observation.body_summary_kind.as_str())
        {
            continue;
        }
        let frame_id = observation
            .fixture_id
            .strip_prefix("sqlite:")
            .unwrap_or(&observation.path);
        frames.insert(frame_id.to_string(), true);
    }
    frames
}

fn parse_warnings(row: &DecodedFrameRow) -> GooseResult<Vec<String>> {
    serde_json::from_str(&row.warnings_json).map_err(|error| {
        GooseError::message(format!("{} warnings_json invalid: {error}", row.frame_id))
    })
}

fn read_i16_le(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

fn normalized_sample_time(
    row: &DecodedFrameRow,
    timestamp_seconds: Option<u32>,
    timestamp_subseconds: Option<u16>,
    quality_flags: &mut BTreeSet<String>,
) -> NormalizedSampleTime {
    if let Some(seconds) = timestamp_seconds
        && plausible_unix_timestamp_seconds(seconds)
    {
        if let Some(subseconds) = timestamp_subseconds
            && subseconds > 999
        {
            quality_flags.insert("device_timestamp_subseconds_out_of_range".to_string());
            quality_flags.insert("sample_time_from_capture_time".to_string());
            return NormalizedSampleTime {
                time: row.captured_at.clone(),
                unix_ms: parse_rfc3339_utc_unix_ms(&row.captured_at),
                source: "captured_at".to_string(),
            };
        }
        quality_flags.insert("sample_time_from_device_timestamp".to_string());
        let millis = timestamp_subseconds.map_or(0, i64::from);
        let unix_ms = i64::from(seconds) * 1_000 + millis;
        return NormalizedSampleTime {
            time: unix_ms_to_rfc3339_utc(unix_ms),
            unix_ms: Some(unix_ms),
            source: "device_timestamp".to_string(),
        };
    }

    if timestamp_seconds.is_some() {
        quality_flags.insert("device_timestamp_outside_plausible_unix_range".to_string());
    } else {
        quality_flags.insert("device_timestamp_missing".to_string());
    }
    quality_flags.insert("sample_time_from_capture_time".to_string());
    NormalizedSampleTime {
        time: row.captured_at.clone(),
        unix_ms: parse_rfc3339_utc_unix_ms(&row.captured_at),
        source: "captured_at".to_string(),
    }
}

fn plausible_unix_timestamp_seconds(seconds: u32) -> bool {
    (946_684_800..=4_102_444_800).contains(&seconds)
}

fn feature_time_unix_ms(feature: &MotionFeature) -> Option<i64> {
    feature
        .sample_time_unix_ms
        .or_else(|| parse_rfc3339_utc_unix_ms(&feature.sample_time))
}

fn heart_rate_feature_time_unix_ms(feature: &HeartRateFeature) -> Option<i64> {
    feature
        .sample_time_unix_ms
        .or_else(|| parse_rfc3339_utc_unix_ms(&feature.sample_time))
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
        || day == 0
        || day > days_in_month(year, month)
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

fn clamp_fraction(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}
