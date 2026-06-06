use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::activity_identity::{
    ActivityIdentityInput, activity_idempotency_key as build_activity_idempotency_key,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthPlatform {
    HealthKit,
    HealthConnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthSyncDryRunInput {
    pub schema: String,
    pub platform: HealthPlatform,
    pub permission_grants: Vec<String>,
    pub backfill: HealthSyncWindow,
    pub candidates: Vec<HealthSyncCandidate>,
    #[serde(default)]
    pub partial_plan_policy: HealthSyncPartialPlanPolicy,
    #[serde(default)]
    pub delete_policy: HealthSyncDeletePolicy,
    #[serde(default)]
    pub existing_records: Vec<ExistingHealthRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityHealthSyncDryRunInput {
    pub schema: String,
    pub platform: HealthPlatform,
    pub permission_grants: Vec<String>,
    pub backfill: HealthSyncWindow,
    pub sessions: Vec<ActivitySyncCandidate>,
    #[serde(default)]
    pub partial_plan_policy: HealthSyncPartialPlanPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthSyncWindow {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthSyncCandidate {
    pub record_id: String,
    pub metric_family: String,
    pub semantic: String,
    pub source_kind: String,
    pub start_time: String,
    pub end_time: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub algorithm_id: Option<String>,
    #[serde(default)]
    pub algorithm_version: Option<String>,
    #[serde(default)]
    pub approved_by_user: bool,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthSyncDeletePolicy {
    #[default]
    None,
    StaleInBackfill,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthSyncPartialPlanPolicy {
    #[default]
    AllowPlannedRowsAfterConfirmation,
    RequireAllRecordsReady,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExistingHealthRecord {
    pub platform_record_id: String,
    pub destination_type: String,
    pub idempotency_key: String,
    pub goose_marker: String,
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySyncCandidate {
    pub session_id: String,
    #[serde(default)]
    pub session_kind: HealthSyncSessionKind,
    pub activity_type: String,
    #[serde(default)]
    pub raw_activity_type: Option<String>,
    #[serde(default)]
    pub custom_label: Option<String>,
    pub source_kind: String,
    pub start_time: String,
    pub end_time: String,
    pub confidence_0_to_1: f64,
    #[serde(default)]
    pub approved_by_user: bool,
    #[serde(default)]
    pub metrics: Vec<ActivitySyncMetric>,
    #[serde(default)]
    pub intervals: Vec<ActivitySyncInterval>,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthSyncSessionKind {
    #[default]
    Activity,
    Workout,
    Sleep,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySyncMetric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub quality_flags: Vec<String>,
    #[serde(default)]
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySyncInterval {
    pub interval_id: String,
    pub kind: String,
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub metrics: Vec<ActivitySyncMetric>,
    #[serde(default)]
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthSyncDryRunReport {
    pub schema: String,
    pub generated_by: String,
    pub platform: HealthPlatform,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub all_candidate_writes_planned: bool,
    #[serde(default)]
    pub all_requested_deletes_planned: bool,
    #[serde(default)]
    pub all_records_ready: bool,
    pub partial_plan_policy: HealthSyncPartialPlanPolicy,
    #[serde(default)]
    pub partial_plan: bool,
    #[serde(default)]
    pub partial_plan_confirmation_required: bool,
    #[serde(default)]
    pub platform_write_blocked_by_partial_plan: bool,
    #[serde(default)]
    pub permissions_ready: bool,
    #[serde(default)]
    pub mappings_ready: bool,
    #[serde(default)]
    pub units_ready: bool,
    #[serde(default)]
    pub provenance_ready: bool,
    #[serde(default)]
    pub source_policy_ready: bool,
    #[serde(default)]
    pub idempotency_ready: bool,
    #[serde(default)]
    pub cleanup_scope_ready: bool,
    pub backfill: HealthSyncWindow,
    pub delete_policy: HealthSyncDeletePolicy,
    pub permission_grants: Vec<String>,
    pub candidate_count: usize,
    pub existing_record_count: usize,
    pub planned_write_count: usize,
    pub blocked_count: usize,
    pub planned_delete_count: usize,
    pub blocked_delete_count: usize,
    pub planned_writes: Vec<PlannedHealthWrite>,
    pub blocked_records: Vec<BlockedHealthRecord>,
    pub planned_deletes: Vec<PlannedHealthDelete>,
    pub blocked_deletes: Vec<BlockedHealthDelete>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<HealthSyncNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityHealthSyncDryRunReport {
    pub schema: String,
    pub generated_by: String,
    pub platform: HealthPlatform,
    pub pass: bool,
    pub input_valid: bool,
    pub partial_plan_policy: HealthSyncPartialPlanPolicy,
    #[serde(default)]
    pub partial_plan: bool,
    #[serde(default)]
    pub partial_plan_confirmation_required: bool,
    #[serde(default)]
    pub platform_write_blocked_by_partial_plan: bool,
    pub backfill: HealthSyncWindow,
    pub permission_grants: Vec<String>,
    pub session_count: usize,
    pub planned_session_count: usize,
    pub blocked_session_count: usize,
    pub planned_sessions: Vec<PlannedActivityHealthWrite>,
    pub blocked_sessions: Vec<BlockedActivityHealthSession>,
    pub issues: Vec<String>,
    pub next_actions: Vec<HealthSyncNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedHealthWrite {
    pub source_record_id: String,
    pub destination_type: String,
    pub value: f64,
    pub unit: String,
    pub start_time: String,
    pub end_time: String,
    pub idempotency_key: String,
    pub goose_marker: String,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedActivityHealthWrite {
    pub session_id: String,
    pub session_kind: HealthSyncSessionKind,
    pub destination_type: String,
    pub activity_type: String,
    pub destination_activity_type: String,
    #[serde(default)]
    pub raw_activity_type: Option<String>,
    #[serde(default)]
    pub custom_label: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub idempotency_key: String,
    pub goose_marker: String,
    pub attached_metric_count: usize,
    pub attached_interval_count: usize,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockedHealthRecord {
    pub source_record_id: String,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<HealthSyncNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockedActivityHealthSession {
    pub session_id: String,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<HealthSyncNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedHealthDelete {
    pub platform_record_id: String,
    pub destination_type: String,
    pub idempotency_key: String,
    pub goose_marker: String,
    pub start_time: String,
    pub end_time: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockedHealthDelete {
    pub platform_record_id: String,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<HealthSyncNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthSyncNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone)]
struct PlatformMapping {
    destination_type: &'static str,
    required_unit: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityAttachmentKind {
    HeartRate,
    Energy,
    Distance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UtcInstant(i128);

#[derive(Debug, Clone, Copy)]
struct ParsedHealthSyncWindow {
    start: Option<UtcInstant>,
    end: Option<UtcInstant>,
}

impl ParsedHealthSyncWindow {
    fn contains_start(&self, value: UtcInstant) -> Option<bool> {
        Some(value >= self.start? && value < self.end?)
    }
}

#[derive(Debug, Clone, Copy)]
struct PartialPlanState {
    partial_plan: bool,
    confirmation_required: bool,
    blocked_by_policy: bool,
}

fn partial_plan_state(
    policy: HealthSyncPartialPlanPolicy,
    input_valid: bool,
    planned_count: usize,
    blocked_count: usize,
) -> PartialPlanState {
    let partial_plan = input_valid && planned_count > 0 && blocked_count > 0;
    PartialPlanState {
        partial_plan,
        confirmation_required: partial_plan
            && policy == HealthSyncPartialPlanPolicy::AllowPlannedRowsAfterConfirmation,
        blocked_by_policy: partial_plan
            && policy == HealthSyncPartialPlanPolicy::RequireAllRecordsReady,
    }
}

fn partial_plan_next_actions(scope: &str, state: PartialPlanState) -> Vec<HealthSyncNextAction> {
    if state.confirmation_required {
        return vec![HealthSyncNextAction {
            scope: scope.to_string(),
            reason: "partial_plan_requires_confirmation".to_string(),
            action: "Ask the user to confirm syncing only the planned Goose rows; blocked rows must remain unwritten.".to_string(),
        }];
    }
    if state.blocked_by_policy {
        return vec![HealthSyncNextAction {
            scope: scope.to_string(),
            reason: "partial_plan_blocked_by_policy".to_string(),
            action: "Resolve blocked rows before writing because the policy requires all selected Health rows to be ready.".to_string(),
        }];
    }
    Vec::new()
}

fn validate_backfill_window(
    backfill: &HealthSyncWindow,
    issues: &mut Vec<String>,
) -> ParsedHealthSyncWindow {
    let start = parse_named_window_instant("backfill.start", &backfill.start, issues);
    let end = parse_named_window_instant("backfill.end", &backfill.end, issues);
    if let (Some(start), Some(end)) = (start, end)
        && start >= end
    {
        issues.push("backfill.start must be earlier than backfill.end".to_string());
    }
    ParsedHealthSyncWindow { start, end }
}

fn parse_named_window_instant(
    field: &str,
    value: &str,
    issues: &mut Vec<String>,
) -> Option<UtcInstant> {
    if value.trim().is_empty() {
        issues.push(format!("{field}_required"));
        return None;
    }
    let parsed = parse_utc_instant(value);
    if parsed.is_none() {
        issues.push(format!("{field}_invalid_timestamp"));
    }
    parsed
}

fn parse_required_field_instant(
    field: &str,
    value: &str,
    reasons: &mut Vec<String>,
) -> Option<UtcInstant> {
    if value.trim().is_empty() {
        return None;
    }
    let parsed = parse_utc_instant(value);
    if parsed.is_none() {
        reasons.push(format!("{field}_invalid_timestamp"));
    }
    parsed
}

fn parse_optional_field_instant(
    field: &str,
    value: Option<&str>,
    reasons: &mut Vec<String>,
) -> Option<UtcInstant> {
    let value = value?;
    if value.trim().is_empty() {
        reasons.push(format!("{field}_invalid_timestamp"));
        return None;
    }
    let parsed = parse_utc_instant(value);
    if parsed.is_none() {
        reasons.push(format!("{field}_invalid_timestamp"));
    }
    parsed
}

fn parse_utc_instant(value: &str) -> Option<UtcInstant> {
    let value = value.trim();
    let (date, time_and_offset) = value.split_once('T')?;
    let (year, month, day) = parse_date(date)?;
    let (time, offset_seconds) = split_time_and_offset(time_and_offset)?;
    let (hour, minute, second, nanos) = parse_time(time)?;
    let days = days_from_civil(year, month, day)?;
    let seconds = i128::from(days) * 86_400
        + i128::from(hour) * 3_600
        + i128::from(minute) * 60
        + i128::from(second)
        - i128::from(offset_seconds);
    Some(UtcInstant(seconds * 1_000_000_000 + i128::from(nanos)))
}

fn parse_date(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || value.len() != 10 {
        return None;
    }
    if !(1..=12).contains(&month) {
        return None;
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn split_time_and_offset(value: &str) -> Option<(&str, i32)> {
    if let Some(time) = value.strip_suffix('Z') {
        return Some((time, 0));
    }
    let split_index = value.rfind(['+', '-'])?;
    if split_index == 0 {
        return None;
    }
    let (time, offset) = value.split_at(split_index);
    let sign = if offset.starts_with('+') { 1 } else { -1 };
    let offset = &offset[1..];
    let (hour, minute) = offset.split_once(':')?;
    if hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    let hour = hour.parse::<i32>().ok()?;
    let minute = minute.parse::<i32>().ok()?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    Some((time, sign * (hour * 3_600 + minute * 60)))
}

fn parse_time(value: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    let seconds_part = parts.next()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second_text, fraction_text) = seconds_part
        .split_once('.')
        .map_or((seconds_part, None), |(second, fraction)| {
            (second, Some(fraction))
        });
    let second = second_text.parse::<u32>().ok()?;
    if second > 59 {
        return None;
    }
    let nanos = match fraction_text {
        Some(fraction) if fraction.is_empty() || fraction.len() > 9 => return None,
        Some(fraction) => {
            if !fraction.chars().all(|char| char.is_ascii_digit()) {
                return None;
            }
            let mut padded = fraction.to_string();
            while padded.len() < 9 {
                padded.push('0');
            }
            padded.parse::<u32>().ok()?
        }
        None => 0,
    };
    Some((hour, minute, second, nanos))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let month_i32 = i32::try_from(month).ok()?;
    let day_i32 = i32::try_from(day).ok()?;
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month_i32 + if month_i32 > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day_i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
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

pub fn run_health_sync_dry_run(input: &HealthSyncDryRunInput) -> HealthSyncDryRunReport {
    let mut issues = Vec::new();
    if input.schema != "goose.health-sync-dry-run.v1" {
        issues.push(format!("unsupported schema {}", input.schema));
    }
    let backfill_times = validate_backfill_window(&input.backfill, &mut issues);

    let grants: BTreeSet<&str> = input.permission_grants.iter().map(String::as_str).collect();
    let mut planned_writes = Vec::new();
    let mut blocked_records = Vec::new();
    let mut seen_keys = BTreeSet::new();

    for candidate in &input.candidates {
        let mut reasons = validate_candidate_policy(candidate, input.platform, &backfill_times);

        let mapping = platform_mapping(input.platform, &candidate.semantic);
        match mapping {
            Some(ref mapping) => {
                if candidate.unit != mapping.required_unit {
                    reasons.push(format!(
                        "unit_mismatch_expected_{}",
                        mapping.required_unit.replace('/', "_per_")
                    ));
                }
                if !grants.contains(mapping.destination_type) {
                    reasons.push("permission_denied".to_string());
                }
            }
            None => reasons.push("unsupported_mapping".to_string()),
        }

        if reasons.is_empty() {
            let mapping = mapping.expect("mapping exists when reasons is empty");
            let idempotency_key =
                idempotency_key(input.platform, mapping.destination_type, candidate);
            if !seen_keys.insert(idempotency_key.clone()) {
                let reasons = vec!["duplicate_idempotency_key".to_string()];
                blocked_records.push(BlockedHealthRecord {
                    source_record_id: candidate.record_id.clone(),
                    next_actions: next_actions_for_health_reasons(
                        &candidate.record_id,
                        &reasons,
                        input.platform,
                        mapping.destination_type,
                    ),
                    reasons,
                });
                continue;
            }
            planned_writes.push(PlannedHealthWrite {
                source_record_id: candidate.record_id.clone(),
                destination_type: mapping.destination_type.to_string(),
                value: candidate.value,
                unit: candidate.unit.clone(),
                start_time: candidate.start_time.clone(),
                end_time: candidate.end_time.clone(),
                idempotency_key,
                goose_marker: goose_marker(candidate),
                provenance: candidate.provenance.clone(),
            });
        } else {
            reasons.sort();
            reasons.dedup();
            let destination_type = mapping
                .as_ref()
                .map(|mapping| mapping.destination_type)
                .unwrap_or("mapped health type");
            blocked_records.push(BlockedHealthRecord {
                source_record_id: candidate.record_id.clone(),
                next_actions: next_actions_for_health_reasons(
                    &candidate.record_id,
                    &reasons,
                    input.platform,
                    destination_type,
                ),
                reasons,
            });
        }
    }

    let (planned_deletes, blocked_deletes) =
        plan_health_deletes(input, &planned_writes, &grants, &backfill_times);
    let mut next_actions = report_next_actions(&issues, &blocked_records, &blocked_deletes);
    let input_valid = issues.is_empty();
    let all_candidate_writes_planned = blocked_records.is_empty();
    let all_requested_deletes_planned = blocked_deletes.is_empty();
    let all_records_ready = all_candidate_writes_planned && all_requested_deletes_planned;
    let partial_plan_state = partial_plan_state(
        input.partial_plan_policy,
        input_valid,
        planned_writes.len() + planned_deletes.len(),
        blocked_records.len() + blocked_deletes.len(),
    );
    next_actions.extend(partial_plan_next_actions(
        "health_sync_report",
        partial_plan_state,
    ));
    next_actions = dedupe_health_next_actions(next_actions);
    let permissions_ready = health_permissions_ready(&blocked_records, &blocked_deletes);
    let mappings_ready = health_mappings_ready(&blocked_records, &blocked_deletes);
    let units_ready = health_units_ready(&blocked_records);
    let provenance_ready = health_provenance_ready(&blocked_records);
    let source_policy_ready = health_source_policy_ready(&blocked_records);
    let idempotency_ready = health_idempotency_ready(&blocked_records);
    let cleanup_scope_ready = health_cleanup_scope_ready(&blocked_deletes);

    HealthSyncDryRunReport {
        schema: "goose.health-sync-dry-run-report.v1".to_string(),
        generated_by: "goose-health-sync-dry-run".to_string(),
        platform: input.platform,
        pass: input_valid,
        input_valid,
        all_candidate_writes_planned,
        all_requested_deletes_planned,
        all_records_ready,
        partial_plan_policy: input.partial_plan_policy,
        partial_plan: partial_plan_state.partial_plan,
        partial_plan_confirmation_required: partial_plan_state.confirmation_required,
        platform_write_blocked_by_partial_plan: partial_plan_state.blocked_by_policy,
        permissions_ready,
        mappings_ready,
        units_ready,
        provenance_ready,
        source_policy_ready,
        idempotency_ready,
        cleanup_scope_ready,
        backfill: input.backfill.clone(),
        delete_policy: input.delete_policy,
        permission_grants: grants.iter().map(|grant| (*grant).to_string()).collect(),
        candidate_count: input.candidates.len(),
        existing_record_count: input.existing_records.len(),
        planned_write_count: planned_writes.len(),
        blocked_count: blocked_records.len(),
        planned_delete_count: planned_deletes.len(),
        blocked_delete_count: blocked_deletes.len(),
        planned_writes,
        blocked_records,
        planned_deletes,
        blocked_deletes,
        issues,
        next_actions,
    }
}

pub fn run_activity_health_sync_dry_run(
    input: &ActivityHealthSyncDryRunInput,
) -> ActivityHealthSyncDryRunReport {
    let mut issues = Vec::new();
    if input.schema != "goose.activity-health-sync-dry-run.v1" {
        issues.push(format!("unsupported schema {}", input.schema));
    }
    let backfill_times = validate_backfill_window(&input.backfill, &mut issues);

    let grants: BTreeSet<&str> = input.permission_grants.iter().map(String::as_str).collect();
    let mut planned_sessions = Vec::new();
    let mut blocked_sessions = Vec::new();
    let mut seen_keys = BTreeSet::new();

    for session in &input.sessions {
        let destination_type = activity_destination_type(input.platform, session.session_kind);
        let destination_activity_type = destination_activity_type(input.platform, session);
        let mut reasons = validate_activity_sync_candidate(session, &backfill_times);
        if destination_activity_type.is_none() {
            reasons.push("unsupported_activity_type_mapping".to_string());
        }
        if !grants.contains(destination_type) {
            reasons.push("permission_denied".to_string());
        }
        if reasons.is_empty() {
            let destination_activity_type =
                destination_activity_type.expect("destination activity type is validated");
            let idempotency_key = activity_idempotency_key(session);
            if !seen_keys.insert(idempotency_key.clone()) {
                reasons.push("duplicate_idempotency_key".to_string());
            } else {
                planned_sessions.push(PlannedActivityHealthWrite {
                    session_id: session.session_id.clone(),
                    session_kind: session.session_kind,
                    destination_type: destination_type.to_string(),
                    activity_type: session.activity_type.clone(),
                    destination_activity_type: destination_activity_type.to_string(),
                    raw_activity_type: session.raw_activity_type.clone(),
                    custom_label: session.custom_label.clone(),
                    start_time: session.start_time.clone(),
                    end_time: session.end_time.clone(),
                    idempotency_key,
                    goose_marker: activity_goose_marker(session),
                    attached_metric_count: syncable_activity_metric_count(session),
                    attached_interval_count: syncable_activity_interval_count(session),
                    provenance: session.provenance.clone(),
                });
                continue;
            }
        }
        reasons.sort();
        reasons.dedup();
        blocked_sessions.push(BlockedActivityHealthSession {
            session_id: session.session_id.clone(),
            next_actions: next_actions_for_health_reasons(
                &session.session_id,
                &reasons,
                input.platform,
                destination_type,
            ),
            reasons,
        });
    }

    let mut next_actions = activity_report_next_actions(&issues, &blocked_sessions);
    let input_valid = issues.is_empty();
    let partial_plan_state = partial_plan_state(
        input.partial_plan_policy,
        input_valid,
        planned_sessions.len(),
        blocked_sessions.len(),
    );
    next_actions.extend(partial_plan_next_actions(
        "activity_health_sync_report",
        partial_plan_state,
    ));
    next_actions = dedupe_health_next_actions(next_actions);
    ActivityHealthSyncDryRunReport {
        schema: "goose.activity-health-sync-dry-run-report.v1".to_string(),
        generated_by: "goose-activity-health-sync-dry-run".to_string(),
        platform: input.platform,
        pass: input_valid,
        input_valid,
        partial_plan_policy: input.partial_plan_policy,
        partial_plan: partial_plan_state.partial_plan,
        partial_plan_confirmation_required: partial_plan_state.confirmation_required,
        platform_write_blocked_by_partial_plan: partial_plan_state.blocked_by_policy,
        backfill: input.backfill.clone(),
        permission_grants: grants.iter().map(|grant| (*grant).to_string()).collect(),
        session_count: input.sessions.len(),
        planned_session_count: planned_sessions.len(),
        blocked_session_count: blocked_sessions.len(),
        planned_sessions,
        blocked_sessions,
        issues,
        next_actions,
    }
}

fn validate_activity_sync_candidate(
    session: &ActivitySyncCandidate,
    backfill: &ParsedHealthSyncWindow,
) -> Vec<String> {
    let mut reasons = Vec::new();
    for (name, value) in [
        ("session_id", session.session_id.as_str()),
        ("activity_type", session.activity_type.as_str()),
        ("source_kind", session.source_kind.as_str()),
        ("start_time", session.start_time.as_str()),
        ("end_time", session.end_time.as_str()),
    ] {
        if value.trim().is_empty() {
            reasons.push(format!("{name}_required"));
        }
    }
    let start_time = parse_required_field_instant("start_time", &session.start_time, &mut reasons);
    let end_time = parse_required_field_instant("end_time", &session.end_time, &mut reasons);
    if let Some(start_time) = start_time
        && backfill.contains_start(start_time) == Some(false)
    {
        reasons.push("outside_backfill_window".to_string());
    }
    if let (Some(start_time), Some(end_time)) = (start_time, end_time)
        && end_time <= start_time
    {
        reasons.push("end_time_not_after_start_time".to_string());
    }
    if !session.confidence_0_to_1.is_finite() {
        reasons.push("activity_confidence_not_finite".to_string());
    } else if !(0.0..=1.0).contains(&session.confidence_0_to_1) {
        reasons.push("activity_confidence_out_of_range".to_string());
    } else if session.confidence_0_to_1 < 0.75 && !session.approved_by_user {
        reasons.push("candidate_activity_requires_user_approval".to_string());
    }
    if !session.approved_by_user {
        reasons.push("not_user_approved".to_string());
    }
    if !is_syncable_activity_source_kind(&session.source_kind, session.session_kind) {
        reasons.push("unsafe_source_kind".to_string());
    }
    if session.session_kind == HealthSyncSessionKind::Sleep
        && imported_platform_sleep_marker_present(session)
    {
        reasons.push("imported_platform_sleep_not_syncable".to_string());
    }
    match &session.provenance {
        serde_json::Value::Object(object) if object.is_empty() => {
            reasons.push("missing_provenance".to_string());
        }
        serde_json::Value::Object(_) => {
            if contains_private_api_marker(&session.provenance) {
                reasons.push("private_api_provenance_not_syncable".to_string());
            }
            if contains_official_whoop_label_marker(&session.provenance) {
                reasons.push("official_whoop_label_not_syncable".to_string());
            }
            if contains_platform_import_marker(&session.provenance) {
                reasons.push("platform_import_not_syncable".to_string());
            }
        }
        serde_json::Value::Null => reasons.push("missing_provenance".to_string()),
        _ => reasons.push("provenance_must_be_object".to_string()),
    }
    for metric in &session.metrics {
        reasons.extend(validate_activity_sync_metric(metric, start_time, end_time));
    }
    for interval in &session.intervals {
        reasons.extend(validate_activity_sync_interval(
            interval, start_time, end_time,
        ));
    }
    reasons
}

fn validate_activity_sync_metric(
    metric: &ActivitySyncMetric,
    container_start_time: Option<UtcInstant>,
    container_end_time: Option<UtcInstant>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if metric.name.trim().is_empty() {
        reasons.push("activity_metric_name_required".to_string());
    }
    if metric.unit.trim().is_empty() {
        reasons.push("activity_metric_unit_required".to_string());
    }
    if !metric.value.is_finite() {
        reasons.push("activity_metric_value_not_finite".to_string());
    }

    let start_time = parse_optional_field_instant(
        "activity_metric_start_time",
        metric.start_time.as_deref(),
        &mut reasons,
    );
    let end_time = parse_optional_field_instant(
        "activity_metric_end_time",
        metric.end_time.as_deref(),
        &mut reasons,
    );
    if metric.start_time.is_some() != metric.end_time.is_some() {
        reasons.push("activity_metric_time_range_incomplete".to_string());
    }
    if let (Some(start_time), Some(end_time)) = (start_time, end_time) {
        if end_time <= start_time {
            reasons.push("activity_metric_end_time_not_after_start_time".to_string());
        } else if let (Some(container_start_time), Some(container_end_time)) =
            (container_start_time, container_end_time)
            && (start_time < container_start_time || end_time > container_end_time)
        {
            return reasons;
        }
    }

    let Some(kind) = activity_metric_attachment_kind(metric) else {
        return reasons;
    };
    if !metric.unit.trim().is_empty()
        && !activity_metric_unit_is_supported(kind, metric.unit.trim())
    {
        reasons.push(format!(
            "unit_mismatch_expected_{}",
            activity_metric_expected_unit(kind).replace('/', "_per_")
        ));
    }
    reasons.extend(activity_sample_provenance_reasons(&metric.provenance));
    reasons
}

fn validate_activity_sync_interval(
    interval: &ActivitySyncInterval,
    session_start_time: Option<UtcInstant>,
    session_end_time: Option<UtcInstant>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if interval.interval_id.trim().is_empty() {
        reasons.push("activity_interval_id_required".to_string());
    }
    if interval.kind.trim().is_empty() {
        reasons.push("activity_interval_kind_required".to_string());
    }
    if interval.start_time.trim().is_empty() {
        reasons.push("activity_interval_start_time_required".to_string());
    }
    if interval.end_time.trim().is_empty() {
        reasons.push("activity_interval_end_time_required".to_string());
    }

    let start_time = parse_required_field_instant(
        "activity_interval_start_time",
        &interval.start_time,
        &mut reasons,
    );
    let end_time = parse_required_field_instant(
        "activity_interval_end_time",
        &interval.end_time,
        &mut reasons,
    );
    if interval.kind.trim().is_empty() {
        return reasons;
    }
    if !activity_interval_is_supported(&interval.kind) {
        return reasons;
    }
    if let (Some(start_time), Some(end_time)) = (start_time, end_time) {
        if end_time <= start_time {
            reasons.push("activity_interval_end_time_not_after_start_time".to_string());
        } else if let (Some(session_start_time), Some(session_end_time)) =
            (session_start_time, session_end_time)
            && (start_time < session_start_time || end_time > session_end_time)
        {
            return reasons;
        }
    }

    reasons.extend(activity_sample_provenance_reasons(&interval.provenance));
    for metric in &interval.metrics {
        reasons.extend(validate_activity_sync_metric(metric, start_time, end_time));
    }
    reasons
}

fn activity_metric_attachment_kind(metric: &ActivitySyncMetric) -> Option<ActivityAttachmentKind> {
    let name = normalized_marker(metric.name.as_str());
    match name.as_str() {
        "heart_rate" | "average_heart_rate" | "heart_rate_bpm" => {
            Some(ActivityAttachmentKind::HeartRate)
        }
        "active_energy" | "active_calories" | "calories" | "energy" => {
            Some(ActivityAttachmentKind::Energy)
        }
        "distance" => Some(ActivityAttachmentKind::Distance),
        _ => None,
    }
}

fn activity_metric_expected_unit(kind: ActivityAttachmentKind) -> &'static str {
    match kind {
        ActivityAttachmentKind::HeartRate => "count/min",
        ActivityAttachmentKind::Energy => "kcal",
        ActivityAttachmentKind::Distance => "km",
    }
}

fn activity_metric_unit_is_supported(kind: ActivityAttachmentKind, unit: &str) -> bool {
    match kind {
        ActivityAttachmentKind::HeartRate => matches!(unit, "count/min" | "bpm"),
        ActivityAttachmentKind::Energy => matches!(unit, "kcal" | "joule"),
        ActivityAttachmentKind::Distance => matches!(unit, "m" | "km" | "mi"),
    }
}

fn activity_sample_provenance_reasons(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(object) if object.is_empty() => {
            vec!["missing_provenance".to_string()]
        }
        serde_json::Value::Object(_) => {
            let mut reasons = Vec::new();
            if contains_private_api_marker(value) {
                reasons.push("private_api_provenance_not_syncable".to_string());
            }
            if contains_official_whoop_label_marker(value) {
                reasons.push("official_whoop_label_not_syncable".to_string());
            }
            if contains_platform_import_marker(value) {
                reasons.push("platform_import_not_syncable".to_string());
            }
            reasons
        }
        serde_json::Value::Null => vec!["missing_provenance".to_string()],
        _ => vec!["provenance_must_be_object".to_string()],
    }
}

fn activity_metric_is_in_attachment_window(
    metric: &ActivitySyncMetric,
    container_start_time: Option<UtcInstant>,
    container_end_time: Option<UtcInstant>,
) -> bool {
    match (&metric.start_time, &metric.end_time) {
        (None, None) => true,
        (Some(start_time), Some(end_time)) => {
            let Some(start_time) = parse_utc_instant(start_time) else {
                return false;
            };
            let Some(end_time) = parse_utc_instant(end_time) else {
                return false;
            };
            if end_time <= start_time {
                return false;
            }
            match (container_start_time, container_end_time) {
                (Some(container_start_time), Some(container_end_time)) => {
                    start_time >= container_start_time && end_time <= container_end_time
                }
                _ => true,
            }
        }
        _ => false,
    }
}

fn activity_interval_is_supported(kind: &str) -> bool {
    matches!(
        normalized_marker(kind).as_str(),
        "lap" | "pause" | "work" | "rest" | "window" | "split"
    )
}

fn activity_interval_is_in_attachment_window(
    interval: &ActivitySyncInterval,
    session_start_time: Option<UtcInstant>,
    session_end_time: Option<UtcInstant>,
) -> bool {
    let Some(start_time) = parse_utc_instant(&interval.start_time) else {
        return false;
    };
    let Some(end_time) = parse_utc_instant(&interval.end_time) else {
        return false;
    };
    if end_time <= start_time {
        return false;
    }
    match (session_start_time, session_end_time) {
        (Some(session_start_time), Some(session_end_time)) => {
            start_time >= session_start_time && end_time <= session_end_time
        }
        _ => true,
    }
}

fn activity_metric_is_attachable(
    metric: &ActivitySyncMetric,
    container_start_time: Option<UtcInstant>,
    container_end_time: Option<UtcInstant>,
) -> bool {
    activity_metric_attachment_kind(metric).is_some()
        && activity_metric_is_in_attachment_window(metric, container_start_time, container_end_time)
        && validate_activity_sync_metric(metric, container_start_time, container_end_time)
            .is_empty()
}

fn activity_interval_is_attachable(
    interval: &ActivitySyncInterval,
    session_start_time: Option<UtcInstant>,
    session_end_time: Option<UtcInstant>,
) -> bool {
    activity_interval_is_supported(&interval.kind)
        && activity_interval_is_in_attachment_window(interval, session_start_time, session_end_time)
        && activity_sample_provenance_reasons(&interval.provenance).is_empty()
        && !interval.interval_id.trim().is_empty()
        && !interval.kind.trim().is_empty()
        && !interval.start_time.trim().is_empty()
        && !interval.end_time.trim().is_empty()
}

fn is_syncable_activity_source_kind(
    source_kind: &str,
    session_kind: HealthSyncSessionKind,
) -> bool {
    match session_kind {
        HealthSyncSessionKind::Activity | HealthSyncSessionKind::Workout => matches!(
            source_kind,
            "activity_session"
                | "workout_session"
                | "packet_derived_activity"
                | "user_confirmed_activity"
                | "manual_activity"
        ),
        HealthSyncSessionKind::Sleep => matches!(
            source_kind,
            "sleep_session" | "packet_derived_sleep" | "user_confirmed_sleep" | "manual_sleep"
        ),
    }
}

fn imported_platform_sleep_marker_present(session: &ActivitySyncCandidate) -> bool {
    let source_kind = normalized_marker(&session.source_kind);
    if source_kind.contains("imported_platform_sleep")
        || source_kind.contains("external_sleep")
        || source_kind.contains("sleep_history_import")
    {
        return true;
    }
    contains_imported_platform_sleep_marker(&session.provenance)
}

fn contains_imported_platform_sleep_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => {
            let marker = normalized_marker(text);
            marker.contains("external_history_context_only")
                || marker.contains("healthkit_sleep_analysis")
                || marker.contains("health_connect_sleep_session")
                || marker.contains("health_connect_sleep_stage")
                || marker.contains("imported_platform_sleep")
                || marker.contains("sleep_history_import")
        }
        serde_json::Value::Object(object) => object.iter().any(|(key, child)| {
            let key_marker = normalized_marker(key);
            key_marker.contains("import_policy") && contains_imported_platform_sleep_marker(child)
                || contains_imported_platform_sleep_marker(child)
        }),
        serde_json::Value::Array(items) => {
            items.iter().any(contains_imported_platform_sleep_marker)
        }
        _ => false,
    }
}

fn activity_destination_type(
    platform: HealthPlatform,
    session_kind: HealthSyncSessionKind,
) -> &'static str {
    match (platform, session_kind) {
        (
            HealthPlatform::HealthKit,
            HealthSyncSessionKind::Activity | HealthSyncSessionKind::Workout,
        ) => "HKWorkout",
        (
            HealthPlatform::HealthConnect,
            HealthSyncSessionKind::Activity | HealthSyncSessionKind::Workout,
        ) => "ExerciseSessionRecord",
        (HealthPlatform::HealthKit, HealthSyncSessionKind::Sleep) => "sleepAnalysis",
        (HealthPlatform::HealthConnect, HealthSyncSessionKind::Sleep) => "SleepSessionRecord",
    }
}

fn destination_activity_type(
    platform: HealthPlatform,
    session: &ActivitySyncCandidate,
) -> Option<&'static str> {
    let activity_type = normalized_marker(&session.activity_type);
    match session.session_kind {
        HealthSyncSessionKind::Sleep => match activity_type.as_str() {
            "sleep" | "asleep" | "nap" => Some("sleep"),
            _ => None,
        },
        HealthSyncSessionKind::Activity | HealthSyncSessionKind::Workout => {
            match (platform, activity_type.as_str()) {
                (HealthPlatform::HealthKit, "cycling" | "bike" | "biking" | "ride") => {
                    Some("cycling")
                }
                (HealthPlatform::HealthConnect, "cycling" | "bike" | "biking" | "ride") => {
                    Some("biking")
                }
                (_, "running" | "run") => Some("running"),
                (_, "walking" | "walk") => Some("walking"),
                (_, "strength" | "strength_training" | "weightlifting" | "weights") => {
                    Some("strength_training")
                }
                (_, "yoga") => Some("yoga"),
                (_, "rowing") => Some("rowing"),
                (_, "swimming") => Some("swimming"),
                (_, "other") => Some("other"),
                _ => None,
            }
        }
    }
}

fn activity_idempotency_key(session: &ActivitySyncCandidate) -> String {
    build_activity_idempotency_key(&ActivityIdentityInput {
        source: session.source_kind.clone(),
        provenance: session.provenance.clone(),
        start_time: session.start_time.clone(),
        end_time: session.end_time.clone(),
        activity_type: session.activity_type.clone(),
        raw_identifiers: Vec::new(),
        labels: session
            .raw_activity_type
            .iter()
            .chain(session.custom_label.iter())
            .cloned()
            .collect(),
    })
}

fn activity_goose_marker(session: &ActivitySyncCandidate) -> String {
    format!(
        "goose:{}:{}:{}",
        session_kind_marker(session.session_kind),
        session.activity_type,
        session.session_id
    )
}

fn session_kind_marker(session_kind: HealthSyncSessionKind) -> &'static str {
    match session_kind {
        HealthSyncSessionKind::Activity => "activity",
        HealthSyncSessionKind::Workout => "workout",
        HealthSyncSessionKind::Sleep => "sleep",
    }
}

fn syncable_activity_metric_count(session: &ActivitySyncCandidate) -> usize {
    let session_start_time = parse_utc_instant(&session.start_time);
    let session_end_time = parse_utc_instant(&session.end_time);
    let mut count = session
        .metrics
        .iter()
        .filter(|metric| {
            activity_metric_is_attachable(metric, session_start_time, session_end_time)
        })
        .count();

    for interval in &session.intervals {
        if activity_interval_is_attachable(interval, session_start_time, session_end_time) {
            let interval_start_time = parse_utc_instant(&interval.start_time);
            let interval_end_time = parse_utc_instant(&interval.end_time);
            count += interval
                .metrics
                .iter()
                .filter(|metric| {
                    activity_metric_is_attachable(metric, interval_start_time, interval_end_time)
                })
                .count();
        }
    }

    count
}

fn syncable_activity_interval_count(session: &ActivitySyncCandidate) -> usize {
    let session_start_time = parse_utc_instant(&session.start_time);
    let session_end_time = parse_utc_instant(&session.end_time);
    session
        .intervals
        .iter()
        .filter(|interval| {
            activity_interval_is_attachable(interval, session_start_time, session_end_time)
        })
        .count()
}

fn activity_report_next_actions(
    issues: &[String],
    blocked_sessions: &[BlockedActivityHealthSession],
) -> Vec<HealthSyncNextAction> {
    let mut actions = Vec::new();
    actions.extend(issues.iter().map(|issue| HealthSyncNextAction {
        scope: "activity_health_sync_report".to_string(),
        reason: issue.clone(),
        action: health_sync_issue_action(issue),
    }));
    actions.extend(
        blocked_sessions
            .iter()
            .flat_map(|session| session.next_actions.clone()),
    );
    dedupe_health_next_actions(actions)
}

fn plan_health_deletes(
    input: &HealthSyncDryRunInput,
    planned_writes: &[PlannedHealthWrite],
    grants: &BTreeSet<&str>,
    backfill: &ParsedHealthSyncWindow,
) -> (Vec<PlannedHealthDelete>, Vec<BlockedHealthDelete>) {
    if input.delete_policy == HealthSyncDeletePolicy::None {
        return (Vec::new(), Vec::new());
    }

    let current_keys = planned_writes
        .iter()
        .map(|write| write.idempotency_key.as_str())
        .collect::<BTreeSet<_>>();
    let mut planned_deletes = Vec::new();
    let mut blocked_deletes = Vec::new();

    for existing in &input.existing_records {
        if current_keys.contains(existing.idempotency_key.as_str()) {
            continue;
        }

        let mut reasons = validate_existing_record_for_delete(existing, input, grants, backfill);
        if reasons.is_empty() {
            planned_deletes.push(PlannedHealthDelete {
                platform_record_id: existing.platform_record_id.clone(),
                destination_type: existing.destination_type.clone(),
                idempotency_key: existing.idempotency_key.clone(),
                goose_marker: existing.goose_marker.clone(),
                start_time: existing.start_time.clone(),
                end_time: existing.end_time.clone(),
                reason: "stale_goose_record_in_backfill".to_string(),
            });
        } else {
            reasons.sort();
            reasons.dedup();
            blocked_deletes.push(BlockedHealthDelete {
                platform_record_id: existing.platform_record_id.clone(),
                next_actions: next_actions_for_health_reasons(
                    &existing.platform_record_id,
                    &reasons,
                    input.platform,
                    &existing.destination_type,
                ),
                reasons,
            });
        }
    }

    (planned_deletes, blocked_deletes)
}

fn health_permissions_ready(
    blocked_records: &[BlockedHealthRecord],
    blocked_deletes: &[BlockedHealthDelete],
) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| reason == "permission_denied")
        && !blocked_deletes
            .iter()
            .flat_map(|record| record.reasons.iter())
            .any(|reason| reason == "delete_permission_denied")
}

fn health_mappings_ready(
    blocked_records: &[BlockedHealthRecord],
    blocked_deletes: &[BlockedHealthDelete],
) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| {
            matches!(
                reason.as_str(),
                "unsupported_mapping"
                    | "healthkit_rmssd_must_not_be_written_as_sdnn"
                    | "health_connect_has_no_sdnn_record"
            )
        })
        && !blocked_deletes
            .iter()
            .flat_map(|record| record.reasons.iter())
            .any(|reason| reason == "unsupported_delete_mapping")
}

fn health_units_ready(blocked_records: &[BlockedHealthRecord]) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| reason.starts_with("unit_mismatch_expected_"))
}

fn health_provenance_ready(blocked_records: &[BlockedHealthRecord]) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| {
            matches!(
                reason.as_str(),
                "missing_provenance"
                    | "provenance_must_be_object"
                    | "private_api_provenance_not_syncable"
                    | "official_whoop_label_not_syncable"
                    | "platform_import_not_syncable"
            )
        })
}

fn health_source_policy_ready(blocked_records: &[BlockedHealthRecord]) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| {
            matches!(
                reason.as_str(),
                "not_user_approved"
                    | "unsafe_source_kind"
                    | "benchmark_only_algorithm_not_syncable"
                    | "private_api_provenance_not_syncable"
                    | "official_whoop_label_not_syncable"
                    | "platform_import_not_syncable"
            )
        })
}

fn health_idempotency_ready(blocked_records: &[BlockedHealthRecord]) -> bool {
    !blocked_records
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| reason == "duplicate_idempotency_key")
}

fn health_cleanup_scope_ready(blocked_deletes: &[BlockedHealthDelete]) -> bool {
    !blocked_deletes
        .iter()
        .flat_map(|record| record.reasons.iter())
        .any(|reason| {
            reason.ends_with("_required")
                || matches!(
                    reason.as_str(),
                    "outside_backfill_window" | "end_time_not_after_start_time" | "not_goose_owned"
                )
        })
}

fn validate_existing_record_for_delete(
    existing: &ExistingHealthRecord,
    input: &HealthSyncDryRunInput,
    grants: &BTreeSet<&str>,
    backfill: &ParsedHealthSyncWindow,
) -> Vec<String> {
    let mut reasons = Vec::new();

    for (name, value) in [
        ("platform_record_id", existing.platform_record_id.as_str()),
        ("destination_type", existing.destination_type.as_str()),
        ("idempotency_key", existing.idempotency_key.as_str()),
        ("goose_marker", existing.goose_marker.as_str()),
        ("start_time", existing.start_time.as_str()),
        ("end_time", existing.end_time.as_str()),
    ] {
        if value.trim().is_empty() {
            reasons.push(format!("{name}_required"));
        }
    }
    let start_time = parse_required_field_instant("start_time", &existing.start_time, &mut reasons);
    let end_time = parse_required_field_instant("end_time", &existing.end_time, &mut reasons);
    if let Some(start_time) = start_time
        && backfill.contains_start(start_time) == Some(false)
    {
        reasons.push("outside_backfill_window".to_string());
    }
    if let (Some(start_time), Some(end_time)) = (start_time, end_time)
        && end_time <= start_time
    {
        reasons.push("end_time_not_after_start_time".to_string());
    }
    if !existing.goose_marker.starts_with("goose:")
        || !existing.idempotency_key.starts_with("goose:")
    {
        reasons.push("not_goose_owned".to_string());
    }
    if !platform_delete_supported(input.platform, &existing.destination_type) {
        reasons.push("unsupported_delete_mapping".to_string());
    }
    if !grants.contains(existing.destination_type.as_str()) {
        reasons.push("delete_permission_denied".to_string());
    }

    reasons
}

fn report_next_actions(
    issues: &[String],
    blocked_records: &[BlockedHealthRecord],
    blocked_deletes: &[BlockedHealthDelete],
) -> Vec<HealthSyncNextAction> {
    let mut actions = Vec::new();
    actions.extend(issues.iter().map(|issue| HealthSyncNextAction {
        scope: "health_sync_report".to_string(),
        reason: issue.clone(),
        action: health_sync_issue_action(issue),
    }));
    actions.extend(
        blocked_records
            .iter()
            .flat_map(|record| record.next_actions.clone()),
    );
    actions.extend(
        blocked_deletes
            .iter()
            .flat_map(|record| record.next_actions.clone()),
    );
    dedupe_health_next_actions(actions)
}

fn next_actions_for_health_reasons(
    scope: &str,
    reasons: &[String],
    platform: HealthPlatform,
    destination_type: &str,
) -> Vec<HealthSyncNextAction> {
    reasons
        .iter()
        .map(|reason| HealthSyncNextAction {
            scope: scope.to_string(),
            reason: reason.clone(),
            action: health_sync_reason_action(reason, platform, destination_type),
        })
        .collect()
}

fn health_sync_issue_action(issue: &str) -> String {
    if issue.starts_with("unsupported schema ") {
        return "Use goose.health-sync-dry-run.v1 input before planning Health sync.".to_string();
    }
    if issue == "backfill.start_required" {
        return "Choose a Health sync backfill start before planning Health sync.".to_string();
    }
    if issue == "backfill.end_required" {
        return "Choose a Health sync backfill end before planning Health sync.".to_string();
    }
    if issue == "backfill.start_invalid_timestamp" {
        return "Use an RFC3339 backfill start timestamp with a timezone offset.".to_string();
    }
    if issue == "backfill.end_invalid_timestamp" {
        return "Use an RFC3339 backfill end timestamp with a timezone offset.".to_string();
    }
    if issue == "backfill.start must be earlier than backfill.end" {
        return "Choose a Health sync backfill start before the end time.".to_string();
    }
    format!("Resolve Health sync issue {issue}, then rerun the dry run.")
}

fn health_sync_reason_action(
    reason: &str,
    platform: HealthPlatform,
    destination_type: &str,
) -> String {
    if let Some(field) = reason.strip_suffix("_required") {
        return format!("Fill {field} before this Health sync row can be planned.");
    }
    if let Some(field) = reason.strip_suffix("_invalid_timestamp") {
        return format!("Use an RFC3339 timestamp with a timezone offset for {field}.");
    }
    if let Some(expected) = reason.strip_prefix("unit_mismatch_expected_") {
        let unit = expected.replace("_per_", "/");
        return format!(
            "Convert or relabel this Health sync value to {unit} for {destination_type}."
        );
    }
    match reason {
        "permission_denied" => format!(
            "Request {destination_type} permission on {} and rerun the dry run.",
            health_platform_label(platform)
        ),
        "delete_permission_denied" => format!(
            "Request delete/read permission for {destination_type} on {} before cleanup.",
            health_platform_label(platform)
        ),
        "unsupported_mapping" => {
            "Add a defended HealthKit/Health Connect mapping for this semantic before syncing it."
                .to_string()
        }
        "unsupported_delete_mapping" => {
            "Leave this platform record untouched or add a supported Goose cleanup mapping."
                .to_string()
        }
        "outside_backfill_window" => {
            "Adjust the backfill window or leave this out-of-window record unsynced.".to_string()
        }
        "end_time_not_after_start_time" => {
            "Fix the record time range so end_time is after start_time.".to_string()
        }
        "not_user_approved" => {
            "Require explicit user approval before syncing this derived or algorithm value."
                .to_string()
        }
        "unsafe_source_kind" => {
            "Use a Goose-owned decoded, derived, user-confirmed, or manual source kind."
                .to_string()
        }
        "unsupported_activity_type_mapping" => {
            "Use a supported Goose activity, workout, or sleep type before planning this session."
                .to_string()
        }
        "activity_metric_time_range_incomplete" => {
            "Provide both activity metric start and end timestamps, or omit both for session-level metrics."
                .to_string()
        }
        "activity_metric_end_time_not_after_start_time" => {
            "Fix the activity metric time range so end_time is after start_time.".to_string()
        }
        "activity_interval_end_time_not_after_start_time" => {
            "Fix the activity interval time range so end_time is after start_time.".to_string()
        }
        "benchmark_only_algorithm_not_syncable" => {
            "Use a Goose-owned primary algorithm output instead of a benchmark/reference output."
                .to_string()
        }
        "missing_provenance" => {
            "Attach non-empty Goose provenance linking this value to owned data or an approved algorithm run."
                .to_string()
        }
        "provenance_must_be_object" => {
            "Store provenance as a non-empty JSON object before syncing.".to_string()
        }
        "private_api_provenance_not_syncable" => {
            "Replace private API provenance with user-owned capture/import provenance; Goose must not sync private API replay material."
                .to_string()
        }
        "official_whoop_label_not_syncable" => {
            "Keep official WHOOP labels as labels only; sync Goose outputs or decoded owned values instead."
                .to_string()
        }
        "platform_import_not_syncable" => {
            "Keep HealthKit and Health Connect values out of Goose local metrics; only sync Goose-owned decoded or derived outputs."
                .to_string()
        }
        "healthkit_rmssd_must_not_be_written_as_sdnn" => {
            "Do not write RMSSD to HealthKit SDNN; sync SDNN only if Goose can calculate SDNN, or use Health Connect RMSSD."
                .to_string()
        }
        "health_connect_has_no_sdnn_record" => {
            "Do not sync SDNN to Health Connect; use an RMSSD value with the Health Connect RMSSD mapping."
                .to_string()
        }
        "duplicate_idempotency_key" => {
            "Deduplicate candidate records or adjust source ids/time windows so each Health record has one idempotency key."
                .to_string()
        }
        "not_goose_owned" => {
            "Do not delete external platform records; cleanup is limited to Goose-owned records."
                .to_string()
        }
        _ => format!("Resolve Health sync blocker {reason}, then rerun the dry run."),
    }
}

fn health_platform_label(platform: HealthPlatform) -> &'static str {
    match platform {
        HealthPlatform::HealthKit => "HealthKit",
        HealthPlatform::HealthConnect => "Health Connect",
    }
}

fn dedupe_health_next_actions(actions: Vec<HealthSyncNextAction>) -> Vec<HealthSyncNextAction> {
    let mut deduped = Vec::new();
    for action in actions {
        if !deduped.iter().any(|existing| existing == &action) {
            deduped.push(action);
        }
    }
    deduped
}

fn validate_candidate_policy(
    candidate: &HealthSyncCandidate,
    platform: HealthPlatform,
    backfill: &ParsedHealthSyncWindow,
) -> Vec<String> {
    let mut reasons = Vec::new();

    for (name, value) in [
        ("record_id", candidate.record_id.as_str()),
        ("metric_family", candidate.metric_family.as_str()),
        ("semantic", candidate.semantic.as_str()),
        ("source_kind", candidate.source_kind.as_str()),
        ("start_time", candidate.start_time.as_str()),
        ("end_time", candidate.end_time.as_str()),
        ("unit", candidate.unit.as_str()),
    ] {
        if value.trim().is_empty() {
            reasons.push(format!("{name}_required"));
        }
    }
    if !candidate.value.is_finite() {
        reasons.push("value_not_finite".to_string());
    }
    let start_time =
        parse_required_field_instant("start_time", &candidate.start_time, &mut reasons);
    let end_time = parse_required_field_instant("end_time", &candidate.end_time, &mut reasons);
    if let Some(start_time) = start_time
        && backfill.contains_start(start_time) == Some(false)
    {
        reasons.push("outside_backfill_window".to_string());
    }
    if let (Some(start_time), Some(end_time)) = (start_time, end_time)
        && end_time <= start_time
    {
        reasons.push("end_time_not_after_start_time".to_string());
    }
    if !candidate.approved_by_user {
        reasons.push("not_user_approved".to_string());
    }
    if !is_syncable_source_kind(&candidate.source_kind) {
        reasons.push("unsafe_source_kind".to_string());
    }
    if candidate
        .algorithm_id
        .as_deref()
        .is_some_and(is_benchmark_only_algorithm_id)
    {
        reasons.push("benchmark_only_algorithm_not_syncable".to_string());
    }
    match &candidate.provenance {
        serde_json::Value::Object(object) if object.is_empty() => {
            reasons.push("missing_provenance".to_string());
        }
        serde_json::Value::Object(_) => {
            if contains_private_api_marker(&candidate.provenance) {
                reasons.push("private_api_provenance_not_syncable".to_string());
            }
            if contains_official_whoop_label_marker(&candidate.provenance) {
                reasons.push("official_whoop_label_not_syncable".to_string());
            }
            if contains_platform_import_marker(&candidate.provenance) {
                reasons.push("platform_import_not_syncable".to_string());
            }
        }
        serde_json::Value::Null => {
            reasons.push("missing_provenance".to_string());
        }
        _ => {
            reasons.push("provenance_must_be_object".to_string());
        }
    }
    if platform == HealthPlatform::HealthKit && candidate.semantic == "hrv_rmssd" {
        reasons.push("healthkit_rmssd_must_not_be_written_as_sdnn".to_string());
    }
    if platform == HealthPlatform::HealthConnect && candidate.semantic == "hrv_sdnn" {
        reasons.push("health_connect_has_no_sdnn_record".to_string());
    }

    reasons
}

fn is_syncable_source_kind(source_kind: &str) -> bool {
    matches!(
        source_kind,
        "decoded_raw" | "local_derived" | "user_approved_algorithm"
    )
}

fn is_benchmark_only_algorithm_id(algorithm_id: &str) -> bool {
    algorithm_id.starts_with("reference.")
}

fn contains_private_api_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            normalized_marker(key).contains("private_api") || contains_private_api_marker(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_private_api_marker),
        serde_json::Value::String(text) => normalized_marker(text).contains("private_api"),
        _ => false,
    }
}

fn contains_official_whoop_label_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized_key = normalized_marker(key);
            if matches!(
                normalized_key.as_str(),
                "label_source"
                    | "source_kind"
                    | "source"
                    | "record_source"
                    | "candidate_source"
                    | "origin"
            ) && value
                .as_str()
                .is_some_and(|text| is_official_whoop_label_value(&normalized_marker(text)))
            {
                return true;
            }

            if matches!(
                normalized_key.as_str(),
                "is_official_label" | "official_whoop_label" | "whoop_label"
            ) && value.as_bool() == Some(true)
            {
                return true;
            }

            contains_official_whoop_label_marker(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_official_whoop_label_marker),
        _ => false,
    }
}

