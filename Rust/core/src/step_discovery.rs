use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    GooseError, GooseResult,
    store::{DecodedFrameRow, GooseStore},
    validation_labels::{
        OFFICIAL_WHOOP_LABEL_POLICY, official_label_policy_issue_action,
        official_label_policy_issues,
    },
};

pub const STEP_PACKET_DISCOVERY_REPORT_SCHEMA: &str = "goose.step-packet-discovery-report.v1";
pub const STEP_CAPTURE_VALIDATION_REPORT_SCHEMA: &str = "goose.step-capture-validation-report.v1";

#[derive(Debug, Clone, Copy)]
pub struct StepPacketDiscoveryOptions {
    pub max_candidate_fields: usize,
}

impl Default for StepPacketDiscoveryOptions {
    fn default() -> Self {
        Self {
            max_candidate_fields: 250,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepPacketDiscoveryReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start: String,
    pub end: String,
    pub decoded_frame_count: usize,
    pub inspected_frame_count: usize,
    pub skipped_frame_count: usize,
    pub candidate_field_count: usize,
    pub emitted_candidate_field_count: usize,
    pub explicit_step_counter_found: bool,
    pub monotonic_counter_candidate_count: usize,
    pub emitted_monotonic_counter_sample_count: usize,
    pub counter_delta_candidate_count: usize,
    pub monotonic_counter_delta_candidate_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_counter_delta: Option<StepCounterDeltaCandidate>,
    pub counter_deltas: Vec<StepCounterDeltaCandidate>,
    pub packet_family_counts: BTreeMap<String, usize>,
    pub inspected_packet_family_counts: BTreeMap<String, usize>,
    pub candidate_fields: Vec<StepPacketDiscoveryCandidate>,
    pub issues: Vec<String>,
    pub next_actions: Vec<StepPacketDiscoveryNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepPacketDiscoveryCandidate {
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub packet_type_name: Option<String>,
    pub packet_k: Option<u8>,
    pub domain: Option<String>,
    pub body_summary_kind: Option<String>,
    pub packet_family: String,
    pub json_path: String,
    pub field_name: String,
    pub value: Value,
    pub match_kind: String,
    pub source_kind_inference: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepPacketDiscoveryNextAction {
    pub reason: String,
    pub summary: String,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct StepCaptureValidationOptions {
    pub max_candidate_fields: usize,
    pub capture_kind: Option<String>,
    pub manual_step_delta: Option<i64>,
    pub official_whoop_step_delta: Option<i64>,
    pub tolerance_steps: i64,
    pub label_provenance: Option<Value>,
}

impl Default for StepCaptureValidationOptions {
    fn default() -> Self {
        Self {
            max_candidate_fields: 1000,
            capture_kind: None,
            manual_step_delta: None,
            official_whoop_step_delta: None,
            tolerance_steps: 10,
            label_provenance: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepCaptureValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub database_path: String,
    pub start: String,
    pub end: String,
    pub capture_kind: Option<String>,
    pub manual_step_delta: Option<i64>,
    pub official_whoop_step_delta: Option<i64>,
    pub tolerance_steps: i64,
    pub label_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_provenance: Option<Value>,
    pub discovery_pass: bool,
    pub explicit_step_counter_found: bool,
    pub decoded_frame_count: usize,
    pub inspected_frame_count: usize,
    pub counter_candidate_count: usize,
    pub monotonic_counter_candidate_count: usize,
    pub counter_delta_candidate_count: usize,
    pub matching_counter_delta_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_counter_delta: Option<StepCounterDeltaCandidate>,
    pub counter_deltas: Vec<StepCounterDeltaCandidate>,
    pub discovery: StepPacketDiscoveryReport,
    pub issues: Vec<String>,
    pub next_actions: Vec<StepPacketDiscoveryNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepCounterDeltaCandidate {
    pub rank: usize,
    pub selected: bool,
    pub selection_reason: String,
    pub match_kind: String,
    pub packet_family: String,
    pub json_path: String,
    pub field_name: String,
    pub source_kind_inference: String,
    pub sample_count: usize,
    pub first_frame_id: String,
    pub last_frame_id: String,
    pub first_captured_at: String,
    pub last_captured_at: String,
    pub first_value: i64,
    pub last_value: i64,
    pub delta: i64,
    pub monotonic_non_decreasing: bool,
    pub manual_delta_error: Option<i64>,
    pub official_delta_error: Option<i64>,
    pub matches_manual_label: Option<bool>,
    pub matches_official_label: Option<bool>,
    pub matches_all_provided_labels: bool,
}

struct FrameContext<'a> {
    row: &'a DecodedFrameRow,
    packet_k: Option<u8>,
    domain: Option<String>,
    body_summary_kind: Option<String>,
    packet_family: String,
}

struct FieldMatch {
    match_kind: &'static str,
    source_kind_inference: &'static str,
    reason: &'static str,
}

pub fn run_step_packet_discovery_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: StepPacketDiscoveryOptions,
) -> GooseResult<StepPacketDiscoveryReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    run_step_packet_discovery(&decoded_rows, database_path, start, end, options)
}

pub fn run_step_packet_discovery(
    decoded_rows: &[DecodedFrameRow],
    database_path: &str,
    start: &str,
    end: &str,
    options: StepPacketDiscoveryOptions,
) -> GooseResult<StepPacketDiscoveryReport> {
    let mut packet_family_counts = BTreeMap::new();
    let mut inspected_packet_family_counts = BTreeMap::new();
    let mut inspected_frame_count = 0;
    let mut candidate_field_count = 0;
    let mut candidate_fields = Vec::new();
    let mut numeric_counter_observations = Vec::new();

    for row in decoded_rows {
        let parsed_payload = parsed_payload_json(row)?;
        let context = frame_context(row, &parsed_payload);
        increment_count(&mut packet_family_counts, &context.packet_family);

        if !is_step_discovery_frame(row, &parsed_payload, &context) {
            continue;
        }

        inspected_frame_count += 1;
        increment_count(&mut inspected_packet_family_counts, &context.packet_family);
        scan_step_fields(
            &parsed_payload,
            "$",
            &context,
            &mut candidate_field_count,
            &mut candidate_fields,
            &mut numeric_counter_observations,
            options.max_candidate_fields,
        );
    }

    let (monotonic_counter_candidate_count, monotonic_counter_samples) =
        monotonic_counter_candidate_samples(&numeric_counter_observations);
    let emitted_monotonic_counter_sample_count = monotonic_counter_samples.len().min(
        options
            .max_candidate_fields
            .saturating_sub(candidate_fields.len()),
    );
    candidate_field_count += monotonic_counter_samples.len();
    for candidate in monotonic_counter_samples {
        if candidate_fields.len() < options.max_candidate_fields {
            candidate_fields.push(candidate);
        }
    }

    let explicit_step_counter_found = candidate_fields
        .iter()
        .any(|candidate| candidate.match_kind == "step_count");
    let counter_deltas = step_counter_deltas(&candidate_fields, None, None, 0);
    let selected_counter_delta = selected_counter_delta(&counter_deltas).cloned();
    let monotonic_counter_delta_candidate_count = counter_deltas
        .iter()
        .filter(|candidate| candidate.match_kind == "monotonic_counter_candidate")
        .count();
    let mut issues = Vec::new();
    if inspected_frame_count == 0 {
        issues.push("no_step_discovery_frames".to_string());
    }
    if candidate_field_count == 0 {
        issues.push("no_step_or_pedometer_fields_in_decoded_frames".to_string());
    }
    if !explicit_step_counter_found {
        issues.push("no_explicit_step_counter_field_found".to_string());
    }
    if monotonic_counter_candidate_count > 0 && !explicit_step_counter_found {
        issues.push("unnamed_monotonic_counter_candidates_found".to_string());
    }
    if candidate_field_count > candidate_fields.len() {
        issues.push("candidate_field_output_truncated".to_string());
    }
    let next_actions = step_discovery_next_actions(&issues, selected_counter_delta.as_ref());

    Ok(StepPacketDiscoveryReport {
        schema: STEP_PACKET_DISCOVERY_REPORT_SCHEMA.to_string(),
        generated_by: "goose-step-packet-discovery".to_string(),
        pass: explicit_step_counter_found
            && !issues
                .iter()
                .any(|issue| issue != "candidate_field_output_truncated"),
        database_path: database_path.to_string(),
        start: start.to_string(),
        end: end.to_string(),
        decoded_frame_count: decoded_rows.len(),
        inspected_frame_count,
        skipped_frame_count: decoded_rows.len().saturating_sub(inspected_frame_count),
        candidate_field_count,
        emitted_candidate_field_count: candidate_fields.len(),
        explicit_step_counter_found,
        monotonic_counter_candidate_count,
        emitted_monotonic_counter_sample_count,
        counter_delta_candidate_count: counter_deltas.len(),
        monotonic_counter_delta_candidate_count,
        selected_counter_delta,
        counter_deltas,
        packet_family_counts,
        inspected_packet_family_counts,
        candidate_fields,
        next_actions,
        issues,
    })
}

pub fn run_step_capture_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: StepCaptureValidationOptions,
) -> GooseResult<StepCaptureValidationReport> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    run_step_capture_validation(&decoded_rows, database_path, start, end, options)
}

pub fn run_step_capture_validation(
    decoded_rows: &[DecodedFrameRow],
    database_path: &str,
    start: &str,
    end: &str,
    options: StepCaptureValidationOptions,
) -> GooseResult<StepCaptureValidationReport> {
    let discovery = run_step_packet_discovery(
        decoded_rows,
        database_path,
        start,
        end,
        StepPacketDiscoveryOptions {
            max_candidate_fields: options.max_candidate_fields,
        },
    )?;
    let counter_deltas = step_counter_deltas(
        &discovery.candidate_fields,
        options.manual_step_delta,
        options.official_whoop_step_delta,
        options.tolerance_steps,
    );
    let counter_candidate_count = discovery
        .candidate_fields
        .iter()
        .filter(|candidate| candidate.match_kind == "step_count")
        .count();
    let matching_counter_delta_count = counter_deltas
        .iter()
        .filter(|candidate| candidate.matches_all_provided_labels)
        .count();
    let selected_counter_delta = selected_counter_delta(&counter_deltas).cloned();
    let mut issues = Vec::new();
    if options.manual_step_delta.is_none() && options.official_whoop_step_delta.is_none() {
        issues.push("no_step_delta_validation_label".to_string());
    }
    issues.extend(official_label_policy_issues(
        options.official_whoop_step_delta.is_some(),
        options.label_provenance.as_ref(),
    ));
    if !discovery.explicit_step_counter_found {
        issues.push("no_explicit_step_counter_field_found".to_string());
    }
    if counter_deltas.is_empty() {
        issues.push("no_counter_delta_candidates".to_string());
    }
    if !counter_deltas.is_empty() && matching_counter_delta_count == 0 {
        issues.push("no_counter_delta_matches_labels".to_string());
    }
    if counter_deltas
        .iter()
        .any(|candidate| !candidate.monotonic_non_decreasing)
    {
        issues.push("counter_delta_decreased_within_window".to_string());
    }
    if selected_counter_delta.as_ref().is_some_and(|candidate| {
        candidate.match_kind == "monotonic_counter_candidate"
            && candidate.matches_all_provided_labels
    }) {
        issues.push("matching_counter_delta_requires_parser_mapping".to_string());
    }
    for issue in &discovery.issues {
        if !issues.contains(issue) {
            issues.push(issue.clone());
        }
    }
    let next_actions =
        step_capture_validation_next_actions(&issues, selected_counter_delta.as_ref());

    Ok(StepCaptureValidationReport {
        schema: STEP_CAPTURE_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-step-capture-validator".to_string(),
        pass: issues.is_empty(),
        database_path: database_path.to_string(),
        start: start.to_string(),
        end: end.to_string(),
        capture_kind: options.capture_kind,
        manual_step_delta: options.manual_step_delta,
        official_whoop_step_delta: options.official_whoop_step_delta,
        tolerance_steps: options.tolerance_steps,
        label_policy: OFFICIAL_WHOOP_LABEL_POLICY.to_string(),
        label_provenance: options.label_provenance,
        discovery_pass: discovery.pass,
        explicit_step_counter_found: discovery.explicit_step_counter_found,
        decoded_frame_count: discovery.decoded_frame_count,
        inspected_frame_count: discovery.inspected_frame_count,
        counter_candidate_count,
        monotonic_counter_candidate_count: discovery.monotonic_counter_candidate_count,
        counter_delta_candidate_count: counter_deltas.len(),
        matching_counter_delta_count,
        selected_counter_delta,
        counter_deltas,
        next_actions,
        issues,
        discovery,
    })
}

fn parsed_payload_json(row: &DecodedFrameRow) -> GooseResult<Value> {
    serde_json::from_str(&row.parsed_payload_json).map_err(|error| {
        GooseError::message(format!(
            "frame {} has invalid parsed_payload_json: {error}",
            row.frame_id
        ))
    })
}

fn frame_context<'a>(row: &'a DecodedFrameRow, payload: &Value) -> FrameContext<'a> {
    let packet_k = payload
        .get("packet_k")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    let domain = string_field(payload, "domain");
    let body_summary_kind = payload
        .get("body_summary")
        .and_then(|body| string_field(body, "kind"));
    let packet_family = packet_family(
        row,
        packet_k,
        domain.as_deref(),
        body_summary_kind.as_deref(),
    );

    FrameContext {
        row,
        packet_k,
        domain,
        body_summary_kind,
        packet_family,
    }
}

fn packet_family(
    row: &DecodedFrameRow,
    packet_k: Option<u8>,
    domain: Option<&str>,
    body_summary_kind: Option<&str>,
) -> String {
    if let Some(packet_k) = packet_k {
        if let Some(domain) = domain.filter(|value| !value.trim().is_empty()) {
            return format!("K{packet_k}/{domain}");
        }
        if let Some(kind) = body_summary_kind.filter(|value| !value.trim().is_empty()) {
            return format!("K{packet_k}/{kind}");
        }
        return format!("K{packet_k}");
    }
    row.packet_type_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn is_step_discovery_frame(
    row: &DecodedFrameRow,
    payload: &Value,
    context: &FrameContext<'_>,
) -> bool {
    if matches!(context.packet_k, Some(10 | 11 | 21)) {
        return true;
    }
    let mut haystack = String::new();
    if let Some(packet_type_name) = &row.packet_type_name {
        haystack.push_str(packet_type_name);
        haystack.push(' ');
    }
    if let Some(domain) = &context.domain {
        haystack.push_str(domain);
        haystack.push(' ');
    }
    if let Some(kind) = &context.body_summary_kind {
        haystack.push_str(kind);
        haystack.push(' ');
    }
    if let Some(kind) = payload.get("kind").and_then(Value::as_str) {
        haystack.push_str(kind);
    }
    let haystack = haystack.to_ascii_lowercase();
    haystack.contains("history")
        || haystack.contains("debug")
        || haystack.contains("console")
        || haystack.contains("pedometer")
}

fn scan_step_fields(
    value: &Value,
    path: &str,
    context: &FrameContext<'_>,
    candidate_field_count: &mut usize,
    candidate_fields: &mut Vec<StepPacketDiscoveryCandidate>,
    numeric_counter_observations: &mut Vec<StepPacketDiscoveryCandidate>,
    max_candidate_fields: usize,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_path = object_path(path, key);
                let field_match = classify_step_field(key, &child_path, child);
                if let Some(field_match) = field_match {
                    *candidate_field_count += 1;
                    if candidate_fields.len() < max_candidate_fields {
                        candidate_fields.push(candidate_from_match(
                            context,
                            key,
                            &child_path,
                            child,
                            field_match,
                        ));
                    }
                } else if is_hidden_counter_candidate_field(key, &child_path, child) {
                    numeric_counter_observations.push(candidate_from_match(
                        context,
                        key,
                        &child_path,
                        child,
                        FieldMatch {
                            match_kind: "monotonic_counter_candidate",
                            source_kind_inference: "device_counter_candidate",
                            reason: "decoded numeric field is monotonic non-decreasing with positive delta across the capture window",
                        },
                    ));
                }
                scan_step_fields(
                    child,
                    &child_path,
                    context,
                    candidate_field_count,
                    candidate_fields,
                    numeric_counter_observations,
                    max_candidate_fields,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                scan_step_fields(
                    child,
                    &child_path,
                    context,
                    candidate_field_count,
                    candidate_fields,
                    numeric_counter_observations,
                    max_candidate_fields,
                );
            }
        }
        _ => {}
    }
}

