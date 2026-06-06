use std::{collections::BTreeSet, fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    GooseError, GooseResult,
    algorithm_compare::{
        ALGORITHM_COMPARISON_SCHEMA, AlgorithmComparisonDelta, AlgorithmComparisonReport,
        SLEEP_V1_BENCHMARK_COMPARISON_POLICY, SLEEP_V1_BENCHMARK_REPORT_INTEGRITY_POLICY,
        algorithm_comparison_next_actions, compare_sleep_v1_goose_to_reference,
        sleep_v1_benchmark_acceptance_summary,
    },
    historical_sync::{
        HISTORICAL_SYNC_PHYSICAL_EVIDENCE_TEMPLATE_SCHEMA,
        HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY,
        HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY,
        HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA,
        HISTORICAL_SYNC_PHYSICAL_VALIDATION_SCHEMA, HistoricalSyncPhysicalEvidenceTemplate,
        HistoricalSyncPhysicalValidationInput, HistoricalSyncPhysicalValidationReport,
        historical_sync_physical_acceptance_summary, physical_validation_next_actions,
        validate_historical_sync_physical_evidence,
    },
    metric_features::{
        HEART_RATE_FEATURE_REPORT_SCHEMA, MOTION_FEATURE_REPORT_SCHEMA,
        SLEEP_FEATURE_SCORE_REPORT_SCHEMA, SleepFeatureScoreOptions, SleepFeatureScoreReport,
        SleepWindowFeature, run_sleep_feature_score_report_for_store,
    },
    metrics::{
        GOOSE_SLEEP_V0_ID, GOOSE_SLEEP_V0_VERSION, GOOSE_SLEEP_V1_ID, GOOSE_SLEEP_V1_VERSION,
        SleepInput, SleepV1Input, SleepV1Output, goose_sleep_v0, goose_sleep_v1,
    },
    reference::{REFERENCE_SLEEP_ACTIGRAPHY_ID, REFERENCE_SLEEP_ACTIGRAPHY_VERSION},
    store::{GooseStore, SleepCorrectionLabelRow},
};

pub const SLEEP_WINDOW_LABEL_VALIDATION_SCHEMA: &str =
    "goose.sleep-window-label-validation-report.v1";
pub const SLEEP_WINDOW_LABEL_VALIDATION_INPUT_SCHEMA: &str =
    "goose.sleep-window-label-validation-input.v1";
pub const SLEEP_STAGE_LABEL_VALIDATION_SCHEMA: &str =
    "goose.sleep-stage-label-validation-report.v1";
pub const SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY: &str =
    "sleep_stage_label_validation_requires_current_stage_comparison_integrity";
pub const SLEEP_WINDOW_LABEL_REPORT_INTEGRITY_POLICY: &str =
    "sleep_window_label_validation_requires_current_feature_and_comparison_integrity";
pub const SLEEP_WINDOW_LABEL_VALIDATION_POLICY: &str =
    "packet_derived_sleep_window_vs_hand_reviewed_sleep_window_label";
pub const SLEEP_STAGE_LABEL_VALIDATION_POLICY: &str =
    "sleep_v1_stage_segments_vs_user_owned_sleep_stage_labels";
pub const SLEEP_V1_EXPLANATION_STABILITY_SCHEMA: &str =
    "goose.sleep-v1-explanation-stability-report.v1";
pub const SLEEP_V1_EXPLANATION_STABILITY_INTEGRITY_POLICY: &str =
    "sleep_v1_explanation_stability_requires_current_component_status_and_delta_integrity";
pub const SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY: &str =
    "sleep_v1_explanation_completeness_and_score_stability";
pub const SLEEP_V1_RELEASE_GATE_SCHEMA: &str = "goose.sleep-v1-release-gate-report.v1";
pub const SLEEP_V1_EVIDENCE_FOLDER_SCHEMA: &str =
    "goose.sleep-v1-validation-evidence-folder-report.v1";
const SLEEP_V1_RELEASE_GATE_THRESHOLD_POLICY: &str =
    "sleep_v1_primary_release_uses_default_or_stricter_review_stage_and_benchmark_thresholds";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1EvidenceFolderReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub evidence_dir: String,
    pub required_file_count: usize,
    pub passing_required_file_count: usize,
    pub required_files: Vec<SleepV1EvidenceFileReport>,
    pub supporting_file_count: usize,
    pub passing_supporting_file_count: usize,
    pub supporting_files: Vec<SleepV1EvidenceSupportingFileReport>,
    #[serde(default)]
    pub unexpected_files: Vec<String>,
    pub derivation_check_count: usize,
    pub passing_derivation_check_count: usize,
    pub derivation_checks: Vec<SleepV1EvidenceDerivationCheck>,
    pub evidence_manifest_sha256: Option<String>,
    pub expected_evidence_manifest_sha256: Option<String>,
    pub issues: Vec<String>,
    pub next_actions: Vec<SleepV1EvidenceFolderNextAction>,
    #[serde(default)]
    pub acceptance_summary: SleepV1EvidenceFolderAcceptanceSummary,
    pub provenance: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SleepV1EvidenceFolderAcceptanceSummary {
    pub policy: String,
    pub evidence_folder_ready: bool,
    pub evidence_dir: String,
    pub required_file_count: usize,
    pub passing_required_file_count: usize,
    pub supporting_file_count: usize,
    pub passing_supporting_file_count: usize,
    pub derivation_check_count: usize,
    pub passing_derivation_check_count: usize,
    pub unexpected_file_count: usize,
    pub evidence_manifest_sha256: Option<String>,
    pub expected_evidence_manifest_sha256: Option<String>,
    pub issue_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1EvidenceFileReport {
    pub filename: String,
    pub path: String,
    pub byte_size: Option<u64>,
    pub sha256: Option<String>,
    pub expected_schema: String,
    pub schema: Option<String>,
    pub expected_generated_by: Option<String>,
    pub generated_by: Option<String>,
    pub exists: bool,
    pub schema_pass: bool,
    pub generated_by_pass: bool,
    pub pass_field_present: bool,
    pub pass: Option<bool>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1EvidenceSupportingFileReport {
    pub filename: String,
    pub path: String,
    pub byte_size: Option<u64>,
    pub sha256: Option<String>,
    pub expected_schema: Option<String>,
    pub schema: Option<String>,
    pub exists: bool,
    pub schema_pass: bool,
    pub contract_pass: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1EvidenceDerivationCheck {
    pub name: String,
    pub source_files: Vec<String>,
    pub report_file: String,
    pub pass: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepWindowLabelValidationEvidenceInput {
    pub schema: String,
    pub database_path: String,
    pub start: String,
    pub end: String,
    pub options: SleepWindowLabelValidationOptions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SleepV1EvidenceFolderOptions {
    pub expected_evidence_manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepV1EvidenceFolderNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

pub fn validate_sleep_v1_evidence_folder(
    evidence_dir: &Path,
) -> GooseResult<SleepV1EvidenceFolderReport> {
    validate_sleep_v1_evidence_folder_with_options(
        evidence_dir,
        SleepV1EvidenceFolderOptions::default(),
    )
}

pub fn validate_sleep_v1_evidence_folder_with_options(
    evidence_dir: &Path,
    options: SleepV1EvidenceFolderOptions,
) -> GooseResult<SleepV1EvidenceFolderReport> {
    let mut file_reports = Vec::new();
    let mut issues = Vec::new();

    for (filename, expected_schema) in sleep_v1_required_evidence_files() {
        let path = evidence_dir.join(filename);
        let file_report = validate_sleep_v1_evidence_file(&path, filename, expected_schema);
        issues.extend(file_report.issues.clone());
        file_reports.push(file_report);
    }
    let mut supporting_file_reports = Vec::new();
    for (filename, expected_schema, kind) in sleep_v1_required_supporting_files() {
        let path = evidence_dir.join(filename);
        let file_report = validate_sleep_v1_supporting_file(&path, filename, expected_schema, kind);
        issues.extend(file_report.issues.clone());
        supporting_file_reports.push(file_report);
    }
    let unexpected_files = sleep_v1_unexpected_evidence_files(evidence_dir);
    for filename in &unexpected_files {
        issues.push(format!("unexpected_evidence_file:{filename}"));
    }
    let derivation_checks = validate_sleep_v1_evidence_derivations(evidence_dir);
    for check in &derivation_checks {
        issues.extend(check.issues.clone());
    }

    let passing_required_file_count = file_reports
        .iter()
        .filter(|file| {
            file.exists
                && file.schema_pass
                && file.generated_by_pass
                && file.pass == Some(true)
                && file.issues.is_empty()
        })
        .count();
    let passing_supporting_file_count = supporting_file_reports
        .iter()
        .filter(|file| {
            file.exists && file.schema_pass && file.contract_pass && file.issues.is_empty()
        })
        .count();
    let passing_derivation_check_count =
        derivation_checks.iter().filter(|check| check.pass).count();
    let evidence_manifest_sha256 =
        sleep_v1_evidence_manifest_sha256(&file_reports, &supporting_file_reports);
    if let Some(expected) = options.expected_evidence_manifest_sha256.as_deref() {
        if !is_sha256_hex(expected) {
            issues.push("expected_manifest_sha256_invalid".to_string());
        } else if evidence_manifest_sha256.as_deref() != Some(expected) {
            issues.push("evidence_manifest_sha256_mismatch".to_string());
        }
    }
    issues.sort();
    issues.dedup();
    let pass = passing_required_file_count == file_reports.len()
        && passing_supporting_file_count == supporting_file_reports.len()
        && passing_derivation_check_count == derivation_checks.len()
        && issues.is_empty();

    let next_actions = sleep_v1_evidence_folder_next_actions(&issues);

    let mut report = SleepV1EvidenceFolderReport {
        schema: SLEEP_V1_EVIDENCE_FOLDER_SCHEMA.to_string(),
        generated_by: "goose-sleep-v1-evidence-folder-validator".to_string(),
        pass,
        evidence_dir: evidence_dir.display().to_string(),
        required_file_count: file_reports.len(),
        passing_required_file_count,
        required_files: file_reports,
        supporting_file_count: supporting_file_reports.len(),
        passing_supporting_file_count,
        supporting_files: supporting_file_reports,
        unexpected_files,
        derivation_check_count: derivation_checks.len(),
        passing_derivation_check_count,
        derivation_checks,
        evidence_manifest_sha256,
        expected_evidence_manifest_sha256: options.expected_evidence_manifest_sha256,
        acceptance_summary: SleepV1EvidenceFolderAcceptanceSummary::default(),
        next_actions,
        issues,
        provenance: json!({
            "validation_policy": "sleep_v1_requires_complete_auditable_evidence_folder",
            "derivation_policy": "source_inputs_must_recompute_generated_reports",
            "required_report_integrity_policy": "required_reports_must_pass_schema_generator_status_and_component_integrity",
            "required_report_integrity_policies": {
                "historical-sync-validation.json": HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY,
                "sleep-window-validation.json": "sleep_window_label_validation_requires_current_feature_and_comparison_integrity",
                "sleep-stage-validation.json": SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY,
                "sleep-v1-stability.json": "sleep_v1_explanation_stability_requires_current_component_status_and_delta_integrity",
                "sleep-v1-benchmark.json": "sleep_v1_benchmark_requires_current_comparison_output_and_delta_integrity",
                "sleep-v1-release-gate.json": "sleep_v1_release_gate_requires_current_subgate_integrity_and_empty_proof_arrays",
            },
            "required_report_validation_policies": {
                "historical-sync-validation.json": HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY,
                "sleep-window-validation.json": SLEEP_WINDOW_LABEL_VALIDATION_POLICY,
                "sleep-stage-validation.json": SLEEP_STAGE_LABEL_VALIDATION_POLICY,
                "sleep-v1-stability.json": SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY,
                "sleep-v1-benchmark.json": SLEEP_V1_BENCHMARK_COMPARISON_POLICY,
            },
            "expected_manifest_policy": "expected_manifest_sha256_must_match_when_provided",
            "required_report_filenames": sleep_v1_required_evidence_files()
                .into_iter()
                .map(|(filename, _)| filename)
                .collect::<Vec<_>>(),
            "required_supporting_filenames": sleep_v1_required_supporting_files()
                .into_iter()
                .map(|(filename, _, _)| filename)
                .collect::<Vec<_>>(),
            "unexpected_file_policy": "only_required_sleep_v1_evidence_files_are_allowed",
        }),
    };
    report.acceptance_summary = sleep_v1_evidence_folder_acceptance_summary(&report);
    Ok(report)
}

fn sleep_v1_evidence_folder_acceptance_summary(
    report: &SleepV1EvidenceFolderReport,
) -> SleepV1EvidenceFolderAcceptanceSummary {
    SleepV1EvidenceFolderAcceptanceSummary {
        policy:
            "sleep_v1_evidence_folder_must_match_required_files_derivations_manifest_and_actions"
                .to_string(),
        evidence_folder_ready: report.pass,
        evidence_dir: report.evidence_dir.clone(),
        required_file_count: report.required_file_count,
        passing_required_file_count: report.passing_required_file_count,
        supporting_file_count: report.supporting_file_count,
        passing_supporting_file_count: report.passing_supporting_file_count,
        derivation_check_count: report.derivation_check_count,
        passing_derivation_check_count: report.passing_derivation_check_count,
        unexpected_file_count: report.unexpected_files.len(),
        evidence_manifest_sha256: report.evidence_manifest_sha256.clone(),
        expected_evidence_manifest_sha256: report.expected_evidence_manifest_sha256.clone(),
        issue_count: report.issues.len(),
        next_action_count: report.next_actions.len(),
    }
}

fn sleep_v1_required_evidence_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "historical-sync-validation.json",
            HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA,
        ),
        (
            "sleep-window-validation.json",
            SLEEP_WINDOW_LABEL_VALIDATION_SCHEMA,
        ),
        (
            "sleep-stage-validation.json",
            SLEEP_STAGE_LABEL_VALIDATION_SCHEMA,
        ),
        (
            "sleep-v1-stability.json",
            SLEEP_V1_EXPLANATION_STABILITY_SCHEMA,
        ),
        ("sleep-v1-benchmark.json", ALGORITHM_COMPARISON_SCHEMA),
        ("sleep-v1-release-gate.json", SLEEP_V1_RELEASE_GATE_SCHEMA),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SleepV1SupportingFileKind {
    HistoricalSyncTemplate,
    HistoricalSyncEvidence,
    SleepWindowValidationInput,
    SleepWindowStore,
    SleepV1Input,
    SleepV1ReleaseGateInput,
}

fn sleep_v1_required_supporting_files() -> Vec<(
    &'static str,
    Option<&'static str>,
    SleepV1SupportingFileKind,
)> {
    vec![
        (
            "historical-sync-template.json",
            Some(HISTORICAL_SYNC_PHYSICAL_EVIDENCE_TEMPLATE_SCHEMA),
            SleepV1SupportingFileKind::HistoricalSyncTemplate,
        ),
        (
            "historical-sync-evidence.json",
            Some(HISTORICAL_SYNC_PHYSICAL_VALIDATION_SCHEMA),
            SleepV1SupportingFileKind::HistoricalSyncEvidence,
        ),
        (
            "sleep-window-validation-input.json",
            Some(SLEEP_WINDOW_LABEL_VALIDATION_INPUT_SCHEMA),
            SleepV1SupportingFileKind::SleepWindowValidationInput,
        ),
        (
            "sleep-window-store.sqlite",
            None,
            SleepV1SupportingFileKind::SleepWindowStore,
        ),
        (
            "sleep-v1-input.json",
            None,
            SleepV1SupportingFileKind::SleepV1Input,
        ),
        (
            "sleep-v1-release-gate-input.json",
            None,
            SleepV1SupportingFileKind::SleepV1ReleaseGateInput,
        ),
    ]
}

const SLEEP_V1_EVIDENCE_SLEEP_WINDOW_STORE: &str = "sleep-window-store.sqlite";

fn sleep_v1_unexpected_evidence_files(evidence_dir: &Path) -> Vec<String> {
    let allowed = sleep_v1_required_evidence_files()
        .into_iter()
        .map(|(filename, _)| filename.to_string())
        .chain(
            sleep_v1_required_supporting_files()
                .into_iter()
                .map(|(filename, _, _)| filename.to_string()),
        )
        .collect::<BTreeSet<_>>();
    let Ok(entries) = fs::read_dir(evidence_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|filename| !filename.starts_with('.') && !allowed.contains(filename))
        .collect()
}

fn sleep_v1_evidence_manifest_sha256(
    required_files: &[SleepV1EvidenceFileReport],
    supporting_files: &[SleepV1EvidenceSupportingFileReport],
) -> Option<String> {
    let mut lines = Vec::new();
    for file in required_files {
        lines.push(evidence_manifest_line(
            "report",
            &file.filename,
            file.byte_size,
            file.sha256.as_deref(),
        )?);
    }
    for file in supporting_files {
        lines.push(evidence_manifest_line(
            "supporting",
            &file.filename,
            file.byte_size,
            file.sha256.as_deref(),
        )?);
    }
    lines.sort();
    Some(sha256_hex(lines.join("\n").as_bytes()))
}

fn evidence_manifest_line(
    kind: &str,
    filename: &str,
    byte_size: Option<u64>,
    sha256: Option<&str>,
) -> Option<String> {
    Some(format!("{kind}\t{filename}\t{}\t{}", byte_size?, sha256?))
}

fn validate_sleep_v1_evidence_file(
    path: &Path,
    filename: &str,
    expected_schema: &str,
) -> SleepV1EvidenceFileReport {
    let mut issues = Vec::new();
    if !path.exists() {
        issues.push(format!("missing_required_file:{filename}"));
        return SleepV1EvidenceFileReport {
            filename: filename.to_string(),
            path: path.display().to_string(),
            byte_size: None,
            sha256: None,
            expected_schema: expected_schema.to_string(),
            schema: None,
            expected_generated_by: expected_sleep_v1_report_generator(filename).map(str::to_string),
            generated_by: None,
            exists: false,
            schema_pass: false,
            generated_by_pass: false,
            pass_field_present: false,
            pass: None,
            issues,
        };
    }
    issues.extend(sleep_v1_evidence_file_path_issues(
        path, filename, "required",
    ));

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            issues.push(format!("unreadable_required_file:{filename}:{error}"));
            return SleepV1EvidenceFileReport {
                filename: filename.to_string(),
                path: path.display().to_string(),
                byte_size: None,
                sha256: None,
                expected_schema: expected_schema.to_string(),
                schema: None,
                expected_generated_by: expected_sleep_v1_report_generator(filename)
                    .map(str::to_string),
                generated_by: None,
                exists: true,
                schema_pass: false,
                generated_by_pass: false,
                pass_field_present: false,
                pass: None,
                issues,
            };
        }
    };
    let byte_size = Some(bytes.len() as u64);
    let sha256 = Some(sha256_hex(&bytes));
    let raw = String::from_utf8_lossy(&bytes);
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            issues.push(format!("invalid_required_file_json:{filename}:{error}"));
            return SleepV1EvidenceFileReport {
                filename: filename.to_string(),
                path: path.display().to_string(),
                byte_size,
                sha256,
                expected_schema: expected_schema.to_string(),
                schema: None,
                expected_generated_by: expected_sleep_v1_report_generator(filename)
                    .map(str::to_string),
                generated_by: None,
                exists: true,
                schema_pass: false,
                generated_by_pass: false,
                pass_field_present: false,
                pass: None,
                issues,
            };
        }
    };

    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .map(str::to_string);
    let schema_pass = schema.as_deref() == Some(expected_schema);
    if !schema_pass {
        issues.push(format!("schema_mismatch:{filename}"));
    }
    let expected_generated_by = expected_sleep_v1_report_generator(filename).map(str::to_string);
    let generated_by = value
        .get("generated_by")
        .and_then(Value::as_str)
        .map(str::to_string);
    let generated_by_pass = expected_generated_by
        .as_deref()
        .is_none_or(|expected| generated_by.as_deref() == Some(expected));
    if !generated_by_pass {
        issues.push(format!("generated_by_mismatch:{filename}"));
    }

    let pass = value.get("pass").and_then(Value::as_bool);
    if pass.is_none() {
        issues.push(format!("missing_pass_field:{filename}"));
    } else if pass != Some(true) {
        issues.push(format!("required_file_not_passing:{filename}"));
    }
    let report_issues = value.get("issues").and_then(Value::as_array);
    if pass == Some(true) {
        match report_issues {
            Some(report_issues) if report_issues.is_empty() => {}
            Some(_) => issues.push(format!("passing_required_file_has_issues:{filename}")),
            None => issues.push(format!("passing_required_file_missing_issues:{filename}")),
        }
        match value.get("next_actions").and_then(Value::as_array) {
            Some(next_actions) if next_actions.is_empty() => {}
            Some(_) => issues.push(format!("passing_required_file_has_next_actions:{filename}")),
            None => issues.push(format!(
                "passing_required_file_missing_next_actions:{filename}"
            )),
        }
        match value.get("errors").and_then(Value::as_array) {
            Some(errors) if errors.is_empty() => {}
            Some(_) => issues.push(format!("passing_required_file_has_errors:{filename}")),
            None => issues.push(format!("passing_required_file_missing_errors:{filename}")),
        }
        match value.get("quality_flags").and_then(Value::as_array) {
            Some(quality_flags) if quality_flags.is_empty() => {}
            Some(_) => issues.push(format!(
                "passing_required_file_has_quality_flags:{filename}:quality_flags"
            )),
            None => issues.push(format!(
                "passing_required_file_missing_quality_flags:{filename}:quality_flags"
            )),
        }
        if filename == "sleep-v1-benchmark.json" {
            for field in ["goose_quality_flags", "reference_quality_flags"] {
                match value.get(field).and_then(Value::as_array) {
                    Some(quality_flags) if quality_flags.is_empty() => {}
                    Some(_) => issues.push(format!(
                        "passing_required_file_has_quality_flags:{filename}:{field}"
                    )),
                    None => issues.push(format!(
                        "passing_required_file_missing_quality_flags:{filename}:{field}"
                    )),
                }
            }
        }
    }

    if filename == "sleep-v1-release-gate.json" {
        for subgate in [
            "physical_historical_sync_pass",
            "timestamp_evidence_pass",
            "sleep_window_label_pass",
            "sleep_stage_label_pass",
            "explanation_stability_pass",
            "benchmark_comparison_pass",
        ] {
            if value.get(subgate).and_then(Value::as_bool) != Some(true) {
                issues.push(format!("release_gate_subgate_not_passing:{subgate}"));
            }
        }
        issues.extend(sleep_v1_release_gate_file_integrity_issues(&value));
    }
    if filename == "historical-sync-validation.json" {
        issues.extend(historical_sync_physical_file_integrity_issues(&value));
    }
    if filename == "sleep-window-validation.json" {
        issues.extend(sleep_window_label_file_integrity_issues(&value));
    }
    if filename == "sleep-stage-validation.json" {
        issues.extend(sleep_stage_label_file_integrity_issues(&value));
    }
    if filename == "sleep-v1-stability.json" {
        issues.extend(sleep_v1_stability_file_integrity_issues(&value));
    }
    if filename == "sleep-v1-benchmark.json" {
        issues.extend(sleep_v1_benchmark_file_integrity_issues(&value));
    }

    issues.sort();
    issues.dedup();
    SleepV1EvidenceFileReport {
        filename: filename.to_string(),
        path: path.display().to_string(),
        byte_size,
        sha256,
        expected_schema: expected_schema.to_string(),
        schema,
        expected_generated_by,
        generated_by,
        exists: true,
        schema_pass,
        generated_by_pass,
        pass_field_present: pass.is_some(),
        pass,
        issues,
    }
}

fn historical_sync_physical_file_integrity_issues(value: &Value) -> Vec<String> {
    let Ok(report) =
        serde_json::from_value::<HistoricalSyncPhysicalValidationReport>(value.clone())
    else {
        return vec!["physical_historical_sync_report_unparseable".to_string()];
    };
    if historical_sync_physical_report_integrity_pass(&report) {
        Vec::new()
    } else {
        vec!["physical_historical_sync_report_integrity_failed".to_string()]
    }
}

fn sleep_window_label_file_integrity_issues(value: &Value) -> Vec<String> {
    let Ok(report) = serde_json::from_value::<SleepWindowLabelValidationReport>(value.clone())
    else {
        return vec!["sleep_window_label_report_unparseable".to_string()];
    };
    if sleep_window_label_report_integrity_pass(&report)
        && sleep_window_label_thresholds_at_least_default(&report)
    {
        Vec::new()
    } else {
        vec!["sleep_window_label_report_integrity_failed".to_string()]
    }
}

fn sleep_stage_label_file_integrity_issues(value: &Value) -> Vec<String> {
    let Ok(report) = serde_json::from_value::<SleepStageLabelValidationReport>(value.clone())
    else {
        return vec!["sleep_stage_label_report_unparseable".to_string()];
    };
    if sleep_stage_label_report_integrity_pass(&report)
        && sleep_stage_label_thresholds_at_least_default(&report)
    {
        Vec::new()
    } else {
        vec!["sleep_stage_label_report_integrity_failed".to_string()]
    }
}

fn sleep_v1_stability_file_integrity_issues(value: &Value) -> Vec<String> {
    let Ok(report) = serde_json::from_value::<SleepV1ExplanationStabilityReport>(value.clone())
    else {
        return vec!["stability_report_unparseable".to_string()];
    };
    let mut issues = Vec::new();
    if !sleep_v1_explanation_stability_report_integrity_pass(&report) {
        issues.push("stability_report_integrity_failed".to_string());
    }
    if !sleep_v1_explanation_stability_thresholds_at_least_default(&report) {
        issues.push("stability_report_threshold_below_default".to_string());
    }
    issues
}

fn sleep_v1_benchmark_file_integrity_issues(value: &Value) -> Vec<String> {
    let Ok(report) = serde_json::from_value::<AlgorithmComparisonReport>(value.clone()) else {
        return vec!["benchmark_report_unparseable".to_string()];
    };
    if sleep_v1_benchmark_report_ready(&report) {
        Vec::new()
    } else {
        vec!["benchmark_report_integrity_failed".to_string()]
    }
}

fn sleep_v1_release_gate_file_integrity_issues(value: &Value) -> Vec<String> {
    let mut issues = Vec::new();
    let Ok(report) = serde_json::from_value::<SleepV1ReleaseGateReport>(value.clone()) else {
        return vec!["release_gate_report_unparseable".to_string()];
    };
    if report
        .provenance
        .get("promotion_policy")
        .and_then(Value::as_str)
        != Some("sleep_v1_primary_requires_all_release_gates")
    {
        issues.push("release_gate_report_promotion_policy_missing".to_string());
    }
    if report
        .provenance
        .get("report_integrity_policy")
        .and_then(Value::as_str)
        != Some("sleep_v1_release_gate_requires_current_subgate_integrity_and_empty_proof_arrays")
    {
        issues.push("release_gate_report_integrity_policy_missing".to_string());
    }
    if report
        .provenance
        .get("threshold_policy")
        .and_then(Value::as_str)
        != Some(SLEEP_V1_RELEASE_GATE_THRESHOLD_POLICY)
    {
        issues.push("release_gate_report_threshold_policy_missing".to_string());
    }
    for (filename, expected_policy) in sleep_v1_release_gate_subgate_integrity_policies() {
        if report
            .provenance
            .get("subgate_report_integrity_policies")
            .and_then(Value::as_object)
            .and_then(|policies| policies.get(filename))
            .and_then(Value::as_str)
            != Some(expected_policy)
        {
            issues.push("release_gate_report_subgate_integrity_policy_missing".to_string());
            break;
        }
    }
    for (filename, expected_policy) in sleep_v1_release_gate_subgate_validation_policies() {
        if report
            .provenance
            .get("subgate_report_validation_policies")
            .and_then(Value::as_object)
            .and_then(|policies| policies.get(filename))
            .and_then(Value::as_str)
            != Some(expected_policy)
        {
            issues.push("release_gate_report_subgate_validation_policy_missing".to_string());
            break;
        }
    }
    let min_hand_reviewed_window_comparisons = report
        .provenance
        .get("min_hand_reviewed_window_comparisons")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let min_benchmark_comparisons = report
        .provenance
        .get("min_benchmark_comparisons")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let min_stage_label_comparisons = report
        .provenance
        .get("min_stage_label_comparisons")
        .and_then(Value::as_u64)
        .map(|value| value as usize);

    if min_hand_reviewed_window_comparisons
        .is_none_or(|minimum| minimum < default_min_hand_reviewed_window_comparisons())
    {
        issues.push("release_gate_report_review_threshold_below_default".to_string());
    }
    if min_stage_label_comparisons
        .is_none_or(|minimum| minimum < default_min_stage_label_comparisons())
    {
        issues.push("release_gate_report_stage_label_threshold_below_default".to_string());
    }
    if min_benchmark_comparisons.is_none_or(|minimum| minimum < default_min_benchmark_comparisons())
    {
        issues.push("release_gate_report_benchmark_threshold_below_default".to_string());
    }
    if report.hand_reviewed_window_comparisons
        < min_hand_reviewed_window_comparisons
            .unwrap_or(default_min_hand_reviewed_window_comparisons())
    {
        issues.push("release_gate_report_hand_reviewed_sample_below_threshold".to_string());
    }
    if report.benchmark_comparison_count
        < min_benchmark_comparisons.unwrap_or(default_min_benchmark_comparisons())
    {
        issues.push("release_gate_report_benchmark_sample_below_threshold".to_string());
    }
    if report.stage_label_comparison_count
        < min_stage_label_comparisons.unwrap_or(default_min_stage_label_comparisons())
    {
        issues.push("release_gate_report_stage_label_sample_below_threshold".to_string());
    }
    if report.next_actions != sleep_v1_release_gate_next_actions(&report.issues) {
        issues.push("release_gate_report_next_actions_mismatch".to_string());
    }
    if report.acceptance_summary != sleep_v1_release_gate_acceptance_summary(&report) {
        issues.push("release_gate_report_acceptance_summary_mismatch".to_string());
    }
    if !report.quality_flags.is_empty() {
        issues.push("release_gate_report_quality_flags_present".to_string());
    }
    if !report.errors.is_empty() {
        issues.push("release_gate_report_errors_present".to_string());
    }
    let expected_report_pass = report.physical_historical_sync_pass
        && report.timestamp_evidence_pass
        && report.sleep_window_label_pass
        && report.sleep_stage_label_pass
        && report.explanation_stability_pass
        && report.benchmark_comparison_pass
        && report.issues.is_empty();
    if report.pass != expected_report_pass {
        issues.push("release_gate_report_pass_state_inconsistent".to_string());
    }
    issues
}

fn expected_sleep_v1_report_generator(filename: &str) -> Option<&'static str> {
    match filename {
        "historical-sync-validation.json" => Some("goose-historical-sync-physical-validator"),
        "sleep-window-validation.json" => Some("goose-sleep-window-label-validator"),
        "sleep-stage-validation.json" => Some("goose-sleep-stage-label-validator"),
        "sleep-v1-stability.json" => Some("goose-sleep-v1-explanation-stability-validator"),
        "sleep-v1-benchmark.json" => Some("goose.algorithm_compare"),
        "sleep-v1-release-gate.json" => Some("goose-sleep-v1-release-gate-validator"),
        _ => None,
    }
}

