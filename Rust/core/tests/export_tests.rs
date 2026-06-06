use std::{collections::BTreeMap, fs, path::Path};

use goose_core::{
    calibration::{
        CalibrationDataset, CalibrationOptions, calibration_run_record, evaluate_linear_calibration,
    },
    capture_import::{
        CaptureImportOptions, CapturedFrameBatchOptions, CapturedFrameInput,
        import_captured_frame_batch, import_fixture_index,
    },
    export::{RawExportFilters, RawExportOptions, export_raw_timeframe, validate_export_bundle},
    fixtures::build_fixture_index,
    metrics::{
        GOOSE_SLEEP_V1_ID, HrvInput, SleepInput, SleepModelStatusInput, SleepV1Input,
        algorithm_run_record, built_in_algorithm_definitions, goose_hrv_v0, goose_sleep_v1,
        hrv_run_record,
    },
    protocol::{DeviceType, PACKET_TYPE_REALTIME_RAW_DATA, build_v5_payload_frame},
    store::{
        ActivityIntervalInput, ActivityLabelInput, ActivityMetricInput, ActivitySessionInput,
        AlgorithmDefinitionRecord, AlgorithmRunRecord, CalibrationLabelInput, CalibrationRunRecord,
        CalibrationRunTimes, CaptureSessionInput, CommandValidationRecord,
        DailyActivityMetricInput, DailyRecoveryMetricInput, DebugCommandRow, DebugEventRow,
        DebugSessionRow, GooseStore, HourlyActivityMetricInput, MetricProvenanceInput,
    },
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const K10_FRAME: &str = "aa0164000001fb212b0a010000000000000000000000000000480000000000000000000000000000 00000000000000000000000000000000000000000000000000000000000000000000000000000000 000000000000000000000000000100feff0300000000000068cc8271";
const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";

#[test]
fn validates_manifest_file_checksum_and_required_shape() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let rows = raw_evidence_jsonl("synthetic-1", &[0x01, 0x02]);
    let csv_rows = raw_evidence_csv("synthetic-1", &[0x01, 0x02]);
    fs::write(
        tempdir.path().join("data/raw_evidence.jsonl"),
        rows.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/raw_evidence.csv"),
        csv_rows.as_bytes(),
    )
    .unwrap();
    let checksum = sha256_hex(rows.as_bytes());
    let csv_checksum = sha256_hex(csv_rows.as_bytes());

    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_evidence"],
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "{checksum}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/raw_evidence.csv", "sha256": "{csv_checksum}", "row_count": 1, "kind": "csv"}}
  ]
}}"#
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.manifest_valid);
    assert!(report.files_valid);
    assert!(report.content_valid);
    assert_eq!(report.files.len(), 2);
    assert!(report.files[0].pass);
    assert!(report.content.csv_valid);
    assert_eq!(report.content.csv_row_count_checks, 1);
    assert_eq!(report.content.raw_evidence_rows, 1);
    assert_eq!(report.content.reimported_evidence_ids, 1);
}

#[test]
fn rejects_checksum_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let rows = raw_evidence_jsonl("synthetic-1", &[0x01, 0x02]);
    let csv_rows = raw_evidence_csv("synthetic-1", &[0x01, 0x02]);
    fs::write(tempdir.path().join("data/raw_evidence.jsonl"), rows).unwrap();
    fs::write(
        tempdir.path().join("data/raw_evidence.csv"),
        csv_rows.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_evidence"],
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "not-the-checksum", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/raw_evidence.csv", "sha256": "{}", "row_count": 1, "kind": "csv"}}
  ]
}}"#,
            sha256_hex(csv_rows.as_bytes()),
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(report.manifest_valid, "{:?}", report.issues);
    assert!(!report.files_valid);
    assert!(report.content_valid, "{:?}", report.content.issues);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("failed file validation"))
    );
    assert!(
        report.files[0]
            .issues
            .iter()
            .any(|issue| issue.contains("sha256 mismatch"))
    );
    assert!(report.next_actions.iter().any(|action| {
        action.scope == "data/raw_evidence.jsonl" && action.reason == "checksum_mismatch"
    }));
    assert!(report.files[0].next_actions.iter().any(|action| {
        action.reason == "checksum_mismatch" && action.action.contains("restore the file bytes")
    }));
}

#[test]
fn rejects_csv_row_count_mismatch_and_missing_required_csv_artifact() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let rows = raw_evidence_jsonl("synthetic-1", &[0x01, 0x02]);
    let csv_rows = raw_evidence_csv("synthetic-1", &[0x01, 0x02]);
    fs::write(
        tempdir.path().join("data/raw_evidence.jsonl"),
        rows.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/raw_evidence.csv"),
        csv_rows.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_evidence"],
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/raw_evidence.csv", "sha256": "{}", "row_count": 2, "kind": "csv"}}
  ]
}}"#,
            sha256_hex(rows.as_bytes()),
            sha256_hex(csv_rows.as_bytes()),
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(report.manifest_valid, "{:?}", report.issues);
    assert!(!report.content_valid);
    assert!(!report.content.csv_valid);
    assert_eq!(report.content.csv_row_count_checks, 1);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("data/raw_evidence.csv row_count mismatch: manifest 2, actual 1")
    }));

    let missing_csv_dir = tempfile::tempdir().unwrap();
    fs::create_dir(missing_csv_dir.path().join("data")).unwrap();
    fs::write(
        missing_csv_dir.path().join("data/raw_evidence.jsonl"),
        rows.as_bytes(),
    )
    .unwrap();
    fs::write(
        missing_csv_dir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_evidence"],
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}}
  ]
}}"#,
            sha256_hex(rows.as_bytes()),
        ),
    )
    .unwrap();

    let missing_csv_report = validate_export_bundle(missing_csv_dir.path()).unwrap();

    assert!(!missing_csv_report.pass);
    assert!(!missing_csv_report.manifest_valid);
    assert!(missing_csv_report.issues.iter().any(|issue| {
        issue.contains("data family raw_evidence requires data/raw_evidence.csv")
    }));
}

#[test]
fn rejects_unknown_data_family_and_unselected_artifact_files() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let rows = raw_evidence_jsonl("synthetic-1", &[0x01, 0x02]);
    fs::write(
        tempdir.path().join("data/raw_evidence.jsonl"),
        rows.as_bytes(),
    )
    .unwrap();
    let checksum = sha256_hex(rows.as_bytes());

    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_packets"],
  "official_labels_are_labels": true,
  "files": [{{"path": "data/raw_evidence.jsonl", "sha256": "{checksum}", "row_count": 1, "kind": "jsonl"}}]
}}"#
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(!report.manifest_valid);
    assert!(report.files_valid);
    assert!(report.content_valid);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("manifest.data_families contains unknown family raw_packets")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("data/raw_evidence.jsonl belongs to unselected data family raw_evidence")
    }));
    assert!(
        report.next_actions.iter().any(|action| {
            action.reason == "manifest_data_family" && action.scope == "manifest"
        })
    );
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "unselected_data_family_artifact"
            && action.scope == "data/raw_evidence.jsonl"
    }));
}

#[test]
fn rejects_sqlite_manifest_when_raw_bytes_are_redacted() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let sqlite_bytes = b"sqlite copy";
    fs::write(tempdir.path().join("data/goose.sqlite"), sqlite_bytes).unwrap();
    let checksum = sha256_hex(sqlite_bytes);

    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["sqlite"],
  "filters": {{"include_raw_bytes": false}},
  "official_labels_are_labels": true,
  "files": [{{"path": "data/goose.sqlite", "sha256": "{checksum}", "kind": "sqlite"}}]
}}"#
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(!report.manifest_valid);
    assert!(report.files_valid);
    assert!(report.content_valid);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("sqlite data family cannot be exported when include_raw_bytes is false")
    }));
    assert!(
        report.next_actions.iter().any(|action| {
            action.scope == "report" && action.reason == "raw_byte_sqlite_policy"
        })
    );
}

#[test]
fn rejects_jsonl_row_count_and_evidence_reimport_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let raw_rows = b"{\"evidence_id\":\"raw-1\"}\n";
    let decoded_rows = b"{\"frame_id\":\"frame-1\",\"evidence_id\":\"missing-raw\"}\n";
    fs::write(tempdir.path().join("data/raw_evidence.jsonl"), raw_rows).unwrap();
    fs::write(
        tempdir.path().join("data/decoded_frames.jsonl"),
        decoded_rows,
    )
    .unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-27T01:00:00Z"}},
  "data_families": ["raw_evidence", "decoded_frames"],
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "{}", "row_count": 2, "kind": "jsonl"}},
    {{"path": "data/decoded_frames.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}}
  ]
}}"#,
            sha256_hex(raw_rows),
            sha256_hex(decoded_rows),
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("data/raw_evidence.jsonl row_count mismatch"))
    );
    assert!(report.issues.iter().any(|issue| {
        issue.contains("decoded frame evidence_id missing-raw is missing from raw evidence export")
    }));
    assert!(report.content.next_actions.iter().any(|action| {
        action.scope == "data/raw_evidence.jsonl" && action.reason == "row_count_mismatch"
    }));
    assert!(report.next_actions.iter().any(|action| {
        action.reason == "broken_export_reference" && action.action.contains("linked raw evidence")
    }));
}

