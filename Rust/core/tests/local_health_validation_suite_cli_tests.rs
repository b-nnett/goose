use goose_core::{
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    local_health_validation::{
        local_health_validation_manifest_runbook_markdown, review_local_health_validation_manifest,
    },
    protocol::{
        DeviceType, PACKET_TYPE_HISTORICAL_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
        build_v5_payload_frame,
    },
    store::{CaptureSessionInput, GooseStore, RawEvidenceInput},
};
use rusqlite::{Connection, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::Path,
};
use zip::{CompressionMethod, ZipWriter, write::FileOptions};

#[test]
fn local_health_validation_suite_accepts_raw_export_directory_bundle() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data").join("goose.sqlite");
    let manifest_path = tempdir.path().join("bundle-validation.json");
    let markdown_output_path = tempdir.path().join("bundle-validation.md");
    let review_output_path = tempdir.path().join("bundle-validation-review.json");
    seed_goose_database(&db);
    let sqlite_sha256 = write_raw_export_manifest(&bundle_dir, &db);
    write_steps_unavailable_manifest(&manifest_path);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--markdown-output")
            .arg(&markdown_output_path)
            .arg("--review-output")
            .arg(&review_output_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let markdown = fs::read_to_string(&markdown_output_path).unwrap();
    let review: serde_json::Value =
        serde_json::from_slice(&fs::read(&review_output_path).unwrap()).unwrap();
    assert!(markdown.contains("# Local Health Validation Report"));
    assert!(markdown.contains("## Readiness"));
    assert!(markdown.contains("## Next Actions"));
    assert!(markdown.contains("## Cases"));
    assert!(markdown.contains("## Metric Records"));
    assert!(markdown.contains("raw-export-bundle-validation-smoke"));
    assert!(markdown.contains("bundle-step-unavailable-status"));
    assert!(markdown.contains("activity.steps"));
    assert!(markdown.contains("unavailable"));
    assert_eq!(
        review["schema"],
        "goose.local-health-validation-manifest-review.v1"
    );
    assert_eq!(review["manifest_id"], "raw-export-bundle-validation-smoke");
    assert_eq!(review["status"], "ready_to_run_validation_suite");
    assert_eq!(review["schema_valid"], true);
    assert_eq!(review["official_label_policy_required"], false);
    assert_eq!(review["label_policy_valid"], true);
    assert_eq!(review["official_label_case_count"], 0);
    assert_eq!(review["placeholder_field_count"].as_u64().unwrap(), 0);
    assert_eq!(review["generated_command_present"], false);
    assert_eq!(review["generated_command_writes_json"], false);
    assert_eq!(review["generated_command_writes_markdown"], false);
    assert_eq!(review["generated_command_writes_review"], false);
    assert_eq!(report["database_path"], db.display().to_string());
    assert_eq!(report["database_source"]["kind"], "raw_export_directory");
    assert_eq!(
        report["database_source"]["input_path"],
        bundle_dir.display().to_string()
    );
    assert_eq!(
        report["database_source"]["resolved_database_path"],
        db.display().to_string()
    );
    assert_eq!(
        report["database_source"]["temporary_extracted_database"],
        false
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["present"],
        true
    );
    assert_eq!(report["database_source"]["raw_export_manifest"]["ok"], true);
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["schema_version"],
        "goose.export.v1"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["official_labels_are_labels"],
        true
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["time_window_start"],
        "2026-06-02T00:00:00Z"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["time_window_end"],
        "2026-06-03T00:00:00Z"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["sqlite_kind"],
        "sqlite"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["expected_sha256"],
        sqlite_sha256
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["actual_sha256"],
        sqlite_sha256
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["sha256_match"],
        true
    );
    assert_eq!(report["database_source"]["sqlite_audit"]["ok"], true);
    assert!(
        report["database_source"]["sqlite_audit"]["storage_schema_version"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert!(
        report["database_source"]["sqlite_audit"]["table_counts"]["raw_evidence"]
            .as_i64()
            .unwrap()
            >= 0
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["raw_evidence_time_window_count"],
        0
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["decoded_frames_time_window_count"],
        0
    );
    assert_eq!(report["cases"][0]["id"], "bundle-step-unavailable-status");
    assert_eq!(report["cases"][0]["pass"], true);

    let connection = Connection::open(&db).unwrap();
    let unavailable_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM daily_activity_metrics WHERE date_key = '2026-06-02' AND source_kind = 'unavailable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unavailable_count, 1);
}

#[test]
fn local_health_validation_suite_accepts_raw_export_zip_bundle() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("source/data/goose.sqlite");
    let zip_path = tempdir.path().join("raw-export.zip");
    let manifest_path = tempdir.path().join("zip-bundle-validation.json");
    seed_goose_database(&db);
    write_steps_unavailable_manifest(&manifest_path);
    let sqlite_sha256 = zip_goose_database(
        &zip_path,
        &db,
        "Goose Raw Export 2026-06-02/data/goose.sqlite",
    );

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--bundle")
            .arg(&zip_path)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let extracted_database_path = report["database_path"].as_str().unwrap();
    assert!(extracted_database_path.contains("goose-local-health-validation"));
    assert_ne!(extracted_database_path, zip_path.display().to_string());
    assert!(!Path::new(extracted_database_path).exists());
    assert_eq!(report["database_source"]["kind"], "raw_export_zip");
    assert_eq!(
        report["database_source"]["input_path"],
        zip_path.display().to_string()
    );
    assert_eq!(
        report["database_source"]["resolved_database_path"],
        extracted_database_path
    );
    assert_eq!(
        report["database_source"]["archive_entry"],
        "Goose Raw Export 2026-06-02/data/goose.sqlite"
    );
    assert_eq!(
        report["database_source"]["temporary_extracted_database"],
        true
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["present"],
        true
    );
    assert_eq!(report["database_source"]["raw_export_manifest"]["ok"], true);
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["archive_entry"],
        "Goose Raw Export 2026-06-02/manifest.json"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["official_labels_are_labels"],
        true
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["time_window_start"],
        "2026-06-02T00:00:00Z"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["time_window_end"],
        "2026-06-03T00:00:00Z"
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["expected_sha256"],
        sqlite_sha256
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["actual_sha256"],
        sqlite_sha256
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["sha256_match"],
        true
    );
    assert_eq!(report["database_source"]["sqlite_audit"]["ok"], true);
    assert!(
        report["database_source"]["sqlite_audit"]["storage_schema_version"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["raw_evidence_time_window_count"],
        0
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["decoded_frames_time_window_count"],
        0
    );
    assert_eq!(report["cases"][0]["id"], "bundle-step-unavailable-status");
    assert_eq!(report["cases"][0]["pass"], true);
}

