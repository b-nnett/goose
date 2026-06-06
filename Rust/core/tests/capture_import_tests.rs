use std::path::Path;

use goose_core::{
    capture_import::{
        CaptureImportOptions, CaptureSqliteImportOptions, CapturedFrameBatchOptions,
        CapturedFrameInput, import_capture_sqlite, import_captured_frame_batch,
        import_fixture_index,
    },
    fixtures::build_fixture_index,
    protocol::DeviceType,
    store::{CaptureSessionInput, GooseStore},
};
use rusqlite::{Connection, params};

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";

#[test]
fn imports_indexed_frame_fixture_into_sqlite_raw_and_decoded_tables() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();

    let report = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty());
    assert_eq!(report.raw_inserted, 8);
    assert_eq!(report.frames_inserted, 8);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 8);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 8);

    let fixture = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.get_hello_frame")
        .unwrap();
    assert_eq!(fixture.packet_type, Some(35));
    assert_eq!(fixture.packet_type_name.as_deref(), Some("COMMAND"));
    assert_eq!(fixture.parsed_payload_kind.as_deref(), Some("command"));
    assert_eq!(fixture.sequence, Some(1));
    assert_eq!(fixture.command_or_event, Some(145));

    let historical = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.historical_k18_packet")
        .unwrap();
    assert_eq!(historical.packet_type, Some(47));
    assert_eq!(
        historical.packet_type_name.as_deref(),
        Some("HISTORICAL_DATA")
    );
    assert_eq!(
        historical.parsed_payload_kind.as_deref(),
        Some("data_packet")
    );
    assert_eq!(historical.sequence, Some(18));

    let event = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.temperature_event")
        .unwrap();
    assert_eq!(event.packet_type, Some(48));
    assert_eq!(event.packet_type_name.as_deref(), Some("EVENT"));
    assert_eq!(event.parsed_payload_kind.as_deref(), Some("event"));
    assert_eq!(event.command_or_event, Some(17));

    let motion = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.goose.v5.k10_motion_summary_short")
        .unwrap();
    assert_eq!(motion.parsed_payload_kind.as_deref(), Some("data_packet"));
    let decoded_motion = store
        .decoded_frame("synthetic.goose.v5.k10_motion_summary_short.frame.0")
        .unwrap()
        .unwrap();
    let parsed_payload: serde_json::Value =
        serde_json::from_str(&decoded_motion.parsed_payload_json).unwrap();
    assert_eq!(parsed_payload["body_summary"]["kind"], "raw_motion_k10");
    assert!(
        parsed_payload["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning == "accelerometer_x_truncated")
    );

    let sanitized_batch_motion = report
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "synthetic.sanitized.corebluetooth.k10_motion")
        .unwrap();
    assert_eq!(
        sanitized_batch_motion.parsed_payload_kind.as_deref(),
        Some("data_packet")
    );
    assert_eq!(
        sanitized_batch_motion.packet_type_name.as_deref(),
        Some("REALTIME_RAW_DATA")
    );
}

#[test]
fn repeated_import_is_idempotent() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let store = GooseStore::open(&db_path).unwrap();
    let fixture_root = Path::new("fixtures");
    let index = build_fixture_index(fixture_root).unwrap();

    let first = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );
    let second = import_fixture_index(
        &store,
        &index,
        CaptureImportOptions {
            fixture_root,
            database_path: &db_path,
            parser_version: "goose-core/test",
        },
    );

    assert!(first.pass);
    assert!(second.pass);
    assert_eq!(second.raw_inserted, 0);
    assert_eq!(second.raw_existing, 8);
    assert_eq!(second.frames_inserted, 0);
    assert_eq!(second.frames_existing, 8);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 8);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 8);
}