#[test]
fn exports_sqlite_timeframe_to_jsonl_csv_and_sqlite_bundle() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("export.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);
    let definition = built_in_algorithm_definitions().remove(0);
    store.upsert_algorithm_definition(&definition).unwrap();
    let hrv_result = goose_hrv_v0(&HrvInput {
        start_time: "2026-05-27T00:15:00Z".to_string(),
        end_time: "2026-05-27T00:20:00Z".to_string(),
        rr_intervals_ms: vec![800.0, 810.0, 790.0, 800.0],
        input_ids: vec!["synthetic.goose.v5.get_hello_frame".to_string()],
    });
    let hrv_record = hrv_run_record("hrv-run-1", &hrv_result).unwrap();
    store.insert_algorithm_run(&hrv_record).unwrap();
    store
        .upsert_algorithm_definition(&AlgorithmDefinitionRecord {
            algorithm_id: "goose.recovery.v0".to_string(),
            version: "0.1.0".to_string(),
            metric_family: "recovery".to_string(),
            display_name: "Goose Recovery v0".to_string(),
            implementation: "rust".to_string(),
            license: "UNLICENSED".to_string(),
            input_schema: "goose.recovery-input.v1".to_string(),
            output_schema: "goose.recovery-output.v1".to_string(),
            input_requirements_json: "{}".to_string(),
            params_json: "{}".to_string(),
            quality_gates_json: "[]".to_string(),
            status: "experimental".to_string(),
        })
        .unwrap();
    let calibration_dataset: CalibrationDataset = serde_json::from_str(include_str!(
        "../fixtures/synthetic/recovery_calibration_linear.json"
    ))
    .unwrap();
    let calibration_report = evaluate_linear_calibration(
        &calibration_dataset,
        &CalibrationOptions {
            metric_family: "recovery".to_string(),
            algorithm_id: "goose.recovery.v0".to_string(),
            algorithm_version: "0.1.0".to_string(),
            split_at: "2026-05-04T00:00:00Z".to_string(),
            min_train_rows: 2,
            min_holdout_rows: 1,
        },
    );
    assert!(calibration_report.pass);
    let calibration_record =
        calibration_run_record("calibration-run-1", &calibration_report).unwrap();
    store.insert_calibration_run(&calibration_record).unwrap();
    store
        .insert_calibration_label(CalibrationLabelInput {
            label_id: "manual.recovery.2026-05-04",
            metric_family: "recovery",
            label_source: "manual",
            captured_at: "2026-05-04T00:00:00Z",
            value: 79.0,
            unit: "score_0_to_100",
            provenance_json: r#"{"entry":"typed_by_user","official_labels_are_labels":true}"#,
        })
        .unwrap();
    store
        .insert_debug_session(&DebugSessionRow {
            session_id: "debug-export-session".to_string(),
            started_at_unix_ms: 1779840000000,
            bridge_url: "ws://127.0.0.1:49152/goose-debug/stream?token=secret-token".to_string(),
            bind_host: "127.0.0.1".to_string(),
            token_required: true,
            token_present: true,
            remote_bind_enabled: false,
            visible_remote_bind_toggle: false,
        })
        .unwrap();
    store
        .insert_debug_command(&DebugCommandRow {
            command_id: "debug-export-command".to_string(),
            session_id: "debug-export-session".to_string(),
            schema: "goose.debug.command.v1".to_string(),
            command: "export.raw_timeframe".to_string(),
            args_json: r#"{"url":"ws://127.0.0.1/goose-debug/stream?token=secret-token"}"#
                .to_string(),
            dry_run: false,
            received_at_unix_ms: 1779840060000,
        })
        .unwrap();
    store
        .insert_debug_event(&DebugEventRow {
            session_id: "debug-export-session".to_string(),
            sequence: 1,
            schema: "goose.debug.event.v1".to_string(),
            time_unix_ms: 1779840120000,
            source: "app".to_string(),
            level: "info".to_string(),
            topic: "export.started".to_string(),
            message: "export requested".to_string(),
            command_id: Some("debug-export-command".to_string()),
            data_json: r#"{"bind_url":"ws://127.0.0.1/goose-debug/stream?token=secret-token&client=agent"}"#
                .to_string(),
        })
        .unwrap();
    seed_command_validation_record(&store);
    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: Vec::new(),
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert!(report.data_families_valid);
    assert!(report.filters_valid);
    assert!(report.time_window_valid);
    assert!(report.version_fields_valid);
    assert!(report.sqlite_policy_valid);
    assert!(report.manifest_ready);
    assert!(report.files_written);
    assert!(report.zip_ready);
    assert!(report.export_ready);
    assert_eq!(report.raw_rows, 8);
    assert_eq!(report.decoded_frame_rows, 8);
    assert_eq!(report.packet_timeline_rows, 8);
    assert_eq!(report.sensor_sample_rows, 19);
    assert_eq!(report.metric_feature_report_rows, 7);
    assert_eq!(report.metric_value_rows, 7);
    assert_eq!(report.metric_component_rows, 4);
    assert_eq!(report.algorithm_run_rows, 1);
    assert_eq!(report.calibration_label_rows, 1);
    assert_eq!(report.calibration_run_rows, 1);
    assert_eq!(report.daily_activity_metric_rows, 0);
    assert_eq!(report.hourly_activity_metric_rows, 0);
    assert_eq!(report.daily_recovery_metric_rows, 0);
    assert_eq!(report.metric_provenance_rows, 0);
    assert_eq!(report.debug_session_rows, 1);
    assert_eq!(report.debug_command_rows, 1);
    assert_eq!(report.debug_event_rows, 1);
    assert_eq!(report.command_validation_rows, 1);
    assert!(
        report
            .manifest
            .files
            .iter()
            .any(|file| file.path == "data/goose.sqlite")
    );
    assert!(
        report
            .manifest
            .data_families
            .contains(&"calibration_labels".to_string())
    );
    assert!(
        report
            .manifest
            .data_families
            .contains(&"debug_events".to_string())
    );
    assert!(
        report
            .manifest
            .data_families
            .contains(&"command_validation".to_string())
    );
    assert!(
        report
            .manifest
            .data_families
            .contains(&"local_health_metrics".to_string())
    );
    assert!(export_dir.join("data/raw_evidence.jsonl").exists());
    assert!(export_dir.join("data/raw_evidence.csv").exists());
    assert!(export_dir.join("data/decoded_frames.jsonl").exists());
    assert!(export_dir.join("data/decoded_frames.csv").exists());
    assert!(export_dir.join("data/packet_timeline.jsonl").exists());
    assert!(export_dir.join("data/packet_timeline.csv").exists());
    assert!(export_dir.join("data/sensor_samples.jsonl").exists());
    assert!(export_dir.join("data/sensor_samples.csv").exists());
    assert!(export_dir.join("data/metric_features.jsonl").exists());
    assert!(export_dir.join("data/metric_features.csv").exists());
    assert!(export_dir.join("data/metric_values.jsonl").exists());
    assert!(export_dir.join("data/metric_values.csv").exists());
    assert!(export_dir.join("data/metric_components.jsonl").exists());
    assert!(export_dir.join("data/metric_components.csv").exists());
    assert!(export_dir.join("data/algorithm_runs.jsonl").exists());
    assert!(export_dir.join("data/algorithm_runs.csv").exists());
    assert!(export_dir.join("data/calibration_labels.jsonl").exists());
    assert!(export_dir.join("data/calibration_labels.csv").exists());
    assert!(export_dir.join("data/calibration_runs.jsonl").exists());
    assert!(export_dir.join("data/calibration_runs.csv").exists());
    assert!(
        export_dir
            .join("data/local_health_daily_activity_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_activity_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_hourly_activity_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_hourly_activity_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_recovery_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_recovery_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_metric_provenance.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_metric_provenance.csv")
            .exists()
    );
    assert!(export_dir.join("data/debug_sessions.jsonl").exists());
    assert!(export_dir.join("data/debug_sessions.csv").exists());
    assert!(export_dir.join("data/debug_commands.jsonl").exists());
    assert!(export_dir.join("data/debug_commands.csv").exists());
    assert!(export_dir.join("data/debug_events.jsonl").exists());
    assert!(export_dir.join("data/debug_events.csv").exists());
    assert!(export_dir.join("data/command_validation.jsonl").exists());
    assert!(export_dir.join("data/command_validation.csv").exists());
    assert!(export_dir.join("data/goose.sqlite").exists());
    let exported_sqlite = Connection::open(export_dir.join("data/goose.sqlite")).unwrap();
    let schema_version: i64 = exported_sqlite
        .query_row(
            "SELECT version FROM goose_schema_migrations ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(schema_version >= 14);
    let decoded_frames = fs::read_to_string(export_dir.join("data/decoded_frames.jsonl")).unwrap();
    let packet_timeline =
        fs::read_to_string(export_dir.join("data/packet_timeline.jsonl")).unwrap();
    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    let metric_features =
        fs::read_to_string(export_dir.join("data/metric_features.jsonl")).unwrap();
    let metric_values = fs::read_to_string(export_dir.join("data/metric_values.jsonl")).unwrap();
    let metric_components =
        fs::read_to_string(export_dir.join("data/metric_components.jsonl")).unwrap();
    assert!(decoded_frames.contains("raw_motion_k10"));
    assert!(packet_timeline.contains("body_summary"));
    assert!(sensor_samples.contains("normal_history_hr_marker"));
    assert!(sensor_samples.contains("r17_samples"));
    assert!(sensor_samples.contains("\"sample_value\":-1000"));
    assert!(metric_features.contains("goose.motion-feature-report.v1"));
    assert!(metric_features.contains("sleep_score_from_features"));
    assert!(metric_values.contains("\"metric_value_id\":\"hrv-run-1.mean_nn_ms\""));
    assert!(metric_values.contains("\"quality_flags\":[\"low_interval_count\"]"));
    assert!(metric_components.contains("\"component_name\":\"rmssd\""));
    let debug_sessions = fs::read_to_string(export_dir.join("data/debug_sessions.jsonl")).unwrap();
    let calibration_labels =
        fs::read_to_string(export_dir.join("data/calibration_labels.jsonl")).unwrap();
    let debug_commands = fs::read_to_string(export_dir.join("data/debug_commands.jsonl")).unwrap();
    let debug_events = fs::read_to_string(export_dir.join("data/debug_events.jsonl")).unwrap();
    let command_validation =
        fs::read_to_string(export_dir.join("data/command_validation.jsonl")).unwrap();
    assert!(calibration_labels.contains("official_labels_are_labels"));
    assert!(calibration_labels.contains("typed_by_user"));
    assert!(!debug_sessions.contains("secret-token"));
    assert!(!debug_commands.contains("secret-token"));
    assert!(!debug_events.contains("secret-token"));
    assert!(debug_sessions.contains("token=<redacted>"));
    assert!(debug_commands.contains("token=<redacted>"));
    assert!(debug_events.contains("token=<redacted>&client=agent"));
    assert!(command_validation.contains("\"command\":\"get_hello\""));
    assert!(command_validation.contains("\"family\":\"device_identity\""));
    assert!(command_validation.contains("\"validated_write_type\":\"with_response\""));
    assert!(command_validation.contains("\"validated_evidence_source\":\"official_app_capture\""));
    assert!(
        command_validation
            .contains("\"validated_capture_kind\":\"official_app_to_macos_emulator\"")
    );
    assert!(command_validation.contains("\"validated_owner\":\"user\""));
    assert!(command_validation.contains("\"validated_local_frame_hex\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert!(validation.content.pass, "{:?}", validation.content.issues);
    assert_eq!(validation.content.raw_evidence_rows, 8);
    assert_eq!(validation.content.decoded_frame_rows, 8);
    assert_eq!(validation.content.packet_timeline_rows, 8);
    assert_eq!(validation.content.sensor_sample_rows, 19);
    assert_eq!(validation.content.metric_feature_report_rows, 7);
    assert_eq!(validation.content.metric_value_rows, 7);
    assert_eq!(validation.content.metric_component_rows, 4);
    assert_eq!(validation.content.algorithm_run_rows, 1);
    assert_eq!(validation.content.calibration_label_rows, 1);
    assert_eq!(validation.content.calibration_run_rows, 1);
    assert_eq!(validation.content.daily_activity_metric_rows, 0);
    assert_eq!(validation.content.hourly_activity_metric_rows, 0);
    assert_eq!(validation.content.daily_recovery_metric_rows, 0);
    assert_eq!(validation.content.metric_provenance_rows, 0);
    assert_eq!(validation.content.command_validation_rows, 1);
    assert_eq!(validation.content.debug_session_rows, 1);
    assert_eq!(validation.content.debug_command_rows, 1);
    assert_eq!(validation.content.debug_event_rows, 1);
    assert_eq!(validation.content.reimported_evidence_ids, 8);
    assert_eq!(validation.content.reimported_frame_ids, 8);
}

#[test]
fn raw_export_sqlite_family_snapshots_live_wal_database() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("wal-snapshot.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let source_connection = Connection::open(&db_path).unwrap();
    source_connection
        .execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA wal_autocheckpoint=0;
            CREATE TABLE raw_export_snapshot_probe (
                id TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            INSERT INTO raw_export_snapshot_probe (id, value)
            VALUES ('wal-row', 42);
            "#,
        )
        .unwrap();
    assert!(
        db_path.with_extension("sqlite-wal").exists(),
        "test setup should leave a WAL sidecar"
    );

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sqlite".to_string()],
            filters: RawExportFilters::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let exported_path = export_dir.join("data/goose.sqlite");
    assert!(exported_path.exists());
    assert!(!export_dir.join("data/goose.sqlite-wal").exists());
    assert!(!export_dir.join("data/goose.sqlite-shm").exists());
    let exported_connection = Connection::open(exported_path).unwrap();
    let value: i64 = exported_connection
        .query_row(
            "SELECT value FROM raw_export_snapshot_probe WHERE id='wal-row'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, 42);
}

#[test]
fn raw_export_can_limit_selected_data_families() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("export.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "decoded_frames".to_string(),
                "raw_evidence".to_string(),
                "raw_evidence".to_string(),
            ],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.manifest.data_families,
        vec!["raw_evidence".to_string(), "decoded_frames".to_string()]
    );
    assert_eq!(report.raw_rows, 8);
    assert_eq!(report.decoded_frame_rows, 8);
    assert_eq!(report.packet_timeline_rows, 0);
    assert_eq!(report.sensor_sample_rows, 0);
    assert_eq!(report.metric_feature_report_rows, 0);
    assert_eq!(report.metric_value_rows, 0);
    assert_eq!(report.metric_component_rows, 0);
    assert_eq!(report.algorithm_run_rows, 0);
    assert_eq!(report.calibration_label_rows, 0);
    assert_eq!(report.debug_event_rows, 0);
    assert_eq!(report.command_validation_rows, 0);
    assert!(export_dir.join("data/raw_evidence.jsonl").exists());
    assert!(export_dir.join("data/decoded_frames.jsonl").exists());
    assert!(!export_dir.join("data/packet_timeline.jsonl").exists());
    assert!(!export_dir.join("data/sensor_samples.jsonl").exists());
    assert!(!export_dir.join("data/metric_features.jsonl").exists());
    assert!(!export_dir.join("data/metric_values.jsonl").exists());
    assert!(!export_dir.join("data/metric_components.jsonl").exists());
    assert!(!export_dir.join("data/algorithm_runs.jsonl").exists());
    assert!(!export_dir.join("data/debug_events.jsonl").exists());
    assert!(!export_dir.join("data/command_validation.jsonl").exists());
    assert!(!export_dir.join("data/goose.sqlite").exists());
    assert!(
        report
            .manifest
            .files
            .iter()
            .all(|file| file.path.contains("raw_evidence") || file.path.contains("decoded_frames"))
    );

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 8);
    assert_eq!(validation.content.decoded_frame_rows, 8);
    assert_eq!(validation.content.packet_timeline_rows, 0);
    assert_eq!(validation.content.sensor_sample_rows, 0);
    assert_eq!(validation.content.metric_feature_report_rows, 0);
    assert_eq!(validation.content.metric_value_rows, 0);
    assert_eq!(validation.content.metric_component_rows, 0);
    assert_eq!(validation.content.reimported_evidence_ids, 8);
    assert_eq!(validation.content.reimported_frame_ids, 8);
}

#[test]
fn raw_export_can_select_sensor_samples_only() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("sensor-samples.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sensor_samples".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 0);
    assert_eq!(report.decoded_frame_rows, 0);
    assert_eq!(report.sensor_sample_rows, 19);
    assert_eq!(
        report.manifest.data_families,
        vec!["sensor_samples".to_string()]
    );
    assert!(export_dir.join("data/sensor_samples.jsonl").exists());
    assert!(export_dir.join("data/sensor_samples.csv").exists());
    assert!(!export_dir.join("data/decoded_frames.jsonl").exists());

    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(sensor_samples.contains("\"source_signal\":\"raw_motion_k10\""));
    assert!(sensor_samples.contains("\"series_name\":\"accelerometer_x\""));
    assert!(sensor_samples.contains("\"raw_i16\":-2"));
    assert!(sensor_samples.contains("\"source_signal\":\"raw_motion_k10_heart_rate\""));
    assert!(sensor_samples.contains("\"raw_u8\":72"));
    assert!(sensor_samples.contains("\"source_signal\":\"normal_history_hr_marker\""));
    assert!(sensor_samples.contains("\"source_signal\":\"r17_optical_or_labrador_filtered\""));
    assert!(sensor_samples.contains("\"source_signal\":\"raw_motion_k21\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.sensor_sample_rows, 19);
    assert_eq!(validation.content.raw_evidence_rows, 0);
    assert_eq!(validation.content.decoded_frame_rows, 0);
}

#[test]
fn raw_export_sensor_samples_store_sample_time_separate_from_capture_time() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir
        .path()
        .join("timestamped-sensor-samples.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let frames = vec![CapturedFrameInput {
        evidence_id: "timestamped-motion".to_string(),
        frame_id: Some("timestamped-motion.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-01-01T20:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: k10_motion_frame_hex_with_timestamp(1_767_304_800),
        sensitivity: "user-owned-live-notification".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-01-01T19:00:00Z",
            end: "2026-01-01T21:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sensor_samples".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(sensor_samples.contains("\"captured_at\":\"2026-01-01T20:00:00Z\""));
    assert!(sensor_samples.contains("\"sample_time\":\"2026-01-01T22:00:00Z\""));
    assert!(sensor_samples.contains("\"sample_time_source\":\"device_timestamp\""));
    assert!(sensor_samples.contains("\"sample_time_unix_ms\":1767304800000"));
    let sensor_samples_csv =
        fs::read_to_string(export_dir.join("data/sensor_samples.csv")).unwrap();
    assert!(sensor_samples_csv.starts_with(
        "sample_id,frame_id,evidence_id,captured_at,sample_time,sample_time_unix_ms,sample_time_source,"
    ));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
}

#[test]
fn raw_export_sensor_samples_reject_invalid_device_timestamp_subseconds() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir
        .path()
        .join("invalid-subsecond-sensor-samples.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let frames = vec![CapturedFrameInput {
        evidence_id: "invalid-subsecond-motion".to_string(),
        frame_id: Some("invalid-subsecond-motion.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-01-01T20:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: k10_motion_frame_hex_with_timestamp_subseconds(1_767_304_800, 1_500),
        sensitivity: "user-owned-live-notification".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-01-01T19:00:00Z",
            end: "2026-01-01T21:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sensor_samples".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(sensor_samples.contains("\"sample_time\":\"2026-01-01T20:00:00Z\""));
    assert!(sensor_samples.contains("\"sample_time_source\":\"captured_at\""));
    assert!(sensor_samples.contains("\"sample_time_unix_ms\":1767297600000"));
    assert!(sensor_samples.contains("\"device_timestamp_subseconds\":1500"));
    assert!(sensor_samples.contains("device_timestamp_subseconds_out_of_range"));
    assert!(!sensor_samples.contains("\"sample_time\":\"2026-01-01T22:00:01Z\""));
}

#[test]
fn raw_export_can_select_metric_feature_reports_only() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("metric-features.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["metric_features".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 0);
    assert_eq!(report.decoded_frame_rows, 0);
    assert_eq!(report.metric_feature_report_rows, 7);
    assert_eq!(
        report.manifest.data_families,
        vec!["metric_features".to_string()]
    );
    assert!(export_dir.join("data/metric_features.jsonl").exists());
    assert!(export_dir.join("data/metric_features.csv").exists());
    assert!(!export_dir.join("data/raw_evidence.jsonl").exists());

    let metric_features =
        fs::read_to_string(export_dir.join("data/metric_features.jsonl")).unwrap();
    assert!(metric_features.contains("\"report_kind\":\"motion\""));
    assert!(metric_features.contains("\"report_kind\":\"heart_rate\""));
    assert!(metric_features.contains("\"report_kind\":\"hrv\""));
    assert!(metric_features.contains("\"issues_json\""));
    assert!(metric_features.contains("\"report_json\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.metric_feature_report_rows, 7);
    assert_eq!(validation.content.raw_evidence_rows, 0);
    assert_eq!(validation.content.decoded_frame_rows, 0);
}

#[test]
fn raw_export_can_select_metric_outputs_only() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("metric-outputs.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let definition = built_in_algorithm_definitions()
        .into_iter()
        .find(|definition| definition.algorithm_id == "goose.hrv.v0")
        .unwrap();
    store.upsert_algorithm_definition(&definition).unwrap();
    let hrv_result = goose_hrv_v0(&HrvInput {
        start_time: "2026-05-27T00:15:00Z".to_string(),
        end_time: "2026-05-27T00:20:00Z".to_string(),
        rr_intervals_ms: vec![800.0, 810.0, 790.0, 800.0],
        input_ids: vec!["metric-output-test".to_string()],
    });
    let hrv_record = hrv_run_record("metric-output-run-1", &hrv_result).unwrap();
    store.insert_algorithm_run(&hrv_record).unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["metric_outputs".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 0);
    assert_eq!(report.decoded_frame_rows, 0);
    assert_eq!(report.metric_value_rows, 7);
    assert_eq!(report.metric_component_rows, 4);
    assert_eq!(report.algorithm_run_rows, 0);
    assert_eq!(
        report.manifest.data_families,
        vec!["metric_outputs".to_string()]
    );
    assert!(export_dir.join("data/metric_values.jsonl").exists());
    assert!(export_dir.join("data/metric_values.csv").exists());
    assert!(export_dir.join("data/metric_components.jsonl").exists());
    assert!(export_dir.join("data/metric_components.csv").exists());
    assert!(!export_dir.join("data/algorithm_runs.jsonl").exists());

    let metric_values = fs::read_to_string(export_dir.join("data/metric_values.jsonl")).unwrap();
    let metric_components =
        fs::read_to_string(export_dir.join("data/metric_components.jsonl")).unwrap();
    assert!(metric_values.contains("\"name\":\"rmssd_ms\""));
    assert!(metric_values.contains("\"unit\":\"ms\""));
    assert!(metric_values.contains("\"quality_flags\":[\"low_interval_count\"]"));
    assert!(metric_components.contains("\"component_name\":\"pnn50\""));
    assert!(metric_components.contains("\"unit\":\"fraction\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.metric_value_rows, 7);
    assert_eq!(validation.content.metric_component_rows, 4);
    assert_eq!(validation.content.raw_evidence_rows, 0);
    assert_eq!(validation.content.decoded_frame_rows, 0);
}

#[test]
fn raw_export_preserves_sleep_v1_output_components_and_goose_provenance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("sleep-v1-export.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let definition = built_in_algorithm_definitions()
        .into_iter()
        .find(|definition| definition.algorithm_id == GOOSE_SLEEP_V1_ID)
        .unwrap();
    store.upsert_algorithm_definition(&definition).unwrap();

    let sleep_result = goose_sleep_v1(&SleepV1Input {
        sleep: SleepInput {
            start_time: "2026-05-27T22:30:00Z".to_string(),
            end_time: "2026-05-28T06:30:00Z".to_string(),
            sleep_duration_minutes: 420.0,
            sleep_need_minutes: 480.0,
            time_in_bed_minutes: 480.0,
            midpoint_deviation_minutes: 30.0,
            disturbance_count: 4,
            sleep_latency_minutes: 18.0,
            wake_after_sleep_onset_minutes: 42.0,
            wake_episode_count: 2,
            stage_minutes: BTreeMap::from([
                ("awake".to_string(), 60.0),
                ("core".to_string(), 210.0),
                ("deep".to_string(), 90.0),
                ("rem".to_string(), 120.0),
            ]),
            heart_rate_dip_percent: Some(12.5),
            input_ids: vec!["sleep-v1-export-input".to_string()],
        },
        model_status: SleepModelStatusInput {
            sleep_permission_granted: true,
            imported_platform_sleep_nights: 10,
            trusted_goose_sleep_nights: 2,
            motion_coverage_fraction: Some(0.94),
            heart_rate_coverage_fraction: Some(0.82),
            ..Default::default()
        },
        rolling_sleep_debt_minutes: 90.0,
        bedtime_deviation_minutes: 20.0,
        wake_time_deviation_minutes: 15.0,
        sleep_hr_average_bpm: Some(61.0),
        sleep_hr_min_bpm: Some(54.0),
        sleep_hr_trend_bpm_per_hour: Some(-1.2),
        naps_minutes: 25.0,
        prior_day_strain: Some(8.5),
        data_coverage_fraction: Some(0.92),
        ..Default::default()
    });
    assert!(sleep_result.errors.is_empty(), "{:?}", sleep_result.errors);
    let sleep_record = algorithm_run_record("sleep-v1-export-run-1", &sleep_result).unwrap();
    let stored_output: serde_json::Value = serde_json::from_str(&sleep_record.output_json).unwrap();
    assert_eq!(stored_output["sleep_hr_trend_bpm_per_hour"], -1.2);
    store.insert_algorithm_run(&sleep_record).unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-29T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["metric_outputs".to_string(), "algorithm_runs".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.algorithm_run_rows, 1);
    assert_eq!(report.metric_component_rows, 7);
    assert!(
        report
            .manifest
            .data_families
            .contains(&"metric_outputs".to_string())
    );
    assert!(
        report
            .manifest
            .data_families
            .contains(&"algorithm_runs".to_string())
    );

    let algorithm_runs = fs::read_to_string(export_dir.join("data/algorithm_runs.jsonl")).unwrap();
    let algorithm_run: serde_json::Value =
        serde_json::from_str(algorithm_runs.lines().next().unwrap()).unwrap();
    assert_eq!(algorithm_run["algorithm_id"], GOOSE_SLEEP_V1_ID);
    let output: serde_json::Value =
        serde_json::from_str(algorithm_run["output_json"].as_str().unwrap()).unwrap();
    assert_eq!(output["algorithm_id"], GOOSE_SLEEP_V1_ID);
    assert_eq!(output["sleep_hr_trend_bpm_per_hour"], -1.2);
    assert_eq!(output["quality_flags"], serde_json::json!([]));
    assert_eq!(
        output["provenance"]["score_policy"],
        "weighted_sleep_v1_components_with_fragmentation_guardrails"
    );
    assert_eq!(output["components"].as_array().unwrap().len(), 7);
    assert_eq!(
        output["component_provenance"]["data_confidence"]["inputs"]["heart_rate_coverage_fraction"],
        0.82
    );
    let run_provenance: serde_json::Value =
        serde_json::from_str(algorithm_run["provenance_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        run_provenance["provenance"]["score_policy"],
        "weighted_sleep_v1_components_with_fragmentation_guardrails"
    );

    let metric_components =
        fs::read_to_string(export_dir.join("data/metric_components.jsonl")).unwrap();
    let component_rows = metric_components
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(component_rows.len(), 7);
    let component_names = component_rows
        .iter()
        .map(|row| row["component_name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(component_names.contains(&"sleep_need_fulfillment"));
    assert!(component_names.contains(&"continuity"));
    assert!(component_names.contains(&"schedule_regularity"));
    assert!(component_names.contains(&"sleep_architecture"));
    assert!(component_names.contains(&"cardiovascular_recovery"));
    assert!(component_names.contains(&"context_adjustment"));
    assert!(component_names.contains(&"data_confidence"));
    for row in component_rows {
        assert_eq!(row["algorithm_id"], GOOSE_SLEEP_V1_ID);
        assert_eq!(
            row["provenance"]["input_source"],
            "algorithm_run.output_json.components"
        );
        assert!(row["score_0_to_100"].is_number());
        assert!(row["weight"].is_number());
        assert!(row["contribution"].is_number());
    }

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.algorithm_run_rows, 1);
    assert_eq!(validation.content.metric_component_rows, 7);
}

#[test]
fn raw_export_can_export_and_validate_activity_families() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("activity.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let session_provenance = serde_json::json!({
        "source": "official_capture",
        "raw_payload_hex": "aa11",
        "nested": {
            "frame_hex": "bb22",
            "inner": {
                "data_bytes": "cc33"
            }
        }
    })
    .to_string();
    let metric_quality_flags = serde_json::json!(["steady", "trusted"]).to_string();
    let metric_provenance = serde_json::json!({
        "source": "session_rollup",
        "sample_bytes": "dd44",
        "nested": {
            "payload_hex": "ee55"
        }
    })
    .to_string();
    let interval_metadata = serde_json::json!({
        "segment_bytes": "ff66",
        "nested": {
            "body_hex": "7788"
        }
    })
    .to_string();
    let interval_provenance = serde_json::json!({
        "source": "manual_split",
        "data_hex": "99aa"
    })
    .to_string();
    let label_provenance = serde_json::json!({
        "source": "manual",
        "payload_hex": "bbcc",
        "nested": {
            "data_bytes": "ddee"
        }
    })
    .to_string();

    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: "activity-session-1",
                source: "official_app",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779843600000,
                activity_type: "running",
                external_activity_type_code: Some("RUN-42"),
                external_activity_type_name: Some("Morning Run"),
                custom_label: Some("morning run"),
                confidence: 0.84,
                detection_method: "official_capture",
                sync_status: "synced",
                provenance_json: &session_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_activity_metric(ActivityMetricInput {
                metric_id: "activity-metric-1",
                activity_session_id: "activity-session-1",
                metric_name: "heart_rate",
                value: 152.5,
                unit: "bpm",
                start_time_unix_ms: 1779840060000,
                end_time_unix_ms: 1779840120000,
                quality_flags_json: &metric_quality_flags,
                provenance_json: &metric_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_activity_interval(ActivityIntervalInput {
                interval_id: "activity-interval-1",
                activity_session_id: "activity-session-1",
                interval_type: "work",
                start_time_unix_ms: 1779840180000,
                end_time_unix_ms: 1779840240000,
                sequence: 1,
                metadata_json: &interval_metadata,
                provenance_json: &interval_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_activity_label(ActivityLabelInput {
                label_id: "activity-label-1",
                activity_session_id: "activity-session-1",
                label_type: "user",
                value: "easy run",
                source: "manual",
                confidence: Some(0.93),
                provenance_json: &label_provenance,
            })
            .unwrap()
    );

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "activity_sessions".to_string(),
                "activity_metrics".to_string(),
                "activity_intervals".to_string(),
                "activity_labels".to_string(),
            ],
            filters: RawExportFilters {
                include_raw_bytes: false,
                ..Default::default()
            },
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.manifest.data_families,
        vec![
            "activity_sessions".to_string(),
            "activity_metrics".to_string(),
            "activity_intervals".to_string(),
            "activity_labels".to_string(),
        ]
    );
    assert_eq!(report.activity_session_rows, 1);
    assert_eq!(report.activity_metric_rows, 1);
    assert_eq!(report.activity_interval_rows, 1);
    assert_eq!(report.activity_label_rows, 1);
    assert!(export_dir.join("data/activity_sessions.jsonl").exists());
    assert!(export_dir.join("data/activity_sessions.csv").exists());
    assert!(export_dir.join("data/activity_metrics.jsonl").exists());
    assert!(export_dir.join("data/activity_metrics.csv").exists());
    assert!(export_dir.join("data/activity_intervals.jsonl").exists());
    assert!(export_dir.join("data/activity_intervals.csv").exists());
    assert!(export_dir.join("data/activity_labels.jsonl").exists());
    assert!(export_dir.join("data/activity_labels.csv").exists());

    let activity_sessions = read_jsonl_values(&export_dir.join("data/activity_sessions.jsonl"));
    let activity_metrics = read_jsonl_values(&export_dir.join("data/activity_metrics.jsonl"));
    let activity_intervals = read_jsonl_values(&export_dir.join("data/activity_intervals.jsonl"));
    let activity_labels = read_jsonl_values(&export_dir.join("data/activity_labels.jsonl"));
    assert_eq!(activity_sessions.len(), 1);
    assert_eq!(activity_metrics.len(), 1);
    assert_eq!(activity_intervals.len(), 1);
    assert_eq!(activity_labels.len(), 1);

    let session_provenance: serde_json::Value =
        serde_json::from_str(activity_sessions[0]["provenance_json"].as_str().unwrap()).unwrap();
    let metric_provenance: serde_json::Value =
        serde_json::from_str(activity_metrics[0]["provenance_json"].as_str().unwrap()).unwrap();
    let metric_quality_flags: serde_json::Value =
        serde_json::from_str(activity_metrics[0]["quality_flags_json"].as_str().unwrap()).unwrap();
    let interval_metadata: serde_json::Value =
        serde_json::from_str(activity_intervals[0]["metadata_json"].as_str().unwrap()).unwrap();
    let interval_provenance: serde_json::Value =
        serde_json::from_str(activity_intervals[0]["provenance_json"].as_str().unwrap()).unwrap();
    let label_provenance: serde_json::Value =
        serde_json::from_str(activity_labels[0]["provenance_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        metric_quality_flags,
        serde_json::json!(["steady", "trusted"])
    );
    assert_no_non_empty_raw_byte_fields(&session_provenance);
    assert_no_non_empty_raw_byte_fields(&metric_provenance);
    assert_no_non_empty_raw_byte_fields(&interval_metadata);
    assert_no_non_empty_raw_byte_fields(&interval_provenance);
    assert_no_non_empty_raw_byte_fields(&label_provenance);

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.activity_session_rows, 1);
    assert_eq!(validation.content.activity_metric_rows, 1);
    assert_eq!(validation.content.activity_interval_rows, 1);
    assert_eq!(validation.content.activity_label_rows, 1);
    assert_eq!(validation.content.csv_row_count_checks, 4);
}

#[test]
fn raw_export_can_export_and_validate_local_health_metric_family() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("local-health.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let daily_inputs = serde_json::json!({
        "algorithm": "goose.energy.local_estimate.v0",
        "raw_payload_hex": "aa11",
        "nested": {
            "frame_hex": "bb22"
        }
    })
    .to_string();
    let hourly_inputs = serde_json::json!({
        "counter_sample_ids": ["step-sample-1", "step-sample-2"],
        "sample_bytes": "cc33"
    })
    .to_string();
    let recovery_inputs = serde_json::json!({
        "hr_frame_ids": ["frame-1", "frame-2"],
        "body_hex": "dd44"
    })
    .to_string();
    let unavailable_recovery_inputs = serde_json::json!({
        "metric_id": "hrv_rmssd_ms",
        "candidate_count": 0,
        "blocker_reasons": ["no_trusted_hrv_rr_intervals"],
        "raw_hex": "aa00"
    })
    .to_string();
    let unavailable_activity_inputs = serde_json::json!({
        "metric_id": "steps",
        "sample_count": 0,
        "blocker_reasons": ["insufficient_step_counter_samples"],
        "raw_hex": "cc00"
    })
    .to_string();
    let unavailable_energy_inputs = serde_json::json!({
        "metric_id": "total_kcal",
        "heart_rate_sample_count": 0,
        "motion_sample_count": 0,
        "blocker_reasons": ["insufficient_heart_rate_samples"],
        "raw_hex": "dd00"
    })
    .to_string();
    let quality_flags = serde_json::json!(["steady", "packet_derived"]).to_string();
    let unavailable_quality_flags =
        serde_json::json!(["recovery_widget_unavailable", "no_trusted_hrv_rr_intervals"])
            .to_string();
    let unavailable_activity_quality_flags = serde_json::json!([
        "activity_steps_unavailable",
        "insufficient_step_counter_samples"
    ])
    .to_string();
    let unavailable_energy_quality_flags = serde_json::json!([
        "total_kcal_unavailable",
        "energy_metric_unavailable",
        "insufficient_heart_rate_samples"
    ])
    .to_string();
    let daily_provenance = serde_json::json!({
        "source": "local_energy_rollup",
        "payload_hex": "ee55"
    })
    .to_string();
    let hourly_provenance = serde_json::json!({
        "source": "step_counter_rollup",
        "data_bytes": "ff66"
    })
    .to_string();
    let recovery_provenance = serde_json::json!({
        "source": "resting_hr_rollup",
        "data_hex": "7788"
    })
    .to_string();
    let unavailable_recovery_provenance = serde_json::json!({
        "algorithm": "goose.recovery.unavailable_status.v0",
        "source_kind": "unavailable",
        "metric_id": "hrv_rmssd_ms",
        "value_policy": "no_metric_value_written_until_packet_semantics_are_verified",
        "payload_hex": "bb11"
    })
    .to_string();
    let unavailable_activity_provenance = serde_json::json!({
        "algorithm": "goose.activity.unavailable_status.v0",
        "source_kind": "unavailable",
        "metric_id": "steps",
        "value_policy": "no_step_value_written_until_whoop_device_counter_or_validated_local_estimator_exists",
        "payload_hex": "cc11"
    })
    .to_string();
    let unavailable_energy_provenance = serde_json::json!({
        "algorithm": "goose.energy.unavailable_status.v0",
        "algorithm_version": "0.1.0",
        "source_kind": "unavailable",
        "metric_id": "total_kcal",
        "value_policy": "no_calorie_value_written_until_whoop_packet_hr_motion_inputs_support_local_estimate",
        "payload_hex": "dd11"
    })
    .to_string();

    assert!(
        store
            .upsert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-2026-05-27-energy",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779926400000,
                steps: None,
                active_kcal: Some(420.5),
                resting_kcal: Some(1710.0),
                total_kcal: Some(2130.5),
                average_cadence_spm: None,
                source_kind: "local_estimate",
                confidence: 0.72,
                inputs_json: &daily_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &daily_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .upsert_hourly_activity_metric(HourlyActivityMetricInput {
                hourly_metric_id: "hourly-activity-2026-05-27-10-step",
                date_key: "2026-05-27T10:00",
                timezone: "Europe/London",
                start_time_unix_ms: 1779876000000,
                end_time_unix_ms: 1779879600000,
                steps: Some(1820),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(92.0),
                source_kind: "device_counter",
                confidence: 0.94,
                inputs_json: &hourly_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &hourly_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .upsert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-2026-05-27-steps-unavailable",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779926400000,
                steps: None,
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: None,
                source_kind: "unavailable",
                confidence: 0.0,
                inputs_json: &unavailable_activity_inputs,
                quality_flags_json: &unavailable_activity_quality_flags,
                provenance_json: &unavailable_activity_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .upsert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-2026-05-27-total-kcal-unavailable",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779926400000,
                steps: None,
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: None,
                source_kind: "unavailable",
                confidence: 0.0,
                inputs_json: &unavailable_energy_inputs,
                quality_flags_json: &unavailable_energy_quality_flags,
                provenance_json: &unavailable_energy_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .upsert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "daily-recovery-2026-05-27-rhr",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779926400000,
                resting_hr_bpm: Some(52.4),
                hrv_rmssd_ms: None,
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.88,
                inputs_json: &recovery_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &recovery_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .upsert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "daily-recovery-2026-05-27-hrv-unavailable",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1779840000000,
                end_time_unix_ms: 1779926400000,
                resting_hr_bpm: None,
                hrv_rmssd_ms: None,
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "unavailable",
                confidence: 0.0,
                inputs_json: &unavailable_recovery_inputs,
                quality_flags_json: &unavailable_quality_flags,
                provenance_json: &unavailable_recovery_provenance,
            })
            .unwrap()
    );

    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "daily-activity-2026-05-27-energy-provenance",
                metric_scope: "daily_activity",
                metric_id: "daily-activity-2026-05-27-energy",
                source_kind: "local_estimate",
                source_detail: "goose.energy.local_estimate.v0",
                confidence: Some(0.72),
                inputs_json: &daily_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &daily_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "daily-activity-2026-05-27-steps-unavailable-provenance",
                metric_scope: "daily_activity",
                metric_id: "daily-activity-2026-05-27-steps-unavailable",
                source_kind: "unavailable",
                source_detail: "activity steps blocked by local WHOOP packet promotion gate",
                confidence: Some(0.0),
                inputs_json: &unavailable_activity_inputs,
                quality_flags_json: &unavailable_activity_quality_flags,
                provenance_json: &unavailable_activity_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "daily-activity-2026-05-27-total-kcal-unavailable-provenance",
                metric_scope: "daily_activity",
                metric_id: "daily-activity-2026-05-27-total-kcal-unavailable",
                source_kind: "unavailable",
                source_detail: "activity calories blocked by local WHOOP packet promotion gate",
                confidence: Some(0.0),
                inputs_json: &unavailable_energy_inputs,
                quality_flags_json: &unavailable_energy_quality_flags,
                provenance_json: &unavailable_energy_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "hourly-activity-2026-05-27-10-step-provenance",
                metric_scope: "hourly_activity",
                metric_id: "hourly-activity-2026-05-27-10-step",
                source_kind: "device_counter",
                source_detail: "goose.step_counter_hourly_rollup.v0",
                confidence: Some(0.94),
                inputs_json: &hourly_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &hourly_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "daily-recovery-2026-05-27-rhr-provenance",
                metric_scope: "daily_recovery",
                metric_id: "daily-recovery-2026-05-27-rhr",
                source_kind: "device_sensor",
                source_detail: "goose.resting_hr_daily_rollup.v0",
                confidence: Some(0.88),
                inputs_json: &recovery_inputs,
                quality_flags_json: &quality_flags,
                provenance_json: &recovery_provenance,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "daily-recovery-2026-05-27-hrv-unavailable-provenance",
                metric_scope: "daily_recovery",
                metric_id: "daily-recovery-2026-05-27-hrv-unavailable",
                source_kind: "unavailable",
                source_detail: "recovery widget blocked by local WHOOP packet promotion gate",
                confidence: Some(0.0),
                inputs_json: &unavailable_recovery_inputs,
                quality_flags_json: &unavailable_quality_flags,
                provenance_json: &unavailable_recovery_provenance,
            })
            .unwrap()
    );

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["local_health_metrics".to_string()],
            filters: RawExportFilters {
                include_raw_bytes: false,
                ..Default::default()
            },
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(
        report.manifest.data_families,
        vec!["local_health_metrics".to_string()]
    );
    assert_eq!(report.daily_activity_metric_rows, 3);
    assert_eq!(report.hourly_activity_metric_rows, 1);
    assert_eq!(report.daily_recovery_metric_rows, 2);
    assert_eq!(report.metric_provenance_rows, 6);
    assert!(
        export_dir
            .join("data/local_health_daily_activity_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_activity_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_hourly_activity_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_hourly_activity_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_recovery_metrics.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_daily_recovery_metrics.csv")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_metric_provenance.jsonl")
            .exists()
    );
    assert!(
        export_dir
            .join("data/local_health_metric_provenance.csv")
            .exists()
    );

    let daily_activity =
        read_jsonl_values(&export_dir.join("data/local_health_daily_activity_metrics.jsonl"));
    let hourly_activity =
        read_jsonl_values(&export_dir.join("data/local_health_hourly_activity_metrics.jsonl"));
    let daily_recovery =
        read_jsonl_values(&export_dir.join("data/local_health_daily_recovery_metrics.jsonl"));
    let metric_provenance =
        read_jsonl_values(&export_dir.join("data/local_health_metric_provenance.jsonl"));
    assert_eq!(daily_activity.len(), 3);
    assert_eq!(hourly_activity.len(), 1);
    assert_eq!(daily_recovery.len(), 2);
    assert_eq!(metric_provenance.len(), 6);

    let daily_activity_inputs: serde_json::Value =
        serde_json::from_str(daily_activity[0]["inputs_json"].as_str().unwrap()).unwrap();
    let daily_activity_provenance: serde_json::Value =
        serde_json::from_str(daily_activity[0]["provenance_json"].as_str().unwrap()).unwrap();
    let hourly_activity_inputs: serde_json::Value =
        serde_json::from_str(hourly_activity[0]["inputs_json"].as_str().unwrap()).unwrap();
    let recovery_inputs: serde_json::Value =
        serde_json::from_str(daily_recovery[0]["inputs_json"].as_str().unwrap()).unwrap();
    let unavailable_recovery = daily_recovery
        .iter()
        .find(|row| row["source_kind"] == "unavailable")
        .unwrap();
    assert!(unavailable_recovery["hrv_rmssd_ms"].is_null());
    assert_eq!(unavailable_recovery["confidence"], 0.0);
    let unavailable_activity = daily_activity
        .iter()
        .find(|row| row["daily_metric_id"] == "daily-activity-2026-05-27-steps-unavailable")
        .unwrap();
    assert!(unavailable_activity["steps"].is_null());
    assert_eq!(unavailable_activity["confidence"], 0.0);
    let unavailable_energy = daily_activity
        .iter()
        .find(|row| row["daily_metric_id"] == "daily-activity-2026-05-27-total-kcal-unavailable")
        .unwrap();
    assert!(unavailable_energy["total_kcal"].is_null());
    assert_eq!(unavailable_energy["confidence"], 0.0);
    let unavailable_activity_inputs: serde_json::Value =
        serde_json::from_str(unavailable_activity["inputs_json"].as_str().unwrap()).unwrap();
    let unavailable_activity_provenance: serde_json::Value =
        serde_json::from_str(unavailable_activity["provenance_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        unavailable_activity_provenance["algorithm"],
        "goose.activity.unavailable_status.v0"
    );
    let unavailable_energy_inputs: serde_json::Value =
        serde_json::from_str(unavailable_energy["inputs_json"].as_str().unwrap()).unwrap();
    let unavailable_energy_provenance: serde_json::Value =
        serde_json::from_str(unavailable_energy["provenance_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        unavailable_energy_provenance["algorithm"],
        "goose.energy.unavailable_status.v0"
    );
    let unavailable_recovery_inputs: serde_json::Value =
        serde_json::from_str(unavailable_recovery["inputs_json"].as_str().unwrap()).unwrap();
    let unavailable_recovery_provenance: serde_json::Value =
        serde_json::from_str(unavailable_recovery["provenance_json"].as_str().unwrap()).unwrap();
    let first_metric_provenance_inputs: serde_json::Value =
        serde_json::from_str(metric_provenance[0]["inputs_json"].as_str().unwrap()).unwrap();
    let unavailable_metric_provenance = metric_provenance
        .iter()
        .find(|row| row["metric_id"] == "daily-activity-2026-05-27-steps-unavailable")
        .unwrap();
    let unavailable_metric_provenance_inputs: serde_json::Value = serde_json::from_str(
        unavailable_metric_provenance["inputs_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let unavailable_energy_metric_provenance = metric_provenance
        .iter()
        .find(|row| row["metric_id"] == "daily-activity-2026-05-27-total-kcal-unavailable")
        .unwrap();
    let unavailable_energy_metric_provenance_inputs: serde_json::Value = serde_json::from_str(
        unavailable_energy_metric_provenance["inputs_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_no_non_empty_raw_byte_fields(&daily_activity_inputs);
    assert_no_non_empty_raw_byte_fields(&daily_activity_provenance);
    assert_no_non_empty_raw_byte_fields(&unavailable_activity_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_activity_provenance);
    assert_no_non_empty_raw_byte_fields(&unavailable_energy_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_energy_provenance);
    assert_no_non_empty_raw_byte_fields(&hourly_activity_inputs);
    assert_no_non_empty_raw_byte_fields(&recovery_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_recovery_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_recovery_provenance);
    assert_no_non_empty_raw_byte_fields(&first_metric_provenance_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_metric_provenance_inputs);
    assert_no_non_empty_raw_byte_fields(&unavailable_energy_metric_provenance_inputs);

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.daily_activity_metric_rows, 3);
    assert_eq!(validation.content.hourly_activity_metric_rows, 1);
    assert_eq!(validation.content.daily_recovery_metric_rows, 2);
    assert_eq!(validation.content.metric_provenance_rows, 6);
    assert_eq!(validation.content.csv_row_count_checks, 4);
}

#[test]
fn validate_export_rejects_malformed_local_health_metric_rows() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();

    let daily_activity_jsonl = serde_json::json!({
        "daily_metric_id": "daily-activity-empty-local",
        "date_key": "2026-06-02",
        "timezone": "Europe/London",
        "start_time_unix_ms": 1779840000000_i64,
        "end_time_unix_ms": 1779843600000_i64,
        "steps": null,
        "active_kcal": null,
        "resting_kcal": null,
        "total_kcal": null,
        "average_cadence_spm": null,
        "source_kind": "local_estimate",
        "confidence": 0.5,
        "inputs_json": "{\"source\":\"healthkit_step_count\"}",
        "quality_flags_json": "[]",
        "provenance_json": "{\"source\":\"official_whoop_app\"}",
        "created_at": "2026-06-02T00:00:00Z",
        "updated_at": "2026-06-02T00:00:00Z"
    })
    .to_string()
        + "\n";
    let hourly_activity_jsonl = serde_json::json!({
        "hourly_metric_id": "hourly-activity-valued-unavailable",
        "date_key": "2026-06-02T10:00",
        "timezone": "Europe/London",
        "start_time_unix_ms": 1779876000000_i64,
        "end_time_unix_ms": 1779879600000_i64,
        "steps": 1,
        "active_kcal": null,
        "resting_kcal": null,
        "total_kcal": null,
        "average_cadence_spm": null,
        "source_kind": "unavailable",
        "confidence": 0.2,
        "inputs_json": "{}",
        "quality_flags_json": "[\"platform_import_not_syncable\"]",
        "provenance_json": "{}",
        "created_at": "2026-06-02T10:00:00Z",
        "updated_at": "2026-06-02T10:00:00Z"
    })
    .to_string()
        + "\n";
    let daily_recovery_jsonl = serde_json::json!({
        "daily_metric_id": "daily-recovery-empty-device",
        "date_key": "2026-06-02",
        "timezone": "Europe/London",
        "start_time_unix_ms": 1779840000000_i64,
        "end_time_unix_ms": 1779926400000_i64,
        "resting_hr_bpm": null,
        "hrv_rmssd_ms": null,
        "respiratory_rate_rpm": null,
        "oxygen_saturation_percent": null,
        "skin_temperature_delta_c": null,
        "source_kind": "device_sensor",
        "confidence": 0.7,
        "inputs_json": "{}",
        "quality_flags_json": "[\"official_whoop_label\"]",
        "provenance_json": "{\"platform\":\"health_connect\"}",
        "created_at": "2026-06-02T00:00:00Z",
        "updated_at": "2026-06-02T00:00:00Z"
    })
    .to_string()
        + "\n";
    let metric_provenance_jsonl = serde_json::json!({
        "provenance_id": "prov-hourly-activity-unavailable-confident",
        "metric_scope": "hourly_activity",
        "metric_id": "hourly-activity-valued-unavailable",
        "source_kind": "unavailable",
        "source_detail": "official_whoop_app HealthKit step count import",
        "confidence": 0.4,
        "inputs_json": "{\"source\":\"apple_health_steps\"}",
        "quality_flags_json": "[]",
        "provenance_json": "{\"official_whoop_label\":true,\"source\":\"health_connect\"}",
        "created_at": "2026-06-02T10:00:00Z"
    })
    .to_string()
        + "\n";

    let files = vec![
        (
            "data/local_health_daily_activity_metrics.jsonl",
            daily_activity_jsonl,
            1_u64,
            "jsonl",
        ),
        (
            "data/local_health_daily_activity_metrics.csv",
            "header\nrow\n".to_string(),
            1_u64,
            "csv",
        ),
        (
            "data/local_health_hourly_activity_metrics.jsonl",
            hourly_activity_jsonl,
            1_u64,
            "jsonl",
        ),
        (
            "data/local_health_hourly_activity_metrics.csv",
            "header\nrow\n".to_string(),
            1_u64,
            "csv",
        ),
        (
            "data/local_health_daily_recovery_metrics.jsonl",
            daily_recovery_jsonl,
            1_u64,
            "jsonl",
        ),
        (
            "data/local_health_daily_recovery_metrics.csv",
            "header\nrow\n".to_string(),
            1_u64,
            "csv",
        ),
        (
            "data/local_health_metric_provenance.jsonl",
            metric_provenance_jsonl,
            1_u64,
            "jsonl",
        ),
        (
            "data/local_health_metric_provenance.csv",
            "header\nrow\n".to_string(),
            1_u64,
            "csv",
        ),
    ];

    let manifest_files = files
        .iter()
        .map(|(path, text, row_count, kind)| {
            fs::write(tempdir.path().join(path), text.as_bytes()).unwrap();
            serde_json::json!({
                "path": path,
                "sha256": sha256_hex(text.as_bytes()),
                "row_count": row_count,
                "kind": kind,
            })
        })
        .collect::<Vec<_>>();

    fs::write(
        tempdir.path().join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "goose.export.v1",
            "app_version": "goose-app/test",
            "core_version": "goose-core/test",
            "time_window": {"start": "2026-06-02T00:00:00Z", "end": "2026-06-03T00:00:00Z"},
            "data_families": ["local_health_metrics"],
            "filters": {"include_raw_bytes": false},
            "official_labels_are_labels": true,
            "files": manifest_files,
        }))
        .unwrap(),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass, "{:?}", report.issues);
    assert!(!report.content.pass);
    assert_eq!(report.content.daily_activity_metric_rows, 1);
    assert_eq!(report.content.hourly_activity_metric_rows, 1);
    assert_eq!(report.content.daily_recovery_metric_rows, 1);
    assert_eq!(report.content.metric_provenance_rows, 1);
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily activity metric daily-activity-empty-local available activity metric must include steps or calorie values",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily activity metric daily-activity-empty-local inputs_json must not contain HealthKit, Health Connect, Apple Health, or platform-import markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily activity metric daily-activity-empty-local provenance_json must not contain official WHOOP label markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "hourly activity metric hourly-activity-valued-unavailable unavailable activity metric must not carry metric values",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "hourly activity metric hourly-activity-valued-unavailable unavailable formatted metric must have confidence 0.0",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "hourly activity metric hourly-activity-valued-unavailable quality_flags_json must not contain HealthKit, Health Connect, Apple Health, or platform-import markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily recovery metric daily-recovery-empty-device available recovery metric must include at least one recovery value",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily recovery metric daily-recovery-empty-device quality_flags_json must not contain official WHOOP label markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "daily recovery metric daily-recovery-empty-device provenance_json must not contain HealthKit, Health Connect, Apple Health, or platform-import markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident source_detail must not identify official WHOOP labels",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident source_detail must not identify HealthKit, Health Connect, Apple Health, or platform imports",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident unavailable metric provenance must have confidence 0.0",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident inputs_json must not contain HealthKit, Health Connect, Apple Health, or platform-import markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident provenance_json must not contain official WHOOP label markers",
        )
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "metric provenance prov-hourly-activity-unavailable-confident provenance_json must not contain HealthKit, Health Connect, Apple Health, or platform-import markers",
        )
    }));
}