fn contains_platform_import_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized_key = normalized_marker(key);
            if matches!(
                normalized_key.as_str(),
                "source"
                    | "source_kind"
                    | "record_source"
                    | "candidate_source"
                    | "origin"
                    | "platform"
                    | "source_platform"
                    | "import_source"
                    | "import_platform"
                    | "external_platform"
            ) && value
                .as_str()
                .is_some_and(|text| is_platform_import_marker(&normalized_marker(text)))
            {
                return true;
            }

            if normalized_key.contains("import_policy")
                && value
                    .as_str()
                    .is_some_and(|text| normalized_marker(text) == "external_history_context_only")
            {
                return true;
            }

            contains_platform_import_marker(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_platform_import_marker),
        serde_json::Value::String(text) => is_platform_import_marker(&normalized_marker(text)),
        _ => false,
    }
}

fn is_platform_import_marker(value: &str) -> bool {
    matches!(
        value,
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
    ) || value.starts_with("healthkit_")
        || value.starts_with("health_kit_")
        || value.contains("_healthkit_")
        || value.contains("_health_connect_")
}

fn is_official_whoop_label_value(value: &str) -> bool {
    matches!(
        value,
        "official_label"
            | "whoop_official"
            | "official_whoop"
            | "official_whoop_label"
            | "whoop_label"
            | "whoop_server"
            | "whoop_server_score"
            | "whoop_score"
            | "screenshot_imported_whoop_label"
            | "whoop_export_label"
    )
}