fn validate_sleep_v1_supporting_file(
    path: &Path,
    filename: &str,
    expected_schema: Option<&str>,
    kind: SleepV1SupportingFileKind,
) -> SleepV1EvidenceSupportingFileReport {
    let mut issues = Vec::new();
    if !path.exists() {
        issues.push(format!("missing_supporting_file:{filename}"));
        return SleepV1EvidenceSupportingFileReport {
            filename: filename.to_string(),
            path: path.display().to_string(),
            byte_size: None,
            sha256: None,
            expected_schema: expected_schema.map(str::to_string),
            schema: None,
            exists: false,
            schema_pass: false,
            contract_pass: false,
            issues,
        };
    }
    issues.extend(sleep_v1_evidence_file_path_issues(
        path,
        filename,
        "supporting",
    ));
    if kind == SleepV1SupportingFileKind::SleepWindowStore {
        let (byte_size, sha256, contract_pass) = match fs::read(path) {
            Ok(bytes) => (
                Some(bytes.len() as u64),
                Some(sha256_hex(&bytes)),
                !bytes.is_empty(),
            ),
            Err(_) => (None, None, false),
        };
        if !contract_pass {
            issues.push(format!("supporting_contract_invalid:{filename}"));
        }
        return SleepV1EvidenceSupportingFileReport {
            filename: filename.to_string(),
            path: path.display().to_string(),
            byte_size,
            sha256,
            expected_schema: None,
            schema: None,
            exists: true,
            schema_pass: true,
            contract_pass,
            issues,
        };
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            issues.push(format!("unreadable_supporting_file:{filename}:{error}"));
            return SleepV1EvidenceSupportingFileReport {
                filename: filename.to_string(),
                path: path.display().to_string(),
                byte_size: None,
                sha256: None,
                expected_schema: expected_schema.map(str::to_string),
                schema: None,
                exists: true,
                schema_pass: false,
                contract_pass: false,
                issues,
            };
        }
    };
    let byte_size = Some(bytes.len() as u64);
    let sha256 = Some(sha256_hex(&bytes));
    let raw = String::from_utf8_lossy(&bytes);
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            issues.push(format!("invalid_supporting_file_json:{filename}:{error}"));
            return SleepV1EvidenceSupportingFileReport {
                filename: filename.to_string(),
                path: path.display().to_string(),
                byte_size,
                sha256,
                expected_schema: expected_schema.map(str::to_string),
                schema: None,
                exists: true,
                schema_pass: false,
                contract_pass: false,
                issues,
            };
        }
    };

    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .map(str::to_string);
    let schema_pass = expected_schema.is_none_or(|expected| schema.as_deref() == Some(expected));
    if !schema_pass {
        issues.push(format!("supporting_schema_mismatch:{filename}"));
    }

    let contract_pass = match kind {
        SleepV1SupportingFileKind::HistoricalSyncTemplate => value.get("input").is_some(),
        SleepV1SupportingFileKind::HistoricalSyncEvidence => {
            serde_json::from_value::<HistoricalSyncPhysicalValidationInput>(value.clone()).is_ok()
        }
        SleepV1SupportingFileKind::SleepWindowValidationInput => {
            serde_json::from_value::<SleepWindowLabelValidationEvidenceInput>(value.clone())
                .is_ok_and(|input| sleep_window_validation_input_references_evidence_store(&input))
        }
        SleepV1SupportingFileKind::SleepWindowStore => fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false),
        SleepV1SupportingFileKind::SleepV1Input => {
            serde_json::from_value::<SleepV1Input>(value.clone()).is_ok()
        }
        SleepV1SupportingFileKind::SleepV1ReleaseGateInput => {
            serde_json::from_value::<SleepV1ReleaseGateInput>(value.clone()).is_ok()
        }
    };
    if !contract_pass {
        issues.push(format!("supporting_contract_invalid:{filename}"));
    }

    issues.sort();
    issues.dedup();
    SleepV1EvidenceSupportingFileReport {
        filename: filename.to_string(),
        path: path.display().to_string(),
        byte_size,
        sha256,
        expected_schema: expected_schema.map(str::to_string),
        schema,
        exists: true,
        schema_pass,
        contract_pass,
        issues,
    }
}

fn sleep_v1_evidence_file_path_issues(path: &Path, filename: &str, prefix: &str) -> Vec<String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            let mut issues = Vec::new();
            if file_type.is_symlink() {
                issues.push(format!("{prefix}_file_symlink:{filename}"));
            } else if !file_type.is_file() {
                issues.push(format!("{prefix}_file_not_regular:{filename}"));
            }
            issues
        }
        Err(error) => vec![format!(
            "{prefix}_file_metadata_unreadable:{filename}:{error}"
        )],
    }
}

fn sleep_window_validation_input_references_evidence_store(
    input: &SleepWindowLabelValidationEvidenceInput,
) -> bool {
    input.database_path == SLEEP_V1_EVIDENCE_SLEEP_WINDOW_STORE
}

fn validate_sleep_v1_evidence_derivations(
    evidence_dir: &Path,
) -> Vec<SleepV1EvidenceDerivationCheck> {
    vec![
        validate_sleep_v1_historical_sync_template_consistency(evidence_dir),
        validate_sleep_v1_historical_sync_derivation(evidence_dir),
        validate_sleep_v1_window_derivation(evidence_dir),
        validate_sleep_v1_stability_derivation(evidence_dir),
        validate_sleep_v1_benchmark_derivation(evidence_dir),
        validate_sleep_v1_release_gate_input_consistency(evidence_dir),
        validate_sleep_v1_release_gate_derivation(evidence_dir),
    ]
}

fn validate_sleep_v1_historical_sync_template_consistency(
    evidence_dir: &Path,
) -> SleepV1EvidenceDerivationCheck {
    let name = "historical_sync_evidence_matches_template";
    let source_files = [
        "historical-sync-template.json",
        "historical-sync-evidence.json",
    ];
    let report_file = "historical-sync-evidence.json";
    let issues = match (
        read_evidence_json::<HistoricalSyncPhysicalEvidenceTemplate>(evidence_dir, source_files[0]),
        read_evidence_json::<HistoricalSyncPhysicalValidationInput>(evidence_dir, source_files[1]),
    ) {
        (Ok(template), Ok(input)) => {
            let mut issues = Vec::new();
            if template.generation != input.generation {
                issues.push("historical_sync_template_generation_mismatch".to_string());
            }
            if template.capture_session_id != input.capture_session_id {
                issues.push("historical_sync_template_capture_session_mismatch".to_string());
            }
            if !input
                .service_uuids
                .iter()
                .any(|uuid| uuid_matches(uuid, &template.expected_service_uuid))
            {
                issues.push("historical_sync_template_service_uuid_missing".to_string());
            }
            issues
        }
        (template, input) => {
            let mut issues = Vec::new();
            push_derivation_result_issue(name, source_files[0], &template, &mut issues);
            push_derivation_result_issue(name, source_files[1], &input, &mut issues);
            issues
        }
    };
    derivation_check(name, &source_files, report_file, issues)
}

fn uuid_matches(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_ascii_hexdigit())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    normalize(left) == normalize(right)
}

fn validate_sleep_v1_historical_sync_derivation(
    evidence_dir: &Path,
) -> SleepV1EvidenceDerivationCheck {
    let name = "historical_sync_validation_matches_evidence";
    let source_file = "historical-sync-evidence.json";
    let report_file = "historical-sync-validation.json";
    let issues = match (
        read_evidence_json::<HistoricalSyncPhysicalValidationInput>(evidence_dir, source_file),
        read_evidence_json::<HistoricalSyncPhysicalValidationReport>(evidence_dir, report_file),
    ) {
        (Ok(input), Ok(report)) => {
            let expected = validate_historical_sync_physical_evidence(&input);
            if expected == report {
                Vec::new()
            } else {
                vec![format!("derived_report_mismatch:{report_file}")]
            }
        }
        (input, report) => derivation_read_issues(name, &[source_file], report_file, input, report),
    };
    derivation_check(name, &[source_file], report_file, issues)
}

fn validate_sleep_v1_window_derivation(evidence_dir: &Path) -> SleepV1EvidenceDerivationCheck {
    let name = "sleep_window_validation_matches_store";
    let source_files = [
        "sleep-window-validation-input.json",
        "sleep-window-store.sqlite",
    ];
    let report_file = "sleep-window-validation.json";
    let issues = match (
        read_evidence_json::<SleepWindowLabelValidationEvidenceInput>(
            evidence_dir,
            source_files[0],
        ),
        read_evidence_json::<SleepWindowLabelValidationReport>(evidence_dir, report_file),
    ) {
        (Ok(input), Ok(report)) => {
            let db_path = evidence_dir.join(SLEEP_V1_EVIDENCE_SLEEP_WINDOW_STORE);
            match GooseStore::open_read_only(&db_path).and_then(|store| {
                run_sleep_window_label_validation_for_store(
                    &store,
                    &input.database_path,
                    &input.start,
                    &input.end,
                    input.options,
                )
            }) {
                Ok(expected) if expected == report => Vec::new(),
                Ok(_) => vec![format!("derived_report_mismatch:{report_file}")],
                Err(error) => vec![format!(
                    "derived_report_recompute_failed:{report_file}:{error}"
                )],
            }
        }
        (input, report) => derivation_read_issues(name, &source_files, report_file, input, report),
    };
    derivation_check(name, &source_files, report_file, issues)
}

fn validate_sleep_v1_stability_derivation(evidence_dir: &Path) -> SleepV1EvidenceDerivationCheck {
    let name = "sleep_v1_stability_matches_input";
    let source_file = "sleep-v1-input.json";
    let report_file = "sleep-v1-stability.json";
    let issues = match (
        read_evidence_json::<SleepV1Input>(evidence_dir, source_file),
        read_evidence_json::<SleepV1ExplanationStabilityReport>(evidence_dir, report_file),
    ) {
        (Ok(input), Ok(report)) => {
            let expected = validate_sleep_v1_explanation_and_stability(&input, Default::default());
            if expected == report {
                Vec::new()
            } else {
                vec![format!("derived_report_mismatch:{report_file}")]
            }
        }
        (input, report) => derivation_read_issues(name, &[source_file], report_file, input, report),
    };
    derivation_check(name, &[source_file], report_file, issues)
}

fn validate_sleep_v1_benchmark_derivation(evidence_dir: &Path) -> SleepV1EvidenceDerivationCheck {
    let name = "sleep_v1_benchmark_matches_input";
    let source_file = "sleep-v1-input.json";
    let report_file = "sleep-v1-benchmark.json";
    let issues = match (
        read_evidence_json::<SleepV1Input>(evidence_dir, source_file),
        read_evidence_json::<AlgorithmComparisonReport>(evidence_dir, report_file),
    ) {
        (Ok(input), Ok(report)) => match compare_sleep_v1_goose_to_reference(&input) {
            Ok(expected)
                if sleep_v1_benchmark_report_equivalent_for_evidence_folder(
                    &expected,
                    &report,
                    evidence_dir,
                    source_file,
                ) =>
            {
                Vec::new()
            }
            Ok(_) => vec![format!("derived_report_mismatch:{report_file}")],
            Err(error) => vec![format!(
                "derived_report_recompute_failed:{report_file}:{error}"
            )],
        },
        (input, report) => derivation_read_issues(name, &[source_file], report_file, input, report),
    };
    derivation_check(name, &[source_file], report_file, issues)
}

fn sleep_v1_benchmark_report_equivalent_for_evidence_folder(
    expected: &AlgorithmComparisonReport,
    observed: &AlgorithmComparisonReport,
    evidence_dir: &Path,
    source_file: &str,
) -> bool {
    if !sleep_v1_benchmark_data_coverage_matches_evidence(
        expected,
        observed,
        evidence_dir,
        source_file,
    ) {
        return false;
    }

    let mut normalized_observed = observed.clone();
    normalized_observed.data_coverage = expected.data_coverage.clone();
    algorithm_comparison_reports_equivalent(expected, &normalized_observed)
}

fn sleep_v1_benchmark_data_coverage_matches_evidence(
    expected: &AlgorithmComparisonReport,
    observed: &AlgorithmComparisonReport,
    evidence_dir: &Path,
    source_file: &str,
) -> bool {
    let Some(expected_coverage) = expected.data_coverage.as_ref() else {
        return observed.data_coverage.is_none();
    };
    let Some(observed_coverage) = observed.data_coverage.as_ref() else {
        return false;
    };
    let (Some(expected_object), Some(observed_object)) =
        (expected_coverage.as_object(), observed_coverage.as_object())
    else {
        return json_values_equivalent(expected_coverage, observed_coverage);
    };

    if !expected_object.iter().all(|(key, expected_value)| {
        observed_object
            .get(key)
            .is_some_and(|observed_value| json_values_equivalent(expected_value, observed_value))
    }) {
        return false;
    }

    let allowed_extra_fields = BTreeSet::from([
        "input_path",
        "input_bytes",
        "input_ids_count",
        "start_time",
        "end_time",
        "output_present",
        "quality_flag_count",
        "error_count",
    ]);
    if observed_object.keys().any(|key| {
        !expected_object.contains_key(key) && !allowed_extra_fields.contains(key.as_str())
    }) {
        return false;
    }

    sleep_v1_benchmark_cli_data_coverage_valid(
        observed_coverage,
        observed,
        evidence_dir,
        source_file,
    )
}

fn sleep_v1_benchmark_cli_data_coverage_valid(
    coverage: &Value,
    report: &AlgorithmComparisonReport,
    evidence_dir: &Path,
    source_file: &str,
) -> bool {
    let Some(object) = coverage.as_object() else {
        return false;
    };
    let cli_fields = [
        "input_path",
        "input_bytes",
        "input_ids_count",
        "start_time",
        "end_time",
        "output_present",
        "quality_flag_count",
        "error_count",
    ];
    if cli_fields.iter().all(|field| !object.contains_key(*field)) {
        return true;
    }
    if cli_fields.iter().any(|field| !object.contains_key(*field)) {
        return false;
    }

    let input_path = object
        .get("input_path")
        .and_then(Value::as_str)
        .filter(|path| {
            Path::new(path).file_name().and_then(|name| name.to_str()) == Some(source_file)
        });
    let input_bytes = object.get("input_bytes").and_then(Value::as_u64);
    let input_ids_count = object.get("input_ids_count").and_then(Value::as_u64);
    let start_time = object.get("start_time").and_then(Value::as_str);
    let end_time = object.get("end_time").and_then(Value::as_str);
    let output_present = object.get("output_present").and_then(Value::as_bool);
    let quality_flag_count = object.get("quality_flag_count").and_then(Value::as_u64);
    let error_count = object.get("error_count").and_then(Value::as_u64);

    let input_raw = match fs::read_to_string(evidence_dir.join(source_file)) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let input_value = serde_json::from_str::<Value>(&input_raw).unwrap_or(Value::Null);
    let expected_input_ids_count = input_value
        .get("input_ids")
        .and_then(Value::as_array)
        .map(|ids| ids.len() as u64)
        .unwrap_or(0);

    input_path.is_some()
        && input_bytes == Some(input_raw.len() as u64)
        && input_ids_count == Some(expected_input_ids_count)
        && start_time == Some(report.start_time.as_str())
        && end_time == Some(report.end_time.as_str())
        && output_present
            == Some(report.goose_output.is_some() && report.reference_output.is_some())
        && quality_flag_count
            == Some(
                (report.goose_quality_flags.len() + report.reference_quality_flags.len()) as u64,
            )
        && error_count == Some(report.errors.len() as u64)
}

fn validate_sleep_v1_release_gate_derivation(
    evidence_dir: &Path,
) -> SleepV1EvidenceDerivationCheck {
    let name = "sleep_v1_release_gate_matches_reports";
    let source_files = ["sleep-v1-release-gate-input.json"];
    let report_file = "sleep-v1-release-gate.json";
    let issues = match (
        read_evidence_json::<SleepV1ReleaseGateInput>(evidence_dir, source_files[0]),
        read_evidence_json::<SleepV1ReleaseGateReport>(evidence_dir, report_file),
    ) {
        (Ok(input), Ok(report)) => {
            let expected = validate_sleep_v1_release_gates(&input);
            if expected == report {
                Vec::new()
            } else {
                vec![format!("derived_report_mismatch:{report_file}")]
            }
        }
        (input, report) => derivation_read_issues(name, &source_files, report_file, input, report),
    };
    derivation_check(name, &source_files, report_file, issues)
}

fn validate_sleep_v1_release_gate_input_consistency(
    evidence_dir: &Path,
) -> SleepV1EvidenceDerivationCheck {
    let name = "sleep_v1_release_gate_input_matches_report_files";
    let source_files = [
        "sleep-v1-release-gate-input.json",
        "historical-sync-validation.json",
        "sleep-window-validation.json",
        "sleep-stage-validation.json",
        "sleep-v1-stability.json",
        "sleep-v1-benchmark.json",
    ];
    let report_file = "sleep-v1-release-gate-input.json";
    let issues = match (
        read_evidence_json::<SleepV1ReleaseGateInput>(evidence_dir, source_files[0]),
        read_evidence_json::<HistoricalSyncPhysicalValidationReport>(evidence_dir, source_files[1]),
        read_evidence_json::<SleepWindowLabelValidationReport>(evidence_dir, source_files[2]),
        read_evidence_json::<SleepStageLabelValidationReport>(evidence_dir, source_files[3]),
        read_evidence_json::<SleepV1ExplanationStabilityReport>(evidence_dir, source_files[4]),
        read_evidence_json::<AlgorithmComparisonReport>(evidence_dir, source_files[5]),
    ) {
        (Ok(input), Ok(physical), Ok(window), Ok(stage), Ok(stability), Ok(benchmark)) => {
            let mut issues = Vec::new();
            if input.physical_historical_sync.as_ref() != Some(&physical) {
                issues.push(
                    "release_gate_input_report_mismatch:historical-sync-validation.json"
                        .to_string(),
                );
            }
            if input.sleep_window_label_validation.as_ref() != Some(&window) {
                issues.push(
                    "release_gate_input_report_mismatch:sleep-window-validation.json".to_string(),
                );
            }
            if input.sleep_stage_label_validation.as_ref() != Some(&stage) {
                issues.push(
                    "release_gate_input_report_mismatch:sleep-stage-validation.json".to_string(),
                );
            }
            if input.explanation_stability.as_ref() != Some(&stability) {
                issues
                    .push("release_gate_input_report_mismatch:sleep-v1-stability.json".to_string());
            }
            if input.benchmark_comparisons.len() != 1
                || !algorithm_comparison_reports_equivalent(
                    &input.benchmark_comparisons[0],
                    &benchmark,
                )
            {
                issues
                    .push("release_gate_input_report_mismatch:sleep-v1-benchmark.json".to_string());
            }
            if input.min_hand_reviewed_window_comparisons
                < default_min_hand_reviewed_window_comparisons()
            {
                issues
                    .push("release_gate_hand_reviewed_window_threshold_below_default".to_string());
            }
            if input.min_stage_label_comparisons < default_min_stage_label_comparisons() {
                issues.push("release_gate_stage_label_threshold_below_default".to_string());
            }
            if input.min_benchmark_comparisons < default_min_benchmark_comparisons() {
                issues.push("release_gate_benchmark_threshold_below_default".to_string());
            }
            issues
        }
        (input, physical, window, stage, stability, benchmark) => {
            let mut issues = Vec::new();
            push_derivation_result_issue(name, source_files[0], &input, &mut issues);
            push_derivation_result_issue(name, source_files[1], &physical, &mut issues);
            push_derivation_result_issue(name, source_files[2], &window, &mut issues);
            push_derivation_result_issue(name, source_files[3], &stage, &mut issues);
            push_derivation_result_issue(name, source_files[4], &stability, &mut issues);
            push_derivation_result_issue(name, source_files[5], &benchmark, &mut issues);
            issues
        }
    };
    derivation_check(name, &source_files, report_file, issues)
}

fn derivation_check(
    name: &str,
    source_files: &[&str],
    report_file: &str,
    mut issues: Vec<String>,
) -> SleepV1EvidenceDerivationCheck {
    issues.sort();
    issues.dedup();
    SleepV1EvidenceDerivationCheck {
        name: name.to_string(),
        source_files: source_files
            .iter()
            .map(|file| (*file).to_string())
            .collect(),
        report_file: report_file.to_string(),
        pass: issues.is_empty(),
        issues,
    }
}

fn algorithm_comparison_reports_equivalent(
    expected: &AlgorithmComparisonReport,
    observed: &AlgorithmComparisonReport,
) -> bool {
    expected.schema == observed.schema
        && expected.generated_by == observed.generated_by
        && expected.family == observed.family
        && json_options_equivalent(&expected.data_coverage, &observed.data_coverage)
        && expected.reference_contract_valid == observed.reference_contract_valid
        && expected.goose_output_ready == observed.goose_output_ready
        && expected.reference_output_ready == observed.reference_output_ready
        && expected.shared_fields_ready == observed.shared_fields_ready
        && expected.pass == observed.pass
        && expected.goose_algorithm_id == observed.goose_algorithm_id
        && expected.goose_algorithm_version == observed.goose_algorithm_version
        && expected.reference_algorithm_id == observed.reference_algorithm_id
        && expected.reference_algorithm_version == observed.reference_algorithm_version
        && expected.start_time == observed.start_time
        && expected.end_time == observed.end_time
        && expected.comparable_fields == observed.comparable_fields
        && expected.non_comparable_fields == observed.non_comparable_fields
        && expected.goose_quality_flags == observed.goose_quality_flags
        && expected.reference_quality_flags == observed.reference_quality_flags
        && expected.quality_flags == observed.quality_flags
        && expected.errors == observed.errors
        && json_options_equivalent(&expected.goose_output, &observed.goose_output)
        && json_options_equivalent(&expected.reference_output, &observed.reference_output)
        && json_values_equivalent(&expected.provenance, &observed.provenance)
        && expected.next_actions == observed.next_actions
        && expected.deltas.len() == observed.deltas.len()
        && expected
            .deltas
            .iter()
            .zip(&observed.deltas)
            .all(|(left, right)| {
                left.field == right.field
                    && left.goose_path == right.goose_path
                    && left.reference_path == right.reference_path
                    && left.unit == right.unit
                    && approx_equal(left.goose_value, right.goose_value)
                    && approx_equal(left.reference_value, right.reference_value)
                    && approx_equal(left.absolute_delta, right.absolute_delta)
                    && optional_approx_equal(
                        left.relative_delta_fraction,
                        right.relative_delta_fraction,
                    )
            })
}

fn json_options_equivalent(left: &Option<Value>, right: &Option<Value>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => json_values_equivalent(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn json_values_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| approx_equal(left, right)),
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_values_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left_value)| {
                    right
                        .get(key)
                        .is_some_and(|right_value| json_values_equivalent(left_value, right_value))
                })
        }
        _ => false,
    }
}

fn approx_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 0.000_001
}

