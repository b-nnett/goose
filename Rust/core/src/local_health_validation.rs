use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::PathBuf,
};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{GooseError, GooseResult};

pub const LOCAL_HEALTH_VALIDATION_MANIFEST_SCHEMA: &str =
    "goose.local-health-validation-manifest.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalHealthValidationManifestScaffoldOptions {
    pub database_path: PathBuf,
    pub manifest_id: String,
    pub timezone: String,
    #[serde(default)]
    pub date_key: Option<String>,
    #[serde(default)]
    pub database_source_kind: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub window_source: Option<String>,
    #[serde(default)]
    pub raw_export_bundle_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ScaffoldEvidenceSummary {
    observed_capture_session_ids: Vec<String>,
    raw_evidence_time_bounds: Option<LocalHealthValidationEvidenceTimeBounds>,
    decoded_frame_time_bounds: Option<LocalHealthValidationEvidenceTimeBounds>,
    packet_family_counts: BTreeMap<String, i64>,
    capture_session_summaries: Vec<LocalHealthValidationCaptureSessionSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct LocalHealthValidationEvidenceTimeBounds {
    first_captured_at: String,
    last_captured_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    span_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_offset_from_case_start_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_offset_before_case_end_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct LocalHealthValidationCaptureSessionSummary {
    session_id: String,
    raw_evidence_time_bounds: Option<LocalHealthValidationEvidenceTimeBounds>,
    decoded_frame_time_bounds: Option<LocalHealthValidationEvidenceTimeBounds>,
    packet_family_counts: BTreeMap<String, i64>,
}

pub fn scaffold_local_health_validation_manifest(
    options: &LocalHealthValidationManifestScaffoldOptions,
) -> GooseResult<Value> {
    let (start, end, window_source) = scaffold_time_window(options)?;
    let date_key = non_empty_string(options.date_key.as_deref())
        .or_else(|| date_key_from_rfc3339_start(&start))
        .unwrap_or_else(|| "fill-local-date".to_string());
    let evidence = scaffold_evidence_summary(&options.database_path, &start, &end)?;
    let cases = scaffold_validation_cases(&evidence.packet_family_counts);
    let run_validation = scaffold_run_validation(options);
    let operator_checklist = scaffold_operator_checklist(&evidence, &cases, &run_validation);

    let mut manifest = Map::new();
    manifest.insert(
        "schema".to_string(),
        json!(LOCAL_HEALTH_VALIDATION_MANIFEST_SCHEMA),
    );
    manifest.insert("manifest_id".to_string(), json!(options.manifest_id));
    manifest.insert(
        "notes".to_string(),
        json!(
            "Generated from Goose-owned packet evidence. Fill manual/official labels before treating labeled cases as acceptance."
        ),
    );
    manifest.insert("start".to_string(), json!(start));
    manifest.insert("end".to_string(), json!(end));
    manifest.insert("date_key".to_string(), json!(date_key));
    manifest.insert("timezone".to_string(), json!(options.timezone));
    manifest.insert("min_owned_captures".to_string(), json!(1));
    manifest.insert(
        "label_provenance".to_string(),
        json!({
            "source": "fill_manual_and_official_whoop_app_labels",
            "official_labels_are_labels": true
        }),
    );
    match evidence.observed_capture_session_ids.as_slice() {
        [session_id] => {
            manifest.insert("capture_session_id".to_string(), json!(session_id));
        }
        [] => {}
        _ => {}
    }
    let capture_session_default = match evidence.observed_capture_session_ids.len() {
        0 => "not_available",
        1 => "single_session_defaulted",
        _ => "multiple_sessions_observed_case_binding_required",
    };
    manifest.insert(
        "generated_evidence".to_string(),
        json!({
            "database_source_kind": options.database_source_kind.as_deref().unwrap_or("direct_database"),
            "database_path": options.database_path.display().to_string(),
            "raw_export_bundle_path": options.raw_export_bundle_path.as_ref().map(|path| path.display().to_string()),
            "window_source": window_source,
            "capture_session_default": capture_session_default,
            "observed_capture_session_ids": evidence.observed_capture_session_ids,
            "raw_evidence_time_bounds": evidence.raw_evidence_time_bounds,
            "decoded_frame_time_bounds": evidence.decoded_frame_time_bounds,
            "packet_family_counts": evidence.packet_family_counts,
            "capture_session_summaries": evidence.capture_session_summaries
        }),
    );
    manifest.insert(
        "operator_checklist".to_string(),
        Value::Array(operator_checklist),
    );
    manifest.insert("run_validation".to_string(), run_validation);
    manifest.insert("cases".to_string(), Value::Array(cases));
    Ok(Value::Object(manifest))
}

pub fn local_health_validation_manifest_runbook_markdown(manifest: &Value) -> String {
    let mut markdown = String::new();
    let object = manifest.as_object();
    let evidence = object
        .and_then(|object| object.get("generated_evidence"))
        .and_then(Value::as_object);
    let run_validation = object
        .and_then(|object| object.get("run_validation"))
        .and_then(Value::as_object);
    let label_provenance = object
        .and_then(|object| object.get("label_provenance"))
        .and_then(Value::as_object);

    let manifest_id = object
        .and_then(|object| object.get("manifest_id"))
        .and_then(value_string)
        .unwrap_or_else(|| "unnamed".to_string());
    let schema = object
        .and_then(|object| object.get("schema"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let start = object
        .and_then(|object| object.get("start"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let end = object
        .and_then(|object| object.get("end"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let timezone = object
        .and_then(|object| object.get("timezone"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let date_key = object
        .and_then(|object| object.get("date_key"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let official_labels_are_labels = label_provenance
        .and_then(|label_provenance| label_provenance.get("official_labels_are_labels"))
        .and_then(value_string)
        .unwrap_or_else(|| "true".to_string());
    let command = run_validation
        .and_then(|run_validation| run_validation.get("command"))
        .and_then(value_string)
        .unwrap_or_else(|| {
            "goose-local-health-validation-suite --manifest local-health-validation-manifest.json"
                .to_string()
        });
    let json_report_path = run_validation
        .and_then(|run_validation| run_validation.get("json_report_path"))
        .and_then(value_string)
        .unwrap_or_else(|| "local-health-validation-report.json".to_string());
    let markdown_report_path = run_validation
        .and_then(|run_validation| run_validation.get("markdown_report_path"))
        .and_then(value_string)
        .unwrap_or_else(|| "local-health-validation-report.md".to_string());
    let review_report_path = run_validation
        .and_then(|run_validation| run_validation.get("review_report_path"))
        .and_then(value_string)
        .unwrap_or_else(|| "local-health-validation-review.json".to_string());

    let _ = writeln!(markdown, "# Local Health Validation Runbook");
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "- Manifest: {}", markdown_inline(&manifest_id));
    let _ = writeln!(markdown, "- Schema: `{}`", markdown_inline(&schema));
    let _ = writeln!(
        markdown,
        "- Window: `{}` to `{}`",
        markdown_inline(&start),
        markdown_inline(&end)
    );
    let _ = writeln!(markdown, "- Timezone: `{}`", markdown_inline(&timezone));
    let _ = writeln!(markdown, "- Date key: `{}`", markdown_inline(&date_key));
    let _ = writeln!(
        markdown,
        "- Official WHOOP values are labels only: `{}`",
        markdown_inline(&official_labels_are_labels)
    );
    if let Some(bundle_path) = evidence
        .and_then(|evidence| evidence.get("raw_export_bundle_path"))
        .and_then(value_string)
    {
        let _ = writeln!(
            markdown,
            "- Raw Export bundle: `{}`",
            markdown_inline(&bundle_path)
        );
    }
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Run Validation");
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "```sh");
    let _ = writeln!(markdown, "{command}");
    let _ = writeln!(markdown, "```");
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "- JSON report: `{}`",
        markdown_inline(&json_report_path)
    );
    let _ = writeln!(
        markdown,
        "- Markdown report: `{}`",
        markdown_inline(&markdown_report_path)
    );
    let _ = writeln!(
        markdown,
        "- Manifest review: `{}`",
        markdown_inline(&review_report_path)
    );

    append_runbook_operator_checklist(
        &mut markdown,
        object
            .and_then(|object| object.get("operator_checklist"))
            .and_then(Value::as_array),
    );
    let review = review_local_health_validation_manifest(manifest);
    append_runbook_acceptance_evidence_checklist(&mut markdown, &review);
    append_runbook_capture_sqlite_imports(&mut markdown, &review);
    append_runbook_capture_session_resolution(&mut markdown, &review);
    append_runbook_capture_session_packet_families(&mut markdown, &review);
    append_runbook_case_window_evidence(&mut markdown, &review);
    append_runbook_validation_labels(&mut markdown, &review);
    append_runbook_capture_session_binding(
        &mut markdown,
        review
            .get("capture_session_binding_required_cases")
            .and_then(Value::as_array),
    );
    append_runbook_generated_evidence(&mut markdown, evidence);
    append_runbook_placeholder_cases(
        &mut markdown,
        object
            .and_then(|object| object.get("cases"))
            .and_then(Value::as_array),
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "WHOOP app values and screenshots are validation labels only. They must not be used as metric inputs or provenance."
    );
    markdown
}

pub fn review_local_health_validation_manifest(manifest: &Value) -> Value {
    let object = manifest.as_object();
    let manifest_id = object
        .and_then(|object| object.get("manifest_id"))
        .and_then(value_string)
        .unwrap_or_else(|| "unnamed".to_string());
    let manifest_schema = object
        .and_then(|object| object.get("schema"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let schema_valid = manifest_schema == LOCAL_HEALTH_VALIDATION_MANIFEST_SCHEMA;
    let top_level_label_policy_valid = object
        .and_then(|object| object.get("label_provenance"))
        .and_then(Value::as_object)
        .and_then(|policy| policy.get("official_labels_are_labels"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let evidence = object
        .and_then(|object| object.get("generated_evidence"))
        .and_then(Value::as_object);
    let observed_capture_session_ids = evidence
        .and_then(|evidence| evidence.get("observed_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let capture_session_default = evidence
        .and_then(|evidence| evidence.get("capture_session_default"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_start = object
        .and_then(|object| object.get("start"))
        .and_then(value_string);
    let manifest_end = object
        .and_then(|object| object.get("end"))
        .and_then(value_string);
    let top_level_capture_session_ids = object
        .map(manifest_review_declared_capture_session_ids)
        .unwrap_or_default();
    let top_level_capture_bound = !top_level_capture_session_ids.is_empty();
    let (capture_sqlite_import_summaries, capture_sqlite_import_session_ids) =
        manifest_review_capture_sqlite_imports(object);
    let known_capture_session_ids = manifest_review_known_capture_session_ids(
        &observed_capture_session_ids,
        &capture_sqlite_import_summaries,
    );
    let capture_sqlite_import_invalid_cases = capture_sqlite_import_summaries
        .iter()
        .filter(|summary| {
            summary.get("status").and_then(value_string).as_deref() == Some("invalid")
        })
        .cloned()
        .collect::<Vec<_>>();
    let cases = object
        .and_then(|object| object.get("cases"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut placeholder_case_summaries = Vec::new();
    let mut placeholder_field_names = BTreeSet::new();
    let mut capture_session_binding_required_cases = Vec::new();
    let mut capture_session_unresolved_cases = Vec::new();
    let mut capture_session_unverified_cases = Vec::new();
    let mut capture_session_packet_family_gap_cases = Vec::new();
    let mut case_window_evidence_gap_cases = Vec::new();
    let mut official_label_case_count = 0usize;
    let mut official_label_policy_missing_cases = Vec::new();
    let mut official_label_required_case_count = 0usize;
    let mut official_label_missing_cases = Vec::new();
    let mut manual_label_required_case_count = 0usize;
    let mut manual_label_missing_cases = Vec::new();
    for case in &cases {
        let Some(case_object) = case.as_object() else {
            continue;
        };
        let case_id = case_object
            .get("id")
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let report = case_object
            .get("report")
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let normalized_report = manifest_review_normalized_report(&report);
        let official_label_fields =
            manifest_review_required_official_label_fields(normalized_report.as_deref());
        if !official_label_fields.is_empty() {
            official_label_required_case_count += 1;
            if !manifest_review_case_has_any_non_null_field(case_object, &official_label_fields) {
                official_label_missing_cases.push(json!({
                    "case_id": case_id.clone(),
                    "report": report.clone(),
                    "normalized_report": normalized_report.clone(),
                    "required_label_kind": "official_whoop_app",
                    "required_label_mode": "any",
                    "required_label_fields": official_label_fields
                }));
            }
        }
        let manual_label_fields =
            manifest_review_required_manual_label_fields(normalized_report.as_deref());
        if !manual_label_fields.is_empty() {
            manual_label_required_case_count += 1;
            if !manifest_review_case_has_all_non_null_fields(case_object, &manual_label_fields) {
                manual_label_missing_cases.push(json!({
                    "case_id": case_id.clone(),
                    "report": report.clone(),
                    "normalized_report": normalized_report.clone(),
                    "required_label_kind": "manual_count",
                    "required_label_mode": "all",
                    "required_label_fields": manual_label_fields
                }));
            }
        }
        let has_official_label_field = case_object
            .keys()
            .any(|key| key.starts_with("official_whoop_"));
        if has_official_label_field {
            official_label_case_count += 1;
            let case_label_policy_valid = case_object
                .get("label_provenance")
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("official_labels_are_labels"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !top_level_label_policy_valid && !case_label_policy_valid {
                official_label_policy_missing_cases.push(json!({
                    "case_id": case_id.clone(),
                    "report": report.clone()
                }));
            }
        }
        let fields = case_object
            .iter()
            .filter_map(|(key, value)| value.is_null().then_some(key.clone()))
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            for field in &fields {
                placeholder_field_names.insert(field.clone());
            }
            placeholder_case_summaries.push(json!({
                "case_id": case_id,
                "report": report,
                "fields": fields
            }));
        }
        let case_declared_session_ids = manifest_review_declared_capture_session_ids(case_object);
        let (declared_capture_session_ids, declared_capture_session_source) =
            if case_declared_session_ids.is_empty() {
                (top_level_capture_session_ids.clone(), "manifest")
            } else {
                (case_declared_session_ids, "case")
            };
        let case_capture_bound = !declared_capture_session_ids.is_empty();
        let case_window = manifest_review_case_window(
            case_object,
            manifest_start.as_deref(),
            manifest_end.as_deref(),
        );
        if manifest_review_requires_capture_session_binding(normalized_report.as_deref())
            && !case_capture_bound
        {
            let required_packet_family_prefixes =
                manifest_review_required_packet_family_prefixes(normalized_report.as_deref());
            let suggested_capture_sessions = manifest_review_suggested_capture_sessions(
                evidence,
                &required_packet_family_prefixes,
                case_window.as_ref(),
            );
            let suggested_capture_session_ids = suggested_capture_sessions
                .iter()
                .filter_map(|session| session.get("session_id").and_then(value_string))
                .collect::<Vec<_>>();
            capture_session_binding_required_cases.push(json!({
                "case_id": case_id,
                "report": report,
                "normalized_report": normalized_report,
                "case_window_start": case_window.as_ref().map(|(start, _end)| start),
                "case_window_end": case_window.as_ref().map(|(_start, end)| end),
                "required_packet_family_prefixes": required_packet_family_prefixes,
                "suggested_capture_session_count": suggested_capture_session_ids.len(),
                "suggested_capture_session_ids": suggested_capture_session_ids,
                "suggested_capture_sessions": suggested_capture_sessions
            }));
        }
        if manifest_review_requires_capture_session_binding(normalized_report.as_deref())
            && case_capture_bound
            && !known_capture_session_ids.is_empty()
        {
            let missing_capture_session_ids = declared_capture_session_ids
                .iter()
                .filter(|session_id| !known_capture_session_ids.contains(*session_id))
                .cloned()
                .collect::<Vec<_>>();
            if !missing_capture_session_ids.is_empty() {
                capture_session_unresolved_cases.push(json!({
                    "case_id": case_id,
                    "report": report,
                    "normalized_report": normalized_report,
                    "declared_capture_session_source": declared_capture_session_source,
                    "declared_capture_session_ids": declared_capture_session_ids,
                    "missing_capture_session_ids": missing_capture_session_ids,
                    "known_capture_session_ids": known_capture_session_ids.clone()
                }));
            } else {
                let required_packet_family_prefixes =
                    manifest_review_required_packet_family_prefixes(normalized_report.as_deref());
                if let Some(packet_family_gap) = manifest_review_capture_session_packet_family_gap(
                    evidence,
                    &declared_capture_session_ids,
                    &required_packet_family_prefixes,
                ) {
                    capture_session_packet_family_gap_cases.push(json!({
                        "case_id": case_id,
                        "report": report,
                        "normalized_report": normalized_report,
                        "declared_capture_session_source": declared_capture_session_source,
                        "declared_capture_session_ids": declared_capture_session_ids,
                        "required_packet_family_prefixes": required_packet_family_prefixes,
                        "declared_session_count": packet_family_gap
                            .get("declared_session_count")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "declared_session_packet_family_counts": packet_family_gap
                            .get("declared_session_packet_family_counts")
                            .cloned()
                            .unwrap_or(Value::Null)
                    }));
                }
            }
        }
        if manifest_review_requires_capture_session_binding(normalized_report.as_deref())
            && case_capture_bound
            && known_capture_session_ids.is_empty()
        {
            capture_session_unverified_cases.push(json!({
                "case_id": case_id,
                "report": report,
                "normalized_report": normalized_report,
                "declared_capture_session_source": declared_capture_session_source,
                "declared_capture_session_ids": declared_capture_session_ids,
                "resolution_status": "unverified_no_known_sessions",
                "reason": "manifest_has_no_generated_evidence_or_valid_capture_sqlite_imports"
            }));
        }
        if manifest_review_requires_capture_session_binding(normalized_report.as_deref())
            && let Some(evidence_gap) =
                manifest_review_case_window_evidence_gap(evidence, case_window.as_ref())
        {
            case_window_evidence_gap_cases.push(json!({
                "case_id": case_id,
                "report": report,
                "normalized_report": normalized_report,
                "case_window_start": case_window.as_ref().map(|(start, _end)| start),
                "case_window_end": case_window.as_ref().map(|(_start, end)| end),
                "evidence_overlap_status": evidence_gap
                    .get("evidence_overlap_status")
                    .cloned()
                    .unwrap_or(Value::Null),
                "evidence_bounds": evidence_gap
                    .get("evidence_bounds")
                    .cloned()
                    .unwrap_or(Value::Null)
            }));
        }
    }
    let run_validation = object
        .and_then(|object| object.get("run_validation"))
        .and_then(Value::as_object);
    let run_validation_args = run_validation
        .and_then(|run_validation| run_validation.get("args"))
        .and_then(Value::as_array)
        .map(|args| args.iter().filter_map(value_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let generated_command_writes_json = run_validation_args.iter().any(|arg| arg == "--output")
        && run_validation
            .and_then(|run_validation| run_validation.get("json_report_path"))
            .and_then(value_string)
            .is_some();
    let generated_command_writes_markdown = run_validation_args
        .iter()
        .any(|arg| arg == "--markdown-output")
        && run_validation
            .and_then(|run_validation| run_validation.get("markdown_report_path"))
            .and_then(value_string)
            .is_some();
    let generated_command_writes_review = run_validation_args
        .iter()
        .any(|arg| arg == "--review-output")
        && run_validation
            .and_then(|run_validation| run_validation.get("review_report_path"))
            .and_then(value_string)
            .is_some();
    let generated_command_present = run_validation.is_some();
    let official_label_policy_required = official_label_case_count > 0;
    let label_policy_valid =
        !official_label_policy_required || official_label_policy_missing_cases.is_empty();
    let placeholder_field_count: usize = placeholder_case_summaries
        .iter()
        .filter_map(|summary| summary.get("fields").and_then(Value::as_array))
        .map(Vec::len)
        .sum();
    let acceptance_evidence_cases = manifest_review_acceptance_evidence_cases(
        object,
        evidence,
        &known_capture_session_ids,
        &top_level_capture_session_ids,
        manifest_start.as_deref(),
        manifest_end.as_deref(),
    );
    let acceptance_evidence_open_case_count = acceptance_evidence_cases
        .iter()
        .filter(|case| {
            case.get("outstanding_requirements")
                .and_then(Value::as_array)
                .is_some_and(|requirements| !requirements.is_empty())
        })
        .count();
    let mut blockers = Vec::new();
    if !schema_valid {
        blockers.push("manifest_schema_invalid");
    }
    if cases.is_empty() {
        blockers.push("no_validation_cases");
    }
    if !label_policy_valid {
        blockers.push("official_label_policy_missing_or_false");
    }
    if !official_label_missing_cases.is_empty() {
        blockers.push("validation_official_labels_missing");
    }
    if !manual_label_missing_cases.is_empty() {
        blockers.push("validation_manual_labels_missing");
    }
    if placeholder_field_count > 0 {
        blockers.push("validation_placeholders_unfilled");
    }
    if !capture_session_binding_required_cases.is_empty() {
        blockers.push("capture_session_binding_required");
    }
    if !capture_session_unresolved_cases.is_empty() {
        blockers.push("capture_session_declared_ids_unresolved");
    }
    if !capture_session_packet_family_gap_cases.is_empty() {
        blockers.push("capture_session_packet_family_unrelated");
    }
    if !case_window_evidence_gap_cases.is_empty() {
        blockers.push("case_window_outside_generated_evidence");
    }
    if !capture_sqlite_import_invalid_cases.is_empty() {
        blockers.push("capture_sqlite_import_declaration_invalid");
    }
    if generated_command_present && !generated_command_writes_json {
        blockers.push("validation_command_missing_json_output");
    }
    if generated_command_present && !generated_command_writes_markdown {
        blockers.push("validation_command_missing_markdown_output");
    }
    if generated_command_present && !generated_command_writes_review {
        blockers.push("validation_command_missing_review_output");
    }
    let next_actions = manifest_review_next_actions(&blockers);
    let status = if blockers.is_empty() {
        "ready_to_run_validation_suite"
    } else {
        "operator_edits_required"
    };
    json!({
        "schema": "goose.local-health-validation-manifest-review.v1",
        "manifest_schema": manifest_schema,
        "manifest_id": manifest_id,
        "status": status,
        "schema_valid": schema_valid,
        "official_label_policy_required": official_label_policy_required,
        "label_policy_valid": label_policy_valid,
        "official_label_case_count": official_label_case_count,
        "official_label_policy_missing_case_count": official_label_policy_missing_cases.len(),
        "official_label_policy_missing_cases": official_label_policy_missing_cases,
        "official_label_required_case_count": official_label_required_case_count,
        "official_label_missing_case_count": official_label_missing_cases.len(),
        "official_label_missing_cases": official_label_missing_cases,
        "manual_label_required_case_count": manual_label_required_case_count,
        "manual_label_missing_case_count": manual_label_missing_cases.len(),
        "manual_label_missing_cases": manual_label_missing_cases,
        "case_count": cases.len(),
        "acceptance_evidence_case_count": acceptance_evidence_cases.len(),
        "acceptance_evidence_open_case_count": acceptance_evidence_open_case_count,
        "acceptance_evidence_cases": acceptance_evidence_cases,
        "placeholder_case_count": placeholder_case_summaries.len(),
        "placeholder_field_count": placeholder_field_count,
        "placeholder_fields": placeholder_field_names.into_iter().collect::<Vec<_>>(),
        "placeholder_cases": placeholder_case_summaries,
        "observed_capture_session_count": observed_capture_session_ids.len(),
        "observed_capture_session_ids": observed_capture_session_ids,
        "capture_session_default": capture_session_default,
        "capture_sqlite_import_count": capture_sqlite_import_summaries.len(),
        "capture_sqlite_import_session_ids": capture_sqlite_import_session_ids,
        "capture_sqlite_import_invalid_count": capture_sqlite_import_invalid_cases.len(),
        "capture_sqlite_import_invalid_cases": capture_sqlite_import_invalid_cases,
        "capture_sqlite_imports": capture_sqlite_import_summaries,
        "known_capture_session_ids": known_capture_session_ids,
        "top_level_capture_session_bound": top_level_capture_bound,
        "capture_session_binding_required_case_count": capture_session_binding_required_cases.len(),
        "capture_session_binding_required_cases": capture_session_binding_required_cases,
        "capture_session_unresolved_case_count": capture_session_unresolved_cases.len(),
        "capture_session_unresolved_cases": capture_session_unresolved_cases,
        "capture_session_unverified_case_count": capture_session_unverified_cases.len(),
        "capture_session_unverified_cases": capture_session_unverified_cases,
        "capture_session_packet_family_gap_case_count": capture_session_packet_family_gap_cases.len(),
        "capture_session_packet_family_gap_cases": capture_session_packet_family_gap_cases,
        "case_window_evidence_gap_case_count": case_window_evidence_gap_cases.len(),
        "case_window_evidence_gap_cases": case_window_evidence_gap_cases,
        "generated_command_present": generated_command_present,
        "generated_command_writes_json": generated_command_writes_json,
        "generated_command_writes_markdown": generated_command_writes_markdown,
        "generated_command_writes_review": generated_command_writes_review,
        "blockers": blockers,
        "next_actions": next_actions
    })
}

fn manifest_review_next_actions(blockers: &[&str]) -> Vec<Value> {
    blockers
        .iter()
        .map(|blocker| {
            let action = match *blocker {
                "manifest_schema_invalid" => {
                    "Regenerate the validation manifest with goose.local-health-validation-manifest.v1."
                }
                "no_validation_cases" => {
                    "Regenerate the validation manifest from a Raw Export bundle or add validation cases before running acceptance."
                }
                "official_label_policy_missing_or_false" => {
                    "Set label_provenance.official_labels_are_labels=true before adding WHOOP app comparison values."
                }
                "validation_official_labels_missing" => {
                    "Fill required WHOOP app comparison labels for each validation report; these labels are for validation only."
                }
                "validation_manual_labels_missing" => {
                    "Fill required manual counted labels for step validation reports before using them as acceptance evidence."
                }
                "validation_placeholders_unfilled" => {
                    "Fill generated placeholder fields from manual counts, profile values, and WHOOP app screenshots used only as labels."
                }
                "capture_session_binding_required" => {
                    "Bind each owned controlled-capture case to the intended capture_session_id before treating it as acceptance evidence."
                }
                "capture_session_declared_ids_unresolved" => {
                    "Fix capture_session_id or capture_session_ids values that do not match any observed or imported owned capture session."
                }
                "capture_session_packet_family_unrelated" => {
                    "Bind each case to an owned capture session whose decoded packet families match the metric being accepted."
                }
                "case_window_outside_generated_evidence" => {
                    "Adjust case start/end or regenerate a Raw Export bundle whose evidence overlaps the validation case window."
                }
                "capture_sqlite_import_declaration_invalid" => {
                    "Fix capture_sqlite_imports entries so every processed capture.sqlite import has id, path, and session_id."
                }
                "validation_command_missing_json_output" => {
                    "Regenerate the scaffold so run_validation writes local-health-validation-report.json."
                }
                "validation_command_missing_markdown_output" => {
                    "Regenerate the scaffold so run_validation writes local-health-validation-report.md."
                }
                "validation_command_missing_review_output" => {
                    "Regenerate the scaffold so run_validation writes local-health-validation-review.json."
                }
                _ => "Review the generated validation manifest before running acceptance.",
            };
            json!({
                "reason": blocker,
                "action": action
            })
        })
        .collect()
}

fn manifest_review_capture_sqlite_imports(
    object: Option<&Map<String, Value>>,
) -> (Vec<Value>, Vec<String>) {
    let imports = object
        .and_then(|object| object.get("capture_sqlite_imports"))
        .and_then(Value::as_array);
    let mut session_ids = BTreeSet::new();
    let summaries = imports
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, import)| {
            let import_object = import.as_object();
            let id = import_object
                .and_then(|object| object.get("id"))
                .and_then(non_empty_value_string);
            let path = import_object
                .and_then(|object| {
                    object
                        .get("path")
                        .or_else(|| object.get("capture_sqlite_path"))
                })
                .and_then(non_empty_value_string);
            let session_id = import_object
                .and_then(|object| object.get("session_id"))
                .and_then(non_empty_value_string);
            if let Some(session_id) = &session_id {
                session_ids.insert(session_id.clone());
            }
            let mut issues = Vec::new();
            if import_object.is_none() {
                issues.push("capture_sqlite_import_object_required".to_string());
            }
            if id.is_none() {
                issues.push("capture_sqlite_import_id_required".to_string());
            }
            if path.is_none() {
                issues.push("capture_sqlite_import_path_required".to_string());
            }
            if session_id.is_none() {
                issues.push("capture_sqlite_import_session_id_required".to_string());
            }
            json!({
                "index": index,
                "id": id,
                "path": path,
                "session_id": session_id,
                "status": if issues.is_empty() { "declared" } else { "invalid" },
                "issues": issues
            })
        })
        .collect::<Vec<_>>();
    (summaries, session_ids.into_iter().collect())
}

fn manifest_review_known_capture_session_ids(
    observed_capture_session_ids: &[String],
    capture_sqlite_import_summaries: &[Value],
) -> Vec<String> {
    let mut ids = observed_capture_session_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for summary in capture_sqlite_import_summaries {
        let Some(object) = summary.as_object() else {
            continue;
        };
        if object.get("status").and_then(value_string).as_deref() != Some("declared") {
            continue;
        }
        if let Some(session_id) = object.get("session_id").and_then(non_empty_value_string) {
            ids.insert(session_id);
        }
    }
    ids.into_iter().collect()
}

fn manifest_review_case_window_evidence_gap(
    evidence: Option<&Map<String, Value>>,
    case_window: Option<&(String, String)>,
) -> Option<Value> {
    let case_window = case_window?;
    let evidence = evidence?;
    let bounds = [
        (
            "decoded_frame_time_bounds",
            evidence.get("decoded_frame_time_bounds"),
        ),
        (
            "raw_evidence_time_bounds",
            evidence.get("raw_evidence_time_bounds"),
        ),
    ]
    .into_iter()
    .filter_map(|(bounds_source, bounds)| {
        let bounds = bounds.filter(|bounds| !bounds.is_null())?;
        let (overlap_status, evidence_time_bounds) =
            manifest_review_session_window_overlap(Some(bounds), Some(case_window));
        Some(json!({
            "bounds_source": bounds_source,
            "overlap_status": overlap_status,
            "evidence_time_bounds": evidence_time_bounds
        }))
    })
    .collect::<Vec<_>>();
    if bounds.is_empty() {
        return None;
    }
    let all_outside = bounds.iter().all(|row| {
        row.get("overlap_status").and_then(value_string).as_deref() == Some("outside_case_window")
    });
    if all_outside {
        Some(json!({
            "evidence_overlap_status": "outside_generated_evidence_window",
            "evidence_bounds": bounds
        }))
    } else {
        None
    }
}

fn manifest_review_capture_session_packet_family_gap(
    evidence: Option<&Map<String, Value>>,
    declared_capture_session_ids: &[String],
    required_packet_family_prefixes: &[String],
) -> Option<Value> {
    if declared_capture_session_ids.is_empty() || required_packet_family_prefixes.is_empty() {
        return None;
    }
    let summaries = evidence
        .and_then(|evidence| evidence.get("capture_session_summaries"))
        .and_then(Value::as_array)?;
    let mut declared_session_summaries = Vec::new();
    let mut any_matching_packet_family = false;
    for declared_session_id in declared_capture_session_ids {
        let Some(summary_object) = summaries.iter().find_map(|summary| {
            let object = summary.as_object()?;
            (object.get("session_id").and_then(value_string).as_deref()
                == Some(declared_session_id.as_str()))
            .then_some(object)
        }) else {
            continue;
        };
        let packet_family_counts = summary_object
            .get("packet_family_counts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let matching_packet_family_counts = packet_family_counts
            .iter()
            .filter(|(family, _count)| {
                required_packet_family_prefixes
                    .iter()
                    .any(|prefix| packet_family_matches_prefix(family, prefix))
            })
            .map(|(family, count)| (family.clone(), count.clone()))
            .collect::<Map<_, _>>();
        if !matching_packet_family_counts.is_empty() {
            any_matching_packet_family = true;
        }
        declared_session_summaries.push(json!({
            "session_id": declared_session_id,
            "packet_family_count": packet_family_counts.len(),
            "packet_family_counts": packet_family_counts,
            "matching_packet_family_count": matching_packet_family_counts.len(),
            "matching_packet_family_counts": matching_packet_family_counts
        }));
    }
    if declared_session_summaries.is_empty() || any_matching_packet_family {
        return None;
    }
    Some(json!({
        "declared_session_count": declared_session_summaries.len(),
        "declared_session_packet_family_counts": declared_session_summaries
    }))
}

fn manifest_review_acceptance_evidence_cases(
    object: Option<&Map<String, Value>>,
    evidence: Option<&Map<String, Value>>,
    known_capture_session_ids: &[String],
    top_level_capture_session_ids: &[String],
    manifest_start: Option<&str>,
    manifest_end: Option<&str>,
) -> Vec<Value> {
    let cases = object
        .and_then(|object| object.get("cases"))
        .and_then(Value::as_array);
    let mut evidence_cases = Vec::new();
    for case in cases.into_iter().flatten() {
        let Some(case_object) = case.as_object() else {
            continue;
        };
        let case_id = case_object
            .get("id")
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let report = case_object
            .get("report")
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let normalized_report = manifest_review_normalized_report(&report);
        let requires_capture_session =
            manifest_review_requires_capture_session_binding(normalized_report.as_deref());
        let required_packet_family_prefixes =
            manifest_review_required_packet_family_prefixes(normalized_report.as_deref());
        let official_label_fields =
            manifest_review_required_official_label_fields(normalized_report.as_deref());
        let manual_label_fields =
            manifest_review_required_manual_label_fields(normalized_report.as_deref());
        let capture_kind = case_object.get("capture_kind").and_then(value_string);
        let has_capture_kind = capture_kind
            .as_deref()
            .is_some_and(|kind| !kind.trim().is_empty());
        let null_fields = case_object
            .iter()
            .filter_map(|(key, value)| value.is_null().then_some(key.clone()))
            .collect::<Vec<_>>();
        if !requires_capture_session
            && !has_capture_kind
            && official_label_fields.is_empty()
            && manual_label_fields.is_empty()
            && null_fields.is_empty()
        {
            continue;
        }

        let case_declared_session_ids = manifest_review_declared_capture_session_ids(case_object);
        let (declared_capture_session_ids, declared_capture_session_source) =
            if case_declared_session_ids.is_empty() {
                (top_level_capture_session_ids.to_vec(), "manifest")
            } else {
                (case_declared_session_ids, "case")
            };
        let case_window = manifest_review_case_window(case_object, manifest_start, manifest_end);
        let suggested_capture_sessions =
            if requires_capture_session && declared_capture_session_ids.is_empty() {
                manifest_review_suggested_capture_sessions(
                    evidence,
                    &required_packet_family_prefixes,
                    case_window.as_ref(),
                )
            } else {
                Vec::new()
            };
        let suggested_capture_session_ids = suggested_capture_sessions
            .iter()
            .filter_map(|session| session.get("session_id").and_then(value_string))
            .collect::<Vec<_>>();

        let missing_official_label_fields = if official_label_fields.is_empty()
            || manifest_review_case_has_any_non_null_field(case_object, &official_label_fields)
        {
            Vec::new()
        } else {
            official_label_fields.clone()
        };
        let missing_manual_label_fields = manual_label_fields
            .iter()
            .filter(|field| case_object.get(*field).is_none_or(|value| value.is_null()))
            .cloned()
            .collect::<Vec<_>>();
        let placeholder_fields = null_fields
            .into_iter()
            .filter(|field| {
                !official_label_fields.contains(field) && !manual_label_fields.contains(field)
            })
            .collect::<Vec<_>>();

        let mut outstanding_requirements = Vec::new();
        for field in &missing_manual_label_fields {
            outstanding_requirements.push(format!("manual_label:{field}"));
        }
        for field in &missing_official_label_fields {
            outstanding_requirements.push(format!("official_label:{field}"));
        }
        for field in &placeholder_fields {
            outstanding_requirements.push(format!("placeholder:{field}"));
        }

        let mut capture_session_status = "not_required";
        let mut missing_capture_session_ids = Vec::new();
        let mut case_window_evidence_status = "not_checked";
        if requires_capture_session {
            if declared_capture_session_ids.is_empty() {
                capture_session_status = "binding_required";
                outstanding_requirements.push("capture_session_id".to_string());
            } else if known_capture_session_ids.is_empty() {
                capture_session_status = "bound_unverified";
                outstanding_requirements
                    .push("runtime_capture_session_evidence_verification".to_string());
            } else {
                missing_capture_session_ids = declared_capture_session_ids
                    .iter()
                    .filter(|session_id| !known_capture_session_ids.contains(*session_id))
                    .cloned()
                    .collect::<Vec<_>>();
                if !missing_capture_session_ids.is_empty() {
                    capture_session_status = "unresolved";
                    outstanding_requirements.push("capture_session_resolution".to_string());
                } else if manifest_review_capture_session_packet_family_gap(
                    evidence,
                    &declared_capture_session_ids,
                    &required_packet_family_prefixes,
                )
                .is_some()
                {
                    capture_session_status = "wrong_packet_family";
                    outstanding_requirements.push("capture_session_packet_family".to_string());
                } else {
                    capture_session_status = "bound";
                }
            }
            if manifest_review_case_window_evidence_gap(evidence, case_window.as_ref()).is_some() {
                case_window_evidence_status = "outside_generated_evidence_window";
                outstanding_requirements.push("case_window_evidence_overlap".to_string());
            } else if evidence.is_some() {
                case_window_evidence_status = "overlaps_or_unbounded";
            }
        }
        let collection_action = manifest_review_acceptance_collection_action(
            &report,
            normalized_report.as_deref(),
            capture_kind.as_deref(),
        );

        evidence_cases.push(json!({
            "case_id": case_id,
            "report": report,
            "normalized_report": normalized_report,
            "capture_kind": capture_kind,
            "case_window_start": case_window.as_ref().map(|(start, _end)| start),
            "case_window_end": case_window.as_ref().map(|(_start, end)| end),
            "requires_capture_session": requires_capture_session,
            "capture_session_status": capture_session_status,
            "declared_capture_session_source": declared_capture_session_source,
            "declared_capture_session_ids": declared_capture_session_ids,
            "missing_capture_session_ids": missing_capture_session_ids,
            "required_packet_family_prefixes": required_packet_family_prefixes,
            "suggested_capture_session_count": suggested_capture_session_ids.len(),
            "suggested_capture_session_ids": suggested_capture_session_ids,
            "suggested_capture_sessions": suggested_capture_sessions,
            "case_window_evidence_status": case_window_evidence_status,
            "official_label_fields_required": official_label_fields,
            "missing_official_label_fields": missing_official_label_fields,
            "manual_label_fields_required": manual_label_fields,
            "missing_manual_label_fields": missing_manual_label_fields,
            "placeholder_fields": placeholder_fields,
            "outstanding_requirement_count": outstanding_requirements.len(),
            "outstanding_requirements": outstanding_requirements,
            "collection_action": collection_action
        }));
    }
    evidence_cases
}

fn manifest_review_acceptance_collection_action(
    report: &str,
    normalized_report: Option<&str>,
    capture_kind: Option<&str>,
) -> String {
    let report = normalized_report.unwrap_or(report).trim();
    let capture_hint = capture_kind
        .filter(|kind| !kind.trim().is_empty())
        .map(|kind| format!(" `{kind}`"))
        .unwrap_or_default();
    match report {
        "step-discovery" => {
            format!(
                "Run the controlled step/motion capture{capture_hint} and keep K10/K11/K21 packet evidence for counter discovery."
            )
        }
        "step-validation" | "raw-motion-steps" => {
            format!(
                "Run the controlled step capture{capture_hint}, record the manual count, then add the WHOOP app step delta as a validation label."
            )
        }
        "energy-validation" => {
            format!(
                "Run the rest/walk/workout energy capture{capture_hint}, keep HR and motion evidence, then add WHOOP app calorie labels for comparison only."
            )
        }
        "rhr-validation" => {
            format!(
                "Use an overnight or low-motion rest capture{capture_hint}, then add the WHOOP app resting-HR label for comparison only."
            )
        }
        "hrv-validation" => {
            format!(
                "Use an overnight capture{capture_hint}, then add the WHOOP app HRV/RMSSD label while keeping RR interval scale promotion blocked until validated."
            )
        }
        "respiratory-rate-validation" => {
            format!(
                "Use an overnight capture{capture_hint}, then add the WHOOP app respiratory-rate label while keeping packet semantics validation-only."
            )
        }
        "oxygen-saturation-validation" => {
            format!(
                "Use overnight, charger, or post-sync/history evidence{capture_hint}, then add the WHOOP app SpO2 label without promoting a value until the decoder is proven."
            )
        }
        "temperature-validation" => {
            format!(
                "Use overnight, charger, or post-sync/history evidence{capture_hint}, then add the WHOOP app temperature label without promoting a value until units and delta semantics are proven."
            )
        }
        "energy-rollup" | "energy_daily_rollup" => {
            format!(
                "Keep HR, motion, and profile inputs for the local calorie rollup{capture_hint}; no WHOOP label is used as an input."
            )
        }
        "rhr-rollup" | "resting_hr_rollup" => {
            format!(
                "Keep packet-derived HR plus low-motion evidence for the local RHR rollup{capture_hint}."
            )
        }
        "recovery-sensor-rollup" | "recovery-sensors" => {
            format!(
                "Keep overnight/charger/history packet evidence{capture_hint} so HRV, respiratory-rate, SpO2, and temperature promotion gates can stay audited."
            )
        }
        _ => {
            "Collect owned packet evidence and validation labels required by this case.".to_string()
        }
    }
}

fn manifest_review_declared_capture_session_ids(case_object: &Map<String, Value>) -> Vec<String> {
    let mut ids = BTreeSet::new();
    if let Some(session_id) = case_object
        .get("capture_session_id")
        .and_then(non_empty_value_string)
    {
        ids.insert(session_id);
    }
    for session_id in case_object
        .get("capture_session_ids")
        .map(string_array)
        .unwrap_or_default()
    {
        if let Some(session_id) = non_empty_string(Some(&session_id)) {
            ids.insert(session_id);
        }
    }
    ids.into_iter().collect()
}

fn manifest_review_required_official_label_fields(normalized_report: Option<&str>) -> Vec<String> {
    let fields: &[&str] = match normalized_report {
        Some("step-validation") | Some("raw-motion-steps") => &["official_whoop_step_delta"],
        Some("energy-validation") => &[
            "official_whoop_active_kcal",
            "official_whoop_resting_kcal",
            "official_whoop_total_kcal",
        ],
        Some("rhr-validation") => &["official_whoop_resting_hr_bpm"],
        Some("hrv-validation") => &["official_whoop_hrv_rmssd_ms"],
        Some("respiratory-rate-validation") => &["official_whoop_respiratory_rate_rpm"],
        Some("oxygen-saturation-validation") => &["official_whoop_oxygen_saturation_percent"],
        Some("temperature-validation") => &["official_whoop_skin_temperature_delta_c"],
        _ => &[],
    };
    fields.iter().map(|field| (*field).to_string()).collect()
}

fn manifest_review_required_manual_label_fields(normalized_report: Option<&str>) -> Vec<String> {
    let fields: &[&str] = match normalized_report {
        Some("step-validation") | Some("raw-motion-steps") => &["manual_step_delta"],
        _ => &[],
    };
    fields.iter().map(|field| (*field).to_string()).collect()
}

fn manifest_review_case_has_any_non_null_field(
    case_object: &Map<String, Value>,
    fields: &[String],
) -> bool {
    fields
        .iter()
        .any(|field| case_object.get(field).is_some_and(|value| !value.is_null()))
}

fn manifest_review_case_has_all_non_null_fields(
    case_object: &Map<String, Value>,
    fields: &[String],
) -> bool {
    fields
        .iter()
        .all(|field| case_object.get(field).is_some_and(|value| !value.is_null()))
}

fn manifest_review_normalized_report(report: &str) -> Option<String> {
    let report = report.trim().to_ascii_lowercase();
    match report.as_str() {
        "step-discovery" | "step_discovery" | "step_packet_discovery" => {
            Some("step-discovery".to_string())
        }
        "steps" | "step-validation" | "step_capture_validation" => {
            Some("step-validation".to_string())
        }
        "raw-motion-steps" | "raw_motion_steps" | "raw_motion_step_estimate" => {
            Some("raw-motion-steps".to_string())
        }
        "energy-validation" | "calorie-validation" | "energy_capture_validation" => {
            Some("energy-validation".to_string())
        }
        "rhr-validation" | "resting-hr-validation" | "resting_hr_capture_validation" => {
            Some("rhr-validation".to_string())
        }
        "hrv-validation" | "hrv_capture_validation" => Some("hrv-validation".to_string()),
        "respiratory-rate-validation"
        | "respiratory_rate_validation"
        | "respiratory_rate_capture_validation" => Some("respiratory-rate-validation".to_string()),
        "oxygen-saturation-validation"
        | "spo2-validation"
        | "oxygen_saturation_validation"
        | "oxygen_saturation_capture_validation" => {
            Some("oxygen-saturation-validation".to_string())
        }
        "temperature-validation" | "temperature_capture_validation" => {
            Some("temperature-validation".to_string())
        }
        _ => None,
    }
}

fn manifest_review_requires_capture_session_binding(normalized_report: Option<&str>) -> bool {
    matches!(
        normalized_report,
        Some("step-discovery")
            | Some("step-validation")
            | Some("raw-motion-steps")
            | Some("energy-validation")
            | Some("rhr-validation")
            | Some("hrv-validation")
            | Some("respiratory-rate-validation")
            | Some("oxygen-saturation-validation")
            | Some("temperature-validation")
    )
}

fn manifest_review_required_packet_family_prefixes(normalized_report: Option<&str>) -> Vec<String> {
    let prefixes: &[&str] = match normalized_report {
        Some("step-discovery") | Some("step-validation") | Some("raw-motion-steps") => {
            &["K10", "K11", "K21"]
        }
        Some("energy-validation") => &["K2", "K10", "K11", "K18", "K21", "K24"],
        Some("rhr-validation") => &["K2", "K10", "K18", "K24"],
        Some("hrv-validation") => &["K17", "K18", "K24", "EVENT"],
        Some("respiratory-rate-validation") => &["K18", "K24", "EVENT"],
        Some("oxygen-saturation-validation") => &["K2", "K17", "K18", "K24", "EVENT"],
        Some("temperature-validation") => &["K18", "K24", "EVENT"],
        _ => &[],
    };
    prefixes
        .iter()
        .map(|prefix| (*prefix).to_string())
        .collect()
}

fn manifest_review_suggested_capture_sessions(
    evidence: Option<&Map<String, Value>>,
    required_packet_family_prefixes: &[String],
    case_window: Option<&(String, String)>,
) -> Vec<Value> {
    if required_packet_family_prefixes.is_empty() {
        return Vec::new();
    }
    let Some(summaries) = evidence
        .and_then(|evidence| evidence.get("capture_session_summaries"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut suggested = Vec::new();
    let mut suggested_session_ids = BTreeSet::new();
    for summary in summaries {
        let Some(object) = summary.as_object() else {
            continue;
        };
        let Some(session_id) = object.get("session_id").and_then(value_string) else {
            continue;
        };
        let Some(packet_family_counts) = object
            .get("packet_family_counts")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let matching_packet_family_counts = packet_family_counts
            .iter()
            .filter(|(family, _count)| {
                required_packet_family_prefixes
                    .iter()
                    .any(|prefix| packet_family_matches_prefix(family, prefix))
            })
            .map(|(family, count)| (family.clone(), count.clone()))
            .collect::<Map<_, _>>();
        let (overlap_status, decoded_bounds) = manifest_review_session_window_overlap(
            object
                .get("decoded_frame_time_bounds")
                .or_else(|| object.get("raw_evidence_time_bounds")),
            case_window,
        );
        if !matching_packet_family_counts.is_empty()
            && overlap_status != "outside_case_window"
            && suggested_session_ids.insert(session_id.clone())
        {
            suggested.push(json!({
                "session_id": session_id,
                "case_window_overlap_status": overlap_status,
                "evidence_time_bounds": decoded_bounds,
                "matching_packet_family_count": matching_packet_family_counts.len(),
                "matching_packet_family_counts": matching_packet_family_counts
            }));
        }
    }
    suggested
}

fn manifest_review_case_window(
    case_object: &Map<String, Value>,
    manifest_start: Option<&str>,
    manifest_end: Option<&str>,
) -> Option<(String, String)> {
    let start = case_object
        .get("start")
        .and_then(value_string)
        .or_else(|| manifest_start.map(str::to_string))?;
    let end = case_object
        .get("end")
        .and_then(value_string)
        .or_else(|| manifest_end.map(str::to_string))?;
    Some((start, end))
}

fn manifest_review_session_window_overlap(
    bounds: Option<&Value>,
    case_window: Option<&(String, String)>,
) -> (&'static str, Value) {
    let evidence_time_bounds = bounds.cloned().unwrap_or(Value::Null);
    let Some((case_start, case_end)) = case_window else {
        return ("case_window_unknown", evidence_time_bounds);
    };
    let Some(case_start_ms) = parse_rfc3339_utc_unix_ms(case_start) else {
        return ("case_window_unknown", evidence_time_bounds);
    };
    let Some(case_end_ms) = parse_rfc3339_utc_unix_ms(case_end) else {
        return ("case_window_unknown", evidence_time_bounds);
    };
    let Some(bounds_object) = bounds.and_then(Value::as_object) else {
        return ("session_bounds_unknown", evidence_time_bounds);
    };
    let Some(first_ms) = bounds_object
        .get("first_captured_at")
        .and_then(value_string)
        .and_then(|value| parse_rfc3339_utc_unix_ms(&value))
    else {
        return ("session_bounds_unknown", evidence_time_bounds);
    };
    let Some(last_ms) = bounds_object
        .get("last_captured_at")
        .and_then(value_string)
        .and_then(|value| parse_rfc3339_utc_unix_ms(&value))
    else {
        return ("session_bounds_unknown", evidence_time_bounds);
    };
    if last_ms >= case_start_ms && first_ms < case_end_ms {
        ("overlaps_case_window", evidence_time_bounds)
    } else {
        ("outside_case_window", evidence_time_bounds)
    }
}

fn append_runbook_operator_checklist(markdown: &mut String, checklist: Option<&Vec<Value>>) {
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Operator Checklist");
    let _ = writeln!(markdown);
    let Some(checklist) = checklist else {
        let _ = writeln!(markdown, "No checklist items were generated.");
        return;
    };
    if checklist.is_empty() {
        let _ = writeln!(markdown, "No checklist items were generated.");
        return;
    }
    let _ = writeln!(markdown, "| Item | Status | Action |");
    let _ = writeln!(markdown, "| --- | --- | --- |");
    for item in checklist {
        let object = item.as_object();
        let id = object
            .and_then(|object| object.get("id"))
            .and_then(value_string)
            .unwrap_or_else(|| "item".to_string());
        let status = object
            .and_then(|object| object.get("status"))
            .and_then(value_string)
            .unwrap_or_else(|| "pending".to_string());
        let action = object
            .and_then(|object| object.get("action"))
            .and_then(value_string)
            .unwrap_or_else(|| "Review this manifest item.".to_string());
        let _ = writeln!(
            markdown,
            "| {} | {} | {} |",
            markdown_table_cell(&id),
            markdown_table_cell(&status),
            markdown_table_cell(&action)
        );
    }
}

fn append_runbook_acceptance_evidence_checklist(markdown: &mut String, review: &Value) {
    let cases = review
        .get("acceptance_evidence_cases")
        .and_then(Value::as_array);
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Acceptance Evidence Checklist");
    let _ = writeln!(markdown);
    let Some(cases) = cases else {
        let _ = writeln!(
            markdown,
            "No capture or label collection cases were detected."
        );
        return;
    };
    if cases.is_empty() {
        let _ = writeln!(
            markdown,
            "No capture or label collection cases were detected."
        );
        return;
    }
    let _ = writeln!(
        markdown,
        "Use this table while collecting owned captures and validation labels. WHOOP app values are labels only, never metric inputs."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Case | Capture Kind | Report | Window | Capture Binding | Required Families | Missing Evidence | Suggested Sessions | Collection Action |"
    );
    let _ = writeln!(
        markdown,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for case in cases {
        let object = case.as_object();
        let case_id = object
            .and_then(|object| object.get("case_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let capture_kind = object
            .and_then(|object| object.get("capture_kind"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let report = object
            .and_then(|object| object.get("report"))
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let case_start = object
            .and_then(|object| object.get("case_window_start"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let case_end = object
            .and_then(|object| object.get("case_window_end"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let capture_binding = runbook_acceptance_capture_binding_summary(case);
        let required_families = object
            .and_then(|object| object.get("required_packet_family_prefixes"))
            .map(string_array)
            .unwrap_or_default();
        let missing_evidence = runbook_acceptance_missing_evidence_summary(case);
        let suggested_sessions = runbook_suggested_capture_sessions_summary(
            object.and_then(|object| object.get("suggested_capture_sessions")),
        );
        let suggested_sessions = if suggested_sessions == "--" {
            let ids = object
                .and_then(|object| object.get("suggested_capture_session_ids"))
                .map(string_array)
                .unwrap_or_default();
            manifest_review_join_or_dash(&ids)
        } else {
            suggested_sessions
        };
        let collection_action = object
            .and_then(|object| object.get("collection_action"))
            .and_then(value_string)
            .unwrap_or_else(|| "Collect owned packet evidence and labels.".to_string());
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            markdown_table_cell(&case_id),
            markdown_table_cell(&capture_kind),
            markdown_table_cell(&report),
            markdown_table_cell(&format!("{case_start} to {case_end}")),
            markdown_table_cell(&capture_binding),
            markdown_table_cell(&manifest_review_join_or_dash(&required_families)),
            markdown_table_cell(&missing_evidence),
            markdown_table_cell(&suggested_sessions),
            markdown_table_cell(&collection_action)
        );
    }
}

fn runbook_acceptance_capture_binding_summary(case: &Value) -> String {
    let object = case.as_object();
    let status = object
        .and_then(|object| object.get("capture_session_status"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let sessions = object
        .and_then(|object| object.get("declared_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    if sessions.is_empty() {
        status
    } else {
        format!("{status}: {}", sessions.join(", "))
    }
}

fn runbook_acceptance_missing_evidence_summary(case: &Value) -> String {
    let object = case.as_object();
    let mut rows = Vec::new();
    let manual_labels = object
        .and_then(|object| object.get("missing_manual_label_fields"))
        .map(string_array)
        .unwrap_or_default();
    if !manual_labels.is_empty() {
        rows.push(format!("manual labels: {}", manual_labels.join(", ")));
    }
    let official_labels = object
        .and_then(|object| object.get("missing_official_label_fields"))
        .map(string_array)
        .unwrap_or_default();
    if !official_labels.is_empty() {
        rows.push(format!("WHOOP labels: {}", official_labels.join(", ")));
    }
    let placeholders = object
        .and_then(|object| object.get("placeholder_fields"))
        .map(string_array)
        .unwrap_or_default();
    if !placeholders.is_empty() {
        rows.push(format!("placeholders: {}", placeholders.join(", ")));
    }
    let requirements = object
        .and_then(|object| object.get("outstanding_requirements"))
        .map(string_array)
        .unwrap_or_default()
        .into_iter()
        .filter(|requirement| {
            !requirement.starts_with("manual_label:")
                && !requirement.starts_with("official_label:")
                && !requirement.starts_with("placeholder:")
        })
        .collect::<Vec<_>>();
    if !requirements.is_empty() {
        rows.push(format!("requirements: {}", requirements.join(", ")));
    }
    if rows.is_empty() {
        "complete".to_string()
    } else {
        rows.join("; ")
    }
}

fn append_runbook_capture_session_binding(markdown: &mut String, cases: Option<&Vec<Value>>) {
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Capture Session Binding");
    let _ = writeln!(markdown);
    let Some(cases) = cases else {
        let _ = writeln!(markdown, "No capture-session binding gaps were detected.");
        return;
    };
    if cases.is_empty() {
        let _ = writeln!(markdown, "No capture-session binding gaps were detected.");
        return;
    }
    let _ = writeln!(
        markdown,
        "These cases must be bound to the owned capture session before they count as acceptance evidence."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Case | Report | Normalized Report | Suggested Sessions | Required Families | Required Edit |"
    );
    let _ = writeln!(markdown, "| --- | --- | --- | --- | --- | --- |");
    for case in cases {
        let object = case.as_object();
        let case_id = object
            .and_then(|object| object.get("case_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let report = object
            .and_then(|object| object.get("report"))
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let normalized_report = object
            .and_then(|object| object.get("normalized_report"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let suggested_sessions = runbook_suggested_capture_sessions_summary(
            object.and_then(|object| object.get("suggested_capture_sessions")),
        );
        let suggested_sessions = if suggested_sessions == "--" {
            let ids = object
                .and_then(|object| object.get("suggested_capture_session_ids"))
                .map(string_array)
                .unwrap_or_default();
            if ids.is_empty() {
                "--".to_string()
            } else {
                ids.join(", ")
            }
        } else {
            suggested_sessions
        };
        let required_families = object
            .and_then(|object| object.get("required_packet_family_prefixes"))
            .map(string_array)
            .unwrap_or_default();
        let required_families = if required_families.is_empty() {
            "--".to_string()
        } else {
            required_families.join(", ")
        };
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | Add `capture_session_id` or `capture_session_ids` |",
            markdown_table_cell(&case_id),
            markdown_table_cell(&report),
            markdown_table_cell(&normalized_report),
            markdown_table_cell(&suggested_sessions),
            markdown_table_cell(&required_families)
        );
    }
}

fn runbook_suggested_capture_sessions_summary(sessions: Option<&Value>) -> String {
    let rows = sessions
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|session| {
            let object = session.as_object()?;
            let session_id = object.get("session_id").and_then(value_string)?;
            let overlap_status = object
                .get("case_window_overlap_status")
                .and_then(value_string)
                .unwrap_or_else(|| "overlap_unknown".to_string());
            let bounds = object
                .get("evidence_time_bounds")
                .filter(|value| !value.is_null())
                .map(time_bounds_summary);
            let matching_counts = object
                .get("matching_packet_family_counts")
                .and_then(Value::as_object)
                .map(|counts| {
                    counts
                        .iter()
                        .map(|(family, count)| {
                            format!(
                                "{family}={}",
                                value_string(count).unwrap_or_else(|| "0".to_string())
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|text| !text.is_empty());
            let mut details = vec![overlap_status];
            if let Some(bounds) = bounds.filter(|bounds| bounds != "--") {
                details.push(bounds);
            }
            if let Some(counts) = matching_counts {
                details.push(counts);
            }
            let detail_text = details.join("; ");
            Some(if detail_text.is_empty() {
                session_id
            } else {
                format!("{session_id} ({detail_text})")
            })
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        "--".to_string()
    } else {
        rows.join("; ")
    }
}

fn append_runbook_capture_sqlite_imports(markdown: &mut String, review: &Value) {
    let imports = review
        .get("capture_sqlite_imports")
        .and_then(Value::as_array);
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Capture SQLite Imports");
    let _ = writeln!(markdown);
    let Some(imports) = imports else {
        let _ = writeln!(markdown, "No capture SQLite imports were declared.");
        return;
    };
    if imports.is_empty() {
        let _ = writeln!(markdown, "No capture SQLite imports were declared.");
        return;
    }
    let _ = writeln!(
        markdown,
        "These processed HCI imports must name the owned capture session before validation cases bind to them."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Index | ID | Path | Session | Status | Issues |"
    );
    let _ = writeln!(markdown, "| ---: | --- | --- | --- | --- | --- |");
    for import in imports {
        let object = import.as_object();
        let index = object
            .and_then(|object| object.get("index"))
            .and_then(value_string)
            .unwrap_or_else(|| "0".to_string());
        let id = object
            .and_then(|object| object.get("id"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let path = object
            .and_then(|object| object.get("path"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let session_id = object
            .and_then(|object| object.get("session_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let status = object
            .and_then(|object| object.get("status"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let issues = object
            .and_then(|object| object.get("issues"))
            .map(string_array)
            .unwrap_or_default();
        let issues = if issues.is_empty() {
            "--".to_string()
        } else {
            issues.join(", ")
        };
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | {} |",
            markdown_table_cell(&index),
            markdown_table_cell(&id),
            markdown_table_cell(&path),
            markdown_table_cell(&session_id),
            markdown_table_cell(&status),
            markdown_table_cell(&issues)
        );
    }
}

fn append_runbook_capture_session_resolution(markdown: &mut String, review: &Value) {
    let unresolved_cases = review
        .get("capture_session_unresolved_cases")
        .and_then(Value::as_array);
    let unverified_cases = review
        .get("capture_session_unverified_cases")
        .and_then(Value::as_array);
    let unresolved_empty = unresolved_cases.is_none_or(Vec::is_empty);
    let unverified_empty = unverified_cases.is_none_or(Vec::is_empty);
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Capture Session Resolution");
    let _ = writeln!(markdown);
    if unresolved_empty && unverified_empty {
        let _ = writeln!(
            markdown,
            "No unresolved capture-session declarations were detected."
        );
        return;
    }

    if let Some(cases) = unresolved_cases.filter(|cases| !cases.is_empty()) {
        let _ = writeln!(
            markdown,
            "These cases declare capture-session IDs that are not present in generated evidence or valid capture SQLite imports."
        );
        let _ = writeln!(markdown);
        let _ = writeln!(
            markdown,
            "| Case | Report | Normalized Report | Source | Declared Sessions | Missing Sessions | Known Sessions | Required Edit |"
        );
        let _ = writeln!(
            markdown,
            "| --- | --- | --- | --- | --- | --- | --- | --- |"
        );
        for case in cases {
            append_runbook_capture_session_unresolved_row(markdown, case);
        }
    }

    if let Some(cases) = unverified_cases.filter(|cases| !cases.is_empty()) {
        let _ = writeln!(markdown);
        let _ = writeln!(
            markdown,
            "These cases declare capture-session IDs, but this review has no generated evidence or valid capture SQLite imports to verify them before runtime."
        );
        let _ = writeln!(markdown);
        let _ = writeln!(
            markdown,
            "| Case | Report | Normalized Report | Source | Declared Sessions | Resolution Status | Reason |"
        );
        let _ = writeln!(markdown, "| --- | --- | --- | --- | --- | --- | --- |");
        for case in cases {
            append_runbook_capture_session_unverified_row(markdown, case);
        }
    }
}

fn append_runbook_capture_session_unresolved_row(markdown: &mut String, case: &Value) {
    let object = case.as_object();
    let case_id = object
        .and_then(|object| object.get("case_id"))
        .and_then(value_string)
        .unwrap_or_else(|| "case".to_string());
    let report = object
        .and_then(|object| object.get("report"))
        .and_then(value_string)
        .unwrap_or_else(|| "report".to_string());
    let normalized_report = object
        .and_then(|object| object.get("normalized_report"))
        .and_then(value_string)
        .unwrap_or_else(|| "--".to_string());
    let source = object
        .and_then(|object| object.get("declared_capture_session_source"))
        .and_then(value_string)
        .unwrap_or_else(|| "case".to_string());
    let declared_sessions = object
        .and_then(|object| object.get("declared_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let missing_sessions = object
        .and_then(|object| object.get("missing_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let known_sessions = object
        .and_then(|object| object.get("known_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let _ = writeln!(
        markdown,
        "| {} | {} | {} | {} | {} | {} | {} | Use an observed/imported `capture_session_id` |",
        markdown_table_cell(&case_id),
        markdown_table_cell(&report),
        markdown_table_cell(&normalized_report),
        markdown_table_cell(&source),
        markdown_table_cell(&manifest_review_join_or_dash(&declared_sessions)),
        markdown_table_cell(&manifest_review_join_or_dash(&missing_sessions)),
        markdown_table_cell(&manifest_review_join_or_dash(&known_sessions))
    );
}

fn append_runbook_capture_session_unverified_row(markdown: &mut String, case: &Value) {
    let object = case.as_object();
    let case_id = object
        .and_then(|object| object.get("case_id"))
        .and_then(value_string)
        .unwrap_or_else(|| "case".to_string());
    let report = object
        .and_then(|object| object.get("report"))
        .and_then(value_string)
        .unwrap_or_else(|| "report".to_string());
    let normalized_report = object
        .and_then(|object| object.get("normalized_report"))
        .and_then(value_string)
        .unwrap_or_else(|| "--".to_string());
    let source = object
        .and_then(|object| object.get("declared_capture_session_source"))
        .and_then(value_string)
        .unwrap_or_else(|| "case".to_string());
    let declared_sessions = object
        .and_then(|object| object.get("declared_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let resolution_status = object
        .and_then(|object| object.get("resolution_status"))
        .and_then(value_string)
        .unwrap_or_else(|| "unverified".to_string());
    let reason = object
        .and_then(|object| object.get("reason"))
        .and_then(value_string)
        .unwrap_or_else(|| "known_sessions_unavailable".to_string());
    let _ = writeln!(
        markdown,
        "| {} | {} | {} | {} | {} | {} | {} |",
        markdown_table_cell(&case_id),
        markdown_table_cell(&report),
        markdown_table_cell(&normalized_report),
        markdown_table_cell(&source),
        markdown_table_cell(&manifest_review_join_or_dash(&declared_sessions)),
        markdown_table_cell(&resolution_status),
        markdown_table_cell(&reason)
    );
}

fn append_runbook_capture_session_packet_families(markdown: &mut String, review: &Value) {
    let cases = review
        .get("capture_session_packet_family_gap_cases")
        .and_then(Value::as_array);
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Capture Session Packet Families");
    let _ = writeln!(markdown);
    let Some(cases) = cases else {
        let _ = writeln!(
            markdown,
            "No capture-session packet-family mismatches were detected."
        );
        return;
    };
    if cases.is_empty() {
        let _ = writeln!(
            markdown,
            "No capture-session packet-family mismatches were detected."
        );
        return;
    }
    let _ = writeln!(
        markdown,
        "These bound cases point at known owned sessions, but those sessions do not contain packet families relevant to the requested metric."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Case | Report | Normalized Report | Declared Sessions | Required Families | Declared Session Families | Required Edit |"
    );
    let _ = writeln!(markdown, "| --- | --- | --- | --- | --- | --- | --- |");
    for case in cases {
        let object = case.as_object();
        let case_id = object
            .and_then(|object| object.get("case_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let report = object
            .and_then(|object| object.get("report"))
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let normalized_report = object
            .and_then(|object| object.get("normalized_report"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let declared_sessions = object
            .and_then(|object| object.get("declared_capture_session_ids"))
            .map(string_array)
            .unwrap_or_default();
        let required_families = object
            .and_then(|object| object.get("required_packet_family_prefixes"))
            .map(string_array)
            .unwrap_or_default();
        let session_families = runbook_capture_session_packet_family_counts_summary(
            object.and_then(|object| object.get("declared_session_packet_family_counts")),
        );
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | {} | Bind to a session with the required packet families |",
            markdown_table_cell(&case_id),
            markdown_table_cell(&report),
            markdown_table_cell(&normalized_report),
            markdown_table_cell(&manifest_review_join_or_dash(&declared_sessions)),
            markdown_table_cell(&manifest_review_join_or_dash(&required_families)),
            markdown_table_cell(&session_families)
        );
    }
}

fn runbook_capture_session_packet_family_counts_summary(value: Option<&Value>) -> String {
    let rows = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|session| {
            let object = session.as_object()?;
            let session_id = object.get("session_id").and_then(value_string)?;
            let counts = object
                .get("packet_family_counts")
                .and_then(Value::as_object)
                .map(packet_family_counts_summary)
                .unwrap_or_else(|| "--".to_string());
            Some(format!("{session_id}: {counts}"))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        "--".to_string()
    } else {
        rows.join("; ")
    }
}

fn packet_family_counts_summary(counts: &Map<String, Value>) -> String {
    let rows = counts
        .iter()
        .map(|(family, count)| {
            format!(
                "{family}={}",
                value_string(count).unwrap_or_else(|| "0".to_string())
            )
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        "--".to_string()
    } else {
        rows.join(", ")
    }
}

fn append_runbook_case_window_evidence(markdown: &mut String, review: &Value) {
    let cases = review
        .get("case_window_evidence_gap_cases")
        .and_then(Value::as_array);
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Case Window Evidence");
    let _ = writeln!(markdown);
    let Some(cases) = cases else {
        let _ = writeln!(markdown, "No case-window evidence gaps were detected.");
        return;
    };
    if cases.is_empty() {
        let _ = writeln!(markdown, "No case-window evidence gaps were detected.");
        return;
    }
    let _ = writeln!(
        markdown,
        "These capture-dependent cases have resolved windows outside the generated raw/decoded evidence bounds."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Case | Report | Normalized Report | Case Window | Evidence Status | Evidence Bounds | Required Edit |"
    );
    let _ = writeln!(markdown, "| --- | --- | --- | --- | --- | --- | --- |");
    for case in cases {
        let object = case.as_object();
        let case_id = object
            .and_then(|object| object.get("case_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "case".to_string());
        let report = object
            .and_then(|object| object.get("report"))
            .and_then(value_string)
            .unwrap_or_else(|| "report".to_string());
        let normalized_report = object
            .and_then(|object| object.get("normalized_report"))
            .and_then(value_string)
            .unwrap_or_else(|| "--".to_string());
        let case_start = object
            .and_then(|object| object.get("case_window_start"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let case_end = object
            .and_then(|object| object.get("case_window_end"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let evidence_status = object
            .and_then(|object| object.get("evidence_overlap_status"))
            .and_then(value_string)
            .unwrap_or_else(|| "unknown".to_string());
        let evidence_bounds = object
            .and_then(|object| object.get("evidence_bounds"))
            .map(runbook_case_window_evidence_bounds_summary)
            .unwrap_or_else(|| "--".to_string());
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} | {} | {} | Adjust `start`/`end` or regenerate the Raw Export bundle |",
            markdown_table_cell(&case_id),
            markdown_table_cell(&report),
            markdown_table_cell(&normalized_report),
            markdown_table_cell(&format!("{case_start} to {case_end}")),
            markdown_table_cell(&evidence_status),
            markdown_table_cell(&evidence_bounds)
        );
    }
}

fn runbook_case_window_evidence_bounds_summary(value: &Value) -> String {
    let rows = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let object = row.as_object()?;
            let source = object.get("bounds_source").and_then(value_string)?;
            let status = object
                .get("overlap_status")
                .and_then(value_string)
                .unwrap_or_else(|| "unknown".to_string());
            let bounds = object
                .get("evidence_time_bounds")
                .map(time_bounds_summary)
                .unwrap_or_else(|| "--".to_string());
            Some(format!("{source}: {status}; {bounds}"))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        "--".to_string()
    } else {
        rows.join("; ")
    }
}

fn append_runbook_validation_labels(markdown: &mut String, review: &Value) {
    let official_missing = review
        .get("official_label_missing_cases")
        .and_then(Value::as_array);
    let manual_missing = review
        .get("manual_label_missing_cases")
        .and_then(Value::as_array);
    let official_empty = official_missing.is_none_or(Vec::is_empty);
    let manual_empty = manual_missing.is_none_or(Vec::is_empty);

    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Validation Labels");
    let _ = writeln!(markdown);
    if official_empty && manual_empty {
        let _ = writeln!(markdown, "No validation-label gaps were detected.");
        return;
    }

    let _ = writeln!(
        markdown,
        "These cases need labels before they count as acceptance evidence. WHOOP app values remain validation labels only."
    );
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Case | Report | Normalized Report | Label Kind | Required Fields | Required Edit |"
    );
    let _ = writeln!(markdown, "| --- | --- | --- | --- | --- | --- |");
    for case in official_missing.into_iter().flatten() {
        append_runbook_validation_label_row(markdown, case);
    }
    for case in manual_missing.into_iter().flatten() {
        append_runbook_validation_label_row(markdown, case);
    }
}

fn append_runbook_validation_label_row(markdown: &mut String, case: &Value) {
    let object = case.as_object();
    let case_id = object
        .and_then(|object| object.get("case_id"))
        .and_then(value_string)
        .unwrap_or_else(|| "case".to_string());
    let report = object
        .and_then(|object| object.get("report"))
        .and_then(value_string)
        .unwrap_or_else(|| "report".to_string());
    let normalized_report = object
        .and_then(|object| object.get("normalized_report"))
        .and_then(value_string)
        .unwrap_or_else(|| "--".to_string());
    let label_kind = object
        .and_then(|object| object.get("required_label_kind"))
        .and_then(value_string)
        .unwrap_or_else(|| "label".to_string());
    let required_fields = object
        .and_then(|object| object.get("required_label_fields"))
        .map(string_array)
        .unwrap_or_default();
    let required_fields = if required_fields.is_empty() {
        "--".to_string()
    } else {
        required_fields.join(", ")
    };
    let required_edit = match label_kind.as_str() {
        "manual_count" => "Add manually counted validation label fields",
        "official_whoop_app" => "Add WHOOP app validation label fields",
        _ => "Add required validation label fields",
    };
    let _ = writeln!(
        markdown,
        "| {} | {} | {} | {} | {} | {} |",
        markdown_table_cell(&case_id),
        markdown_table_cell(&report),
        markdown_table_cell(&normalized_report),
        markdown_table_cell(&label_kind),
        markdown_table_cell(&required_fields),
        required_edit
    );
}

fn append_runbook_generated_evidence(markdown: &mut String, evidence: Option<&Map<String, Value>>) {
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Generated Evidence");
    let _ = writeln!(markdown);
    let database_source_kind = evidence
        .and_then(|evidence| evidence.get("database_source_kind"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let window_source = evidence
        .and_then(|evidence| evidence.get("window_source"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let capture_session_default = evidence
        .and_then(|evidence| evidence.get("capture_session_default"))
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let observed_sessions = evidence
        .and_then(|evidence| evidence.get("observed_capture_session_ids"))
        .map(string_array)
        .unwrap_or_default();
    let observed_session_text = if observed_sessions.is_empty() {
        "`none`".to_string()
    } else {
        observed_sessions
            .iter()
            .map(|session| format!("`{}`", markdown_inline(session)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(
        markdown,
        "- Database source: `{}`",
        markdown_inline(&database_source_kind)
    );
    let _ = writeln!(
        markdown,
        "- Window source: `{}`",
        markdown_inline(&window_source)
    );
    let _ = writeln!(
        markdown,
        "- Capture session default: `{}`",
        markdown_inline(&capture_session_default)
    );
    let _ = writeln!(
        markdown,
        "- Observed capture sessions: {observed_session_text}"
    );
    append_runbook_counts(
        markdown,
        "Packet Families",
        evidence
            .and_then(|evidence| evidence.get("packet_family_counts"))
            .and_then(Value::as_object),
    );
    append_runbook_capture_session_summaries(
        markdown,
        evidence
            .and_then(|evidence| evidence.get("capture_session_summaries"))
            .and_then(Value::as_array),
    );
}

fn append_runbook_counts(markdown: &mut String, title: &str, counts: Option<&Map<String, Value>>) {
    let Some(counts) = counts else {
        return;
    };
    if counts.is_empty() {
        return;
    }
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "### {title}");
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "| Name | Count |");
    let _ = writeln!(markdown, "| --- | ---: |");
    for (name, count) in counts {
        let _ = writeln!(
            markdown,
            "| {} | {} |",
            markdown_table_cell(name),
            markdown_table_cell(&value_string(count).unwrap_or_else(|| "0".to_string()))
        );
    }
}

fn append_runbook_capture_session_summaries(markdown: &mut String, summaries: Option<&Vec<Value>>) {
    let Some(summaries) = summaries else {
        return;
    };
    if summaries.is_empty() {
        return;
    }
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "### Capture Sessions");
    let _ = writeln!(markdown);
    let _ = writeln!(
        markdown,
        "| Session | Raw Bounds | Decoded Bounds | Packet Families |"
    );
    let _ = writeln!(markdown, "| --- | --- | --- | --- |");
    for summary in summaries {
        let object = summary.as_object();
        let session_id = object
            .and_then(|object| object.get("session_id"))
            .and_then(value_string)
            .unwrap_or_else(|| "session".to_string());
        let raw_bounds = object
            .and_then(|object| object.get("raw_evidence_time_bounds"))
            .map(time_bounds_summary)
            .unwrap_or_else(|| "--".to_string());
        let decoded_bounds = object
            .and_then(|object| object.get("decoded_frame_time_bounds"))
            .map(time_bounds_summary)
            .unwrap_or_else(|| "--".to_string());
        let packet_families = object
            .and_then(|object| object.get("packet_family_counts"))
            .and_then(Value::as_object)
            .map(|counts| {
                counts
                    .iter()
                    .map(|(family, count)| {
                        format!(
                            "{family}={}",
                            value_string(count).unwrap_or_else(|| "0".to_string())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "--".to_string());
        let _ = writeln!(
            markdown,
            "| {} | {} | {} | {} |",
            markdown_table_cell(&session_id),
            markdown_table_cell(&raw_bounds),
            markdown_table_cell(&decoded_bounds),
            markdown_table_cell(&packet_families)
        );
    }
}

fn append_runbook_placeholder_cases(markdown: &mut String, cases: Option<&Vec<Value>>) {
    let rows = cases
        .into_iter()
        .flatten()
        .filter_map(|case| {
            let object = case.as_object()?;
            let fields = object
                .iter()
                .filter_map(|(key, value)| value.is_null().then_some(key.clone()))
                .collect::<Vec<_>>();
            (!fields.is_empty()).then(|| {
                (
                    object
                        .get("id")
                        .and_then(value_string)
                        .unwrap_or_else(|| "case".to_string()),
                    object
                        .get("report")
                        .and_then(value_string)
                        .unwrap_or_else(|| "report".to_string()),
                    fields,
                )
            })
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "## Fields To Fill");
    let _ = writeln!(markdown);
    let _ = writeln!(markdown, "| Case | Report | Fields |");
    let _ = writeln!(markdown, "| --- | --- | --- |");
    for (case_id, report, fields) in rows {
        let _ = writeln!(
            markdown,
            "| {} | {} | {} |",
            markdown_table_cell(&case_id),
            markdown_table_cell(&report),
            markdown_table_cell(&fields.join(", "))
        );
    }
}

fn time_bounds_summary(value: &Value) -> String {
    let Some(bounds) = value.as_object() else {
        return "--".to_string();
    };
    let first = bounds
        .get("first_captured_at")
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    let last = bounds
        .get("last_captured_at")
        .and_then(value_string)
        .unwrap_or_else(|| "unknown".to_string());
    if let Some(span) = bounds.get("span_ms").and_then(value_string) {
        return format!("{first} to {last} ({span} ms)");
    }
    format!("{first} to {last}")
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|values| values.iter().filter_map(value_string).collect())
        .unwrap_or_default()
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => Some(value.to_string()),
    }
}

fn non_empty_value_string(value: &Value) -> Option<String> {
    non_empty_string(value_string(value).as_deref())
}

fn manifest_review_join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "--".to_string()
    } else {
        values.join(", ")
    }
}

fn markdown_inline(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn markdown_table_cell(value: &str) -> String {
    markdown_inline(value).replace('|', "\\|")
}

fn scaffold_time_window(
    options: &LocalHealthValidationManifestScaffoldOptions,
) -> GooseResult<(String, String, String)> {
    if let Some(start) = non_empty_string(options.start.as_deref())
        && let Some(end) = non_empty_string(options.end.as_deref())
    {
        return Ok((
            start,
            end,
            non_empty_string(options.window_source.as_deref())
                .unwrap_or_else(|| "provided_window".to_string()),
        ));
    }
    let Some((start, end)) = query_database_raw_evidence_window(&options.database_path)? else {
        return Err(GooseError::message(
            "cannot scaffold a validation manifest without a provided time window or raw_evidence timestamps",
        ));
    };
    Ok((start, end, "raw_evidence_bounds".to_string()))
}

fn query_database_raw_evidence_window(
    database_path: &PathBuf,
) -> GooseResult<Option<(String, String)>> {
    let connection = open_read_only_database(database_path)?;
    connection
        .query_row(
            "SELECT MIN(captured_at), MAX(captured_at) FROM raw_evidence",
            [],
            |row| {
                let start = row.get::<_, Option<String>>(0)?;
                let end = row.get::<_, Option<String>>(1)?;
                Ok(start.zip(end).map(|(start, end)| {
                    let inclusive_end = parse_rfc3339_utc_unix_ms(&end)
                        .map(|end_ms| rfc3339_utc_from_unix_ms(end_ms + 1_000))
                        .unwrap_or(end);
                    (start, inclusive_end)
                }))
            },
        )
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect raw_evidence timestamps for manifest scaffold: {error}"
            ))
        })
}

fn scaffold_evidence_summary(
    database_path: &PathBuf,
    start: &str,
    end: &str,
) -> GooseResult<ScaffoldEvidenceSummary> {
    let connection = open_read_only_database(database_path)?;
    let observed_capture_session_ids = query_observed_capture_session_ids(&connection, start, end)
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect capture_session_id values for manifest scaffold: {error}"
            ))
        })?;
    let raw_evidence_time_bounds = query_raw_evidence_time_bounds(&connection, start, end, None)
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect raw_evidence bounds for manifest scaffold: {error}"
            ))
        })?;
    let decoded_frame_time_bounds = query_decoded_frames_time_bounds(&connection, start, end, None)
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect decoded_frames bounds for manifest scaffold: {error}"
            ))
        })?;
    let packet_family_counts = query_decoded_packet_family_counts(&connection, start, end, None)
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect decoded packet families for manifest scaffold: {error}"
            ))
        })?;
    let mut capture_session_summaries = Vec::new();
    for session_id in &observed_capture_session_ids {
        let session_ids = vec![session_id.clone()];
        let raw_evidence_time_bounds = query_raw_evidence_time_bounds(
            &connection,
            start,
            end,
            Some(&session_ids),
        )
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect raw_evidence bounds for capture session {session_id}: {error}"
            ))
        })?;
        let decoded_frame_time_bounds = query_decoded_frames_time_bounds(
            &connection,
            start,
            end,
            Some(&session_ids),
        )
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect decoded_frames bounds for capture session {session_id}: {error}"
            ))
        })?;
        let packet_family_counts = query_decoded_packet_family_counts(
            &connection,
            start,
            end,
            Some(&session_ids),
        )
        .map_err(|error| {
            GooseError::message(format!(
                "cannot inspect decoded packet families for capture session {session_id}: {error}"
            ))
        })?;
        capture_session_summaries.push(LocalHealthValidationCaptureSessionSummary {
            session_id: session_id.clone(),
            raw_evidence_time_bounds,
            decoded_frame_time_bounds,
            packet_family_counts,
        });
    }
    Ok(ScaffoldEvidenceSummary {
        observed_capture_session_ids,
        raw_evidence_time_bounds,
        decoded_frame_time_bounds,
        packet_family_counts,
        capture_session_summaries,
    })
}

fn open_read_only_database(database_path: &PathBuf) -> GooseResult<Connection> {
    Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| {
        GooseError::message(format!(
            "cannot open {} for manifest scaffold: {error}",
            database_path.display()
        ))
    })
}

fn query_observed_capture_session_ids(
    connection: &Connection,
    start: &str,
    end: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        r#"
        SELECT DISTINCT capture_session_id
        FROM raw_evidence
        WHERE captured_at >= ?1
          AND captured_at < ?2
          AND capture_session_id IS NOT NULL
          AND TRIM(capture_session_id) != ''
        ORDER BY capture_session_id
        "#,
    )?;
    let rows = statement.query_map([start, end], |row| row.get::<_, String>(0))?;
    rows.collect()
}

fn query_raw_evidence_time_bounds(
    connection: &Connection,
    start: &str,
    end: &str,
    capture_session_ids: Option<&[String]>,
) -> rusqlite::Result<Option<LocalHealthValidationEvidenceTimeBounds>> {
    let session_clause = capture_session_ids
        .filter(|ids| !ids.is_empty())
        .map(|ids| {
            format!(
                " AND capture_session_id IN ({})",
                capture_session_sql_list(ids)
            )
        })
        .unwrap_or_default();
    query_time_bounds(
        connection,
        &format!(
            r#"
            SELECT MIN(captured_at), MAX(captured_at)
            FROM raw_evidence
            WHERE captured_at >= ?1
              AND captured_at < ?2
              {session_clause}
            "#
        ),
        start,
        end,
    )
}

fn query_decoded_frames_time_bounds(
    connection: &Connection,
    start: &str,
    end: &str,
    capture_session_ids: Option<&[String]>,
) -> rusqlite::Result<Option<LocalHealthValidationEvidenceTimeBounds>> {
    let session_clause = capture_session_ids
        .filter(|ids| !ids.is_empty())
        .map(|ids| {
            format!(
                " AND raw_evidence.capture_session_id IN ({})",
                capture_session_sql_list(ids)
            )
        })
        .unwrap_or_default();
    query_time_bounds(
        connection,
        &format!(
            r#"
            SELECT MIN(raw_evidence.captured_at), MAX(raw_evidence.captured_at)
            FROM decoded_frames
            INNER JOIN raw_evidence ON raw_evidence.evidence_id = decoded_frames.evidence_id
            WHERE raw_evidence.captured_at >= ?1
              AND raw_evidence.captured_at < ?2
              {session_clause}
            "#
        ),
        start,
        end,
    )
}

fn query_time_bounds(
    connection: &Connection,
    sql: &str,
    start: &str,
    end: &str,
) -> rusqlite::Result<Option<LocalHealthValidationEvidenceTimeBounds>> {
    let window_duration_ms = rfc3339_duration_ms(start, end);
    let window_start_unix_ms = parse_rfc3339_utc_unix_ms(start);
    let window_end_unix_ms = parse_rfc3339_utc_unix_ms(end);
    connection.query_row(sql, [start, end], |row| {
        let first_captured_at = row.get::<_, Option<String>>(0)?;
        let last_captured_at = row.get::<_, Option<String>>(1)?;
        Ok(first_captured_at
            .zip(last_captured_at)
            .map(|(first_captured_at, last_captured_at)| {
                evidence_time_bounds_from_first_last(
                    &first_captured_at,
                    &last_captured_at,
                    window_duration_ms,
                    window_start_unix_ms,
                    window_end_unix_ms,
                )
            }))
    })
}

fn evidence_time_bounds_from_first_last(
    first_captured_at: &str,
    last_captured_at: &str,
    window_duration_ms: Option<i64>,
    window_start_unix_ms: Option<i64>,
    window_end_unix_ms: Option<i64>,
) -> LocalHealthValidationEvidenceTimeBounds {
    let first_captured_at_unix_ms = parse_rfc3339_utc_unix_ms(first_captured_at);
    let last_captured_at_unix_ms = parse_rfc3339_utc_unix_ms(last_captured_at);
    let span_ms = rfc3339_duration_ms(first_captured_at, last_captured_at);
    let coverage_ratio = span_ms
        .zip(window_duration_ms)
        .map(|(span, duration)| span as f64 / duration as f64);
    let first_offset_from_case_start_ms = first_captured_at_unix_ms
        .zip(window_start_unix_ms)
        .and_then(|(captured_at, window_start)| {
            (captured_at >= window_start).then_some(captured_at - window_start)
        });
    let last_offset_before_case_end_ms =
        last_captured_at_unix_ms
            .zip(window_end_unix_ms)
            .and_then(|(captured_at, window_end)| {
                (window_end >= captured_at).then_some(window_end - captured_at)
            });
    LocalHealthValidationEvidenceTimeBounds {
        first_captured_at: first_captured_at.to_string(),
        last_captured_at: last_captured_at.to_string(),
        span_ms,
        coverage_ratio,
        first_offset_from_case_start_ms,
        last_offset_before_case_end_ms,
    }
}

fn query_decoded_packet_family_counts(
    connection: &Connection,
    start: &str,
    end: &str,
    capture_session_ids: Option<&[String]>,
) -> rusqlite::Result<BTreeMap<String, i64>> {
    let session_clause = capture_session_ids
        .filter(|ids| !ids.is_empty())
        .map(|ids| {
            format!(
                " AND raw_evidence.capture_session_id IN ({})",
                capture_session_sql_list(ids)
            )
        })
        .unwrap_or_default();
    let mut statement = connection.prepare(&format!(
        r#"
        SELECT decoded_frames.packet_type_name, decoded_frames.parsed_payload_json
        FROM decoded_frames
        INNER JOIN raw_evidence ON raw_evidence.evidence_id = decoded_frames.evidence_id
        WHERE raw_evidence.captured_at >= ?1
          AND raw_evidence.captured_at < ?2
          {session_clause}
        "#
    ))?;
    let rows = statement.query_map([start, end], |row| {
        Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let (packet_type_name, parsed_payload_json) = row?;
        let family =
            decoded_packet_family(packet_type_name.as_deref(), parsed_payload_json.as_str());
        *counts.entry(family).or_insert(0) += 1;
    }
    Ok(counts)
}

fn decoded_packet_family(packet_type_name: Option<&str>, parsed_payload_json: &str) -> String {
    let parsed_payload = serde_json::from_str::<Value>(parsed_payload_json).ok();
    let packet_k = parsed_payload
        .as_ref()
        .and_then(|payload| payload.get("packet_k"))
        .and_then(Value::as_u64);
    let domain = parsed_payload
        .as_ref()
        .and_then(|payload| str_field(payload, &["domain"]));
    let body_summary_kind = parsed_payload
        .as_ref()
        .and_then(|payload| payload.get("body_summary"))
        .and_then(|body| str_field(body, &["kind"]));
    if let Some(packet_k) = packet_k {
        if let Some(domain) = domain.as_deref().filter(|value| !value.trim().is_empty()) {
            return format!("K{packet_k}/{domain}");
        }
        if let Some(kind) = body_summary_kind
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return format!("K{packet_k}/{kind}");
        }
        return format!("K{packet_k}");
    }
    packet_type_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn capture_session_sql_list(capture_session_ids: &[String]) -> String {
    capture_session_ids
        .iter()
        .map(|session_id| sql_string_literal(session_id))
        .collect::<Vec<_>>()
        .join(",")
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn date_key_from_rfc3339_start(start: &str) -> Option<String> {
    non_empty_string(start.split_once('T').map(|(date, _)| date))
}

fn non_empty_string(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn scaffold_validation_cases(packet_family_counts: &BTreeMap<String, i64>) -> Vec<Value> {
    let mut cases = Vec::new();
    if packet_family_prefix_present(packet_family_counts, &["K10", "K11", "K21"]) {
        cases.extend(scaffold_step_cases());
    }
    if packet_family_prefix_present(packet_family_counts, &["K2", "K10", "K11", "K18", "K21"]) {
        cases.extend(scaffold_energy_cases());
    }
    if packet_family_prefix_present(packet_family_counts, &["K2", "K18", "K24"]) {
        cases.extend(scaffold_rhr_cases());
    }
    if packet_family_prefix_present(packet_family_counts, &["K17", "K18", "K24", "EVENT"]) {
        cases.extend(scaffold_recovery_cases());
    }
    if cases.is_empty() {
        cases.extend(scaffold_step_cases());
        cases.push(json!({
            "id": "owned-recovery-sensors",
            "report": "recovery-sensors",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2
        }));
    }
    cases
}

fn packet_family_prefix_present(
    packet_family_counts: &BTreeMap<String, i64>,
    prefixes: &[&str],
) -> bool {
    packet_family_counts.keys().any(|family| {
        prefixes
            .iter()
            .any(|prefix| packet_family_matches_prefix(family, prefix))
    })
}

fn packet_family_matches_prefix(family: &str, required_prefix: &str) -> bool {
    family
        .strip_prefix(required_prefix)
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('/'))
}

fn scaffold_step_cases() -> Vec<Value> {
    vec![
        json!({
            "id": "owned-step-discovery",
            "report": "step-discovery",
            "capture_kind": "owned_capture",
            "max_candidate_fields": 250
        }),
        json!({
            "id": "owned-step-validation",
            "report": "step-validation",
            "capture_kind": "owned_capture",
            "manual_step_delta": Value::Null,
            "official_whoop_step_delta": Value::Null,
            "step_delta_tolerance": 5
        }),
        json!({
            "id": "owned-raw-motion-steps",
            "report": "raw-motion-steps",
            "capture_kind": "owned_capture",
            "manual_step_delta": Value::Null,
            "official_whoop_step_delta": Value::Null,
            "step_delta_tolerance": 15,
            "sample_rate_hz": 50.0,
            "peak_threshold_i16": 1200.0,
            "min_peak_spacing_samples": 10,
            "require_trusted_evidence": true
        }),
    ]
}

fn scaffold_energy_cases() -> Vec<Value> {
    vec![
        json!({
            "id": "owned-energy-rollup",
            "report": "energy-rollup",
            "capture_kind": "owned_capture",
            "profile_weight_kg": Value::Null,
            "profile_age_years": Value::Null,
            "profile_sex": Value::Null,
            "resting_hr_bpm": Value::Null,
            "max_hr_bpm": Value::Null,
            "min_heart_rate_samples": 2
        }),
        json!({
            "id": "owned-energy-validation",
            "report": "energy-validation",
            "capture_kind": "owned_capture",
            "profile_weight_kg": Value::Null,
            "profile_age_years": Value::Null,
            "profile_sex": Value::Null,
            "resting_hr_bpm": Value::Null,
            "max_hr_bpm": Value::Null,
            "official_whoop_total_kcal": Value::Null,
            "energy_tolerance_kcal": 250.0,
            "energy_relative_tolerance": 0.25
        }),
    ]
}

fn scaffold_rhr_cases() -> Vec<Value> {
    vec![
        json!({
            "id": "owned-rhr-rollup",
            "report": "rhr-rollup",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_sample_count": 2
        }),
        json!({
            "id": "owned-rhr-validation",
            "report": "rhr-validation",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_sample_count": 2,
            "official_whoop_resting_hr_bpm": Value::Null,
            "rhr_tolerance_bpm": 3.0
        }),
    ]
}

fn scaffold_recovery_cases() -> Vec<Value> {
    vec![
        json!({
            "id": "owned-hrv-validation",
            "report": "hrv-validation",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2,
            "official_whoop_hrv_rmssd_ms": Value::Null,
            "hrv_tolerance_ms": 10.0
        }),
        json!({
            "id": "owned-respiratory-rate-validation",
            "report": "respiratory-rate-validation",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "official_whoop_respiratory_rate_rpm": Value::Null,
            "respiratory_rate_tolerance_rpm": 1.0
        }),
        json!({
            "id": "owned-oxygen-saturation-validation",
            "report": "spo2-validation",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "official_whoop_oxygen_saturation_percent": Value::Null,
            "oxygen_saturation_tolerance_percent": 2.0
        }),
        json!({
            "id": "owned-temperature-validation",
            "report": "temperature-validation",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "official_whoop_skin_temperature_delta_c": Value::Null,
            "temperature_tolerance_c": 0.3
        }),
        json!({
            "id": "owned-recovery-sensor-rollup",
            "report": "recovery-sensor-rollup",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2
        }),
        json!({
            "id": "owned-recovery-sensors",
            "report": "recovery-sensors",
            "capture_kind": "owned_capture",
            "require_trusted_evidence": true,
            "min_rr_intervals_to_compute": 2
        }),
    ]
}

fn scaffold_run_validation(options: &LocalHealthValidationManifestScaffoldOptions) -> Value {
    let manifest_path = "local-health-validation-manifest.json";
    let json_report_path = "local-health-validation-report.json";
    let markdown_report_path = "local-health-validation-report.md";
    let review_report_path = "local-health-validation-review.json";
    let mut args = vec!["goose-local-health-validation-suite".to_string()];
    if let Some(bundle_path) = &options.raw_export_bundle_path {
        args.push("--raw-export-bundle".to_string());
        args.push(bundle_path.display().to_string());
    } else if options
        .database_source_kind
        .as_deref()
        .is_some_and(|kind| kind.starts_with("raw_export"))
    {
        args.push("--raw-export-bundle".to_string());
        args.push("<raw-export-bundle>".to_string());
    } else {
        args.push("--database".to_string());
        args.push(options.database_path.display().to_string());
    }
    args.push("--manifest".to_string());
    args.push(manifest_path.to_string());
    args.push("--output".to_string());
    args.push(json_report_path.to_string());
    args.push("--markdown-output".to_string());
    args.push(markdown_report_path.to_string());
    args.push("--review-output".to_string());
    args.push(review_report_path.to_string());
    let command = args
        .iter()
        .map(|arg| shell_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    json!({
        "cli": "goose-local-health-validation-suite",
        "args": args,
        "manifest_path": manifest_path,
        "json_report_path": json_report_path,
        "markdown_report_path": markdown_report_path,
        "review_report_path": review_report_path,
        "command": command,
        "official_whoop_values_are_validation_labels_not_inputs": true
    })
}

fn scaffold_operator_checklist(
    evidence: &ScaffoldEvidenceSummary,
    cases: &[Value],
    run_validation: &Value,
) -> Vec<Value> {
    let capture_session_status = match evidence.observed_capture_session_ids.len() {
        0 => "no_capture_session_ids_observed",
        1 => "single_capture_session_defaulted",
        _ => "case_binding_required",
    };
    let capture_session_action = match evidence.observed_capture_session_ids.len() {
        0 => {
            "Import or export the owned capture with capture_session_id values before treating labeled cases as acceptance."
        }
        1 => {
            "Review the default capture_session_id and keep it only if every case belongs to that owned capture."
        }
        _ => {
            "Copy the correct session_id from generated_evidence.capture_session_summaries into each case that should validate that controlled still, walk, workout, charger, or overnight capture."
        }
    };
    let placeholder_fields = scaffold_placeholder_fields(cases);
    let placeholder_action = if placeholder_fields.is_empty() {
        "No generated placeholder fields were needed for these cases.".to_string()
    } else {
        format!(
            "Fill these generated placeholders from manual counts, profile values, and official WHOOP app screenshots used only as labels: {}.",
            placeholder_fields.join(", ")
        )
    };
    let command = run_validation
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or(
            "goose-local-health-validation-suite --manifest local-health-validation-manifest.json",
        );

    vec![
        json!({
            "id": "bind_capture_sessions",
            "status": capture_session_status,
            "observed_capture_session_ids": evidence.observed_capture_session_ids,
            "action": capture_session_action
        }),
        json!({
            "id": "fill_validation_placeholders",
            "status": if placeholder_fields.is_empty() { "not_needed" } else { "required_before_acceptance" },
            "fields": placeholder_fields,
            "official_whoop_values_are_validation_labels_not_inputs": true,
            "action": placeholder_action
        }),
        json!({
            "id": "run_validation_suite",
            "status": "ready_after_manifest_edits",
            "command": command,
            "action": "Run the validation suite after filling labels/profile fields and binding each controlled case to the intended capture_session_id. Keep the JSON report, Markdown summary, and manifest review JSON with the controlled-capture evidence."
        }),
    ]
}

fn scaffold_placeholder_fields(cases: &[Value]) -> Vec<String> {
    let mut fields = BTreeSet::new();
    for case in cases {
        let Some(object) = case.as_object() else {
            continue;
        };
        for (key, value) in object {
            if value.is_null() {
                fields.insert(key.to_string());
            }
        }
    }
    fields.into_iter().collect()
}

fn shell_arg(value: &str) -> String {
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/' | ':')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn rfc3339_duration_ms(start: &str, end: &str) -> Option<i64> {
    let (start, end) = parse_rfc3339_utc_unix_ms(start).zip(parse_rfc3339_utc_unix_ms(end))?;
    (end >= start).then_some(end - start)
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

fn rfc3339_utc_from_unix_ms(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1_000);
    let millis = unix_ms.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    if millis == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
    }
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}