#[test]
fn local_health_validation_suite_scaffolds_manifest_from_raw_export_bundle() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let output_path = tempdir.path().join("generated-validation-manifest.json");
    let review_output_path = tempdir.path().join("generated-validation-review.json");
    let runbook_output_path = tempdir.path().join("generated-validation-runbook.md");
    seed_goose_database(&db);
    let store = GooseStore::open(&db).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "walk-capture-session",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true}"#,
        })
        .unwrap();
    for (evidence_id, captured_at, packet_k, domain, sequence) in [
        (
            "raw-walk-k10",
            "2026-06-02T10:00:30Z",
            10,
            "raw_motion_stream_result",
            10,
        ),
        (
            "raw-walk-k11",
            "2026-06-02T10:04:30Z",
            11,
            "raw_stream_counted",
            11,
        ),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &[packet_k as u8, sequence as u8],
                sensitivity: "public-test-fixture",
                capture_session_id: Some("walk-capture-session"),
            })
            .unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                r#"
                INSERT INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, ?3, 'DATA', ?4, NULL, ?5, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(packet_k),
                    i64::from(sequence),
                    json!({
                        "packet_k": packet_k,
                        "domain": domain,
                        "body_summary": {
                            "kind": domain
                        }
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }
    drop(store);
    write_raw_export_manifest(&bundle_dir, &db);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--scaffold-manifest")
            .arg("--manifest-id")
            .arg("walk-capture-scaffold")
            .arg("--timezone")
            .arg("Europe/London")
            .arg("--output")
            .arg(&output_path)
            .arg("--review-output")
            .arg(&review_output_path)
            .arg("--markdown-output")
            .arg(&runbook_output_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(persisted, report);
    let review: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review_output_path).unwrap()).unwrap();
    assert_eq!(
        review["schema"],
        "goose.local-health-validation-manifest-review.v1"
    );
    assert_eq!(review["manifest_id"], "walk-capture-scaffold");
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["schema_valid"], true);
    assert_eq!(review["label_policy_valid"], true);
    assert_eq!(review["official_label_required_case_count"], 3);
    assert_eq!(review["official_label_missing_case_count"], 3);
    assert_eq!(review["manual_label_required_case_count"], 2);
    assert_eq!(review["manual_label_missing_case_count"], 2);
    assert_eq!(review["acceptance_evidence_case_count"], 5);
    assert_eq!(review["acceptance_evidence_open_case_count"], 4);
    let step_validation_evidence = review["acceptance_evidence_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == "owned-step-validation")
        .unwrap();
    assert_eq!(step_validation_evidence["capture_session_status"], "bound");
    assert_eq!(
        step_validation_evidence["declared_capture_session_ids"],
        json!(["walk-capture-session"])
    );
    assert_eq!(
        step_validation_evidence["missing_manual_label_fields"],
        json!(["manual_step_delta"])
    );
    assert_eq!(
        step_validation_evidence["missing_official_label_fields"],
        json!(["official_whoop_step_delta"])
    );
    assert!(
        step_validation_evidence["collection_action"]
            .as_str()
            .unwrap()
            .contains("WHOOP app step delta as a validation label")
    );
    assert_eq!(review["placeholder_field_count"].as_u64().unwrap(), 15);
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["generated_command_writes_json"], true);
    assert_eq!(review["generated_command_writes_markdown"], true);
    assert_eq!(review["generated_command_writes_review"], true);
    assert!(
        review["placeholder_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "official_whoop_step_delta")
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "validation_official_labels_missing")
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "validation_manual_labels_missing")
    );
    let runbook = std::fs::read_to_string(&runbook_output_path).unwrap();
    assert!(runbook.contains("# Local Health Validation Runbook"));
    assert!(runbook.contains("walk-capture-scaffold"));
    assert!(runbook.contains("goose-local-health-validation-suite"));
    assert!(runbook.contains("--markdown-output local-health-validation-report.md"));
    assert!(runbook.contains("--review-output local-health-validation-review.json"));
    assert!(runbook.contains("- Manifest review: `local-health-validation-review.json`"));
    assert!(runbook.contains("## Acceptance Evidence Checklist"));
    assert!(runbook.contains("| owned-step-validation | owned_capture | step-validation | 2026-06-02T00:00:00Z to 2026-06-03T00:00:00Z | bound: walk-capture-session | K10, K11, K21 | manual labels: manual_step_delta; WHOOP labels: official_whoop_step_delta | -- | Run the controlled step capture `owned_capture`, record the manual count, then add the WHOOP app step delta as a validation label. |"));
    assert!(runbook.contains("| owned-energy-validation | owned_capture | energy-validation | 2026-06-02T00:00:00Z to 2026-06-03T00:00:00Z | bound: walk-capture-session | K2, K10, K11, K18, K21, K24 | WHOOP labels: official_whoop_active_kcal, official_whoop_resting_kcal, official_whoop_total_kcal; placeholders: max_hr_bpm, profile_age_years, profile_sex, profile_weight_kg, resting_hr_bpm | -- | Run the rest/walk/workout energy capture `owned_capture`, keep HR and motion evidence, then add WHOOP app calorie labels for comparison only. |"));
    assert!(runbook.contains("## Validation Labels"));
    assert!(runbook.contains("| owned-step-validation | step-validation | step-validation | official_whoop_app | official_whoop_step_delta | Add WHOOP app validation label fields |"));
    assert!(runbook.contains("| owned-step-validation | step-validation | step-validation | manual_count | manual_step_delta | Add manually counted validation label fields |"));
    assert!(runbook.contains("## Capture Session Binding"));
    assert!(runbook.contains("No capture-session binding gaps were detected."));
    assert!(runbook.contains("walk-capture-session"));
    assert!(runbook.contains("K10/raw_motion_stream_result"));
    assert!(runbook.contains("official_whoop_step_delta"));
    assert!(runbook.contains("validation labels only"));
    assert_eq!(
        report["schema"],
        "goose.local-health-validation-manifest.v1"
    );
    assert_eq!(report["manifest_id"], "walk-capture-scaffold");
    assert_eq!(report["start"], "2026-06-02T00:00:00Z");
    assert_eq!(report["end"], "2026-06-03T00:00:00Z");
    assert_eq!(report["date_key"], "2026-06-02");
    assert_eq!(report["timezone"], "Europe/London");
    assert_eq!(report["capture_session_id"], "walk-capture-session");
    assert_eq!(
        report["label_provenance"]["official_labels_are_labels"],
        true
    );
    assert_eq!(
        report["generated_evidence"]["database_source_kind"],
        "raw_export_directory"
    );
    assert_eq!(
        report["generated_evidence"]["raw_export_bundle_path"],
        bundle_dir.display().to_string()
    );
    assert_eq!(
        report["generated_evidence"]["window_source"],
        "raw_export_manifest"
    );
    assert_eq!(
        report["generated_evidence"]["capture_session_default"],
        "single_session_defaulted"
    );
    assert_eq!(
        report["generated_evidence"]["observed_capture_session_ids"][0],
        "walk-capture-session"
    );
    assert_eq!(
        report["generated_evidence"]["packet_family_counts"]["K10/raw_motion_stream_result"],
        1
    );
    assert_eq!(
        report["generated_evidence"]["packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert_eq!(report["run_validation"]["args"][1], "--raw-export-bundle");
    assert_eq!(
        report["run_validation"]["args"][2],
        bundle_dir.display().to_string()
    );
    assert!(
        report["run_validation"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--markdown-output")
    );
    assert!(
        report["run_validation"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--review-output")
    );
    assert_eq!(
        report["run_validation"]["json_report_path"],
        "local-health-validation-report.json"
    );
    assert_eq!(
        report["run_validation"]["markdown_report_path"],
        "local-health-validation-report.md"
    );
    assert_eq!(
        report["run_validation"]["review_report_path"],
        "local-health-validation-review.json"
    );
    assert!(
        report["run_validation"]["command"]
            .as_str()
            .unwrap()
            .contains("--markdown-output local-health-validation-report.md")
    );
    assert!(
        report["run_validation"]["command"]
            .as_str()
            .unwrap()
            .contains("--review-output local-health-validation-review.json")
    );
    assert_eq!(
        report["run_validation"]["official_whoop_values_are_validation_labels_not_inputs"],
        true
    );
    let checklist = report["operator_checklist"].as_array().unwrap();
    assert!(checklist.iter().any(|item| {
        item["id"] == "bind_capture_sessions"
            && item["status"] == "single_capture_session_defaulted"
    }));
    assert!(checklist.iter().any(|item| {
        item["id"] == "fill_validation_placeholders"
            && item["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "official_whoop_step_delta")
    }));
    assert!(checklist.iter().any(|item| {
        item["id"] == "run_validation_suite"
            && item["action"]
                .as_str()
                .unwrap()
                .contains("manifest review JSON")
    }));

    let case_ids = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    for expected in [
        "owned-step-discovery",
        "owned-step-validation",
        "owned-raw-motion-steps",
        "owned-energy-rollup",
        "owned-energy-validation",
    ] {
        assert!(
            case_ids.contains(expected),
            "scaffold missing case {expected}"
        );
    }
    let step_validation = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "owned-step-validation")
        .unwrap();
    assert!(step_validation["manual_step_delta"].is_null());
    assert!(step_validation["official_whoop_step_delta"].is_null());
}

#[test]
fn local_health_validation_scaffold_leaves_multi_session_cases_unbound() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    seed_goose_database(&db);
    let store = GooseStore::open(&db).unwrap();
    for (session_id, started_at_unix_ms) in [
        ("still-capture-session", 1_780_392_000_000),
        ("walk-capture-session", 1_780_395_600_000),
    ] {
        store
            .start_capture_session(CaptureSessionInput {
                session_id,
                source: "synthetic.validation",
                started_at_unix_ms,
                device_model: "WHOOP 5.0 Goose",
                active_device_id: None,
                provenance_json: r#"{"owned_capture":true}"#,
            })
            .unwrap();
    }
    for (evidence_id, captured_at, capture_session_id, packet_k, domain, sequence) in [
        (
            "raw-still-k10",
            "2026-06-02T10:00:30Z",
            "still-capture-session",
            10,
            "raw_motion_stream_result",
            10,
        ),
        (
            "raw-walk-k11",
            "2026-06-02T11:00:30Z",
            "walk-capture-session",
            11,
            "raw_stream_counted",
            11,
        ),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &[packet_k as u8, sequence as u8],
                sensitivity: "public-test-fixture",
                capture_session_id: Some(capture_session_id),
            })
            .unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                r#"
                INSERT INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, ?3, 'DATA', ?4, NULL, ?5, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(packet_k),
                    i64::from(sequence),
                    json!({
                        "packet_k": packet_k,
                        "domain": domain,
                        "body_summary": {
                            "kind": domain
                        }
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }
    drop(store);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--scaffold-manifest")
            .arg("--manifest-id")
            .arg("multi-session-capture-scaffold")
            .arg("--timezone")
            .arg("Europe/London")
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["manifest_id"], "multi-session-capture-scaffold");
    assert!(report["capture_session_id"].is_null());
    assert!(report["capture_session_ids"].is_null());
    assert_eq!(
        report["generated_evidence"]["capture_session_default"],
        "multiple_sessions_observed_case_binding_required"
    );
    assert!(
        report["operator_checklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "bind_capture_sessions"
                && item["status"] == "case_binding_required")
    );
    assert_eq!(
        report["generated_evidence"]["observed_capture_session_ids"],
        json!(["still-capture-session", "walk-capture-session"])
    );
    let summaries = report["generated_evidence"]["capture_session_summaries"]
        .as_array()
        .unwrap();
    assert_eq!(summaries.len(), 2);
    let still_summary = summaries
        .iter()
        .find(|summary| summary["session_id"] == "still-capture-session")
        .unwrap();
    assert_eq!(
        still_summary["packet_family_counts"]["K10/raw_motion_stream_result"],
        1
    );
    let walk_summary = summaries
        .iter()
        .find(|summary| summary["session_id"] == "walk-capture-session")
        .unwrap();
    assert_eq!(
        walk_summary["packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    let review = review_local_health_validation_manifest(&report);
    assert_eq!(review["capture_session_binding_required_case_count"], 4);
    let step_validation_binding = review["capture_session_binding_required_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == "owned-step-validation")
        .unwrap();
    assert_eq!(
        step_validation_binding["required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert_eq!(
        step_validation_binding["suggested_capture_session_ids"],
        json!(["still-capture-session", "walk-capture-session"])
    );
    assert_eq!(
        step_validation_binding["suggested_capture_sessions"][0]["session_id"],
        "still-capture-session"
    );
    assert_eq!(
        step_validation_binding["suggested_capture_sessions"][0]["matching_packet_family_counts"]["K10/raw_motion_stream_result"],
        1
    );
    assert_eq!(
        step_validation_binding["suggested_capture_sessions"][0]["case_window_overlap_status"],
        "overlaps_case_window"
    );
    assert_eq!(
        step_validation_binding["suggested_capture_sessions"][1]["session_id"],
        "walk-capture-session"
    );
    assert_eq!(
        step_validation_binding["suggested_capture_sessions"][1]["matching_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    let runbook = local_health_validation_manifest_runbook_markdown(&report);
    assert!(runbook.contains("## Capture Session Binding"));
    assert!(runbook.contains("owned-step-validation"));
    assert!(runbook.contains("still-capture-session (overlaps_case_window"));
    assert!(runbook.contains("K10/raw_motion_stream_result=1"));
    assert!(runbook.contains("walk-capture-session (overlaps_case_window"));
    assert!(runbook.contains("K11/raw_stream_counted=1"));

    let mut narrowed_manifest = report.clone();
    let cases = narrowed_manifest["cases"].as_array_mut().unwrap();
    let step_validation_case = cases
        .iter_mut()
        .find(|case| case["id"] == "owned-step-validation")
        .unwrap();
    step_validation_case["start"] = json!("2026-06-02T11:00:00Z");
    step_validation_case["end"] = json!("2026-06-02T11:01:00Z");
    let narrowed_review = review_local_health_validation_manifest(&narrowed_manifest);
    let narrowed_step_binding = narrowed_review["capture_session_binding_required_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == "owned-step-validation")
        .unwrap();
    assert_eq!(
        narrowed_step_binding["suggested_capture_session_ids"],
        json!(["walk-capture-session"])
    );
    assert_eq!(
        narrowed_step_binding["suggested_capture_sessions"][0]["case_window_overlap_status"],
        "overlaps_case_window"
    );
}

#[test]
fn local_health_validation_manifest_review_flags_missing_required_labels() {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "hand-authored-missing-labels",
        "label_provenance": {
            "source": "official_app_manual_entry",
            "official_labels_are_labels": true
        },
        "capture_session_id": "owned-capture-session",
        "cases": [
            {
                "id": "missing-step-labels",
                "report": "step-validation",
                "date_key": "2026-06-02",
                "timezone": "Europe/London",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z"
            },
            {
                "id": "missing-energy-label",
                "report": "energy-validation",
                "date_key": "2026-06-02",
                "timezone": "Europe/London",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z",
                "profile_weight_kg": 82.0,
                "profile_age_years": 37.0,
                "profile_sex": "male",
                "resting_hr_bpm": 55.0,
                "max_hr_bpm": 185.0
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["label_policy_valid"], true);
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["official_label_required_case_count"], 2);
    assert_eq!(review["official_label_missing_case_count"], 2);
    assert_eq!(review["manual_label_required_case_count"], 1);
    assert_eq!(review["manual_label_missing_case_count"], 1);
    assert_eq!(
        review["official_label_missing_cases"][0]["case_id"],
        "missing-step-labels"
    );
    assert_eq!(
        review["official_label_missing_cases"][0]["required_label_fields"],
        json!(["official_whoop_step_delta"])
    );
    assert_eq!(
        review["official_label_missing_cases"][1]["required_label_fields"],
        json!([
            "official_whoop_active_kcal",
            "official_whoop_resting_kcal",
            "official_whoop_total_kcal"
        ])
    );
    assert_eq!(
        review["manual_label_missing_cases"][0]["required_label_fields"],
        json!(["manual_step_delta"])
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "validation_official_labels_missing")
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "validation_manual_labels_missing")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Validation Labels"));
    assert!(runbook.contains("| missing-step-labels | step-validation | step-validation | official_whoop_app | official_whoop_step_delta | Add WHOOP app validation label fields |"));
    assert!(runbook.contains("| missing-step-labels | step-validation | step-validation | manual_count | manual_step_delta | Add manually counted validation label fields |"));
    assert!(runbook.contains(
        "official_whoop_active_kcal, official_whoop_resting_kcal, official_whoop_total_kcal"
    ));
}

#[test]
fn local_health_validation_manifest_review_flags_invalid_capture_sqlite_imports() {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "invalid-capture-sqlite-imports",
        "capture_sqlite_imports": [
            {
                "id": "missing-path",
                "session_id": "capture-sqlite-session"
            },
            {
                "capture_sqlite_path": "capture.sqlite",
                "session_id": ""
            },
            42
        ],
        "cases": [
            {
                "id": "step-discovery",
                "report": "step-discovery",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z",
                "capture_session_id": "capture-sqlite-session"
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["capture_sqlite_import_count"], 3);
    assert_eq!(review["capture_sqlite_import_invalid_count"], 3);
    assert_eq!(
        review["capture_sqlite_import_session_ids"],
        json!(["capture-sqlite-session"])
    );
    assert_eq!(review["capture_sqlite_imports"][0]["id"], "missing-path");
    assert_eq!(
        review["capture_sqlite_imports"][0]["issues"],
        json!(["capture_sqlite_import_path_required"])
    );
    assert_eq!(
        review["capture_sqlite_imports"][1]["path"],
        "capture.sqlite"
    );
    assert!(
        review["capture_sqlite_imports"][1]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "capture_sqlite_import_id_required")
    );
    assert!(
        review["capture_sqlite_imports"][1]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "capture_sqlite_import_session_id_required")
    );
    assert!(
        review["capture_sqlite_imports"][2]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "capture_sqlite_import_object_required")
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_sqlite_import_declaration_invalid")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Capture SQLite Imports"));
    assert!(runbook.contains("| 0 | missing-path | -- | capture-sqlite-session | invalid | capture_sqlite_import_path_required |"));
    assert!(runbook.contains("capture_sqlite_import_object_required"));
}

#[test]
fn local_health_validation_manifest_review_flags_unresolved_capture_session_ids() {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "unresolved-capture-session",
        "capture_sqlite_imports": [
            {
                "id": "walk-hci",
                "path": "capture.sqlite",
                "session_id": "imported-walk-session"
            }
        ],
        "cases": [
            {
                "id": "typo-step-discovery",
                "report": "step-discovery",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z",
                "capture_session_id": "imported-walk-sesion"
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["capture_sqlite_import_invalid_count"], 0);
    assert_eq!(
        review["known_capture_session_ids"],
        json!(["imported-walk-session"])
    );
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["capture_session_unresolved_case_count"], 1);
    let unresolved = &review["capture_session_unresolved_cases"][0];
    assert_eq!(unresolved["case_id"], "typo-step-discovery");
    assert_eq!(
        unresolved["declared_capture_session_ids"],
        json!(["imported-walk-sesion"])
    );
    assert_eq!(
        unresolved["missing_capture_session_ids"],
        json!(["imported-walk-sesion"])
    );
    assert_eq!(
        unresolved["known_capture_session_ids"],
        json!(["imported-walk-session"])
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_declared_ids_unresolved")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Capture Session Resolution"));
    assert!(runbook.contains("| typo-step-discovery | step-discovery | step-discovery | case | imported-walk-sesion | imported-walk-sesion | imported-walk-session | Use an observed/imported `capture_session_id` |"));
}

#[test]
fn local_health_validation_manifest_review_marks_declared_sessions_unverified_without_known_sessions()
 {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "direct-db-session-unverified",
        "cases": [
            {
                "id": "direct-db-step-discovery",
                "report": "step-discovery",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z",
                "capture_session_id": "direct-db-owned-session"
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "ready_to_run_validation_suite");
    assert_eq!(
        review["known_capture_session_ids"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["capture_session_unresolved_case_count"], 0);
    assert_eq!(review["capture_session_unverified_case_count"], 1);
    let unverified = &review["capture_session_unverified_cases"][0];
    assert_eq!(unverified["case_id"], "direct-db-step-discovery");
    assert_eq!(
        unverified["declared_capture_session_ids"],
        json!(["direct-db-owned-session"])
    );
    assert_eq!(
        unverified["resolution_status"],
        "unverified_no_known_sessions"
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|blocker| blocker != "capture_session_declared_ids_unresolved")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Capture Session Resolution"));
    assert!(runbook.contains("| direct-db-step-discovery | step-discovery | step-discovery | case | direct-db-owned-session | unverified_no_known_sessions | manifest_has_no_generated_evidence_or_valid_capture_sqlite_imports |"));
}

#[test]
fn local_health_validation_manifest_review_flags_case_windows_outside_generated_evidence() {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "case-window-outside-evidence",
        "generated_evidence": {
            "capture_session_default": "single_session_defaulted",
            "observed_capture_session_ids": ["owned-walk-session"],
            "raw_evidence_time_bounds": {
                "first_captured_at": "2026-06-02T10:00:00Z",
                "last_captured_at": "2026-06-02T10:05:00Z",
                "span_ms": 300000
            },
            "decoded_frame_time_bounds": {
                "first_captured_at": "2026-06-02T10:00:30Z",
                "last_captured_at": "2026-06-02T10:04:30Z",
                "span_ms": 240000
            }
        },
        "cases": [
            {
                "id": "outside-window-step-discovery",
                "report": "step-discovery",
                "start": "2026-06-03T10:00:00Z",
                "end": "2026-06-03T10:05:00Z",
                "capture_session_id": "owned-walk-session"
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["capture_session_unresolved_case_count"], 0);
    assert_eq!(review["case_window_evidence_gap_case_count"], 1);
    let gap = &review["case_window_evidence_gap_cases"][0];
    assert_eq!(gap["case_id"], "outside-window-step-discovery");
    assert_eq!(
        gap["evidence_overlap_status"],
        "outside_generated_evidence_window"
    );
    assert_eq!(
        gap["evidence_bounds"][0]["bounds_source"],
        "decoded_frame_time_bounds"
    );
    assert_eq!(
        gap["evidence_bounds"][0]["overlap_status"],
        "outside_case_window"
    );
    assert_eq!(
        gap["evidence_bounds"][1]["bounds_source"],
        "raw_evidence_time_bounds"
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "case_window_outside_generated_evidence")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Case Window Evidence"));
    assert!(runbook.contains("| outside-window-step-discovery | step-discovery | step-discovery | 2026-06-03T10:00:00Z to 2026-06-03T10:05:00Z | outside_generated_evidence_window | decoded_frame_time_bounds: outside_case_window"));
}

#[test]
fn local_health_validation_manifest_review_flags_bound_session_with_unrelated_packet_families() {
    let manifest = json!({
        "schema": "goose.local-health-validation-manifest.v1",
        "manifest_id": "wrong-family-capture-session",
        "generated_evidence": {
            "capture_session_default": "multiple_sessions_observed_case_binding_required",
            "observed_capture_session_ids": ["history-session"],
            "raw_evidence_time_bounds": {
                "first_captured_at": "2026-06-02T10:00:00Z",
                "last_captured_at": "2026-06-02T10:05:00Z",
                "span_ms": 300000
            },
            "decoded_frame_time_bounds": {
                "first_captured_at": "2026-06-02T10:00:30Z",
                "last_captured_at": "2026-06-02T10:04:30Z",
                "span_ms": 240000
            },
            "capture_session_summaries": [
                {
                    "session_id": "history-session",
                    "decoded_frame_time_bounds": {
                        "first_captured_at": "2026-06-02T10:00:30Z",
                        "last_captured_at": "2026-06-02T10:04:30Z",
                        "span_ms": 240000
                    },
                    "packet_family_counts": {
                        "K18/normal_history": 2
                    }
                }
            ]
        },
        "cases": [
            {
                "id": "wrong-family-step-discovery",
                "report": "step-discovery",
                "start": "2026-06-02T10:00:00Z",
                "end": "2026-06-02T10:05:00Z",
                "capture_session_id": "history-session"
            }
        ]
    });

    let review = review_local_health_validation_manifest(&manifest);
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["capture_session_binding_required_case_count"], 0);
    assert_eq!(review["capture_session_unresolved_case_count"], 0);
    assert_eq!(review["capture_session_unverified_case_count"], 0);
    assert_eq!(review["case_window_evidence_gap_case_count"], 0);
    assert_eq!(review["capture_session_packet_family_gap_case_count"], 1);
    let gap = &review["capture_session_packet_family_gap_cases"][0];
    assert_eq!(gap["case_id"], "wrong-family-step-discovery");
    assert_eq!(
        gap["required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert_eq!(
        gap["declared_session_packet_family_counts"][0]["packet_family_counts"]["K18/normal_history"],
        2
    );
    assert_eq!(
        gap["declared_session_packet_family_counts"][0]["matching_packet_family_count"],
        0
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_packet_family_unrelated")
    );

    let runbook = local_health_validation_manifest_runbook_markdown(&manifest);
    assert!(runbook.contains("## Capture Session Packet Families"));
    assert!(runbook.contains("| wrong-family-step-discovery | step-discovery | step-discovery | history-session | K10, K11, K21 | history-session: K18/normal_history=2 | Bind to a session with the required packet families |"));
}

#[test]
fn local_health_validation_suite_flags_raw_export_capture_case_without_packet_window_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("step-discovery-validation.json");
    seed_goose_database(&db);
    write_raw_export_manifest(&bundle_dir, &db);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "raw-export-empty-step-discovery",
            "cases": [
                {
                    "id": "empty-bundle-step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["database_source"]["sqlite_audit"]["raw_evidence_time_window_count"],
        0
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["decoded_frames_time_window_count"],
        0
    );
    let summary = &report["database_source"]["case_packet_evidence_summary"];
    assert_eq!(summary["case_count"], 1);
    assert_eq!(summary["decoded_packet_evidence_case_count"], 0);
    assert_eq!(summary["raw_only_packet_evidence_case_count"], 0);
    assert_eq!(summary["no_packet_evidence_case_count"], 1);
    assert_eq!(summary["case_window_raw_evidence_count_sum"], 0);
    assert_eq!(summary["case_window_decoded_frame_count_sum"], 0);
    assert!(summary["packet_family_counts"].is_null());
    let case_packet_evidence = &report["database_source"]["case_packet_evidence"][0];
    assert_eq!(
        case_packet_evidence["case_id"],
        "empty-bundle-step-discovery"
    );
    assert_eq!(case_packet_evidence["report"], "step-discovery");
    assert_eq!(case_packet_evidence["status"], "no_packet_evidence");
    assert_eq!(case_packet_evidence["raw_evidence_count"], 0);
    assert_eq!(case_packet_evidence["decoded_frame_count"], 0);
    assert!(
        case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "case_window_no_packet_evidence:empty-bundle-step-discovery")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "database_source:no_packet_evidence_in_raw_export_time_window")
    );
    let action = report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["case_id"] == "database_source"
                && action["scope"] == "database_source"
                && action["reason"] == "no_packet_evidence_in_raw_export_time_window"
        })
        .expect("missing database_source packet-evidence next action");
    assert_eq!(
        action["action"],
        "Regenerate the Raw Export bundle from the owned capture window so raw_evidence/decoded_frames contain packet evidence, or adjust validation cases to a bundle with packet data."
    );
}

#[test]
fn local_health_validation_suite_flags_raw_export_capture_case_without_case_window_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("step-discovery-validation.json");
    seed_goose_database(&db);
    let store = GooseStore::open(&db).unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "raw-outside-case-window",
            source: "synthetic.validation",
            captured_at: "2026-06-02T08:00:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &[0x10, 0x01],
            sensitivity: "public-test-fixture",
            capture_session_id: None,
        })
        .unwrap();
    drop(store);
    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            r#"
            INSERT INTO decoded_frames (
                frame_id,
                evidence_id,
                device_type,
                raw_len,
                header_len,
                declared_len,
                payload_hex,
                payload_crc_hex,
                header_crc_valid,
                payload_crc_valid,
                packet_type,
                packet_type_name,
                sequence,
                command_or_event,
                parsed_payload_json,
                parser_version,
                warnings_json
            ) VALUES (
                'frame-outside-case-window',
                'raw-outside-case-window',
                'Goose',
                2,
                0,
                2,
                '0000',
                '',
                1,
                1,
                11,
                'DATA',
                4100,
                NULL,
                ?1,
                'test',
                '[]'
            )
            "#,
            [json!({
                "kind": "data_packet",
                "packet_k": 11,
                "domain": "raw_stream_counted",
                "body_summary": {
                    "kind": "raw_stream_counted",
                    "step_count": 4100
                },
                "warnings": []
            })
            .to_string()],
        )
        .unwrap();
    drop(connection);
    write_raw_export_manifest(&bundle_dir, &db);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "raw-export-wrong-case-window",
            "cases": [
                {
                    "id": "empty-walk-window-step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["database_source"]["sqlite_audit"]["raw_evidence_time_window_count"],
        1
    );
    assert_eq!(
        report["database_source"]["sqlite_audit"]["decoded_frames_time_window_count"],
        1
    );
    let case_packet_evidence = &report["database_source"]["case_packet_evidence"][0];
    assert_eq!(
        case_packet_evidence["case_id"],
        "empty-walk-window-step-discovery"
    );
    assert_eq!(case_packet_evidence["report"], "step-discovery");
    assert_eq!(case_packet_evidence["status"], "no_packet_evidence");
    assert_eq!(case_packet_evidence["raw_evidence_count"], 0);
    assert_eq!(case_packet_evidence["decoded_frame_count"], 0);
    assert!(
        case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_no_packet_evidence:empty-walk-window-step-discovery")
    );
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue == "database_source:case_window_no_packet_evidence:empty-walk-window-step-discovery"
    }));
    let action = report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["case_id"] == "database_source"
                && action["scope"] == "database_source"
                && action["reason"]
                    == "case_window_no_packet_evidence:empty-walk-window-step-discovery"
        })
        .expect("missing database_source case-window packet-evidence next action");
    assert_eq!(
        action["action"],
        "Adjust this validation case start/end to the owned capture window, or regenerate the Raw Export bundle so the case window contains raw and decoded packet evidence."
    );
}