fn optional_approx_equal(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => approx_equal(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn finite_non_negative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_evidence_json<T: serde::de::DeserializeOwned>(
    evidence_dir: &Path,
    filename: &str,
) -> Result<T, String> {
    let path = evidence_dir.join(filename);
    let raw = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_json::from_str::<T>(&raw).map_err(|error| error.to_string())
}

fn derivation_read_issues<T, U>(
    name: &str,
    source_files: &[&str],
    report_file: &str,
    source_result: Result<T, String>,
    report_result: Result<U, String>,
) -> Vec<String> {
    let mut issues = Vec::new();
    if let Err(error) = source_result {
        issues.push(format!(
            "derived_report_source_unreadable:{name}:{}:{error}",
            source_files[0]
        ));
    }
    if let Err(error) = report_result {
        issues.push(format!(
            "derived_report_output_unreadable:{name}:{report_file}:{error}"
        ));
    }
    issues
}

fn push_derivation_result_issue<T>(
    name: &str,
    filename: &str,
    result: &Result<T, String>,
    issues: &mut Vec<String>,
) {
    if let Err(error) = result {
        issues.push(format!(
            "derived_report_source_unreadable:{name}:{filename}:{error}"
        ));
    }
}

fn sleep_v1_evidence_folder_next_actions(
    issues: &[String],
) -> Vec<SleepV1EvidenceFolderNextAction> {
    issues
        .iter()
        .map(|issue| SleepV1EvidenceFolderNextAction {
            scope: sleep_v1_evidence_folder_issue_scope(issue).to_string(),
            reason: issue.clone(),
            action: sleep_v1_evidence_folder_issue_action(issue),
        })
        .collect()
}

fn sleep_v1_evidence_folder_issue_scope(issue: &str) -> &'static str {
    if let Some(filename) = issue.strip_prefix("missing_required_file:") {
        sleep_v1_required_report_issue_scope(filename)
    } else if issue.starts_with("missing_supporting_file:") {
        "sleep_v1.evidence_folder.files"
    } else if issue.starts_with("unexpected_evidence_file:") {
        "sleep_v1.evidence_folder.files"
    } else if issue.starts_with("schema_mismatch:")
        || issue.starts_with("generated_by_mismatch:")
        || issue.starts_with("invalid_required_file_json:")
        || issue.starts_with("unreadable_required_file:")
        || issue.starts_with("supporting_schema_mismatch:")
        || issue.starts_with("invalid_supporting_file_json:")
        || issue.starts_with("unreadable_supporting_file:")
    {
        "sleep_v1.evidence_folder.schema"
    } else if issue.starts_with("required_file_symlink:")
        || issue.starts_with("required_file_not_regular:")
        || issue.starts_with("required_file_metadata_unreadable:")
        || issue.starts_with("supporting_file_symlink:")
        || issue.starts_with("supporting_file_not_regular:")
        || issue.starts_with("supporting_file_metadata_unreadable:")
    {
        "sleep_v1.evidence_folder.files"
    } else if issue.starts_with("supporting_contract_invalid:") {
        "sleep_v1.evidence_folder.inputs"
    } else if issue.starts_with("derived_report_mismatch:")
        || issue.starts_with("derived_report_recompute_failed:")
        || issue.starts_with("derived_report_source_unreadable:")
        || issue.starts_with("derived_report_output_unreadable:")
        || issue.starts_with("release_gate_input_report_mismatch:")
        || issue.starts_with("historical_sync_template_")
    {
        "sleep_v1.evidence_folder.derivations"
    } else if issue == "release_gate_hand_reviewed_window_threshold_below_default"
        || issue == "release_gate_report_review_threshold_below_default"
        || issue == "release_gate_report_hand_reviewed_sample_below_threshold"
    {
        "sleep_window.labels"
    } else if issue == "sleep_stage_label_report_unparseable"
        || issue == "sleep_stage_label_report_integrity_failed"
        || issue == "release_gate_stage_label_threshold_below_default"
        || issue == "release_gate_report_stage_label_sample_below_threshold"
        || issue == "release_gate_report_stage_label_threshold_below_default"
    {
        "sleep_stage.labels"
    } else if issue == "release_gate_benchmark_threshold_below_default"
        || issue == "release_gate_report_benchmark_threshold_below_default"
        || issue == "release_gate_report_benchmark_sample_below_threshold"
    {
        "sleep_v1.benchmark"
    } else if issue == "evidence_manifest_sha256_mismatch"
        || issue == "expected_manifest_sha256_invalid"
    {
        "sleep_v1.evidence_folder.manifest"
    } else if issue.starts_with("release_gate_subgate_not_passing:")
        || issue == "release_gate_report_unparseable"
        || issue == "release_gate_report_promotion_policy_missing"
        || issue == "release_gate_report_integrity_policy_missing"
        || issue == "release_gate_report_threshold_policy_missing"
        || issue == "release_gate_report_subgate_integrity_policy_missing"
        || issue == "release_gate_report_subgate_validation_policy_missing"
        || issue == "required_report_validation_policy_missing"
        || issue == "release_gate_report_next_actions_mismatch"
        || issue == "release_gate_report_acceptance_summary_mismatch"
        || issue == "release_gate_report_quality_flags_present"
        || issue == "release_gate_report_errors_present"
        || issue == "release_gate_report_pass_state_inconsistent"
    {
        "sleep_v1.release_gate"
    } else if issue == "physical_historical_sync_report_unparseable"
        || issue == "physical_historical_sync_report_integrity_failed"
    {
        "historical_sync.physical"
    } else if issue == "sleep_window_label_report_unparseable"
        || issue == "sleep_window_label_report_integrity_failed"
    {
        "sleep_window.labels"
    } else if issue == "stability_report_unparseable"
        || issue == "stability_report_integrity_failed"
        || issue == "stability_report_threshold_below_default"
    {
        "sleep_v1.stability"
    } else if issue == "benchmark_report_unparseable"
        || issue == "benchmark_report_integrity_failed"
    {
        "sleep_v1.benchmark"
    } else if let Some(filename) = issue.strip_prefix("required_file_not_passing:") {
        sleep_v1_required_report_issue_scope(filename)
    } else if issue.starts_with("missing_pass_field:")
        || issue.starts_with("passing_required_file_has_issues:")
        || issue.starts_with("passing_required_file_missing_issues:")
        || issue.starts_with("passing_required_file_has_next_actions:")
        || issue.starts_with("passing_required_file_missing_next_actions:")
        || issue.starts_with("passing_required_file_has_errors:")
        || issue.starts_with("passing_required_file_missing_errors:")
        || issue.starts_with("passing_required_file_has_quality_flags:")
        || issue.starts_with("passing_required_file_missing_quality_flags:")
    {
        "sleep_v1.evidence_folder.reports"
    } else {
        "sleep_v1.evidence_folder"
    }
}

fn sleep_v1_required_report_issue_scope(filename: &str) -> &'static str {
    match filename {
        "historical-sync-validation.json" => "historical_sync.physical",
        "sleep-window-validation.json" => "sleep_window.labels",
        "sleep-stage-validation.json" => "sleep_stage.labels",
        "sleep-v1-stability.json" => "sleep_v1.stability",
        "sleep-v1-benchmark.json" => "sleep_v1.benchmark",
        "sleep-v1-release-gate.json" => "sleep_v1.release_gate",
        _ => "sleep_v1.evidence_folder.files",
    }
}