#[test]
fn imports_app_captured_frame_batch_and_returns_timeline_rows() {
    let store = GooseStore::open_in_memory().unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "capture-import-session",
            source: "ios.corebluetooth.notification",
            started_at_unix_ms: 1770000000000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: "{}",
        })
        .unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app-capture-1".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: Some("capture-import-session".to_string()),
        device_type: DeviceType::Goose,
    }];

    let report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.next_actions.is_empty());
    assert_eq!(report.raw_inserted, 1);
    assert_eq!(report.frames_inserted, 1);
    assert_eq!(report.timeline_rows.len(), 1);
    assert_eq!(report.timeline_rows[0].category, "command");
    assert_eq!(
        report.results[0].packet_type_name.as_deref(),
        Some("COMMAND")
    );
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 1);
    let raw = store.raw_evidence("app-capture-1").unwrap().unwrap();
    assert_eq!(
        raw.capture_session_id.as_deref(),
        Some("capture-import-session")
    );
}

#[test]
fn captured_frame_batch_preserves_raw_bytes_when_session_reference_is_broken() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app-capture-missing-session".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: Some("missing-capture-session".to_string()),
        device_type: DeviceType::Goose,
    }];

    let report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.raw_inserted, 1);
    assert_eq!(report.frames_inserted, 1);
    assert!(
        report.results[0].issues.iter().any(|issue| issue.contains(
            "raw evidence inserted without capture_session_id after session-scoped insert failed"
        )),
        "{:?}",
        report.results[0].issues
    );
    let raw = store
        .raw_evidence("app-capture-missing-session")
        .unwrap()
        .unwrap();
    assert_eq!(raw.capture_session_id, None);
    assert!(
        store
            .decoded_frame("app-capture-missing-session.frame.0")
            .unwrap()
            .is_some()
    );
}

#[test]
fn captured_frame_batch_preserves_raw_bytes_when_parse_fails() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app-capture-malformed".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: "00010203".to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];

    let report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert_eq!(report.raw_inserted, 1);
    assert_eq!(report.frames_inserted, 0);
    assert_eq!(report.timeline_rows.len(), 0);
    assert!(!report.results[0].parse_ok);
    assert!(
        report.results[0]
            .issues
            .iter()
            .any(|issue| issue.contains("does not start with 0xaa"))
    );
    assert!(
        report.results[0].next_actions.iter().any(|action| {
            action.reason == "frame_parse_failed"
                && action.action.contains("add this frame as a parser fixture")
        }),
        "{:?}",
        report.results[0].next_actions
    );
    assert!(
        report.next_actions.iter().any(|action| {
            action.scope == "app-capture-malformed" && action.reason == "frame_parse_failed"
        }),
        "{:?}",
        report.next_actions
    );
    let raw = store
        .raw_evidence("app-capture-malformed")
        .unwrap()
        .unwrap();
    assert_eq!(raw.payload_hex, "00010203");
}

#[test]
fn repeated_captured_frame_batch_import_is_idempotent() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app-capture-repeat".to_string(),
        frame_id: Some("app-capture-repeat.frame.known".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];

    let first = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    let second = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(first.pass);
    assert!(second.pass);
    assert!(second.next_actions.is_empty());
    assert_eq!(second.raw_inserted, 0);
    assert_eq!(second.raw_existing, 1);
    assert_eq!(second.frames_inserted, 0);
    assert_eq!(second.frames_existing, 1);
    assert_eq!(second.timeline_rows.len(), 1);
}