#[test]
fn local_health_validation_suite_reports_raw_export_case_capture_session_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("step-discovery-validation.json");
    seed_goose_database(&db);
    let store = GooseStore::open(&db).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "actual-session",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true}"#,
        })
        .unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "raw-wrong-session-window",
            source: "synthetic.validation",
            captured_at: "2026-06-02T10:01:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &[0x10, 0x01],
            sensitivity: "public-test-fixture",
            capture_session_id: Some("actual-session"),
        })
        .unwrap();
    drop(store);
    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            r#"
            INSERT INTO decoded_frames (
                frame_id,
                evidence_id,
                device_type,
                raw_len,
                header_len,
                declared_len,
                payload_hex,
                payload_crc_hex,
                header_crc_valid,
                payload_crc_valid,
                packet_type,
                packet_type_name,
                sequence,
                command_or_event,
                parsed_payload_json,
                parser_version,
                warnings_json
            ) VALUES (
                'frame-wrong-session-window',
                'raw-wrong-session-window',
                'Goose',
                2,
                0,
                2,
                '0000',
                '',
                1,
                1,
                11,
                'DATA',
                4101,
                NULL,
                ?1,
                'test',
                '[]'
            )
            "#,
            [json!({
                "kind": "data_packet",
                "packet_k": 11,
                "domain": "raw_stream_counted",
                "body_summary": {
                    "kind": "raw_stream_counted",
                    "step_count": 4101
                },
                "warnings": []
            })
            .to_string()],
        )
        .unwrap();
    drop(connection);
    write_raw_export_manifest(&bundle_dir, &db);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "raw-export-session-mismatch",
            "cases": [
                {
                    "id": "wrong-session-step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_session_id": "declared-session",
                    "max_candidate_fields": 20
                },
                {
                    "id": "actual-session-step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_session_id": "actual-session",
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let summary = &report["database_source"]["case_packet_evidence_summary"];
    assert_eq!(summary["case_count"], 2);
    assert_eq!(summary["decoded_packet_evidence_case_count"], 2);
    assert_eq!(summary["raw_only_packet_evidence_case_count"], 0);
    assert_eq!(summary["no_packet_evidence_case_count"], 0);
    assert_eq!(summary["declared_capture_session_case_count"], 2);
    assert_eq!(
        summary["declared_capture_session_with_evidence_case_count"],
        1
    );
    assert_eq!(
        summary["declared_capture_session_missing_evidence_case_count"],
        1
    );
    assert_eq!(
        summary["declared_capture_session_partial_evidence_case_count"],
        0
    );
    assert_eq!(summary["case_window_raw_evidence_count_sum"], 2);
    assert_eq!(summary["case_window_decoded_frame_count_sum"], 2);
    assert_eq!(summary["capture_session_raw_evidence_count_sum"], 1);
    assert_eq!(summary["capture_session_decoded_frame_count_sum"], 1);
    assert_eq!(summary["decoded_evidence_zero_span_case_count"], 2);
    assert_eq!(summary["decoded_evidence_zero_coverage_case_count"], 2);
    assert_eq!(
        summary["decoded_evidence_too_sparse_for_capture_acceptance_case_count"],
        2
    );
    assert_eq!(
        summary["capture_session_decoded_evidence_too_sparse_for_capture_acceptance_case_count"],
        1
    );
    assert_eq!(
        summary["packet_family_unrelated_for_capture_acceptance_case_count"],
        0
    );
    assert_eq!(
        summary["capture_session_packet_family_unrelated_for_capture_acceptance_case_count"],
        0
    );
    assert_eq!(summary["packet_family_counts"]["K11/raw_stream_counted"], 2);
    assert_eq!(
        summary["relevant_packet_family_counts"]["K11/raw_stream_counted"],
        2
    );
    assert_eq!(
        summary["capture_session_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert_eq!(
        summary["capture_session_relevant_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    let case_packet_evidence = &report["database_source"]["case_packet_evidence"][0];
    assert_eq!(
        case_packet_evidence["case_id"],
        "wrong-session-step-discovery"
    );
    assert_eq!(case_packet_evidence["status"], "decoded_packet_evidence");
    assert_eq!(case_packet_evidence["case_window_duration_ms"], 300_000);
    assert_eq!(case_packet_evidence["raw_evidence_count"], 1);
    assert_eq!(case_packet_evidence["decoded_frame_count"], 1);
    assert_eq!(
        case_packet_evidence["packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert_eq!(
        case_packet_evidence["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert_eq!(
        case_packet_evidence["relevant_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert!(
        case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_decoded_evidence_too_sparse_for_capture_acceptance:wrong-session-step-discovery")
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["first_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["last_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["span_ms"],
        0
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["coverage_ratio"],
        0.0
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["first_offset_from_case_start_ms"],
        60_000
    );
    assert_eq!(
        case_packet_evidence["raw_evidence_time_bounds"]["last_offset_before_case_end_ms"],
        240_000
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["first_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["last_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["span_ms"],
        0
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["coverage_ratio"],
        0.0
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["first_offset_from_case_start_ms"],
        60_000
    );
    assert_eq!(
        case_packet_evidence["decoded_frame_time_bounds"]["last_offset_before_case_end_ms"],
        240_000
    );
    assert_eq!(
        case_packet_evidence["capture_session_status"],
        "declared_missing_evidence"
    );
    assert_eq!(
        case_packet_evidence["expected_capture_session_ids"],
        json!(["declared-session"])
    );
    assert_eq!(
        case_packet_evidence["observed_capture_session_ids"],
        json!(["actual-session"])
    );
    assert_eq!(
        case_packet_evidence["missing_capture_session_ids"],
        json!(["declared-session"])
    );
    assert_eq!(
        case_packet_evidence["capture_session_raw_evidence_count"],
        0
    );
    assert_eq!(
        case_packet_evidence["capture_session_decoded_frame_count"],
        0
    );
    assert!(case_packet_evidence["capture_session_raw_evidence_time_bounds"].is_null());
    assert!(case_packet_evidence["capture_session_decoded_frame_time_bounds"].is_null());
    assert!(case_packet_evidence["capture_session_packet_family_counts"].is_null());
    assert!(case_packet_evidence["capture_session_relevant_packet_family_counts"].is_null());
    let actual_session_case_packet_evidence = &report["database_source"]["case_packet_evidence"][1];
    assert_eq!(
        actual_session_case_packet_evidence["case_id"],
        "actual-session-step-discovery"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_status"],
        "declared_with_evidence"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_count"],
        1
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_count"],
        1
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["first_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["last_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["span_ms"],
        0
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["coverage_ratio"],
        0.0
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["first_offset_from_case_start_ms"],
        60_000
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_raw_evidence_time_bounds"]["last_offset_before_case_end_ms"],
        240_000
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["first_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["last_captured_at"],
        "2026-06-02T10:01:00Z"
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["span_ms"],
        0
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["coverage_ratio"],
        0.0
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["first_offset_from_case_start_ms"],
        60_000
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_decoded_frame_time_bounds"]["last_offset_before_case_end_ms"],
        240_000
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert_eq!(
        actual_session_case_packet_evidence["relevant_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert_eq!(
        actual_session_case_packet_evidence["capture_session_relevant_packet_family_counts"]["K11/raw_stream_counted"],
        1
    );
    assert!(
        actual_session_case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_decoded_evidence_too_sparse_for_capture_acceptance:actual-session-step-discovery")
    );
    assert!(
        actual_session_case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_capture_session_decoded_evidence_too_sparse_for_capture_acceptance:actual-session-step-discovery")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "wrong-session-step-discovery:capture_session_evidence_missing")
    );
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue
            == "database_source:case_window_decoded_evidence_too_sparse_for_capture_acceptance:actual-session-step-discovery"
    }));
    assert!(report["next_actions"].as_array().unwrap().iter().any(|action| {
        action["case_id"] == "database_source"
            && action["reason"]
                == "case_window_capture_session_decoded_evidence_too_sparse_for_capture_acceptance:actual-session-step-discovery"
    }));
}

#[test]
fn local_health_validation_suite_flags_raw_export_case_with_unrelated_packet_families() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("wrong-family-validation.json");
    seed_goose_database(&db);
    let store = GooseStore::open(&db).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "history-only-session",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true,"note":"history packets in step case"}"#,
        })
        .unwrap();
    for (evidence_id, captured_at, sequence) in [
        ("raw-history-only-1", "2026-06-02T10:01:00Z", 18),
        ("raw-history-only-2", "2026-06-02T10:03:00Z", 19),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &[0x18, sequence],
                sensitivity: "public-test-fixture",
                capture_session_id: Some("history-only-session"),
            })
            .unwrap();
    }
    drop(store);
    let connection = Connection::open(&db).unwrap();
    for (evidence_id, sequence) in [("raw-history-only-1", 18), ("raw-history-only-2", 19)] {
        connection
            .execute(
                r#"
                INSERT INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, 18, 'DATA', ?3, NULL, ?4, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(sequence),
                    json!({
                        "packet_k": 18,
                        "domain": "normal_history",
                        "body_summary": {
                            "kind": "normal_history",
                            "heart_rate_bpm": 72
                        }
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }
    drop(connection);
    write_raw_export_manifest(&bundle_dir, &db);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "raw-export-wrong-family",
            "cases": [
                {
                    "id": "history-only-step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_session_id": "history-only-session",
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let summary = &report["database_source"]["case_packet_evidence_summary"];
    assert_eq!(summary["case_count"], 1);
    assert_eq!(summary["decoded_packet_evidence_case_count"], 1);
    assert_eq!(
        summary["decoded_evidence_too_sparse_for_capture_acceptance_case_count"],
        0
    );
    assert_eq!(
        summary["capture_session_decoded_evidence_too_sparse_for_capture_acceptance_case_count"],
        0
    );
    assert_eq!(
        summary["packet_family_unrelated_for_capture_acceptance_case_count"],
        1
    );
    assert_eq!(
        summary["capture_session_packet_family_unrelated_for_capture_acceptance_case_count"],
        1
    );
    assert_eq!(summary["packet_family_counts"]["K18/normal_history"], 2);
    assert!(summary["relevant_packet_family_counts"].is_null());
    assert_eq!(
        summary["capture_session_packet_family_counts"]["K18/normal_history"],
        2
    );
    assert!(summary["capture_session_relevant_packet_family_counts"].is_null());

    let case_packet_evidence = &report["database_source"]["case_packet_evidence"][0];
    assert_eq!(
        case_packet_evidence["case_id"],
        "history-only-step-discovery"
    );
    assert_eq!(
        case_packet_evidence["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert_eq!(
        case_packet_evidence["packet_family_counts"]["K18/normal_history"],
        2
    );
    assert!(case_packet_evidence["relevant_packet_family_counts"].is_null());
    assert_eq!(
        case_packet_evidence["capture_session_packet_family_counts"]["K18/normal_history"],
        2
    );
    assert!(case_packet_evidence["capture_session_relevant_packet_family_counts"].is_null());
    assert!(
        case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_packet_family_unrelated_for_capture_acceptance:history-only-step-discovery")
    );
    assert!(
        case_packet_evidence["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "case_window_capture_session_packet_family_unrelated_for_capture_acceptance:history-only-step-discovery")
    );
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue
            == "database_source:case_window_capture_session_packet_family_unrelated_for_capture_acceptance:history-only-step-discovery"
    }));
    assert!(report["next_actions"].as_array().unwrap().iter().any(|action| {
        action["case_id"] == "database_source"
            && action["reason"]
                == "case_window_packet_family_unrelated_for_capture_acceptance:history-only-step-discovery"
    }));
}

#[test]
fn local_health_validation_suite_rejects_case_window_outside_raw_export_time_window() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("bundle-validation.json");
    seed_goose_database(&db);
    write_raw_export_manifest(&bundle_dir, &db);
    write_steps_unavailable_manifest_for_day(&manifest_path, "2026-06-04");

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "database_source:case_window_outside_raw_export_time_window:bundle-step-unavailable-status")
    );
    let action = report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["case_id"] == "database_source"
                && action["scope"] == "database_source"
                && action["reason"]
                    == "case_window_outside_raw_export_time_window:bundle-step-unavailable-status"
        })
        .expect("missing database_source time-window next action");
    assert_eq!(
        action["action"],
        "Regenerate the Raw Export bundle with a time_window that covers this validation case, or adjust the validation manifest to the exported capture window."
    );
}

#[test]
fn local_health_validation_suite_rejects_raw_export_bundle_with_malformed_sqlite() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("bundle-validation.json");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let malformed_bytes = b"not a sqlite database";
    fs::write(&db, malformed_bytes).unwrap();
    fs::write(
        bundle_dir.join("manifest.json"),
        raw_export_manifest_bytes(&sha256_hex(malformed_bytes)),
    )
    .unwrap();
    write_steps_unavailable_manifest(&manifest_path);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["database_source"]["raw_export_manifest"]["ok"], true);
    assert_eq!(report["database_source"]["sqlite_audit"]["ok"], false);
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue
            .as_str()
            .is_some_and(|issue| issue.starts_with("database_source:sqlite_"))
    }));
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["case_id"] == "database_source"
                    && action["scope"] == "database_source"
                    && action["action"]
                        .as_str()
                        .is_some_and(|action| action.contains("data/goose.sqlite"))
            })
    );
}