fn sleep_v1_evidence_folder_issue_action(issue: &str) -> String {
    if let Some(filename) = issue.strip_prefix("missing_required_file:") {
        format!("Generate {filename} in the evidence folder before running final promotion.")
    } else if let Some(filename) = issue.strip_prefix("missing_supporting_file:") {
        format!("Add {filename} to the evidence folder so the validation run is reproducible.")
    } else if let Some(filename) = issue.strip_prefix("unexpected_evidence_file:") {
        format!(
            "Remove or archive {filename} outside the Sleep V1 evidence folder before promotion review."
        )
    } else if issue.starts_with("schema_mismatch:") {
        "Regenerate the report with the current Goose validator so the expected schema matches."
            .to_string()
    } else if issue.starts_with("generated_by_mismatch:") {
        "Regenerate the report with the expected Goose validator instead of editing or substituting the artifact."
            .to_string()
    } else if issue.starts_with("supporting_schema_mismatch:") {
        "Regenerate or replace the supporting JSON with the expected Goose schema.".to_string()
    } else if issue.starts_with("invalid_required_file_json:")
        || issue.starts_with("unreadable_required_file:")
        || issue.starts_with("invalid_supporting_file_json:")
        || issue.starts_with("unreadable_supporting_file:")
    {
        "Fix or regenerate the report JSON before relying on the evidence folder.".to_string()
    } else if issue.starts_with("required_file_symlink:")
        || issue.starts_with("supporting_file_symlink:")
    {
        "Replace symlinked evidence with a real file inside the Sleep V1 evidence folder."
            .to_string()
    } else if issue.starts_with("required_file_not_regular:")
        || issue.starts_with("supporting_file_not_regular:")
        || issue.starts_with("required_file_metadata_unreadable:")
        || issue.starts_with("supporting_file_metadata_unreadable:")
    {
        "Replace the evidence artifact with a readable regular file inside the Sleep V1 evidence folder."
            .to_string()
    } else if issue.starts_with("supporting_contract_invalid:") {
        "Replace the supporting input with the exact JSON used to generate the validation reports."
            .to_string()
    } else if issue.starts_with("derived_report_mismatch:") {
        "Regenerate the mismatched report from the source JSON in this evidence folder.".to_string()
    } else if issue.starts_with("derived_report_recompute_failed:")
        || issue.starts_with("derived_report_source_unreadable:")
        || issue.starts_with("derived_report_output_unreadable:")
    {
        "Fix the source or report JSON, then rerun the evidence-folder audit.".to_string()
    } else if issue.starts_with("release_gate_input_report_mismatch:") {
        "Regenerate the release-gate input manifest from the report files in this evidence folder."
            .to_string()
    } else if issue.starts_with("historical_sync_template_") {
        "Regenerate the historical-sync template and filled evidence from the same physical capture session before rerunning the folder audit.".to_string()
    } else if issue == "release_gate_hand_reviewed_window_threshold_below_default"
        || issue == "release_gate_report_review_threshold_below_default"
        || issue == "release_gate_report_hand_reviewed_sample_below_threshold"
    {
        format!(
            "Collect at least {} distinct hand-reviewed sleep windows before using this evidence folder for Sleep V1 promotion.",
            default_min_hand_reviewed_window_comparisons()
        )
    } else if issue == "release_gate_benchmark_threshold_below_default"
        || issue == "release_gate_report_benchmark_threshold_below_default"
        || issue == "release_gate_report_benchmark_sample_below_threshold"
    {
        format!(
            "Keep at least {} passing Sleep V1 benchmark comparison before using this evidence folder for promotion.",
            default_min_benchmark_comparisons()
        )
    } else if issue == "evidence_manifest_sha256_mismatch" {
        "Use the evidence folder that matches the pinned manifest SHA-256, or update the pinned hash after review.".to_string()
    } else if issue == "expected_manifest_sha256_invalid" {
        "Provide a 64-character hexadecimal SHA-256 value for the pinned evidence manifest."
            .to_string()
    } else if issue.starts_with("release_gate_subgate_not_passing:") {
        "Resolve the failing release-gate subcheck and rerun the final Sleep V1 gate.".to_string()
    } else if issue == "release_gate_report_unparseable" {
        "Regenerate the release-gate report with the current Goose validator.".to_string()
    } else if issue == "release_gate_report_promotion_policy_missing" {
        "Regenerate the release-gate report with the current promotion policy provenance instead of editing the report by hand.".to_string()
    } else if issue == "release_gate_report_integrity_policy_missing" {
        "Regenerate the release-gate report with the current subgate-integrity provenance instead of editing the report by hand.".to_string()
    } else if issue == "release_gate_report_threshold_policy_missing" {
        "Regenerate the release-gate report with the current primary-threshold policy provenance instead of editing the report by hand.".to_string()
    } else if issue == "release_gate_report_subgate_integrity_policy_missing" {
        "Regenerate the release-gate report with the current per-subgate evidence integrity policies instead of editing the report by hand.".to_string()
    } else if issue == "release_gate_report_subgate_validation_policy_missing" {
        "Regenerate the release-gate report with the current per-subgate validation policies instead of editing the report by hand.".to_string()
    } else if issue == "required_report_validation_policy_missing" {
        "Regenerate the evidence-folder audit with the current per-report validation policy provenance instead of editing the report by hand.".to_string()
    } else if issue == "release_gate_report_next_actions_mismatch" {
        "Regenerate the release-gate report so its next actions match the reported issues."
            .to_string()
    } else if issue == "release_gate_report_acceptance_summary_mismatch" {
        "Regenerate the release-gate report so its acceptance summary matches the subgates, thresholds, proof arrays, and sample counts.".to_string()
    } else if issue == "release_gate_report_quality_flags_present" {
        "Regenerate the release-gate report after resolving quality flags; passing promotion evidence must carry an empty quality_flags list.".to_string()
    } else if issue == "release_gate_report_errors_present" {
        "Regenerate the release-gate report after resolving errors; passing promotion evidence must carry an empty errors list.".to_string()
    } else if issue == "release_gate_report_pass_state_inconsistent" {
        "Regenerate the release-gate report instead of editing pass status or subgate fields."
            .to_string()
    } else if issue == "physical_historical_sync_report_unparseable" {
        "Regenerate historical-sync-validation.json with the current Goose physical-sync validator."
            .to_string()
    } else if issue == "physical_historical_sync_report_integrity_failed" {
        "Regenerate historical-sync-validation.json from the current physical capture evidence, including all required physical-flow subgates, proof counts, and acceptance summary.".to_string()
    } else if issue == "sleep_window_label_report_unparseable" {
        "Regenerate sleep-window-validation.json with the current Goose sleep-window label validator.".to_string()
    } else if issue == "sleep_window_label_report_integrity_failed" {
        "Regenerate sleep-window-validation.json from the current sleep-window store snapshot, including the current label-report integrity policy.".to_string()
    } else if issue == "sleep_stage_label_report_unparseable" {
        "Regenerate sleep-stage-validation.json with the current Goose sleep-stage label validator."
            .to_string()
    } else if issue == "sleep_stage_label_report_integrity_failed" {
        "Regenerate sleep-stage-validation.json from the current sleep-window store snapshot and Sleep V1 input, including the current stage-label integrity policy.".to_string()
    } else if issue == "release_gate_stage_label_threshold_below_default"
        || issue == "release_gate_report_stage_label_threshold_below_default"
        || issue == "release_gate_report_stage_label_sample_below_threshold"
    {
        format!(
            "Collect at least {} passing user-owned sleep-stage comparison before using this evidence folder for Sleep V1 promotion.",
            default_min_stage_label_comparisons()
        )
    } else if issue == "stability_report_unparseable" {
        "Regenerate sleep-v1-stability.json with the current Goose stability validator.".to_string()
    } else if issue == "stability_report_integrity_failed" {
        "Regenerate sleep-v1-stability.json from the current Sleep V1 input and component contract."
            .to_string()
    } else if issue == "stability_report_threshold_below_default" {
        "Regenerate sleep-v1-stability.json with release-default stability thresholds before promotion."
            .to_string()
    } else if issue == "benchmark_report_unparseable" {
        "Regenerate sleep-v1-benchmark.json with the current Goose benchmark comparator."
            .to_string()
    } else if issue == "benchmark_report_integrity_failed" {
        "Regenerate sleep-v1-benchmark.json from the current Sleep V1 input and benchmark contract."
            .to_string()
    } else if let Some(filename) = issue.strip_prefix("required_file_not_passing:") {
        match filename {
            "historical-sync-validation.json" => {
                "Regenerate historical-sync-validation.json from a passing physical WHOOP sync evidence bundle before promotion.".to_string()
            }
            "sleep-window-validation.json" => {
                "Regenerate sleep-window-validation.json from passing hand-reviewed sleep-window labels before promotion.".to_string()
            }
            "sleep-stage-validation.json" => {
                "Regenerate sleep-stage-validation.json from passing user-owned sleep-stage labels before promotion.".to_string()
            }
            "sleep-v1-stability.json" => {
                "Regenerate sleep-v1-stability.json from a passing Sleep V1 explanation/stability validation before promotion.".to_string()
            }
            "sleep-v1-benchmark.json" => {
                "Regenerate sleep-v1-benchmark.json from a passing Sleep V1 reference benchmark before promotion.".to_string()
            }
            "sleep-v1-release-gate.json" => {
                "Regenerate sleep-v1-release-gate.json so the final promotion pass state matches all release evidence.".to_string()
            }
            _ => {
                "Use the failing report's issues and next_actions to collect or fix the missing evidence.".to_string()
            }
        }
    } else if issue.starts_with("missing_pass_field:") {
        "Regenerate the report with a Goose validator that emits an explicit pass field."
            .to_string()
    } else if issue.starts_with("passing_required_file_has_issues:") {
        "Regenerate the report instead of editing pass status; passing Sleep V1 reports must carry an empty issues list.".to_string()
    } else if issue.starts_with("passing_required_file_missing_issues:") {
        "Regenerate the report with the current Goose validator; passing Sleep V1 reports must carry an explicit empty issues list.".to_string()
    } else if issue.starts_with("passing_required_file_has_next_actions:") {
        "Regenerate the report instead of editing next actions; passing Sleep V1 reports must carry an empty next_actions list.".to_string()
    } else if issue.starts_with("passing_required_file_missing_next_actions:") {
        "Regenerate the report with the current Goose validator; passing Sleep V1 reports must carry an explicit empty next_actions list.".to_string()
    } else if issue.starts_with("passing_required_file_has_errors:") {
        "Regenerate the report after resolving errors; passing Sleep V1 reports must carry empty error lists.".to_string()
    } else if issue.starts_with("passing_required_file_missing_errors:") {
        "Regenerate the report with the current Goose validator; passing Sleep V1 reports must carry an explicit empty errors list.".to_string()
    } else if issue.starts_with("passing_required_file_has_quality_flags:") {
        "Regenerate the report after resolving quality flags; passing Sleep V1 reports must carry empty quality-flag lists.".to_string()
    } else if issue.starts_with("passing_required_file_missing_quality_flags:") {
        let field = issue.rsplit(':').next().unwrap_or("quality_flags");
        format!(
            "Regenerate the report with the current Goose validator; passing Sleep V1 reports must carry an explicit empty {field} list."
        )
    } else {
        "Inspect the Sleep V1 evidence folder before promotion.".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepWindowLabelValidationOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_trusted_evidence: bool,
    pub sleep_need_minutes: f64,
    pub low_motion_threshold_0_to_1: f64,
    pub disturbance_motion_threshold_0_to_1: f64,
    pub target_midpoint_minutes_since_midnight: f64,
    pub start_tolerance_minutes: f64,
    pub end_tolerance_minutes: f64,
    pub duration_tolerance_minutes: f64,
    pub min_label_confidence: f64,
}

impl Default for SleepWindowLabelValidationOptions {
    fn default() -> Self {
        let feature_options = SleepFeatureScoreOptions::default();
        Self {
            min_owned_captures_per_summary: feature_options.min_owned_captures_per_summary,
            require_trusted_evidence: feature_options.require_trusted_evidence,
            sleep_need_minutes: feature_options.sleep_need_minutes,
            low_motion_threshold_0_to_1: feature_options.low_motion_threshold_0_to_1,
            disturbance_motion_threshold_0_to_1: feature_options
                .disturbance_motion_threshold_0_to_1,
            target_midpoint_minutes_since_midnight: feature_options
                .target_midpoint_minutes_since_midnight,
            start_tolerance_minutes: 20.0,
            end_tolerance_minutes: 20.0,
            duration_tolerance_minutes: 30.0,
            min_label_confidence: 0.70,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepWindowLabelValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub start_time: String,
    pub end_time: String,
    pub label_count: usize,
    pub compared_label_count: usize,
    pub passing_label_count: usize,
    #[serde(default)]
    pub distinct_compared_sleep_window_count: usize,
    #[serde(default)]
    pub distinct_passing_sleep_window_count: usize,
    pub sleep_window_available: bool,
    #[serde(default)]
    pub acceptance_summary: SleepWindowLabelAcceptanceSummary,
    pub sleep_feature_report: SleepFeatureScoreReport,
    pub comparisons: Vec<SleepWindowLabelComparison>,
    pub issues: Vec<String>,
    pub quality_flags: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<SleepWindowLabelValidationNextAction>,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SleepWindowLabelAcceptanceSummary {
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub hand_reviewed_sample_ready: bool,
    #[serde(default)]
    pub label_count: usize,
    #[serde(default)]
    pub compared_label_count: usize,
    #[serde(default)]
    pub passing_label_count: usize,
    #[serde(default)]
    pub distinct_compared_sleep_window_count: usize,
    #[serde(default)]
    pub distinct_passing_sleep_window_count: usize,
    #[serde(default)]
    pub required_release_distinct_passing_sleep_windows: usize,
    #[serde(default)]
    pub accepted_sleep_ids: Vec<String>,
    #[serde(default)]
    pub start_tolerance_minutes: f64,
    #[serde(default)]
    pub end_tolerance_minutes: f64,
    #[serde(default)]
    pub duration_tolerance_minutes: f64,
    #[serde(default)]
    pub min_label_confidence: f64,
    #[serde(default)]
    pub min_observed_label_confidence: f64,
    #[serde(default)]
    pub max_start_delta_minutes: f64,
    #[serde(default)]
    pub max_end_delta_minutes: f64,
    #[serde(default)]
    pub max_duration_delta_minutes: f64,
    pub issue_count: usize,
    pub quality_flag_count: usize,
    pub error_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepWindowLabelComparison {
    pub label_id: String,
    pub sleep_id: Option<String>,
    pub source: String,
    pub provenance_source: String,
    pub label_provenance_policy: String,
    pub confidence: Option<f64>,
    #[serde(default)]
    pub expected_start_time: String,
    #[serde(default)]
    pub expected_end_time: String,
    pub expected_start_time_unix_ms: i64,
    pub expected_end_time_unix_ms: i64,
    pub observed_start_time: String,
    pub observed_end_time: String,
    pub observed_start_time_unix_ms: i64,
    pub observed_end_time_unix_ms: i64,
    pub start_delta_minutes: f64,
    pub end_delta_minutes: f64,
    pub duration_delta_minutes: f64,
    pub pass: bool,
    pub quality_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepWindowLabelValidationNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepStageLabelValidationOptions {
    pub min_label_confidence: f64,
    pub min_overlap_fraction: f64,
}

impl Default for SleepStageLabelValidationOptions {
    fn default() -> Self {
        Self {
            min_label_confidence: 0.70,
            min_overlap_fraction: 0.50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepStageLabelValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub sleep_id: String,
    pub start_time: String,
    pub end_time: String,
    pub label_count: usize,
    pub compared_label_count: usize,
    pub passing_label_count: usize,
    pub stage_segment_count: usize,
    #[serde(default)]
    pub acceptance_summary: SleepStageLabelAcceptanceSummary,
    pub comparisons: Vec<SleepStageLabelComparison>,
    pub issues: Vec<String>,
    pub quality_flags: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<SleepStageLabelValidationNextAction>,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SleepStageLabelAcceptanceSummary {
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub user_owned_stage_sample_ready: bool,
    #[serde(default)]
    pub label_count: usize,
    #[serde(default)]
    pub compared_label_count: usize,
    #[serde(default)]
    pub passing_label_count: usize,
    #[serde(default)]
    pub stage_segment_count: usize,
    #[serde(default)]
    pub required_release_passing_stage_labels: usize,
    #[serde(default)]
    pub accepted_label_ids: Vec<String>,
    #[serde(default)]
    pub accepted_stage_kinds: Vec<String>,
    #[serde(default)]
    pub min_label_confidence: f64,
    #[serde(default)]
    pub min_overlap_fraction: f64,
    #[serde(default)]
    pub min_observed_overlap_fraction: f64,
    #[serde(default)]
    pub min_observed_label_confidence: f64,
    pub issue_count: usize,
    pub quality_flag_count: usize,
    pub error_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepStageLabelComparison {
    pub label_id: String,
    pub sleep_id: Option<String>,
    pub source: String,
    pub provenance_source: String,
    pub label_provenance_policy: String,
    pub confidence: Option<f64>,
    pub expected_stage_kind: String,
    pub observed_stage_kind: Option<String>,
    pub label_start_time: String,
    pub label_end_time: String,
    pub label_start_time_unix_ms: i64,
    pub label_end_time_unix_ms: i64,
    pub observed_start_time: Option<String>,
    pub observed_end_time: Option<String>,
    pub overlap_minutes: f64,
    pub overlap_fraction: f64,
    pub pass: bool,
    pub quality_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepStageLabelValidationNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1ExplanationStabilityOptions {
    pub max_repeated_run_delta: f64,
    pub max_small_perturbation_delta: f64,
    pub perturb_sleep_duration_minutes: f64,
    pub min_v1_component_count: usize,
    pub min_explanation_quality_signal_count: usize,
}

impl Default for SleepV1ExplanationStabilityOptions {
    fn default() -> Self {
        Self {
            max_repeated_run_delta: 0.000_001,
            max_small_perturbation_delta: 5.0,
            perturb_sleep_duration_minutes: 5.0,
            min_v1_component_count: sleep_v1_expected_component_names().len(),
            min_explanation_quality_signal_count: sleep_v1_expected_explanation_quality_signals()
                .len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1ExplanationStabilityReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub explanation_pass: bool,
    pub explanation_quality_pass: bool,
    pub repeated_run_stability_pass: bool,
    pub perturbation_stability_pass: bool,
    pub v0_component_count: usize,
    pub v1_component_count: usize,
    pub v1_component_names: Vec<String>,
    pub explanation_quality_signal_count: usize,
    pub explanation_quality_signals: Vec<String>,
    pub missing_component_provenance: Vec<String>,
    pub missing_component_inputs: Vec<String>,
    pub missing_component_policy: Vec<String>,
    #[serde(default)]
    pub sleep_window_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub perturbed_sleep_window_confidence_0_to_1: Option<f64>,
    pub repeated_run_delta: Option<f64>,
    pub small_perturbation_delta: Option<f64>,
    #[serde(default)]
    pub acceptance_summary: SleepV1ExplanationStabilityAcceptanceSummary,
    pub quality_flags: Vec<String>,
    pub errors: Vec<String>,
    pub issues: Vec<String>,
    pub next_actions: Vec<SleepV1ExplanationStabilityNextAction>,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SleepV1ExplanationStabilityAcceptanceSummary {
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub explanation_and_stability_ready: bool,
    #[serde(default)]
    pub expected_component_names: Vec<String>,
    #[serde(default)]
    pub observed_component_names: Vec<String>,
    #[serde(default)]
    pub v0_component_count: usize,
    #[serde(default)]
    pub v1_component_count: usize,
    #[serde(default)]
    pub min_v1_component_count: usize,
    #[serde(default)]
    pub explanation_quality_signals: Vec<String>,
    #[serde(default)]
    pub explanation_quality_signal_count: usize,
    #[serde(default)]
    pub min_explanation_quality_signal_count: usize,
    #[serde(default)]
    pub sleep_window_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub perturbed_sleep_window_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub repeated_run_delta: Option<f64>,
    #[serde(default)]
    pub max_repeated_run_delta: f64,
    #[serde(default)]
    pub small_perturbation_delta: Option<f64>,
    #[serde(default)]
    pub max_small_perturbation_delta: f64,
    #[serde(default)]
    pub perturb_sleep_duration_minutes: f64,
    pub issue_count: usize,
    pub quality_flag_count: usize,
    pub error_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepV1ExplanationStabilityNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1ReleaseGateInput {
    #[serde(default)]
    pub physical_historical_sync: Option<HistoricalSyncPhysicalValidationReport>,
    #[serde(default)]
    pub sleep_window_label_validation: Option<SleepWindowLabelValidationReport>,
    #[serde(default)]
    pub sleep_stage_label_validation: Option<SleepStageLabelValidationReport>,
    #[serde(default)]
    pub explanation_stability: Option<SleepV1ExplanationStabilityReport>,
    #[serde(default)]
    pub benchmark_comparisons: Vec<AlgorithmComparisonReport>,
    #[serde(default = "default_min_hand_reviewed_window_comparisons")]
    pub min_hand_reviewed_window_comparisons: usize,
    #[serde(default = "default_min_stage_label_comparisons")]
    pub min_stage_label_comparisons: usize,
    #[serde(default = "default_min_benchmark_comparisons")]
    pub min_benchmark_comparisons: usize,
}

impl Default for SleepV1ReleaseGateInput {
    fn default() -> Self {
        Self {
            physical_historical_sync: None,
            sleep_window_label_validation: None,
            sleep_stage_label_validation: None,
            explanation_stability: None,
            benchmark_comparisons: Vec::new(),
            min_hand_reviewed_window_comparisons: default_min_hand_reviewed_window_comparisons(),
            min_stage_label_comparisons: default_min_stage_label_comparisons(),
            min_benchmark_comparisons: default_min_benchmark_comparisons(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepV1ReleaseGateReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub physical_historical_sync_pass: bool,
    pub timestamp_evidence_pass: bool,
    pub sleep_window_label_pass: bool,
    pub sleep_stage_label_pass: bool,
    pub explanation_stability_pass: bool,
    pub benchmark_comparison_pass: bool,
    pub hand_reviewed_window_comparisons: usize,
    pub stage_label_comparison_count: usize,
    pub benchmark_comparison_count: usize,
    pub issues: Vec<String>,
    pub quality_flags: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<SleepV1ReleaseGateNextAction>,
    pub provenance: Value,
    #[serde(default)]
    pub acceptance_summary: SleepV1ReleaseGateAcceptanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SleepV1ReleaseGateAcceptanceSummary {
    pub policy: String,
    pub release_ready: bool,
    pub physical_historical_sync_pass: bool,
    pub timestamp_evidence_pass: bool,
    pub sleep_window_label_pass: bool,
    pub sleep_stage_label_pass: bool,
    pub explanation_stability_pass: bool,
    pub benchmark_comparison_pass: bool,
    pub hand_reviewed_window_comparisons: usize,
    pub min_hand_reviewed_window_comparisons: usize,
    pub stage_label_comparison_count: usize,
    pub min_stage_label_comparisons: usize,
    pub benchmark_comparison_count: usize,
    pub min_benchmark_comparisons: usize,
    pub issue_count: usize,
    pub quality_flag_count: usize,
    pub error_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SleepV1ReleaseGateNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

pub fn validate_sleep_v1_release_gates(
    input: &SleepV1ReleaseGateInput,
) -> SleepV1ReleaseGateReport {
    let mut issues = Vec::new();

    let physical_historical_sync_pass =
        input
            .physical_historical_sync
            .as_ref()
            .is_some_and(|report| {
                report.pass
                    && historical_sync_physical_report_integrity_pass(report)
                    && report.service_uuid_confirmed
                    && report.characteristic_roles_confirmed
                    && report.notification_behavior_confirmed
                    && report.auth_session_handshake_confirmed
                    && report.command_flow_confirmed
                    && report.evidence_session_confirmed
            });
    if input.physical_historical_sync.is_none() {
        issues.push("physical_historical_sync_report_missing".to_string());
    }
    if !physical_historical_sync_pass {
        issues.push("physical_historical_sync_not_validated".to_string());
    }
    if input
        .physical_historical_sync
        .as_ref()
        .is_some_and(|report| !historical_sync_physical_report_integrity_pass(report))
    {
        issues.push("physical_historical_sync_report_integrity_failed".to_string());
    }
    if let Some(report) = input
        .physical_historical_sync
        .as_ref()
        .filter(|report| historical_sync_physical_report_integrity_pass(report))
    {
        for issue in &report.issues {
            if matches!(
                issue.as_str(),
                "historical_motion_timestamp_fields_unproven"
                    | "historical_heart_rate_timestamp_fields_unproven"
            ) {
                issues.push(issue.clone());
            }
        }
    }

    let timestamp_evidence_pass = input
        .physical_historical_sync
        .as_ref()
        .is_some_and(|report| {
            historical_sync_physical_report_integrity_pass(report)
                && report.pass
                && report.timestamp_fields_confirmed
        });
    if !timestamp_evidence_pass {
        issues.push("historical_motion_hr_timestamps_not_proven".to_string());
    }

    let hand_reviewed_window_comparisons = input
        .sleep_window_label_validation
        .as_ref()
        .map_or(0, unique_passing_sleep_window_count);
    let sleep_window_label_pass =
        input
            .sleep_window_label_validation
            .as_ref()
            .is_some_and(|report| {
                report.pass
                    && sleep_window_label_report_integrity_pass(report)
                    && sleep_window_label_thresholds_at_least_default(report)
                    && hand_reviewed_window_comparisons
                        >= input.min_hand_reviewed_window_comparisons
                    && report.passing_label_count == report.compared_label_count
            });
    if input.sleep_window_label_validation.is_none() {
        issues.push("sleep_window_label_report_missing".to_string());
    }
    if !sleep_window_label_pass {
        issues.push("packet_sleep_windows_not_validated_against_hand_review".to_string());
    }
    if hand_reviewed_window_comparisons < input.min_hand_reviewed_window_comparisons {
        issues.push("hand_reviewed_sleep_window_sample_below_gate".to_string());
    }
    if input.min_hand_reviewed_window_comparisons < default_min_hand_reviewed_window_comparisons() {
        issues.push("release_gate_hand_reviewed_window_threshold_below_default".to_string());
    }
    if input
        .sleep_window_label_validation
        .as_ref()
        .is_some_and(|report| !sleep_window_label_report_integrity_pass(report))
    {
        issues.push("sleep_window_label_report_integrity_failed".to_string());
    }
    if input
        .sleep_window_label_validation
        .as_ref()
        .is_some_and(|report| !sleep_window_label_thresholds_at_least_default(report))
    {
        issues.push("sleep_window_label_threshold_below_default".to_string());
    }

    let stage_label_comparison_count = input
        .sleep_stage_label_validation
        .as_ref()
        .map_or(0, unique_passing_sleep_stage_label_count);
    let sleep_stage_label_pass =
        input
            .sleep_stage_label_validation
            .as_ref()
            .is_some_and(|report| {
                report.pass
                    && sleep_stage_label_report_integrity_pass(report)
                    && sleep_stage_label_thresholds_at_least_default(report)
                    && stage_label_comparison_count >= input.min_stage_label_comparisons
                    && report.passing_label_count == report.compared_label_count
            });
    if input.sleep_stage_label_validation.is_none() {
        issues.push("sleep_stage_label_report_missing".to_string());
    }
    if !sleep_stage_label_pass {
        issues.push("sleep_v1_stage_labels_not_validated".to_string());
    }
    if stage_label_comparison_count < input.min_stage_label_comparisons {
        issues.push("sleep_stage_label_sample_below_gate".to_string());
    }
    if input.min_stage_label_comparisons < default_min_stage_label_comparisons() {
        issues.push("release_gate_stage_label_threshold_below_default".to_string());
    }
    if input
        .sleep_stage_label_validation
        .as_ref()
        .is_some_and(|report| !sleep_stage_label_report_integrity_pass(report))
    {
        issues.push("sleep_stage_label_report_integrity_failed".to_string());
    }
    if input
        .sleep_stage_label_validation
        .as_ref()
        .is_some_and(|report| !sleep_stage_label_thresholds_at_least_default(report))
    {
        issues.push("sleep_stage_label_threshold_below_default".to_string());
    }

    let explanation_stability_pass = input.explanation_stability.as_ref().is_some_and(|report| {
        report.pass
            && sleep_v1_explanation_stability_report_integrity_pass(report)
            && sleep_v1_explanation_stability_thresholds_at_least_default(report)
            && report.explanation_pass
            && report.repeated_run_stability_pass
            && report.perturbation_stability_pass
    });
    if input.explanation_stability.is_none() {
        issues.push("sleep_v1_explanation_stability_report_missing".to_string());
    }
    if !explanation_stability_pass {
        issues.push("sleep_v1_explanation_or_stability_not_validated".to_string());
    }
    if input
        .explanation_stability
        .as_ref()
        .is_some_and(|report| !sleep_v1_explanation_stability_report_integrity_pass(report))
    {
        issues.push("sleep_v1_explanation_stability_report_integrity_failed".to_string());
    }
    if input
        .explanation_stability
        .as_ref()
        .is_some_and(|report| !sleep_v1_explanation_stability_thresholds_at_least_default(report))
    {
        issues.push("sleep_v1_explanation_stability_threshold_below_default".to_string());
    }

    let passing_benchmark_count = input
        .benchmark_comparisons
        .iter()
        .filter(|report| sleep_v1_benchmark_report_ready(report))
        .count();
    let benchmark_comparison_pass = passing_benchmark_count >= input.min_benchmark_comparisons;
    if input.benchmark_comparisons.is_empty() {
        issues.push("sleep_v1_benchmark_report_missing".to_string());
    }
    if !benchmark_comparison_pass {
        issues.push("sleep_v1_benchmark_comparison_below_gate".to_string());
    }
    if input.min_benchmark_comparisons < default_min_benchmark_comparisons() {
        issues.push("release_gate_benchmark_threshold_below_default".to_string());
    }
    if input
        .benchmark_comparisons
        .iter()
        .any(|report| report.family == "sleep" && !sleep_v1_benchmark_report_ready(report))
    {
        issues.push("sleep_v1_benchmark_report_integrity_failed".to_string());
    }

    issues.sort();
    issues.dedup();
    let pass = physical_historical_sync_pass
        && timestamp_evidence_pass
        && sleep_window_label_pass
        && sleep_stage_label_pass
        && explanation_stability_pass
        && benchmark_comparison_pass
        && issues.is_empty();

    let mut report = SleepV1ReleaseGateReport {
        schema: SLEEP_V1_RELEASE_GATE_SCHEMA.to_string(),
        generated_by: "goose-sleep-v1-release-gate-validator".to_string(),
        pass,
        physical_historical_sync_pass,
        timestamp_evidence_pass,
        sleep_window_label_pass,
        sleep_stage_label_pass,
        explanation_stability_pass,
        benchmark_comparison_pass,
        hand_reviewed_window_comparisons,
        stage_label_comparison_count,
        benchmark_comparison_count: passing_benchmark_count,
        quality_flags: Vec::new(),
        errors: Vec::new(),
        next_actions: sleep_v1_release_gate_next_actions(&issues),
        issues,
        provenance: json!({
            "promotion_policy": "sleep_v1_primary_requires_all_release_gates",
            "report_integrity_policy": "sleep_v1_release_gate_requires_current_subgate_integrity_and_empty_proof_arrays",
            "threshold_policy": SLEEP_V1_RELEASE_GATE_THRESHOLD_POLICY,
            "subgate_report_integrity_policies": {
                "historical-sync-validation.json": HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY,
                "sleep-window-validation.json": "sleep_window_label_validation_requires_current_feature_and_comparison_integrity",
                "sleep-stage-validation.json": SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY,
                "sleep-v1-stability.json": "sleep_v1_explanation_stability_requires_current_component_status_and_delta_integrity",
                "sleep-v1-benchmark.json": "sleep_v1_benchmark_requires_current_comparison_output_and_delta_integrity",
            },
            "subgate_report_validation_policies": {
                "historical-sync-validation.json": HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY,
                "sleep-window-validation.json": SLEEP_WINDOW_LABEL_VALIDATION_POLICY,
                "sleep-stage-validation.json": SLEEP_STAGE_LABEL_VALIDATION_POLICY,
                "sleep-v1-stability.json": SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY,
                "sleep-v1-benchmark.json": SLEEP_V1_BENCHMARK_COMPARISON_POLICY,
            },
            "min_hand_reviewed_window_comparisons": input.min_hand_reviewed_window_comparisons,
            "min_stage_label_comparisons": input.min_stage_label_comparisons,
            "min_benchmark_comparisons": input.min_benchmark_comparisons,
        }),
        acceptance_summary: SleepV1ReleaseGateAcceptanceSummary::default(),
    };
    report.acceptance_summary = sleep_v1_release_gate_acceptance_summary(&report);
    report
}

fn sleep_v1_release_gate_acceptance_summary(
    report: &SleepV1ReleaseGateReport,
) -> SleepV1ReleaseGateAcceptanceSummary {
    SleepV1ReleaseGateAcceptanceSummary {
        policy: "sleep_v1_release_gate_must_match_current_subgate_threshold_and_proof_contract"
            .to_string(),
        release_ready: report.pass,
        physical_historical_sync_pass: report.physical_historical_sync_pass,
        timestamp_evidence_pass: report.timestamp_evidence_pass,
        sleep_window_label_pass: report.sleep_window_label_pass,
        sleep_stage_label_pass: report.sleep_stage_label_pass,
        explanation_stability_pass: report.explanation_stability_pass,
        benchmark_comparison_pass: report.benchmark_comparison_pass,
        hand_reviewed_window_comparisons: report.hand_reviewed_window_comparisons,
        min_hand_reviewed_window_comparisons: report
            .provenance
            .get("min_hand_reviewed_window_comparisons")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or_default(),
        stage_label_comparison_count: report.stage_label_comparison_count,
        min_stage_label_comparisons: report
            .provenance
            .get("min_stage_label_comparisons")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or_default(),
        benchmark_comparison_count: report.benchmark_comparison_count,
        min_benchmark_comparisons: report
            .provenance
            .get("min_benchmark_comparisons")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or_default(),
        issue_count: report.issues.len(),
        quality_flag_count: report.quality_flags.len(),
        error_count: report.errors.len(),
        next_action_count: report.next_actions.len(),
    }
}

fn sleep_v1_release_gate_subgate_integrity_policies() -> [(&'static str, &'static str); 5] {
    [
        (
            "historical-sync-validation.json",
            HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY,
        ),
        (
            "sleep-window-validation.json",
            SLEEP_WINDOW_LABEL_REPORT_INTEGRITY_POLICY,
        ),
        (
            "sleep-stage-validation.json",
            SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY,
        ),
        (
            "sleep-v1-stability.json",
            SLEEP_V1_EXPLANATION_STABILITY_INTEGRITY_POLICY,
        ),
        (
            "sleep-v1-benchmark.json",
            SLEEP_V1_BENCHMARK_REPORT_INTEGRITY_POLICY,
        ),
    ]
}

fn sleep_v1_release_gate_subgate_validation_policies() -> [(&'static str, &'static str); 5] {
    [
        (
            "historical-sync-validation.json",
            HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY,
        ),
        (
            "sleep-window-validation.json",
            SLEEP_WINDOW_LABEL_VALIDATION_POLICY,
        ),
        (
            "sleep-stage-validation.json",
            SLEEP_STAGE_LABEL_VALIDATION_POLICY,
        ),
        (
            "sleep-v1-stability.json",
            SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY,
        ),
        (
            "sleep-v1-benchmark.json",
            SLEEP_V1_BENCHMARK_COMPARISON_POLICY,
        ),
    ]
}

fn unique_passing_sleep_window_count(report: &SleepWindowLabelValidationReport) -> usize {
    report
        .comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .map(|comparison| {
            comparison
                .sleep_id
                .as_deref()
                .unwrap_or(&comparison.label_id)
                .to_string()
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn unique_passing_sleep_stage_label_count(report: &SleepStageLabelValidationReport) -> usize {
    report
        .comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .map(|comparison| comparison.label_id.clone())
        .collect::<BTreeSet<_>>()
        .len()
}

fn historical_sync_physical_report_integrity_pass(
    report: &HistoricalSyncPhysicalValidationReport,
) -> bool {
    let all_subgates = report.service_uuid_confirmed
        && report.characteristic_roles_confirmed
        && report.notification_behavior_confirmed
        && report.auth_session_handshake_confirmed
        && report.command_flow_confirmed
        && report.event_order_confirmed
        && report.evidence_session_confirmed
        && report.raw_evidence_anchored
        && report.timestamp_fields_confirmed;
    let passing_proof_counts_present = !report.pass
        || (report.service_uuid_count > 0
            && report.characteristic_count >= 3
            && report.notification_subscription_count >= 2
            && report.auth_event_count >= 3
            && report.command_event_count >= 2
            && report.metadata_event_count >= 3
            && report.timestamp_evidence_count >= 2
            && report.raw_evidence_anchor_count >= 10
            && report.motion_timestamp_evidence_count > 0
            && report.heart_rate_timestamp_evidence_count > 0);
    report.schema == HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA
        && report.generated_by == "goose-historical-sync-physical-validator"
        && !report.capture_session_id.trim().is_empty()
        && passing_proof_counts_present
        && report.quality_flags.is_empty()
        && report.errors.is_empty()
        && report
            .provenance
            .get("report_integrity_policy")
            .and_then(Value::as_str)
            == Some(HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY)
        && report
            .provenance
            .get("validation_policy")
            .and_then(Value::as_str)
            == Some(HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY)
        && report.next_actions == physical_validation_next_actions(&report.issues)
        && report.acceptance_summary == historical_sync_physical_acceptance_summary(report)
        && report.pass == (all_subgates && report.issues.is_empty())
}

fn sleep_window_label_report_integrity_pass(report: &SleepWindowLabelValidationReport) -> bool {
    let min_label_confidence = report
        .provenance
        .get("min_label_confidence")
        .and_then(Value::as_f64)
        .unwrap_or(SleepWindowLabelValidationOptions::default().min_label_confidence);
    let provenance_valid = sleep_window_label_thresholds_at_least_default(report)
        && report
            .provenance
            .get("start_tolerance_minutes")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        && report
            .provenance
            .get("end_tolerance_minutes")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        && report
            .provenance
            .get("duration_tolerance_minutes")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        && report
            .provenance
            .get("min_label_confidence")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        && report
            .provenance
            .get("label_source")
            .and_then(Value::as_str)
            == Some("sleep_correction_labels")
        && report
            .provenance
            .get("comparison_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_WINDOW_LABEL_VALIDATION_POLICY)
        && report
            .provenance
            .get("validation_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_WINDOW_LABEL_VALIDATION_POLICY)
        && report
            .provenance
            .get("distinct_window_policy")
            .and_then(Value::as_str)
            == Some("one_hand_reviewed_sleep_window_per_sleep_id")
        && report
            .provenance
            .get("report_integrity_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_WINDOW_LABEL_REPORT_INTEGRITY_POLICY);
    let comparison_rows_valid = report.comparisons.iter().all(|comparison| {
        !comparison.label_id.trim().is_empty()
            && !comparison.source.trim().is_empty()
            && comparison
                .sleep_id
                .as_deref()
                .is_some_and(|sleep_id| !sleep_id.trim().is_empty())
            && comparison.confidence.is_some_and(|confidence| {
                confidence.is_finite()
                    && (0.0..=1.0).contains(&confidence)
                    && confidence >= min_label_confidence
            })
            && comparison.pass == comparison.quality_flags.is_empty()
            && sleep_window_label_comparison_integrity_pass(comparison)
    });
    let passing_label_count = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .count();
    let distinct_compared_sleep_window_count = unique_sleep_window_count(&report.comparisons);
    let passing_comparisons = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .cloned()
        .collect::<Vec<_>>();
    let distinct_passing_sleep_window_count = unique_sleep_window_count(&passing_comparisons);
    let acceptance_summary = sleep_window_label_acceptance_summary(
        report.label_count,
        &report.comparisons,
        min_label_confidence,
        &SleepWindowLabelValidationOptions {
            start_tolerance_minutes: report
                .provenance
                .get("start_tolerance_minutes")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            end_tolerance_minutes: report
                .provenance
                .get("end_tolerance_minutes")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            duration_tolerance_minutes: report
                .provenance
                .get("duration_tolerance_minutes")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            min_label_confidence,
            ..SleepWindowLabelValidationOptions::default()
        },
        report.issues.len(),
        report.quality_flags.len(),
        report.errors.len(),
        report.next_actions.len(),
    );

    let sleep_feature_report_valid =
        sleep_feature_report_integrity_pass(&report.sleep_feature_report);

    let pass = sleep_feature_report_valid
        && !report.comparisons.is_empty()
        && report.label_count == report.comparisons.len()
        && passing_label_count == report.comparisons.len()
        && report.issues.is_empty();

    report.schema == SLEEP_WINDOW_LABEL_VALIDATION_SCHEMA
        && report.generated_by == "goose-sleep-window-label-validator"
        && report.pass == pass
        && sleep_feature_report_valid
        && provenance_valid
        && report.quality_flags.is_empty()
        && report.errors.is_empty()
        && comparison_rows_valid
        && report.start_time == report.sleep_feature_report.start_time
        && report.end_time == report.sleep_feature_report.end_time
        && report.sleep_window_available == report.sleep_feature_report.sleep_window.is_some()
        && report.label_count >= report.comparisons.len()
        && report.compared_label_count == report.comparisons.len()
        && report.passing_label_count == passing_label_count
        && report.distinct_compared_sleep_window_count == distinct_compared_sleep_window_count
        && report.distinct_passing_sleep_window_count == distinct_passing_sleep_window_count
        && report.acceptance_summary == acceptance_summary
        && report.next_actions == sleep_window_label_validation_next_actions(&report.issues)
}

fn sleep_stage_label_report_integrity_pass(report: &SleepStageLabelValidationReport) -> bool {
    let min_label_confidence = report
        .provenance
        .get("min_label_confidence")
        .and_then(Value::as_f64)
        .unwrap_or(SleepStageLabelValidationOptions::default().min_label_confidence);
    let min_overlap_fraction = report
        .provenance
        .get("min_overlap_fraction")
        .and_then(Value::as_f64)
        .unwrap_or(SleepStageLabelValidationOptions::default().min_overlap_fraction);
    let provenance_valid = sleep_stage_label_thresholds_at_least_default(report)
        && report
            .provenance
            .get("label_source")
            .and_then(Value::as_str)
            == Some("sleep_correction_labels")
        && report
            .provenance
            .get("comparison_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_STAGE_LABEL_VALIDATION_POLICY)
        && report
            .provenance
            .get("validation_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_STAGE_LABEL_VALIDATION_POLICY)
        && report
            .provenance
            .get("official_labels_policy")
            .and_then(Value::as_str)
            == Some("official_or_platform_stage_values_are_labels_not_goose_outputs")
        && report
            .provenance
            .get("report_integrity_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY);
    let comparison_rows_valid = report.comparisons.iter().all(|comparison| {
        !comparison.label_id.trim().is_empty()
            && !comparison.source.trim().is_empty()
            && comparison.sleep_id.as_deref() == Some(report.sleep_id.as_str())
            && !comparison.provenance_source.trim().is_empty()
            && comparison.provenance_source == comparison.source
            && matches!(
                comparison.label_provenance_policy.as_str(),
                "user_owned_sleep_stage_label" | "official_labels_are_labels"
            )
            && comparison.confidence.is_some_and(|confidence| {
                confidence.is_finite()
                    && (0.0..=1.0).contains(&confidence)
                    && confidence >= min_label_confidence
            })
            && canonical_sleep_stage_label_kind(&comparison.expected_stage_kind).is_some()
            && comparison.observed_stage_kind.as_deref()
                == Some(comparison.expected_stage_kind.as_str())
            && !comparison.label_start_time.trim().is_empty()
            && !comparison.label_end_time.trim().is_empty()
            && comparison
                .observed_start_time
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && comparison
                .observed_end_time
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && comparison.label_end_time_unix_ms > comparison.label_start_time_unix_ms
            && comparison.overlap_minutes.is_finite()
            && comparison.overlap_minutes > 0.0
            && comparison.overlap_fraction.is_finite()
            && comparison.overlap_fraction >= min_overlap_fraction
            && comparison.overlap_fraction <= 1.0
            && comparison.pass
            && comparison.quality_flags.is_empty()
    });
    let distinct_label_count = report
        .comparisons
        .iter()
        .map(|comparison| comparison.label_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let passing_label_count = report
        .comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .count();
    let acceptance_summary = sleep_stage_label_acceptance_summary(
        report.label_count,
        report.stage_segment_count,
        &report.comparisons,
        min_label_confidence,
        min_overlap_fraction,
        report.issues.len(),
        report.quality_flags.len(),
        report.errors.len(),
        report.next_actions.len(),
    );
    let pass = !report.comparisons.is_empty()
        && report.label_count == report.comparisons.len()
        && report.compared_label_count == report.comparisons.len()
        && distinct_label_count == report.comparisons.len()
        && report.passing_label_count == passing_label_count
        && passing_label_count == report.comparisons.len()
        && report.stage_segment_count > 0
        && report.issues.is_empty();

    report.schema == SLEEP_STAGE_LABEL_VALIDATION_SCHEMA
        && report.generated_by == "goose-sleep-stage-label-validator"
        && report.pass == pass
        && provenance_valid
        && comparison_rows_valid
        && report.quality_flags.is_empty()
        && report.errors.is_empty()
        && !report.sleep_id.trim().is_empty()
        && !report.start_time.trim().is_empty()
        && !report.end_time.trim().is_empty()
        && report.acceptance_summary == acceptance_summary
        && report.next_actions == sleep_stage_label_validation_next_actions(&report.issues)
}

fn sleep_stage_label_thresholds_at_least_default(report: &SleepStageLabelValidationReport) -> bool {
    let defaults = SleepStageLabelValidationOptions::default();
    report
        .provenance
        .get("min_label_confidence")
        .and_then(Value::as_f64)
        .is_some_and(|value| {
            value.is_finite()
                && (0.0..=1.0).contains(&value)
                && value >= defaults.min_label_confidence
        })
        && report
            .provenance
            .get("min_overlap_fraction")
            .and_then(Value::as_f64)
            .is_some_and(|value| {
                value.is_finite()
                    && (0.0..=1.0).contains(&value)
                    && value >= defaults.min_overlap_fraction
            })
}

fn sleep_feature_report_integrity_pass(report: &SleepFeatureScoreReport) -> bool {
    let (Some(window), Some(input), Some(score_result)) = (
        report.sleep_window.as_ref(),
        report.sleep_input.as_ref(),
        report.score_result.as_ref(),
    ) else {
        return false;
    };
    let Some(score_output) = score_result.output.as_ref() else {
        return false;
    };

    report.schema == SLEEP_FEATURE_SCORE_REPORT_SCHEMA
        && report.generated_by == "goose-sleep-feature-score-builder"
        && report.pass == report.issues.is_empty()
        && report.pass
        && report.require_trusted_evidence
        && report.next_actions.is_empty()
        && sleep_motion_report_integrity_pass(report)
        && sleep_heart_rate_report_integrity_pass(report)
        && sleep_window_feature_input_integrity_pass(window, input)
        && score_result.algorithm_id == GOOSE_SLEEP_V0_ID
        && score_result.algorithm_version == GOOSE_SLEEP_V0_VERSION
        && score_result.family == "sleep"
        && score_result.start_time == input.start_time
        && score_result.end_time == input.end_time
        && score_result.errors.is_empty()
        && score_output.algorithm_id == GOOSE_SLEEP_V0_ID
        && score_output.algorithm_version == GOOSE_SLEEP_V0_VERSION
        && finite_non_negative(score_output.score_0_to_100)
        && score_output.score_0_to_100 <= 100.0
        && finite_non_negative(score_output.sleep_performance_fraction)
        && score_output.sleep_performance_fraction <= 1.0
        && finite_non_negative(score_output.efficiency_fraction)
        && score_output.efficiency_fraction <= 1.0
        && finite_non_negative(score_output.sleep_debt_minutes)
        && approx_equal(
            score_output.sleep_latency_minutes,
            input.sleep_latency_minutes,
        )
        && approx_equal(
            score_output.wake_after_sleep_onset_minutes,
            input.wake_after_sleep_onset_minutes,
        )
        && score_output.wake_episode_count == input.wake_episode_count
        && optional_approx_equal(
            score_output.heart_rate_dip_percent,
            input.heart_rate_dip_percent,
        )
}

fn sleep_motion_report_integrity_pass(report: &SleepFeatureScoreReport) -> bool {
    let motion = &report.motion_report;
    motion.schema == MOTION_FEATURE_REPORT_SCHEMA
        && motion.generated_by == "goose-motion-feature-extractor"
        && motion.pass == motion.issues.is_empty()
        && motion.pass
        && motion.require_trusted_evidence
        && motion.capture_correlation_pass
        && motion.candidate_frame_count >= motion.feature_count
        && motion.feature_count == motion.features.len()
        && motion.trusted_feature_count
            == motion
                .features
                .iter()
                .filter(|feature| feature.trusted_metric_input)
                .count()
        && motion.trusted_feature_count > 0
        && motion.next_actions.is_empty()
}

fn sleep_heart_rate_report_integrity_pass(report: &SleepFeatureScoreReport) -> bool {
    let heart_rate = &report.heart_rate_report;
    heart_rate.schema == HEART_RATE_FEATURE_REPORT_SCHEMA
        && heart_rate.generated_by == "goose-heart-rate-feature-extractor"
        && heart_rate.pass == heart_rate.issues.is_empty()
        && heart_rate.pass
        && heart_rate.require_trusted_evidence
        && heart_rate.capture_correlation_pass
        && heart_rate.candidate_frame_count >= heart_rate.feature_count
        && heart_rate.feature_count == heart_rate.features.len()
        && heart_rate.trusted_feature_count
            == heart_rate
                .features
                .iter()
                .filter(|feature| feature.trusted_metric_input)
                .count()
        && heart_rate.trusted_feature_count > 0
        && heart_rate.next_actions.is_empty()
}

fn sleep_window_feature_input_integrity_pass(
    window: &SleepWindowFeature,
    input: &SleepInput,
) -> bool {
    let Some(window_start_unix_ms) = parse_rfc3339_utc_unix_ms(&window.start_time) else {
        return false;
    };
    let Some(window_end_unix_ms) = parse_rfc3339_utc_unix_ms(&window.end_time) else {
        return false;
    };
    if window_end_unix_ms <= window_start_unix_ms {
        return false;
    }

    input.start_time == window.start_time
        && input.end_time == window.end_time
        && approx_equal(input.sleep_duration_minutes, window.sleep_duration_minutes)
        && approx_equal(input.time_in_bed_minutes, window.time_in_bed_minutes)
        && approx_equal(
            input.midpoint_deviation_minutes,
            window.midpoint_deviation_minutes,
        )
        && input.disturbance_count == window.disturbance_count
        && approx_equal(input.sleep_latency_minutes, window.sleep_latency_minutes)
        && approx_equal(
            input.wake_after_sleep_onset_minutes,
            window.wake_after_sleep_onset_minutes,
        )
        && input.wake_episode_count == window.wake_episode_count
        && input.stage_minutes == window.stage_minutes
        && optional_approx_equal(input.heart_rate_dip_percent, window.heart_rate_dip_percent)
        && input.input_ids == window.input_ids
        && finite_positive(window.time_in_bed_minutes)
        && finite_non_negative(window.sleep_duration_minutes)
        && window.sleep_duration_minutes <= window.time_in_bed_minutes
        && finite_non_negative(window.sleep_latency_minutes)
        && finite_non_negative(window.wake_after_sleep_onset_minutes)
        && finite_non_negative(window.midpoint_deviation_minutes)
        && finite_non_negative(window.motion_coverage_fraction)
        && window.motion_coverage_fraction <= 1.0
        && finite_non_negative(window.heart_rate_coverage_fraction)
        && window.heart_rate_coverage_fraction <= 1.0
        && window.motion_feature_count > 0
        && window.heart_rate_feature_count > 0
        && !window.stage_segments.is_empty()
        && !window.input_ids.is_empty()
        && window.trusted_metric_input
        && window
            .provenance
            .get("promotion_policy")
            .and_then(Value::as_str)
            == Some("requires_all_contributing_features_trusted")
}

fn sleep_window_label_comparison_integrity_pass(comparison: &SleepWindowLabelComparison) -> bool {
    let Some(expected_start_time_unix_ms) =
        parse_rfc3339_utc_unix_ms(&comparison.expected_start_time)
    else {
        return false;
    };
    let Some(expected_end_time_unix_ms) = parse_rfc3339_utc_unix_ms(&comparison.expected_end_time)
    else {
        return false;
    };
    let Some(observed_start_time_unix_ms) =
        parse_rfc3339_utc_unix_ms(&comparison.observed_start_time)
    else {
        return false;
    };
    let Some(observed_end_time_unix_ms) = parse_rfc3339_utc_unix_ms(&comparison.observed_end_time)
    else {
        return false;
    };
    if expected_start_time_unix_ms != comparison.expected_start_time_unix_ms
        || expected_end_time_unix_ms != comparison.expected_end_time_unix_ms
        || observed_start_time_unix_ms != comparison.observed_start_time_unix_ms
        || observed_end_time_unix_ms != comparison.observed_end_time_unix_ms
        || comparison.provenance_source.trim().is_empty()
        || comparison.provenance_source != comparison.source
        || comparison.label_provenance_policy != "hand_reviewed_sleep_window"
        || comparison.observed_end_time_unix_ms <= comparison.observed_start_time_unix_ms
        || comparison.expected_end_time_unix_ms <= comparison.expected_start_time_unix_ms
        || !comparison.start_delta_minutes.is_finite()
        || !comparison.end_delta_minutes.is_finite()
        || !comparison.duration_delta_minutes.is_finite()
    {
        return false;
    }

    let expected_start_delta_minutes = (comparison.observed_start_time_unix_ms
        - comparison.expected_start_time_unix_ms)
        .abs() as f64
        / 60_000.0;
    let expected_end_delta_minutes =
        (comparison.observed_end_time_unix_ms - comparison.expected_end_time_unix_ms).abs() as f64
            / 60_000.0;
    let observed_duration_minutes = (comparison.observed_end_time_unix_ms
        - comparison.observed_start_time_unix_ms) as f64
        / 60_000.0;
    let expected_duration_minutes = (comparison.expected_end_time_unix_ms
        - comparison.expected_start_time_unix_ms) as f64
        / 60_000.0;
    let expected_duration_delta_minutes =
        (observed_duration_minutes - expected_duration_minutes).abs();

    approx_equal(comparison.start_delta_minutes, expected_start_delta_minutes)
        && approx_equal(comparison.end_delta_minutes, expected_end_delta_minutes)
        && approx_equal(
            comparison.duration_delta_minutes,
            expected_duration_delta_minutes,
        )
}

fn sleep_window_label_thresholds_at_least_default(
    report: &SleepWindowLabelValidationReport,
) -> bool {
    let defaults = SleepWindowLabelValidationOptions::default();
    let start_tolerance_minutes = report
        .provenance
        .get("start_tolerance_minutes")
        .and_then(Value::as_f64);
    let end_tolerance_minutes = report
        .provenance
        .get("end_tolerance_minutes")
        .and_then(Value::as_f64);
    let duration_tolerance_minutes = report
        .provenance
        .get("duration_tolerance_minutes")
        .and_then(Value::as_f64);
    let min_label_confidence = report
        .provenance
        .get("min_label_confidence")
        .and_then(Value::as_f64);

    start_tolerance_minutes.is_some_and(|value| value <= defaults.start_tolerance_minutes)
        && end_tolerance_minutes.is_some_and(|value| value <= defaults.end_tolerance_minutes)
        && duration_tolerance_minutes
            .is_some_and(|value| value <= defaults.duration_tolerance_minutes)
        && min_label_confidence.is_some_and(|value| value >= defaults.min_label_confidence)
}

fn sleep_v1_benchmark_report_ready(report: &AlgorithmComparisonReport) -> bool {
    report.schema == ALGORITHM_COMPARISON_SCHEMA
        && report.generated_by == "goose.algorithm_compare"
        && report.family == "sleep"
        && report.pass
        && benchmark_delta_rows_integrity_pass(report)
        && report.reference_contract_valid
        && report.goose_output_ready
        && report.reference_output_ready
        && report.shared_fields_ready
        && sleep_v1_benchmark_data_coverage_integrity_pass(report)
        && report.goose_algorithm_id == GOOSE_SLEEP_V1_ID
        && report.goose_algorithm_version == GOOSE_SLEEP_V1_VERSION
        && report.reference_algorithm_id == REFERENCE_SLEEP_ACTIGRAPHY_ID
        && report.reference_algorithm_version == REFERENCE_SLEEP_ACTIGRAPHY_VERSION
        && report
            .provenance
            .get("report_integrity_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_V1_BENCHMARK_REPORT_INTEGRITY_POLICY)
        && report
            .provenance
            .get("validation_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_V1_BENCHMARK_COMPARISON_POLICY)
        && report.goose_output.is_some()
        && report.reference_output.is_some()
        && sleep_v1_benchmark_goose_output_integrity_pass(report)
        && sleep_v1_benchmark_delta_output_integrity_pass(report)
        && !report.deltas.is_empty()
        && report.goose_quality_flags.is_empty()
        && report.reference_quality_flags.is_empty()
        && report.quality_flags.is_empty()
        && report.errors.is_empty()
        && report.issues.is_empty()
        && report.acceptance_summary.as_ref()
            == Some(&sleep_v1_benchmark_acceptance_summary(report))
        && report.next_actions
            == algorithm_comparison_next_actions(&report.quality_flags, &report.errors)
}

fn sleep_v1_benchmark_data_coverage_integrity_pass(report: &AlgorithmComparisonReport) -> bool {
    report.data_coverage.as_ref().is_some_and(|coverage| {
        coverage
            .get("goose_output_data_coverage_fraction")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    })
}

fn sleep_v1_benchmark_goose_output_integrity_pass(report: &AlgorithmComparisonReport) -> bool {
    let Some(output) = report.goose_output.as_ref() else {
        return false;
    };
    let Some(output_quality_flags) = output.get("quality_flags").and_then(Value::as_array) else {
        return false;
    };
    let output_quality_flags = output_quality_flags
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .map(|flags| {
            flags
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        });
    let Some(output_quality_flags) = output_quality_flags else {
        return false;
    };
    output.get("algorithm_id").and_then(Value::as_str) == Some(report.goose_algorithm_id.as_str())
        && output.get("algorithm_version").and_then(Value::as_str)
            == Some(report.goose_algorithm_version.as_str())
        && output_quality_flags == report.goose_quality_flags
        && sleep_v1_output_provenance_integrity_pass(output)
        && sleep_v1_benchmark_component_explanations_pass(output)
        && sleep_v1_benchmark_goose_output_contract_pass(output)
        && sleep_v1_status_report_contract_pass(output)
        && sleep_v1_previous_night_comparison_integrity_pass(output)
}

fn sleep_v1_output_provenance_integrity_pass(output: &Value) -> bool {
    let Some(provenance) = output.get("provenance").and_then(Value::as_object) else {
        return false;
    };
    provenance.get("score_policy").and_then(Value::as_str)
        == Some("weighted_sleep_v1_components_with_fragmentation_guardrails")
        && provenance.get("status_policy").and_then(Value::as_str)
            == Some("rust_sleep_model_status_report")
        && provenance
            .get("expected_values_policy")
            .and_then(Value::as_str)
            == Some("hand-derived-tests-and-versioned-goose-output")
        && provenance
            .get("input_ids")
            .and_then(Value::as_array)
            .is_some_and(|input_ids| {
                !input_ids.is_empty()
                    && input_ids.iter().all(|input_id| {
                        input_id
                            .as_str()
                            .is_some_and(|value| !value.trim().is_empty())
                    })
            })
}

fn sleep_v1_benchmark_component_explanations_pass(output: &Value) -> bool {
    let Some(components) = output.get("components").and_then(Value::as_array) else {
        return false;
    };
    let Some(score_0_to_100) = sleep_v1_json_number(output.get("score_0_to_100")) else {
        return false;
    };
    let Some(component_provenance) = output
        .get("component_provenance")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let expected_component_names = sleep_v1_expected_component_names();
    let expected_component_count = expected_component_names.len();
    let mut names = BTreeSet::new();
    let mut contribution_sum = 0.0;
    let mut weight_sum = 0.0;
    let components_ready = components.len() == expected_component_count
        && component_provenance.len() == expected_component_count
        && components.iter().all(|component| {
            let Some(component) = component.as_object() else {
                return false;
            };
            let Some(name) = component.get("name").and_then(Value::as_str) else {
                return false;
            };
            if name.trim().is_empty() {
                return false;
            }
            if !expected_component_names.contains(&name) {
                return false;
            }
            let normalized_name = sleep_v1_normalized_component_name(name);
            if !names.insert(normalized_name.clone()) {
                return false;
            }
            let Some(component_score) = sleep_v1_json_number(component.get("score_0_to_100"))
            else {
                return false;
            };
            let Some(weight) = sleep_v1_json_number(component.get("weight")) else {
                return false;
            };
            let Some(contribution) = sleep_v1_json_number(component.get("contribution")) else {
                return false;
            };
            let component_numbers_ready = sleep_v1_json_finite_number(component.get("value"))
                && (0.0..=100.0).contains(&component_score)
                && (0.0..=1.0).contains(&weight)
                && contribution >= 0.0
                && approx_equal(contribution, component_score * weight);
            let component_unit_ready = component
                .get("unit")
                .and_then(Value::as_str)
                .is_some_and(|unit| !unit.trim().is_empty());
            let provenance_ready = component_provenance
                .get(name)
                .and_then(Value::as_object)
                .is_some_and(|provenance| {
                    provenance
                        .get("inputs")
                        .and_then(Value::as_object)
                        .is_some_and(|inputs| {
                            sleep_v1_component_inputs_ready(&normalized_name, inputs)
                        })
                        && provenance
                            .get("policy")
                            .and_then(Value::as_str)
                            .is_some_and(|policy| {
                                sleep_v1_component_policy_ready(&normalized_name, policy)
                            })
                });
            contribution_sum += contribution;
            weight_sum += weight;
            component_numbers_ready && component_unit_ready && provenance_ready
        });
    components_ready
        && expected_component_names
            .iter()
            .all(|expected_name| names.contains(*expected_name))
        && approx_equal(weight_sum, 1.0)
        && approx_equal(contribution_sum, score_0_to_100)
}

fn sleep_v1_normalized_component_name(component_name: &str) -> String {
    component_name.trim().to_lowercase().replace(' ', "_")
}

fn sleep_v1_expected_component_names() -> [&'static str; 7] {
    [
        "sleep_need_fulfillment",
        "continuity",
        "schedule_regularity",
        "sleep_architecture",
        "cardiovascular_recovery",
        "context_adjustment",
        "data_confidence",
    ]
}

fn sleep_v1_expected_explanation_quality_signals() -> [&'static str; 8] {
    [
        "model_status_label_and_reason",
        "score_visibility_gate",
        "previous_night_comparison",
        "component_breakdown_with_provenance",
        "stage_prior_calibration",
        "cardiovascular_recovery_context",
        "confidence_and_window_quality",
        "why_changed_and_score_policy_provenance",
    ]
}

fn sleep_v1_component_inputs_ready(
    component_name: &str,
    inputs: &serde_json::Map<String, Value>,
) -> bool {
    if inputs.is_empty() {
        return false;
    }
    let Some(required_keys) = sleep_v1_required_component_input_keys(component_name) else {
        return false;
    };
    if !required_keys.iter().all(|key| inputs.contains_key(*key)) {
        return false;
    }
    match component_name {
        "sleep_architecture" => {
            sleep_v1_stage_prior_calibration_ready(inputs.get("stage_prior_calibration"))
        }
        "data_confidence" => {
            sleep_v1_json_bounded_number(inputs.get("sleep_window_confidence_0_to_1"), 0.0, 1.0)
        }
        _ => true,
    }
}

fn sleep_v1_required_component_input_keys(component_name: &str) -> Option<&'static [&'static str]> {
    match component_name {
        "sleep_need_fulfillment" => Some(&[
            "sleep_duration_minutes",
            "sleep_need_minutes",
            "rolling_sleep_debt_minutes",
            "naps_minutes",
        ]),
        "continuity" => Some(&[
            "time_in_bed_minutes",
            "sleep_duration_minutes",
            "sleep_latency_minutes",
            "wake_after_sleep_onset_minutes",
            "wake_episode_count",
        ]),
        "schedule_regularity" => Some(&[
            "bedtime_deviation_minutes",
            "wake_time_deviation_minutes",
            "midpoint_deviation_minutes",
        ]),
        "sleep_architecture" => Some(&[
            "stage_minutes",
            "stage_segment_count",
            "stage_segment_confidence_0_to_1",
            "sleep_architecture_confidence_0_to_1",
            "stage_prior_calibration",
        ]),
        "cardiovascular_recovery" => Some(&[
            "sleep_hr_average_bpm",
            "sleep_hr_min_bpm",
            "pre_sleep_awake_hr_average_bpm",
            "sleep_hr_trend_bpm_per_hour",
            "heart_rate_dip_percent",
        ]),
        "context_adjustment" => Some(&["prior_day_strain", "naps_minutes"]),
        "data_confidence" => Some(&[
            "sleep_window_confidence_0_to_1",
            "data_coverage_fraction",
            "motion_coverage_fraction",
            "heart_rate_coverage_fraction",
            "stage_segment_confidence_0_to_1",
            "sleep_architecture_confidence_0_to_1",
            "timestamp_sync_blocked",
        ]),
        _ => None,
    }
}

fn sleep_v1_required_component_inputs_provenance() -> Value {
    let entries = sleep_v1_expected_component_names()
        .into_iter()
        .map(|component_name| {
            let keys = sleep_v1_required_component_input_keys(component_name)
                .unwrap_or_default()
                .iter()
                .map(|key| Value::String((*key).to_string()))
                .collect::<Vec<_>>();
            (component_name.to_string(), Value::Array(keys))
        })
        .collect::<serde_json::Map<_, _>>();
    Value::Object(entries)
}

fn sleep_v1_required_component_inputs_provenance_ready(provenance: &Value) -> bool {
    let Some(required_inputs) = provenance
        .get("required_component_inputs")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let expected_component_names = sleep_v1_expected_component_names();
    required_inputs.len() == expected_component_names.len()
        && expected_component_names.iter().all(|component_name| {
            let Some(expected_keys) = sleep_v1_required_component_input_keys(component_name) else {
                return false;
            };
            required_inputs
                .get(*component_name)
                .and_then(Value::as_array)
                .is_some_and(|observed_keys| {
                    observed_keys.len() == expected_keys.len()
                        && observed_keys
                            .iter()
                            .zip(expected_keys.iter())
                            .all(|(observed, expected)| observed.as_str() == Some(*expected))
                })
        })
}

fn sleep_v1_stage_prior_calibration_ready(value: Option<&Value>) -> bool {
    let Some(prior) = value.and_then(Value::as_object) else {
        return false;
    };
    let source_ready = prior
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| {
            matches!(
                source,
                "personal_stage_baseline_blended_with_population_prior"
                    | "population_stage_fraction_prior"
            )
        });
    let policy_ready = prior
        .get("policy")
        .and_then(Value::as_str)
        .is_some_and(|policy| {
            matches!(
                policy,
                "blend_personal_stage_priors_by_baseline_maturity_and_confidence"
                    | "use_population_stage_priors_until_personal_baseline_is_available"
            )
        });
    let Some(personal_weight) = sleep_v1_json_number(prior.get("personal_prior_weight")) else {
        return false;
    };
    let Some(population_weight) = sleep_v1_json_number(prior.get("population_prior_weight")) else {
        return false;
    };
    source_ready
        && policy_ready
        && (0.0..=1.0).contains(&personal_weight)
        && (0.0..=1.0).contains(&population_weight)
        && approx_equal(personal_weight + population_weight, 1.0)
}

fn sleep_v1_component_policy_ready(component_name: &str, policy: &str) -> bool {
    let policy = policy.trim();
    if policy.is_empty() {
        return false;
    }
    match component_name {
        "sleep_need_fulfillment" => policy == "duration_vs_need_with_debt_pressure_and_nap_credit",
        "continuity" => policy == "efficiency_latency_waso_and_wake_episode_curve",
        "schedule_regularity" => policy == "weighted_bedtime_wake_time_midpoint_deviation",
        "sleep_architecture" => {
            policy
                == "deep_rem_restorative_balance_vs_personal_baseline_when_available_with_architecture_confidence"
        }
        "cardiovascular_recovery" => {
            policy
                == "hr_dip_pre_sleep_awake_hr_overnight_trend_and_personal_baseline_when_available"
        }
        "context_adjustment" => policy == "strain_and_long_nap_penalty",
        "data_confidence" => {
            policy == "combined_sleep_v1_confidence_window_confidence_and_coverage"
        }
        _ => false,
    }
}

fn sleep_v1_benchmark_goose_output_contract_pass(output: &Value) -> bool {
    let Some(time_in_bed_minutes) = sleep_v1_json_number(output.get("time_in_bed_minutes")) else {
        return false;
    };
    let Some(sleep_duration_minutes) = sleep_v1_json_number(output.get("sleep_duration_minutes"))
    else {
        return false;
    };
    let Some(awake_minutes) = sleep_v1_json_number(output.get("awake_minutes")) else {
        return false;
    };
    let Some(deep_sleep_minutes) = sleep_v1_json_number(output.get("deep_sleep_minutes")) else {
        return false;
    };
    let Some(rem_sleep_minutes) = sleep_v1_json_number(output.get("rem_sleep_minutes")) else {
        return false;
    };
    let Some(core_sleep_minutes) = sleep_v1_json_number(output.get("core_sleep_minutes")) else {
        return false;
    };
    let Some(restorative_sleep_minutes) =
        sleep_v1_json_number(output.get("restorative_sleep_minutes"))
    else {
        return false;
    };

    sleep_v1_json_bounded_number(output.get("score_0_to_100"), 0.0, 100.0)
        && sleep_v1_json_bounded_number(output.get("sleep_window_confidence_0_to_1"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("sleep_performance_fraction"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("sleep_efficiency_fraction"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("restorative_sleep_fraction"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("confidence_0_to_1"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("data_coverage_fraction"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(output.get("stage_segment_confidence_0_to_1"), 0.0, 1.0)
        && sleep_v1_json_bounded_number(
            output.get("sleep_architecture_confidence_0_to_1"),
            0.0,
            1.0,
        )
        && sleep_v1_json_non_negative(output.get("sleep_need_minutes"))
        && sleep_v1_json_non_negative(output.get("sleep_debt_minutes"))
        && sleep_v1_json_non_negative(output.get("rolling_sleep_debt_minutes"))
        && sleep_v1_json_non_negative(output.get("time_in_bed_minutes"))
        && sleep_v1_json_non_negative(output.get("sleep_duration_minutes"))
        && sleep_v1_json_non_negative(output.get("awake_minutes"))
        && sleep_v1_json_non_negative(output.get("sleep_latency_minutes"))
        && sleep_v1_json_non_negative(output.get("wake_after_sleep_onset_minutes"))
        && output
            .get("wake_episode_count")
            .and_then(Value::as_u64)
            .is_some()
        && sleep_v1_json_non_negative(output.get("deep_sleep_minutes"))
        && sleep_v1_json_non_negative(output.get("rem_sleep_minutes"))
        && sleep_v1_json_non_negative(output.get("core_sleep_minutes"))
        && sleep_v1_json_non_negative(output.get("restorative_sleep_minutes"))
        && sleep_duration_minutes <= time_in_bed_minutes + 1.0
        && approx_equal(
            awake_minutes,
            (time_in_bed_minutes - sleep_duration_minutes).max(0.0),
        )
        && approx_equal(
            restorative_sleep_minutes,
            deep_sleep_minutes + rem_sleep_minutes,
        )
        && sleep_v1_stage_minutes_contract_pass(
            output,
            awake_minutes,
            deep_sleep_minutes,
            rem_sleep_minutes,
            core_sleep_minutes,
            time_in_bed_minutes,
        )
}

fn sleep_v1_status_report_contract_pass(output: &Value) -> bool {
    let Some(status_report) = output.get("status_report").and_then(Value::as_object) else {
        return false;
    };
    let Some(model_status) = output.get("model_status").and_then(Value::as_str) else {
        return false;
    };
    let Some(status) = status_report.get("status").and_then(Value::as_str) else {
        return false;
    };
    let Some(status_label) = status_report.get("status_label").and_then(Value::as_str) else {
        return false;
    };
    let Some(status_reason) = status_report.get("status_reason").and_then(Value::as_str) else {
        return false;
    };
    let Some(report_state) = status_report.get("report_state").and_then(Value::as_str) else {
        return false;
    };
    let Some(can_show_provisional_score) = status_report
        .get("can_show_provisional_score")
        .and_then(Value::as_bool)
    else {
        return false;
    };
    let Some(can_show_final_score) = status_report
        .get("can_show_final_score")
        .and_then(Value::as_bool)
    else {
        return false;
    };
    let Some(can_show_personal_baseline) = status_report
        .get("can_show_personal_baseline")
        .and_then(Value::as_bool)
    else {
        return false;
    };
    let Some(can_show_trained_score) = status_report
        .get("can_show_trained_score")
        .and_then(Value::as_bool)
    else {
        return false;
    };

    let Some(valid_sleep_nights) = status_report
        .get("valid_sleep_nights")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(trusted_goose_sleep_nights) = status_report
        .get("trusted_goose_sleep_nights")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(imported_platform_sleep_nights) = status_report
        .get("imported_platform_sleep_nights")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(_excluded_sleep_nights) = status_report
        .get("excluded_sleep_nights")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(calibration_label_count) = status_report
        .get("calibration_label_count")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(nights_until_baseline) = status_report
        .get("nights_until_baseline")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(nights_until_training) = status_report
        .get("nights_until_training")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(nights_until_goose_training) = status_report
        .get("nights_until_goose_training")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let status_counts_within_rust_contract = [
        valid_sleep_nights,
        trusted_goose_sleep_nights,
        imported_platform_sleep_nights,
        calibration_label_count,
        nights_until_baseline,
        nights_until_training,
        nights_until_goose_training,
    ]
    .into_iter()
    .all(|count| count <= u32::MAX as u64);
    let counts_coherent = valid_sleep_nights
        == trusted_goose_sleep_nights.saturating_add(imported_platform_sleep_nights)
        && nights_until_baseline == 7u64.saturating_sub(valid_sleep_nights);
    let training_progress_coherent = nights_until_training <= 14
        && nights_until_training == 14u64.saturating_sub(calibration_label_count)
        && nights_until_goose_training <= 7
        && nights_until_goose_training == 7u64.saturating_sub(trusted_goose_sleep_nights)
        && (!matches!(status, "training" | "trained")
            || (nights_until_training == 0 && nights_until_goose_training == 0));
    let status_quality_flags_clean = status_report
        .get("quality_flags")
        .and_then(Value::as_array)
        .is_some_and(|flags| flags.is_empty());
    let status_next_actions_ready = status_report
        .get("next_actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| {
            let action_required = matches!(
                status,
                "setup_needed"
                    | "importing_history"
                    | "learning"
                    | "training"
                    | "needs_relearn"
                    | "blocked"
            ) || (status == "baseline_ready" && !can_show_trained_score)
                || matches!(report_state, "pending" | "provisional" | "blocked");
            (!action_required || !actions.is_empty())
                && actions.iter().all(|action| {
                    if let Some(action) = action.as_str() {
                        return !action.trim().is_empty();
                    }
                    action.as_object().is_some_and(|action| {
                        ["scope", "reason", "action"].iter().all(|field| {
                            action
                                .get(*field)
                                .and_then(Value::as_str)
                                .is_some_and(|value| !value.trim().is_empty())
                        })
                    })
                })
        });
    let top_level_status_ready = output.get("model_status_label").and_then(Value::as_str)
        == Some(status_label)
        && output.get("model_status_reason").and_then(Value::as_str) == Some(status_reason);
    let expected_can_show_provisional_score = valid_sleep_nights > 0 && status != "blocked";
    let expected_report_state = if status == "blocked" {
        "blocked"
    } else if can_show_final_score {
        "final"
    } else if expected_can_show_provisional_score {
        "provisional"
    } else {
        "pending"
    };

    status == model_status
        && matches!(
            status,
            "setup_needed"
                | "importing_history"
                | "learning"
                | "baseline_ready"
                | "training"
                | "trained"
                | "needs_relearn"
                | "blocked"
        )
        && !status_label.trim().is_empty()
        && !status_reason.trim().is_empty()
        && matches!(
            report_state,
            "blocked" | "final" | "provisional" | "pending"
        )
        && status_counts_within_rust_contract
        && counts_coherent
        && training_progress_coherent
        && status_quality_flags_clean
        && status_next_actions_ready
        && top_level_status_ready
        && can_show_provisional_score == expected_can_show_provisional_score
        && report_state == expected_report_state
        && (valid_sleep_nights > 0 || (!can_show_provisional_score && !can_show_final_score))
        && (!can_show_final_score || trusted_goose_sleep_nights > 0)
        && (!can_show_final_score || (report_state == "final" && can_show_provisional_score))
        && (can_show_final_score || report_state != "final")
        && (can_show_trained_score == (status == "trained" && can_show_final_score))
        && (can_show_personal_baseline
            == matches!(status, "baseline_ready" | "training" | "trained"))
        && (status != "blocked"
            || (report_state == "blocked" && !can_show_provisional_score && !can_show_final_score))
}

fn sleep_v1_stage_minutes_contract_pass(
    output: &Value,
    awake_minutes: f64,
    deep_sleep_minutes: f64,
    rem_sleep_minutes: f64,
    core_sleep_minutes: f64,
    time_in_bed_minutes: f64,
) -> bool {
    let Some(stage_minutes) = output.get("stage_minutes").and_then(Value::as_object) else {
        return false;
    };
    if stage_minutes
        .keys()
        .any(|stage| !matches!(stage.as_str(), "awake" | "core" | "deep" | "rem"))
    {
        return false;
    }
    let mut total_stage_minutes = 0.0;
    for value in stage_minutes.values() {
        let Some(minutes) = sleep_v1_json_number(Some(value)) else {
            return false;
        };
        if minutes < 0.0 {
            return false;
        }
        total_stage_minutes += minutes;
    }
    sleep_v1_stage_minutes_value_matches(stage_minutes, "awake", awake_minutes)
        && sleep_v1_stage_minutes_value_matches(stage_minutes, "deep", deep_sleep_minutes)
        && sleep_v1_stage_minutes_value_matches(stage_minutes, "rem", rem_sleep_minutes)
        && sleep_v1_stage_minutes_value_matches(stage_minutes, "core", core_sleep_minutes)
        && total_stage_minutes <= time_in_bed_minutes + 1.0
}

fn sleep_v1_stage_minutes_value_matches(
    stage_minutes: &serde_json::Map<String, Value>,
    stage: &str,
    expected_minutes: f64,
) -> bool {
    match stage_minutes.get(stage) {
        Some(value) => value
            .as_f64()
            .is_some_and(|minutes| minutes.is_finite() && approx_equal(minutes, expected_minutes)),
        None => approx_equal(expected_minutes, 0.0),
    }
}

fn sleep_v1_previous_night_comparison_integrity_pass(output: &Value) -> bool {
    let Some(comparison) = output.get("previous_night_comparison") else {
        return false;
    };
    if comparison.is_null() {
        return false;
    }
    let Some(object) = comparison.as_object() else {
        return false;
    };
    let required_fields = [
        "night_id",
        "sleep_duration_delta_minutes",
        "awake_minutes_delta",
        "sleep_debt_delta_minutes",
        "sleep_efficiency_delta_fraction",
        "sleep_latency_delta_minutes",
        "wake_after_sleep_onset_delta_minutes",
        "wake_episode_count_delta",
        "deep_sleep_delta_minutes",
        "rem_sleep_delta_minutes",
        "core_sleep_delta_minutes",
        "restorative_sleep_delta_minutes",
        "bedtime_deviation_delta_minutes",
        "wake_time_deviation_delta_minutes",
        "sleep_hr_average_delta_bpm",
        "sleep_hr_min_delta_bpm",
        "sleep_hr_trend_delta_bpm_per_hour",
        "sleep_hr_dip_delta_percent",
    ];
    if object.len() != required_fields.len()
        || required_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return false;
    }
    object
        .get("night_id")
        .and_then(Value::as_str)
        .is_some_and(|night_id| !night_id.trim().is_empty())
        && sleep_v1_previous_night_comparison_provenance_integrity_pass(output, object)
        && sleep_v1_json_finite_number(object.get("sleep_duration_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("awake_minutes_delta"))
        && sleep_v1_json_finite_number(object.get("sleep_debt_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("sleep_efficiency_delta_fraction"))
        && sleep_v1_json_finite_number(object.get("sleep_latency_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("wake_after_sleep_onset_delta_minutes"))
        && sleep_v1_json_integer(object.get("wake_episode_count_delta"))
        && sleep_v1_json_finite_number(object.get("deep_sleep_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("rem_sleep_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("core_sleep_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("restorative_sleep_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("bedtime_deviation_delta_minutes"))
        && sleep_v1_json_finite_number(object.get("wake_time_deviation_delta_minutes"))
        && sleep_v1_json_optional_finite_number(object.get("sleep_hr_average_delta_bpm"))
        && sleep_v1_json_optional_finite_number(object.get("sleep_hr_min_delta_bpm"))
        && sleep_v1_json_optional_finite_number(object.get("sleep_hr_trend_delta_bpm_per_hour"))
        && sleep_v1_json_optional_finite_number(object.get("sleep_hr_dip_delta_percent"))
}

fn sleep_v1_previous_night_comparison_provenance_integrity_pass(
    output: &Value,
    comparison: &serde_json::Map<String, Value>,
) -> bool {
    let Some(night_id) = comparison.get("night_id").and_then(Value::as_str) else {
        return false;
    };
    let Some(provenance) = output.get("provenance").and_then(Value::as_object) else {
        return false;
    };
    let Some(previous_provenance) = provenance
        .get("previous_night_comparison")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let expected_fields = [
        "sleep_duration_delta_minutes",
        "awake_minutes_delta",
        "sleep_debt_delta_minutes",
        "sleep_efficiency_delta_fraction",
        "sleep_latency_delta_minutes",
        "wake_after_sleep_onset_delta_minutes",
        "wake_episode_count_delta",
        "deep_sleep_delta_minutes",
        "rem_sleep_delta_minutes",
        "core_sleep_delta_minutes",
        "restorative_sleep_delta_minutes",
        "bedtime_deviation_delta_minutes",
        "wake_time_deviation_delta_minutes",
        "sleep_hr_average_delta_bpm",
        "sleep_hr_min_delta_bpm",
        "sleep_hr_trend_delta_bpm_per_hour",
        "sleep_hr_dip_delta_percent",
    ];
    previous_provenance.get("policy").and_then(Value::as_str)
        == Some("latest_usable_prior_night_before_scored_sleep")
        && previous_provenance
            .get("selected_night_id")
            .and_then(Value::as_str)
            == Some(night_id)
        && previous_provenance
            .get("usable_prior_night_count")
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0)
        && previous_provenance
            .get("fields")
            .and_then(Value::as_array)
            .is_some_and(|fields| {
                fields.len() == expected_fields.len()
                    && fields
                        .iter()
                        .zip(expected_fields.iter())
                        .all(|(observed, expected)| observed.as_str() == Some(*expected))
            })
}

fn sleep_v1_json_finite_number(value: Option<&Value>) -> bool {
    value.and_then(Value::as_f64).is_some_and(f64::is_finite)
}

fn sleep_v1_json_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn sleep_v1_json_bounded_number(value: Option<&Value>, min: f64, max: f64) -> bool {
    sleep_v1_json_number(value).is_some_and(|value| (min..=max).contains(&value))
}

fn sleep_v1_json_non_negative(value: Option<&Value>) -> bool {
    sleep_v1_json_number(value).is_some_and(|value| value >= 0.0)
}

fn sleep_v1_json_integer(value: Option<&Value>) -> bool {
    value.and_then(Value::as_i64).is_some()
}

fn sleep_v1_json_optional_finite_number(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.is_null() || value.as_f64().is_some_and(f64::is_finite))
}

fn sleep_v1_benchmark_delta_output_integrity_pass(report: &AlgorithmComparisonReport) -> bool {
    let (Some(goose_output), Some(reference_output)) = (
        report.goose_output.as_ref(),
        report.reference_output.as_ref(),
    ) else {
        return false;
    };
    report
        .deltas
        .iter()
        .all(|delta| match delta.field.as_str() {
            "time_in_bed_minutes" => sleep_v1_benchmark_delta_values_match(
                delta,
                goose_output,
                "time_in_bed_minutes",
                reference_output,
                "time_in_bed_minutes",
            ),
            "sleep_minutes" => sleep_v1_benchmark_delta_values_match(
                delta,
                goose_output,
                "sleep_duration_minutes",
                reference_output,
                "sleep_minutes",
            ),
            "wake_minutes" => sleep_v1_benchmark_delta_values_match(
                delta,
                goose_output,
                "awake_minutes",
                reference_output,
                "wake_minutes",
            ),
            "sleep_efficiency_fraction" => sleep_v1_benchmark_delta_values_match(
                delta,
                goose_output,
                "sleep_efficiency_fraction",
                reference_output,
                "sleep_efficiency_fraction",
            ),
            "wake_after_sleep_onset_minutes" => sleep_v1_benchmark_delta_values_match(
                delta,
                goose_output,
                "wake_after_sleep_onset_minutes",
                reference_output,
                "wake_after_sleep_onset_minutes",
            ),
            "disturbance_count" => sleep_v1_benchmark_delta_goose_input_values_match(
                delta,
                &report.provenance,
                "disturbance_count",
                reference_output,
                "disturbance_count",
            ),
            "fragmentation_index_per_hour" => sleep_v1_benchmark_delta_goose_input_values_match(
                delta,
                &report.provenance,
                "fragmentation_index_per_hour",
                reference_output,
                "fragmentation_index_per_hour",
            ),
            _ => false,
        })
}

fn sleep_v1_benchmark_delta_values_match(
    delta: &AlgorithmComparisonDelta,
    goose_output: &Value,
    goose_key: &str,
    reference_output: &Value,
    reference_key: &str,
) -> bool {
    goose_output
        .get(goose_key)
        .and_then(Value::as_f64)
        .is_some_and(|value| approx_equal(value, delta.goose_value))
        && reference_output
            .get(reference_key)
            .and_then(Value::as_f64)
            .is_some_and(|value| approx_equal(value, delta.reference_value))
}

fn sleep_v1_benchmark_delta_goose_input_values_match(
    delta: &AlgorithmComparisonDelta,
    provenance: &Value,
    goose_key: &str,
    reference_output: &Value,
    reference_key: &str,
) -> bool {
    provenance
        .get("goose_comparable_inputs")
        .and_then(Value::as_object)
        .and_then(|inputs| inputs.get(goose_key))
        .and_then(Value::as_f64)
        .is_some_and(|value| approx_equal(value, delta.goose_value))
        && reference_output
            .get(reference_key)
            .and_then(Value::as_f64)
            .is_some_and(|value| approx_equal(value, delta.reference_value))
}

fn benchmark_delta_rows_integrity_pass(report: &AlgorithmComparisonReport) -> bool {
    let delta_fields = report
        .deltas
        .iter()
        .map(|delta| delta.field.clone())
        .collect::<Vec<_>>();
    report.comparable_fields == delta_fields
        && report.deltas.iter().all(|delta| {
            delta.goose_value.is_finite()
                && delta.reference_value.is_finite()
                && delta.absolute_delta.is_finite()
                && optional_finite(delta.relative_delta_fraction)
                && approx_equal(
                    delta.absolute_delta,
                    delta.goose_value - delta.reference_value,
                )
                && expected_relative_delta_matches(
                    delta.relative_delta_fraction,
                    delta.goose_value,
                    delta.reference_value,
                )
        })
}

fn optional_finite(value: Option<f64>) -> bool {
    value.is_none_or(f64::is_finite)
}

fn expected_relative_delta_matches(
    observed: Option<f64>,
    goose_value: f64,
    reference_value: f64,
) -> bool {
    if reference_value.abs() < f64::EPSILON {
        observed.is_none()
    } else {
        observed.is_some_and(|value| {
            approx_equal(
                value,
                (goose_value - reference_value) / reference_value.abs(),
            )
        })
    }
}

fn sleep_v1_explanation_stability_report_integrity_pass(
    report: &SleepV1ExplanationStabilityReport,
) -> bool {
    let Some(provenance_v0_component_count) = report
        .provenance
        .get("v0_component_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
    else {
        return false;
    };
    let Some(provenance_v1_component_count) = report
        .provenance
        .get("v1_component_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
    else {
        return false;
    };
    let Some(max_repeated_run_delta) = report
        .provenance
        .get("max_repeated_run_delta")
        .and_then(Value::as_f64)
    else {
        return false;
    };
    let Some(max_small_perturbation_delta) = report
        .provenance
        .get("max_small_perturbation_delta")
        .and_then(Value::as_f64)
    else {
        return false;
    };
    let Some(min_v1_component_count) = report
        .provenance
        .get("min_v1_component_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
    else {
        return false;
    };
    let Some(min_explanation_quality_signal_count) = report
        .provenance
        .get("min_explanation_quality_signal_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
    else {
        return false;
    };
    let provenance_thresholds_valid = max_repeated_run_delta.is_finite()
        && max_repeated_run_delta >= 0.0
        && max_small_perturbation_delta.is_finite()
        && max_small_perturbation_delta >= 0.0
        && report
            .provenance
            .get("report_integrity_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_V1_EXPLANATION_STABILITY_INTEGRITY_POLICY)
        && report
            .provenance
            .get("validation_policy")
            .and_then(Value::as_str)
            == Some(SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY)
        && report
            .provenance
            .get("perturb_sleep_duration_minutes")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        && report
            .provenance
            .get("perturbed_score_0_to_100")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && (0.0..=100.0).contains(&value))
        && sleep_v1_required_component_inputs_provenance_ready(&report.provenance);
    let mut unique_component_names = BTreeSet::new();
    let component_names_unique = report
        .v1_component_names
        .iter()
        .all(|name| unique_component_names.insert(name.clone()));
    let expected_component_names = sleep_v1_expected_component_names();
    let expected_component_name_set = expected_component_names
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let observed_component_name_set = report
        .v1_component_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let provenance_component_names_match = report
        .provenance
        .get("v1_component_names")
        .and_then(Value::as_array)
        .is_some_and(|names| {
            names.len() == report.v1_component_names.len()
                && names
                    .iter()
                    .zip(report.v1_component_names.iter())
                    .all(|(observed, expected)| observed.as_str() == Some(expected.as_str()))
        });
    let component_order_matches = report
        .v1_component_names
        .iter()
        .map(String::as_str)
        .eq(expected_component_names.iter().copied());
    let component_contract_ready = report.v1_component_count == expected_component_names.len()
        && observed_component_name_set == expected_component_name_set
        && component_order_matches;
    let explanation_quality_signals_match = report
        .provenance
        .get("explanation_quality_signals")
        .and_then(Value::as_array)
        .is_some_and(|signals| {
            signals.len() == report.explanation_quality_signals.len()
                && signals
                    .iter()
                    .zip(report.explanation_quality_signals.iter())
                    .all(|(observed, expected)| observed.as_str() == Some(expected.as_str()))
        });
    let expected_explanation_quality_signals = sleep_v1_expected_explanation_quality_signals();
    let explanation_quality_signal_contract_ready = report.explanation_quality_signals.len()
        == expected_explanation_quality_signals.len()
        && report
            .explanation_quality_signals
            .iter()
            .zip(expected_explanation_quality_signals.iter())
            .all(|(observed, expected)| observed == expected);
    let explanation_quality_signal_names_valid = !report.explanation_quality_signals.is_empty()
        && report
            .explanation_quality_signals
            .iter()
            .all(|signal| !signal.trim().is_empty())
        && report
            .explanation_quality_signals
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            == report.explanation_quality_signals.len();

    let explanation_pass = report.v1_component_count >= report.v0_component_count
        && report.v1_component_count >= min_v1_component_count
        && report.v1_component_names.len() == report.v1_component_count
        && component_names_unique
        && component_contract_ready
        && report
            .v1_component_names
            .iter()
            .all(|name| !name.trim().is_empty())
        && provenance_thresholds_valid
        && report.missing_component_provenance.is_empty()
        && report.missing_component_inputs.is_empty()
        && report.missing_component_policy.is_empty()
        && sleep_v1_stability_confidence_valid(report.sleep_window_confidence_0_to_1)
        && sleep_v1_stability_confidence_valid(report.perturbed_sleep_window_confidence_0_to_1)
        && report.quality_flags.is_empty();
    let explanation_quality_pass = report.explanation_quality_signal_count
        == report.explanation_quality_signals.len()
        && report.explanation_quality_signal_count >= min_explanation_quality_signal_count
        && min_explanation_quality_signal_count >= expected_explanation_quality_signals.len()
        && explanation_quality_signal_names_valid
        && explanation_quality_signal_contract_ready
        && explanation_quality_signals_match;
    let repeated_run_stability_pass = report
        .repeated_run_delta
        .is_some_and(|delta| delta.is_finite() && delta >= 0.0 && delta <= max_repeated_run_delta);
    let perturbation_stability_pass = report.small_perturbation_delta.is_some_and(|delta| {
        delta.is_finite() && delta >= 0.0 && delta <= max_small_perturbation_delta
    });
    let acceptance_summary = sleep_v1_explanation_stability_acceptance_summary(
        report.explanation_pass,
        report.explanation_quality_pass,
        report.repeated_run_stability_pass,
        report.perturbation_stability_pass,
        report.v0_component_count,
        report.v1_component_count,
        &report.v1_component_names,
        report.explanation_quality_signal_count,
        &report.explanation_quality_signals,
        report.sleep_window_confidence_0_to_1,
        report.perturbed_sleep_window_confidence_0_to_1,
        report.repeated_run_delta,
        report.small_perturbation_delta,
        &SleepV1ExplanationStabilityOptions {
            max_repeated_run_delta,
            max_small_perturbation_delta,
            perturb_sleep_duration_minutes: report
                .provenance
                .get("perturb_sleep_duration_minutes")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            min_v1_component_count,
            min_explanation_quality_signal_count,
        },
        report.issues.len(),
        report.quality_flags.len(),
        report.errors.len(),
        report.next_actions.len(),
    );
    let pass = report.explanation_pass
        && report.explanation_quality_pass
        && report.repeated_run_stability_pass
        && report.perturbation_stability_pass
        && report.issues.is_empty();

    report.schema == SLEEP_V1_EXPLANATION_STABILITY_SCHEMA
        && report.generated_by == "goose-sleep-v1-explanation-stability-validator"
        && provenance_v0_component_count == report.v0_component_count
        && provenance_v1_component_count == report.v1_component_count
        && provenance_component_names_match
        && provenance_optional_f64_matches(
            &report.provenance,
            "sleep_window_confidence_0_to_1",
            report.sleep_window_confidence_0_to_1,
        )
        && provenance_optional_f64_matches(
            &report.provenance,
            "perturbed_sleep_window_confidence_0_to_1",
            report.perturbed_sleep_window_confidence_0_to_1,
        )
        && report.next_actions == explanation_stability_next_actions(&report.issues)
        && report.errors.is_empty()
        && report.explanation_pass == explanation_pass
        && report.explanation_quality_pass == explanation_quality_pass
        && report.repeated_run_stability_pass == repeated_run_stability_pass
        && report.perturbation_stability_pass == perturbation_stability_pass
        && report.acceptance_summary == acceptance_summary
        && report.pass == pass
}

fn provenance_optional_f64_matches(provenance: &Value, key: &str, expected: Option<f64>) -> bool {
    match expected {
        Some(expected) => provenance
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(|observed| approx_equal(observed, expected)),
        None => provenance.get(key).is_some_and(Value::is_null),
    }
}

fn sleep_v1_stability_confidence_valid(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

fn sleep_v1_explanation_stability_thresholds_at_least_default(
    report: &SleepV1ExplanationStabilityReport,
) -> bool {
    let defaults = SleepV1ExplanationStabilityOptions::default();
    let max_repeated_run_delta = report
        .provenance
        .get("max_repeated_run_delta")
        .and_then(Value::as_f64);
    let max_small_perturbation_delta = report
        .provenance
        .get("max_small_perturbation_delta")
        .and_then(Value::as_f64);
    let perturb_sleep_duration_minutes = report
        .provenance
        .get("perturb_sleep_duration_minutes")
        .and_then(Value::as_f64);
    let min_v1_component_count = report
        .provenance
        .get("min_v1_component_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let min_explanation_quality_signal_count = report
        .provenance
        .get("min_explanation_quality_signal_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize);

    max_repeated_run_delta.is_some_and(|value| value <= defaults.max_repeated_run_delta)
        && max_small_perturbation_delta
            .is_some_and(|value| value <= defaults.max_small_perturbation_delta)
        && perturb_sleep_duration_minutes
            .is_some_and(|value| value >= defaults.perturb_sleep_duration_minutes)
        && min_v1_component_count.is_some_and(|value| value >= defaults.min_v1_component_count)
        && min_explanation_quality_signal_count
            .is_some_and(|value| value >= defaults.min_explanation_quality_signal_count)
}

pub fn validate_sleep_v1_explanation_and_stability(
    input: &SleepV1Input,
    options: SleepV1ExplanationStabilityOptions,
) -> SleepV1ExplanationStabilityReport {
    let first_result = goose_sleep_v1(input);
    let second_result = goose_sleep_v1(input);
    let v0_result = goose_sleep_v0(&input.sleep);
    let mut perturbed = input.clone();
    perturbed.sleep.sleep_duration_minutes = (perturbed.sleep.sleep_duration_minutes
        + options.perturb_sleep_duration_minutes)
        .min(perturbed.sleep.time_in_bed_minutes);
    let perturbed_result = goose_sleep_v1(&perturbed);

    let mut issues = Vec::new();
    let mut quality_flags = Vec::new();
    quality_flags.extend(first_result.quality_flags.clone());
    quality_flags.sort();
    quality_flags.dedup();

    let Some(first_output) = first_result.output.as_ref() else {
        issues.push("sleep_v1_output_missing".to_string());
        return explanation_stability_report(
            None,
            None,
            None,
            v0_result
                .output
                .as_ref()
                .map_or(0, |output| output.components.len()),
            None,
            quality_flags,
            issues,
            &options,
        );
    };
    let Some(second_output) = second_result.output.as_ref() else {
        issues.push("sleep_v1_repeated_output_missing".to_string());
        return explanation_stability_report(
            Some(first_output),
            None,
            None,
            v0_result
                .output
                .as_ref()
                .map_or(0, |output| output.components.len()),
            None,
            quality_flags,
            issues,
            &options,
        );
    };

    let repeated_run_delta = (first_output.score_0_to_100 - second_output.score_0_to_100).abs();
    if repeated_run_delta > options.max_repeated_run_delta {
        issues.push("sleep_v1_repeated_run_delta_exceeds_threshold".to_string());
    }

    let small_perturbation_delta = perturbed_result
        .output
        .as_ref()
        .map(|output| (first_output.score_0_to_100 - output.score_0_to_100).abs());
    match small_perturbation_delta {
        Some(delta) if delta > options.max_small_perturbation_delta => {
            issues.push("sleep_v1_small_perturbation_delta_exceeds_threshold".to_string());
        }
        Some(_) => {}
        None => issues.push("sleep_v1_perturbed_output_missing".to_string()),
    }

    explanation_stability_report(
        Some(first_output),
        Some(repeated_run_delta),
        small_perturbation_delta,
        v0_result
            .output
            .as_ref()
            .map_or(0, |output| output.components.len()),
        perturbed_result.output.as_ref(),
        quality_flags,
        issues,
        &options,
    )
}

fn explanation_stability_report(
    output: Option<&SleepV1Output>,
    repeated_run_delta: Option<f64>,
    small_perturbation_delta: Option<f64>,
    v0_component_count: usize,
    perturbed_output: Option<&SleepV1Output>,
    quality_flags: Vec<String>,
    mut issues: Vec<String>,
    options: &SleepV1ExplanationStabilityOptions,
) -> SleepV1ExplanationStabilityReport {
    let mut missing_component_provenance = Vec::new();
    let mut missing_component_inputs = Vec::new();
    let mut missing_component_policy = Vec::new();
    let mut v1_component_names = Vec::new();
    let explanation_quality_signals = output
        .map(sleep_v1_explanation_quality_signals)
        .unwrap_or_default();
    let explanation_quality_signal_count = explanation_quality_signals.len();
    let expected_component_names = sleep_v1_expected_component_names();
    let expected_component_name_set = expected_component_names
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let v1_component_count = output.map_or(0, |output| {
        v1_component_names = output
            .components
            .iter()
            .map(|component| component.name.clone())
            .collect();
        for component in &output.components {
            let Some(provenance) = output.component_provenance.get(&component.name) else {
                missing_component_provenance.push(component.name.clone());
                continue;
            };
            let normalized_name = sleep_v1_normalized_component_name(&component.name);
            if provenance
                .get("inputs")
                .and_then(Value::as_object)
                .is_none_or(|inputs| !sleep_v1_component_inputs_ready(&normalized_name, inputs))
            {
                missing_component_inputs.push(component.name.clone());
            }
            if provenance
                .get("policy")
                .and_then(Value::as_str)
                .is_none_or(|policy| !sleep_v1_component_policy_ready(&normalized_name, policy))
            {
                missing_component_policy.push(component.name.clone());
            }
        }
        output.components.len()
    });

    if v1_component_count < options.min_v1_component_count {
        issues.push("sleep_v1_component_count_below_quality_gate".to_string());
    }
    if v1_component_count < v0_component_count {
        issues.push("sleep_v1_has_fewer_explanation_components_than_v0".to_string());
    }
    if v1_component_names.iter().collect::<BTreeSet<_>>().len() != v1_component_names.len() {
        issues.push("sleep_v1_duplicate_explanation_components".to_string());
    }
    if v1_component_count != expected_component_names.len()
        || v1_component_names
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != expected_component_name_set
    {
        issues.push("sleep_v1_component_contract_mismatch".to_string());
    }
    if !missing_component_provenance.is_empty() {
        issues.push("sleep_v1_component_provenance_missing".to_string());
    }
    if !missing_component_inputs.is_empty() {
        issues.push("sleep_v1_component_inputs_missing".to_string());
    }
    if !missing_component_policy.is_empty() {
        issues.push("sleep_v1_component_policy_missing".to_string());
    }
    if output.is_some_and(|output| output.status_report.status_label.is_empty()) {
        issues.push("sleep_v1_status_label_missing".to_string());
    }
    if output.is_some_and(|output| output.status_report.status_reason.is_empty()) {
        issues.push("sleep_v1_status_reason_missing".to_string());
    }
    if explanation_quality_signal_count < options.min_explanation_quality_signal_count {
        issues.push("sleep_v1_explanation_quality_signal_count_below_gate".to_string());
    }
    if output.is_some()
        && explanation_quality_signals.iter().map(String::as_str).ne(
            sleep_v1_expected_explanation_quality_signals()
                .iter()
                .copied(),
        )
    {
        issues.push("sleep_v1_explanation_quality_signal_contract_mismatch".to_string());
    }
    let sleep_window_confidence_0_to_1 = output.map(|output| output.sleep_window_confidence_0_to_1);
    let perturbed_sleep_window_confidence_0_to_1 =
        perturbed_output.map(|output| output.sleep_window_confidence_0_to_1);
    if output.is_some() && !sleep_v1_stability_confidence_valid(sleep_window_confidence_0_to_1) {
        issues.push("sleep_v1_sleep_window_confidence_missing_or_invalid".to_string());
    }
    if perturbed_output.is_some()
        && !sleep_v1_stability_confidence_valid(perturbed_sleep_window_confidence_0_to_1)
    {
        issues.push("sleep_v1_perturbed_sleep_window_confidence_missing_or_invalid".to_string());
    }
    if !quality_flags.is_empty() {
        issues.push("sleep_v1_quality_flags_present".to_string());
    }

    issues.sort();
    issues.dedup();
    let explanation_pass = output.is_some()
        && v1_component_count >= options.min_v1_component_count
        && v1_component_count >= v0_component_count
        && missing_component_provenance.is_empty()
        && missing_component_inputs.is_empty()
        && missing_component_policy.is_empty()
        && !issues.iter().any(|issue| {
            matches!(
                issue.as_str(),
                "sleep_v1_component_count_below_quality_gate"
                    | "sleep_v1_has_fewer_explanation_components_than_v0"
                    | "sleep_v1_duplicate_explanation_components"
                    | "sleep_v1_component_contract_mismatch"
                    | "sleep_v1_component_provenance_missing"
                    | "sleep_v1_component_inputs_missing"
                    | "sleep_v1_component_policy_missing"
                    | "sleep_v1_status_label_missing"
                    | "sleep_v1_status_reason_missing"
                    | "sleep_v1_sleep_window_confidence_missing_or_invalid"
                    | "sleep_v1_perturbed_sleep_window_confidence_missing_or_invalid"
                    | "sleep_v1_quality_flags_present"
            )
        });
    let repeated_run_stability_pass =
        repeated_run_delta.is_some_and(|delta| delta <= options.max_repeated_run_delta);
    let perturbation_stability_pass =
        small_perturbation_delta.is_some_and(|delta| delta <= options.max_small_perturbation_delta);
    let explanation_quality_pass = explanation_quality_signal_count
        >= options.min_explanation_quality_signal_count
        && !explanation_quality_signals.is_empty()
        && !issues.iter().any(|issue| {
            issue == "sleep_v1_explanation_quality_signal_count_below_gate"
                || issue == "sleep_v1_explanation_quality_signal_contract_mismatch"
        });
    let pass = explanation_pass
        && explanation_quality_pass
        && repeated_run_stability_pass
        && perturbation_stability_pass
        && issues.is_empty();
    let next_actions = explanation_stability_next_actions(&issues);
    let provenance = json!({
        "comparison_policy": SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY,
        "validation_policy": SLEEP_V1_EXPLANATION_STABILITY_VALIDATION_POLICY,
        "report_integrity_policy": SLEEP_V1_EXPLANATION_STABILITY_INTEGRITY_POLICY,
        "v0_component_count": v0_component_count,
        "v1_component_count": v1_component_count,
        "v1_component_names": v1_component_names.clone(),
        "min_v1_component_count": options.min_v1_component_count,
        "min_explanation_quality_signal_count": options.min_explanation_quality_signal_count,
        "explanation_quality_signals": explanation_quality_signals.clone(),
        "required_component_inputs": sleep_v1_required_component_inputs_provenance(),
        "max_repeated_run_delta": options.max_repeated_run_delta,
        "max_small_perturbation_delta": options.max_small_perturbation_delta,
        "perturb_sleep_duration_minutes": options.perturb_sleep_duration_minutes,
        "sleep_window_confidence_0_to_1": sleep_window_confidence_0_to_1,
        "perturbed_score_0_to_100": perturbed_output.map(|output| output.score_0_to_100),
        "perturbed_sleep_window_confidence_0_to_1": perturbed_sleep_window_confidence_0_to_1,
    });

    SleepV1ExplanationStabilityReport {
        schema: SLEEP_V1_EXPLANATION_STABILITY_SCHEMA.to_string(),
        generated_by: "goose-sleep-v1-explanation-stability-validator".to_string(),
        pass,
        explanation_pass,
        explanation_quality_pass,
        repeated_run_stability_pass,
        perturbation_stability_pass,
        v0_component_count,
        v1_component_count,
        v1_component_names: v1_component_names.clone(),
        explanation_quality_signal_count,
        explanation_quality_signals: explanation_quality_signals.clone(),
        missing_component_provenance,
        missing_component_inputs,
        missing_component_policy,
        sleep_window_confidence_0_to_1,
        perturbed_sleep_window_confidence_0_to_1,
        repeated_run_delta,
        small_perturbation_delta,
        acceptance_summary: sleep_v1_explanation_stability_acceptance_summary(
            explanation_pass,
            explanation_quality_pass,
            repeated_run_stability_pass,
            perturbation_stability_pass,
            v0_component_count,
            v1_component_count,
            &v1_component_names,
            explanation_quality_signal_count,
            &explanation_quality_signals,
            sleep_window_confidence_0_to_1,
            perturbed_sleep_window_confidence_0_to_1,
            repeated_run_delta,
            small_perturbation_delta,
            options,
            issues.len(),
            quality_flags.len(),
            0,
            next_actions.len(),
        ),
        quality_flags,
        errors: Vec::new(),
        issues,
        next_actions,
        provenance,
    }
}

fn sleep_v1_explanation_stability_acceptance_summary(
    explanation_pass: bool,
    explanation_quality_pass: bool,
    repeated_run_stability_pass: bool,
    perturbation_stability_pass: bool,
    v0_component_count: usize,
    v1_component_count: usize,
    v1_component_names: &[String],
    explanation_quality_signal_count: usize,
    explanation_quality_signals: &[String],
    sleep_window_confidence_0_to_1: Option<f64>,
    perturbed_sleep_window_confidence_0_to_1: Option<f64>,
    repeated_run_delta: Option<f64>,
    small_perturbation_delta: Option<f64>,
    options: &SleepV1ExplanationStabilityOptions,
    issue_count: usize,
    quality_flag_count: usize,
    error_count: usize,
    next_action_count: usize,
) -> SleepV1ExplanationStabilityAcceptanceSummary {
    SleepV1ExplanationStabilityAcceptanceSummary {
        policy: "sleep_v1_explanation_components_signals_and_stability_must_match_release_contract"
            .to_string(),
        explanation_and_stability_ready: explanation_pass
            && explanation_quality_pass
            && repeated_run_stability_pass
            && perturbation_stability_pass,
        expected_component_names: sleep_v1_expected_component_names()
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        observed_component_names: v1_component_names.to_vec(),
        v0_component_count,
        v1_component_count,
        min_v1_component_count: options.min_v1_component_count,
        explanation_quality_signals: explanation_quality_signals.to_vec(),
        explanation_quality_signal_count,
        min_explanation_quality_signal_count: options.min_explanation_quality_signal_count,
        sleep_window_confidence_0_to_1,
        perturbed_sleep_window_confidence_0_to_1,
        repeated_run_delta,
        max_repeated_run_delta: options.max_repeated_run_delta,
        small_perturbation_delta,
        max_small_perturbation_delta: options.max_small_perturbation_delta,
        perturb_sleep_duration_minutes: options.perturb_sleep_duration_minutes,
        issue_count,
        quality_flag_count,
        error_count,
        next_action_count,
    }
}

fn sleep_v1_explanation_quality_signals(output: &SleepV1Output) -> Vec<String> {
    let mut signals = Vec::new();
    if !output.status_report.status_label.trim().is_empty()
        && !output.status_report.status_reason.trim().is_empty()
    {
        signals.push("model_status_label_and_reason".to_string());
    }
    if output.status_report.can_show_provisional_score || output.status_report.can_show_final_score
    {
        signals.push("score_visibility_gate".to_string());
    }
    if output.previous_night_comparison.is_some() {
        signals.push("previous_night_comparison".to_string());
    }
    if output.components.len() == sleep_v1_expected_component_names().len()
        && output
            .components
            .iter()
            .all(|component| output.component_provenance.contains_key(&component.name))
    {
        signals.push("component_breakdown_with_provenance".to_string());
    }
    if output
        .component_provenance
        .get("sleep_architecture")
        .and_then(|provenance| provenance.get("inputs"))
        .and_then(Value::as_object)
        .is_some_and(|inputs| inputs.contains_key("stage_prior_calibration"))
    {
        signals.push("stage_prior_calibration".to_string());
    }
    if output
        .component_provenance
        .get("cardiovascular_recovery")
        .and_then(|provenance| provenance.get("inputs"))
        .and_then(Value::as_object)
        .is_some_and(|inputs| {
            inputs.contains_key("sleep_hr_trend_bpm_per_hour")
                && inputs.contains_key("pre_sleep_awake_hr_average_bpm")
        })
    {
        signals.push("cardiovascular_recovery_context".to_string());
    }
    if finite_non_negative(output.sleep_window_confidence_0_to_1)
        && output.sleep_window_confidence_0_to_1 <= 1.0
        && finite_non_negative(output.confidence_0_to_1)
        && output.confidence_0_to_1 <= 1.0
    {
        signals.push("confidence_and_window_quality".to_string());
    }
    if output.provenance.get("previous_night_comparison").is_some()
        && output.provenance.get("score_policy").is_some()
    {
        signals.push("why_changed_and_score_policy_provenance".to_string());
    }
    signals
}

fn explanation_stability_next_actions(
    issues: &[String],
) -> Vec<SleepV1ExplanationStabilityNextAction> {
    issues
        .iter()
        .map(|issue| SleepV1ExplanationStabilityNextAction {
            scope: explanation_stability_issue_scope(issue).to_string(),
            reason: issue.to_string(),
            action: explanation_stability_issue_action(issue),
        })
        .collect()
}

fn explanation_stability_issue_scope(issue: &str) -> &'static str {
    match issue {
        "sleep_v1_output_missing"
        | "sleep_v1_repeated_output_missing"
        | "sleep_v1_perturbed_output_missing" => "sleep_v1.output",
        "sleep_v1_repeated_run_delta_exceeds_threshold"
        | "sleep_v1_small_perturbation_delta_exceeds_threshold" => "sleep_v1.stability",
        "sleep_v1_component_count_below_quality_gate"
        | "sleep_v1_has_fewer_explanation_components_than_v0"
        | "sleep_v1_duplicate_explanation_components"
        | "sleep_v1_component_contract_mismatch"
        | "sleep_v1_component_provenance_missing"
        | "sleep_v1_component_inputs_missing"
        | "sleep_v1_component_policy_missing" => "sleep_v1.explanation",
        "sleep_v1_status_label_missing" | "sleep_v1_status_reason_missing" => "sleep_v1.status",
        "sleep_v1_explanation_quality_signal_count_below_gate"
        | "sleep_v1_explanation_quality_signal_contract_mismatch" => "sleep_v1.explanation_quality",
        "sleep_v1_sleep_window_confidence_missing_or_invalid"
        | "sleep_v1_perturbed_sleep_window_confidence_missing_or_invalid" => {
            "sleep_v1.window_confidence"
        }
        "sleep_v1_quality_flags_present" => "sleep_v1.quality",
        _ => "sleep_v1.validation",
    }
}

fn explanation_stability_issue_action(issue: &str) -> String {
    match issue {
        "sleep_v1_output_missing" => {
            "Fix Sleep V1 input validation or scoring errors before assessing explanation quality.".to_string()
        }
        "sleep_v1_repeated_output_missing" | "sleep_v1_repeated_run_delta_exceeds_threshold" => {
            "Keep Sleep V1 behind validation until repeated scoring is deterministic for the same input.".to_string()
        }
        "sleep_v1_perturbed_output_missing"
        | "sleep_v1_small_perturbation_delta_exceeds_threshold" => {
            "Inspect score curves and guardrails so small plausible input changes do not cause unstable score swings.".to_string()
        }
        "sleep_v1_component_count_below_quality_gate" => {
            "Add or restore Sleep V1 component explanations before promoting it over V0.".to_string()
        }
        "sleep_v1_has_fewer_explanation_components_than_v0" => {
            "Keep V1 experimental until it preserves at least the V0 component breadth while adding provenance and model status.".to_string()
        }
        "sleep_v1_duplicate_explanation_components" => {
            "Regenerate Sleep V1 explanations with one unique row per weighted component.".to_string()
        }
        "sleep_v1_component_contract_mismatch" => {
            "Regenerate Sleep V1 explanations with exactly the current weighted component contract.".to_string()
        }
        "sleep_v1_component_provenance_missing"
        | "sleep_v1_component_inputs_missing"
        | "sleep_v1_component_policy_missing" => {
            "Attach component provenance with inputs and policy text for every weighted Sleep V1 component.".to_string()
        }
        "sleep_v1_status_label_missing" | "sleep_v1_status_reason_missing" => {
            "Return user-visible Sleep V1 model status labels and reasons with every report.".to_string()
        }
        "sleep_v1_explanation_quality_signal_count_below_gate"
        | "sleep_v1_explanation_quality_signal_contract_mismatch" => {
            "Keep Sleep V1 experimental until reports include the full expected explanation-quality contract: model status, score visibility, previous-night context, component provenance, stage-prior calibration, HR recovery context, confidence provenance, and why-changed policy.".to_string()
        }
        "sleep_v1_sleep_window_confidence_missing_or_invalid"
        | "sleep_v1_perturbed_sleep_window_confidence_missing_or_invalid" => {
            "Return bounded Sleep V1 sleep-window confidence for base and perturbed reports before promotion.".to_string()
        }
        "sleep_v1_quality_flags_present" => {
            "Resolve Sleep V1 quality flags on the validation input before using the stability report for promotion.".to_string()
        }
        _ => "Inspect the Sleep V1 explanation/stability issue before promotion.".to_string(),
    }
}

fn sleep_v1_release_gate_next_actions(issues: &[String]) -> Vec<SleepV1ReleaseGateNextAction> {
    issues
        .iter()
        .map(|issue| SleepV1ReleaseGateNextAction {
            scope: sleep_v1_release_gate_issue_scope(issue).to_string(),
            reason: issue.clone(),
            action: sleep_v1_release_gate_issue_action(issue),
        })
        .collect()
}

fn sleep_v1_release_gate_issue_scope(issue: &str) -> &'static str {
    match issue {
        "physical_historical_sync_report_missing" | "physical_historical_sync_not_validated" => {
            "historical_sync.physical"
        }
        "physical_historical_sync_report_integrity_failed" => "historical_sync.physical",
        "release_gate_report_integrity_policy_missing"
        | "release_gate_report_threshold_policy_missing" => "sleep_v1.release_gate",
        "historical_motion_hr_timestamps_not_proven"
        | "historical_motion_timestamp_fields_unproven"
        | "historical_heart_rate_timestamp_fields_unproven" => "historical_sync.timestamps",
        "sleep_window_label_report_missing"
        | "packet_sleep_windows_not_validated_against_hand_review"
        | "hand_reviewed_sleep_window_sample_below_gate"
        | "release_gate_hand_reviewed_window_threshold_below_default"
        | "sleep_window_label_report_integrity_failed"
        | "sleep_window_label_threshold_below_default" => "sleep_window.labels",
        "sleep_stage_label_report_missing"
        | "sleep_v1_stage_labels_not_validated"
        | "sleep_stage_label_sample_below_gate"
        | "release_gate_stage_label_threshold_below_default"
        | "sleep_stage_label_report_integrity_failed"
        | "sleep_stage_label_threshold_below_default" => "sleep_stage.labels",
        "sleep_v1_explanation_stability_report_missing"
        | "sleep_v1_explanation_or_stability_not_validated"
        | "sleep_v1_explanation_stability_report_integrity_failed"
        | "sleep_v1_explanation_stability_threshold_below_default" => {
            "sleep_v1.explanation_stability"
        }
        "sleep_v1_benchmark_report_missing"
        | "sleep_v1_benchmark_comparison_below_gate"
        | "release_gate_benchmark_threshold_below_default"
        | "sleep_v1_benchmark_report_integrity_failed" => "sleep_v1.benchmark",
        _ => "sleep_v1.release_gate",
    }
}

fn sleep_v1_release_gate_issue_action(issue: &str) -> String {
    match issue {
        "physical_historical_sync_report_missing" => {
            "Run physical historical-sync validation from a filled WHOOP capture template before opening the Sleep V1 promotion gate.".to_string()
        }
        "physical_historical_sync_not_validated" => {
            "Keep Sleep V1 provisional until a physical strap capture validates historical service discovery, subscriptions, auth/session flow, and historical commands.".to_string()
        }
        "physical_historical_sync_report_integrity_failed" => {
            "Regenerate the physical historical-sync validation report from the capture evidence so schema, generator, subgates, proof counts, acceptance summary, issues, and pass status agree.".to_string()
        }
        "release_gate_report_integrity_policy_missing" => {
            "Regenerate the release-gate report with the current subgate-integrity provenance before making Sleep V1 primary.".to_string()
        }
        "release_gate_report_threshold_policy_missing" => {
            "Regenerate the release-gate report with the current primary-threshold policy provenance before making Sleep V1 primary.".to_string()
        }
        "historical_motion_hr_timestamps_not_proven" => {
            "Capture historical motion and heart-rate packets with device timestamp fields and normalized sample times before promoting packet-derived sleep.".to_string()
        }
        "historical_motion_timestamp_fields_unproven" => {
            "Capture historical motion packets with device timestamp fields and normalized sample times before promoting packet-derived sleep.".to_string()
        }
        "historical_heart_rate_timestamp_fields_unproven" => {
            "Capture historical heart-rate packets with device timestamp fields and normalized sample times before promoting packet-derived sleep.".to_string()
        }
        "sleep_window_label_report_missing" => {
            "Run sleep-window label validation and attach the report before opening the Sleep V1 promotion gate.".to_string()
        }
        "packet_sleep_windows_not_validated_against_hand_review" => {
            "Run sleep-window label validation against hand-reviewed nights and fix onset/final-wake tolerances before promotion.".to_string()
        }
        "hand_reviewed_sleep_window_sample_below_gate" => {
            "Add enough hand-reviewed sleep-window labels to satisfy the configured release sample gate.".to_string()
        }
        "release_gate_hand_reviewed_window_threshold_below_default" => {
            format!(
                "Require at least {} distinct hand-reviewed sleep windows before making Sleep V1 primary.",
                default_min_hand_reviewed_window_comparisons()
            )
        }
        "sleep_window_label_report_integrity_failed" => {
            "Regenerate the sleep-window validation report from the store snapshot so summary counts match the comparison rows.".to_string()
        }
        "sleep_window_label_threshold_below_default" => {
            "Regenerate the sleep-window validation report with release-default or stricter reviewer confidence and start, end, and duration tolerances.".to_string()
        }
        "sleep_stage_label_report_missing" => {
            "Run sleep-stage label validation and attach the report before opening the Sleep V1 promotion gate.".to_string()
        }
        "sleep_v1_stage_labels_not_validated" => {
            "Validate Sleep V1 stage segments against user-owned sleep-stage labels before promotion.".to_string()
        }
        "sleep_stage_label_sample_below_gate" => {
            "Add enough user-owned sleep-stage labels to satisfy the configured release sample gate.".to_string()
        }
        "release_gate_stage_label_threshold_below_default" => {
            format!(
                "Require at least {} passing user-owned sleep-stage label comparison before making Sleep V1 primary.",
                default_min_stage_label_comparisons()
            )
        }
        "sleep_stage_label_report_integrity_failed" => {
            "Regenerate the sleep-stage validation report from the store snapshot so summary counts, comparison rows, provenance, and pass status agree.".to_string()
        }
        "sleep_stage_label_threshold_below_default" => {
            "Regenerate the sleep-stage validation report with release-default or stricter label confidence and overlap thresholds.".to_string()
        }
        "sleep_v1_explanation_stability_report_missing" => {
            "Run Sleep V1 explanation/stability validation from the exact Sleep V1 input before opening the promotion gate.".to_string()
        }
        "sleep_v1_explanation_or_stability_not_validated" => {
            "Run Sleep V1 explanation/stability validation and resolve component provenance, status, determinism, or perturbation failures.".to_string()
        }
        "sleep_v1_explanation_stability_report_integrity_failed" => {
            "Regenerate the Sleep V1 explanation/stability report from the exact Sleep V1 input so booleans, counts, deltas, and issues agree.".to_string()
        }
        "sleep_v1_explanation_stability_threshold_below_default" => {
            "Regenerate the Sleep V1 explanation/stability report with release-default or stricter component, determinism, perturbation-size, and score-delta thresholds.".to_string()
        }
        "sleep_v1_benchmark_report_missing" => {
            "Run the Sleep V1 benchmark comparison and attach at least one passing sleep-actigraphy report before opening the promotion gate.".to_string()
        }
        "sleep_v1_benchmark_comparison_below_gate" => {
            "Run Sleep V1 benchmark comparison against the reference sleep algorithm and require the configured number of passing reports.".to_string()
        }
        "release_gate_benchmark_threshold_below_default" => {
            format!(
                "Require at least {} passing Sleep V1 benchmark comparison before making Sleep V1 primary.",
                default_min_benchmark_comparisons()
            )
        }
        "sleep_v1_benchmark_report_integrity_failed" => {
            "Regenerate benchmark comparison reports with goose.sleep.v1 output, valid reference output, shared deltas, and no errors before promotion.".to_string()
        }
        _ => "Resolve the Sleep V1 release-gate issue before making V1 primary.".to_string(),
    }
}

pub fn validate_sleep_v1_stage_labels_for_store(
    store: &GooseStore,
    input: &SleepV1Input,
    options: SleepStageLabelValidationOptions,
) -> GooseResult<SleepStageLabelValidationReport> {
    let sleep_id = input
        .sleep
        .input_ids
        .first()
        .cloned()
        .unwrap_or_else(|| "sleep-v1-input".to_string());
    let start_unix_ms = parse_rfc3339_utc_unix_ms(&input.sleep.start_time).ok_or_else(|| {
        GooseError::message("sleep v1 stage label validation start_time is invalid")
    })?;
    let end_unix_ms = parse_rfc3339_utc_unix_ms(&input.sleep.end_time).ok_or_else(|| {
        GooseError::message("sleep v1 stage label validation end_time is invalid")
    })?;
    if end_unix_ms <= start_unix_ms {
        return Err(GooseError::message(
            "sleep v1 stage label validation window is invalid",
        ));
    }

    let result = goose_sleep_v1(input);
    let labels = store
        .sleep_correction_labels_between(start_unix_ms, end_unix_ms)?
        .into_iter()
        .filter(|label| label.label_type == "sleep_stage")
        .collect::<Vec<_>>();
    let mut issues = result
        .errors
        .iter()
        .map(|error| format!("sleep_v1_error:{error}"))
        .collect::<Vec<_>>();
    let output = result.output.as_ref();
    if output.is_none() {
        issues.push("sleep_v1_output_missing".to_string());
    }

    let mut comparisons = Vec::new();
    if let Some(output) = output {
        for label in &labels {
            if let Some(comparison) =
                sleep_stage_label_comparison(label, &sleep_id, output, &options, &mut issues)?
            {
                comparisons.push(comparison);
            }
        }
    }

    Ok(sleep_stage_label_validation_report(
        &sleep_id,
        &input.sleep.start_time,
        &input.sleep.end_time,
        labels,
        comparisons,
        output.map_or(0, |output| output.stage_segments.len()),
        issues,
        result.quality_flags,
        &options,
    ))
}

fn sleep_stage_label_comparison(
    label: &SleepCorrectionLabelRow,
    sleep_id: &str,
    output: &SleepV1Output,
    options: &SleepStageLabelValidationOptions,
    issues: &mut Vec<String>,
) -> GooseResult<Option<SleepStageLabelComparison>> {
    let Some(confidence) = label.confidence else {
        issues.push(format!("{}:label_confidence_missing", label.label_id));
        return Ok(None);
    };
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        issues.push(format!("{}:label_confidence_out_of_range", label.label_id));
        return Ok(None);
    }
    if confidence < options.min_label_confidence {
        issues.push(format!(
            "{}:label_confidence_below_threshold",
            label.label_id
        ));
        return Ok(None);
    }
    let Some(label_provenance) = stage_label_provenance(label) else {
        issues.push(format!("{}:label_provenance_missing", label.label_id));
        return Ok(None);
    };
    if label_provenance.source.trim().is_empty() {
        issues.push(format!(
            "{}:label_provenance_source_missing",
            label.label_id
        ));
        return Ok(None);
    }
    if label_provenance.source != label.source {
        issues.push(format!(
            "{}:label_provenance_source_mismatch",
            label.label_id
        ));
        return Ok(None);
    }
    let Some(label_sleep_id) = label.sleep_id.as_deref().map(str::trim) else {
        issues.push(format!("{}:label_sleep_id_missing", label.label_id));
        return Ok(None);
    };
    if label_sleep_id.is_empty() {
        issues.push(format!("{}:label_sleep_id_missing", label.label_id));
        return Ok(None);
    }
    if label_sleep_id != sleep_id {
        issues.push(format!("{}:label_sleep_id_mismatch", label.label_id));
        return Ok(None);
    }
    let value_json = serde_json::from_str::<Value>(&label.value_json).map_err(|error| {
        GooseError::message(format!(
            "sleep stage label {} value_json invalid: {error}",
            label.label_id
        ))
    })?;
    let Some(expected_stage_kind) = value_json
        .get("stage_kind")
        .or_else(|| value_json.get("expected_stage_kind"))
        .and_then(Value::as_str)
        .and_then(canonical_sleep_stage_label_kind)
    else {
        issues.push(format!("{}:label_stage_kind_invalid", label.label_id));
        return Ok(None);
    };
    let label_duration_ms = label.end_time_unix_ms - label.start_time_unix_ms;
    if label_duration_ms <= 0 {
        issues.push(format!("{}:label_window_invalid", label.label_id));
        return Ok(None);
    }

    let mut best: Option<(&crate::metrics::SleepStageSegment, i64)> = None;
    for segment in &output.stage_segments {
        let Some(segment_start) = parse_rfc3339_utc_unix_ms(&segment.start_time) else {
            continue;
        };
        let Some(segment_end) = parse_rfc3339_utc_unix_ms(&segment.end_time) else {
            continue;
        };
        let overlap =
            segment_end.min(label.end_time_unix_ms) - segment_start.max(label.start_time_unix_ms);
        if overlap > 0 && best.is_none_or(|(_, best_overlap)| overlap > best_overlap) {
            best = Some((segment, overlap));
        }
    }

    let (observed_stage_kind, observed_start_time, observed_end_time, overlap_ms) =
        if let Some((segment, overlap_ms)) = best {
            (
                Some(segment.stage_kind.clone()),
                Some(segment.start_time.clone()),
                Some(segment.end_time.clone()),
                overlap_ms,
            )
        } else {
            (None, None, None, 0)
        };
    let overlap_minutes = overlap_ms as f64 / 60_000.0;
    let overlap_fraction = overlap_ms as f64 / label_duration_ms as f64;
    let mut quality_flags = Vec::new();
    if overlap_fraction < options.min_overlap_fraction {
        quality_flags.push("stage_label_overlap_below_threshold".to_string());
    }
    if observed_stage_kind.as_deref() != Some(expected_stage_kind) {
        quality_flags.push("stage_label_kind_mismatch".to_string());
    }

    Ok(Some(SleepStageLabelComparison {
        label_id: label.label_id.clone(),
        sleep_id: label.sleep_id.clone(),
        source: label.source.clone(),
        provenance_source: label_provenance.source,
        label_provenance_policy: label_provenance.policy,
        confidence: label.confidence,
        expected_stage_kind: expected_stage_kind.to_string(),
        observed_stage_kind,
        label_start_time: unix_ms_to_rfc3339_utc(label.start_time_unix_ms),
        label_end_time: unix_ms_to_rfc3339_utc(label.end_time_unix_ms),
        label_start_time_unix_ms: label.start_time_unix_ms,
        label_end_time_unix_ms: label.end_time_unix_ms,
        observed_start_time,
        observed_end_time,
        overlap_minutes,
        overlap_fraction,
        pass: quality_flags.is_empty(),
        quality_flags,
    }))
}

fn sleep_stage_label_validation_report(
    sleep_id: &str,
    start: &str,
    end: &str,
    labels: Vec<SleepCorrectionLabelRow>,
    comparisons: Vec<SleepStageLabelComparison>,
    stage_segment_count: usize,
    mut issues: Vec<String>,
    mut quality_flags: Vec<String>,
    options: &SleepStageLabelValidationOptions,
) -> SleepStageLabelValidationReport {
    if labels.is_empty() {
        issues.push("no_sleep_stage_labels_in_range".to_string());
    }
    if !labels.is_empty() && comparisons.is_empty() {
        issues.push("no_sleep_stage_labels_met_confidence_threshold".to_string());
    }
    for comparison in &comparisons {
        for flag in &comparison.quality_flags {
            issues.push(format!("{}:{flag}", comparison.label_id));
        }
    }
    issues.sort();
    issues.dedup();
    quality_flags.sort();
    quality_flags.dedup();
    let passing_label_count = comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .count();
    let pass = !comparisons.is_empty()
        && passing_label_count == comparisons.len()
        && issues.is_empty()
        && quality_flags.is_empty();
    let next_actions = sleep_stage_label_validation_next_actions(&issues);

    SleepStageLabelValidationReport {
        schema: SLEEP_STAGE_LABEL_VALIDATION_SCHEMA.to_string(),
        generated_by: "goose-sleep-stage-label-validator".to_string(),
        pass,
        sleep_id: sleep_id.to_string(),
        start_time: start.to_string(),
        end_time: end.to_string(),
        label_count: labels.len(),
        compared_label_count: comparisons.len(),
        passing_label_count,
        stage_segment_count,
        acceptance_summary: sleep_stage_label_acceptance_summary(
            labels.len(),
            stage_segment_count,
            &comparisons,
            options.min_label_confidence,
            options.min_overlap_fraction,
            issues.len(),
            quality_flags.len(),
            0,
            next_actions.len(),
        ),
        comparisons,
        issues,
        quality_flags,
        errors: Vec::new(),
        next_actions,
        provenance: json!({
            "label_source": "sleep_correction_labels",
            "comparison_policy": SLEEP_STAGE_LABEL_VALIDATION_POLICY,
            "validation_policy": SLEEP_STAGE_LABEL_VALIDATION_POLICY,
            "min_label_confidence": options.min_label_confidence,
            "min_overlap_fraction": options.min_overlap_fraction,
            "official_labels_policy": "official_or_platform_stage_values_are_labels_not_goose_outputs",
            "report_integrity_policy": SLEEP_STAGE_LABEL_REPORT_INTEGRITY_POLICY,
        }),
    }
}

fn sleep_stage_label_acceptance_summary(
    label_count: usize,
    stage_segment_count: usize,
    comparisons: &[SleepStageLabelComparison],
    min_label_confidence: f64,
    min_overlap_fraction: f64,
    issue_count: usize,
    quality_flag_count: usize,
    error_count: usize,
    next_action_count: usize,
) -> SleepStageLabelAcceptanceSummary {
    let passing_comparisons = comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .collect::<Vec<_>>();
    let mut accepted_label_ids = passing_comparisons
        .iter()
        .map(|comparison| comparison.label_id.clone())
        .collect::<Vec<_>>();
    accepted_label_ids.sort();
    let mut accepted_stage_kinds = passing_comparisons
        .iter()
        .map(|comparison| comparison.expected_stage_kind.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    accepted_stage_kinds.sort();
    let min_observed_overlap_fraction = passing_comparisons
        .iter()
        .map(|comparison| comparison.overlap_fraction)
        .fold(1.0, f64::min);
    let min_observed_label_confidence = passing_comparisons
        .iter()
        .filter_map(|comparison| comparison.confidence)
        .fold(1.0, f64::min);
    let passing_label_count = passing_comparisons.len();
    SleepStageLabelAcceptanceSummary {
        policy: "sleep_v1_stages_must_match_user_owned_stage_labels".to_string(),
        user_owned_stage_sample_ready: !comparisons.is_empty()
            && label_count == comparisons.len()
            && passing_label_count == comparisons.len()
            && stage_segment_count > 0,
        label_count,
        compared_label_count: comparisons.len(),
        passing_label_count,
        stage_segment_count,
        required_release_passing_stage_labels: default_min_stage_label_comparisons(),
        accepted_label_ids,
        accepted_stage_kinds,
        min_label_confidence,
        min_overlap_fraction,
        min_observed_overlap_fraction,
        min_observed_label_confidence,
        issue_count,
        quality_flag_count,
        error_count,
        next_action_count,
    }
}

fn canonical_sleep_stage_label_kind(stage: &str) -> Option<&'static str> {
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
        _ => None,
    }
}

struct StageLabelProvenance {
    policy: String,
    source: String,
}

fn stage_label_provenance(label: &SleepCorrectionLabelRow) -> Option<StageLabelProvenance> {
    let provenance = serde_json::from_str::<Value>(&label.provenance_json).ok()?;
    let policy = if provenance.get("review_policy").and_then(Value::as_str)
        == Some("user_owned_sleep_stage_label")
    {
        "user_owned_sleep_stage_label"
    } else if provenance
        .get("official_labels_are_labels")
        .and_then(Value::as_bool)
        == Some(true)
    {
        "official_labels_are_labels"
    } else {
        return None;
    };
    Some(StageLabelProvenance {
        policy: policy.to_string(),
        source: provenance
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn sleep_stage_label_validation_next_actions(
    issues: &[String],
) -> Vec<SleepStageLabelValidationNextAction> {
    issues
        .iter()
        .map(|issue| {
            let reason = issue
                .split_once(':')
                .map_or(issue.as_str(), |(_, reason)| reason);
            SleepStageLabelValidationNextAction {
                scope: sleep_stage_label_validation_issue_scope(issue, reason).to_string(),
                reason: reason.to_string(),
                action: sleep_stage_label_validation_action(reason),
            }
        })
        .collect()
}

fn sleep_stage_label_validation_issue_scope(issue: &str, reason: &str) -> &'static str {
    if matches!(
        reason,
        "stage_label_overlap_below_threshold" | "stage_label_kind_mismatch"
    ) {
        "sleep_stage.comparison"
    } else if issue.contains(':')
        || matches!(
            reason,
            "no_sleep_stage_labels_in_range"
                | "no_sleep_stage_labels_met_confidence_threshold"
                | "label_confidence_missing"
                | "label_confidence_out_of_range"
                | "label_confidence_below_threshold"
                | "label_provenance_missing"
                | "label_provenance_source_missing"
                | "label_provenance_source_mismatch"
                | "label_sleep_id_missing"
                | "label_sleep_id_mismatch"
                | "label_stage_kind_invalid"
                | "label_window_invalid"
        )
    {
        "sleep_stage.labels"
    } else {
        "sleep_stage.validation"
    }
}

fn sleep_stage_label_validation_action(reason: &str) -> String {
    match reason {
        "no_sleep_stage_labels_in_range" => {
            "Import user-owned sleep-stage labels from HealthKit, Health Connect, WHOOP export, screenshot review, or manual review before calibrating stage accuracy.".to_string()
        }
        "no_sleep_stage_labels_met_confidence_threshold" => {
            "Attach higher-confidence user-owned sleep-stage labels before using stage validation for Sleep V1 calibration.".to_string()
        }
        "label_confidence_missing" | "label_confidence_out_of_range" => {
            "Add a bounded confidence value to each sleep-stage label before validation.".to_string()
        }
        "label_confidence_below_threshold" => {
            "Use sleep-stage labels with confidence at or above the validation threshold.".to_string()
        }
        "label_provenance_missing" => {
            "Mark stage labels as user-owned labels, not Goose outputs, with review_policy=user_owned_sleep_stage_label or official_labels_are_labels=true.".to_string()
        }
        "label_provenance_source_missing" => {
            "Store the user-owned sleep-stage label source in provenance before validation.".to_string()
        }
        "label_provenance_source_mismatch" => {
            "Keep the sleep-stage label source and provenance source identical before validation.".to_string()
        }
        "label_sleep_id_missing" => {
            "Attach the Sleep V1 input sleep_id to each user-owned sleep-stage label before validation.".to_string()
        }
        "label_sleep_id_mismatch" => {
            "Use sleep-stage labels from the same concrete sleep_id as the Sleep V1 input being validated.".to_string()
        }
        "label_stage_kind_invalid" => {
            "Normalize stage labels to awake, core, deep, or rem before validating Sleep V1 stages.".to_string()
        }
        "label_window_invalid" => {
            "Fix sleep-stage label start/end timestamps before validation.".to_string()
        }
        "stage_label_overlap_below_threshold" => {
            "Align stage-label timestamps with the scored Sleep V1 window or lower thresholds only with documented evidence.".to_string()
        }
        "stage_label_kind_mismatch" => {
            "Inspect Sleep V1 stage probabilities and stage-prior calibration where user-owned stage labels disagree with predicted stages.".to_string()
        }
        "sleep_v1_output_missing" => {
            "Fix Sleep V1 scoring before comparing stage labels.".to_string()
        }
        _ if reason.starts_with("sleep_v1_error") => {
            "Resolve Sleep V1 input or scoring errors before validating stage labels.".to_string()
        }
        _ => "Inspect the sleep-stage label validation issue before using labels for calibration.".to_string(),
    }
}

pub fn run_sleep_window_label_validation_for_store(
    store: &GooseStore,
    database_path: &str,
    start: &str,
    end: &str,
    options: SleepWindowLabelValidationOptions,
) -> GooseResult<SleepWindowLabelValidationReport> {
    let sleep_feature_report = run_sleep_feature_score_report_for_store(
        store,
        database_path,
        start,
        end,
        SleepFeatureScoreOptions {
            min_owned_captures_per_summary: options.min_owned_captures_per_summary,
            require_trusted_evidence: options.require_trusted_evidence,
            sleep_need_minutes: options.sleep_need_minutes,
            low_motion_threshold_0_to_1: options.low_motion_threshold_0_to_1,
            disturbance_motion_threshold_0_to_1: options.disturbance_motion_threshold_0_to_1,
            target_midpoint_minutes_since_midnight: options.target_midpoint_minutes_since_midnight,
        },
    )?;

    let mut issues = Vec::new();
    let mut comparisons = Vec::new();
    let mut labels = Vec::new();
    if let Some(window) = &sleep_feature_report.sleep_window {
        let Some(observed_start_time_unix_ms) = parse_rfc3339_utc_unix_ms(&window.start_time)
        else {
            issues.push("sleep_window_start_time_unparseable".to_string());
            return Ok(validation_report(
                start,
                end,
                labels,
                comparisons,
                sleep_feature_report,
                issues,
                &options,
            ));
        };
        let Some(observed_end_time_unix_ms) = parse_rfc3339_utc_unix_ms(&window.end_time) else {
            issues.push("sleep_window_end_time_unparseable".to_string());
            return Ok(validation_report(
                start,
                end,
                labels,
                comparisons,
                sleep_feature_report,
                issues,
                &options,
            ));
        };
        let query_start = observed_start_time_unix_ms.saturating_sub(
            options
                .start_tolerance_minutes
                .max(options.duration_tolerance_minutes)
                .ceil() as i64
                * 60_000,
        );
        let query_end = observed_end_time_unix_ms.saturating_add(
            options
                .end_tolerance_minutes
                .max(options.duration_tolerance_minutes)
                .ceil() as i64
                * 60_000,
        );
        labels = store
            .sleep_correction_labels_between(query_start.max(0), query_end)?
            .into_iter()
            .filter(|label| label.label_type == "sleep_window")
            .collect();
        for label in &labels {
            let Some(confidence) = label.confidence else {
                issues.push(format!("{}:label_confidence_missing", label.label_id));
                continue;
            };
            if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                issues.push(format!("{}:label_confidence_out_of_range", label.label_id));
                continue;
            }
            if confidence < options.min_label_confidence {
                issues.push(format!(
                    "{}:label_confidence_below_threshold",
                    label.label_id
                ));
                continue;
            }
            let Some(label_provenance) = window_label_provenance(label) else {
                issues.push(format!("{}:label_review_policy_missing", label.label_id));
                continue;
            };
            if label_provenance.source.trim().is_empty() {
                issues.push(format!(
                    "{}:label_provenance_source_missing",
                    label.label_id
                ));
                continue;
            }
            if label_provenance.source != label.source {
                issues.push(format!(
                    "{}:label_provenance_source_mismatch",
                    label.label_id
                ));
                continue;
            }
            if label
                .sleep_id
                .as_deref()
                .is_none_or(|sleep_id| sleep_id.trim().is_empty())
            {
                issues.push(format!("{}:label_sleep_id_missing", label.label_id));
                continue;
            }
            if let Some(comparison) = sleep_window_label_comparison(
                label,
                &window.start_time,
                &window.end_time,
                observed_start_time_unix_ms,
                observed_end_time_unix_ms,
                label_provenance,
                &options,
                &mut issues,
            )? {
                comparisons.push(comparison);
            }
        }
    } else {
        issues.push("sleep_window_missing".to_string());
    }

    Ok(validation_report(
        start,
        end,
        labels,
        comparisons,
        sleep_feature_report,
        issues,
        &options,
    ))
}

fn sleep_window_label_comparison(
    label: &SleepCorrectionLabelRow,
    observed_start_time: &str,
    observed_end_time: &str,
    observed_start_time_unix_ms: i64,
    observed_end_time_unix_ms: i64,
    label_provenance: WindowLabelProvenance,
    options: &SleepWindowLabelValidationOptions,
    issues: &mut Vec<String>,
) -> GooseResult<Option<SleepWindowLabelComparison>> {
    let value_json = serde_json::from_str::<Value>(&label.value_json).map_err(|error| {
        GooseError::message(format!(
            "sleep correction label {} value_json invalid: {error}",
            label.label_id
        ))
    })?;
    let Some(expected_start_time_unix_ms) = corrected_label_time_unix_ms(
        label,
        &value_json,
        "corrected_start_time_unix_ms",
        "label_corrected_start_time_invalid",
        label.start_time_unix_ms,
        issues,
    ) else {
        return Ok(None);
    };
    let Some(expected_end_time_unix_ms) = corrected_label_time_unix_ms(
        label,
        &value_json,
        "corrected_end_time_unix_ms",
        "label_corrected_end_time_invalid",
        label.end_time_unix_ms,
        issues,
    ) else {
        return Ok(None);
    };
    if expected_end_time_unix_ms <= expected_start_time_unix_ms {
        issues.push(format!("{}:label_window_invalid", label.label_id));
        return Ok(None);
    }

    let start_delta_minutes =
        (observed_start_time_unix_ms - expected_start_time_unix_ms).abs() as f64 / 60_000.0;
    let end_delta_minutes =
        (observed_end_time_unix_ms - expected_end_time_unix_ms).abs() as f64 / 60_000.0;
    let observed_duration_minutes =
        (observed_end_time_unix_ms - observed_start_time_unix_ms) as f64 / 60_000.0;
    let expected_duration_minutes =
        (expected_end_time_unix_ms - expected_start_time_unix_ms) as f64 / 60_000.0;
    let duration_delta_minutes = (observed_duration_minutes - expected_duration_minutes).abs();

    let mut quality_flags = Vec::new();
    if start_delta_minutes > options.start_tolerance_minutes {
        quality_flags.push("sleep_window_start_outside_tolerance".to_string());
    }
    if end_delta_minutes > options.end_tolerance_minutes {
        quality_flags.push("sleep_window_end_outside_tolerance".to_string());
    }
    if duration_delta_minutes > options.duration_tolerance_minutes {
        quality_flags.push("sleep_window_duration_outside_tolerance".to_string());
    }
    let pass = quality_flags.is_empty();

    Ok(Some(SleepWindowLabelComparison {
        label_id: label.label_id.clone(),
        sleep_id: label.sleep_id.clone(),
        source: label.source.clone(),
        provenance_source: label_provenance.source,
        label_provenance_policy: label_provenance.policy,
        confidence: label.confidence,
        expected_start_time: unix_ms_to_rfc3339_utc(expected_start_time_unix_ms),
        expected_end_time: unix_ms_to_rfc3339_utc(expected_end_time_unix_ms),
        expected_start_time_unix_ms,
        expected_end_time_unix_ms,
        observed_start_time: observed_start_time.to_string(),
        observed_end_time: observed_end_time.to_string(),
        observed_start_time_unix_ms,
        observed_end_time_unix_ms,
        start_delta_minutes,
        end_delta_minutes,
        duration_delta_minutes,
        pass,
        quality_flags,
    }))
}

fn corrected_label_time_unix_ms(
    label: &SleepCorrectionLabelRow,
    value_json: &Value,
    field: &str,
    issue: &str,
    fallback: i64,
    issues: &mut Vec<String>,
) -> Option<i64> {
    match value_json.get(field) {
        Some(value) => value.as_i64().or_else(|| {
            issues.push(format!("{}:{issue}", label.label_id));
            None
        }),
        None => Some(fallback),
    }
}

struct WindowLabelProvenance {
    policy: String,
    source: String,
}

fn window_label_provenance(label: &SleepCorrectionLabelRow) -> Option<WindowLabelProvenance> {
    let provenance = serde_json::from_str::<Value>(&label.provenance_json).ok()?;
    let provenance_policy = provenance
        .get("review_policy")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "hand_reviewed_sleep_window");
    let value_policy = serde_json::from_str::<Value>(&label.value_json)
        .ok()
        .and_then(|value| {
            value
                .get("review_source")
                .and_then(Value::as_str)
                .map(|value| value == "hand_reviewed")
        })
        .unwrap_or(false);
    if !provenance_policy && !value_policy {
        return None;
    }
    Some(WindowLabelProvenance {
        policy: "hand_reviewed_sleep_window".to_string(),
        source: provenance
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn validation_report(
    start: &str,
    end: &str,
    labels: Vec<SleepCorrectionLabelRow>,
    comparisons: Vec<SleepWindowLabelComparison>,
    sleep_feature_report: SleepFeatureScoreReport,
    mut issues: Vec<String>,
    options: &SleepWindowLabelValidationOptions,
) -> SleepWindowLabelValidationReport {
    if labels.is_empty() {
        issues.push("no_sleep_window_labels_in_range".to_string());
    }
    if !labels.is_empty() && comparisons.is_empty() {
        issues.push("no_sleep_window_labels_met_confidence_threshold".to_string());
    }
    for comparison in &comparisons {
        for flag in &comparison.quality_flags {
            issues.push(format!("{}:{flag}", comparison.label_id));
        }
    }
    let distinct_compared_sleep_window_count = unique_sleep_window_count(&comparisons);
    if distinct_compared_sleep_window_count < comparisons.len() {
        issues.push("duplicate_reviewed_sleep_id".to_string());
    }
    issues.sort();
    issues.dedup();

    let passing_label_count = comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .count();
    let passing_comparisons = comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .cloned()
        .collect::<Vec<_>>();
    let distinct_passing_sleep_window_count = unique_sleep_window_count(&passing_comparisons);
    let pass = sleep_feature_report.pass
        && !comparisons.is_empty()
        && passing_label_count == comparisons.len()
        && issues.is_empty();
    let next_actions = sleep_window_label_validation_next_actions(&issues);

    SleepWindowLabelValidationReport {
        schema: SLEEP_WINDOW_LABEL_VALIDATION_SCHEMA.to_string(),
        generated_by: "goose-sleep-window-label-validator".to_string(),
        pass,
        start_time: start.to_string(),
        end_time: end.to_string(),
        label_count: labels.len(),
        compared_label_count: comparisons.len(),
        passing_label_count,
        distinct_compared_sleep_window_count,
        distinct_passing_sleep_window_count,
        sleep_window_available: sleep_feature_report.sleep_window.is_some(),
        acceptance_summary: sleep_window_label_acceptance_summary(
            labels.len(),
            &comparisons,
            options.min_label_confidence,
            options,
            issues.len(),
            0,
            0,
            next_actions.len(),
        ),
        sleep_feature_report,
        comparisons,
        issues,
        quality_flags: Vec::new(),
        errors: Vec::new(),
        next_actions,
        provenance: json!({
            "label_source": "sleep_correction_labels",
            "comparison_policy": SLEEP_WINDOW_LABEL_VALIDATION_POLICY,
            "validation_policy": SLEEP_WINDOW_LABEL_VALIDATION_POLICY,
            "start_tolerance_minutes": options.start_tolerance_minutes,
            "end_tolerance_minutes": options.end_tolerance_minutes,
            "duration_tolerance_minutes": options.duration_tolerance_minutes,
            "min_label_confidence": options.min_label_confidence,
            "distinct_window_policy": "one_hand_reviewed_sleep_window_per_sleep_id",
            "report_integrity_policy": SLEEP_WINDOW_LABEL_REPORT_INTEGRITY_POLICY,
        }),
    }
}

fn sleep_window_label_acceptance_summary(
    label_count: usize,
    comparisons: &[SleepWindowLabelComparison],
    min_label_confidence: f64,
    options: &SleepWindowLabelValidationOptions,
    issue_count: usize,
    quality_flag_count: usize,
    error_count: usize,
    next_action_count: usize,
) -> SleepWindowLabelAcceptanceSummary {
    let passing_comparisons = comparisons
        .iter()
        .filter(|comparison| comparison.pass)
        .cloned()
        .collect::<Vec<_>>();
    let mut accepted_sleep_ids = passing_comparisons
        .iter()
        .filter_map(|comparison| comparison.sleep_id.as_deref())
        .filter(|sleep_id| !sleep_id.trim().is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    accepted_sleep_ids.sort();
    let passing_label_count = passing_comparisons.len();
    let distinct_compared_sleep_window_count = unique_sleep_window_count(comparisons);
    let distinct_passing_sleep_window_count = unique_sleep_window_count(&passing_comparisons);
    SleepWindowLabelAcceptanceSummary {
        policy: "packet_sleep_window_must_match_distinct_hand_reviewed_nights".to_string(),
        hand_reviewed_sample_ready: !comparisons.is_empty()
            && label_count == comparisons.len()
            && passing_label_count == comparisons.len()
            && distinct_compared_sleep_window_count == comparisons.len()
            && distinct_passing_sleep_window_count == passing_label_count,
        label_count,
        compared_label_count: comparisons.len(),
        passing_label_count,
        distinct_compared_sleep_window_count,
        distinct_passing_sleep_window_count,
        required_release_distinct_passing_sleep_windows:
            default_min_hand_reviewed_window_comparisons(),
        accepted_sleep_ids,
        start_tolerance_minutes: options.start_tolerance_minutes,
        end_tolerance_minutes: options.end_tolerance_minutes,
        duration_tolerance_minutes: options.duration_tolerance_minutes,
        min_label_confidence,
        min_observed_label_confidence: passing_comparisons
            .iter()
            .filter_map(|comparison| comparison.confidence)
            .fold(1.0, f64::min),
        max_start_delta_minutes: comparisons
            .iter()
            .map(|comparison| comparison.start_delta_minutes)
            .fold(0.0, f64::max),
        max_end_delta_minutes: comparisons
            .iter()
            .map(|comparison| comparison.end_delta_minutes)
            .fold(0.0, f64::max),
        max_duration_delta_minutes: comparisons
            .iter()
            .map(|comparison| comparison.duration_delta_minutes)
            .fold(0.0, f64::max),
        issue_count,
        quality_flag_count,
        error_count,
        next_action_count,
    }
}

fn unique_sleep_window_count(comparisons: &[SleepWindowLabelComparison]) -> usize {
    comparisons
        .iter()
        .filter_map(|comparison| comparison.sleep_id.as_deref())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .len()
}

fn sleep_window_label_validation_next_actions(
    issues: &[String],
) -> Vec<SleepWindowLabelValidationNextAction> {
    issues
        .iter()
        .map(|issue| {
            let reason = issue
                .split_once(':')
                .map_or(issue.as_str(), |(_, reason)| reason);
            SleepWindowLabelValidationNextAction {
                scope: sleep_window_label_validation_issue_scope(issue, reason).to_string(),
                reason: reason.to_string(),
                action: sleep_window_label_validation_action(reason),
            }
        })
        .collect()
}

fn sleep_window_label_validation_issue_scope(issue: &str, reason: &str) -> &'static str {
    if issue.contains(':')
        || matches!(
            reason,
            "no_sleep_window_labels_in_range"
                | "no_sleep_window_labels_met_confidence_threshold"
                | "label_confidence_missing"
                | "label_confidence_out_of_range"
                | "label_confidence_below_threshold"
                | "label_review_policy_missing"
                | "label_provenance_source_missing"
                | "label_provenance_source_mismatch"
                | "label_sleep_id_missing"
                | "duplicate_reviewed_sleep_id"
                | "label_window_invalid"
                | "label_corrected_start_time_invalid"
                | "label_corrected_end_time_invalid"
        )
    {
        "sleep_window.labels"
    } else if matches!(
        reason,
        "sleep_window_start_outside_tolerance"
            | "sleep_window_end_outside_tolerance"
            | "sleep_window_duration_outside_tolerance"
            | "sleep_window_missing"
    ) {
        "sleep_window.detection"
    } else {
        "sleep_window_validation"
    }
}

fn sleep_window_label_validation_action(reason: &str) -> String {
    match reason {
        "sleep_window_missing" => {
            "Fix packet-derived sleep window extraction before comparing labels.".to_string()
        }
        "no_sleep_window_labels_in_range" => {
            "Add hand-reviewed sleep-window correction labels for this night before promoting Sleep V1.".to_string()
        }
        "no_sleep_window_labels_met_confidence_threshold" => {
            "Review label confidence or collect higher-confidence manual sleep-window labels.".to_string()
        }
        "label_confidence_missing" => {
            "Attach explicit reviewer confidence to the hand-reviewed sleep-window label before using it for promotion.".to_string()
        }
        "label_confidence_out_of_range" => {
            "Set reviewer confidence to a finite value between 0.0 and 1.0 before using the sleep-window label for promotion.".to_string()
        }
        "label_confidence_below_threshold" => {
            "Review the sleep-window label or collect a higher-confidence hand-reviewed label for this night.".to_string()
        }
        "label_review_policy_missing" => {
            "Mark the correction label provenance as a hand-reviewed sleep window before using it for promotion.".to_string()
        }
        "label_provenance_source_missing" => {
            "Store the hand-reviewed sleep-window label source in provenance before validation.".to_string()
        }
        "label_provenance_source_mismatch" => {
            "Keep the sleep-window label source and provenance source identical before validation.".to_string()
        }
        "label_sleep_id_missing" => {
            "Attach the reviewed sleep_id to the hand-reviewed sleep-window label so the comparison is tied to a concrete night.".to_string()
        }
        "duplicate_reviewed_sleep_id" => {
            "Keep one reviewed sleep-window label per sleep_id, or split the evidence into distinct reviewed nights before promotion.".to_string()
        }
        "sleep_window_start_outside_tolerance" => {
            "Inspect sleep onset detection and tune motion/HR thresholds against the reviewed start label.".to_string()
        }
        "sleep_window_end_outside_tolerance" => {
            "Inspect wake detection and tune motion/HR thresholds against the reviewed end label.".to_string()
        }
        "sleep_window_duration_outside_tolerance" => {
            "Inspect both sleep onset and final wake detection before trusting duration metrics.".to_string()
        }
        "label_window_invalid" => {
            "Fix the hand-reviewed sleep-window label so its end is after its start.".to_string()
        }
        "label_corrected_start_time_invalid" => {
            "Store corrected_start_time_unix_ms as an integer Unix timestamp before using the sleep-window label for promotion.".to_string()
        }
        "label_corrected_end_time_invalid" => {
            "Store corrected_end_time_unix_ms as an integer Unix timestamp before using the sleep-window label for promotion.".to_string()
        }
        _ => "Inspect the sleep-window validation issue and repair labels or packet-derived extraction.".to_string(),
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

fn default_min_hand_reviewed_window_comparisons() -> usize {
    3
}

fn default_min_stage_label_comparisons() -> usize {
    1
}

fn default_min_benchmark_comparisons() -> usize {
    1
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