#[test]
fn validate_export_rejects_invalid_activity_metric_reimport() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();

    let session_jsonl = serde_json::json!({
        "session_id": "activity-session-1",
        "source": "official_app",
        "start_time_unix_ms": 1779840000000_i64,
        "end_time_unix_ms": 1779843600000_i64,
        "duration_ms": 3600000_i64,
        "activity_type": "running",
        "external_activity_type_code": "RUN-42",
        "external_activity_type_name": "Morning Run",
        "custom_label": "morning run",
        "confidence": 0.84,
        "detection_method": "official_capture",
        "sync_status": "synced",
        "provenance_json": "{\"source\":\"manual\"}",
        "created_at": "2026-05-27T00:00:00Z",
        "updated_at": "2026-05-27T00:00:00Z"
    })
    .to_string()
        + "\n";
    let metric_jsonl = serde_json::json!({
        "metric_id": "activity-metric-bad",
        "activity_session_id": "missing-session",
        "metric_name": "heart_rate",
        "value": 152.5,
        "unit": "watts",
        "start_time_unix_ms": 1779840060000_i64,
        "end_time_unix_ms": 1779840120000_i64,
        "quality_flags_json": "[\"steady\"]",
        "provenance_json": "{\"source\":\"manual\"}",
        "created_at": "2026-05-27T00:10:00Z"
    })
    .to_string()
        + "\n";
    let session_csv = csv_row(&[
        "activity-session-1",
        "official_app",
        "1779840000000",
        "1779843600000",
        "3600000",
        "running",
        "RUN-42",
        "Morning Run",
        "morning run",
        "0.84",
        "official_capture",
        "synced",
        "{\"source\":\"manual\"}",
        "2026-05-27T00:00:00Z",
        "2026-05-27T00:00:00Z",
    ]);
    let metric_csv = csv_row(&[
        "activity-metric-bad",
        "missing-session",
        "heart_rate",
        "152.5",
        "watts",
        "1779840060000",
        "1779840120000",
        "[\"steady\"]",
        "{\"source\":\"manual\"}",
        "2026-05-27T00:10:00Z",
    ]);
    fs::write(
        tempdir.path().join("data/activity_sessions.jsonl"),
        session_jsonl.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/activity_sessions.csv"),
        session_csv.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/activity_metrics.jsonl"),
        metric_jsonl.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/activity_metrics.csv"),
        metric_csv.as_bytes(),
    )
    .unwrap();

    let session_sha256 = sha256_hex(session_jsonl.as_bytes());
    let session_csv_sha256 = sha256_hex(session_csv.as_bytes());
    let metric_sha256 = sha256_hex(metric_jsonl.as_bytes());
    let metric_csv_sha256 = sha256_hex(metric_csv.as_bytes());
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "goose-app/test",
  "core_version": "goose-core/test",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-28T00:00:00Z"}},
  "data_families": ["activity_sessions", "activity_metrics"],
  "filters": {{"include_raw_bytes": false}},
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/activity_sessions.jsonl", "sha256": "{session_sha256}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/activity_sessions.csv", "sha256": "{session_csv_sha256}", "row_count": 1, "kind": "csv"}},
    {{"path": "data/activity_metrics.jsonl", "sha256": "{metric_sha256}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/activity_metrics.csv", "sha256": "{metric_csv_sha256}", "row_count": 1, "kind": "csv"}}
  ]
}}"#
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass, "{:?}", report.issues);
    assert_eq!(report.content.activity_session_rows, 1);
    assert_eq!(report.content.activity_metric_rows, 1);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("activity metric activity-metric-bad unit must be one of")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "activity metric activity-metric-bad activity_session_id missing-session is missing from activity session export",
        )
    }));
    assert!(
        report.next_actions.iter().any(|action| {
            action.reason == "activity_export_shape" && action.scope == "content"
        })
    );
    assert!(
        report.next_actions.iter().any(|action| {
            action.reason == "broken_export_reference" && action.scope == "content"
        })
    );
}