#[test]
fn local_health_validation_suite_rejects_raw_export_bundle_with_unmarked_official_label_family() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("bundle-validation.json");
    seed_goose_database(&db);
    let sqlite_sha256 = sha256_hex(&fs::read(&db).unwrap());
    fs::write(
        bundle_dir.join("manifest.json"),
        raw_export_manifest_bytes_with_options(
            &sqlite_sha256,
            json!(["sqlite", "calibration_labels"]),
            false,
        ),
    )
    .unwrap();
    write_steps_unavailable_manifest(&manifest_path);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["official_labels_are_labels"],
        false
    );
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue == "database_source:official_labels_are_labels_not_true_for_calibration_labels"
    }));
    let action = report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["case_id"] == "database_source"
                && action["scope"] == "database_source"
                && action["reason"] == "official_labels_are_labels_not_true_for_calibration_labels"
        })
        .expect("missing database_source label-policy next action");
    assert_eq!(
        action["action"],
        "Regenerate the Raw Export bundle with official_labels_are_labels=true before using official WHOOP comparison labels."
    );
}

#[test]
fn local_health_validation_suite_rejects_raw_export_bundle_with_sqlite_sha_mismatch() {
    let tempdir = tempfile::tempdir().unwrap();
    let bundle_dir = tempdir.path().join("raw-export");
    let db = bundle_dir.join("data/goose.sqlite");
    let manifest_path = tempdir.path().join("bundle-validation.json");
    seed_goose_database(&db);
    fs::write(
        bundle_dir.join("manifest.json"),
        raw_export_manifest_bytes(
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
    )
    .unwrap();
    write_steps_unavailable_manifest(&manifest_path);

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--raw-export-bundle")
            .arg(&bundle_dir)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["ok"],
        false
    );
    assert_eq!(
        report["database_source"]["raw_export_manifest"]["sha256_match"],
        false
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "database_source:sqlite_manifest_sha256_mismatch")
    );
    let action = report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["case_id"] == "database_source"
                && action["scope"] == "database_source"
                && action["reason"] == "sqlite_manifest_sha256_mismatch"
        })
        .expect("missing database_source checksum next action");
    assert_eq!(
        action["action"],
        "Regenerate the Raw Export bundle; manifest.json and data/goose.sqlite do not describe the same SQLite snapshot."
    );
}

