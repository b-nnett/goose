use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GooseError, GooseResult,
    metric_features::{
        MetricFeatureNextAction, RecoverySensorDiscoveryOptions, RecoverySensorDiscoveryReport,
        RecoverySensorWidgetDiscovery, RestingHeartRateFeatureOptions,
        RestingHeartRateFeatureReport, run_recovery_sensor_discovery_report_for_store,
        run_resting_heart_rate_feature_report_for_store,
    },
    store::{DailyRecoveryMetricInput, GooseStore, MetricProvenanceInput},
    validation_labels::{
        OFFICIAL_WHOOP_LABEL_POLICY, official_label_policy_issue_action,
        official_label_policy_issues,
    },
};

pub const RESTING_HEART_RATE_DAILY_ROLLUP_REPORT_SCHEMA: &str =
    "goose.resting-heart-rate-daily-rollup-report.v1";
pub const RESTING_HEART_RATE_CAPTURE_VALIDATION_REPORT_SCHEMA: &str =
    "goose.resting-heart-rate-capture-validation-report.v1";
pub const RECOVERY_UNAVAILABLE_DAILY_STATUS_REPORT_SCHEMA: &str =
    "goose.recovery-unavailable-daily-status-report.v1";
pub const RECOVERY_SENSOR_DAILY_ROLLUP_REPORT_SCHEMA: &str =
    "goose.recovery-sensor-daily-rollup-report.v1";
pub const GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID: &str =
    "goose.resting_heart_rate.device_sensor.v0";
pub const GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION: &str = "0.1.0";
pub const GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_ID: &str = "goose.recovery.unavailable_status.v0";
pub const GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_VERSION: &str = "0.1.0";
pub const GOOSE_RECOVERY_SENSOR_DEVICE_SENSOR_V0_ID: &str =
    "goose.recovery_sensor.device_sensor.v0";