#[test]
fn captured_frame_batch_reports_next_actions_for_invalid_hex_and_empty_input() {
    let store = GooseStore::open_in_memory().unwrap();
    let invalid_hex = import_captured_frame_batch(
        &store,
        &[CapturedFrameInput {
            evidence_id: "app-capture-invalid-hex".to_string(),
            frame_id: None,
            source: "ios.corebluetooth.notification".to_string(),
            captured_at: "2026-05-28T12:00:00Z".to_string(),
            device_model: "WHOOP 5.0 Goose".to_string(),
            frame_hex: "not hex".to_string(),
            sensitivity: "user-owned-capture".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        }],
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(!invalid_hex.pass);
    assert!(
        invalid_hex.next_actions.iter().any(|action| {
            action.scope == "app-capture-invalid-hex" && action.reason == "frame_hex_invalid"
        }),
        "{:?}",
        invalid_hex.next_actions
    );

    let empty = import_captured_frame_batch(
        &store,
        &[],
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(!empty.pass);
    assert!(
        empty.next_actions.iter().any(|action| {
            action.scope == "captured_frame_batch" && action.reason == "captured_frame_batch_empty"
        }),
        "{:?}",
        empty.next_actions
    );
}

#[test]
fn imports_processed_capture_sqlite_into_owned_goose_session() {
    let tempdir = tempfile::tempdir().unwrap();
    let source_path = tempdir.path().join("capture.sqlite");
    let db_path = tempdir.path().join("goose.sqlite");
    seed_processed_capture_sqlite(&source_path, &[("2026-05-29T00:50:27.270763+00:00", 3)]);

    let store = GooseStore::open(&db_path).unwrap();
    let first = import_capture_sqlite(
        &store,
        CaptureSqliteImportOptions {
            source_database_path: &source_path,
            target_database_path: &db_path,
            session_id: "capture.sqlite.import.test",
            device_model: "WHOOP 5.0 Goose",
            sensitivity: "user-owned-capture",
            parser_version: "goose-core/test",
        },
    )
    .unwrap();

    assert!(first.pass, "{:?}", first.issues);
    assert!(first.decode_pass);
    assert_eq!(first.source_frame_count, 1);
    assert_eq!(first.raw_inserted, 1);
    assert_eq!(first.frames_inserted, 1);
    assert_eq!(first.parse_failed_count, 0);
    assert!(first.raw_import_completed);
    assert!(first.session_started);
    assert!(first.session_finished);
    assert_eq!(store.table_count("capture_sessions").unwrap(), 1);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 1);

    let session = store
        .capture_session("capture.sqlite.import.test")
        .unwrap()
        .unwrap();
    assert_eq!(session.status, "finished");
    assert_eq!(session.frame_count, 1);
    assert_eq!(session.started_at_unix_ms, 1_780_015_827_270);
    assert_eq!(session.ended_at_unix_ms, Some(1_780_015_827_270));

    let raw = store
        .raw_evidence("capture.sqlite.import.test.line-3.decode-0")
        .unwrap()
        .unwrap();
    assert_eq!(raw.captured_at, "2026-05-29T00:50:27.270763Z");
    assert_eq!(
        raw.capture_session_id.as_deref(),
        Some("capture.sqlite.import.test")
    );

    let second = import_capture_sqlite(
        &store,
        CaptureSqliteImportOptions {
            source_database_path: &source_path,
            target_database_path: &db_path,
            session_id: "capture.sqlite.import.test",
            device_model: "WHOOP 5.0 Goose",
            sensitivity: "user-owned-capture",
            parser_version: "goose-core/test",
        },
    )
    .unwrap();

    assert!(second.pass, "{:?}", second.issues);
    assert_eq!(second.raw_inserted, 0);
    assert_eq!(second.raw_existing, 1);
    assert_eq!(second.frames_inserted, 0);
    assert_eq!(second.frames_existing, 1);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 1);
}

#[test]
fn capture_sqlite_import_preserves_raw_evidence_for_parser_failures() {
    let tempdir = tempfile::tempdir().unwrap();
    let source_path = tempdir.path().join("capture.sqlite");
    let db_path = tempdir.path().join("goose.sqlite");
    seed_processed_capture_sqlite_with_hex(
        &source_path,
        &[("2026-05-29T00:50:27Z", 3, "00010203")],
    );

    let store = GooseStore::open(&db_path).unwrap();
    let report = import_capture_sqlite(
        &store,
        CaptureSqliteImportOptions {
            source_database_path: &source_path,
            target_database_path: &db_path,
            session_id: "capture.sqlite.malformed",
            device_model: "WHOOP 5.0 Goose",
            sensitivity: "user-owned-capture",
            parser_version: "goose-core/test",
        },
    )
    .unwrap();

    assert!(!report.pass);
    assert!(!report.decode_pass);
    assert!(report.raw_import_completed);
    assert_eq!(report.raw_inserted, 1);
    assert_eq!(report.frames_inserted, 0);
    assert_eq!(report.parse_failed_count, 1);
    assert!(
        report.next_actions.iter().any(|action| {
            action.reason == "capture_sqlite_decode_incomplete"
                || action.reason == "frame_parse_failed"
        }),
        "{:?}",
        report.next_actions
    );
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 0);
}

