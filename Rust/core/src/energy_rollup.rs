use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GooseError, GooseResult,
    capture_correlation::DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
    metric_features::{
        MetricWindowFeatureOptions, MetricWindowFeatureReport,
        run_metric_window_feature_report_for_store,
    },
    store::{
        DailyActivityMetricInput, DailyActivityMetricRow, GooseStore, HourlyActivityMetricInput,
        HourlyActivityMetricRow, MetricProvenanceInput,
    },
    validation_labels::{
        OFFICIAL_WHOOP_LABEL_POLICY, official_label_policy_issue_action,
        official_label_policy_issues,
    },
};

pub const ENERGY_DAILY_ROLLUP_REPORT_SCHEMA: &str = "goose.energy-daily-rollup-report.v1";
pub const ENERGY_HOURLY_ROLLUP_REPORT_SCHEMA: &str = "goose.energy-hourly-rollup-report.v1";
pub const ENERGY_CAPTURE_VALIDATION_REPORT_SCHEMA: &str =
    "goose.energy-capture-validation-report.v1";
pub const ENERGY_UNAVAILABLE_DAILY_STATUS_REPORT_SCHEMA: &str =
    "goose.energy-unavailable-daily-status-report.v1";
pub const GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID: &str = "goose.energy.local_estimate.v0";
pub const GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION: &str = "0.1.0";
pub const GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_ID: &str = "goose.energy.unavailable_status.v0";
pub const GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_VERSION: &str = "0.1.0";
const STEP_CADENCE_SUPPORT_SOURCE_KIND: &str = "device_counter";

#[derive(Debug, Clone)]
pub struct EnergyDailyRollupOptions<'a> {
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub profile_weight_kg: Option<f64>,
    pub profile_age_years: Option<u32>,
    pub profile_sex: Option<&'a str>,
    pub resting_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub min_heart_rate_samples: usize,
    pub write_metric: bool,
}

#[derive(Debug, Clone)]
pub struct EnergyHourlyRollupOptions<'a> {
    pub date_key: &'a str,
    pub timezone: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub profile_weight_kg: Option<f64>,
    pub profile_age_years: Option<u32>,
    pub profile_sex: Option<&'a str>,
    pub resting_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub min_heart_rate_samples: usize,
    pub write_metric: bool,
}

impl Default for EnergyDailyRollupOptions<'_> {
    fn default() -> Self {
        Self {
            date_key: "",
            timezone: "",
            start: "0000",
            end: "9999",
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            profile_weight_kg: None,
            profile_age_years: None,
            profile_sex: None,
            resting_hr_bpm: None,
            max_hr_bpm: None,
            min_heart_rate_samples: 2,
            write_metric: false,
        }
    }
}