#[test]
fn validate_export_rejects_invalid_activity_session_reimport() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();

    let session_jsonl = serde_json::json!({
        "session_id": "activity-session-bad",
        "source": "official_app",
        "start_time_unix_ms": 1779840000000_i64,
        "end_time_unix_ms": 1779843600000_i64,
        "duration_ms": 3599000_i64,
        "activity_type": "running",
        "external_activity_type_code": "RUN-42",
        "external_activity_type_name": "Morning Run",
        "custom_label": "morning run",
        "confidence": 0.84,
        "detection_method": "official_capture",
        "sync_status": "synced",
        "provenance_json": "{\"source\":\"manual\"}",
        "created_at": "2026-05-27T00:00:00Z",
        "updated_at": "2026-05-27T00:00:00Z"
    })
    .to_string()
        + "\n";
    let session_csv = csv_row(&[
        "activity-session-bad",
        "official_app",
        "1779840000000",
        "1779843600000",
        "3599000",
        "running",
        "RUN-42",
        "Morning Run",
        "morning run",
        "0.84",
        "official_capture",
        "synced",
        "{\"source\":\"manual\"}",
        "2026-05-27T00:00:00Z",
        "2026-05-27T00:00:00Z",
    ]);
    fs::write(
        tempdir.path().join("data/activity_sessions.jsonl"),
        session_jsonl.as_bytes(),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/activity_sessions.csv"),
        session_csv.as_bytes(),
    )
    .unwrap();

    let session_sha256 = sha256_hex(session_jsonl.as_bytes());
    let session_csv_sha256 = sha256_hex(session_csv.as_bytes());
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "goose-app/test",
  "core_version": "goose-core/test",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-28T00:00:00Z"}},
  "data_families": ["activity_sessions"],
  "filters": {{"include_raw_bytes": false}},
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/activity_sessions.jsonl", "sha256": "{session_sha256}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/activity_sessions.csv", "sha256": "{session_csv_sha256}", "row_count": 1, "kind": "csv"}}
  ]
}}"#
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass, "{:?}", report.issues);
    assert_eq!(report.content.activity_session_rows, 1);
    assert!(report.issues.iter().any(|issue| {
        issue.contains(
            "activity session activity-session-bad duration_ms does not match end_time_unix_ms - start_time_unix_ms",
        )
    }));
    assert!(
        report.next_actions.iter().any(|action| {
            action.reason == "activity_export_shape" && action.scope == "content"
        })
    );
}