// D-02 regression: upload bridge filters decoded_frames by device_type column, NOT by JOIN
// to capture_sessions. This test verifies that the separation is present at the store layer:
// HR monitor frames are stored with device_type "HR_MONITOR" and Goose frames with "Goose".
// The upload bridge code at bridge.rs:3194 gates on `frame.device_type == "HR_MONITOR"` which
// prevents cross-contamination between device types.
#[test]
fn upload_device_type_filter_hr_frames_are_stored_separate_from_goose_frames() {
    let store = GooseStore::open_in_memory().unwrap();

    // Insert an HR monitor frame (GATT 0x2A37 standard measurement format)
    // flags=0x00: 8-bit HR only; HR=72 bpm
    let hr_frames = vec![CapturedFrameInput {
        evidence_id: "d02-hr-frame".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-06-01T12:00:00Z".to_string(),
        device_model: "HR-Monitor-Model".to_string(),
        frame_hex: "0048".to_string(), // valid HR_MONITOR GATT bytes
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::HrMonitor,
    }];

    // Insert a Goose BLE frame (WHOOP protocol)
    let goose_frames = vec![CapturedFrameInput {
        evidence_id: "d02-goose-frame".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-06-01T12:01:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
    }];

    // Import both batches
    let hr_report = import_captured_frame_batch(
        &store,
        &hr_frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    let goose_report = import_captured_frame_batch(
        &store,
        &goose_frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    // Both imports must succeed
    assert!(hr_report.pass, "HR import failed: {:?}", hr_report.issues);
    assert!(
        goose_report.pass,
        "Goose import failed: {:?}",
        goose_report.issues
    );

    // Verify decoded_frames stores both types and they are distinct
    let all_frames = store
        .decoded_frames_between("2026-06-01T11:00:00Z", "2026-06-01T13:00:00Z")
        .unwrap();
    assert_eq!(all_frames.len(), 2, "expected 2 decoded frames total");

    let hr_decoded = all_frames
        .iter()
        .find(|f| f.evidence_id == "d02-hr-frame")
        .expect("HR frame must be in decoded_frames");
    let goose_decoded = all_frames
        .iter()
        .find(|f| f.evidence_id == "d02-goose-frame")
        .expect("Goose frame must be in decoded_frames");

    // D-02: device_type column is the separation mechanism — no JOIN needed
    assert_eq!(
        hr_decoded.device_type, "HR_MONITOR",
        "HR monitor frame must have device_type HR_MONITOR"
    );
    assert_eq!(
        goose_decoded.device_type, "GOOSE",
        "WHOOP frame must have device_type GOOSE"
    );
    // D-02 confirmed: upload bridge at bridge.rs gates on device_type column value,
    // no JOIN to capture_sessions is needed or present.
}

fn seed_processed_capture_sqlite(path: &Path, rows: &[(&str, i64)]) {
    let rows = rows
        .iter()
        .map(|(timestamp, line_no)| (*timestamp, *line_no, GET_HELLO_FRAME))
        .collect::<Vec<_>>();
    seed_processed_capture_sqlite_with_hex(path, &rows);
}

#[test]
fn batch_import_with_active_device_id_stores_non_null_device_id_in_capture_session() {
    // FIX-01: capture.import_frame_batch should persist active_device_id in capture_sessions.
    // RED test — will fail to compile until Task 2 adds the active_device_id field to
    // CapturedFrameBatchOptions.

    let store = GooseStore::open_in_memory().unwrap();

    // Arrange: create the capture session without an active_device_id (as the Swift side
    // currently does — the fix will wire it up so the Rust side records it).
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "device-id-test-session",
            source: "ios.corebluetooth.notification",
            started_at_unix_ms: 1770000000000,
            device_model: "WHOOP 5.0 HRMonitor",
            active_device_id: None,
            provenance_json: "{}",
        })
        .unwrap();

    let frames = vec![CapturedFrameInput {
        evidence_id: "device-id-test-frame-1".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 HRMonitor".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: Some("device-id-test-session".to_string()),
        device_type: DeviceType::Goose,
    }];

    // Act: import with active_device_id supplied in batch options.
    let report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: Some("test-uuid-1234"),
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);

    // Assert: the session row now carries the device id.
    let session = store
        .capture_session("device-id-test-session")
        .unwrap()
        .unwrap();
    assert_eq!(
        session.active_device_id.as_deref(),
        Some("test-uuid-1234"),
        "capture_sessions.active_device_id must be non-NULL after batch import with device id"
    );
}