#[test]
fn local_health_validation_suite_runs_step_energy_and_recovery_sensor_cases() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let manifest_path = tempdir.path().join("local-health-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "empty-db-validation-smoke",
            "notes": "Smoke test for reproducible local health validation manifests.",
            "cases": [
                {
                    "id": "walk-100-steps",
                    "report": "step-validation",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "100_counted_steps",
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 5,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "walk-raw-motion-steps",
                    "report": "raw-motion-steps",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 10,
                    "sample_rate_hz": 50.0,
                    "peak_threshold_i16": 1200.0,
                    "min_peak_spacing_samples": 10,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "walk-step-daily-rollup",
                    "report": "step-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-03T00:00:00Z",
                    "min_sample_count": 2
                },
                {
                    "id": "walk-step-hourly-rollup",
                    "report": "step-hourly-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T11:00:00Z",
                    "min_sample_count": 2
                },
                {
                    "id": "walk-step-unavailable-status",
                    "report": "steps-unavailable-status",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-03T00:00:00Z",
                    "min_sample_count": 2,
                    "write_metric": true
                },
                {
                    "id": "walk-energy",
                    "report": "energy-validation",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "walk",
                    "profile_weight_kg": 80.0,
                    "resting_hr_bpm": 60.0,
                    "max_hr_bpm": 180.0,
                    "official_whoop_total_kcal": 2100.0,
                    "energy_tolerance_kcal": 250.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "walk-hourly-energy",
                    "report": "energy-hourly-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T11:00:00Z",
                    "capture_kind": "walk",
                    "profile_weight_kg": 80.0,
                    "resting_hr_bpm": 60.0,
                    "max_hr_bpm": 180.0,
                    "min_heart_rate_samples": 2
                },
                {
                    "id": "walk-energy-unavailable-status",
                    "report": "calories-unavailable-status",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T11:00:00Z",
                    "capture_kind": "walk",
                    "profile_weight_kg": 80.0,
                    "resting_hr_bpm": 60.0,
                    "max_hr_bpm": 180.0,
                    "min_heart_rate_samples": 2,
                    "write_metric": true
                },
                {
                    "id": "overnight-rhr",
                    "report": "rhr-validation",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "capture_kind": "overnight_rest",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "min_sample_count": 2,
                    "official_whoop_resting_hr_bpm": 56.0,
                    "rhr_tolerance_bpm": 3.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "overnight-hrv",
                    "report": "hrv-validation",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "capture_kind": "overnight_rest",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "min_rr_intervals_to_compute": 2,
                    "official_whoop_hrv_rmssd_ms": 42.0,
                    "hrv_tolerance_ms": 10.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "overnight-respiratory-rate",
                    "report": "respiratory-rate-validation",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "capture_kind": "overnight_rest",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "official_whoop_respiratory_rate_rpm": 14.5,
                    "respiratory_rate_tolerance_rpm": 1.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "overnight-oxygen-saturation",
                    "report": "spo2-validation",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "capture_kind": "overnight_rest",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "official_whoop_oxygen_saturation_percent": 97.0,
                    "oxygen_saturation_tolerance_percent": 2.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "overnight-temperature",
                    "report": "temperature-validation",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "capture_kind": "overnight_rest",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "official_whoop_skin_temperature_delta_c": 0.2,
                    "temperature_tolerance_c": 0.3,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "overnight-sensors",
                    "report": "recovery-sensors",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true
                },
                {
                    "id": "overnight-recovery-sensor-rollup",
                    "report": "recovery-sensor-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "min_rr_intervals_to_compute": 2,
                    "write_metric": true
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "goose.local-health-validation-suite-report.v1"
    );
    assert_eq!(report["manifest_id"], "empty-db-validation-smoke");
    assert_eq!(report["database_source"]["kind"], "direct_database");
    assert_eq!(
        report["database_source"]["input_path"],
        db.display().to_string()
    );
    assert_eq!(
        report["database_source"]["resolved_database_path"],
        db.display().to_string()
    );
    assert_eq!(
        report["database_source"]["temporary_extracted_database"],
        false
    );
    assert_eq!(
        report["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(report["case_count"], 15);
    assert_eq!(report["ok_case_count"], 15);
    assert_eq!(report["passing_case_count"], 2);
    assert_eq!(report["failing_case_count"], 13);
    assert_eq!(report["readiness_summary"]["case_count"], 15);
    assert_eq!(
        report["readiness_summary"]["acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["missing_packet_evidence_case_count"],
        13
    );
    assert_eq!(
        report["readiness_summary"]["unavailable_status_case_count"],
        2
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_declared_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_required_case_count"],
        8
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_missing_evidence_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_sparse_evidence_case_count"],
        0
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "walk-energy:case_report_not_passed")
    );
    let metric_records = report["metric_records"].as_array().unwrap();
    assert_eq!(metric_records.len(), 27);

    let cases = report["cases"].as_array().unwrap();
    let step = cases
        .iter()
        .find(|case| case["id"] == "walk-100-steps")
        .unwrap();
    assert_eq!(step["method"], "metrics.step_capture_validation");
    assert_eq!(step["label_policy_valid"], true);
    assert_eq!(step["readiness"]["normalized_report"], "step-validation");
    assert_eq!(
        step["readiness"]["evidence_status"],
        "missing_packet_evidence"
    );
    assert_eq!(
        step["readiness"]["official_label_status"],
        "official_labels_valid"
    );
    assert_eq!(
        step["readiness"]["manual_label_status"],
        "manual_label_present"
    );
    assert_eq!(step["readiness"]["acceptance_ready"], false);
    assert_eq!(
        step["result"]["schema"],
        "goose.step-capture-validation-report.v1"
    );
    assert_eq!(step["result"]["official_whoop_step_delta"], 97);

    let raw_motion = cases
        .iter()
        .find(|case| case["id"] == "walk-raw-motion-steps")
        .unwrap();
    assert_eq!(raw_motion["method"], "metrics.raw_motion_step_estimate");
    assert_eq!(
        raw_motion["result"]["schema"],
        "goose.raw-motion-step-estimate-report.v1"
    );
    assert_eq!(
        raw_motion["result"]["algorithm_id"],
        "goose.steps.raw_motion_estimate.v0"
    );

    let step_daily_rollup = cases
        .iter()
        .find(|case| case["id"] == "walk-step-daily-rollup")
        .unwrap();
    assert_eq!(
        step_daily_rollup["method"],
        "metrics.step_counter_daily_rollup"
    );
    assert_eq!(
        step_daily_rollup["result"]["schema"],
        "goose.step-counter-daily-rollup-report.v1"
    );
    assert_eq!(step_daily_rollup["result"]["daily_metric_written"], false);

    let step_hourly_rollup = cases
        .iter()
        .find(|case| case["id"] == "walk-step-hourly-rollup")
        .unwrap();
    assert_eq!(
        step_hourly_rollup["method"],
        "metrics.step_counter_hourly_rollup"
    );
    assert_eq!(
        step_hourly_rollup["result"]["schema"],
        "goose.step-counter-hourly-rollup-report.v1"
    );
    assert_eq!(step_hourly_rollup["result"]["hourly_metric_written"], false);

    let step_unavailable = cases
        .iter()
        .find(|case| case["id"] == "walk-step-unavailable-status")
        .unwrap();
    assert_eq!(
        step_unavailable["method"],
        "metrics.activity_unavailable_daily_status"
    );
    assert_eq!(
        step_unavailable["result"]["schema"],
        "goose.activity-unavailable-daily-status-report.v1"
    );
    assert_eq!(step_unavailable["pass"], true);
    assert_eq!(step_unavailable["result"]["unavailable_metric_count"], 1);
    assert_eq!(step_unavailable["result"]["written_metric_count"], 1);
    assert_eq!(
        step_unavailable["readiness"]["evidence_status"],
        "unavailable_status_recorded"
    );

    let energy = cases
        .iter()
        .find(|case| case["id"] == "walk-energy")
        .unwrap();
    assert_eq!(energy["method"], "metrics.energy_capture_validation");
    assert_eq!(
        energy["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        energy["result"]["energy_rollup"]["daily_metric_written"],
        false
    );

    let hourly_energy = cases
        .iter()
        .find(|case| case["id"] == "walk-hourly-energy")
        .unwrap();
    assert_eq!(hourly_energy["method"], "metrics.energy_hourly_rollup");
    assert_eq!(
        hourly_energy["result"]["schema"],
        "goose.energy-hourly-rollup-report.v1"
    );
    assert_eq!(hourly_energy["result"]["hourly_metric_written"], false);

    let energy_unavailable = cases
        .iter()
        .find(|case| case["id"] == "walk-energy-unavailable-status")
        .unwrap();
    assert_eq!(
        energy_unavailable["method"],
        "metrics.energy_unavailable_daily_status"
    );
    assert_eq!(
        energy_unavailable["result"]["schema"],
        "goose.energy-unavailable-daily-status-report.v1"
    );
    assert_eq!(energy_unavailable["pass"], true);
    assert_eq!(energy_unavailable["result"]["unavailable_metric_count"], 3);
    assert_eq!(energy_unavailable["result"]["written_metric_count"], 3);

    let rhr = cases
        .iter()
        .find(|case| case["id"] == "overnight-rhr")
        .unwrap();
    assert_eq!(rhr["method"], "metrics.resting_hr_capture_validation");
    assert_eq!(
        rhr["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        rhr["result"]["resting_hr_rollup"]["daily_metric_written"],
        false
    );
    assert_eq!(rhr["result"]["official_whoop_resting_hr_bpm"], 56.0);

    let hrv = cases
        .iter()
        .find(|case| case["id"] == "overnight-hrv")
        .unwrap();
    assert_eq!(hrv["method"], "metrics.hrv_capture_validation");
    assert_eq!(
        hrv["result"]["schema"],
        "goose.hrv-capture-validation-report.v1"
    );
    assert_eq!(
        hrv["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(hrv["result"]["official_whoop_hrv_rmssd_ms"], 42.0);
    assert_eq!(
        hrv["result"]["promotion_status"],
        "validation_only_rr_interval_scale_still_unverified"
    );

    let respiratory = cases
        .iter()
        .find(|case| case["id"] == "overnight-respiratory-rate")
        .unwrap();
    assert_eq!(
        respiratory["method"],
        "metrics.respiratory_rate_capture_validation"
    );
    assert_eq!(
        respiratory["result"]["schema"],
        "goose.respiratory-rate-capture-validation-report.v1"
    );
    assert_eq!(
        respiratory["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        respiratory["result"]["official_whoop_respiratory_rate_rpm"],
        14.5
    );
    assert_eq!(
        respiratory["result"]["promotion_status"],
        "validation_only_respiratory_rate_semantics_still_unverified"
    );

    let oxygen_saturation = cases
        .iter()
        .find(|case| case["id"] == "overnight-oxygen-saturation")
        .unwrap();
    assert_eq!(
        oxygen_saturation["method"],
        "metrics.oxygen_saturation_capture_validation"
    );
    assert_eq!(
        oxygen_saturation["result"]["schema"],
        "goose.oxygen-saturation-capture-validation-report.v1"
    );
    assert_eq!(
        oxygen_saturation["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        oxygen_saturation["result"]["official_whoop_oxygen_saturation_percent"],
        97.0
    );
    assert_eq!(
        oxygen_saturation["result"]["promotion_status"],
        "validation_only_oxygen_saturation_decoder_not_implemented"
    );

    let temperature = cases
        .iter()
        .find(|case| case["id"] == "overnight-temperature")
        .unwrap();
    assert_eq!(
        temperature["method"],
        "metrics.temperature_capture_validation"
    );
    assert_eq!(
        temperature["result"]["schema"],
        "goose.temperature-capture-validation-report.v1"
    );
    assert_eq!(
        temperature["result"]["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        temperature["result"]["official_whoop_skin_temperature_delta_c"],
        0.2
    );
    assert_eq!(
        temperature["result"]["promotion_status"],
        "validation_only_temperature_units_still_unverified"
    );

    for validation_only_case in [hrv, respiratory, oxygen_saturation, temperature] {
        assert_eq!(
            validation_only_case["readiness"]["acceptance_ready"], false,
            "{}",
            validation_only_case["id"]
        );
        assert!(
            validation_only_case["readiness"]["missing"]
                .as_array()
                .unwrap()
                .iter()
                .any(|missing| missing == "metric_promotion"),
            "{}",
            validation_only_case["id"]
        );
        assert!(
            validation_only_case["readiness"]["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|blocker| blocker == "validation_only_promotion_status"),
            "{}",
            validation_only_case["id"]
        );
    }

    let sensors = cases
        .iter()
        .find(|case| case["id"] == "overnight-sensors")
        .unwrap();
    assert_eq!(sensors["method"], "metrics.recovery_sensor_discovery");
    assert_eq!(
        sensors["result"]["schema"],
        "goose.recovery-sensor-discovery-report.v1"
    );
    assert_eq!(sensors["result"]["widgets"].as_array().unwrap().len(), 4);
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "overnight-sensors"
                && action["reason"] == "oxygen_saturation_decoder_not_implemented")
    );

    let recovery_sensor_rollup = cases
        .iter()
        .find(|case| case["id"] == "overnight-recovery-sensor-rollup")
        .unwrap();
    assert_eq!(
        recovery_sensor_rollup["method"],
        "metrics.recovery_sensor_daily_rollup"
    );
    assert_eq!(
        recovery_sensor_rollup["result"]["schema"],
        "goose.recovery-sensor-daily-rollup-report.v1"
    );
    assert_eq!(recovery_sensor_rollup["pass"], false);
    assert_eq!(recovery_sensor_rollup["result"]["metric_count"], 4);
    assert_eq!(recovery_sensor_rollup["result"]["promoted_metric_count"], 0);
    assert_eq!(recovery_sensor_rollup["result"]["written_metric_count"], 0);
    assert_eq!(
        recovery_sensor_rollup["readiness"]["evidence_status"],
        "missing_packet_evidence"
    );

    let step_record = metric_record(metric_records, "walk-100-steps", "steps");
    assert_eq!(step_record["metric_family"], "activity");
    assert_eq!(step_record["source_kind"], "unavailable");
    assert_eq!(step_record["official_label_value"], 97);
    assert_eq!(step_record["manual_label_value"], 100);
    assert_eq!(step_record["input_packet_count"], 0);
    assert_eq!(step_record["algorithm_id"], "goose.steps.device_counter.v0");
    assert_eq!(step["metric_records"][0]["metric_name"], "steps");

    let daily_step_rollup_record = metric_record(metric_records, "walk-step-daily-rollup", "steps");
    assert_eq!(daily_step_rollup_record["metric_family"], "activity");
    assert_eq!(daily_step_rollup_record["source_kind"], "unavailable");
    assert_eq!(
        daily_step_rollup_record["algorithm_id"],
        "goose.steps.device_counter.v0"
    );
    assert_eq!(daily_step_rollup_record["promotion_status"], "daily_rollup");
    assert_eq!(daily_step_rollup_record["input_counts"]["sample_count"], 0);

    let hourly_step_rollup_record =
        metric_record(metric_records, "walk-step-hourly-rollup", "steps");
    assert_eq!(hourly_step_rollup_record["metric_family"], "activity");
    assert_eq!(hourly_step_rollup_record["source_kind"], "unavailable");
    assert_eq!(
        hourly_step_rollup_record["algorithm_id"],
        "goose.steps.device_counter.v0"
    );
    assert_eq!(
        hourly_step_rollup_record["promotion_status"],
        "hourly_rollup"
    );
    assert_eq!(hourly_step_rollup_record["input_counts"]["sample_count"], 0);

    let step_unavailable_record =
        metric_record(metric_records, "walk-step-unavailable-status", "steps");
    assert_eq!(step_unavailable_record["metric_family"], "activity");
    assert_eq!(step_unavailable_record["source_kind"], "unavailable");
    assert_eq!(
        step_unavailable_record["algorithm_id"],
        "goose.activity.unavailable_status.v0"
    );
    assert_eq!(step_unavailable_record["promotion_status"], "blocked");
    assert_eq!(step_unavailable_record["input_counts"]["sample_count"], 0);
    assert!(
        step_unavailable_record["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "insufficient_step_counter_samples")
    );

    let total_energy_record = metric_record(metric_records, "walk-energy", "total_kcal");
    assert_eq!(total_energy_record["metric_family"], "activity");
    assert_eq!(total_energy_record["source_kind"], "unavailable");
    assert_eq!(total_energy_record["official_label_value"], 2100.0);
    assert_eq!(
        total_energy_record["algorithm_id"],
        "goose.energy.local_estimate.v0"
    );
    assert_eq!(
        total_energy_record["label_policy"],
        "official_whoop_values_are_validation_labels_not_inputs"
    );
    assert_eq!(
        total_energy_record["input_counts"]["heart_rate_sample_count"],
        0
    );
    assert_eq!(
        total_energy_record["input_counts"]["motion_sample_count"],
        0
    );

    let hourly_total_energy_record =
        metric_record(metric_records, "walk-hourly-energy", "total_kcal");
    assert_eq!(hourly_total_energy_record["metric_family"], "activity");
    assert_eq!(hourly_total_energy_record["source_kind"], "unavailable");
    assert_eq!(
        hourly_total_energy_record["algorithm_id"],
        "goose.energy.local_estimate.v0"
    );
    assert_eq!(
        hourly_total_energy_record["promotion_status"],
        "hourly_rollup"
    );
    assert_eq!(
        hourly_total_energy_record["input_counts"]["heart_rate_sample_count"],
        0
    );
    assert_eq!(
        hourly_total_energy_record["input_counts"]["motion_sample_count"],
        0
    );

    let energy_unavailable_record = metric_record(
        metric_records,
        "walk-energy-unavailable-status",
        "total_kcal",
    );
    assert_eq!(energy_unavailable_record["metric_family"], "activity");
    assert_eq!(energy_unavailable_record["source_kind"], "unavailable");
    assert_eq!(
        energy_unavailable_record["algorithm_id"],
        "goose.energy.unavailable_status.v0"
    );
    assert_eq!(energy_unavailable_record["promotion_status"], "blocked");
    assert_eq!(
        energy_unavailable_record["input_counts"]["heart_rate_sample_count"],
        0
    );
    assert!(
        energy_unavailable_record["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "insufficient_heart_rate_samples")
    );

    let rhr_record = metric_record(metric_records, "overnight-rhr", "resting_hr");
    assert_eq!(rhr_record["metric_family"], "recovery");
    assert_eq!(rhr_record["source_kind"], "unavailable");
    assert_eq!(rhr_record["official_label_value"], 56.0);
    assert_eq!(
        rhr_record["algorithm_id"],
        "goose.resting_heart_rate.device_sensor.v0"
    );

    let hrv_record = metric_record(metric_records, "overnight-hrv", "hrv_rmssd");
    assert_eq!(hrv_record["source_kind"], "unavailable");
    assert_eq!(hrv_record["official_label_value"], 42.0);
    assert_eq!(
        hrv_record["promotion_status"],
        "validation_only_rr_interval_scale_still_unverified"
    );

    let respiratory_record = metric_record(
        metric_records,
        "overnight-respiratory-rate",
        "respiratory_rate",
    );
    assert_eq!(respiratory_record["source_kind"], "unavailable");
    assert_eq!(respiratory_record["official_label_value"], 14.5);
    assert_eq!(
        respiratory_record["promotion_status"],
        "validation_only_respiratory_rate_semantics_still_unverified"
    );

    let oxygen_validation_record = metric_record(
        metric_records,
        "overnight-oxygen-saturation",
        "oxygen_saturation",
    );
    assert_eq!(oxygen_validation_record["source_kind"], "unavailable");
    assert_eq!(oxygen_validation_record["official_label_value"], 97.0);
    assert_eq!(
        oxygen_validation_record["promotion_status"],
        "validation_only_oxygen_saturation_decoder_not_implemented"
    );
    assert_eq!(
        oxygen_validation_record["algorithm_id"],
        "goose.oxygen_saturation.packet_candidate.v0"
    );

    let temperature_validation_record = metric_record(
        metric_records,
        "overnight-temperature",
        "skin_temperature_delta",
    );
    assert_eq!(temperature_validation_record["source_kind"], "unavailable");
    assert_eq!(temperature_validation_record["official_label_value"], 0.2);
    assert_eq!(
        temperature_validation_record["promotion_status"],
        "validation_only_temperature_units_still_unverified"
    );
    assert_eq!(
        temperature_validation_record["algorithm_id"],
        "goose.skin_temperature.history_candidate.v0"
    );

    let oxygen_record = metric_record(metric_records, "overnight-sensors", "oxygen_saturation");
    assert_eq!(oxygen_record["source_kind"], "unavailable");
    assert_eq!(oxygen_record["confidence"], 0.0);
    assert_eq!(oxygen_record["input_packet_count"], 0);
    assert!(
        oxygen_record["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "oxygen_saturation_decoder_not_implemented")
    );

    let recovery_rollup_oxygen_record = metric_record(
        metric_records,
        "overnight-recovery-sensor-rollup",
        "oxygen_saturation",
    );
    assert_eq!(recovery_rollup_oxygen_record["source_kind"], "unavailable");
    assert_eq!(
        recovery_rollup_oxygen_record["algorithm_id"],
        "goose.recovery_sensor.device_sensor.v0"
    );
    assert_eq!(recovery_rollup_oxygen_record["confidence"], 0.0);
    assert!(
        recovery_rollup_oxygen_record["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "oxygen_saturation_decoder_not_implemented")
    );
}

#[test]
fn local_health_validation_suite_reports_capture_session_evidence_readiness() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "capture-session-1",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true}"#,
        })
        .unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "unrelated-session",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true,"note":"same-window unrelated"}"#,
        })
        .unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "wrong-family-session",
            source: "synthetic.validation",
            started_at_unix_ms: 1_780_392_000_000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: r#"{"owned_capture":true,"note":"history packets in step case"}"#,
        })
        .unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "raw-capture-session-1",
            source: "synthetic.validation",
            captured_at: "2026-06-02T10:01:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &[0x01, 0x02, 0x03],
            sensitivity: "public-test-fixture",
            capture_session_id: Some("capture-session-1"),
        })
        .unwrap();
    for (evidence_id, captured_at, payload, step_count) in [
        (
            "raw-unrelated-step-1",
            "2026-06-02T10:01:00Z",
            [0x10, 0x01],
            10,
        ),
        (
            "raw-unrelated-step-2",
            "2026-06-02T10:03:00Z",
            [0x10, 0x02],
            110,
        ),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &payload,
                sensitivity: "public-test-fixture",
                capture_session_id: Some("unrelated-session"),
            })
            .unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                r#"
                INSERT OR IGNORE INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, 11, 'DATA', ?3, NULL, ?4, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(step_count),
                    json!({
                        "packet_k": 11,
                        "domain": "raw_stream_counted",
                        "step_count": step_count
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }
    for (evidence_id, captured_at, sequence) in [
        ("raw-wrong-family-1", "2026-06-02T10:01:00Z", 18),
        ("raw-wrong-family-2", "2026-06-02T10:03:00Z", 19),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &[0x18, sequence],
                sensitivity: "public-test-fixture",
                capture_session_id: Some("wrong-family-session"),
            })
            .unwrap();
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                r#"
                INSERT OR IGNORE INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (?1, ?2, 'Goose', 2, 0, 2, '0000', '', 1, 1, 18, 'DATA', ?3, NULL, ?4, 'test', '[]')
                "#,
                (
                    format!("frame-{evidence_id}"),
                    evidence_id,
                    i64::from(sequence),
                    json!({
                        "packet_k": 18,
                        "domain": "normal_history",
                        "body_summary": {
                            "kind": "normal_history",
                            "heart_rate_bpm": 72
                        }
                    })
                    .to_string(),
                ),
            )
            .unwrap();
    }
    drop(store);

    let manifest_path = tempdir.path().join("session-bound-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "cases": [
                {
                    "id": "session-bound-step",
                    "report": "step-validation",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "100_counted_steps",
                    "capture_session_id": "capture-session-1",
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 5,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "missing-session-step",
                    "report": "step-validation",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "100_counted_steps",
                    "capture_session_ids": ["missing-session"],
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 5,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                },
                {
                    "id": "wrong-family-step",
                    "report": "step-validation",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "100_counted_steps",
                    "capture_session_id": "wrong-family-session",
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 5,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["readiness_summary"]["case_count"], 3);
    assert_eq!(
        report["readiness_summary"]["capture_session_declared_case_count"],
        3
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_required_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_missing_evidence_case_count"],
        1
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_sparse_evidence_case_count"],
        1
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_unrelated_packet_family_case_count"],
        1
    );

    let session_bound = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "session-bound-step")
        .unwrap();
    assert_eq!(
        session_bound["readiness"]["capture_session_status"],
        "declared_with_evidence"
    );
    assert_eq!(
        session_bound["readiness"]["expected_capture_session_ids"][0],
        "capture-session-1"
    );
    assert_eq!(
        session_bound["readiness"]["capture_session_raw_evidence_count"],
        1
    );
    assert_eq!(
        session_bound["readiness"]["capture_session_decoded_frame_count"],
        0
    );
    assert_eq!(session_bound["result"]["decoded_frame_count"], 0);
    assert_eq!(session_bound["result"]["counter_delta_candidate_count"], 0);
    assert_eq!(session_bound["pass"], false);
    assert!(
        !session_bound["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "capture_session_evidence")
    );

    let missing_session = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "missing-session-step")
        .unwrap();
    assert_eq!(
        missing_session["readiness"]["capture_session_status"],
        "declared_missing_evidence"
    );
    assert_eq!(
        missing_session["readiness"]["missing_capture_session_ids"][0],
        "missing-session"
    );
    assert!(
        missing_session["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "capture_session_evidence_missing")
    );
    assert!(
        missing_session["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "capture_session_evidence")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "missing-session-step"
                && action["reason"] == "capture_session_evidence_missing")
    );

    let wrong_family = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "wrong-family-step")
        .unwrap();
    assert_eq!(
        wrong_family["readiness"]["capture_session_status"],
        "declared_with_evidence"
    );
    assert_eq!(
        wrong_family["readiness"]["capture_session_decoded_frame_count"],
        2
    );
    assert_eq!(
        wrong_family["readiness"]["capture_session_decoded_frame_time_bounds"]["span_ms"],
        120_000
    );
    assert_eq!(
        wrong_family["readiness"]["capture_session_packet_family_counts"]["K18/normal_history"],
        2
    );
    assert_eq!(
        wrong_family["readiness"]["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert!(wrong_family["readiness"]["capture_session_relevant_packet_family_counts"].is_null());
    assert_eq!(wrong_family["readiness"]["acceptance_ready"], false);
    assert_eq!(wrong_family["readiness"]["capture_acceptance_ready"], false);
    assert!(
        wrong_family["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "capture_session_relevant_packet_family")
    );
    assert!(
        wrong_family["readiness"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker
                == "capture_session_packet_family_unrelated_for_capture_acceptance")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "wrong-family-step"
                && action["reason"]
                    == "capture_session_packet_family_unrelated_for_capture_acceptance")
    );
}

#[test]
fn local_health_validation_suite_imports_capture_sqlite_before_running_cases() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let capture_sqlite_path = tempdir.path().join("capture.sqlite");
    let manifest_path = tempdir.path().join("capture-sqlite-validation.json");
    let output_path = tempdir.path().join("capture-sqlite-validation-report.json");
    let review_output_path = tempdir.path().join("capture-sqlite-validation-review.json");

    let frame_hex = k10_motion_step_frame_hex(&[10, 25, 40, 55, 70]);
    seed_processed_capture_sqlite(&capture_sqlite_path, &frame_hex);

    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "capture-sqlite-validation-smoke",
            "capture_sqlite_imports": [
                {
                    "id": "walk-hci",
                    "path": "capture.sqlite",
                    "session_id": "capture-sqlite-session",
                    "device_model": "WHOOP 5.0 Goose",
                    "sensitivity": "user-owned-capture",
                    "parser_version": "goose-core/test"
                }
            ],
            "cases": [
                {
                    "id": "capture-sqlite-raw-motion-steps",
                    "report": "raw-motion-steps",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_session_id": "capture-sqlite-session",
                    "manual_step_delta": 5,
                    "official_whoop_step_delta": 5,
                    "step_delta_tolerance": 0,
                    "sample_rate_hz": 50.0,
                    "peak_threshold_i16": 1200.0,
                    "min_peak_spacing_samples": 10,
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "write_metric": true,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--output")
            .arg(&output_path)
            .arg("--review-output")
            .arg(&review_output_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        report["schema"],
        "goose.local-health-validation-suite-report.v1"
    );
    assert_eq!(report["pass"], true);
    assert_eq!(report["manifest_id"], "capture-sqlite-validation-smoke");
    assert_eq!(report["case_count"], 1);
    assert_eq!(report["ok_case_count"], 1);
    assert_eq!(report["passing_case_count"], 1);
    assert_eq!(report["failing_case_count"], 0);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
    let review: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review_output_path).unwrap()).unwrap();
    assert_eq!(review["status"], "ready_to_run_validation_suite");
    assert_eq!(review["capture_sqlite_import_count"], 1);
    assert_eq!(review["capture_sqlite_import_invalid_count"], 0);
    assert_eq!(
        review["capture_sqlite_import_session_ids"],
        json!(["capture-sqlite-session"])
    );
    assert_eq!(
        review["known_capture_session_ids"],
        json!(["capture-sqlite-session"])
    );
    assert_eq!(review["capture_session_unresolved_case_count"], 0);
    assert_eq!(review["capture_sqlite_imports"][0]["id"], "walk-hci");
    assert_eq!(
        review["capture_sqlite_imports"][0]["path"],
        "capture.sqlite"
    );
    assert_eq!(review["capture_sqlite_imports"][0]["status"], "declared");

    let import = &report["capture_sqlite_imports"][0];
    assert_eq!(import["id"], "walk-hci");
    assert_eq!(import["session_id"], "capture-sqlite-session");
    assert_eq!(import["ok"], true);
    assert_eq!(import["import_ready"], true);
    assert_eq!(import["raw_import_completed"], true);
    assert_eq!(import["decode_pass"], true);
    assert_eq!(import["source_frame_count"], 1);
    assert_eq!(import["raw_inserted"], 1);
    assert_eq!(import["frames_inserted"], 1);
    assert_eq!(import["parse_failed_count"], 0);

    let case = &report["cases"][0];
    assert_eq!(case["id"], "capture-sqlite-raw-motion-steps");
    assert_eq!(case["pass"], true);
    assert_eq!(
        case["readiness"]["capture_session_status"],
        "declared_with_evidence"
    );
    assert_eq!(case["readiness"]["capture_session_raw_evidence_count"], 1);
    assert_eq!(case["readiness"]["capture_session_decoded_frame_count"], 1);
    assert_eq!(
        case["readiness"]["capture_session_decoded_frame_time_bounds"]["first_captured_at"],
        "2026-06-02T10:00:30Z"
    );
    assert_eq!(
        case["readiness"]["capture_session_decoded_frame_time_bounds"]["last_captured_at"],
        "2026-06-02T10:00:30Z"
    );
    assert_eq!(
        case["readiness"]["capture_session_decoded_frame_time_bounds"]["span_ms"],
        0
    );
    assert_eq!(case["readiness"]["acceptance_ready"], true);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], false);
    assert!(
        case["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "capture_session_packet_span")
    );
    assert!(
        case["readiness"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker
                == "capture_session_decoded_evidence_too_sparse_for_capture_acceptance")
    );
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_sparse_evidence_case_count"],
        1
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_unrelated_packet_family_case_count"],
        0
    );
    assert!(
        case["readiness"]["capture_session_packet_family_counts"]
            .as_object()
            .unwrap()
            .keys()
            .any(|family| family.starts_with("K10"))
    );
    assert_eq!(
        case["readiness"]["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert!(
        case["readiness"]["capture_session_relevant_packet_family_counts"]
            .as_object()
            .unwrap()
            .keys()
            .any(|family| family.starts_with("K10"))
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |action| action["case_id"] == "capture-sqlite-raw-motion-steps"
                    && action["reason"]
                        == "capture_session_decoded_evidence_too_sparse_for_capture_acceptance"
            )
    );
    assert_eq!(case["result"]["estimated_steps"], 5);
    assert_eq!(case["result"]["promotion_status"], "validated_candidate");
    assert_eq!(case["result"]["user_visible_value_allowed"], true);
    assert_eq!(case["result"]["daily_metric_written"], true);

    let metric_records = report["metric_records"].as_array().unwrap();
    let step_record = metric_record(metric_records, "capture-sqlite-raw-motion-steps", "steps");
    assert_eq!(step_record["metric_family"], "activity");
    assert_eq!(step_record["source_kind"], "local_estimate");
    assert_eq!(step_record["local_value"], 5);
    assert_eq!(step_record["manual_label_value"], 5);
    assert_eq!(step_record["official_label_value"], 5);
    assert_eq!(step_record["promotion_status"], "validated_candidate");

    let store = GooseStore::open(&db).unwrap();
    let daily_metric_id = case["result"]["daily_metric_id"]
        .as_str()
        .expect("raw-motion daily metric id");
    let metric = store
        .daily_activity_metric(daily_metric_id)
        .unwrap()
        .expect("source DB raw-motion step metric");
    assert_eq!(metric.steps, Some(5));
    assert_eq!(metric.source_kind, "local_estimate");
    assert_eq!(
        store
            .metric_provenance_for_metric("daily_activity", daily_metric_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn local_health_validation_suite_applies_manifest_case_defaults() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let capture_sqlite_path = tempdir.path().join("capture.sqlite");
    let manifest_path = tempdir.path().join("defaulted-capture-validation.json");

    let frame_hex = k10_motion_step_frame_hex(&[10, 25, 40, 55, 70]);
    seed_processed_capture_sqlite_frames(
        &capture_sqlite_path,
        &[
            ("2026-06-02T10:00:30+00:00", &frame_hex),
            ("2026-06-02T10:04:30+00:00", &frame_hex),
        ],
    );

    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "defaulted-capture-validation-smoke",
            "start": "2026-06-02T10:00:00Z",
            "end": "2026-06-02T10:05:00Z",
            "date_key": "2026-06-02",
            "timezone": "Europe/London",
            "capture_session_id": "defaulted-capture-session",
            "min_owned_captures": 1,
            "label_provenance": {
                "source": "manual_plus_official_app",
                "official_labels_are_labels": true
            },
            "capture_sqlite_imports": [
                {
                    "id": "defaulted-walk-hci",
                    "path": "capture.sqlite",
                    "session_id": "defaulted-capture-session",
                    "device_model": "WHOOP 5.0 Goose",
                    "sensitivity": "user-owned-capture",
                    "parser_version": "goose-core/test"
                }
            ],
            "cases": [
                {
                    "id": "defaulted-raw-motion-steps",
                    "report": "raw-motion-steps",
                    "manual_step_delta": 10,
                    "official_whoop_step_delta": 10,
                    "step_delta_tolerance": 0,
                    "sample_rate_hz": 50.0,
                    "peak_threshold_i16": 1200.0,
                    "min_peak_spacing_samples": 10,
                    "require_trusted_evidence": true,
                    "write_metric": true
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["manifest_id"], "defaulted-capture-validation-smoke");
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        1
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_sparse_evidence_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_unrelated_packet_family_case_count"],
        0
    );

    let case = &report["cases"][0];
    assert_eq!(case["id"], "defaulted-raw-motion-steps");
    assert_eq!(case["pass"], true);
    assert_eq!(
        case["readiness"]["capture_session_status"],
        "declared_with_evidence"
    );
    assert_eq!(
        case["readiness"]["expected_capture_session_ids"][0],
        "defaulted-capture-session"
    );
    assert_eq!(case["readiness"]["acceptance_ready"], true);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], true);
    assert_eq!(
        case["readiness"]["capture_session_decoded_frame_time_bounds"]["span_ms"],
        240_000
    );
    assert!(
        case["readiness"]["capture_session_packet_family_counts"]
            .as_object()
            .unwrap()
            .keys()
            .any(|family| family.starts_with("K10"))
    );
    assert_eq!(
        case["readiness"]["capture_acceptance_required_packet_family_prefixes"],
        json!(["K10", "K11", "K21"])
    );
    assert!(
        case["readiness"]["capture_session_relevant_packet_family_counts"]
            .as_object()
            .unwrap()
            .keys()
            .any(|family| family.starts_with("K10"))
    );
    assert_eq!(case["label_policy_valid"], true);
    assert_eq!(
        case["readiness"]["official_label_status"],
        "official_labels_valid"
    );
    assert_eq!(case["result"]["estimated_steps"], 10);
    assert_eq!(case["result"]["daily_metric_written"], true);
}