#[test]
fn export_validator_rejects_algorithm_runs_with_untrusted_provided_vitals() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("bad-algorithm-run.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    store
        .insert_algorithm_run(&AlgorithmRunRecord {
            run_id: "bad-recovery-run".to_string(),
            algorithm_id: "goose.recovery.v0".to_string(),
            version: "0.1.0".to_string(),
            start_time: "2026-05-27T00:00:00Z".to_string(),
            end_time: "2026-05-27T23:59:00Z".to_string(),
            output_json: r#"{"algorithm_id":"goose.recovery.v0","algorithm_version":"0.1.0","score_0_to_100":72.0,"components":[]}"#
                .to_string(),
            quality_flags_json: "[]".to_string(),
            provenance_json: r#"{"provenance":{"input_ids":["manual-vitals"],"provided_vitals":{"metric_input_id":"provided_recovery_vitals.2026-05-27","source":"manual_test","trusted_metric_input":false,"quality_flags":["provided_resp_temp_inputs_not_packet_derived","provided_resp_temp_provenance_untrusted"],"provenance":{"input_source":"manual_test","provided_vitals_provenance":{}}}},"errors":[]}"#
                .to_string(),
        })
        .unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["algorithm_runs".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();
    assert!(report.pass, "{:?}", report.issues);

    let validation = validate_export_bundle(&export_dir).unwrap();

    assert!(!validation.pass);
    assert_eq!(validation.content.algorithm_run_rows, 1);
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "algorithm run bad-recovery-run provided_vitals.trusted_metric_input must be true",
        )
    }));
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "algorithm run bad-recovery-run provided_vitals quality_flags must not include provided_resp_temp_inputs_not_packet_derived",
        )
    }));
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "algorithm run bad-recovery-run provided_vitals quality_flags must not include provided_resp_temp_provenance_untrusted",
        )
    }));
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "algorithm run bad-recovery-run provided_vitals.provenance.provided_vitals_provenance must be a non-empty object",
        )
    }));
    assert!(
        validation
            .next_actions
            .iter()
            .any(|action| { action.reason == "algorithm_run_shape" && action.scope == "content" })
    );
}