#[test]
fn batch_import_without_active_device_id_leaves_session_device_id_null() {
    // FIX-01 backward-compatibility path: when active_device_id is None in options,
    // the import still succeeds and capture_sessions.active_device_id stays NULL.

    let store = GooseStore::open_in_memory().unwrap();

    store
        .start_capture_session(CaptureSessionInput {
            session_id: "device-id-null-session",
            source: "ios.corebluetooth.notification",
            started_at_unix_ms: 1770000000000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: "{}",
        })
        .unwrap();

    let frames = vec![CapturedFrameInput {
        evidence_id: "device-id-null-frame-1".to_string(),
        frame_id: None,
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-28T12:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: GET_HELLO_FRAME.to_string(),
        sensitivity: "user-owned-capture".to_string(),
        capture_session_id: Some("device-id-null-session".to_string()),
        device_type: DeviceType::Goose,
    }];

    // Act: import WITHOUT active_device_id (backward-compatible path).
    let report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);

    // Assert: active_device_id remains NULL — backward compat preserved.
    let session = store
        .capture_session("device-id-null-session")
        .unwrap()
        .unwrap();
    assert_eq!(
        session.active_device_id, None,
        "capture_sessions.active_device_id must stay NULL when not supplied"
    );
}

fn seed_processed_capture_sqlite_with_hex(path: &Path, rows: &[(&str, i64, &str)]) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE records (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                line_no INTEGER NOT NULL,
                ts TEXT,
                kind TEXT,
                direction TEXT,
                address TEXT,
                role TEXT,
                service_uuid TEXT,
                characteristic_uuid TEXT,
                descriptor_uuid TEXT,
                value_hex TEXT,
                raw_json TEXT NOT NULL
            );
            CREATE TABLE packets (
                id INTEGER PRIMARY KEY,
                record_id INTEGER NOT NULL,
                decode_index INTEGER NOT NULL,
                packet_type TEXT,
                packet_type_id INTEGER,
                command TEXT,
                command_id INTEGER,
                event TEXT,
                event_id INTEGER,
                result TEXT,
                result_id INTEGER,
                sequence INTEGER,
                origin_sequence INTEGER,
                data_packet_revision INTEGER,
                data_packet_domain TEXT,
                raw_stream TEXT,
                request_schema TEXT,
                request_domain TEXT,
                request_operation TEXT,
                request_complete INTEGER,
                request_payload_len INTEGER,
                request_padding_len INTEGER,
                request_padding_is_zero INTEGER,
                response_schema TEXT,
                event_schema TEXT,
                event_domain TEXT,
                payload_hex TEXT,
                payload_len INTEGER,
                is_frame INTEGER,
                frame_complete INTEGER,
                frame_header_crc_valid INTEGER,
                frame_payload_crc32_valid INTEGER,
                decoded_json TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
    for (index, (timestamp, line_no, frame_hex)) in rows.iter().enumerate() {
        let record_id = index as i64 + 1;
        connection
            .execute(
                r#"
                INSERT INTO records (
                    id, file_id, line_no, ts, kind, direction, role, value_hex, raw_json
                ) VALUES (?1, 1, ?2, ?3, 'att', 'notify', 'data_from_strap', ?4, '{}')
                "#,
                params![record_id, line_no, timestamp, frame_hex],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO packets (
                    id, record_id, decode_index, packet_type, packet_type_id, is_frame, decoded_json
                ) VALUES (?1, ?2, 0, 'COMMAND', 35, 1, '{}')
                "#,
                params![record_id, record_id],
            )
            .unwrap();
    }
}