#[test]
fn local_health_validation_suite_requires_capture_session_binding_for_labeled_acceptance() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let capture_sqlite_path = tempdir.path().join("capture.sqlite");
    let manifest_path = tempdir.path().join("unbound-labeled-validation.json");

    let frame_hex = k10_motion_step_frame_hex(&[10, 25, 40, 55, 70]);
    seed_processed_capture_sqlite(&capture_sqlite_path, &frame_hex);

    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "unbound-labeled-validation-smoke",
            "capture_sqlite_imports": [
                {
                    "id": "walk-hci",
                    "path": "capture.sqlite",
                    "session_id": "capture-sqlite-session",
                    "device_model": "WHOOP 5.0 Goose",
                    "sensitivity": "user-owned-capture",
                    "parser_version": "goose-core/test"
                }
            ],
            "cases": [
                {
                    "id": "unbound-raw-motion-steps",
                    "report": "raw-motion-steps",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "manual_step_delta": 5,
                    "official_whoop_step_delta": 5,
                    "step_delta_tolerance": 0,
                    "sample_rate_hz": 50.0,
                    "peak_threshold_i16": 1200.0,
                    "min_peak_spacing_samples": 10,
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "write_metric": true,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], false);
    assert_eq!(report["case_count"], 1);
    assert_eq!(report["ok_case_count"], 1);
    assert_eq!(report["passing_case_count"], 1);
    assert_eq!(
        report["readiness_summary"]["acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_declared_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_required_case_count"],
        1
    );
    assert!(
        report["issues"].as_array().unwrap().iter().any(
            |issue| issue == "unbound-raw-motion-steps:capture_session_required_for_acceptance"
        )
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                == "unbound-raw-motion-steps:capture_session_required_for_metric_write")
    );

    let case = &report["cases"][0];
    assert_eq!(case["pass"], true);
    assert_eq!(case["result"]["estimated_steps"], 5);
    assert_eq!(case["result"]["daily_metric_written"], false);
    assert_eq!(case["readiness"]["capture_session_status"], "not_declared");
    assert_eq!(case["readiness"]["acceptance_ready"], false);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], false);
    assert!(
        case["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "capture_session_id")
    );
    assert!(
        case["readiness"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_required_for_acceptance")
    );
    assert!(
        case["readiness"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_required_for_capture_acceptance")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "unbound-raw-motion-steps"
                && action["reason"] == "capture_session_required_for_acceptance")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "unbound-raw-motion-steps"
                && action["reason"] == "capture_session_required_for_capture_acceptance")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "unbound-raw-motion-steps"
                && action["reason"] == "capture_session_required_for_metric_write")
    );

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("metric_provenance").unwrap(), 0);
}

#[test]
fn local_health_validation_suite_blocks_raw_motion_writes_for_partial_capture_session_evidence() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let capture_sqlite_path = tempdir.path().join("capture.sqlite");
    let manifest_path = tempdir.path().join("partial-session-write-validation.json");

    let frame_hex = k10_motion_step_frame_hex(&[10, 25, 40, 55, 70]);
    seed_processed_capture_sqlite(&capture_sqlite_path, &frame_hex);

    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "partial-session-write-validation-smoke",
            "capture_sqlite_imports": [
                {
                    "id": "walk-hci",
                    "path": "capture.sqlite",
                    "session_id": "capture-sqlite-session",
                    "device_model": "WHOOP 5.0 Goose",
                    "sensitivity": "user-owned-capture",
                    "parser_version": "goose-core/test"
                }
            ],
            "cases": [
                {
                    "id": "partial-raw-motion-steps",
                    "report": "raw-motion-steps",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_session_ids": ["capture-sqlite-session", "missing-session"],
                    "manual_step_delta": 5,
                    "official_whoop_step_delta": 5,
                    "step_delta_tolerance": 0,
                    "sample_rate_hz": 50.0,
                    "peak_threshold_i16": 1200.0,
                    "min_peak_spacing_samples": 10,
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "write_metric": true,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let case = &report["cases"][0];
    assert_eq!(case["pass"], true);
    assert_eq!(
        case["readiness"]["capture_session_status"],
        "declared_partial_evidence"
    );
    assert_eq!(
        case["readiness"]["missing_capture_session_ids"][0],
        "missing-session"
    );
    assert_eq!(case["result"]["estimated_steps"], 5);
    assert_eq!(case["result"]["daily_metric_written"], false);
    assert!(
        case["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "capture_session_evidence_missing")
    );

    let store = GooseStore::open(&db).unwrap();
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("metric_provenance").unwrap(), 0);
}

#[test]
fn local_health_validation_suite_rejects_metric_writes_for_label_only_validation_reports() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let manifest_path = tempdir.path().join("write-metric-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "cases": [
                {
                    "id": "bad-energy-write",
                    "report": "energy-validation",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "walk",
                    "profile_weight_kg": 80.0,
                    "resting_hr_bpm": 60.0,
                    "max_hr_bpm": 180.0,
                    "official_whoop_total_kcal": 2100.0,
                    "energy_tolerance_kcal": 250.0,
                    "write_metric": true,
                    "label_provenance": {
                        "source": "official_app_manual_entry",
                        "official_labels_are_labels": true
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "bad-energy-write:write_metric_not_allowed_for_validation_report")
    );
    assert!(
        report["cases"][0]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "write_metric_not_allowed_for_validation_report")
    );
    assert_eq!(
        report["cases"][0]["result"]["energy_rollup"]["daily_metric_written"],
        false
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "bad-energy-write"
                && action["reason"] == "write_metric_not_allowed_for_validation_report")
    );
}