fn classify_step_field(key: &str, path: &str, value: &Value) -> Option<FieldMatch> {
    let key_lower = key.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();
    let combined = format!("{path_lower} {key_lower}");
    if key_lower.contains("step") && value_is_scalar(value) {
        return Some(FieldMatch {
            match_kind: "step_count",
            source_kind_inference: "device_counter",
            reason: "decoded field name contains step",
        });
    }
    if key_lower.contains("cadence") && value_is_scalar(value) {
        return Some(FieldMatch {
            match_kind: "cadence",
            source_kind_inference: "device_sensor",
            reason: "decoded field name contains cadence",
        });
    }
    if key_lower.contains("activity") && value_is_scalar(value) {
        return Some(FieldMatch {
            match_kind: "activity_state",
            source_kind_inference: "device_sensor",
            reason: "decoded field name contains activity",
        });
    }
    if key_lower.contains("pedometer") {
        return Some(FieldMatch {
            match_kind: "pedometer_field",
            source_kind_inference: "device_sensor",
            reason: "decoded field name contains pedometer",
        });
    }
    if (key_lower.contains("threshold") || key_lower.contains("sensitivity"))
        && combined.contains("pedometer")
    {
        return Some(FieldMatch {
            match_kind: "pedometer_config",
            source_kind_inference: "device_sensor",
            reason: "decoded pedometer path contains threshold or sensitivity",
        });
    }
    None
}