impl Default for EnergyHourlyRollupOptions<'_> {
    fn default() -> Self {
        Self {
            date_key: "",
            timezone: "",
            start: "0000",
            end: "9999",
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_trusted_evidence: false,
            profile_weight_kg: None,
            profile_age_years: None,
            profile_sex: None,
            resting_hr_bpm: None,
            max_hr_bpm: None,
            min_heart_rate_samples: 2,
            write_metric: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergyDailyRollupReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub covered_minutes: f64,
    pub requested_minutes: f64,
    pub coverage_fraction: f64,
    pub heart_rate_sample_count: usize,
    pub motion_sample_count: usize,
    pub average_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub resting_hr_bpm: Option<f64>,
    pub average_motion_intensity_0_to_1: Option<f64>,
    pub hr_zone_minutes: Vec<f64>,
    pub profile_weight_kg: Option<f64>,
    pub profile_age_years: Option<u32>,
    pub profile_sex: Option<String>,
    pub step_cadence_source_kind: String,
    pub step_metric_count: usize,
    pub step_count: Option<i64>,
    pub average_cadence_spm: Option<f64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub confidence: f64,
    pub daily_metric_id: Option<String>,
    pub daily_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
    pub quality_flags: Vec<String>,
    pub window_report: MetricWindowFeatureReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<EnergyRollupNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergyHourlyRollupReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub date_key: String,
    pub timezone: String,
    pub start: String,
    pub end: String,
    pub start_time_unix_ms: i64,
    pub end_time_unix_ms: i64,
    pub covered_minutes: f64,
    pub requested_minutes: f64,
    pub coverage_fraction: f64,
    pub heart_rate_sample_count: usize,
    pub motion_sample_count: usize,
    pub average_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub resting_hr_bpm: Option<f64>,
    pub average_motion_intensity_0_to_1: Option<f64>,
    pub hr_zone_minutes: Vec<f64>,
    pub profile_weight_kg: Option<f64>,
    pub profile_age_years: Option<u32>,
    pub profile_sex: Option<String>,
    pub step_cadence_source_kind: String,
    pub step_metric_count: usize,
    pub step_count: Option<i64>,
    pub average_cadence_spm: Option<f64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
    pub confidence: f64,
    pub hourly_metric_id: Option<String>,
    pub hourly_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
    pub quality_flags: Vec<String>,
    pub window_report: MetricWindowFeatureReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<EnergyRollupNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergyUnavailableDailyStatusReport {
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
    pub available_energy_metric_count: usize,
    pub unavailable_metric_count: usize,
    pub written_metric_count: usize,
    pub metric_provenance_written_count: usize,
    pub statuses: Vec<EnergyUnavailableMetricStatus>,
    pub energy_daily_rollup: EnergyDailyRollupReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<EnergyRollupNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergyUnavailableMetricStatus {
    pub metric_id: String,
    pub metric_name: String,
    pub source_kind: String,
    pub promotion_status: String,
    pub available_metric_count: usize,
    pub heart_rate_sample_count: usize,
    pub motion_sample_count: usize,
    pub min_heart_rate_samples: usize,
    pub coverage_fraction: f64,
    pub blocker_reasons: Vec<String>,
    pub quality_flags: Vec<String>,
    pub daily_metric_id: Option<String>,
    pub daily_metric_written: bool,
    pub metric_provenance_id: Option<String>,
    pub metric_provenance_written: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnergyRollupNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct EnergyCaptureValidationOptions<'a> {
    pub rollup_options: EnergyDailyRollupOptions<'a>,
    pub capture_kind: Option<String>,
    pub official_whoop_active_kcal: Option<f64>,
    pub official_whoop_resting_kcal: Option<f64>,
    pub official_whoop_total_kcal: Option<f64>,
    pub tolerance_kcal: f64,
    pub relative_tolerance_fraction: f64,
    pub label_provenance: Option<Value>,
}

#[derive(Debug, Clone, Default)]
struct StepCadenceSupport {
    metric_ids: Vec<String>,
    metric_count: usize,
    step_count: Option<i64>,
    average_cadence_spm: Option<f64>,
}

impl StepCadenceSupport {
    fn source_kind(&self) -> &'static str {
        if self.metric_count > 0 {
            STEP_CADENCE_SUPPORT_SOURCE_KIND
        } else {
            "unavailable"
        }
    }

    fn has_device_support(&self) -> bool {
        self.metric_count > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergyCaptureValidationReport {
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
    pub official_whoop_active_kcal: Option<f64>,
    pub official_whoop_resting_kcal: Option<f64>,
    pub official_whoop_total_kcal: Option<f64>,
    pub tolerance_kcal: f64,
    pub relative_tolerance_fraction: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub local_active_kcal: Option<f64>,
    pub local_resting_kcal: Option<f64>,
    pub local_total_kcal: Option<f64>,
    pub active_kcal_error: Option<f64>,
    pub resting_kcal_error: Option<f64>,
    pub total_kcal_error: Option<f64>,
    pub active_kcal_within_tolerance: Option<bool>,
    pub resting_kcal_within_tolerance: Option<bool>,
    pub total_kcal_within_tolerance: Option<bool>,
    pub provided_label_count: usize,
    pub matching_label_count: usize,
    pub confidence: f64,
    pub heart_rate_sample_count: usize,
    pub motion_sample_count: usize,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub energy_rollup: EnergyDailyRollupReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<EnergyRollupNextAction>,
}

pub fn rollup_energy_day_for_store(
    store: &GooseStore,
    database_path: &str,
    options: EnergyDailyRollupOptions<'_>,
) -> GooseResult<EnergyDailyRollupReport> {
    validate_options(&options)?;
    let start_time_unix_ms = parse_rfc3339_utc_unix_ms(options.start)
        .ok_or_else(|| GooseError::message("start must be an RFC3339 UTC timestamp"))?;
    let end_time_unix_ms = parse_rfc3339_utc_unix_ms(options.end)
        .ok_or_else(|| GooseError::message("end must be an RFC3339 UTC timestamp"))?;
    if end_time_unix_ms <= start_time_unix_ms {
        return Err(GooseError::message("end must be after start"));
    }
    let requested_minutes = (end_time_unix_ms - start_time_unix_ms) as f64 / 60_000.0;

    let window_report = run_metric_window_feature_report_for_store(
        store,
        database_path,
        options.start,
        options.end,
        MetricWindowFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            resting_hr_bpm: options.resting_hr_bpm,
            max_hr_bpm: options.max_hr_bpm,
        },
    )?;
    let window = window_report.window.as_ref();

    let mut issues = window_report.issues.clone();
    if window.is_none() {
        issues.push("no_energy_metric_window".to_string());
    }
    if window
        .map(|window| window.heart_rate_sample_count < options.min_heart_rate_samples)
        .unwrap_or(true)
    {
        issues.push("insufficient_heart_rate_samples".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = window
        .map(|window| window.quality_flags.clone())
        .unwrap_or_default();
    quality_flags.push("local_energy_estimate".to_string());
    quality_flags.push("not_whoop_backend".to_string());

    let effective_weight_kg = match options.profile_weight_kg {
        Some(weight) => weight,
        None => {
            quality_flags.push("profile_weight_missing_default_70kg_used".to_string());
            70.0
        }
    };
    if options.profile_age_years.is_none() {
        quality_flags.push("profile_age_missing".to_string());
    }
    if options.profile_sex.is_none() {
        quality_flags.push("profile_sex_missing".to_string());
    }

    let (
        covered_minutes,
        heart_rate_sample_count,
        motion_sample_count,
        average_hr_bpm,
        max_hr_bpm,
        average_motion_intensity_0_to_1,
        hr_zone_minutes,
    ) = if let Some(window) = window {
        (
            window.duration_minutes,
            window.heart_rate_sample_count,
            window.motion_sample_count,
            Some(window.average_hr_bpm),
            Some(window.max_hr_bpm),
            window.average_motion_intensity_0_to_1,
            window.hr_zone_minutes.clone(),
        )
    } else {
        (0.0, 0, 0, None, None, None, Vec::new())
    };
    if requested_minutes > 0.0 && covered_minutes / requested_minutes < 0.25 {
        quality_flags.push("partial_day_coverage".to_string());
    }
    if hr_zone_minutes.is_empty() {
        quality_flags.push("hr_zone_basis_missing".to_string());
    }
    let step_cadence_support = daily_step_cadence_support(
        store,
        options.date_key,
        options.timezone,
        start_time_unix_ms,
        end_time_unix_ms,
    )?;
    if step_cadence_support.has_device_support() {
        quality_flags.push("device_counter_step_cadence_support".to_string());
    }
    quality_flags.sort();
    quality_flags.dedup();

    let pass = issues.is_empty();
    let (active_kcal, resting_kcal, total_kcal) = if pass {
        let resting = resting_kcal(effective_weight_kg, covered_minutes);
        let active = active_kcal(
            effective_weight_kg,
            covered_minutes,
            &hr_zone_minutes,
            average_hr_bpm,
            options.resting_hr_bpm,
            options.max_hr_bpm,
            average_motion_intensity_0_to_1,
        );
        let total = resting + active;
        (
            Some(round_1(active)),
            Some(round_1(resting)),
            Some(round_1(total)),
        )
    } else {
        (None, None, None)
    };
    let confidence = if pass {
        energy_confidence(
            options.profile_weight_kg.is_some(),
            options.profile_age_years.is_some(),
            options.profile_sex.is_some(),
            heart_rate_sample_count,
            options.min_heart_rate_samples,
            motion_sample_count,
            step_cadence_support.has_device_support(),
            covered_minutes,
            requested_minutes,
        )
    } else {
        0.0
    };

    let metric_id = daily_activity_metric_id(options.date_key, options.timezone);
    let provenance_id = format!("prov-{metric_id}");
    let mut daily_metric_written = false;
    let mut metric_provenance_written = false;

    if pass && options.write_metric {
        let inputs_json = json!({
            "profile_weight_kg": options.profile_weight_kg,
            "effective_weight_kg": effective_weight_kg,
            "profile_age_years": options.profile_age_years,
            "profile_sex": options.profile_sex,
            "covered_minutes": covered_minutes,
            "requested_minutes": requested_minutes,
            "heart_rate_sample_count": heart_rate_sample_count,
            "motion_sample_count": motion_sample_count,
            "average_hr_bpm": average_hr_bpm,
            "resting_hr_bpm": options.resting_hr_bpm,
            "max_hr_bpm": options.max_hr_bpm,
            "average_motion_intensity_0_to_1": average_motion_intensity_0_to_1,
            "hr_zone_minutes": hr_zone_minutes,
            "step_cadence_support": step_cadence_support_json(&step_cadence_support),
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!("cannot serialize energy quality flags: {error}"))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID,
            "algorithm_version": GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION,
            "source_kind": "local_estimate",
            "date_key": options.date_key,
            "timezone": options.timezone,
            "start": options.start,
            "end": options.end,
            "start_time_unix_ms": start_time_unix_ms,
            "end_time_unix_ms": end_time_unix_ms,
            "formula": "resting_kcal_from_weight_scaled_rmr + active_kcal_from_hr_reserve_zones_and_motion",
            "step_cadence_support": step_cadence_support_json(&step_cadence_support),
            "official_labels_policy": "not_used",
        })
        .to_string();

        daily_metric_written = store.upsert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: &metric_id,
            date_key: options.date_key,
            timezone: options.timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            steps: None,
            active_kcal,
            resting_kcal,
            total_kcal,
            average_cadence_spm: None,
            source_kind: "local_estimate",
            confidence,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "daily_activity",
            metric_id: &metric_id,
            source_kind: "local_estimate",
            source_detail: "packet HR/motion local energy estimate",
            confidence: Some(confidence),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    let coverage_fraction = if requested_minutes > 0.0 {
        (covered_minutes / requested_minutes).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Ok(EnergyDailyRollupReport {
        schema: ENERGY_DAILY_ROLLUP_REPORT_SCHEMA.to_string(),
        generated_by: "goose-energy-daily-rollup".to_string(),
        pass,
        date_key: options.date_key.to_string(),
        timezone: options.timezone.to_string(),
        start: options.start.to_string(),
        end: options.end.to_string(),
        start_time_unix_ms,
        end_time_unix_ms,
        covered_minutes,
        requested_minutes,
        coverage_fraction,
        heart_rate_sample_count,
        motion_sample_count,
        average_hr_bpm,
        max_hr_bpm,
        resting_hr_bpm: options.resting_hr_bpm,
        average_motion_intensity_0_to_1,
        hr_zone_minutes,
        profile_weight_kg: options.profile_weight_kg,
        profile_age_years: options.profile_age_years,
        profile_sex: options.profile_sex.map(str::to_string),
        step_cadence_source_kind: step_cadence_support.source_kind().to_string(),
        step_metric_count: step_cadence_support.metric_count,
        step_count: step_cadence_support.step_count,
        average_cadence_spm: step_cadence_support.average_cadence_spm,
        active_kcal,
        resting_kcal,
        total_kcal,
        confidence,
        daily_metric_id: (pass && options.write_metric).then_some(metric_id),
        daily_metric_written,
        metric_provenance_id: (pass && options.write_metric).then_some(provenance_id),
        metric_provenance_written,
        quality_flags,
        window_report,
        issues: issues.clone(),
        next_actions: rollup_next_actions(&issues),
    })
}

pub fn rollup_energy_unavailable_daily_status_for_store(
    store: &GooseStore,
    database_path: &str,
    mut options: EnergyDailyRollupOptions<'_>,
) -> GooseResult<EnergyUnavailableDailyStatusReport> {
    let requested_write_metric = options.write_metric;
    let min_heart_rate_samples = options.min_heart_rate_samples;
    options.write_metric = false;
    let energy_daily_rollup = rollup_energy_day_for_store(store, database_path, options)?;

    let mut statuses = Vec::new();
    let mut available_energy_metric_count = 0usize;
    for metric_id in ENERGY_UNAVAILABLE_METRIC_IDS {
        let available_metric_count = available_energy_metric_count_for_metric(
            store,
            &energy_daily_rollup.date_key,
            &energy_daily_rollup.timezone,
            energy_daily_rollup.start_time_unix_ms,
            energy_daily_rollup.end_time_unix_ms,
            metric_id,
        )?;
        available_energy_metric_count += available_metric_count;
        if available_metric_count == 0 && !energy_daily_rollup.pass {
            statuses.push(energy_unavailable_metric_status_for_rollup(
                store,
                &energy_daily_rollup,
                metric_id,
                available_metric_count,
                requested_write_metric,
                min_heart_rate_samples,
            )?);
        }
    }

    let written_metric_count = statuses
        .iter()
        .filter(|status| status.daily_metric_written)
        .count();
    let metric_provenance_written_count = statuses
        .iter()
        .filter(|status| status.metric_provenance_written)
        .count();
    let next_actions = if statuses.is_empty() {
        Vec::new()
    } else {
        energy_daily_rollup.next_actions.clone()
    };

    Ok(EnergyUnavailableDailyStatusReport {
        schema: ENERGY_UNAVAILABLE_DAILY_STATUS_REPORT_SCHEMA.to_string(),
        generated_by: "goose-energy-unavailable-daily-status".to_string(),
        pass: true,
        date_key: energy_daily_rollup.date_key.clone(),
        timezone: energy_daily_rollup.timezone.clone(),
        start: energy_daily_rollup.start.clone(),
        end: energy_daily_rollup.end.clone(),
        start_time_unix_ms: energy_daily_rollup.start_time_unix_ms,
        end_time_unix_ms: energy_daily_rollup.end_time_unix_ms,
        write_metric: requested_write_metric,
        available_energy_metric_count,
        unavailable_metric_count: statuses.len(),
        written_metric_count,
        metric_provenance_written_count,
        statuses,
        energy_daily_rollup,
        issues: Vec::new(),
        next_actions,
    })
}

fn energy_unavailable_metric_status_for_rollup(
    store: &GooseStore,
    rollup: &EnergyDailyRollupReport,
    metric_id: &str,
    available_metric_count: usize,
    write_metric: bool,
    min_heart_rate_samples: usize,
) -> GooseResult<EnergyUnavailableMetricStatus> {
    let metric_name = energy_metric_name(metric_id);
    let daily_metric_id =
        energy_unavailable_metric_id(metric_id, &rollup.date_key, &rollup.timezone);
    let provenance_id = format!("prov-{daily_metric_id}");
    let blocker_reasons = unavailable_energy_blocker_reasons(rollup);
    let quality_flags = unavailable_energy_quality_flags(rollup, metric_id, &blocker_reasons);
    let mut daily_metric_written = false;
    let mut metric_provenance_written = false;

    if write_metric {
        let inputs_json = json!({
            "metric_id": metric_id,
            "metric_name": metric_name,
            "energy_daily_rollup_schema": rollup.schema,
            "date_key": rollup.date_key,
            "timezone": rollup.timezone,
            "start": rollup.start,
            "end": rollup.end,
            "start_time_unix_ms": rollup.start_time_unix_ms,
            "end_time_unix_ms": rollup.end_time_unix_ms,
            "covered_minutes": rollup.covered_minutes,
            "requested_minutes": rollup.requested_minutes,
            "coverage_fraction": rollup.coverage_fraction,
            "heart_rate_sample_count": rollup.heart_rate_sample_count,
            "motion_sample_count": rollup.motion_sample_count,
            "min_heart_rate_samples": min_heart_rate_samples,
            "average_hr_bpm": rollup.average_hr_bpm,
            "resting_hr_bpm": rollup.resting_hr_bpm,
            "max_hr_bpm": rollup.max_hr_bpm,
            "average_motion_intensity_0_to_1": rollup.average_motion_intensity_0_to_1,
            "profile_weight_kg": rollup.profile_weight_kg,
            "profile_age_years": rollup.profile_age_years,
            "profile_sex": rollup.profile_sex,
            "available_metric_count": available_metric_count,
            "blocker_reasons": blocker_reasons,
            "rollup_next_actions": rollup.next_actions,
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!(
                "cannot serialize energy unavailable quality flags: {error}"
            ))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_ID,
            "algorithm_version": GOOSE_ENERGY_UNAVAILABLE_STATUS_V0_VERSION,
            "source_kind": "unavailable",
            "metric_id": metric_id,
            "metric_name": metric_name,
            "date_key": rollup.date_key,
            "timezone": rollup.timezone,
            "start": rollup.start,
            "end": rollup.end,
            "start_time_unix_ms": rollup.start_time_unix_ms,
            "end_time_unix_ms": rollup.end_time_unix_ms,
            "promotion_status": "blocked",
            "promotion_allowed": false,
            "user_visible_value_allowed": false,
            "blocker_reasons": blocker_reasons,
            "energy_daily_rollup_schema": rollup.schema,
            "official_labels_policy": "not_used",
            "value_policy": "no_calorie_value_written_until_whoop_packet_hr_motion_inputs_support_local_estimate",
        })
        .to_string();

        daily_metric_written = store.upsert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: &daily_metric_id,
            date_key: &rollup.date_key,
            timezone: &rollup.timezone,
            start_time_unix_ms: rollup.start_time_unix_ms,
            end_time_unix_ms: rollup.end_time_unix_ms,
            steps: None,
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "unavailable",
            confidence: 0.0,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "daily_activity",
            metric_id: &daily_metric_id,
            source_kind: "unavailable",
            source_detail: "activity calories blocked by local WHOOP packet promotion gate",
            confidence: Some(0.0),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    Ok(EnergyUnavailableMetricStatus {
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        source_kind: "unavailable".to_string(),
        promotion_status: "blocked".to_string(),
        available_metric_count,
        heart_rate_sample_count: rollup.heart_rate_sample_count,
        motion_sample_count: rollup.motion_sample_count,
        min_heart_rate_samples,
        coverage_fraction: rollup.coverage_fraction,
        blocker_reasons,
        quality_flags,
        daily_metric_id: write_metric.then_some(daily_metric_id),
        daily_metric_written,
        metric_provenance_id: write_metric.then_some(provenance_id),
        metric_provenance_written,
    })
}

pub fn rollup_energy_hour_for_store(
    store: &GooseStore,
    database_path: &str,
    options: EnergyHourlyRollupOptions<'_>,
) -> GooseResult<EnergyHourlyRollupReport> {
    validate_hourly_options(&options)?;
    let start_time_unix_ms = parse_rfc3339_utc_unix_ms(options.start)
        .ok_or_else(|| GooseError::message("start must be an RFC3339 UTC timestamp"))?;
    let end_time_unix_ms = parse_rfc3339_utc_unix_ms(options.end)
        .ok_or_else(|| GooseError::message("end must be an RFC3339 UTC timestamp"))?;
    if end_time_unix_ms <= start_time_unix_ms {
        return Err(GooseError::message("end must be after start"));
    }
    let requested_minutes = (end_time_unix_ms - start_time_unix_ms) as f64 / 60_000.0;

    let window_report = run_metric_window_feature_report_for_store(
        store,
        database_path,
        options.start,
        options.end,
        MetricWindowFeatureOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            resting_hr_bpm: options.resting_hr_bpm,
            max_hr_bpm: options.max_hr_bpm,
        },
    )?;
    let window = window_report.window.as_ref();

    let mut issues = window_report.issues.clone();
    if window.is_none() {
        issues.push("no_energy_metric_window".to_string());
    }
    if window
        .map(|window| window.heart_rate_sample_count < options.min_heart_rate_samples)
        .unwrap_or(true)
    {
        issues.push("insufficient_heart_rate_samples".to_string());
    }
    issues.sort();
    issues.dedup();

    let mut quality_flags = window
        .map(|window| window.quality_flags.clone())
        .unwrap_or_default();
    quality_flags.push("hourly_energy_rollup".to_string());
    quality_flags.push("local_energy_estimate".to_string());
    quality_flags.push("not_whoop_backend".to_string());

    let effective_weight_kg = match options.profile_weight_kg {
        Some(weight) => weight,
        None => {
            quality_flags.push("profile_weight_missing_default_70kg_used".to_string());
            70.0
        }
    };
    if options.profile_age_years.is_none() {
        quality_flags.push("profile_age_missing".to_string());
    }
    if options.profile_sex.is_none() {
        quality_flags.push("profile_sex_missing".to_string());
    }

    let (
        covered_minutes,
        heart_rate_sample_count,
        motion_sample_count,
        average_hr_bpm,
        max_hr_bpm,
        average_motion_intensity_0_to_1,
        hr_zone_minutes,
    ) = if let Some(window) = window {
        (
            window.duration_minutes,
            window.heart_rate_sample_count,
            window.motion_sample_count,
            Some(window.average_hr_bpm),
            Some(window.max_hr_bpm),
            window.average_motion_intensity_0_to_1,
            window.hr_zone_minutes.clone(),
        )
    } else {
        (0.0, 0, 0, None, None, None, Vec::new())
    };
    if requested_minutes > 0.0 && covered_minutes / requested_minutes < 0.25 {
        quality_flags.push("partial_day_coverage".to_string());
    }
    if hr_zone_minutes.is_empty() {
        quality_flags.push("hr_zone_basis_missing".to_string());
    }
    let step_cadence_support = hourly_step_cadence_support(
        store,
        options.date_key,
        options.timezone,
        start_time_unix_ms,
        end_time_unix_ms,
    )?;
    if step_cadence_support.has_device_support() {
        quality_flags.push("device_counter_step_cadence_support".to_string());
    }
    quality_flags.sort();
    quality_flags.dedup();

    let pass = issues.is_empty();
    let (active_kcal, resting_kcal, total_kcal) = if pass {
        let resting = resting_kcal(effective_weight_kg, covered_minutes);
        let active = active_kcal(
            effective_weight_kg,
            covered_minutes,
            &hr_zone_minutes,
            average_hr_bpm,
            options.resting_hr_bpm,
            options.max_hr_bpm,
            average_motion_intensity_0_to_1,
        );
        let total = resting + active;
        (
            Some(round_1(active)),
            Some(round_1(resting)),
            Some(round_1(total)),
        )
    } else {
        (None, None, None)
    };
    let confidence = if pass {
        energy_confidence(
            options.profile_weight_kg.is_some(),
            options.profile_age_years.is_some(),
            options.profile_sex.is_some(),
            heart_rate_sample_count,
            options.min_heart_rate_samples,
            motion_sample_count,
            step_cadence_support.has_device_support(),
            covered_minutes,
            requested_minutes,
        )
    } else {
        0.0
    };

    let metric_id = hourly_activity_metric_id(
        options.date_key,
        options.timezone,
        start_time_unix_ms,
        end_time_unix_ms,
    );
    let provenance_id = format!("prov-{metric_id}");
    let mut hourly_metric_written = false;
    let mut metric_provenance_written = false;

    if pass && options.write_metric {
        let inputs_json = json!({
            "profile_weight_kg": options.profile_weight_kg,
            "effective_weight_kg": effective_weight_kg,
            "profile_age_years": options.profile_age_years,
            "profile_sex": options.profile_sex,
            "covered_minutes": covered_minutes,
            "requested_minutes": requested_minutes,
            "heart_rate_sample_count": heart_rate_sample_count,
            "motion_sample_count": motion_sample_count,
            "average_hr_bpm": average_hr_bpm,
            "resting_hr_bpm": options.resting_hr_bpm,
            "max_hr_bpm": options.max_hr_bpm,
            "average_motion_intensity_0_to_1": average_motion_intensity_0_to_1,
            "hr_zone_minutes": hr_zone_minutes,
            "step_cadence_support": step_cadence_support_json(&step_cadence_support),
        })
        .to_string();
        let quality_flags_json = serde_json::to_string(&quality_flags).map_err(|error| {
            GooseError::message(format!("cannot serialize energy quality flags: {error}"))
        })?;
        let provenance_json = json!({
            "algorithm": GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID,
            "algorithm_version": GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION,
            "source_kind": "local_estimate",
            "date_key": options.date_key,
            "timezone": options.timezone,
            "start": options.start,
            "end": options.end,
            "start_time_unix_ms": start_time_unix_ms,
            "end_time_unix_ms": end_time_unix_ms,
            "rollup_kind": "hourly_activity",
            "formula": "resting_kcal_from_weight_scaled_rmr + active_kcal_from_hr_reserve_zones_and_motion",
            "step_cadence_support": step_cadence_support_json(&step_cadence_support),
            "official_labels_policy": "not_used",
        })
        .to_string();

        hourly_metric_written = store.upsert_hourly_activity_metric(HourlyActivityMetricInput {
            hourly_metric_id: &metric_id,
            date_key: options.date_key,
            timezone: options.timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            steps: None,
            active_kcal,
            resting_kcal,
            total_kcal,
            average_cadence_spm: None,
            source_kind: "local_estimate",
            confidence,
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;

        metric_provenance_written = store.upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: &provenance_id,
            metric_scope: "hourly_activity",
            metric_id: &metric_id,
            source_kind: "local_estimate",
            source_detail: "packet HR/motion local hourly energy estimate",
            confidence: Some(confidence),
            inputs_json: &inputs_json,
            quality_flags_json: &quality_flags_json,
            provenance_json: &provenance_json,
        })?;
    }

    let coverage_fraction = if requested_minutes > 0.0 {
        (covered_minutes / requested_minutes).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Ok(EnergyHourlyRollupReport {
        schema: ENERGY_HOURLY_ROLLUP_REPORT_SCHEMA.to_string(),
        generated_by: "goose-energy-hourly-rollup".to_string(),
        pass,
        date_key: options.date_key.to_string(),
        timezone: options.timezone.to_string(),
        start: options.start.to_string(),
        end: options.end.to_string(),
        start_time_unix_ms,
        end_time_unix_ms,
        covered_minutes,
        requested_minutes,
        coverage_fraction,
        heart_rate_sample_count,
        motion_sample_count,
        average_hr_bpm,
        max_hr_bpm,
        resting_hr_bpm: options.resting_hr_bpm,
        average_motion_intensity_0_to_1,
        hr_zone_minutes,
        profile_weight_kg: options.profile_weight_kg,
        profile_age_years: options.profile_age_years,
        profile_sex: options.profile_sex.map(str::to_string),
        step_cadence_source_kind: step_cadence_support.source_kind().to_string(),
        step_metric_count: step_cadence_support.metric_count,
        step_count: step_cadence_support.step_count,
        average_cadence_spm: step_cadence_support.average_cadence_spm,
        active_kcal,
        resting_kcal,
        total_kcal,
        confidence,
        hourly_metric_id: (pass && options.write_metric).then_some(metric_id),
        hourly_metric_written,
        metric_provenance_id: (pass && options.write_metric).then_some(provenance_id),
        metric_provenance_written,
        quality_flags,
        window_report,
        issues: issues.clone(),
        next_actions: rollup_next_actions(&issues),
    })
}

pub fn validate_energy_capture_for_store(
    store: &GooseStore,
    database_path: &str,
    options: EnergyCaptureValidationOptions<'_>,
) -> GooseResult<EnergyCaptureValidationReport> {
    validate_energy_validation_options(&options)?;
    let mut rollup_options = options.rollup_options.clone();
    rollup_options.write_metric = false;
    let energy_rollup = rollup_energy_day_for_store(store, database_path, rollup_options)?;

    let active = compare_energy_label(
        energy_rollup.active_kcal,
        options.official_whoop_active_kcal,
        options.tolerance_kcal,
        options.relative_tolerance_fraction,
    );
    let resting = compare_energy_label(
        energy_rollup.resting_kcal,
        options.official_whoop_resting_kcal,
        options.tolerance_kcal,
        options.relative_tolerance_fraction,
    );
    let total = compare_energy_label(
        energy_rollup.total_kcal,
        options.official_whoop_total_kcal,
        options.tolerance_kcal,
        options.relative_tolerance_fraction,
    );

    let provided_label_count = [
        options.official_whoop_active_kcal,
        options.official_whoop_resting_kcal,
        options.official_whoop_total_kcal,
    ]
    .into_iter()
    .flatten()
    .count();
    let matching_label_count = [
        active.within_tolerance,
        resting.within_tolerance,
        total.within_tolerance,
    ]
    .into_iter()
    .flatten()
    .filter(|matches| *matches)
    .count();

    let mut issues = Vec::new();
    if provided_label_count == 0 {
        issues.push("no_energy_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        provided_label_count > 0,
        options.label_provenance.as_ref(),
    ));
    if !energy_rollup.pass {
        issues.push("energy_rollup_blocked".to_string());
        for issue in &energy_rollup.issues {
            issues.push(format!("energy_rollup_issue:{issue}"));
        }
    }
    push_energy_label_issue(
        &mut issues,
        "active_kcal",
        energy_rollup.active_kcal,
        options.official_whoop_active_kcal,
        active.within_tolerance,
    );
    push_energy_label_issue(
        &mut issues,
        "resting_kcal",
        energy_rollup.resting_kcal,
        options.official_whoop_resting_kcal,
        resting.within_tolerance,
    );
    push_energy_label_issue(
        &mut issues,
        "total_kcal",
        energy_rollup.total_kcal,
        options.official_whoop_total_kcal,
        total.within_tolerance,
    );
    issues.sort();
    issues.dedup();

    Ok(EnergyCaptureValidationReport {
        schema: ENERGY_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-energy-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        date_key: energy_rollup.date_key.clone(),
        timezone: energy_rollup.timezone.clone(),
        start: energy_rollup.start.clone(),
        end: energy_rollup.end.clone(),
        capture_kind: options.capture_kind,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        official_whoop_active_kcal: options.official_whoop_active_kcal,
        official_whoop_resting_kcal: options.official_whoop_resting_kcal,
        official_whoop_total_kcal: options.official_whoop_total_kcal,
        tolerance_kcal: options.tolerance_kcal,
        relative_tolerance_fraction: options.relative_tolerance_fraction,
        label_provenance: options.label_provenance,
        local_active_kcal: energy_rollup.active_kcal,
        local_resting_kcal: energy_rollup.resting_kcal,
        local_total_kcal: energy_rollup.total_kcal,
        active_kcal_error: active.error,
        resting_kcal_error: resting.error,
        total_kcal_error: total.error,
        active_kcal_within_tolerance: active.within_tolerance,
        resting_kcal_within_tolerance: resting.within_tolerance,
        total_kcal_within_tolerance: total.within_tolerance,
        provided_label_count,
        matching_label_count,
        confidence: energy_rollup.confidence,
        heart_rate_sample_count: energy_rollup.heart_rate_sample_count,
        motion_sample_count: energy_rollup.motion_sample_count,
        algorithm_id: GOOSE_ENERGY_LOCAL_ESTIMATE_V0_ID.to_string(),
        algorithm_version: GOOSE_ENERGY_LOCAL_ESTIMATE_V0_VERSION.to_string(),
        energy_rollup,
        next_actions: energy_validation_next_actions(&issues),
        issues,
    })
}

fn resting_kcal(weight_kg: f64, minutes: f64) -> f64 {
    let rmr_kcal_per_day = weight_kg * 22.0;
    rmr_kcal_per_day * minutes.max(0.0) / 1440.0
}

fn active_kcal(
    weight_kg: f64,
    covered_minutes: f64,
    hr_zone_minutes: &[f64],
    average_hr_bpm: Option<f64>,
    resting_hr_bpm: Option<f64>,
    max_hr_bpm: Option<f64>,
    average_motion_intensity_0_to_1: Option<f64>,
) -> f64 {
    let kcal_per_met_minute = 3.5 * weight_kg / 200.0;
    let zone_active_met_minutes = if hr_zone_minutes.len() == 5 {
        let active_met_by_zone = [0.0, 1.0, 2.5, 5.0, 8.0];
        hr_zone_minutes
            .iter()
            .zip(active_met_by_zone.iter())
            .map(|(minutes, active_met)| minutes.max(0.0) * active_met)
            .sum::<f64>()
    } else {
        0.0
    };
    let reserve_active_met_minutes = match (average_hr_bpm, resting_hr_bpm, max_hr_bpm) {
        (Some(average_hr_bpm), Some(resting_hr_bpm), Some(max_hr_bpm))
            if max_hr_bpm > resting_hr_bpm =>
        {
            let reserve_fraction =
                ((average_hr_bpm - resting_hr_bpm) / (max_hr_bpm - resting_hr_bpm)).clamp(0.0, 1.0);
            let active_met = 7.0 * reserve_fraction.powf(1.35);
            active_met * covered_minutes.max(0.0)
        }
        _ => 0.0,
    };
    let motion_active_met_minutes = average_motion_intensity_0_to_1
        .map(|intensity| intensity.clamp(0.0, 1.0) * 2.0 * covered_minutes.max(0.0))
        .unwrap_or(0.0);
    zone_active_met_minutes
        .max(reserve_active_met_minutes)
        .max(motion_active_met_minutes)
        * kcal_per_met_minute
}

fn energy_confidence(
    has_weight: bool,
    has_age: bool,
    has_sex: bool,
    heart_rate_sample_count: usize,
    min_heart_rate_samples: usize,
    motion_sample_count: usize,
    has_device_step_cadence_support: bool,
    covered_minutes: f64,
    requested_minutes: f64,
) -> f64 {
    let profile_score = if has_weight { 0.20 } else { 0.04 }
        + if has_age { 0.04 } else { 0.0 }
        + if has_sex { 0.04 } else { 0.0 };
    let hr_score = (heart_rate_sample_count as f64 / (min_heart_rate_samples.max(1) * 6) as f64)
        .clamp(0.0, 1.0)
        * 0.28;
    let motion_score = if motion_sample_count > 0 { 0.12 } else { 0.0 };
    let step_cadence_score = if has_device_step_cadence_support {
        0.04
    } else {
        0.0
    };
    let coverage_score = if requested_minutes > 0.0 {
        (covered_minutes / requested_minutes).clamp(0.0, 1.0) * 0.18
    } else {
        0.0
    };
    (0.18 + profile_score + hr_score + motion_score + step_cadence_score + coverage_score)
        .clamp(0.20, 0.90)
}

fn daily_step_cadence_support(
    store: &GooseStore,
    date_key: &str,
    timezone: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) -> GooseResult<StepCadenceSupport> {
    let rows = store.daily_activity_metrics_between(start_time_unix_ms, end_time_unix_ms)?;
    Ok(step_cadence_support_from_daily_rows(
        rows.iter()
            .filter(|row| row.date_key == date_key)
            .filter(|row| row.timezone == timezone)
            .filter(|row| row.source_kind == STEP_CADENCE_SUPPORT_SOURCE_KIND),
    ))
}

fn hourly_step_cadence_support(
    store: &GooseStore,
    date_key: &str,
    timezone: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) -> GooseResult<StepCadenceSupport> {
    let rows = store.hourly_activity_metrics_between(start_time_unix_ms, end_time_unix_ms)?;
    Ok(step_cadence_support_from_hourly_rows(
        rows.iter()
            .filter(|row| row.date_key == date_key)
            .filter(|row| row.timezone == timezone)
            .filter(|row| row.source_kind == STEP_CADENCE_SUPPORT_SOURCE_KIND),
    ))
}

fn step_cadence_support_from_daily_rows<'a>(
    rows: impl Iterator<Item = &'a DailyActivityMetricRow>,
) -> StepCadenceSupport {
    step_cadence_support_from_parts(rows.map(|row| {
        (
            row.daily_metric_id.as_str(),
            row.steps,
            row.average_cadence_spm,
        )
    }))
}

fn step_cadence_support_from_hourly_rows<'a>(
    rows: impl Iterator<Item = &'a HourlyActivityMetricRow>,
) -> StepCadenceSupport {
    step_cadence_support_from_parts(rows.map(|row| {
        (
            row.hourly_metric_id.as_str(),
            row.steps,
            row.average_cadence_spm,
        )
    }))
}

fn step_cadence_support_from_parts<'a>(
    parts: impl Iterator<Item = (&'a str, Option<i64>, Option<f64>)>,
) -> StepCadenceSupport {
    let mut support = StepCadenceSupport::default();
    let mut cadence_sum = 0.0;
    let mut cadence_count = 0usize;
    let mut step_sum = 0i64;
    let mut step_count = 0usize;

    for (metric_id, steps, cadence) in parts {
        if steps.is_none() && cadence.is_none() {
            continue;
        }
        support.metric_count += 1;
        support.metric_ids.push(metric_id.to_string());
        if let Some(steps) = steps {
            step_sum += steps;
            step_count += 1;
        }
        if let Some(cadence) = cadence {
            cadence_sum += cadence;
            cadence_count += 1;
        }
    }

    support.metric_ids.sort();
    if step_count > 0 {
        support.step_count = Some(step_sum);
    }
    if cadence_count > 0 {
        support.average_cadence_spm = Some(round_1(cadence_sum / cadence_count as f64));
    }
    support
}

fn step_cadence_support_json(support: &StepCadenceSupport) -> Value {
    json!({
        "source_kind": support.source_kind(),
        "metric_ids": support.metric_ids,
        "metric_count": support.metric_count,
        "steps": support.step_count,
        "average_cadence_spm": support.average_cadence_spm,
        "policy": "device_counter_step_cadence_only",
    })
}

fn daily_activity_metric_id(date_key: &str, timezone: &str) -> String {
    format!(
        "daily-activity-energy-{}-{}-local-estimate-v0",
        sanitize_id_part(date_key),
        sanitize_id_part(timezone)
    )
}

const ENERGY_UNAVAILABLE_METRIC_IDS: &[&str] = &["active_kcal", "resting_kcal", "total_kcal"];

fn energy_metric_name(metric_id: &str) -> &'static str {
    match metric_id {
        "active_kcal" => "active_kcal",
        "resting_kcal" => "resting_kcal",
        "total_kcal" => "total_kcal",
        _ => "unknown_energy_metric",
    }
}

fn energy_unavailable_metric_id(metric_id: &str, date_key: &str, timezone: &str) -> String {
    format!(
        "daily-activity-energy-{}-{}-{}-unavailable-v0",
        sanitize_id_part(metric_id),
        sanitize_id_part(date_key),
        sanitize_id_part(timezone)
    )
}

fn available_energy_metric_count_for_metric(
    store: &GooseStore,
    date_key: &str,
    timezone: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
    metric_id: &str,
) -> GooseResult<usize> {
    Ok(store
        .daily_activity_metrics_between(start_time_unix_ms, end_time_unix_ms)?
        .into_iter()
        .filter(|row| row.date_key == date_key)
        .filter(|row| row.timezone == timezone)
        .filter(|row| row.source_kind != "unavailable")
        .filter(|row| match metric_id {
            "active_kcal" => row.active_kcal.is_some(),
            "resting_kcal" => row.resting_kcal.is_some(),
            "total_kcal" => row.total_kcal.is_some(),
            _ => false,
        })
        .count())
}

fn unavailable_energy_blocker_reasons(rollup: &EnergyDailyRollupReport) -> Vec<String> {
    let mut blockers = rollup.issues.clone();
    if blockers.is_empty() {
        blockers.push("no_available_local_energy_metric".to_string());
    }
    blockers.sort();
    blockers.dedup();
    blockers
}

fn unavailable_energy_quality_flags(
    rollup: &EnergyDailyRollupReport,
    metric_id: &str,
    blocker_reasons: &[String],
) -> Vec<String> {
    rollup
        .quality_flags
        .iter()
        .chain(blocker_reasons.iter())
        .cloned()
        .chain([
            format!("{metric_id}_unavailable"),
            "energy_metric_unavailable".to_string(),
            "source_kind_unavailable".to_string(),
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn hourly_activity_metric_id(
    date_key: &str,
    timezone: &str,
    start_time_unix_ms: i64,
    end_time_unix_ms: i64,
) -> String {
    format!(
        "hourly-activity-energy-{}-{}-{}-{}-local-estimate-v0",
        sanitize_id_part(date_key),
        sanitize_id_part(timezone),
        start_time_unix_ms,
        end_time_unix_ms
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

fn round_1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn validate_options(options: &EnergyDailyRollupOptions<'_>) -> GooseResult<()> {
    if options.date_key.trim().is_empty() {
        return Err(GooseError::message("date_key is required"));
    }
    if options.timezone.trim().is_empty() {
        return Err(GooseError::message("timezone is required"));
    }
    if let Some(weight) = options.profile_weight_kg
        && (!weight.is_finite() || !(25.0..=300.0).contains(&weight))
    {
        return Err(GooseError::message(
            "profile_weight_kg must be between 25 and 300",
        ));
    }
    if let Some(resting_hr_bpm) = options.resting_hr_bpm
        && (!resting_hr_bpm.is_finite() || resting_hr_bpm <= 0.0)
    {
        return Err(GooseError::message("resting_hr_bpm must be positive"));
    }
    if let Some(max_hr_bpm) = options.max_hr_bpm
        && (!max_hr_bpm.is_finite() || max_hr_bpm <= 0.0)
    {
        return Err(GooseError::message("max_hr_bpm must be positive"));
    }
    if options.min_heart_rate_samples == 0 {
        return Err(GooseError::message(
            "min_heart_rate_samples must be at least 1",
        ));
    }
    Ok(())
}

fn validate_hourly_options(options: &EnergyHourlyRollupOptions<'_>) -> GooseResult<()> {
    if options.date_key.trim().is_empty() {
        return Err(GooseError::message("date_key is required"));
    }
    if options.timezone.trim().is_empty() {
        return Err(GooseError::message("timezone is required"));
    }
    if let Some(weight) = options.profile_weight_kg
        && (!weight.is_finite() || !(25.0..=300.0).contains(&weight))
    {
        return Err(GooseError::message(
            "profile_weight_kg must be between 25 and 300",
        ));
    }
    if let Some(resting_hr_bpm) = options.resting_hr_bpm
        && (!resting_hr_bpm.is_finite() || resting_hr_bpm <= 0.0)
    {
        return Err(GooseError::message("resting_hr_bpm must be positive"));
    }
    if let Some(max_hr_bpm) = options.max_hr_bpm
        && (!max_hr_bpm.is_finite() || max_hr_bpm <= 0.0)
    {
        return Err(GooseError::message("max_hr_bpm must be positive"));
    }
    if options.min_heart_rate_samples == 0 {
        return Err(GooseError::message(
            "min_heart_rate_samples must be at least 1",
        ));
    }
    Ok(())
}

fn validate_energy_validation_options(
    options: &EnergyCaptureValidationOptions<'_>,
) -> GooseResult<()> {
    if !options.tolerance_kcal.is_finite() || options.tolerance_kcal < 0.0 {
        return Err(GooseError::message("tolerance_kcal must be nonnegative"));
    }
    if !options.relative_tolerance_fraction.is_finite()
        || !(0.0..=1.0).contains(&options.relative_tolerance_fraction)
    {
        return Err(GooseError::message(
            "relative_tolerance_fraction must be between 0 and 1",
        ));
    }
    for (name, value) in [
        (
            "official_whoop_active_kcal",
            options.official_whoop_active_kcal,
        ),
        (
            "official_whoop_resting_kcal",
            options.official_whoop_resting_kcal,
        ),
        (
            "official_whoop_total_kcal",
            options.official_whoop_total_kcal,
        ),
    ] {
        if let Some(value) = value
            && (!value.is_finite() || value < 0.0)
        {
            return Err(GooseError::message(format!("{name} must be nonnegative")));
        }
    }
    Ok(())
}

struct EnergyLabelComparison {
    error: Option<f64>,
    within_tolerance: Option<bool>,
}

fn compare_energy_label(
    local_kcal: Option<f64>,
    label_kcal: Option<f64>,
    tolerance_kcal: f64,
    relative_tolerance_fraction: f64,
) -> EnergyLabelComparison {
    let Some(label_kcal) = label_kcal else {
        return EnergyLabelComparison {
            error: None,
            within_tolerance: None,
        };
    };
    let Some(local_kcal) = local_kcal else {
        return EnergyLabelComparison {
            error: None,
            within_tolerance: Some(false),
        };
    };
    let error = local_kcal - label_kcal;
    let tolerance = tolerance_kcal.max(label_kcal.abs() * relative_tolerance_fraction);
    EnergyLabelComparison {
        error: Some(round_1(error)),
        within_tolerance: Some(error.abs() <= tolerance),
    }
}

fn push_energy_label_issue(
    issues: &mut Vec<String>,
    metric: &str,
    local_kcal: Option<f64>,
    label_kcal: Option<f64>,
    within_tolerance: Option<bool>,
) {
    if label_kcal.is_none() {
        return;
    }
    if local_kcal.is_none() {
        issues.push(format!("local_{metric}_missing"));
        return;
    }
    if within_tolerance == Some(false) {
        issues.push(format!("official_whoop_{metric}_outside_tolerance"));
    }
}

fn rollup_next_actions(issues: &[String]) -> Vec<EnergyRollupNextAction> {
    let mut actions = Vec::new();
    if issues
        .iter()
        .any(|issue| issue == "insufficient_heart_rate_samples")
    {
        actions.push(EnergyRollupNextAction {
            scope: "energy:daily-rollup".to_string(),
            reason: "insufficient_hr_samples".to_string(),
            action: "Capture packet-derived heart-rate samples in the selected window before writing local calories.".to_string(),
        });
    }
    if issues
        .iter()
        .any(|issue| issue == "no_energy_metric_window")
    {
        actions.push(EnergyRollupNextAction {
            scope: "energy:daily-rollup".to_string(),
            reason: "no_metric_window".to_string(),
            action:
                "Run metric-window extraction and verify WHOOP HR packets are present for the day."
                    .to_string(),
        });
    }
    actions.sort();
    actions.dedup();
    actions
}

fn energy_validation_next_actions(issues: &[String]) -> Vec<EnergyRollupNextAction> {
    let mut actions = Vec::new();
    for issue in issues {
        let (scope, reason, action) = match issue.as_str() {
            _ if official_label_policy_issue_action(issue).is_some() => (
                "energy:validation",
                issue.as_str(),
                official_label_policy_issue_action(issue).unwrap(),
            ),
            "no_energy_validation_label" => (
                "energy:validation",
                "no_validation_label",
                "Rerun with one or more manually captured WHOOP app calorie labels for the same rest, walk, or workout window.",
            ),
            "energy_rollup_blocked" => (
                "energy:validation",
                "energy_rollup_blocked",
                "Fix the local energy rollup blockers before comparing calories against validation labels.",
            ),
            "official_whoop_active_kcal_outside_tolerance"
            | "official_whoop_resting_kcal_outside_tolerance"
            | "official_whoop_total_kcal_outside_tolerance" => (
                "energy:validation",
                "label_delta_outside_tolerance",
                "Compare rest, walk, and workout captures, then tune the local energy estimator only from packet-derived inputs.",
            ),
            "local_active_kcal_missing"
            | "local_resting_kcal_missing"
            | "local_total_kcal_missing" => (
                "energy:validation",
                "local_value_missing",
                "Capture enough WHOOP heart-rate and motion packets for the selected window before validating calorie labels.",
            ),
            _ if issue.starts_with("energy_rollup_issue:") => (
                "energy:validation",
                "energy_rollup_issue",
                "Inspect the embedded energy_rollup report and rerun after its next actions are resolved.",
            ),
            _ => (
                "energy:validation",
                "review_issue",
                "Inspect the validation issue and rerun with a narrower capture window or clearer label provenance.",
            ),
        };
        actions.push(EnergyRollupNextAction {
            scope: scope.to_string(),
            reason: reason.to_string(),
            action: action.to_string(),
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
