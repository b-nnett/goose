use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    GooseResult,
    protocol::{DeviceType, parse_frame_hex},
    store::{
        CURRENT_SCHEMA_VERSION, DecodedFrameInput, GooseStore, RawEvidenceInput, known_tables,
    },
};

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";

#[derive(Debug, Clone)]
pub struct StorageCheckOptions<'a> {
    pub database_path: &'a Path,
    pub run_self_test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCheckReport {
    pub schema: String,
    pub generated_by: String,
    pub database_path: String,
    pub pass: bool,
    #[serde(default)]
    pub schema_version_valid: bool,
    #[serde(default)]
    pub foreign_keys_valid: bool,
    #[serde(default)]
    pub integrity_valid: bool,
    #[serde(default)]
    pub tables_present: bool,
    #[serde(default)]
    pub required_columns_present: bool,
    #[serde(default)]
    pub row_counts_ready: bool,
    #[serde(default)]
    pub self_test_ready: bool,
    #[serde(default)]
    pub storage_ready: bool,
    pub expected_schema_version: i64,
    pub actual_schema_version: i64,
    pub foreign_keys_enabled: bool,
    pub integrity_check: String,
    pub tables: Vec<StorageTableCheck>,
    pub self_test: Option<StorageSelfTestReport>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<StorageCheckNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTableCheck {
    pub table: String,
    pub exists: bool,
    pub row_count: Option<i64>,
    pub columns: Vec<String>,
    pub missing_columns: Vec<String>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<StorageCheckNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSelfTestReport {
    pub ran: bool,
    pub raw_inserted: bool,
    pub raw_idempotent: bool,
    pub decoded_inserted: bool,
    pub query_roundtrip: bool,
    pub foreign_key_rejected: bool,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<StorageCheckNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageCheckNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

pub fn check_storage_database(options: StorageCheckOptions<'_>) -> GooseResult<StorageCheckReport> {
    let store = GooseStore::open(options.database_path)?;
    let actual_schema_version = store.schema_version()?;
    let foreign_keys_enabled = store.foreign_keys_enabled()?;
    let integrity_check = store.integrity_check()?;
    let mut issues = Vec::new();

    if actual_schema_version != CURRENT_SCHEMA_VERSION {
        issues.push(format!(
            "expected schema version {CURRENT_SCHEMA_VERSION}, got {actual_schema_version}"
        ));
    }
    if !foreign_keys_enabled {
        issues.push("SQLite foreign key enforcement is disabled".to_string());
    }
    if integrity_check != "ok" {
        issues.push(format!("SQLite integrity_check returned {integrity_check}"));
    }

    let tables = required_columns()
        .iter()
        .map(|(table, columns)| check_table(&store, table, columns))
        .collect::<Vec<_>>();
    for table in &tables {
        if !table.issues.is_empty() {
            issues.extend(
                table
                    .issues
                    .iter()
                    .map(|issue| format!("{}: {issue}", table.table)),
            );
        }
    }

    let self_test = if options.run_self_test {
        let report = run_storage_self_test(&store);
        if !report.issues.is_empty() {
            issues.extend(
                report
                    .issues
                    .iter()
                    .map(|issue| format!("self-test: {issue}")),
            );
        }
        Some(report)
    } else {
        None
    };
    let next_actions = storage_check_report_next_actions(&issues, &tables, &self_test);
    let schema_version_valid = actual_schema_version == CURRENT_SCHEMA_VERSION;
    let foreign_keys_valid = foreign_keys_enabled;
    let integrity_valid = integrity_check == "ok";
    let tables_present = tables.iter().all(|table| table.exists);
    let required_columns_present = tables.iter().all(|table| table.missing_columns.is_empty());
    let row_counts_ready = tables.iter().all(|table| table.row_count.is_some());
    let self_test_ready = self_test.as_ref().is_none_or(storage_self_test_ready);
    let storage_ready = schema_version_valid
        && foreign_keys_valid
        && integrity_valid
        && tables_present
        && required_columns_present
        && row_counts_ready
        && self_test_ready
        && issues.is_empty();

    Ok(StorageCheckReport {
        schema: "goose.storage-check-report.v1".to_string(),
        generated_by: "goose-storage-check".to_string(),
        database_path: options.database_path.display().to_string(),
        pass: storage_ready,
        schema_version_valid,
        foreign_keys_valid,
        integrity_valid,
        tables_present,
        required_columns_present,
        row_counts_ready,
        self_test_ready,
        storage_ready,
        expected_schema_version: CURRENT_SCHEMA_VERSION,
        actual_schema_version,
        foreign_keys_enabled,
        integrity_check,
        tables,
        self_test,
        issues,
        next_actions,
    })
}

fn check_table(store: &GooseStore, table: &str, required: &[&str]) -> StorageTableCheck {
    let mut issues = Vec::new();
    let columns = match store.table_columns(table) {
        Ok(columns) => columns,
        Err(error) => {
            issues.push(format!("cannot inspect columns: {error}"));
            Default::default()
        }
    };
    let exists = !columns.is_empty();
    if !exists {
        issues.push("missing table".to_string());
    }
    let missing_columns = required
        .iter()
        .filter(|column| !columns.contains(**column))
        .map(|column| (*column).to_string())
        .collect::<Vec<_>>();
    for column in &missing_columns {
        issues.push(format!("missing column {column}"));
    }
    let row_count = if exists {
        match store.table_count(table) {
            Ok(count) => Some(count),
            Err(error) => {
                issues.push(format!("cannot count rows: {error}"));
                None
            }
        }
    } else {
        None
    };
    let next_actions = storage_table_next_actions(table, &issues);

    StorageTableCheck {
        table: table.to_string(),
        exists,
        row_count,
        columns: columns.into_iter().collect(),
        missing_columns,
        issues,
        next_actions,
    }
}

fn run_storage_self_test(store: &GooseStore) -> StorageSelfTestReport {
    let mut issues = Vec::new();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap_or_default();
    let parsed = match parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME) {
        Ok(parsed) => parsed,
        Err(error) => {
            return StorageSelfTestReport {
                ran: true,
                raw_inserted: false,
                raw_idempotent: false,
                decoded_inserted: false,
                query_roundtrip: false,
                foreign_key_rejected: false,
                issues: vec![format!("cannot parse synthetic self-test frame: {error}")],
                next_actions: vec![StorageCheckNextAction {
                    scope: "self-test:synthetic-frame".to_string(),
                    reason: "synthetic_frame_parse_failed".to_string(),
                    action:
                        "Fix the synthetic storage-check frame or parser regression before trusting storage self-tests."
                            .to_string(),
                }],
            };
        }
    };

    let raw_input = RawEvidenceInput {
        evidence_id: "goose.storage-check.raw",
        source: "goose.storage-check",
        captured_at: "2026-05-28T00:00:00Z",
        device_model: "WHOOP 5.0 Goose",
        payload: &raw,
        sensitivity: "synthetic-self-test",
        capture_session_id: None,
    };

    let raw_inserted = match store.insert_raw_evidence(raw_input.clone()) {
        Ok(inserted) => inserted,
        Err(error) => {
            issues.push(format!("raw evidence insert failed: {error}"));
            false
        }
    };
    let raw_idempotent = match store.insert_raw_evidence(raw_input) {
        Ok(inserted) => !inserted,
        Err(error) => {
            issues.push(format!("raw evidence idempotency failed: {error}"));
            false
        }
    };

    let decoded_inserted = match store.insert_decoded_frame(DecodedFrameInput {
        frame_id: "goose.storage-check.frame",
        evidence_id: "goose.storage-check.raw",
        parsed: &parsed,
        parser_version: "goose-storage-check",
    }) {
        Ok(inserted) => inserted,
        Err(error) => {
            issues.push(format!("decoded frame insert failed: {error}"));
            false
        }
    };

    let query_roundtrip = match store.raw_evidence("goose.storage-check.raw") {
        Ok(Some(row)) if row.payload_hex == GET_HELLO_FRAME && row.sha256.len() == 64 => true,
        Ok(Some(_)) => {
            issues.push("raw evidence roundtrip returned unexpected row".to_string());
            false
        }
        Ok(None) => {
            issues.push("raw evidence roundtrip returned no row".to_string());
            false
        }
        Err(error) => {
            issues.push(format!("raw evidence roundtrip query failed: {error}"));
            false
        }
    };

    let foreign_key_rejected = match store.insert_decoded_frame(DecodedFrameInput {
        frame_id: "goose.storage-check.missing-evidence-frame",
        evidence_id: "goose.storage-check.missing-evidence",
        parsed: &parsed,
        parser_version: "goose-storage-check",
    }) {
        Ok(_) => {
            issues.push("decoded frame insert unexpectedly accepted missing evidence".to_string());
            false
        }
        Err(error) if error.to_string().contains("FOREIGN KEY") => true,
        Err(error) => {
            issues.push(format!(
                "decoded frame missing-evidence insert failed for unexpected reason: {error}"
            ));
            false
        }
    };

    StorageSelfTestReport {
        ran: true,
        raw_inserted,
        raw_idempotent,
        decoded_inserted,
        query_roundtrip,
        foreign_key_rejected,
        next_actions: storage_self_test_next_actions(&issues),
        issues,
    }
}

fn storage_self_test_ready(report: &StorageSelfTestReport) -> bool {
    report.ran
        && report.raw_inserted
        && report.raw_idempotent
        && report.decoded_inserted
        && report.query_roundtrip
        && report.foreign_key_rejected
        && report.issues.is_empty()
}

fn storage_check_report_next_actions(
    issues: &[String],
    tables: &[StorageTableCheck],
    self_test: &Option<StorageSelfTestReport>,
) -> Vec<StorageCheckNextAction> {
    let mut actions = Vec::new();
    for issue in issues {
        if issue.starts_with("expected schema version") {
            actions.push(StorageCheckNextAction {
                scope: "database:schema_version".to_string(),
                reason: "schema_version_mismatch".to_string(),
                action: "Back up the SQLite store, run Goose migrations with the current Rust core, then rerun storage.check before capture/import/export writes.".to_string(),
            });
        } else if issue == "SQLite foreign key enforcement is disabled" {
            actions.push(StorageCheckNextAction {
                scope: "database:foreign_keys".to_string(),
                reason: "foreign_keys_disabled".to_string(),
                action: "Reopen the database through GooseStore or enable PRAGMA foreign_keys before any write path uses the connection.".to_string(),
            });
        } else if issue.starts_with("SQLite integrity_check returned") {
            actions.push(StorageCheckNextAction {
                scope: "database:integrity_check".to_string(),
                reason: "integrity_check_failed".to_string(),
                action: "Stop writes, preserve a copy of the SQLite file, restore from a known-good export or rebuild from raw evidence, then rerun storage.check.".to_string(),
            });
        }
    }
    actions.extend(
        tables
            .iter()
            .flat_map(|table| table.next_actions.iter().cloned()),
    );
    if let Some(self_test) = self_test {
        actions.extend(self_test.next_actions.iter().cloned());
    }
    dedupe_storage_next_actions(actions)
}

fn storage_table_next_actions(table: &str, issues: &[String]) -> Vec<StorageCheckNextAction> {
    dedupe_storage_next_actions(
        issues
            .iter()
            .map(|issue| {
                if issue == "missing table" {
                    StorageCheckNextAction {
                        scope: table.to_string(),
                        reason: "missing_table".to_string(),
                        action: "Run Goose migrations on this SQLite store before trusting app capture, metrics, export, debug, or health-sync paths.".to_string(),
                    }
                } else if let Some(column) = issue.strip_prefix("missing column ") {
                    StorageCheckNextAction {
                        scope: format!("{table}.{column}"),
                        reason: "missing_column".to_string(),
                        action: "Run the migration that adds this column, then rerun storage.check before writing new rows.".to_string(),
                    }
                } else if issue.starts_with("cannot inspect columns") {
                    StorageCheckNextAction {
                        scope: table.to_string(),
                        reason: "column_inspection_failed".to_string(),
                        action: "Inspect the SQLite schema manually, repair table metadata or permissions, then rerun storage.check.".to_string(),
                    }
                } else if issue.starts_with("cannot count rows") {
                    StorageCheckNextAction {
                        scope: table.to_string(),
                        reason: "row_count_failed".to_string(),
                        action: "Repair the table or query permissions so storage.check can read row counts before relying on this store.".to_string(),
                    }
                } else {
                    StorageCheckNextAction {
                        scope: table.to_string(),
                        reason: "table_issue".to_string(),
                        action: "Inspect and repair this table, then add a storage regression before trusting the local store.".to_string(),
                    }
                }
            })
            .collect(),
    )
}

fn storage_self_test_next_actions(issues: &[String]) -> Vec<StorageCheckNextAction> {
    dedupe_storage_next_actions(
        issues
            .iter()
            .map(|issue| {
                let (reason, action) = if issue.starts_with("raw evidence insert failed") {
                    (
                        "raw_evidence_insert_failed",
                        "Fix raw_evidence insert schema, constraints, or payload serialization before capture/import writes are trusted.",
                    )
                } else if issue.starts_with("raw evidence idempotency failed") {
                    (
                        "raw_evidence_idempotency_failed",
                        "Fix raw_evidence conflict handling so duplicate capture imports are idempotent before enabling repeated sync/import flows.",
                    )
                } else if issue.starts_with("decoded frame insert failed") {
                    (
                        "decoded_frame_insert_failed",
                        "Fix decoded_frames insert schema or raw-evidence reference handling before parser output is trusted.",
                    )
                } else if issue.starts_with("raw evidence roundtrip") {
                    (
                        "raw_evidence_roundtrip_failed",
                        "Fix raw evidence query/serialization roundtrip before export, debug, or metric feature extraction reads from this store.",
                    )
                } else if issue.contains("unexpectedly accepted missing evidence") {
                    (
                        "foreign_key_accepts_orphans",
                        "Fix SQLite foreign-key enforcement so decoded frames cannot reference missing raw evidence before any write path is trusted.",
                    )
                } else if issue.contains("missing-evidence insert failed for unexpected reason") {
                    (
                        "foreign_key_failure_unexpected",
                        "Inspect decoded frame foreign-key failure handling and update storage.check expectations only with a regression fixture.",
                    )
                } else {
                    (
                        "self_test_issue",
                        "Inspect the storage self-test failure and add a focused SQLite regression before trusting this store.",
                    )
                };
                StorageCheckNextAction {
                    scope: "self-test".to_string(),
                    reason: reason.to_string(),
                    action: action.to_string(),
                }
            })
            .collect(),
    )
}

fn dedupe_storage_next_actions(
    actions: Vec<StorageCheckNextAction>,
) -> Vec<StorageCheckNextAction> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for action in actions {
        let key = format!("{}:{}:{}", action.scope, action.reason, action.action);
        if seen.insert(key) {
            deduped.push(action);
        }
    }
    deduped
}

fn required_columns() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut columns = BTreeMap::new();
    columns.insert("goose_schema_migrations", vec!["version", "applied_at"]);
    columns.insert(
        "raw_evidence",
        vec![
            "evidence_id",
            "source",
            "captured_at",
            "device_model",
            "payload_hex",
            "sha256",
            "sensitivity",
            "capture_session_id",
            "created_at",
        ],
    );
    columns.insert(
        "decoded_frames",
        vec![
            "frame_id",
            "evidence_id",
            "device_type",
            "raw_len",
            "header_len",
            "declared_len",
            "payload_hex",
            "payload_crc_hex",
            "header_crc_valid",
            "payload_crc_valid",
            "packet_type",
            "packet_type_name",
            "sequence",
            "command_or_event",
            "parsed_payload_json",
            "parser_version",
            "warnings_json",
            "created_at",
        ],
    );
    columns.insert(
        "algorithm_definitions",
        vec![
            "algorithm_id",
            "version",
            "metric_family",
            "display_name",
            "implementation",
            "license",
            "input_schema",
            "output_schema",
            "input_requirements_json",
            "params_json",
            "quality_gates_json",
            "status",
            "created_at",
        ],
    );
    columns.insert(
        "algorithm_runs",
        vec![
            "run_id",
            "algorithm_id",
            "version",
            "start_time",
            "end_time",
            "output_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "command_validation_records",
        vec![
            "command",
            "risk_gate",
            "direct_send_ready",
            "report_json",
            "updated_at",
        ],
    );
    columns.insert(
        "metric_values",
        vec![
            "metric_value_id",
            "run_id",
            "metric_family",
            "name",
            "value",
            "unit",
            "start_time",
            "end_time",
            "created_at",
        ],
    );
    columns.insert(
        "metric_components",
        vec![
            "metric_component_id",
            "run_id",
            "component_name",
            "value",
            "unit",
            "contribution_json",
            "created_at",
        ],
    );
    columns.insert(
        "calibration_labels",
        vec![
            "label_id",
            "metric_family",
            "label_source",
            "captured_at",
            "value",
            "unit",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "calibration_runs",
        vec![
            "calibration_run_id",
            "algorithm_id",
            "version",
            "train_start",
            "train_end",
            "holdout_start",
            "holdout_end",
            "metrics_json",
            "params_json",
            "created_at",
        ],
    );
    columns.insert(
        "algorithm_preferences",
        vec![
            "scope",
            "metric_family",
            "algorithm_id",
            "version",
            "updated_at",
        ],
    );
    columns.insert(
        "capture_sessions",
        vec![
            "session_id",
            "source",
            "started_at_unix_ms",
            "ended_at_unix_ms",
            "device_model",
            "active_device_id",
            "status",
            "frame_count",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "activity_sessions",
        vec![
            "session_id",
            "source",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "duration_ms",
            "activity_type",
            "external_activity_type_code",
            "external_activity_type_name",
            "custom_label",
            "confidence",
            "detection_method",
            "sync_status",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "activity_metrics",
        vec![
            "metric_id",
            "activity_session_id",
            "metric_name",
            "value",
            "unit",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "quality_flags_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "daily_activity_metrics",
        vec![
            "daily_metric_id",
            "date_key",
            "timezone",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "steps",
            "active_kcal",
            "resting_kcal",
            "total_kcal",
            "average_cadence_spm",
            "source_kind",
            "confidence",
            "inputs_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "hourly_activity_metrics",
        vec![
            "hourly_metric_id",
            "date_key",
            "timezone",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "steps",
            "active_kcal",
            "resting_kcal",
            "total_kcal",
            "average_cadence_spm",
            "source_kind",
            "confidence",
            "inputs_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "daily_recovery_metrics",
        vec![
            "daily_metric_id",
            "date_key",
            "timezone",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "resting_hr_bpm",
            "hrv_rmssd_ms",
            "respiratory_rate_rpm",
            "oxygen_saturation_percent",
            "skin_temperature_delta_c",
            "source_kind",
            "confidence",
            "inputs_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "metric_provenance",
        vec![
            "provenance_id",
            "metric_scope",
            "metric_id",
            "source_kind",
            "source_detail",
            "confidence",
            "inputs_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "metric_debug_features",
        vec![
            "feature_id",
            "metric_family",
            "feature_name",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "source_kind",
            "confidence",
            "feature_json",
            "inputs_json",
            "quality_flags_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "step_counter_samples",
        vec![
            "sample_id",
            "sample_time_unix_ms",
            "counter_value",
            "cadence_spm",
            "activity_state",
            "source_kind",
            "packet_family",
            "json_path",
            "frame_id",
            "evidence_id",
            "capture_session_id",
            "quality_flags_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "activity_intervals",
        vec![
            "interval_id",
            "activity_session_id",
            "interval_type",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "duration_ms",
            "sequence",
            "metadata_json",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "activity_labels",
        vec![
            "label_id",
            "activity_session_id",
            "label_type",
            "value",
            "source",
            "confidence",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "external_sleep_sessions",
        vec![
            "sleep_id",
            "source",
            "platform",
            "platform_record_id",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "duration_ms",
            "timezone",
            "stage_summary_json",
            "confidence",
            "provenance_json",
            "created_at",
            "updated_at",
        ],
    );
    columns.insert(
        "external_sleep_stages",
        vec![
            "stage_id",
            "sleep_id",
            "stage_kind",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "duration_ms",
            "confidence",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "sleep_correction_labels",
        vec![
            "label_id",
            "sleep_id",
            "label_type",
            "start_time_unix_ms",
            "end_time_unix_ms",
            "value_json",
            "source",
            "confidence",
            "provenance_json",
            "created_at",
        ],
    );
    columns.insert(
        "debug_sessions",
        vec![
            "session_id",
            "started_at_unix_ms",
            "bridge_url",
            "bind_host",
            "token_required",
            "token_present",
            "remote_bind_enabled",
            "visible_remote_bind_toggle",
            "created_at",
        ],
    );
    columns.insert(
        "debug_commands",
        vec![
            "command_id",
            "session_id",
            "schema",
            "command",
            "args_json",
            "dry_run",
            "received_at_unix_ms",
            "created_at",
        ],
    );
    columns.insert(
        "debug_events",
        vec![
            "session_id",
            "sequence",
            "schema",
            "time_unix_ms",
            "source",
            "level",
            "topic",
            "message",
            "command_id",
            "data_json",
            "created_at",
        ],
    );

    for table in known_tables() {
        debug_assert!(columns.contains_key(table));
    }
    columns
}