fn is_hidden_counter_candidate_field(key: &str, path: &str, value: &Value) -> bool {
    if numeric_i64(value).is_none() {
        return false;
    }
    let path_lower = path.to_ascii_lowercase();
    if !path_lower.starts_with("$.body_summary.") {
        return false;
    }
    let key_lower = key.to_ascii_lowercase();
    let excluded_fragments = [
        "timestamp",
        "time",
        "packet",
        "sequence",
        "seq",
        "version",
        "crc",
        "length",
        "len",
        "byte",
        "sample_count",
        "axis_count",
        "gap",
        "hr",
        "bpm",
        "temp",
        "battery",
    ];
    !excluded_fragments
        .iter()
        .any(|fragment| key_lower.contains(fragment) || path_lower.contains(fragment))
}

fn candidate_from_match(
    context: &FrameContext<'_>,
    field_name: &str,
    json_path: &str,
    value: &Value,
    field_match: FieldMatch,
) -> StepPacketDiscoveryCandidate {
    StepPacketDiscoveryCandidate {
        frame_id: context.row.frame_id.clone(),
        evidence_id: context.row.evidence_id.clone(),
        captured_at: context.row.captured_at.clone(),
        packet_type_name: context.row.packet_type_name.clone(),
        packet_k: context.packet_k,
        domain: context.domain.clone(),
        body_summary_kind: context.body_summary_kind.clone(),
        packet_family: context.packet_family.clone(),
        json_path: json_path.to_string(),
        field_name: field_name.to_string(),
        value: value.clone(),
        match_kind: field_match.match_kind.to_string(),
        source_kind_inference: field_match.source_kind_inference.to_string(),
        reason: field_match.reason.to_string(),
    }
}