#[test]
fn export_validator_rejects_failed_or_malformed_calibration_runs() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("bad-calibration-run.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    store
        .insert_calibration_run(&CalibrationRunRecord {
            calibration_run_id: "bad-calibration-run".to_string(),
            algorithm_id: "goose.recovery.v0".to_string(),
            version: "0.1.0".to_string(),
            times: CalibrationRunTimes {
                train_start: "2026-05-01T00:00:00Z".to_string(),
                train_end: "2026-05-05T00:00:00Z".to_string(),
                holdout_start: "2026-05-04T00:00:00Z".to_string(),
                holdout_end: "2026-05-06T00:00:00Z".to_string(),
            },
            metrics_json: serde_json::json!({
                "dataset_valid": true,
                "labels_valid": true,
                "split_valid": true,
                "model_fit_ready": true,
                "train_metrics_ready": true,
                "holdout_metrics_ready": true,
                "holdout_improvement_valid": false,
                "calibration_ready": false,
                "issues": ["holdout_not_improved"],
                "next_actions": []
            })
            .to_string(),
            params_json: serde_json::json!({
                "model": {
                    "model_type": "ordinary_least_squares_1d",
                    "slope": 1.0,
                    "intercept": 0.0
                },
                "split_policy": "",
                "dataset_valid": true,
                "labels_valid": true,
                "split_valid": true,
                "model_fit_ready": true,
                "holdout_improvement_valid": false,
                "calibration_ready": false,
                "pass": false
            })
            .to_string(),
        })
        .unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["calibration_runs".to_string()],
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();
    assert!(report.pass, "{:?}", report.issues);

    let validation = validate_export_bundle(&export_dir).unwrap();

    assert!(!validation.pass);
    assert_eq!(validation.content.calibration_run_rows, 1);
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "calibration run bad-calibration-run train_end must not be after holdout_start",
        )
    }));
    assert!(validation.issues.iter().any(|issue| {
        issue.contains(
            "calibration run bad-calibration-run metrics_json.calibration_ready must be true",
        )
    }));
    assert!(validation.issues.iter().any(|issue| {
        issue.contains("calibration run bad-calibration-run params_json.pass must be true")
    }));
    assert!(
        validation.next_actions.iter().any(|action| {
            action.reason == "calibration_run_shape" && action.scope == "content"
        })
    );
}

#[test]
fn export_validator_rejects_malformed_debug_rows() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let session_rows = br#"{"session_id":"debug-session-1","started_at_unix_ms":1,"bridge_url":"ws://0.0.0.0:49152/goose-debug/stream?token=secret","bind_host":"0.0.0.0","token_required":false,"token_present":false,"remote_bind_enabled":true,"visible_remote_bind_toggle":false}
"#;
    let command_rows = br#"{"command_id":"debug-command-1","session_id":"missing-session","schema":"bad.schema","command":"export.raw_timeframe","args_json":"{\"frame_hex\":\"aa\",\"url\":\"ws://127.0.0.1/goose-debug/stream?token=secret\"}","dry_run":false,"received_at_unix_ms":2}