fn normalized_marker(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn platform_mapping(platform: HealthPlatform, semantic: &str) -> Option<PlatformMapping> {
    match (platform, semantic) {
        (HealthPlatform::HealthKit, "heart_rate") => Some(PlatformMapping {
            destination_type: "heartRate",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthKit, "resting_heart_rate") => Some(PlatformMapping {
            destination_type: "restingHeartRate",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthKit, "hrv_sdnn") => Some(PlatformMapping {
            destination_type: "heartRateVariabilitySDNN",
            required_unit: "ms",
        }),
        (HealthPlatform::HealthKit, "respiratory_rate") => Some(PlatformMapping {
            destination_type: "respiratoryRate",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthKit, "oxygen_saturation") => Some(PlatformMapping {
            destination_type: "oxygenSaturation",
            required_unit: "%",
        }),
        (HealthPlatform::HealthKit, "body_temperature") => Some(PlatformMapping {
            destination_type: "bodyTemperature",
            required_unit: "degC",
        }),
        (HealthPlatform::HealthKit, "steps") => Some(PlatformMapping {
            destination_type: "stepCount",
            required_unit: "count",
        }),
        (HealthPlatform::HealthKit, "active_energy") => Some(PlatformMapping {
            destination_type: "activeEnergyBurned",
            required_unit: "kcal",
        }),
        (HealthPlatform::HealthConnect, "heart_rate") => Some(PlatformMapping {
            destination_type: "HeartRateRecord",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthConnect, "resting_heart_rate") => Some(PlatformMapping {
            destination_type: "RestingHeartRateRecord",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthConnect, "hrv_rmssd") => Some(PlatformMapping {
            destination_type: "HeartRateVariabilityRmssdRecord",
            required_unit: "ms",
        }),
        (HealthPlatform::HealthConnect, "respiratory_rate") => Some(PlatformMapping {
            destination_type: "RespiratoryRateRecord",
            required_unit: "count/min",
        }),
        (HealthPlatform::HealthConnect, "oxygen_saturation") => Some(PlatformMapping {
            destination_type: "OxygenSaturationRecord",
            required_unit: "%",
        }),
        (HealthPlatform::HealthConnect, "skin_temperature") => Some(PlatformMapping {
            destination_type: "SkinTemperatureRecord",
            required_unit: "degC",
        }),
        (HealthPlatform::HealthConnect, "steps") => Some(PlatformMapping {
            destination_type: "StepsRecord",
            required_unit: "count",
        }),
        (HealthPlatform::HealthConnect, "active_energy") => Some(PlatformMapping {
            destination_type: "ActiveCaloriesBurnedRecord",
            required_unit: "kcal",
        }),
        _ => None,
    }
}

fn platform_delete_supported(platform: HealthPlatform, destination_type: &str) -> bool {
    matches!(
        (platform, destination_type),
        (HealthPlatform::HealthKit, "heartRate")
            | (HealthPlatform::HealthKit, "restingHeartRate")
            | (HealthPlatform::HealthKit, "heartRateVariabilitySDNN")
            | (HealthPlatform::HealthKit, "respiratoryRate")
            | (HealthPlatform::HealthKit, "oxygenSaturation")
            | (HealthPlatform::HealthKit, "bodyTemperature")
            | (HealthPlatform::HealthKit, "stepCount")
            | (HealthPlatform::HealthKit, "activeEnergyBurned")
            | (HealthPlatform::HealthConnect, "HeartRateRecord")
            | (HealthPlatform::HealthConnect, "RestingHeartRateRecord")
            | (
                HealthPlatform::HealthConnect,
                "HeartRateVariabilityRmssdRecord"
            )
            | (HealthPlatform::HealthConnect, "RespiratoryRateRecord")
            | (HealthPlatform::HealthConnect, "OxygenSaturationRecord")
            | (HealthPlatform::HealthConnect, "SkinTemperatureRecord")
            | (HealthPlatform::HealthConnect, "StepsRecord")
            | (HealthPlatform::HealthConnect, "ActiveCaloriesBurnedRecord")
    )
}

fn idempotency_key(
    platform: HealthPlatform,
    destination_type: &str,
    candidate: &HealthSyncCandidate,
) -> String {
    format!(
        "goose:{platform:?}:{destination_type}:{}:{}:{}",
        candidate.record_id, candidate.start_time, candidate.end_time
    )
}

fn goose_marker(candidate: &HealthSyncCandidate) -> String {
    format!(
        "goose:{}:{}:{}",
        candidate.metric_family,
        candidate.algorithm_id.as_deref().unwrap_or("raw"),
        candidate.record_id
    )
}