fn object_path(parent: &str, key: &str) -> String {
    if parent == "$" {
        format!("$.{key}")
    } else {
        format!("{parent}.{key}")
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn value_is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn monotonic_counter_candidate_samples(
    observations: &[StepPacketDiscoveryCandidate],
) -> (usize, Vec<StepPacketDiscoveryCandidate>) {
    let mut grouped: BTreeMap<(String, String, String), Vec<&StepPacketDiscoveryCandidate>> =
        BTreeMap::new();
    for observation in observations {
        grouped
            .entry((
                observation.packet_family.clone(),
                normalized_counter_json_path(&observation.json_path),
                observation.field_name.clone(),
            ))
            .or_default()
            .push(observation);
    }

    let mut candidate_group_count = 0;
    let mut candidate_samples = Vec::new();
    for (_key, mut samples) in grouped {
        if samples.len() < 2 {
            continue;
        }
        samples.sort_by(|left, right| {
            left.captured_at
                .cmp(&right.captured_at)
                .then_with(|| left.frame_id.cmp(&right.frame_id))
        });
        if !has_distinct_counter_observations(&samples) {
            continue;
        }
        let values = samples
            .iter()
            .filter_map(|candidate| numeric_i64(&candidate.value))
            .collect::<Vec<_>>();
        if values.len() != samples.len() {
            continue;
        }
        let Some(first_value) = values.first() else {
            continue;
        };
        let Some(last_value) = values.last() else {
            continue;
        };
        if *last_value <= *first_value {
            continue;
        }
        if !values.windows(2).all(|pair| pair[1] >= pair[0]) {
            continue;
        }
        candidate_group_count += 1;
        candidate_samples.extend(samples.into_iter().cloned());
    }

    (candidate_group_count, candidate_samples)
}

fn step_counter_deltas(
    candidates: &[StepPacketDiscoveryCandidate],
    manual_step_delta: Option<i64>,
    official_whoop_step_delta: Option<i64>,
    tolerance_steps: i64,
) -> Vec<StepCounterDeltaCandidate> {
    let mut grouped: BTreeMap<(String, String, String), Vec<&StepPacketDiscoveryCandidate>> =
        BTreeMap::new();
    for candidate in candidates {
        if !matches!(
            candidate.match_kind.as_str(),
            "step_count" | "monotonic_counter_candidate"
        ) || numeric_i64(&candidate.value).is_none()
        {
            continue;
        }
        grouped
            .entry((
                candidate.packet_family.clone(),
                normalized_counter_json_path(&candidate.json_path),
                candidate.field_name.clone(),
            ))
            .or_default()
            .push(candidate);
    }

    let mut deltas = grouped
        .into_iter()
        .filter_map(|((packet_family, json_path, field_name), mut samples)| {
            if samples.len() < 2 {
                return None;
            }
            samples.sort_by(|left, right| {
                left.captured_at
                    .cmp(&right.captured_at)
                    .then_with(|| left.frame_id.cmp(&right.frame_id))
            });
            if !has_distinct_counter_observations(&samples) {
                return None;
            }
            let first = samples.first()?;
            let last = samples.last()?;
            let first_value = numeric_i64(&first.value)?;
            let last_value = numeric_i64(&last.value)?;
            let values = samples
                .iter()
                .filter_map(|candidate| numeric_i64(&candidate.value))
                .collect::<Vec<_>>();
            let monotonic_non_decreasing = values.windows(2).all(|pair| pair[1] >= pair[0]);
            let delta = last_value - first_value;
            let manual_delta_error = manual_step_delta.map(|label| delta - label);
            let official_delta_error = official_whoop_step_delta.map(|label| delta - label);
            let matches_manual_label = manual_delta_error
                .map(|error| error.abs() <= tolerance_steps && monotonic_non_decreasing);
            let matches_official_label = official_delta_error
                .map(|error| error.abs() <= tolerance_steps && monotonic_non_decreasing);
            let matches_all_provided_labels = [matches_manual_label, matches_official_label]
                .into_iter()
                .flatten()
                .all(|matches| matches)
                && (matches_manual_label.is_some() || matches_official_label.is_some());

            Some(StepCounterDeltaCandidate {
                rank: 0,
                selected: false,
                selection_reason: String::new(),
                match_kind: first.match_kind.clone(),
                packet_family,
                json_path,
                field_name,
                source_kind_inference: first.source_kind_inference.clone(),
                sample_count: samples.len(),
                first_frame_id: first.frame_id.clone(),
                last_frame_id: last.frame_id.clone(),
                first_captured_at: first.captured_at.clone(),
                last_captured_at: last.captured_at.clone(),
                first_value,
                last_value,
                delta,
                monotonic_non_decreasing,
                manual_delta_error,
                official_delta_error,
                matches_manual_label,
                matches_official_label,
                matches_all_provided_labels,
            })
        })
        .collect::<Vec<_>>();
    rank_counter_deltas(&mut deltas);
    deltas
}

fn has_distinct_counter_observations(samples: &[&StepPacketDiscoveryCandidate]) -> bool {
    let Some(first) = samples.first() else {
        return false;
    };
    samples
        .iter()
        .any(|sample| sample.frame_id != first.frame_id || sample.captured_at != first.captured_at)
}

fn selected_counter_delta(
    deltas: &[StepCounterDeltaCandidate],
) -> Option<&StepCounterDeltaCandidate> {
    deltas.iter().find(|candidate| candidate.selected)
}

fn rank_counter_deltas(deltas: &mut [StepCounterDeltaCandidate]) {
    deltas.sort_by_key(|left| counter_delta_rank_key(left));
    for (index, delta) in deltas.iter_mut().enumerate() {
        delta.rank = index + 1;
        delta.selected = index == 0;
        delta.selection_reason = counter_delta_selection_reason(delta).to_string();
    }
}

fn counter_delta_rank_key(
    delta: &StepCounterDeltaCandidate,
) -> (u8, u8, u8, std::cmp::Reverse<usize>, String, String) {
    let labels_present =
        delta.matches_manual_label.is_some() || delta.matches_official_label.is_some();
    let label_rank = if labels_present {
        if delta.matches_all_provided_labels {
            0
        } else {
            2
        }
    } else {
        1
    };
    let kind_rank = if delta.match_kind == "step_count" {
        0
    } else {
        1
    };
    let monotonic_rank = if delta.monotonic_non_decreasing && delta.delta > 0 {
        0
    } else if delta.monotonic_non_decreasing {
        1
    } else {
        2
    };
    (
        label_rank,
        kind_rank,
        monotonic_rank,
        std::cmp::Reverse(delta.sample_count),
        delta.packet_family.clone(),
        delta.json_path.clone(),
    )
}

fn counter_delta_selection_reason(delta: &StepCounterDeltaCandidate) -> &'static str {
    if !delta.monotonic_non_decreasing {
        return "counter_delta_not_monotonic";
    }
    let labels_present =
        delta.matches_manual_label.is_some() || delta.matches_official_label.is_some();
    if delta.matches_all_provided_labels {
        if delta.match_kind == "step_count" {
            "explicit_step_counter_matches_labels"
        } else {
            "hidden_counter_matches_labels_requires_parser_mapping"
        }
    } else if labels_present {
        if delta.match_kind == "step_count" {
            "explicit_step_counter_label_mismatch"
        } else {
            "hidden_counter_label_mismatch"
        }
    } else if delta.match_kind == "step_count" {
        "explicit_step_counter_delta"
    } else {
        "hidden_monotonic_counter_delta"
    }
}

fn numeric_i64(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(value);
    }
    if let Some(value) = value.as_u64() {
        return i64::try_from(value).ok();
    }
    value.as_str()?.trim().parse::<i64>().ok()
}