#[test]
fn local_health_validation_suite_reports_step_discovery_without_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "raw-step-discovery-1",
            source: "synthetic.validation",
            captured_at: "2026-06-02T10:01:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &[0x10, 0x01],
            sensitivity: "public-test-fixture",
            capture_session_id: None,
        })
        .unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "raw-step-discovery-2",
            source: "synthetic.validation",
            captured_at: "2026-06-02T10:03:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &[0x10, 0x02],
            sensitivity: "public-test-fixture",
            capture_session_id: None,
        })
        .unwrap();
    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            r#"
            INSERT INTO decoded_frames (
                frame_id,
                evidence_id,
                device_type,
                raw_len,
                header_len,
                declared_len,
                payload_hex,
                payload_crc_hex,
                header_crc_valid,
                payload_crc_valid,
                packet_type,
                packet_type_name,
                sequence,
                command_or_event,
                parsed_payload_json,
                parser_version,
                warnings_json
            ) VALUES (
                'frame-step-discovery-1',
                'raw-step-discovery-1',
                'Goose',
                2,
                0,
                2,
                '0000',
                '',
                1,
                1,
                11,
                'DATA',
                4100,
                NULL,
                ?1,
                'test',
                '[]'
            )
            "#,
            [json!({
                "kind": "data_packet",
                "packet_k": 11,
                "domain": "raw_stream_counted",
                "body_summary": {
                    "kind": "raw_stream_counted",
                    "step_count": 4100,
                    "cadence": 98,
                    "activity": 2
                },
                "warnings": []
            })
            .to_string()],
        )
        .unwrap();
    connection
        .execute(
            r#"
            INSERT INTO decoded_frames (
                frame_id,
                evidence_id,
                device_type,
                raw_len,
                header_len,
                declared_len,
                payload_hex,
                payload_crc_hex,
                header_crc_valid,
                payload_crc_valid,
                packet_type,
                packet_type_name,
                sequence,
                command_or_event,
                parsed_payload_json,
                parser_version,
                warnings_json
            ) VALUES (
                'frame-step-discovery-2',
                'raw-step-discovery-2',
                'Goose',
                2,
                0,
                2,
                '0000',
                '',
                1,
                1,
                11,
                'DATA',
                4200,
                NULL,
                ?1,
                'test',
                '[]'
            )
            "#,
            [json!({
                "kind": "data_packet",
                "packet_k": 11,
                "domain": "raw_stream_counted",
                "body_summary": {
                    "kind": "raw_stream_counted",
                    "step_count": 4200,
                    "cadence": 101,
                    "activity": 2
                },
                "warnings": []
            })
            .to_string()],
        )
        .unwrap();
    drop(connection);
    drop(store);

    let manifest_path = tempdir.path().join("step-discovery-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "step-discovery-smoke",
            "cases": [
                {
                    "id": "step-discovery",
                    "report": "step-discovery",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["case_count"], 1);
    assert_eq!(report["passing_case_count"], 1);
    let case = &report["cases"][0];
    assert_eq!(case["method"], "metrics.step_packet_discovery");
    assert_eq!(
        case["result"]["schema"],
        "goose.step-packet-discovery-report.v1"
    );
    assert_eq!(case["result"]["explicit_step_counter_found"], true);
    assert_eq!(case["result"]["counter_delta_candidate_count"], 1);
    assert_eq!(case["result"]["selected_counter_delta"]["delta"], 100);
    assert_eq!(
        case["result"]["selected_counter_delta"]["selection_reason"],
        "explicit_step_counter_delta"
    );
    assert_eq!(case["readiness"]["official_label_status"], "not_required");
    assert_eq!(case["readiness"]["manual_label_status"], "not_required");
    assert_eq!(case["readiness"]["evidence_status"], "ready");
    assert_eq!(case["readiness"]["acceptance_ready"], true);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], false);
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );
    assert!(
        case["readiness"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_required_for_capture_acceptance")
    );
    assert!(
        report["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["case_id"] == "step-discovery"
                && action["reason"] == "capture_session_required_for_capture_acceptance")
    );

    let metric_records = report["metric_records"].as_array().unwrap();
    let record = metric_record(metric_records, "step-discovery", "step_counter_presence");
    assert_eq!(record["source_kind"], "device_counter");
    assert_eq!(record["local_value"], true);
    assert_eq!(record["promotion_status"], "device_counter_candidate");
    assert_eq!(record["input_counts"]["candidate_field_count"], 6);
    assert_eq!(record["input_counts"]["selected_counter_delta_rank"], 1);
    assert!(
        record["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "selected_counter_delta_json_path:$.body_summary.step_count")
    );
    assert!(
        record["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "selected_counter_delta_reason:explicit_step_counter_delta")
    );
}

#[test]
fn local_health_validation_suite_keeps_hidden_step_counter_candidate_unavailable_until_parser_mapping()
 {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    for (evidence_id, captured_at, payload) in [
        (
            "raw-hidden-step-1",
            "2026-06-02T10:01:00Z",
            &[0x11, 0x01][..],
        ),
        (
            "raw-hidden-step-2",
            "2026-06-02T10:03:00Z",
            &[0x11, 0x02][..],
        ),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.validation",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload,
                sensitivity: "public-test-fixture",
                capture_session_id: None,
            })
            .unwrap();
    }
    let connection = Connection::open(&db).unwrap();
    for (frame_id, evidence_id, sequence, hidden_counter) in [
        ("frame-hidden-step-1", "raw-hidden-step-1", 4100, 4100),
        ("frame-hidden-step-2", "raw-hidden-step-2", 4200, 4200),
    ] {
        connection
            .execute(
                r#"
                INSERT INTO decoded_frames (
                    frame_id,
                    evidence_id,
                    device_type,
                    raw_len,
                    header_len,
                    declared_len,
                    payload_hex,
                    payload_crc_hex,
                    header_crc_valid,
                    payload_crc_valid,
                    packet_type,
                    packet_type_name,
                    sequence,
                    command_or_event,
                    parsed_payload_json,
                    parser_version,
                    warnings_json
                ) VALUES (
                    ?1,
                    ?2,
                    'Goose',
                    2,
                    0,
                    2,
                    '0000',
                    '',
                    1,
                    1,
                    11,
                    'DATA',
                    ?3,
                    NULL,
                    ?4,
                    'test',
                    '[]'
                )
                "#,
                params![
                    frame_id,
                    evidence_id,
                    sequence,
                    json!({
                        "kind": "data_packet",
                        "packet_k": 11,
                        "domain": "raw_stream_counted",
                        "body_summary": {
                            "kind": "raw_stream_counted",
                            "field_7": hidden_counter,
                            "sample_count": 24
                        },
                        "warnings": []
                    })
                    .to_string()
                ],
            )
            .unwrap();
    }
    drop(connection);
    drop(store);

    let manifest_path = tempdir.path().join("hidden-step-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "hidden-step-validation",
            "cases": [
                {
                    "id": "hidden-step-validation",
                    "report": "step-validation",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:05:00Z",
                    "capture_kind": "100_counted_steps",
                    "manual_step_delta": 100,
                    "official_whoop_step_delta": 97,
                    "step_delta_tolerance": 5,
                    "label_provenance": {
                        "source": "manual_plus_official_app",
                        "official_labels_are_labels": true
                    },
                    "max_candidate_fields": 20
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let case = &report["cases"][0];
    assert_eq!(case["result"]["explicit_step_counter_found"], false);
    assert_eq!(case["result"]["monotonic_counter_candidate_count"], 1);
    assert_eq!(case["result"]["counter_delta_candidate_count"], 1);
    assert_eq!(case["result"]["matching_counter_delta_count"], 1);
    assert_eq!(
        case["result"]["counter_deltas"][0]["match_kind"],
        "monotonic_counter_candidate"
    );
    assert_eq!(case["result"]["counter_deltas"][0]["delta"], 100);
    assert!(
        case["result"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "matching_counter_delta_requires_parser_mapping")
    );

    let metric_records = report["metric_records"].as_array().unwrap();
    let record = metric_record(metric_records, "hidden-step-validation", "steps");
    assert_eq!(record["source_kind"], "unavailable");
    assert!(record["local_value"].is_null());
    assert_eq!(record["manual_label_value"], 100);
    assert_eq!(record["official_label_value"], 97);
    assert_eq!(record["promotion_status"], "parser_mapping_required");
    assert_eq!(
        record["input_counts"]["monotonic_counter_candidate_count"],
        1
    );
    assert_eq!(record["input_counts"]["matching_counter_delta_count"], 1);
    assert_eq!(record["input_counts"]["selected_counter_delta_rank"], 1);
    assert!(
        record["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag == "selected_counter_delta_json_path:$.body_summary.field_7")
    );
    assert!(
        record["quality_flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|flag| flag
                == "selected_counter_delta_reason:hidden_counter_matches_labels_requires_parser_mapping")
    );
    assert!(
        record["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "matching_counter_delta_requires_parser_mapping")
    );
}

#[test]
fn local_health_validation_suite_reports_rhr_rollup_without_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    let frames = [
        (
            "rhr-rollup-history-1",
            "2026-06-02T04:00:00Z",
            historical_k18_frame_hex(58),
        ),
        (
            "rhr-rollup-history-2",
            "2026-06-02T04:10:00Z",
            historical_k18_frame_hex(100),
        ),
    ]
    .into_iter()
    .map(|(id, captured_at, frame_hex)| CapturedFrameInput {
        evidence_id: id.to_string(),
        frame_id: Some(format!("{id}.frame.0")),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex,
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    })
    .collect::<Vec<_>>();
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);
    drop(store);

    let manifest_path = tempdir.path().join("rhr-rollup-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "rhr-rollup-smoke",
            "cases": [
                {
                    "id": "overnight-rhr-rollup",
                    "report": "rhr-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-02T08:00:00Z",
                    "min_owned_captures": 1,
                    "require_trusted_evidence": true,
                    "min_sample_count": 2,
                    "write_metric": true
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["case_count"], 1);
    assert_eq!(report["passing_case_count"], 1);
    let case = &report["cases"][0];
    assert_eq!(case["method"], "metrics.resting_hr_daily_rollup");
    assert_eq!(
        case["result"]["schema"],
        "goose.resting-heart-rate-daily-rollup-report.v1"
    );
    assert_eq!(case["result"]["resting_hr_bpm"], 58.0);
    assert_eq!(case["result"]["sample_count"], 2);
    assert_eq!(case["result"]["daily_metric_written"], true);
    assert_eq!(case["readiness"]["official_label_status"], "not_required");
    assert_eq!(case["readiness"]["manual_label_status"], "not_required");
    assert_eq!(case["readiness"]["evidence_status"], "ready");
    assert_eq!(case["readiness"]["acceptance_ready"], true);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], false);
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );

    let metric_records = report["metric_records"].as_array().unwrap();
    let record = metric_record(metric_records, "overnight-rhr-rollup", "resting_hr");
    assert_eq!(record["metric_family"], "recovery");
    assert_eq!(record["source_kind"], "device_sensor");
    assert_eq!(record["local_value"], 58.0);
    assert_eq!(record["promotion_status"], "daily_rollup");
    assert_eq!(record["input_counts"]["sample_count"], 2);
    assert_eq!(record["input_counts"]["motion_sample_count"], 0);
    assert_eq!(
        record["input_counts"]["selected_heart_rate_sample_count"],
        2
    );
    assert_eq!(
        record["input_counts"]["unmatched_heart_rate_sample_count"],
        2
    );

    let store = GooseStore::open(&db).unwrap();
    let recovery_metric = store
        .daily_recovery_metric(
            case["result"]["daily_metric_id"]
                .as_str()
                .expect("daily metric id"),
        )
        .unwrap()
        .expect("persisted RHR daily recovery metric");
    assert_eq!(recovery_metric.source_kind, "device_sensor");
    assert_eq!(recovery_metric.resting_hr_bpm, Some(58.0));
}

#[test]
fn local_health_validation_suite_reports_energy_rollup_without_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db).unwrap();
    let frames = [
        (
            "energy-rollup-history-1",
            "2026-06-02T10:00:00Z",
            historical_k18_frame_hex(90),
        ),
        (
            "energy-rollup-motion",
            "2026-06-02T10:05:00Z",
            k10_motion_frame_hex_with_value(1000),
        ),
        (
            "energy-rollup-history-2",
            "2026-06-02T10:10:00Z",
            historical_k18_frame_hex(120),
        ),
    ]
    .into_iter()
    .map(|(id, captured_at, frame_hex)| CapturedFrameInput {
        evidence_id: id.to_string(),
        frame_id: Some(format!("{id}.frame.0")),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: captured_at.to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex,
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    })
    .collect::<Vec<_>>();
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
        },
    )
    .unwrap();
    assert!(import_report.pass, "{:?}", import_report.issues);
    drop(store);

    let manifest_path = tempdir.path().join("energy-rollup-validation.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "energy-rollup-smoke",
            "profile_weight_kg": 80.0,
            "profile_age_years": 35,
            "profile_sex": "unknown",
            "resting_hr_bpm": 60.0,
            "max_hr_bpm": 180.0,
            "min_owned_captures": 1,
            "cases": [
                {
                    "id": "walk-energy-rollup",
                    "report": "energy-rollup",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T10:00:00Z",
                    "end": "2026-06-02T10:15:00Z",
                    "capture_kind": "walk",
                    "min_heart_rate_samples": 2,
                    "require_trusted_evidence": true,
                    "write_metric": true
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .output()
            .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], true);
    assert_eq!(report["case_count"], 1);
    assert_eq!(report["passing_case_count"], 1);
    let case = &report["cases"][0];
    assert_eq!(case["method"], "metrics.energy_daily_rollup");
    assert_eq!(
        case["result"]["schema"],
        "goose.energy-daily-rollup-report.v1"
    );
    assert_eq!(case["result"]["active_kcal"], 17.9);
    assert_eq!(case["result"]["resting_kcal"], 12.2);
    assert_eq!(case["result"]["total_kcal"], 30.1);
    assert_eq!(case["result"]["confidence"], 0.77);
    assert_eq!(case["result"]["heart_rate_sample_count"], 3);
    assert_eq!(case["result"]["motion_sample_count"], 1);
    assert_eq!(case["result"]["daily_metric_written"], true);
    assert_eq!(case["readiness"]["official_label_status"], "not_required");
    assert_eq!(case["readiness"]["manual_label_status"], "not_required");
    assert_eq!(case["readiness"]["evidence_status"], "ready");
    assert_eq!(case["readiness"]["acceptance_ready"], true);
    assert_eq!(case["readiness"]["capture_acceptance_ready"], false);
    assert_eq!(
        report["readiness_summary"]["capture_acceptance_ready_case_count"],
        0
    );

    let metric_records = report["metric_records"].as_array().unwrap();
    assert_eq!(metric_records.len(), 3);
    for (metric_name, expected_value) in [
        ("active_kcal", 17.9),
        ("resting_kcal", 12.2),
        ("total_kcal", 30.1),
    ] {
        let record = metric_record(metric_records, "walk-energy-rollup", metric_name);
        assert_eq!(record["metric_family"], "activity");
        assert_eq!(record["source_kind"], "local_estimate");
        assert_eq!(record["local_value"], expected_value);
        assert_eq!(record["promotion_status"], "daily_rollup");
        assert_eq!(record["input_counts"]["heart_rate_sample_count"], 3);
        assert_eq!(record["input_counts"]["motion_sample_count"], 1);
        assert_eq!(record["input_counts"]["step_metric_count"], 0);
        assert_eq!(record["confidence"], 0.77);
    }

    let store = GooseStore::open(&db).unwrap();
    let activity_metric = store
        .daily_activity_metric(
            case["result"]["daily_metric_id"]
                .as_str()
                .expect("daily metric id"),
        )
        .unwrap()
        .expect("persisted energy daily activity metric");
    assert_eq!(activity_metric.source_kind, "local_estimate");
    assert_eq!(activity_metric.active_kcal, Some(17.9));
    assert_eq!(activity_metric.resting_kcal, Some(12.2));
    assert_eq!(activity_metric.total_kcal, Some(30.1));
    assert_eq!(activity_metric.confidence, 0.77);
}