pub const GOOSE_RECOVERY_SENSOR_DEVICE_SENSOR_V0_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Copy)]
pub struct RestingHeartRateDailyRollupOptions<'a> {
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub baseline_min_days: usize,
    pub require_baseline: bool,
    pub min_sample_count: usize,
    pub write_metric: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryUnavailableDailyStatusOptions<'a> {
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub min_rr_intervals_to_compute: usize,
    pub write_metric: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoverySensorDailyRollupOptions<'a> {
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub min_rr_intervals_to_compute: usize,
    pub write_metric: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateDailyRollupReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub min_sample_count: usize,
    pub sample_count: usize,
    pub trusted_metric_input: bool,
    pub resting_hr_bpm: Option<f64>,
    pub rolling_7_day_average_bpm: Option<f64>,
    pub rolling_7_day_sample_count: usize,
    pub rolling_30_day_average_bpm: Option<f64>,
    pub rolling_30_day_sample_count: usize,
    pub selected_vs_7_day_average_bpm: Option<f64>,
    pub selected_vs_30_day_average_bpm: Option<f64>,
    pub confidence: f64,
    pub source_signals: Vec<String>,
    pub daily_metric_id: Option<String>,
    pub daily_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
    pub quality_flags: Vec<String>,
    pub feature_report: RestingHeartRateFeatureReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<RestingHeartRateRollupNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryUnavailableDailyStatusReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub write_metric: bool,
    pub unavailable_metric_count: usize,
    pub written_metric_count: usize,
    pub metric_provenance_written_count: usize,
    pub statuses: Vec<RecoveryUnavailableMetricStatus>,
    pub recovery_sensor_discovery: RecoverySensorDiscoveryReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySensorDailyRollupReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub write_metric: bool,
    pub metric_count: usize,
    pub promotable_metric_count: usize,
    pub promoted_metric_count: usize,
    pub written_metric_count: usize,
    pub metric_provenance_written_count: usize,
    pub statuses: Vec<RecoverySensorDailyMetricStatus>,
    pub recovery_sensor_discovery: RecoverySensorDiscoveryReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<MetricFeatureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySensorDailyMetricStatus {
    pub metric_id: String,
    pub metric_name: String,
    pub unit: String,
    pub source_kind: String,
    pub promotion_status: String,
    pub promotion_allowed: bool,
    pub user_visible_value_allowed: bool,
    pub local_value: Option<f64>,
    pub confidence: f64,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub resolved_metric_input_count: usize,
    pub value_semantics_verified_count: usize,
    pub candidate_source_signals: Vec<String>,
    pub value_source: Option<String>,
    pub input_ids: Vec<String>,
    pub blocker_reasons: Vec<String>,
    pub quality_flags: Vec<String>,
    pub daily_metric_id: Option<String>,
    pub daily_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryUnavailableMetricStatus {
    pub metric_id: String,
    pub metric_name: String,
    pub source_kind: String,
    pub promotion_status: String,
    pub candidate_count: usize,
    pub trusted_candidate_count: usize,
    pub resolved_metric_input_count: usize,
    pub value_semantics_verified_count: usize,
    pub candidate_source_signals: Vec<String>,
    pub blocker_reasons: Vec<String>,
    pub quality_flags: Vec<String>,
    pub daily_metric_id: Option<String>,
    pub daily_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RestingHeartRateRollupNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct RestingHeartRateCaptureValidationOptions<'a> {
    pub rollup_options: RestingHeartRateDailyRollupOptions<'a>,
    pub capture_kind: Option<String>,
    pub official_whoop_resting_hr_bpm: Option<f64>,
    pub tolerance_bpm: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestingHeartRateCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub capture_kind: Option<String>,
    pub label_policy: String,
    pub official_whoop_resting_hr_bpm: Option<f64>,
    pub tolerance_bpm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_resting_hr_bpm: Option<f64>,
    pub resting_hr_error_bpm: Option<f64>,
    pub resting_hr_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub confidence: f64,
    pub sample_count: usize,
    pub trusted_metric_input: bool,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub resting_hr_rollup: RestingHeartRateDailyRollupReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<RestingHeartRateRollupNextAction>,
}

#[derive(Debug, Clone)]
struct CurrentRestingHeartRateMetric {
    metric_id: String,
    resting_hr_bpm: f64,
}

pub fn rollup_resting_heart_rate_day_for_store(
    store: &GooseStore,
    database_path: &str,
    options: RestingHeartRateDailyRollupOptions<'_>,
) -> GooseResult<RestingHeartRateDailyRollupReport> {
    validate_options(&options)?;
    let start_time_unix_ms = parse_rfc3339_utc_unix_ms(options.start)
        .ok_or_else(|| GooseError::message("start must be an RFC3339 UTC timestamp"))?;
    let end_time_unix_ms = parse_rfc3339_utc_unix_ms(options.end)
        .ok_or_else(|| GooseError::message("end must be an RFC3339 UTC timestamp"))?;
    if end_time_unix_ms <= start_time_unix_ms {
        return Err(GooseError::message("end must be after start"));
    }

    let feature_report = run_resting_heart_rate_feature_report_for_store(
        store,
        database_path,
        options.start,
        options.end,
        RestingHeartRateFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            baseline_min_days: options.baseline_min_days,
            require_baseline: options.require_baseline,
        },
    )?;

    let mut issues = feature_report.issues.clone();
    let resting = feature_report.resting.as_ref();
    let sample_count = resting.map(|feature| feature.sample_count).unwrap_or(0);
    if resting.is_none() {
        issues.push("no_resting_heart_rate_feature".to_string());
    }
    if sample_count < options.min_sample_count {
        issues.push("insufficient_heart_rate_samples".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = resting
        .map(|feature| feature.quality_flags.clone())
        .unwrap_or_default();
    quality_flags.push("daily_rhr_lowest_quartile_hr".to_string());
    if options.require_trusted_evidence {
        quality_flags.push("requires_trusted_capture_evidence".to_string());
    }
    if resting
        .map(|feature| !feature.trusted_metric_input)
        .unwrap_or(false)
    {
        quality_flags.push("untrusted_capture_evidence".to_string());
    }
    quality_flags.sort();
    quality_flags.dedup();

    let source_signals = resting
        .map(|feature| source_signals_from_provenance(&feature.provenance))
        .unwrap_or_default();
    let pass = issues.is_empty();
    let trusted_metric_input = resting
        .map(|feature| feature.trusted_metric_input)
        .unwrap_or(false);
    let confidence = if let Some(resting) = resting {
        if pass {
            resting_heart_rate_confidence(
                resting.sample_count,
                options.min_sample_count,
                resting.trusted_metric_input,
                source_signals.len(),
            )
        } else {
            0.0
        }
    } else {
        0.0
    };

    let metric_id = daily_recovery_metric_id(options.date_key, options.timezone);
    let provenance_id = format!("prov-{metric_id}");
    let mut daily_metric_written = false;
    let mut metric_provenance_written = false;

    if pass && options.write_metric {
        let resting = resting.expect("pass requires resting feature");
        let motion_filter = resting
            .provenance
            .get("motion_filter")
            .cloned()
            .unwrap_or(Value::Null);
        let inputs_json = json!({
            "heart_rate_feature_input_ids": resting.input_ids,
            "heart_rate_sample_count": resting.sample_count,
            "min_sample_count": options.min_sample_count,
            "source_signals": source_signals,
            "feature_report_schema": feature_report.schema,
            "resting_feature_id": resting.metric_input_id,
            "method": resting.method,
            "motion_filter": motion_filter,
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!("cannot serialize RHR quality flags: {error}"))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID,
            "algorithm_version": GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION,
            "source_kind": "device_sensor",
            "date_key": options.date_key,
            "timezone": options.timezone,
            "start": options.start,
            "end": options.end,
            "start_time_unix_ms": start_time_unix_ms,
            "end_time_unix_ms": end_time_unix_ms,
            "motion_filter": motion_filter,
            "promotion_policy": "explicit_write_metric_flag_required",
            "official_labels_policy": "not_used",
        })
        .to_string();

        daily_metric_written = store.upsert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: &metric_id,
            date_key: options.date_key,
            timezone: options.timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            resting_hr_bpm: Some(resting.resting_hr_bpm),
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "device_sensor",
            confidence,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "daily_recovery",
            metric_id: &metric_id,
            source_kind: "device_sensor",
            source_detail: "WHOOP packet-derived heart-rate samples",
            confidence: Some(confidence),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    let current = if pass {
        resting.map(|feature| CurrentRestingHeartRateMetric {
            metric_id: metric_id.clone(),
            resting_hr_bpm: feature.resting_hr_bpm,
        })
    } else {
        None
    };
    let rolling_7_day = rolling_average(store, end_time_unix_ms, 7, current.as_ref())?;
    let rolling_30_day = rolling_average(store, end_time_unix_ms, 30, current.as_ref())?;
    let resting_hr_bpm = pass
        .then_some(resting.map(|feature| feature.resting_hr_bpm))
        .flatten();
    let selected_vs_7_day_average_bpm =
        delta_from_average(resting_hr_bpm, rolling_7_day.average_bpm);
    let selected_vs_30_day_average_bpm =
        delta_from_average(resting_hr_bpm, rolling_30_day.average_bpm);
    let next_actions = rollup_next_actions(&issues);

    Ok(RestingHeartRateDailyRollupReport {
        schema: RESTING_HEART_RATE_DAILY_ROLLUP_REPORT_SCHEMA.to_string(),
        generated_by: "goose-resting-heart-rate-daily-rollup".to_string(),
        pass,
        date_key: options.date_key.to_string(),
        timezone: options.timezone.to_string(),
        start: options.start.to_string(),
        end: options.end.to_string(),
        start_time_unix_ms,
        end_time_unix_ms,
        min_sample_count: options.min_sample_count,
        sample_count,
        trusted_metric_input,
        resting_hr_bpm,
        rolling_7_day_average_bpm: rolling_7_day.average_bpm,
        rolling_7_day_sample_count: rolling_7_day.sample_count,
        rolling_30_day_average_bpm: rolling_30_day.average_bpm,
        rolling_30_day_sample_count: rolling_30_day.sample_count,
        selected_vs_7_day_average_bpm,
        selected_vs_30_day_average_bpm,
        confidence,
        source_signals,
        daily_metric_id: (pass && options.write_metric).then_some(metric_id),
        daily_metric_written,
        metric_provenance_id: (pass && options.write_metric).then_some(provenance_id),
        metric_provenance_written,
        quality_flags,
        feature_report,
        issues,
        next_actions,
    })
}

pub fn validate_resting_heart_rate_capture_for_store(
    store: &GooseStore,
    database_path: &str,
    options: RestingHeartRateCaptureValidationOptions<'_>,
) -> GooseResult<RestingHeartRateCaptureValidationReport> {
    validate_rhr_validation_options(&options)?;
    let mut rollup_options = options.rollup_options;
    rollup_options.write_metric = false;
    let resting_hr_rollup =
        rollup_resting_heart_rate_day_for_store(store, database_path, rollup_options)?;

    let comparison = compare_rhr_label(
        resting_hr_rollup.resting_hr_bpm,
        options.official_whoop_resting_hr_bpm,
        options.tolerance_bpm,
    );
    let provided_label_count = usize::from(options.official_whoop_resting_hr_bpm.is_some());
    let matching_label_count = usize::from(comparison.within_tolerance == Some(true));

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_resting_hr_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !resting_hr_rollup.pass {
        issues.push("resting_hr_rollup_blocked".to_string());
        for issue in &resting_hr_rollup.issues {
            issues.push(format!("resting_hr_rollup_issue:{issue}"));
        }
    }
    if options.official_whoop_resting_hr_bpm.is_some() && resting_hr_rollup.resting_hr_bpm.is_none()
    {
        issues.push("local_resting_hr_missing".to_string());
    }
    if comparison.within_tolerance == Some(false) {
        issues.push("resting_hr_label_delta_out_of_tolerance".to_string());
    }
    issues.sort();
    issues.dedup();

    Ok(RestingHeartRateCaptureValidationReport {
        schema: RESTING_HEART_RATE_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-resting-heart-rate-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        date_key: resting_hr_rollup.date_key.clone(),
        timezone: resting_hr_rollup.timezone.clone(),
        start: resting_hr_rollup.start.clone(),
        end: resting_hr_rollup.end.clone(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_resting_hr_bpm: options.official_whoop_resting_hr_bpm,
        tolerance_bpm: options.tolerance_bpm,
        label_provenance: options.label_provenance,
        local_resting_hr_bpm: resting_hr_rollup.resting_hr_bpm,
        resting_hr_error_bpm: comparison.error,
        resting_hr_within_tolerance: comparison.within_tolerance,
        provided_label_count,
        matching_label_count,
        confidence: resting_hr_rollup.confidence,
        sample_count: resting_hr_rollup.sample_count,
        trusted_metric_input: resting_hr_rollup.trusted_metric_input,
        algorithm_id: GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_ID.to_string(),
        algorithm_version: GOOSE_RESTING_HEART_RATE_DEVICE_SENSOR_V0_VERSION.to_string(),
        resting_hr_rollup,
        next_actions: rhr_validation_next_actions(&issues),
        issues,
    })
}

pub fn rollup_recovery_unavailable_daily_status_for_store(
    store: &GooseStore,
    database_path: &str,
    options: RecoveryUnavailableDailyStatusOptions<'_>,
) -> GooseResult<RecoveryUnavailableDailyStatusReport> {
    validate_recovery_unavailable_options(&options)?;
    let start_time_unix_ms = parse_rfc3339_utc_unix_ms(options.start)
        .ok_or_else(|| GooseError::message("start must be an RFC3339 UTC timestamp"))?;
    let end_time_unix_ms = parse_rfc3339_utc_unix_ms(options.end)
        .ok_or_else(|| GooseError::message("end must be an RFC3339 UTC timestamp"))?;
    if end_time_unix_ms <= start_time_unix_ms {
        return Err(GooseError::message("end must be after start"));
    }

    let recovery_sensor_discovery = run_recovery_sensor_discovery_report_for_store(
        store,
        database_path,
        options.start,
        options.end,
        RecoverySensorDiscoveryOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.min_rr_intervals_to_compute,
        },
    )?;

    let mut statuses = Vec::new();
    let mut written_metric_count = 0usize;
    let mut metric_provenance_written_count = 0usize;
    for widget in recovery_sensor_discovery
        .widgets
        .iter()
        .filter(|widget| !widget.user_visible_value_allowed)
    {
        let status = recovery_unavailable_metric_status_for_widget(
            store,
            widget,
            &recovery_sensor_discovery,
            &options,
            start_time_unix_ms,
            end_time_unix_ms,
        )?;
        if status.daily_metric_written {
            written_metric_count += 1;
        }
        if status.metric_provenance_written {
            metric_provenance_written_count += 1;
        }
        statuses.push(status);
    }

    let next_actions = recovery_sensor_discovery.next_actions.clone();
    let issues = Vec::new();

    Ok(RecoveryUnavailableDailyStatusReport {
        schema: RECOVERY_UNAVAILABLE_DAILY_STATUS_REPORT_SCHEMA.to_string(),
        generated_by: "goose-recovery-unavailable-daily-status".to_string(),
        pass: issues.is_empty(),
        date_key: options.date_key.to_string(),
        timezone: options.timezone.to_string(),
        start: options.start.to_string(),
        end: options.end.to_string(),
        start_time_unix_ms,
        end_time_unix_ms,
        write_metric: options.write_metric,
        unavailable_metric_count: statuses.len(),
        written_metric_count,
        metric_provenance_written_count,
        statuses,
        recovery_sensor_discovery,
        issues,
        next_actions,
    })
}

pub fn rollup_recovery_sensor_daily_for_store(
    store: &GooseStore,
    database_path: &str,
    options: RecoverySensorDailyRollupOptions<'_>,
) -> GooseResult<RecoverySensorDailyRollupReport> {
    validate_recovery_sensor_daily_options(&options)?;
    let start_time_unix_ms = parse_rfc3339_utc_unix_ms(options.start)
        .ok_or_else(|| GooseError::message("start must be an RFC3339 UTC timestamp"))?;
    let end_time_unix_ms = parse_rfc3339_utc_unix_ms(options.end)
        .ok_or_else(|| GooseError::message("end must be after start"))?;
    if end_time_unix_ms <= start_time_unix_ms {
        return Err(GooseError::message("end must be after start"));
    }

    let recovery_sensor_discovery = run_recovery_sensor_discovery_report_for_store(
        store,
        database_path,
        options.start,
        options.end,
        RecoverySensorDiscoveryOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            min_rr_intervals_to_compute: options.min_rr_intervals_to_compute,
        },
    )?;

    let mut statuses = Vec::new();
    let mut written_metric_count = 0usize;
    let mut metric_provenance_written_count = 0usize;
    for widget in &recovery_sensor_discovery.widgets {
        let status = recovery_sensor_daily_metric_status_for_widget(
            store,
            widget,
            &recovery_sensor_discovery,
            &options,
            start_time_unix_ms,
            end_time_unix_ms,
        )?;
        if status.daily_metric_written {
            written_metric_count += 1;
        }
        if status.metric_provenance_written {
            metric_provenance_written_count += 1;
        }
        statuses.push(status);
    }

    let mut issues = recovery_sensor_discovery.issues.clone();
    for status in &statuses {
        for blocker in &status.blocker_reasons {
            issues.push(format!("{}:{blocker}", status.metric_id));
        }
    }
    issues.sort();
    issues.dedup();

    let promotable_metric_count = statuses
        .iter()
        .filter(|status| status.promotion_allowed)
        .count();
    let promoted_metric_count = statuses
        .iter()
        .filter(|status| status.source_kind == "device_sensor" && status.local_value.is_some())
        .count();
    let mut next_actions = recovery_sensor_discovery.next_actions.clone();
    next_actions.extend(
        statuses
            .iter()
            .flat_map(|status| status.blocker_reasons.iter().map(move |reason| (status, reason)))
            .filter(|(_, reason)| reason.as_str() == "promotable_metric_value_missing")
            .map(|(status, reason)| MetricFeatureNextAction {
                scope: status.metric_id.clone(),
                reason: reason.clone(),
                action: format!(
                    "Add a validated value extraction path for {} before writing daily recovery metrics.",
                    status.metric_name
                ),
            }),
    );
    next_actions.sort();
    next_actions.dedup();

    Ok(RecoverySensorDailyRollupReport {
        schema: RECOVERY_SENSOR_DAILY_ROLLUP_REPORT_SCHEMA.to_string(),
        generated_by: "goose-recovery-sensor-daily-rollup".to_string(),
        pass: issues.is_empty(),
        date_key: options.date_key.to_string(),
        timezone: options.timezone.to_string(),
        start: options.start.to_string(),
        end: options.end.to_string(),
        start_time_unix_ms,
        end_time_unix_ms,
        write_metric: options.write_metric,
        metric_count: statuses.len(),
        promotable_metric_count,
        promoted_metric_count,
        written_metric_count,
        metric_provenance_written_count,
        statuses,
        recovery_sensor_discovery,
        issues,
        next_actions,
    })
}

fn recovery_unavailable_metric_status_for_widget(
    store: &GooseStore,
    widget: &RecoverySensorWidgetDiscovery,
    discovery: &RecoverySensorDiscoveryReport,
    options: &RecoveryUnavailableDailyStatusOptions<'_>,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) -> GooseResult<RecoveryUnavailableMetricStatus> {
    let metric_name = recovery_metric_name(&widget.metric_id).to_string();
    let metric_id =
        recovery_unavailable_metric_id(&widget.metric_id, options.date_key, options.timezone);
    let provenance_id = format!("prov-{metric_id}");
    let mut daily_metric_written = false;
    let mut metric_provenance_written = false;

    let quality_flags = unavailable_quality_flags(widget);
    if options.write_metric {
        let inputs_json = json!({
            "metric_id": widget.metric_id,
            "metric_name": metric_name,
            "discovery_report_schema": discovery.schema,
            "start": options.start,
            "end": options.end,
            "candidate_count": widget.candidate_count,
            "trusted_candidate_count": widget.trusted_candidate_count,
            "resolved_metric_input_count": widget.resolved_metric_input_count,
            "value_semantics_verified_count": widget.value_semantics_verified_count,
            "candidate_source_signals": widget.candidate_source_signals,
            "blocker_reasons": widget.blocker_reasons,
            "widget_provenance": widget.provenance,
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!(
                "cannot serialize recovery unavailable quality flags: {error}"
            ))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_ID,
            "algorithm_version": GOOSE_RECOVERY_UNAVAILABLE_STATUS_V0_VERSION,
            "source_kind": "unavailable",
            "metric_id": widget.metric_id,
            "metric_name": metric_name,
            "date_key": options.date_key,
            "timezone": options.timezone,
            "start": options.start,
            "end": options.end,
            "start_time_unix_ms": start_time_unix_ms,
            "end_time_unix_ms": end_time_unix_ms,
            "promotion_status": widget.promotion_status,
            "promotion_allowed": widget.promotion_allowed,
            "user_visible_value_allowed": widget.user_visible_value_allowed,
            "blocker_reasons": widget.blocker_reasons,
            "candidate_source_signals": widget.candidate_source_signals,
            "discovery_report_schema": discovery.schema,
            "official_labels_policy": "not_used",
            "value_policy": "no_metric_value_written_until_packet_semantics_are_verified",
        })
        .to_string();

        daily_metric_written = store.upsert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: &metric_id,
            date_key: options.date_key,
            timezone: options.timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "unavailable",
            confidence: 0.0,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "daily_recovery",
            metric_id: &metric_id,
            source_kind: "unavailable",
            source_detail: "recovery widget blocked by local WHOOP packet promotion gate",
            confidence: Some(0.0),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    Ok(RecoveryUnavailableMetricStatus {
        metric_id: widget.metric_id.clone(),
        metric_name,
        source_kind: "unavailable".to_string(),
        promotion_status: widget.promotion_status.clone(),
        candidate_count: widget.candidate_count,
        trusted_candidate_count: widget.trusted_candidate_count,
        resolved_metric_input_count: widget.resolved_metric_input_count,
        value_semantics_verified_count: widget.value_semantics_verified_count,
        candidate_source_signals: widget.candidate_source_signals.clone(),
        blocker_reasons: widget.blocker_reasons.clone(),
        quality_flags,
        daily_metric_id: options.write_metric.then_some(metric_id),
        daily_metric_written,
        metric_provenance_id: options.write_metric.then_some(provenance_id),
        metric_provenance_written,
    })
}

fn recovery_sensor_daily_metric_status_for_widget(
    store: &GooseStore,
    widget: &RecoverySensorWidgetDiscovery,
    discovery: &RecoverySensorDiscoveryReport,
    options: &RecoverySensorDailyRollupOptions<'_>,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) -> GooseResult<RecoverySensorDailyMetricStatus> {
    let metric_name = recovery_metric_name(&widget.metric_id).to_string();
    let value = recovery_sensor_metric_value(widget, discovery, options.date_key);
    let mut blocker_reasons = widget.blocker_reasons.clone();
    blocker_reasons.extend(value.blocker_reasons.clone());
    blocker_reasons.sort();
    blocker_reasons.dedup();

    let promotable_value = widget.promotion_allowed && value.local_value.is_some();
    let source_kind = if promotable_value {
        "device_sensor"
    } else {
        "unavailable"
    };
    let confidence = if promotable_value {
        widget.confidence
    } else {
        0.0
    };
    let daily_metric_id =
        recovery_sensor_metric_id(&widget.metric_id, options.date_key, options.timezone);
    let provenance_id = format!("prov-{daily_metric_id}");
    let quality_flags = recovery_sensor_daily_quality_flags(widget, &blocker_reasons, source_kind);
    let mut daily_metric_written = false;
    let mut metric_provenance_written = false;

    if options.write_metric && promotable_value {
        let local_value = value.local_value.expect("promotable value checked");
        let inputs_json = json!({
            "metric_id": widget.metric_id,
            "metric_name": metric_name,
            "unit": value.unit,
            "local_value": local_value,
            "value_source": value.value_source,
            "input_ids": value.input_ids,
            "discovery_report_schema": discovery.schema,
            "start": options.start,
            "end": options.end,
            "candidate_count": widget.candidate_count,
            "trusted_candidate_count": widget.trusted_candidate_count,
            "resolved_metric_input_count": widget.resolved_metric_input_count,
            "value_semantics_verified_count": widget.value_semantics_verified_count,
            "candidate_source_signals": widget.candidate_source_signals,
            "widget_provenance": widget.provenance,
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!(
                "cannot serialize recovery sensor daily quality flags: {error}"
            ))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_RECOVERY_SENSOR_DEVICE_SENSOR_V0_ID,
            "algorithm_version": GOOSE_RECOVERY_SENSOR_DEVICE_SENSOR_V0_VERSION,
            "source_kind": "device_sensor",
            "metric_id": widget.metric_id,
            "metric_name": metric_name,
            "date_key": options.date_key,
            "timezone": options.timezone,
            "start": options.start,
            "end": options.end,
            "start_time_unix_ms": start_time_unix_ms,
            "end_time_unix_ms": end_time_unix_ms,
            "promotion_status": widget.promotion_status,
            "promotion_allowed": widget.promotion_allowed,
            "user_visible_value_allowed": widget.user_visible_value_allowed,
            "candidate_source_signals": widget.candidate_source_signals,
            "value_source": value.value_source,
            "input_ids": value.input_ids,
            "discovery_report_schema": discovery.schema,
            "promotion_policy": "requires_recovery_sensor_discovery_promotable_widget_and_validated_value",
            "official_labels_policy": "not_used",
        })
        .to_string();

        let (
            hrv_rmssd_ms,
            respiratory_rate_rpm,
            oxygen_saturation_percent,
            skin_temperature_delta_c,
        ) = recovery_sensor_daily_metric_values(&widget.metric_id, local_value);
        daily_metric_written = store.upsert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: &daily_metric_id,
            date_key: options.date_key,
            timezone: options.timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            resting_hr_bpm: None,
            hrv_rmssd_ms,
            respiratory_rate_rpm,
            oxygen_saturation_percent,
            skin_temperature_delta_c,
            source_kind: "device_sensor",
            confidence,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "daily_recovery",
            metric_id: &daily_metric_id,
            source_kind: "device_sensor",
            source_detail: "WHOOP packet-derived recovery sensor metric",
            confidence: Some(confidence),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    Ok(RecoverySensorDailyMetricStatus {
        metric_id: widget.metric_id.clone(),
        metric_name,
        unit: value.unit,
        source_kind: source_kind.to_string(),
        promotion_status: widget.promotion_status.clone(),
        promotion_allowed: widget.promotion_allowed,
        user_visible_value_allowed: widget.user_visible_value_allowed,
        local_value: value.local_value,
        confidence,
        candidate_count: widget.candidate_count,
        trusted_candidate_count: widget.trusted_candidate_count,
        resolved_metric_input_count: widget.resolved_metric_input_count,
        value_semantics_verified_count: widget.value_semantics_verified_count,
        candidate_source_signals: widget.candidate_source_signals.clone(),
        value_source: value.value_source,
        input_ids: value.input_ids,
        blocker_reasons,
        quality_flags,
        daily_metric_id: (options.write_metric && promotable_value).then_some(daily_metric_id),
        daily_metric_written,
        metric_provenance_id: (options.write_metric && promotable_value).then_some(provenance_id),
        metric_provenance_written,
    })
}

#[derive(Debug, Clone)]
struct RecoverySensorMetricValue {
    local_value: Option<f64>,
    unit: String,
    value_source: Option<String>,
    input_ids: Vec<String>,
    blocker_reasons: Vec<String>,
}

fn recovery_sensor_metric_value(
    widget: &RecoverySensorWidgetDiscovery,
    discovery: &RecoverySensorDiscoveryReport,
    date_key: &str,
) -> RecoverySensorMetricValue {
    if !widget.promotion_allowed {
        return RecoverySensorMetricValue {
            local_value: None,
            unit: recovery_metric_unit(&widget.metric_id).to_string(),
            value_source: None,
            input_ids: Vec::new(),
            blocker_reasons: Vec::new(),
        };
    }

    match widget.metric_id.as_str() {
        "hrv_rmssd_ms" => {
            if let Some(day) = discovery
                .hrv_report
                .daily
                .iter()
                .find(|day| day.date == date_key && day.trusted_metric_input)
            {
                RecoverySensorMetricValue {
                    local_value: Some(day.rmssd_ms),
                    unit: "ms".to_string(),
                    value_source: Some("metrics.hrv_features.daily.rmssd_ms".to_string()),
                    input_ids: day.input_ids.clone(),
                    blocker_reasons: Vec::new(),
                }
            } else {
                recovery_sensor_missing_value("ms", "no_promotable_hrv_daily_feature_for_date")
            }
        }
        "respiratory_rate_rpm" => {
            let features = discovery
                .vital_event_report
                .respiratory_rate_inputs
                .iter()
                .filter(|feature| {
                    feature.trusted_candidate_evidence
                        && feature.resolved_metric_input
                        && feature.value_semantics_verified
                        && feature.respiratory_rate_rpm.is_some()
                })
                .collect::<Vec<_>>();
            let values = features
                .iter()
                .filter_map(|feature| feature.respiratory_rate_rpm)
                .collect::<Vec<_>>();
            if values.is_empty() {
                recovery_sensor_missing_value("rpm", "no_promotable_respiratory_rate_value")
            } else {
                let mut input_ids = features
                    .iter()
                    .map(|feature| feature.metric_input_id.clone())
                    .collect::<Vec<_>>();
                input_ids.sort();
                RecoverySensorMetricValue {
                    local_value: Some(round_1(median_f64(values))),
                    unit: "rpm".to_string(),
                    value_source: Some(
                        "metrics.vital_event_features.respiratory_rate_inputs".to_string(),
                    ),
                    input_ids,
                    blocker_reasons: Vec::new(),
                }
            }
        }
        "skin_temperature_delta_c" => {
            let features = discovery
                .vital_event_report
                .skin_temperature_inputs
                .iter()
                .filter(|feature| {
                    feature.trusted_candidate_evidence
                        && feature.resolved_metric_input
                        && feature.value_semantics_verified
                })
                .collect::<Vec<_>>();
            let values = features
                .iter()
                .filter_map(|feature| {
                    feature
                        .provenance
                        .get("skin_temperature_delta_c")
                        .and_then(serde_json::Value::as_f64)
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                recovery_sensor_missing_value(
                    "celsius_delta",
                    "no_promotable_skin_temperature_delta_value",
                )
            } else {
                let mut input_ids = features
                    .iter()
                    .map(|feature| feature.metric_input_id.clone())
                    .collect::<Vec<_>>();
                input_ids.sort();
                RecoverySensorMetricValue {
                    local_value: Some(round_1(median_f64(values))),
                    unit: "celsius_delta".to_string(),
                    value_source: Some(
                        "metrics.vital_event_features.skin_temperature_delta_c".to_string(),
                    ),
                    input_ids,
                    blocker_reasons: Vec::new(),
                }
            }
        }
        "oxygen_saturation_percent" => {
            recovery_sensor_missing_value("percent", "no_promotable_oxygen_saturation_value")
        }
        _ => recovery_sensor_missing_value("unknown", "unknown_recovery_sensor_metric"),
    }
}

fn recovery_sensor_missing_value(unit: &str, reason: &str) -> RecoverySensorMetricValue {
    RecoverySensorMetricValue {
        local_value: None,
        unit: unit.to_string(),
        value_source: None,
        input_ids: Vec::new(),
        blocker_reasons: vec![
            "promotable_metric_value_missing".to_string(),
            reason.to_string(),
        ],
    }
}

fn recovery_sensor_daily_metric_values(
    metric_id: &str,
    local_value: f64,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match metric_id {
        "hrv_rmssd_ms" => (Some(local_value), None, None, None),
        "respiratory_rate_rpm" => (None, Some(local_value), None, None),
        "oxygen_saturation_percent" => (None, None, Some(local_value), None),
        "skin_temperature_delta_c" => (None, None, None, Some(local_value)),
        _ => (None, None, None, None),
    }
}

fn recovery_sensor_daily_quality_flags(
    widget: &RecoverySensorWidgetDiscovery,
    blocker_reasons: &[String],
    source_kind: &str,
) -> Vec<String> {
    widget
        .quality_flags
        .iter()
        .chain(blocker_reasons.iter())
        .cloned()
        .chain([
            "recovery_sensor_daily_rollup".to_string(),
            format!("source_kind_{source_kind}"),
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn unavailable_quality_flags(widget: &RecoverySensorWidgetDiscovery) -> Vec<String> {
    widget
        .quality_flags
        .iter()
        .chain(widget.blocker_reasons.iter())
        .cloned()
        .chain([
            "recovery_widget_unavailable".to_string(),
            "source_kind_unavailable".to_string(),
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn recovery_metric_name(metric_id: &str) -> &'static str {
    match metric_id {
        "hrv_rmssd_ms" => "hrv_rmssd",
        "respiratory_rate_rpm" => "respiratory_rate",
        "oxygen_saturation_percent" => "oxygen_saturation",
        "skin_temperature_delta_c" => "skin_temperature_delta",
        _ => "unknown_recovery_metric",
    }
}

fn recovery_metric_unit(metric_id: &str) -> &'static str {
    match metric_id {
        "hrv_rmssd_ms" => "ms",
        "respiratory_rate_rpm" => "rpm",
        "oxygen_saturation_percent" => "percent",
        "skin_temperature_delta_c" => "celsius_delta",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, Copy)]
struct RollingAverage {
    average_bpm: Option<f64>,
    sample_count: usize,
}

fn rolling_average(
    store: &GooseStore,
    end_time_unix_ms: i64,
    days: i64,
    current: Option<&CurrentRestingHeartRateMetric>,
) -> GooseResult<RollingAverage> {
    let window_start = end_time_unix_ms.saturating_sub(days * 86_400_000);
    let rows = store.daily_recovery_metrics_between(window_start, end_time_unix_ms)?;
    let mut values = rows
        .iter()
        .filter(|row| row.source_kind != "unavailable")
        .filter_map(|row| {
            row.resting_hr_bpm
                .map(|value| (row.daily_metric_id.as_str(), value))
        })
        .collect::<Vec<_>>();
    if let Some(current) = current
        && !values
            .iter()
            .any(|(metric_id, _)| *metric_id == current.metric_id)
    {
        values.push((current.metric_id.as_str(), current.resting_hr_bpm));
    }
    let sample_count = values.len();
    let average_bpm = if values.is_empty() {
        None
    } else {
        Some(values.iter().map(|(_, value)| *value).sum::<f64>() / values.len() as f64)
    };
    Ok(RollingAverage {
        average_bpm,
        sample_count,
    })
}

fn delta_from_average(value: Option<f64>, average: Option<f64>) -> Option<f64> {
    match (value, average) {
        (Some(value), Some(average)) => Some(value - average),
        _ => None,
    }
}

fn resting_heart_rate_confidence(
    sample_count: usize,
    min_sample_count: usize,
    trusted_metric_input: bool,
    source_signal_count: usize,
) -> f64 {
    let target_samples = min_sample_count.max(1) * 6;
    let sample_score = (sample_count as f64 / target_samples as f64).clamp(0.0, 1.0);
    let trust_adjustment = if trusted_metric_input { 0.08 } else { -0.08 };
    let source_adjustment = if source_signal_count > 1 { -0.03 } else { 0.0 };
    (0.60 + sample_score * 0.20 + trust_adjustment + source_adjustment).clamp(0.40, 0.88)
}

fn source_signals_from_provenance(provenance: &serde_json::Value) -> Vec<String> {
    provenance
        .get("source_signals")
        .and_then(serde_json::Value::as_array)
        .map(|signals| {
            signals
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|signals| !signals.is_empty())
        .or_else(|| {
            provenance
                .get("source_signal")
                .and_then(serde_json::Value::as_str)
                .map(|signal| vec![signal.to_string()])
        })
        .unwrap_or_default()
}

fn daily_recovery_metric_id(date_key: &str, timezone: &str) -> String {
    format!(
        "daily-recovery-rhr-{}-{}-device-sensor-v0",
        sanitize_id_part(date_key),
        sanitize_id_part(timezone)
    )
}

fn recovery_unavailable_metric_id(metric_id: &str, date_key: &str, timezone: &str) -> String {
    format!(
        "daily-recovery-{}-{}-{}-unavailable-v0",
        sanitize_id_part(metric_id),
        sanitize_id_part(date_key),
        sanitize_id_part(timezone)
    )
}

fn recovery_sensor_metric_id(metric_id: &str, date_key: &str, timezone: &str) -> String {
    format!(
        "daily-recovery-{}-{}-{}-device-sensor-v0",
        sanitize_id_part(metric_id),
        sanitize_id_part(date_key),
        sanitize_id_part(timezone)
    )
}

fn sanitize_id_part(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    sanitized.trim_matches('-').to_string()
}

fn validate_options(options: &RestingHeartRateDailyRollupOptions<'_>) -> GooseResult<()> {
    if options.date_key.trim().is_empty() {
        return Err(GooseError::message("date_key is required"));
    }
    if options.timezone.trim().is_empty() {
        return Err(GooseError::message("timezone is required"));
    }
    if options.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if options.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if options.min_sample_count == 0 {
        return Err(GooseError::message("min_sample_count must be at least 1"));
    }
    Ok(())
}

fn validate_recovery_unavailable_options(
    options: &RecoveryUnavailableDailyStatusOptions<'_>,
) -> GooseResult<()> {
    if options.date_key.trim().is_empty() {
        return Err(GooseError::message("date_key is required"));
    }
    if options.timezone.trim().is_empty() {
        return Err(GooseError::message("timezone is required"));
    }
    if options.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if options.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if options.min_rr_intervals_to_compute == 0 {
        return Err(GooseError::message(
            "min_rr_intervals_to_compute must be at least 1",
        ));
    }
    Ok(())
}

fn validate_recovery_sensor_daily_options(
    options: &RecoverySensorDailyRollupOptions<'_>,
) -> GooseResult<()> {
    if options.date_key.trim().is_empty() {
        return Err(GooseError::message("date_key is required"));
    }
    if options.timezone.trim().is_empty() {
        return Err(GooseError::message("timezone is required"));
    }
    if options.start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if options.end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if options.min_rr_intervals_to_compute == 0 {
        return Err(GooseError::message(
            "min_rr_intervals_to_compute must be at least 1",
        ));
    }
    Ok(())
}

fn validate_rhr_validation_options(
    options: &RestingHeartRateCaptureValidationOptions<'_>,
) -> GooseResult<()> {
    if !options.tolerance_bpm.is_finite() || options.tolerance_bpm < 0.0 {
        return Err(GooseError::message("tolerance_bpm must be nonnegative"));
    }
    if let Some(value) = options.official_whoop_resting_hr_bpm
        && (!value.is_finite() || value <= 0.0)
    {
        return Err(GooseError::message(
            "official_whoop_resting_hr_bpm must be positive",
        ));
    }
    Ok(())
}

struct RhrLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

fn compare_rhr_label(
    local_resting_hr_bpm: Option<f64>,
    label_resting_hr_bpm: Option<f64>,
    tolerance_bpm: f64,
) -> RhrLabelComparison {
    let Some(label_resting_hr_bpm) = label_resting_hr_bpm else {
        return RhrLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_resting_hr_bpm) = local_resting_hr_bpm else {
        return RhrLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_resting_hr_bpm - label_resting_hr_bpm;
    RhrLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance_bpm),
    }
}

fn round_1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(|left, right| left.total_cmp(right));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn rollup_next_actions(issues: &[String]) -> Vec<RestingHeartRateRollupNextAction> {
    let mut actions = Vec::new();
    if issues
        .iter()
        .any(|issue| issue == "insufficient_heart_rate_samples")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:daily-rollup".to_string(),
            reason: "insufficient_samples".to_string(),
            action: "Capture more packet-derived heart-rate samples in the selected day window before writing daily resting HR.".to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "no_resting_heart_rate_feature")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:daily-rollup".to_string(),
            reason: "no_heart_rate_features".to_string(),
            action: "Run a heart-rate feature report for the same window and verify decoded WHOOP HR packets are present.".to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "capture_correlation_report_not_passed")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:daily-rollup".to_string(),
            reason: "untrusted_capture_evidence".to_string(),
            action: "Use user-owned capture evidence or lower the trusted-evidence requirement for exploratory diagnostics only.".to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn rhr_validation_next_actions(issues: &[String]) -> Vec<RestingHeartRateRollupNextAction> {
    let mut actions = rollup_next_actions(issues);
    for issue in issues {
        if let Some(action) = official_label_policy_issue_action(issue) {
            actions.push(RestingHeartRateRollupNextAction {
                scope: "rhr:validation-label".to_string(),
                reason: issue.clone(),
                action: action.to_string(),
            });
        }
    }
    if issues
        .iter()
        .any(|issue| issue == "no_resting_hr_validation_label")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:validation-label".to_string(),
            reason: "no_resting_hr_validation_label".to_string(),
            action:
                "Record the official WHOOP app resting-HR value as a validation label before passing RHR validation."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "resting_hr_rollup_blocked" || issue == "local_resting_hr_missing")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:local-rollup".to_string(),
            reason: "local_resting_hr_missing".to_string(),
            action:
                "Capture enough packet-derived heart-rate samples for the selected rest window before comparing against labels."
                    .to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "resting_hr_label_delta_out_of_tolerance")
    {
        actions.push(RestingHeartRateRollupNextAction {
            scope: "rhr:validation-delta".to_string(),
            reason: "resting_hr_label_delta_out_of_tolerance".to_string(),
            action: "Keep the local RHR candidate in validation until more rest/overnight captures explain the label delta.".to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn parse_rfc3339_utc_unix_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    let date_time = value.strip_suffix('Z')?;
    let (date, time) = date_time.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let (time_main, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut time_parts = time_main.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if time_parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let millis = if fraction.is_empty() {
        0
    } else {
        let digits = fraction
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .take(3)
            .collect::<String>();
        if digits.is_empty() {
            0
        } else {
            format!("{digits:0<3}").parse::<i64>().ok()?
        }
    };

    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400
        + i64::from(hour) * 3_600
        + i64::from(minute) * 60
        + i64::from(second.min(59));
    Some(seconds * 1_000 + millis)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146_097 + i64::from(doe) - 719_468
}