fn normalized_counter_json_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '[' {
            normalized.push(character);
            continue;
        }

        let mut digits = String::new();
        while let Some(next) = chars.peek().copied() {
            if next.is_ascii_digit() {
                digits.push(next);
                chars.next();
            } else {
                break;
            }
        }

        if !digits.is_empty() && chars.peek() == Some(&']') {
            chars.next();
            normalized.push_str("[]");
        } else {
            normalized.push('[');
            normalized.push_str(&digits);
        }
    }
    normalized
}

fn selected_counter_delta_parser_action(
    selected_counter_delta: Option<&StepCounterDeltaCandidate>,
    fallback: &str,
) -> String {
    if let Some(delta) = selected_counter_delta {
        format!(
            "{fallback} Selected decoded path `{}` rank {} delta {} with reason `{}`.",
            delta.json_path, delta.rank, delta.delta, delta.selection_reason
        )
    } else {
        fallback.to_string()
    }
}

fn step_discovery_next_actions(
    issues: &[String],
    selected_counter_delta: Option<&StepCounterDeltaCandidate>,
) -> Vec<StepPacketDiscoveryNextAction> {
    let mut actions = Vec::new();
    for issue in issues {
        let (summary, action): (&str, String) = match issue.as_str() {
            "no_step_discovery_frames" => (
                "No K10/K11/K21/history/debug decoded frames in the selected window",
                "Run a controlled capture with realtime K10/K11 streams and a post-capture history sync.".to_string(),
            ),
            "no_step_or_pedometer_fields_in_decoded_frames" => (
                "Decoded frames did not expose step, cadence, activity, or pedometer fields",
                "Capture still, hand-motion, counted-step, walk, and stairs windows, then rerun step discovery before building an estimator.".to_string(),
            ),
            "no_explicit_step_counter_field_found" => (
                "No explicit decoded step counter was found",
                "Compare K11/K21/history payload deltas against manual counted steps and update the parser if a hidden counter byte range is identified.".to_string(),
            ),
            "unnamed_monotonic_counter_candidates_found" => (
                "Decoded numeric fields look like monotonic counter candidates but are not named as step counters",
                selected_counter_delta_parser_action(
                    selected_counter_delta,
                    "Compare these JSON paths against counted-step and WHOOP-app labels; if one matches, update the packet parser to expose it as step_count before persistence.",
                ),
            ),
            "candidate_field_output_truncated" => (
                "Candidate field output was truncated",
                "Rerun with a higher max_candidate_fields value or a narrower capture window.".to_string(),
            ),
            _ => (
                "Review step discovery issue",
                "Inspect the report issue and rerun with a narrower capture window.".to_string(),
            ),
        };
        actions.push(StepPacketDiscoveryNextAction {
            reason: issue.clone(),
            summary: summary.to_string(),
            action,
        });
    }
    actions
}

