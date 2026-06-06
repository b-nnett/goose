use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{GooseError, GooseResult};

pub const UI_COVERAGE_AUDIT_SCHEMA: &str = "goose.ui-coverage-audit.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageAuditInput {
    pub schema: String,
    pub inventory: UiCoverageInventoryPaths,
    #[serde(default)]
    pub expected_inventory: Option<UiCoverageExpectedInventory>,
    pub coverage: UiCoverageRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageInventoryPaths {
    pub navigation_destinations_csv: String,
    pub layouts_csv: String,
    pub ui_resources_csv: String,
    pub source_ui_classes_csv: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageExpectedInventory {
    #[serde(default)]
    pub navigation_count: Option<usize>,
    #[serde(default)]
    pub layout_count: Option<usize>,
    #[serde(default)]
    pub ui_resource_count: Option<usize>,
    #[serde(default)]
    pub source_class_count: Option<usize>,
    #[serde(default)]
    pub navigation_destinations_sha256: Option<String>,
    #[serde(default)]
    pub layouts_sha256: Option<String>,
    #[serde(default)]
    pub ui_resources_sha256: Option<String>,
    #[serde(default)]
    pub source_ui_classes_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageRules {
    #[serde(default)]
    pub navigation: Vec<NavigationCoverageRule>,
    #[serde(default)]
    pub layouts: Vec<LayoutCoverageRule>,
    #[serde(default)]
    pub ui_resources: Vec<UiResourceCoverageRule>,
    #[serde(default)]
    pub source_classes: Vec<SourceClassCoverageRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavigationCoverageRule {
    pub rule_id: String,
    #[serde(default)]
    pub graph: Option<String>,
    #[serde(default)]
    pub destination_id: Option<String>,
    #[serde(default)]
    pub destination_type: Option<String>,
    #[serde(default)]
    pub class_or_graph: Option<String>,
    #[serde(default)]
    pub class_prefix: Option<String>,
    pub status: UiCoverageStatus,
    pub goose_area: String,
    #[serde(default)]
    pub target_level: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutCoverageRule {
    pub rule_id: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub status: UiCoverageStatus,
    pub goose_area: String,
    #[serde(default)]
    pub target_level: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiResourceCoverageRule {
    pub rule_id: String,
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
    pub status: UiCoverageStatus,
    pub goose_area: String,
    #[serde(default)]
    pub target_level: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceClassCoverageRule {
    pub rule_id: String,
    #[serde(default)]
    pub class_name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    pub status: UiCoverageStatus,
    pub goose_area: String,
    #[serde(default)]
    pub target_level: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum UiCoverageStatus {
    Implement,
    ApproximateLocally,
    DebugOnly,
    Defer,
    Omit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageAuditReport {
    pub schema: String,
    pub generated_by: String,
    pub inventory_valid: bool,
    pub coverage_map_valid: bool,
    pub all_surfaces_classified: bool,
    pub has_deferred_review_debt: bool,
    pub pass: bool,
    pub inventory: UiCoverageInventorySummary,
    pub navigation: UiCoverageBucketReport,
    pub layouts: UiCoverageBucketReport,
    pub ui_resources: UiCoverageBucketReport,
    pub source_classes: UiCoverageBucketReport,
    pub rule_matches: Vec<UiCoverageRuleMatch>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<UiCoverageNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageInventorySummary {
    pub navigation_count: usize,
    pub layout_count: usize,
    pub ui_resource_count: usize,
    pub source_class_count: usize,
    pub navigation_destinations_sha256: String,
    pub layouts_sha256: String,
    pub ui_resources_sha256: String,
    pub source_ui_classes_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageBucketReport {
    pub total_count: usize,
    pub covered_count: usize,
    pub missing_count: usize,
    pub deferred_count: usize,
    pub status_counts: BTreeMap<String, usize>,
    pub missing_surfaces: Vec<MissingUiSurface>,
    pub deferred_surfaces: Vec<DeferredUiSurface>,
    #[serde(default)]
    pub next_actions: Vec<UiCoverageNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissingUiSurface {
    pub key: String,
    pub group: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeferredUiSurface {
    pub key: String,
    pub group: String,
    pub path: String,
    pub rule_id: String,
    pub goose_area: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiCoverageRuleMatch {
    pub surface_kind: String,
    pub rule_id: String,
    pub match_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct UiCoverageNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

type CsvRow = BTreeMap<String, String>;

pub fn run_ui_coverage_audit(
    input: &UiCoverageAuditInput,
    base_dir: &Path,
) -> GooseResult<UiCoverageAuditReport> {
    let mut issues = Vec::new();
    if input.schema != UI_COVERAGE_AUDIT_SCHEMA {
        issues.push(format!("unsupported_schema:{}", input.schema));
    }

    let navigation_path = resolve_path(base_dir, &input.inventory.navigation_destinations_csv);
    let layouts_path = resolve_path(base_dir, &input.inventory.layouts_csv);
    let ui_resources_path = resolve_path(base_dir, &input.inventory.ui_resources_csv);
    let source_classes_path = resolve_path(base_dir, &input.inventory.source_ui_classes_csv);

    let navigation_rows = load_csv_rows(
        &navigation_path,
        &["graph", "type", "id", "class_or_graph", "path"],
        &mut issues,
    )?;
    let layout_rows = load_csv_rows(
        &layouts_path,
        &["resource", "category", "path"],
        &mut issues,
    )?;
    let ui_resource_rows = load_csv_rows(
        &ui_resources_path,
        &["type", "variant", "resource", "path"],
        &mut issues,
    )?;
    let source_class_rows = load_csv_rows(
        &source_classes_path,
        &["class_name", "category", "module", "path"],
        &mut issues,
    )?;
    let inventory_summary = UiCoverageInventorySummary {
        navigation_count: navigation_rows.len(),
        layout_count: layout_rows.len(),
        ui_resource_count: ui_resource_rows.len(),
        source_class_count: source_class_rows.len(),
        navigation_destinations_sha256: file_sha256(&navigation_path)?,
        layouts_sha256: file_sha256(&layouts_path)?,
        ui_resources_sha256: file_sha256(&ui_resources_path)?,
        source_ui_classes_sha256: file_sha256(&source_classes_path)?,
    };
    validate_expected_inventory(
        input.expected_inventory.as_ref(),
        &inventory_summary,
        &mut issues,
    );

    let mut rule_matches = initialize_rule_matches(&input.coverage);
    validate_rules(&input.coverage, &mut issues);

    let navigation = audit_navigation(
        &navigation_rows,
        &input.coverage.navigation,
        &mut rule_matches,
        &mut issues,
    );
    let layouts = audit_layouts(
        &layout_rows,
        &input.coverage.layouts,
        &mut rule_matches,
        &mut issues,
    );
    let ui_resources = audit_ui_resources(
        &ui_resource_rows,
        &input.coverage.ui_resources,
        &mut rule_matches,
        &mut issues,
    );
    let source_classes = audit_source_classes(
        &source_class_rows,
        &input.coverage.source_classes,
        &mut rule_matches,
        &mut issues,
    );

    for ((surface_kind, rule_id), match_count) in &rule_matches {
        if *match_count == 0 {
            issues.push(format!(
                "coverage_rule_matched_no_surfaces:{surface_kind}:{rule_id}"
            ));
        }
    }

    issues.sort();
    issues.dedup();
    let inventory_valid = issues
        .iter()
        .all(|issue| !ui_coverage_inventory_issue(issue));
    let coverage_map_valid = issues
        .iter()
        .all(|issue| !ui_coverage_coverage_map_issue(issue));
    let all_surfaces_classified = navigation.missing_count == 0
        && layouts.missing_count == 0
        && ui_resources.missing_count == 0
        && source_classes.missing_count == 0;
    let has_deferred_review_debt = navigation.deferred_count > 0
        || layouts.deferred_count > 0
        || ui_resources.deferred_count > 0
        || source_classes.deferred_count > 0;
    let pass = issues.is_empty()
        && navigation.missing_count == 0
        && layouts.missing_count == 0
        && ui_resources.missing_count == 0
        && source_classes.missing_count == 0;
    let next_actions = ui_coverage_report_next_actions(
        &issues,
        &navigation,
        &layouts,
        &ui_resources,
        &source_classes,
    );

    Ok(UiCoverageAuditReport {
        schema: "goose.ui-coverage-audit-report.v1".to_string(),
        generated_by: "goose-ui-coverage-audit".to_string(),
        inventory_valid,
        coverage_map_valid,
        all_surfaces_classified,
        has_deferred_review_debt,
        pass,
        inventory: inventory_summary,
        navigation,
        layouts,
        ui_resources,
        source_classes,
        rule_matches: rule_matches
            .into_iter()
            .map(
                |((surface_kind, rule_id), match_count)| UiCoverageRuleMatch {
                    surface_kind,
                    rule_id,
                    match_count,
                },
            )
            .collect(),
        issues,
        next_actions,
    })
}

fn audit_navigation(
    rows: &[CsvRow],
    rules: &[NavigationCoverageRule],
    rule_matches: &mut BTreeMap<(String, String), usize>,
    issues: &mut Vec<String>,
) -> UiCoverageBucketReport {
    let mut bucket = empty_bucket(rows.len());

    for row in rows {
        let key = format!(
            "{}:{}",
            row_value(row, "graph"),
            row_value(row, "id").if_empty(row_value(row, "class_or_graph"))
        );
        let group = row_value(row, "graph");
        let path = row_value(row, "path");
        if let Some(rule) = rules.iter().find(|rule| rule.matches(row)) {
            mark_covered(&mut bucket, rule.status);
            record_deferred_surface(
                &mut bucket,
                rule.status,
                &key,
                group,
                path,
                &rule.rule_id,
                &rule.goose_area,
                rule.reason.as_deref(),
            );
            increment_rule_match(rule_matches, "navigation", &rule.rule_id);
        } else {
            issues.push(format!("navigation_missing_coverage:{key}"));
            bucket.missing_surfaces.push(MissingUiSurface {
                key,
                group: group.to_string(),
                path: path.to_string(),
            });
        }
    }
    finalize_bucket("navigation", &mut bucket);
    bucket
}

fn audit_layouts(
    rows: &[CsvRow],
    rules: &[LayoutCoverageRule],
    rule_matches: &mut BTreeMap<(String, String), usize>,
    issues: &mut Vec<String>,
) -> UiCoverageBucketReport {
    let mut bucket = empty_bucket(rows.len());

    for row in rows {
        let key = row_value(row, "resource");
        let group = row_value(row, "category");
        let path = row_value(row, "path");
        if let Some(rule) = rules.iter().find(|rule| rule.matches(row)) {
            mark_covered(&mut bucket, rule.status);
            record_deferred_surface(
                &mut bucket,
                rule.status,
                key,
                group,
                path,
                &rule.rule_id,
                &rule.goose_area,
                rule.reason.as_deref(),
            );
            increment_rule_match(rule_matches, "layout", &rule.rule_id);
        } else {
            let key = key.to_string();
            issues.push(format!("layout_missing_coverage:{key}"));
            bucket.missing_surfaces.push(MissingUiSurface {
                key,
                group: group.to_string(),
                path: path.to_string(),
            });
        }
    }
    finalize_bucket("layout", &mut bucket);
    bucket
}

fn audit_ui_resources(
    rows: &[CsvRow],
    rules: &[UiResourceCoverageRule],
    rule_matches: &mut BTreeMap<(String, String), usize>,
    issues: &mut Vec<String>,
) -> UiCoverageBucketReport {
    let mut bucket = empty_bucket(rows.len());

    for row in rows {
        let resource_type = row_value(row, "type");
        let resource = row_value(row, "resource");
        let key = format!("{resource_type}:{resource}");
        let path = row_value(row, "path");
        if let Some(rule) = rules.iter().find(|rule| rule.matches(row)) {
            mark_covered(&mut bucket, rule.status);
            record_deferred_surface(
                &mut bucket,
                rule.status,
                &key,
                resource_type,
                path,
                &rule.rule_id,
                &rule.goose_area,
                rule.reason.as_deref(),
            );
            increment_rule_match(rule_matches, "ui_resource", &rule.rule_id);
        } else {
            issues.push(format!("ui_resource_missing_coverage:{key}"));
            bucket.missing_surfaces.push(MissingUiSurface {
                key,
                group: resource_type.to_string(),
                path: path.to_string(),
            });
        }
    }
    finalize_bucket("ui_resource", &mut bucket);
    bucket
}

fn audit_source_classes(
    rows: &[CsvRow],
    rules: &[SourceClassCoverageRule],
    rule_matches: &mut BTreeMap<(String, String), usize>,
    issues: &mut Vec<String>,
) -> UiCoverageBucketReport {
    let mut bucket = empty_bucket(rows.len());

    for row in rows {
        let key = row_value(row, "class_name");
        let group = row_value(row, "module");
        let path = row_value(row, "path");
        if let Some(rule) = rules.iter().find(|rule| rule.matches(row)) {
            mark_covered(&mut bucket, rule.status);
            record_deferred_surface(
                &mut bucket,
                rule.status,
                key,
                group,
                path,
                &rule.rule_id,
                &rule.goose_area,
                rule.reason.as_deref(),
            );
            increment_rule_match(rule_matches, "source_class", &rule.rule_id);
        } else {
            let key = key.to_string();
            issues.push(format!("source_class_missing_coverage:{key}"));
            bucket.missing_surfaces.push(MissingUiSurface {
                key,
                group: group.to_string(),
                path: path.to_string(),
            });
        }
    }
    finalize_bucket("source_class", &mut bucket);
    bucket
}

fn empty_bucket(total_count: usize) -> UiCoverageBucketReport {
    UiCoverageBucketReport {
        total_count,
        covered_count: 0,
        missing_count: 0,
        deferred_count: 0,
        status_counts: BTreeMap::new(),
        missing_surfaces: Vec::new(),
        deferred_surfaces: Vec::new(),
        next_actions: Vec::new(),
    }
}

fn finalize_bucket(surface_kind: &str, bucket: &mut UiCoverageBucketReport) {
    bucket.missing_count = bucket.missing_surfaces.len();
    bucket.deferred_count = bucket.deferred_surfaces.len();
    bucket.next_actions = ui_coverage_bucket_next_actions(surface_kind, bucket);
}

fn mark_covered(bucket: &mut UiCoverageBucketReport, status: UiCoverageStatus) {
    bucket.covered_count += 1;
    *bucket
        .status_counts
        .entry(status.as_str().to_string())
        .or_default() += 1;
}

fn record_deferred_surface(
    bucket: &mut UiCoverageBucketReport,
    status: UiCoverageStatus,
    key: &str,
    group: &str,
    path: &str,
    rule_id: &str,
    goose_area: &str,
    reason: Option<&str>,
) {
    if status != UiCoverageStatus::Defer {
        return;
    }
    bucket.deferred_surfaces.push(DeferredUiSurface {
        key: key.to_string(),
        group: group.to_string(),
        path: path.to_string(),
        rule_id: rule_id.to_string(),
        goose_area: goose_area.to_string(),
        reason: reason.unwrap_or_default().to_string(),
    });
}

fn initialize_rule_matches(coverage: &UiCoverageRules) -> BTreeMap<(String, String), usize> {
    let mut matches = BTreeMap::new();
    for rule in &coverage.navigation {
        matches.insert(("navigation".to_string(), rule.rule_id.clone()), 0);
    }
    for rule in &coverage.layouts {
        matches.insert(("layout".to_string(), rule.rule_id.clone()), 0);
    }
    for rule in &coverage.ui_resources {
        matches.insert(("ui_resource".to_string(), rule.rule_id.clone()), 0);
    }
    for rule in &coverage.source_classes {
        matches.insert(("source_class".to_string(), rule.rule_id.clone()), 0);
    }
    matches
}

fn increment_rule_match(
    rule_matches: &mut BTreeMap<(String, String), usize>,
    surface_kind: &str,
    rule_id: &str,
) {
    *rule_matches
        .entry((surface_kind.to_string(), rule_id.to_string()))
        .or_default() += 1;
}

fn ui_coverage_report_next_actions(
    issues: &[String],
    navigation: &UiCoverageBucketReport,
    layouts: &UiCoverageBucketReport,
    ui_resources: &UiCoverageBucketReport,
    source_classes: &UiCoverageBucketReport,
) -> Vec<UiCoverageNextAction> {
    let mut actions = BTreeSet::new();
    actions.extend(navigation.next_actions.iter().cloned());
    actions.extend(layouts.next_actions.iter().cloned());
    actions.extend(ui_resources.next_actions.iter().cloned());
    actions.extend(source_classes.next_actions.iter().cloned());
    for issue in issues {
        actions.insert(ui_coverage_issue_next_action(issue));
    }
    actions.into_iter().collect()
}

fn ui_coverage_bucket_next_actions(
    surface_kind: &str,
    bucket: &UiCoverageBucketReport,
) -> Vec<UiCoverageNextAction> {
    let mut actions = BTreeSet::new();
    for surface in &bucket.missing_surfaces {
        actions.insert(UiCoverageNextAction {
            scope: format!("{surface_kind}:{}", surface.key),
            reason: format!("{surface_kind}_missing_coverage"),
            action: format!(
                "Add a coverage-map rule for {surface_kind} {} with implement, approximate_locally, debug_only, defer, or omit status; include target_level or reason, then rerun UI coverage audit.",
                surface.key
            ),
        });
    }
    for surface in &bucket.deferred_surfaces {
        actions.insert(UiCoverageNextAction {
            scope: format!("{surface_kind}:{}", surface.key),
            reason: "deferred_review_debt".to_string(),
            action: format!(
                "Review deferred {surface_kind} {} from rule {}; implement or approximate it locally, keep it debug-only, omit it with reason, or keep defer with a concrete follow-up.",
                surface.key, surface.rule_id
            ),
        });
    }
    actions.into_iter().collect()
}

fn ui_coverage_issue_next_action(issue: &str) -> UiCoverageNextAction {
    let parts = issue.split(':').collect::<Vec<_>>();
    let issue_kind = parts.first().copied().unwrap_or(issue);
    let scope = if parts.len() > 1 {
        parts[1..].join(":")
    } else {
        "coverage-map".to_string()
    };
    let action = match issue_kind {
        "unsupported_schema" => {
            "Update the coverage-map schema to goose.ui-coverage-audit.v1, then rerun UI coverage audit."
        }
        "navigation_rule_id_required"
        | "layout_rule_id_required"
        | "ui_resource_rule_id_required"
        | "source_class_rule_id_required" => {
            "Assign a stable rule_id for this coverage-map rule, then rerun UI coverage audit."
        }
        "navigation_rule_selector_required"
        | "layout_rule_selector_required"
        | "ui_resource_rule_selector_required"
        | "source_class_rule_selector_required" => {
            "Add a selector to the rule so it matches a concrete APK surface, then rerun UI coverage audit."
        }
        "navigation_rule_goose_area_required"
        | "layout_rule_goose_area_required"
        | "ui_resource_rule_goose_area_required"
        | "source_class_rule_goose_area_required" => {
            "Set goose_area to the Goose screen or area responsible for this surface, then rerun UI coverage audit."
        }
        "navigation_rule_reason_required"
        | "layout_rule_reason_required"
        | "ui_resource_rule_reason_required"
        | "source_class_rule_reason_required" => {
            "Add the omit/defer reason explaining why this APK surface is not implemented now, then rerun UI coverage audit."
        }
        "navigation_rule_target_level_required"
        | "layout_rule_target_level_required"
        | "ui_resource_rule_target_level_required"
        | "source_class_rule_target_level_required" => {
            "Set target_level for the implemented, approximate, or debug-only Goose coverage target, then rerun UI coverage audit."
        }
        "coverage_rule_matched_no_surfaces" => {
            "Remove the stale coverage rule or update its selector to match the current APK inventory, then rerun UI coverage audit."
        }
        "inventory_count_changed" | "inventory_checksum_changed" => {
            "Review the regenerated APK UI inventory diff, update coverage-map rules for any new/removed surfaces, then refresh expected_inventory counts and checksums."
        }
        "csv_empty" | "csv_missing_header" | "csv_row_width_mismatch" => {
            "Regenerate the APK UI inventory CSVs with tools/analysis/extract_apk_ui_inventory.py, then rerun UI coverage audit."
        }
        _ if issue_kind.ends_with("_missing_coverage") => {
            "Add or update a coverage-map rule for this APK surface with a status, Goose area, target level or reason, then rerun UI coverage audit."
        }
        _ => {
            "Resolve the UI coverage audit issue, update the coverage map or inventory, then rerun UI coverage audit."
        }
    };
    UiCoverageNextAction {
        scope,
        reason: issue_kind.to_string(),
        action: action.to_string(),
    }
}

fn ui_coverage_inventory_issue(issue: &str) -> bool {
    issue.starts_with("inventory_count_changed:")
        || issue.starts_with("inventory_checksum_changed:")
        || issue.starts_with("csv_empty:")
        || issue.starts_with("csv_missing_header:")
        || issue.starts_with("csv_row_width_mismatch:")
}

fn ui_coverage_coverage_map_issue(issue: &str) -> bool {
    issue.starts_with("unsupported_schema:")
        || issue.starts_with("coverage_rule_matched_no_surfaces:")
        || issue.starts_with("navigation_rule_")
        || issue.starts_with("layout_rule_")
        || issue.starts_with("ui_resource_rule_")
        || issue.starts_with("source_class_rule_")
}

fn validate_rules(coverage: &UiCoverageRules, issues: &mut Vec<String>) {
    for rule in &coverage.navigation {
        if rule.rule_id.trim().is_empty() {
            issues.push("navigation_rule_id_required".to_string());
        }
        if rule.graph.is_none()
            && rule.destination_id.is_none()
            && rule.destination_type.is_none()
            && rule.class_or_graph.is_none()
            && rule.class_prefix.is_none()
        {
            issues.push(format!(
                "navigation_rule_selector_required:{}",
                rule.rule_id
            ));
        }
        validate_decision(
            "navigation",
            &rule.rule_id,
            rule.status,
            &rule.goose_area,
            rule.target_level.as_deref(),
            rule.reason.as_deref(),
            issues,
        );
    }

    for rule in &coverage.layouts {
        if rule.rule_id.trim().is_empty() {
            issues.push("layout_rule_id_required".to_string());
        }
        if rule.resource.is_none() && rule.category.is_none() {
            issues.push(format!("layout_rule_selector_required:{}", rule.rule_id));
        }
        validate_decision(
            "layout",
            &rule.rule_id,
            rule.status,
            &rule.goose_area,
            rule.target_level.as_deref(),
            rule.reason.as_deref(),
            issues,
        );
    }

    for rule in &coverage.ui_resources {
        if rule.rule_id.trim().is_empty() {
            issues.push("ui_resource_rule_id_required".to_string());
        }
        if rule.resource_type.is_none() && rule.variant.is_none() && rule.resource.is_none() {
            issues.push(format!(
                "ui_resource_rule_selector_required:{}",
                rule.rule_id
            ));
        }
        validate_decision(
            "ui_resource",
            &rule.rule_id,
            rule.status,
            &rule.goose_area,
            rule.target_level.as_deref(),
            rule.reason.as_deref(),
            issues,
        );
    }

    for rule in &coverage.source_classes {
        if rule.rule_id.trim().is_empty() {
            issues.push("source_class_rule_id_required".to_string());
        }
        if rule.class_name.is_none() && rule.category.is_none() && rule.module.is_none() {
            issues.push(format!(
                "source_class_rule_selector_required:{}",
                rule.rule_id
            ));
        }
        validate_decision(
            "source_class",
            &rule.rule_id,
            rule.status,
            &rule.goose_area,
            rule.target_level.as_deref(),
            rule.reason.as_deref(),
            issues,
        );
    }
}

fn validate_decision(
    surface_kind: &str,
    rule_id: &str,
    status: UiCoverageStatus,
    goose_area: &str,
    target_level: Option<&str>,
    reason: Option<&str>,
    issues: &mut Vec<String>,
) {
    if goose_area.trim().is_empty() {
        issues.push(format!("{surface_kind}_rule_goose_area_required:{rule_id}"));
    }
    if status.requires_reason() && reason.is_none_or(|value| value.trim().is_empty()) {
        issues.push(format!("{surface_kind}_rule_reason_required:{rule_id}"));
    }
    if !status.requires_reason() && target_level.is_none_or(|value| value.trim().is_empty()) {
        issues.push(format!(
            "{surface_kind}_rule_target_level_required:{rule_id}"
        ));
    }
}

fn load_csv_rows(
    path: &Path,
    required_headers: &[&str],
    issues: &mut Vec<String>,
) -> GooseResult<Vec<CsvRow>> {
    let raw = fs::read_to_string(path).map_err(|source| GooseError::io(path, source))?;
    let mut lines = raw.lines();
    let Some(header_line) = lines.next() else {
        issues.push(format!("csv_empty:{}", path.display()));
        return Ok(Vec::new());
    };
    let headers = parse_csv_record(header_line);
    for required in required_headers {
        if !headers.iter().any(|header| header == required) {
            issues.push(format!("csv_missing_header:{}:{required}", path.display()));
        }
    }

    let mut rows = Vec::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = parse_csv_record(line);
        if fields.len() != headers.len() {
            issues.push(format!(
                "csv_row_width_mismatch:{}:{}",
                path.display(),
                index + 2
            ));
            continue;
        }
        rows.push(
            headers
                .iter()
                .cloned()
                .zip(fields)
                .collect::<BTreeMap<_, _>>(),
        );
    }
    Ok(rows)
}

fn file_sha256(path: &Path) -> GooseResult<String> {
    let bytes = fs::read(path).map_err(|source| GooseError::io(path, source))?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("{digest:x}"))
}

fn validate_expected_inventory(
    expected: Option<&UiCoverageExpectedInventory>,
    actual: &UiCoverageInventorySummary,
    issues: &mut Vec<String>,
) {
    let Some(expected) = expected else {
        return;
    };
    validate_expected_count(
        "navigation",
        expected.navigation_count,
        actual.navigation_count,
        issues,
    );
    validate_expected_count("layout", expected.layout_count, actual.layout_count, issues);
    validate_expected_count(
        "ui_resource",
        expected.ui_resource_count,
        actual.ui_resource_count,
        issues,
    );
    validate_expected_count(
        "source_class",
        expected.source_class_count,
        actual.source_class_count,
        issues,
    );
    validate_expected_sha256(
        "navigation_destinations_csv",
        expected.navigation_destinations_sha256.as_deref(),
        &actual.navigation_destinations_sha256,
        issues,
    );
    validate_expected_sha256(
        "layouts_csv",
        expected.layouts_sha256.as_deref(),
        &actual.layouts_sha256,
        issues,
    );
    validate_expected_sha256(
        "ui_resources_csv",
        expected.ui_resources_sha256.as_deref(),
        &actual.ui_resources_sha256,
        issues,
    );
    validate_expected_sha256(
        "source_ui_classes_csv",
        expected.source_ui_classes_sha256.as_deref(),
        &actual.source_ui_classes_sha256,
        issues,
    );
}

fn validate_expected_count(
    surface_kind: &str,
    expected: Option<usize>,
    actual: usize,
    issues: &mut Vec<String>,
) {
    if let Some(expected) = expected
        && expected != actual
    {
        issues.push(format!(
            "inventory_count_changed:{surface_kind}:expected_{expected}:actual_{actual}"
        ));
    }
}

fn validate_expected_sha256(
    inventory_file: &str,
    expected: Option<&str>,
    actual: &str,
    issues: &mut Vec<String>,
) {
    if let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty())
        && !expected.eq_ignore_ascii_case(actual)
    {
        issues.push(format!(
            "inventory_checksum_changed:{inventory_file}:expected_{expected}:actual_{actual}"
        ));
    }
}

fn parse_csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(field);
                field = String::new();
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn resolve_path(base_dir: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn row_value<'a>(row: &'a CsvRow, key: &str) -> &'a str {
    row.get(key).map(String::as_str).unwrap_or_default()
}

trait EmptyFallback<'a> {
    fn if_empty(self, fallback: &'a str) -> &'a str;
}

impl<'a> EmptyFallback<'a> for &'a str {
    fn if_empty(self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

impl NavigationCoverageRule {
    fn matches(&self, row: &CsvRow) -> bool {
        selector_matches(&self.graph, row_value(row, "graph"))
            && selector_matches(&self.destination_id, row_value(row, "id"))
            && selector_matches(&self.destination_type, row_value(row, "type"))
            && selector_matches(&self.class_or_graph, row_value(row, "class_or_graph"))
            && prefix_matches(&self.class_prefix, row_value(row, "class_or_graph"))
    }
}

impl LayoutCoverageRule {
    fn matches(&self, row: &CsvRow) -> bool {
        selector_matches(&self.resource, row_value(row, "resource"))
            && selector_matches(&self.category, row_value(row, "category"))
    }
}

impl UiResourceCoverageRule {
    fn matches(&self, row: &CsvRow) -> bool {
        selector_matches(&self.resource_type, row_value(row, "type"))
            && selector_matches(&self.variant, row_value(row, "variant"))
            && selector_matches(&self.resource, row_value(row, "resource"))
    }
}

impl SourceClassCoverageRule {
    fn matches(&self, row: &CsvRow) -> bool {
        selector_matches(&self.class_name, row_value(row, "class_name"))
            && selector_matches(&self.category, row_value(row, "category"))
            && selector_matches(&self.module, row_value(row, "module"))
    }
}

impl UiCoverageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::ApproximateLocally => "approximate_locally",
            Self::DebugOnly => "debug_only",
            Self::Defer => "defer",
            Self::Omit => "omit",
        }
    }

    fn requires_reason(self) -> bool {
        matches!(self, Self::Defer | Self::Omit)
    }
}

fn selector_matches(selector: &Option<String>, value: &str) -> bool {
    selector.as_deref().is_none_or(|selector| selector == value)
}

fn prefix_matches(prefix: &Option<String>, value: &str) -> bool {
    prefix
        .as_deref()
        .is_none_or(|prefix| value.starts_with(prefix))
}