#[test]
fn local_health_validation_example_manifest_covers_controlled_step_matrix() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let review_output_path = tempdir.path().join("example-manifest-review.json");
    // The manifest lives under the repository `docs/` directory, which is two
    // `parent()` hops up from `Rust/core`. The example manifest is an optional
    // documentation artifact that is not always vendored, so skip the coverage
    // assertions when it is absent rather than hard-failing.
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/local-health-validation-manifest.example.json");
    if !manifest_path.exists() {
        eprintln!(
            "skipping local_health_validation_example_manifest_covers_controlled_step_matrix: {} not present",
            manifest_path.display()
        );
        return;
    }

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--review-output")
            .arg(&review_output_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let review: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review_output_path).unwrap()).unwrap();
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["schema_valid"], true);
    assert_eq!(review["label_policy_valid"], true);
    assert_eq!(review["official_label_missing_case_count"], 0);
    assert_eq!(review["manual_label_missing_case_count"], 0);
    assert_eq!(review["capture_session_binding_required_case_count"], 17);
    assert!(
        review["capture_session_binding_required_cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|case| case["case_id"] == "walk-100-steps"
                && case["normalized_report"] == "step-validation")
    );
    assert!(
        review["capture_session_binding_required_cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|case| case["case_id"] == "overnight-temperature"
                && case["normalized_report"] == "temperature-validation")
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "capture_session_binding_required")
    );
    let runbook_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let runbook = local_health_validation_manifest_runbook_markdown(&runbook_manifest);
    assert!(runbook.contains("## Validation Labels"));
    assert!(runbook.contains("No validation-label gaps were detected."));
    assert!(runbook.contains("## Capture Session Binding"));
    assert!(runbook.contains(
        "These cases must be bound to the owned capture session before they count as acceptance evidence."
    ));
    assert!(runbook.contains("| walk-100-steps | step-validation | step-validation | -- | K10, K11, K21 | Add `capture_session_id` or `capture_session_ids` |"));
    assert!(runbook.contains("| overnight-temperature | temperature-validation | temperature-validation | -- | K18, K24, EVENT | Add `capture_session_id` or `capture_session_ids` |"));
    assert_eq!(
        report["schema"],
        "goose.local-health-validation-suite-report.v1"
    );
    assert_eq!(
        report["manifest_id"],
        "local-health-capture-validation-template"
    );
    assert_eq!(report["case_count"], 27);
    assert_eq!(report["ok_case_count"], 27);
    assert_eq!(report["passing_case_count"], 3);
    assert_eq!(report["failing_case_count"], 24);
    assert_eq!(report["metric_records"].as_array().unwrap().len(), 44);
    assert_eq!(report["readiness_summary"]["case_count"], 27);
    assert_eq!(
        report["readiness_summary"]["acceptance_ready_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["missing_packet_evidence_case_count"],
        24
    );
    assert_eq!(
        report["readiness_summary"]["missing_or_invalid_official_label_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["manual_label_missing_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["unavailable_status_case_count"],
        3
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_declared_case_count"],
        0
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_required_case_count"],
        17
    );
    assert_eq!(
        report["readiness_summary"]["capture_session_missing_evidence_case_count"],
        0
    );

    let case_ids = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    for expected_id in [
        "still-desk-step-validation",
        "still-desk-raw-motion-steps",
        "hand-motion-step-validation",
        "hand-motion-raw-motion-steps",
        "walk-step-discovery",
        "walk-100-steps",
        "walk-raw-motion-steps",
        "walk-5-minute-step-validation",
        "walk-5-minute-raw-motion-steps",
        "stairs-uneven-step-validation",
        "stairs-uneven-raw-motion-steps",
        "walk-step-unavailable-status",
        "walk-energy-rollup",
        "walk-energy-unavailable-status",
        "overnight-rhr-rollup",
        "overnight-oxygen-saturation",
        "overnight-temperature",
        "overnight-recovery-unavailable-status",
        "overnight-recovery-sensor-rollup",
    ] {
        assert!(
            case_ids.contains(expected_id),
            "example manifest missing {expected_id}"
        );
    }

    let step_discovery_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "walk-step-discovery")
        .unwrap();
    assert_eq!(
        step_discovery_case["method"],
        "metrics.step_packet_discovery"
    );
    assert_eq!(
        step_discovery_case["result"]["schema"],
        "goose.step-packet-discovery-report.v1"
    );
    assert_eq!(
        step_discovery_case["readiness"]["official_label_status"],
        "not_required"
    );
    assert_eq!(
        step_discovery_case["readiness"]["manual_label_status"],
        "not_required"
    );
    let step_discovery_record = metric_record(
        report["metric_records"].as_array().unwrap(),
        "walk-step-discovery",
        "step_counter_presence",
    );
    assert_eq!(step_discovery_record["source_kind"], "unavailable");
    assert_eq!(
        step_discovery_record["promotion_status"],
        "no_decoded_step_counter"
    );

    let activity_unavailable_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "walk-step-unavailable-status")
        .unwrap();
    assert_eq!(
        activity_unavailable_case["method"],
        "metrics.activity_unavailable_daily_status"
    );
    assert_eq!(activity_unavailable_case["pass"], true);
    assert_eq!(
        activity_unavailable_case["result"]["schema"],
        "goose.activity-unavailable-daily-status-report.v1"
    );
    assert_eq!(
        activity_unavailable_case["result"]["unavailable_metric_count"],
        1
    );
    assert_eq!(
        activity_unavailable_case["result"]["written_metric_count"],
        1
    );
    assert_eq!(
        activity_unavailable_case["readiness"]["evidence_status"],
        "unavailable_status_recorded"
    );

    let energy_unavailable_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "walk-energy-unavailable-status")
        .unwrap();
    assert_eq!(
        energy_unavailable_case["method"],
        "metrics.energy_unavailable_daily_status"
    );
    assert_eq!(energy_unavailable_case["pass"], true);
    assert_eq!(
        energy_unavailable_case["result"]["schema"],
        "goose.energy-unavailable-daily-status-report.v1"
    );
    assert_eq!(
        energy_unavailable_case["result"]["unavailable_metric_count"],
        3
    );
    assert_eq!(energy_unavailable_case["result"]["written_metric_count"], 3);
    assert_eq!(
        energy_unavailable_case["readiness"]["evidence_status"],
        "unavailable_status_recorded"
    );

    let energy_rollup_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "walk-energy-rollup")
        .unwrap();
    assert_eq!(energy_rollup_case["method"], "metrics.energy_daily_rollup");
    assert_eq!(energy_rollup_case["pass"], false);
    assert_eq!(
        energy_rollup_case["result"]["schema"],
        "goose.energy-daily-rollup-report.v1"
    );
    assert_eq!(
        energy_rollup_case["readiness"]["official_label_status"],
        "not_required"
    );
    assert_eq!(
        energy_rollup_case["readiness"]["manual_label_status"],
        "not_required"
    );
    let energy_rollup_record = metric_record(
        report["metric_records"].as_array().unwrap(),
        "walk-energy-rollup",
        "total_kcal",
    );
    assert_eq!(energy_rollup_record["source_kind"], "unavailable");
    assert_eq!(energy_rollup_record["promotion_status"], "daily_rollup");
    assert_eq!(
        energy_rollup_record["input_counts"]["heart_rate_sample_count"],
        0
    );

    let rhr_rollup_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "overnight-rhr-rollup")
        .unwrap();
    assert_eq!(rhr_rollup_case["method"], "metrics.resting_hr_daily_rollup");
    assert_eq!(
        rhr_rollup_case["result"]["schema"],
        "goose.resting-heart-rate-daily-rollup-report.v1"
    );
    assert_eq!(
        rhr_rollup_case["readiness"]["official_label_status"],
        "not_required"
    );
    let rhr_rollup_record = metric_record(
        report["metric_records"].as_array().unwrap(),
        "overnight-rhr-rollup",
        "resting_hr",
    );
    assert_eq!(rhr_rollup_record["source_kind"], "unavailable");
    assert_eq!(rhr_rollup_record["promotion_status"], "daily_rollup");
    assert_eq!(rhr_rollup_record["input_counts"]["sample_count"], 0);

    let unavailable_case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "overnight-recovery-unavailable-status")
        .unwrap();
    assert_eq!(
        unavailable_case["method"],
        "metrics.recovery_unavailable_daily_status"
    );
    assert_eq!(unavailable_case["pass"], true);
    assert_eq!(
        unavailable_case["result"]["schema"],
        "goose.recovery-unavailable-daily-status-report.v1"
    );
    assert_eq!(unavailable_case["result"]["unavailable_metric_count"], 4);
    assert_eq!(unavailable_case["result"]["written_metric_count"], 4);

    for case in report["cases"].as_array().unwrap() {
        if case["id"].as_str().unwrap().contains("step")
            || case["id"].as_str().unwrap().contains("raw-motion")
        {
            assert_eq!(case["label_policy_valid"], true, "{}", case["id"]);
        }
    }
}

#[test]
fn local_health_validation_suite_rejects_unmarked_official_labels() {
    let tempdir = tempfile::tempdir().unwrap();
    let db = tempdir.path().join("goose.sqlite");
    let manifest_path = tempdir.path().join("bad-label-policy.json");
    let review_output_path = tempdir.path().join("bad-label-policy-review.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "cases": [
                {
                    "id": "bad-energy-label",
                    "report": "energy-validation",
                    "date_key": "2026-06-02",
                    "timezone": "Europe/London",
                    "start": "2026-06-02T00:00:00Z",
                    "end": "2026-06-03T00:00:00Z",
                    "official_whoop_total_kcal": 2100.0,
                    "label_provenance": {
                        "source": "official_app_manual_entry"
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_goose-local-health-validation-suite"))
            .arg("--database")
            .arg(&db)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--review-output")
            .arg(&review_output_path)
            .output()
            .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let review: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review_output_path).unwrap()).unwrap();
    assert_eq!(review["status"], "operator_edits_required");
    assert_eq!(review["official_label_policy_required"], true);
    assert_eq!(review["label_policy_valid"], false);
    assert_eq!(review["official_label_case_count"], 1);
    assert_eq!(review["official_label_policy_missing_case_count"], 1);
    assert_eq!(
        review["official_label_policy_missing_cases"][0]["case_id"],
        "bad-energy-label"
    );
    assert!(
        review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker == "official_label_policy_missing_or_false")
    );
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "bad-energy-label:official_label_policy_not_marked")
    );
    assert_eq!(report["cases"][0]["label_policy_valid"], false);
    assert_eq!(
        report["readiness_summary"]["missing_or_invalid_official_label_case_count"],
        1
    );
    assert_eq!(
        report["cases"][0]["readiness"]["official_label_status"],
        "official_label_policy_invalid"
    );
    assert_eq!(
        report["cases"][0]["readiness"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|missing| missing == "official_label_policy"),
        true
    );
}

fn metric_record<'a>(
    metric_records: &'a [serde_json::Value],
    case_id: &str,
    metric_name: &str,
) -> &'a serde_json::Value {
    metric_records
        .iter()
        .find(|record| record["case_id"] == case_id && record["metric_name"] == metric_name)
        .unwrap_or_else(|| panic!("missing metric record {case_id}:{metric_name}"))
}

fn seed_goose_database(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let store = GooseStore::open(path).unwrap();
    assert!(store.schema_version().unwrap() > 0);
}

fn write_raw_export_manifest(bundle_dir: &Path, database_path: &Path) -> String {
    let sqlite_bytes = fs::read(database_path).unwrap();
    let sqlite_sha256 = sha256_hex(&sqlite_bytes);
    fs::write(
        bundle_dir.join("manifest.json"),
        raw_export_manifest_bytes(&sqlite_sha256),
    )
    .unwrap();
    sqlite_sha256
}

fn write_steps_unavailable_manifest(path: &Path) {
    write_steps_unavailable_manifest_for_day(path, "2026-06-02");
}

fn write_steps_unavailable_manifest_for_day(path: &Path, date_key: &str) {
    let end_date = match date_key {
        "2026-06-04" => "2026-06-05",
        _ => "2026-06-03",
    };
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema": "goose.local-health-validation-manifest.v1",
            "manifest_id": "raw-export-bundle-validation-smoke",
            "cases": [
                {
                    "id": "bundle-step-unavailable-status",
                    "report": "steps-unavailable-status",
                    "date_key": date_key,
                    "timezone": "Europe/London",
                    "start": format!("{date_key}T00:00:00Z"),
                    "end": format!("{end_date}T00:00:00Z"),
                    "write_metric": true
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn zip_goose_database(zip_path: &Path, database_path: &Path, archive_path: &str) -> String {
    let zip_file = File::create(zip_path).unwrap();
    let mut writer = ZipWriter::new(zip_file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let bytes = fs::read(database_path).unwrap();
    let sqlite_sha256 = sha256_hex(&bytes);
    let prefix = archive_path.strip_suffix("data/goose.sqlite").unwrap_or("");
    writer
        .start_file(format!("{prefix}manifest.json"), options)
        .unwrap();
    writer
        .write_all(&raw_export_manifest_bytes(&sqlite_sha256))
        .unwrap();
    writer.start_file(archive_path, options).unwrap();
    writer.write_all(&bytes).unwrap();
    writer.finish().unwrap();
    sqlite_sha256
}

fn raw_export_manifest_bytes(sqlite_sha256: &str) -> Vec<u8> {
    raw_export_manifest_bytes_with_options(sqlite_sha256, json!(["sqlite"]), true)
}

fn raw_export_manifest_bytes_with_options(
    sqlite_sha256: &str,
    data_families: serde_json::Value,
    official_labels_are_labels: bool,
) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schema_version": "goose.export.v1",
        "app_version": "test",
        "core_version": "test",
        "time_window": {
            "start": "2026-06-02T00:00:00Z",
            "end": "2026-06-03T00:00:00Z"
        },
        "data_families": data_families,
        "files": [
            {
                "path": "data/goose.sqlite",
                "sha256": sqlite_sha256,
                "kind": "sqlite"
            }
        ],
        "official_labels_are_labels": official_labels_are_labels
    }))
    .unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn seed_processed_capture_sqlite(path: &Path, frame_hex: &str) {
    seed_processed_capture_sqlite_frames(path, &[("2026-06-02T10:00:30+00:00", frame_hex)]);
}

fn seed_processed_capture_sqlite_frames(path: &Path, frames: &[(&str, &str)]) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE records (
                id INTEGER PRIMARY KEY,
                line_no INTEGER NOT NULL,
                ts TEXT NOT NULL,
                role TEXT,
                value_hex TEXT
            );
            CREATE TABLE packets (
                id INTEGER PRIMARY KEY,
                record_id INTEGER NOT NULL,
                decode_index INTEGER NOT NULL,
                is_frame INTEGER
            );
            "#,
        )
        .unwrap();
    for (index, (timestamp, frame_hex)) in frames.iter().enumerate() {
        let id = i64::try_from(index + 1).unwrap();
        connection
            .execute(
                r#"
                INSERT INTO records (id, line_no, ts, role, value_hex)
                VALUES (?1, ?2, ?3, 'data_from_strap', ?4)
                "#,
                params![id, 41 + id, timestamp, frame_hex],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO packets (id, record_id, decode_index, is_frame)
                VALUES (?1, ?1, 0, 1)
                "#,
                params![id],
            )
            .unwrap();
    }
}

fn k10_motion_step_frame_hex(peak_indices: &[usize]) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 84;
    for offset in [85, 285, 485] {
        for index in peak_indices {
            put_i16(&mut payload, offset + index * 2, 4_000);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn k10_motion_frame_hex_with_value(sample_value: i16) -> String {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    for offset in [85, 285, 485, 688, 888, 1088] {
        for index in 0..100 {
            put_i16(&mut payload, offset + index * 2, sample_value);
        }
    }
    hex::encode(build_v5_payload_frame(&payload))
}

fn historical_k18_frame_hex(marker_value: u8) -> String {
    let mut payload = vec![
        PACKET_TYPE_HISTORICAL_DATA,
        18,
        1,
        0x04,
        0x03,
        0x02,
        0x01,
        0x44,
        0x33,
        0x22,
        0x11,
        0x66,
        0x55,
        0xaa,
        marker_value,
        0xbb,
        0xcc,
        0xdd,
        0xee,
        0xff,
    ];
    payload.resize(24, 0);
    hex::encode(build_v5_payload_frame(&payload))
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    let encoded = value.to_le_bytes();
    bytes[offset] = encoded[0];
    bytes[offset + 1] = encoded[1];
}