fn step_capture_validation_next_actions(
    issues: &[String],
    selected_counter_delta: Option<&StepCounterDeltaCandidate>,
) -> Vec<StepPacketDiscoveryNextAction> {
    let mut actions = Vec::new();
    for issue in issues {
        let (summary, action): (&str, String) = match issue.as_str() {
            _ if official_label_policy_issue_action(issue).is_some() => (
                "Official WHOOP label provenance is not marked as validation-only",
                official_label_policy_issue_action(issue).unwrap().to_string(),
            ),
            "no_step_delta_validation_label" => (
                "No manual or official-app step delta label was supplied",
                "Rerun the validator with manual_step_delta and/or official_whoop_step_delta for this capture window.".to_string(),
            ),
            "no_explicit_step_counter_field_found" => (
                "No explicit decoded step counter was found",
                "Run a controlled counted-step capture and inspect K11/K21/history bytes for an undiscovered counter field.".to_string(),
            ),
            "no_counter_delta_candidates" => (
                "Decoded step-like fields did not have at least two numeric samples",
                "Capture a longer window with repeated K11/K21/history packets, then rerun validation.".to_string(),
            ),
            "no_counter_delta_matches_labels" => (
                "No decoded counter delta matched the supplied validation labels",
                "Compare candidate byte ranges against the manual and official-app deltas before promoting any step counter.".to_string(),
            ),
            "counter_delta_decreased_within_window" => (
                "A decoded counter candidate decreased within the capture window",
                "Check for counter reset, reconnect, or a non-counter field before using this path.".to_string(),
            ),
            "matching_counter_delta_requires_parser_mapping" => (
                "An unnamed monotonic decoded field matched the supplied labels",
                selected_counter_delta_parser_action(
                    selected_counter_delta,
                    "Treat this as parser evidence only: rename the validated JSON path to step_count in the decoder, then rerun validation before writing device-counter samples.",
                ),
            ),
            "unnamed_monotonic_counter_candidates_found" => (
                "Decoded numeric fields look like monotonic counter candidates but are not named as step counters",
                selected_counter_delta_parser_action(
                    selected_counter_delta,
                    "Compare candidate JSON paths against labels and update the parser before promoting any device-counter step samples.",
                ),
            ),
            "candidate_field_output_truncated" => (
                "Candidate field output was truncated",
                "Rerun with a higher max_candidate_fields value or a narrower capture window.".to_string(),
            ),
            "no_step_discovery_frames" => (
                "No K10/K11/K21/history/debug decoded frames were available",
                "Import the controlled capture into a Goose SQLite store before validating step deltas.".to_string(),
            ),
            "no_step_or_pedometer_fields_in_decoded_frames" => (
                "Decoded frames did not expose step, cadence, activity, or pedometer fields",
                "Use this capture as evidence for a decode-path gap and inspect raw payload byte deltas.".to_string(),
            ),
            _ => (
                "Review step validation issue",
                "Inspect the validation report issue and rerun with a narrower capture window.".to_string(),
            ),
        };
        actions.push(StepPacketDiscoveryNextAction {
            reason: issue.clone(),
            summary: summary.to_string(),
            action,
        });
    }
    actions
}