"#;
    let event_rows = br#"{"session_id":"debug-session-1","sequence":1,"schema":"goose.debug.event.v1","time_unix_ms":3,"source":"app","level":"info","topic":"export.started","message":"stream token=secret","command_id":"missing-command","data_json":"{\"payload_hex\":\"aa\",\"url\":\"ws://127.0.0.1/goose-debug/stream?token=secret\"}"}
{"session_id":"debug-session-1","sequence":1,"schema":"bad.event","time_unix_ms":2,"source":"app","level":"info","topic":"export.started","message":"duplicate","command_id":"debug-command-1","data_json":"{}"}
"#;
    let session_csv = b"session_id,started_at_unix_ms,bridge_url,bind_host,token_required,token_present,remote_bind_enabled,visible_remote_bind_toggle\ndebug-session-1,1,ws://0.0.0.0:49152/goose-debug/stream?token=secret,0.0.0.0,false,false,true,false\n";
    let command_csv =
        b"command_id,session_id,schema,command,args_json,dry_run,received_at_unix_ms\ndebug-command-1,missing-session,bad.schema,export.raw_timeframe,{},false,2\n";
    let event_csv = b"session_id,sequence,schema,time_unix_ms,source,level,topic,message,command_id,data_json\ndebug-session-1,1,goose.debug.event.v1,3,app,info,export.started,stream token=secret,missing-command,{}\ndebug-session-1,1,bad.event,2,app,info,export.started,duplicate,debug-command-1,{}\n";
    fs::write(
        tempdir.path().join("data/debug_sessions.jsonl"),
        session_rows,
    )
    .unwrap();
    fs::write(
        tempdir.path().join("data/debug_commands.jsonl"),
        command_rows,
    )
    .unwrap();
    fs::write(tempdir.path().join("data/debug_events.jsonl"), event_rows).unwrap();
    fs::write(tempdir.path().join("data/debug_sessions.csv"), session_csv).unwrap();
    fs::write(tempdir.path().join("data/debug_commands.csv"), command_csv).unwrap();
    fs::write(tempdir.path().join("data/debug_events.csv"), event_csv).unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-28T00:00:00Z"}},
  "data_families": ["debug_sessions", "debug_commands", "debug_events"],
  "filters": {{"include_raw_bytes": false}},
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/debug_sessions.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/debug_sessions.csv", "sha256": "{}", "row_count": 1, "kind": "csv"}},
    {{"path": "data/debug_commands.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/debug_commands.csv", "sha256": "{}", "row_count": 1, "kind": "csv"}},
    {{"path": "data/debug_events.jsonl", "sha256": "{}", "row_count": 2, "kind": "jsonl"}},
    {{"path": "data/debug_events.csv", "sha256": "{}", "row_count": 2, "kind": "csv"}}
  ]
}}"#,
            sha256_hex(session_rows),
            sha256_hex(session_csv),
            sha256_hex(command_rows),
            sha256_hex(command_csv),
            sha256_hex(event_rows),
            sha256_hex(event_csv),
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert_eq!(report.content.debug_session_rows, 1);
    assert_eq!(report.content.debug_command_rows, 1);
    assert_eq!(report.content.debug_event_rows, 2);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug session debug-session-1 bind_host must be loopback")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug command debug-command-1 schema must be goose.debug.command.v1")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug command debug-command-1 session_id missing-session is missing from debug session export")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug event debug-session-1:1 command_id missing-command is missing from debug command export")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug event debug-session-1:1 data_json.payload_hex must be empty when include_raw_bytes is false")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("debug event debug-session-1:1 contains an unredacted token query parameter")
    }));
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| action.reason == "debug_export_shape" && action.scope == "content")
    );
}

#[test]
fn raw_export_filters_algorithm_outputs_and_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("filtered-metrics.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    let hrv_result = goose_hrv_v0(&HrvInput {
        start_time: "2026-05-27T00:15:00Z".to_string(),
        end_time: "2026-05-27T00:20:00Z".to_string(),
        rr_intervals_ms: vec![800.0, 810.0, 790.0, 800.0],
        input_ids: vec!["metric-filter-test".to_string()],
    });
    let hrv_record = hrv_run_record("filter-hrv-run", &hrv_result).unwrap();
    store.insert_algorithm_run(&hrv_record).unwrap();
    store
        .insert_algorithm_run(&AlgorithmRunRecord {
            run_id: "filter-recovery-run".to_string(),
            algorithm_id: "goose.recovery.v0".to_string(),
            version: "0.1.0".to_string(),
            start_time: "2026-05-27T00:00:00Z".to_string(),
            end_time: "2026-05-27T23:59:00Z".to_string(),
            output_json: r#"{"algorithm_id":"goose.recovery.v0","algorithm_version":"0.1.0","score_0_to_100":72.0,"components":[]}"#
                .to_string(),
            quality_flags_json: "[]".to_string(),
            provenance_json: r#"{"input_ids":["metric-filter-recovery"]}"#.to_string(),
        })
        .unwrap();
    store
        .insert_calibration_label(CalibrationLabelInput {
            label_id: "manual.hrv.2026-05-27",
            metric_family: "hrv",
            label_source: "manual",
            captured_at: "2026-05-27T12:00:00Z",
            value: 14.14,
            unit: "ms",
            provenance_json: r#"{"entry":"typed_by_user","official_labels_are_labels":true}"#,
        })
        .unwrap();
    store
        .insert_calibration_label(CalibrationLabelInput {
            label_id: "manual.recovery.2026-05-27",
            metric_family: "recovery",
            label_source: "manual",
            captured_at: "2026-05-27T12:00:00Z",
            value: 72.0,
            unit: "score_0_to_100",
            provenance_json: r#"{"entry":"typed_by_user","official_labels_are_labels":true}"#,
        })
        .unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "metric_outputs".to_string(),
                "algorithm_runs".to_string(),
                "calibration_labels".to_string(),
            ],
            filters: RawExportFilters {
                include_raw_bytes: true,
                capture_session_ids: Vec::new(),
                packet_type_names: Vec::new(),
                sensor_source_signals: Vec::new(),
                metric_families: vec![" hrv ".to_string(), "hrv".to_string()],
                algorithm_ids: vec!["goose.hrv.v0".to_string()],
                algorithm_versions: vec!["0.1.0".to_string()],
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.algorithm_run_rows, 1);
    assert_eq!(report.metric_value_rows, 7);
    assert_eq!(report.metric_component_rows, 4);
    assert_eq!(report.calibration_label_rows, 1);
    assert_eq!(report.manifest.filters.metric_families, vec!["hrv"]);
    assert_eq!(report.manifest.filters.algorithm_ids, vec!["goose.hrv.v0"]);

    let algorithm_runs = fs::read_to_string(export_dir.join("data/algorithm_runs.jsonl")).unwrap();
    let metric_values = fs::read_to_string(export_dir.join("data/metric_values.jsonl")).unwrap();
    let calibration_labels =
        fs::read_to_string(export_dir.join("data/calibration_labels.jsonl")).unwrap();
    assert!(algorithm_runs.contains("filter-hrv-run"));
    assert!(!algorithm_runs.contains("filter-recovery-run"));
    assert!(metric_values.contains("filter-hrv-run.rmssd_ms"));
    assert!(!metric_values.contains("score_0_to_100"));
    assert!(calibration_labels.contains("manual.hrv.2026-05-27"));
    assert!(!calibration_labels.contains("manual.recovery.2026-05-27"));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.metric_value_rows, 7);
    assert_eq!(validation.content.calibration_label_rows, 1);
}

#[test]
fn raw_export_filters_metric_feature_reports_by_metric_family() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("filtered-features.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["metric_features".to_string()],
            filters: RawExportFilters {
                include_raw_bytes: true,
                capture_session_ids: Vec::new(),
                packet_type_names: Vec::new(),
                sensor_source_signals: Vec::new(),
                metric_families: vec!["hrv".to_string()],
                algorithm_ids: Vec::new(),
                algorithm_versions: Vec::new(),
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.metric_feature_report_rows, 1);
    let metric_features =
        fs::read_to_string(export_dir.join("data/metric_features.jsonl")).unwrap();
    assert!(metric_features.contains("\"report_kind\":\"hrv\""));
    assert!(!metric_features.contains("\"report_kind\":\"heart_rate\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.metric_feature_report_rows, 1);
}

#[test]
fn raw_export_filters_capture_session_rows() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("filtered-capture-session.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    for session_id in ["capture-session-a", "capture-session-b"] {
        store
            .start_capture_session(CaptureSessionInput {
                session_id,
                source: "ios.corebluetooth.notification",
                started_at_unix_ms: 1770000000000,
                device_model: "WHOOP 5.0 Goose",
                active_device_id: None,
                provenance_json: "{}",
            })
            .unwrap();
    }

    let frames = vec![
        CapturedFrameInput {
            evidence_id: "capture-a-k10".to_string(),
            frame_id: Some("capture-a-k10.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:00Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: K10_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: Some("capture-session-a".to_string()),
            device_type: DeviceType::Goose,
        },
        CapturedFrameInput {
            evidence_id: "capture-b-k10".to_string(),
            frame_id: Some("capture-b-k10.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:01Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: K10_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: Some("capture-session-b".to_string()),
            device_type: DeviceType::Goose,
        },
    ];
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);
    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "raw_evidence".to_string(),
                "decoded_frames".to_string(),
                "packet_timeline".to_string(),
                "sensor_samples".to_string(),
            ],
            filters: RawExportFilters {
                include_raw_bytes: true,
                capture_session_ids: vec![
                    " capture-session-a ".to_string(),
                    "capture-session-a".to_string(),
                ],
                packet_type_names: Vec::new(),
                sensor_source_signals: Vec::new(),
                metric_families: Vec::new(),
                algorithm_ids: Vec::new(),
                algorithm_versions: Vec::new(),
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 1);
    assert_eq!(report.decoded_frame_rows, 1);
    assert_eq!(report.packet_timeline_rows, 1);
    assert!(report.sensor_sample_rows > 0);
    assert_eq!(
        report.manifest.filters.capture_session_ids,
        vec!["capture-session-a"]
    );

    let raw_evidence = fs::read_to_string(export_dir.join("data/raw_evidence.jsonl")).unwrap();
    let decoded_frames = fs::read_to_string(export_dir.join("data/decoded_frames.jsonl")).unwrap();
    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(raw_evidence.contains("capture-a-k10"));
    assert!(raw_evidence.contains("capture-session-a"));
    assert!(!raw_evidence.contains("capture-b-k10"));
    assert!(decoded_frames.contains("capture-a-k10.frame.0"));
    assert!(!decoded_frames.contains("capture-b-k10.frame.0"));
    assert!(sensor_samples.contains("capture-a-k10.frame.0"));
    assert!(!sensor_samples.contains("capture-b-k10.frame.0"));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 1);
    assert_eq!(validation.content.decoded_frame_rows, 1);
    assert_eq!(validation.content.packet_timeline_rows, 1);
    assert_eq!(
        validation.content.sensor_sample_rows,
        report.sensor_sample_rows
    );
}

#[test]
fn raw_export_filters_packet_type_and_sensor_source_rows() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("filtered-packet-signal.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let frames = vec![
        CapturedFrameInput {
            evidence_id: "command-frame".to_string(),
            frame_id: Some("command-frame.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:00Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: GET_HELLO_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
        CapturedFrameInput {
            evidence_id: "motion-k10-frame".to_string(),
            frame_id: Some("motion-k10-frame.frame.0".to_string()),
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-27T00:00:01Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: K10_FRAME.to_string(),
            sensitivity: "user-owned-live-notification".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        },
    ];
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);
    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "raw_evidence".to_string(),
                "decoded_frames".to_string(),
                "packet_timeline".to_string(),
                "sensor_samples".to_string(),
            ],
            filters: RawExportFilters {
                include_raw_bytes: true,
                capture_session_ids: Vec::new(),
                packet_type_names: vec![
                    " REALTIME_RAW_DATA ".to_string(),
                    "REALTIME_RAW_DATA".to_string(),
                ],
                sensor_source_signals: vec![
                    "raw_motion_k10".to_string(),
                    "raw_motion_k10".to_string(),
                ],
                metric_families: Vec::new(),
                algorithm_ids: Vec::new(),
                algorithm_versions: Vec::new(),
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 1);
    assert_eq!(report.decoded_frame_rows, 1);
    assert_eq!(report.packet_timeline_rows, 1);
    assert!(report.sensor_sample_rows > 0);
    assert_eq!(
        report.manifest.filters.packet_type_names,
        vec!["REALTIME_RAW_DATA"]
    );
    assert_eq!(
        report.manifest.filters.sensor_source_signals,
        vec!["raw_motion_k10"]
    );

    let raw_evidence = fs::read_to_string(export_dir.join("data/raw_evidence.jsonl")).unwrap();
    let decoded_frames = fs::read_to_string(export_dir.join("data/decoded_frames.jsonl")).unwrap();
    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(raw_evidence.contains("motion-k10-frame"));
    assert!(!raw_evidence.contains("command-frame"));
    assert!(decoded_frames.contains("\"packet_type_name\":\"REALTIME_RAW_DATA\""));
    assert!(!decoded_frames.contains("\"packet_type_name\":\"COMMAND\""));
    assert!(sensor_samples.contains("\"source_signal\":\"raw_motion_k10\""));
    assert!(!sensor_samples.contains("\"source_signal\":\"raw_motion_k10_heart_rate\""));

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 1);
    assert_eq!(validation.content.decoded_frame_rows, 1);
    assert_eq!(validation.content.packet_timeline_rows, 1);
    assert_eq!(
        validation.content.sensor_sample_rows,
        report.sensor_sample_rows
    );
}

#[test]
fn raw_export_can_omit_raw_bytes_but_keep_hashes_and_decoded_samples() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("hash-only.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let frames = vec![CapturedFrameInput {
        evidence_id: "hash-only-motion".to_string(),
        frame_id: Some("hash-only-motion.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-27T00:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: K10_FRAME.to_string(),
        sensitivity: "user-owned-live-notification".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);
    seed_command_validation_record(&store);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec![
                "raw_evidence".to_string(),
                "decoded_frames".to_string(),
                "packet_timeline".to_string(),
                "sensor_samples".to_string(),
                "command_validation".to_string(),
            ],
            filters: RawExportFilters {
                include_raw_bytes: false,
                capture_session_ids: Vec::new(),
                packet_type_names: Vec::new(),
                sensor_source_signals: Vec::new(),
                metric_families: Vec::new(),
                algorithm_ids: Vec::new(),
                algorithm_versions: Vec::new(),
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.raw_rows, 1);
    assert_eq!(report.decoded_frame_rows, 1);
    assert_eq!(report.packet_timeline_rows, 1);
    assert!(report.sensor_sample_rows > 0);
    assert_eq!(report.command_validation_rows, 1);
    assert!(!report.manifest.filters.include_raw_bytes);

    let raw_rows = read_jsonl_values(&export_dir.join("data/raw_evidence.jsonl"));
    assert_eq!(raw_rows[0]["payload_hex"], "");
    assert_eq!(raw_rows[0]["sha256"].as_str().unwrap().len(), 64);

    let decoded_rows = read_jsonl_values(&export_dir.join("data/decoded_frames.jsonl"));
    assert_eq!(decoded_rows[0]["payload_hex"], "");
    let parsed_payload: serde_json::Value =
        serde_json::from_str(decoded_rows[0]["parsed_payload_json"].as_str().unwrap()).unwrap();
    assert_no_non_empty_raw_byte_fields(&parsed_payload);

    let timeline_rows = read_jsonl_values(&export_dir.join("data/packet_timeline.jsonl"));
    assert!(timeline_rows[0]["body_hex"].is_null());
    assert_no_non_empty_raw_byte_fields(&timeline_rows[0]["summary"]);

    let sensor_samples = fs::read_to_string(export_dir.join("data/sensor_samples.jsonl")).unwrap();
    assert!(sensor_samples.contains("\"source_signal\":\"raw_motion_k10\""));
    assert!(!sensor_samples.contains(K10_FRAME.split_whitespace().next().unwrap()));
    let command_validation = read_jsonl_values(&export_dir.join("data/command_validation.jsonl"));
    assert_no_non_empty_raw_byte_fields(&command_validation[0]["report_json"]);
    assert_eq!(
        command_validation[0]["report_json"]["validated_write_type"],
        "with_response"
    );
    assert_eq!(
        command_validation[0]["validated_capture_kind"],
        "official_app_to_macos_emulator"
    );

    let validation = validate_export_bundle(&export_dir).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert_eq!(validation.content.raw_evidence_rows, 1);
    assert_eq!(validation.content.command_validation_rows, 1);
    assert_eq!(validation.content.decoded_frame_rows, 1);
    assert_eq!(validation.content.packet_timeline_rows, 1);
    assert_eq!(
        validation.content.sensor_sample_rows,
        report.sensor_sample_rows
    );
}

#[test]
fn raw_export_rejects_sqlite_when_raw_bytes_are_omitted() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("hash-only-sqlite.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-27T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sqlite".to_string()],
            filters: RawExportFilters {
                include_raw_bytes: false,
                ..Default::default()
            },
            sqlite_source_path: Some(&db_path),
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(!report.input_valid);
    assert!(report.data_families_valid);
    assert!(report.filters_valid);
    assert!(report.time_window_valid);
    assert!(report.version_fields_valid);
    assert!(!report.sqlite_policy_valid);
    assert!(!report.manifest_ready);
    assert!(!report.files_written);
    assert!(report.zip_ready);
    assert!(!report.export_ready);
    assert!(report.issues.iter().any(|issue| {
        issue.contains("sqlite data family cannot be exported when include_raw_bytes is false")
    }));
}

#[test]
fn raw_export_rejects_unknown_or_unavailable_data_families() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let unknown_export_dir = tempdir.path().join("unknown.goosebundle");
    let sqlite_export_dir = tempdir.path().join("sqlite.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let unknown_report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &unknown_export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["raw_evidence".to_string(), "private_api".to_string()],
            filters: Default::default(),
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(!unknown_report.pass);
    assert!(!unknown_report.input_valid);
    assert!(!unknown_report.data_families_valid);
    assert!(unknown_report.filters_valid);
    assert!(unknown_report.time_window_valid);
    assert!(unknown_report.version_fields_valid);
    assert!(unknown_report.sqlite_policy_valid);
    assert!(!unknown_report.export_ready);
    assert!(
        unknown_report
            .issues
            .iter()
            .any(|issue| issue.contains("unknown data family: private_api"))
    );

    let sqlite_report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &sqlite_export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: vec!["sqlite".to_string()],
            filters: Default::default(),
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(!sqlite_report.pass);
    assert!(!sqlite_report.input_valid);
    assert!(sqlite_report.data_families_valid);
    assert!(!sqlite_report.sqlite_policy_valid);
    assert!(!sqlite_report.export_ready);
    assert!(
        sqlite_report
            .issues
            .iter()
            .any(|issue| issue.contains("sqlite data family requires sqlite_source_path"))
    );
}

#[test]
fn rejects_unreimportable_raw_rows_and_unmarked_calibration_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::create_dir(tempdir.path().join("data")).unwrap();
    let raw_rows = br#"{"evidence_id":"raw-1","source":"synthetic","captured_at":"2026-05-27T00:00:00Z","device_model":"whoop_5","payload_hex":"00ff","sha256":"wrong-checksum","sensitivity":"synthetic"}
"#;
    let label_rows = br#"{"label_id":"label-1","metric_family":"recovery","label_source":"manual","captured_at":"2026-05-27T00:00:00Z","value":88.0,"unit":"score_0_to_100","provenance_json":"{}","official_labels_are_labels":false}
"#;
    fs::write(tempdir.path().join("data/raw_evidence.jsonl"), raw_rows).unwrap();
    fs::write(
        tempdir.path().join("data/calibration_labels.jsonl"),
        label_rows,
    )
    .unwrap();
    fs::write(
        tempdir.path().join("manifest.json"),
        format!(
            r#"{{
  "schema_version": "goose.export.v1",
  "app_version": "0.1.0",
  "core_version": "0.1.0",
  "time_window": {{"start": "2026-05-27T00:00:00Z", "end": "2026-05-28T00:00:00Z"}},
  "data_families": ["raw_evidence", "calibration_labels"],
  "official_labels_are_labels": true,
  "files": [
    {{"path": "data/raw_evidence.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}},
    {{"path": "data/calibration_labels.jsonl", "sha256": "{}", "row_count": 1, "kind": "jsonl"}}
  ]
}}"#,
            sha256_hex(raw_rows),
            sha256_hex(label_rows),
        ),
    )
    .unwrap();

    let report = validate_export_bundle(tempdir.path()).unwrap();

    assert!(!report.pass);
    assert!(!report.content.pass);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("raw evidence raw-1 sha256 does not match payload_hex"))
    );
    assert!(report.issues.iter().any(|issue| {
        issue.contains("calibration label label-1 must keep official_labels_are_labels=true")
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.contains("calibration label label-1 provenance_json must be a non-empty JSON object")
    }));
}

#[test]
fn exports_and_validates_zipped_goosebundle_without_extracting() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("export.goosebundle");
    let zip_path = tempdir.path().join("export.goosebundle.zip");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: Vec::new(),
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: Some(&zip_path),
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert_eq!(report.zip_path.as_deref(), Some(zip_path.to_str().unwrap()));
    assert!(zip_path.exists());

    let validation = validate_export_bundle(&zip_path).unwrap();
    assert!(validation.pass, "{:?}", validation.issues);
    assert!(
        validation
            .files
            .iter()
            .any(|file| file.path == "data/raw_evidence.jsonl")
    );
    assert!(
        validation
            .files
            .iter()
            .any(|file| file.path == "data/goose.sqlite")
    );
}

#[test]
fn raw_export_applies_half_open_time_window() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("export.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();
    let import_report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    assert!(import_report.pass);

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-28T00:00:00Z",
            end: "2026-05-29T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: Vec::new(),
            filters: Default::default(),
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(report.pass);
    assert_eq!(report.raw_rows, 0);
    assert_eq!(report.decoded_frame_rows, 0);
    assert_eq!(report.packet_timeline_rows, 0);
    assert_eq!(report.algorithm_run_rows, 0);
    assert_eq!(report.calibration_label_rows, 0);
    assert_eq!(report.calibration_run_rows, 0);
    assert_eq!(report.debug_session_rows, 0);
    assert_eq!(report.debug_command_rows, 0);
    assert_eq!(report.debug_event_rows, 0);

    let raw_jsonl = fs::read_to_string(export_dir.join("data/raw_evidence.jsonl")).unwrap();
    assert!(raw_jsonl.is_empty());
    let debug_events = fs::read_to_string(export_dir.join("data/debug_events.jsonl")).unwrap();
    assert!(debug_events.is_empty());
}

#[test]
fn raw_export_rejects_inverted_time_window() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let export_dir = tempdir.path().join("export.goosebundle");
    let store = GooseStore::open(&db_path).unwrap();

    let report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &export_dir,
            start: "2026-05-29T00:00:00Z",
            end: "2026-05-28T00:00:00Z",
            app_version: "goose-app/test",
            core_version: "goose-core/test",
            data_families: Vec::new(),
            filters: Default::default(),
            sqlite_source_path: None,
            zip_output_path: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(!report.input_valid);
    assert!(report.data_families_valid);
    assert!(report.filters_valid);
    assert!(!report.time_window_valid);
    assert!(report.version_fields_valid);
    assert!(report.sqlite_policy_valid);
    assert!(!report.export_ready);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("start must be earlier"))
    );
}

fn seed_command_validation_record(store: &GooseStore) {
    let command_report = serde_json::json!({
        "command": "get_hello",
        "command_number": 145,
        "family": "device_identity",
        "risk_gate": "read_only",
        "description": "Read hello and identity payload.",
        "direct_send_ready": true,
        "missing_requirements": [],
        "warnings": [],
        "next_capture_actions": [],
        "validated_local_frame_hex": GET_HELLO_FRAME,
        "validated_official_frame_hex": GET_HELLO_FRAME,
        "validated_service_uuid": "61080001-8d6d-82b8-4f49-2b1b01010100",
        "validated_characteristic_uuid": "61080002-8d6d-82b8-4f49-2b1b01010100",
        "validated_write_type": "with_response",
        "validated_evidence_source": "official_app_capture",
        "validated_capture_kind": "official_app_to_macos_emulator",
        "validated_owner": "user",
        "validated_provenance_json": "{\"capture_app\":\"whoop_official\",\"capture_kind\":\"official_app_to_macos_emulator\",\"owner\":\"user\"}"
    });
    store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: "get_hello".to_string(),
            risk_gate: "read_only".to_string(),
            direct_send_ready: true,
            report_json: command_report.to_string(),
        })
        .unwrap();
}

fn read_jsonl_values(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn csv_row(fields: &[&str]) -> String {
    let mut row = fields
        .iter()
        .map(|field| csv_escape(field))
        .collect::<Vec<_>>()
        .join(",");
    row.push('\n');
    row
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn assert_no_non_empty_raw_byte_fields(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key.ends_with("_hex")
                    || key.ends_with("_bytes")
                    || key == "frame_hex"
                    || key == "payload_hex"
                    || key == "body_hex"
                    || key == "data_hex"
                {
                    assert!(
                        value.as_str().is_some_and(str::is_empty) || value.is_null(),
                        "{key} was not redacted: {value}"
                    );
                } else {
                    assert_no_non_empty_raw_byte_fields(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                assert_no_non_empty_raw_byte_fields(value);
            }
        }
        _ => {}
    }
}

fn k10_motion_frame_hex_with_timestamp(timestamp_seconds: u32) -> String {
    k10_motion_frame_hex_with_timestamp_subseconds(timestamp_seconds, 0)
}

fn k10_motion_frame_hex_with_timestamp_subseconds(
    timestamp_seconds: u32,
    timestamp_subseconds: u16,
) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    put_u32(&mut payload, 7, timestamp_seconds);
    put_u16(&mut payload, 11, timestamp_subseconds);
    for offset in [85, 285, 485, 688, 888, 1088] {
        put_i16(&mut payload, offset, -2);
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn raw_evidence_jsonl(evidence_id: &str, payload: &[u8]) -> String {
    format!(
        r#"{{"evidence_id":"{evidence_id}","source":"fixture","captured_at":"2026-05-27T00:10:00Z","device_model":"whoop-5.0","payload_hex":"{}","sha256":"{}","sensitivity":"user_owned"}}"#,
        hex::encode(payload),
        sha256_hex(payload)
    ) + "\n"
}

fn raw_evidence_csv(evidence_id: &str, payload: &[u8]) -> String {
    format!(
        "evidence_id,source,captured_at,device_model,payload_hex,sha256,sensitivity,capture_session_id\n{evidence_id},fixture,2026-05-27T00:10:00Z,whoop-5.0,{},{},user_owned,\n",
        hex::encode(payload),
        sha256_hex(payload)
    )
}
