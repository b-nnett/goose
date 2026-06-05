use std::path::Path;

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use goose_core::{
    activity_sessions::{
        ActivitySessionCorrectionKind, activity_session_correction_plans,
        append_activity_session_correction_history,
    },
    commands::{CommandEvidence, validate_commands},
    metrics::{built_in_algorithm_definitions, built_in_default_algorithm_preferences},
    protocol::{DeviceType, build_v5_payload_frame, parse_frame, parse_frame_hex},
    store::{
        ActivityIntervalInput, ActivityLabelInput, ActivityMetricInput, ActivitySessionInput,
        AlgorithmPreferenceRecord, CURRENT_SCHEMA_VERSION, CalibrationLabelInput,
        CaptureSessionInput, CommandValidationRecord, DailyActivityMetricInput,
        DailyRecoveryMetricInput, DebugCommandRow, DebugEventRow, DebugSessionRow,
        DecodedFrameInput, ExternalSleepSessionInput, ExternalSleepStageInput, GooseStore,
        HourlyActivityMetricInput, MetricDebugFeatureInput, MetricProvenanceInput,
        RawEvidenceInput, StepCounterSampleInput,
    },
};
use serde_json::json;

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";
const GET_HELLO_RESPONSE_FRAME: &str = "aa010c000001e7412409910100000000401adc66";
const COMMAND_SERVICE_UUID: &str = "61080001-0000-1000-8000-00805f9b34fb";
const COMMAND_CHARACTERISTIC_UUID: &str = "61080002-0000-1000-8000-00805f9b34fb";
const COMMAND_WRITE_TYPE: &str = "with_response";

#[test]
fn large_cached_body_hex_is_dropped_small_is_kept() {
    // The cached parsed-payload `body_hex` duplicates `payload_hex`; for large bodies
    // (the high-volume raw-motion stream) it is dropped at insert time to bound storage.
    let store = GooseStore::open_in_memory().unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "s",
            source: "synthetic.fixture",
            started_at_unix_ms: 1770000000000,
            device_model: "WHOOP",
            active_device_id: None,
            provenance_json: "{}",
        })
        .unwrap();

    // Large data packet (>128-byte body) -> body_hex cleared in the cached JSON.
    let mut payload = vec![47u8, 0, 0];
    payload.extend(std::iter::repeat(0xABu8).take(200));
    let big_frame = build_v5_payload_frame(&payload);
    let parsed_big = parse_frame(DeviceType::Goose, &big_frame).unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "big",
            source: "synthetic.fixture",
            captured_at: "2026-05-27T00:00:00Z",
            device_model: "WHOOP",
            payload: &big_frame,
            sensitivity: "public-test-fixture",
            capture_session_id: Some("s"),
        })
        .unwrap();
    store
        .insert_decoded_frame(DecodedFrameInput {
            frame_id: "big-1",
            evidence_id: "big",
            parsed: &parsed_big,
            parser_version: "t",
        })
        .unwrap();

    // Small data packet (real V24 history frame) keeps its body_hex.
    let small_hex = "aa6400a12f18053ffead0148b1216af822805454015c0000000000000000000071ec05d080c5c53cf600b03ec31dd7beece9633f00009dc5f600b03ec31dd7beece9633f2702690206036e0255015002010c020c010000000046000186060000000000005fd78f0e";
    let small = hex::decode(small_hex).unwrap();
    let parsed_small = parse_frame_hex(DeviceType::Gen4, small_hex).unwrap();
    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "small",
            source: "synthetic.fixture",
            captured_at: "2026-05-27T00:01:00Z",
            device_model: "WHOOP",
            payload: &small,
            sensitivity: "public-test-fixture",
            capture_session_id: Some("s"),
        })
        .unwrap();
    store
        .insert_decoded_frame(DecodedFrameInput {
            frame_id: "small-1",
            evidence_id: "small",
            parsed: &parsed_small,
            parser_version: "t",
        })
        .unwrap();

    let rows = store
        .decoded_frames_between("2026-05-27T00:00:00Z", "2026-05-28T00:00:00Z")
        .unwrap();
    let big = rows.iter().find(|r| r.frame_id == "big-1").unwrap();
    let small_row = rows.iter().find(|r| r.frame_id == "small-1").unwrap();
    // Large frame: body_hex cleared in the cache, but payload_hex (source of truth) intact.
    assert!(
        big.parsed_payload_json.contains("\"body_hex\":\"\""),
        "large body_hex must be cleared from the cached JSON"
    );
    assert!(!big.payload_hex.is_empty(), "payload_hex must be preserved");
    // Small frame: body_hex kept (debug timeline still has it).
    assert!(
        !small_row.parsed_payload_json.contains("\"body_hex\":\"\""),
        "small body_hex must be kept"
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn seed_legacy_capture_metric_database(db_path: &Path) {
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = OFF;

        CREATE TABLE goose_schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
        INSERT INTO goose_schema_migrations(version) VALUES (1);
        INSERT INTO goose_schema_migrations(version) VALUES (2);
        INSERT INTO goose_schema_migrations(version) VALUES (3);
        INSERT INTO goose_schema_migrations(version) VALUES (4);
        INSERT INTO goose_schema_migrations(version) VALUES (5);
        INSERT INTO goose_schema_migrations(version) VALUES (6);
        PRAGMA user_version = 6;

        CREATE TABLE raw_evidence (
            evidence_id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            captured_at TEXT NOT NULL,
            device_model TEXT NOT NULL,
            payload_hex TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            sensitivity TEXT NOT NULL
        );

        CREATE TABLE capture_sessions (
            session_id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            started_at_unix_ms INTEGER NOT NULL,
            ended_at_unix_ms INTEGER,
            device_model TEXT NOT NULL,
            active_device_id TEXT,
            status TEXT NOT NULL,
            frame_count INTEGER NOT NULL DEFAULT 0,
            provenance_json TEXT NOT NULL
        );

        CREATE TABLE algorithm_definitions (
            algorithm_id TEXT NOT NULL,
            version TEXT NOT NULL,
            metric_family TEXT NOT NULL,
            input_schema TEXT NOT NULL,
            output_schema TEXT NOT NULL,
            params_json TEXT NOT NULL,
            PRIMARY KEY (algorithm_id, version)
        );

        CREATE TABLE algorithm_runs (
            run_id TEXT PRIMARY KEY,
            algorithm_id TEXT NOT NULL,
            version TEXT NOT NULL,
            start_time TEXT NOT NULL,
            end_time TEXT NOT NULL,
            output_json TEXT NOT NULL,
            quality_flags_json TEXT NOT NULL,
            provenance_json TEXT NOT NULL,
            FOREIGN KEY (algorithm_id, version)
                REFERENCES algorithm_definitions(algorithm_id, version)
        );

        CREATE TABLE metric_values (
            metric_value_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL REFERENCES algorithm_runs(run_id) ON DELETE CASCADE,
            metric_family TEXT NOT NULL,
            name TEXT NOT NULL,
            value REAL NOT NULL,
            unit TEXT NOT NULL,
            start_time TEXT NOT NULL,
            end_time TEXT NOT NULL
        );

        CREATE TABLE metric_components (
            metric_component_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL REFERENCES algorithm_runs(run_id) ON DELETE CASCADE,
            component_name TEXT NOT NULL,
            value REAL NOT NULL,
            unit TEXT NOT NULL,
            contribution_json TEXT NOT NULL DEFAULT '{}'
        );
        "#,
    )
    .unwrap();

    let payload = hex::decode(GET_HELLO_FRAME).unwrap();
    let checksum = sha256_hex(&payload);

    conn.execute(
        r#"
        INSERT INTO raw_evidence (
            evidence_id,
            source,
            captured_at,
            device_model,
            payload_hex,
            sha256,
            sensitivity
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            "legacy.raw.1",
            "legacy.capture.source",
            "2026-05-27T12:00:00Z",
            "WHOOP 5.0 Goose",
            GET_HELLO_FRAME,
            checksum,
            "public-test-fixture",
        ],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO capture_sessions (
            session_id,
            source,
            started_at_unix_ms,
            ended_at_unix_ms,
            device_model,
            active_device_id,
            status,
            frame_count,
            provenance_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            "legacy.capture.session.1",
            "legacy.capture.source",
            1_770_000_000_000_i64,
            1_770_000_120_000_i64,
            "WHOOP 5.0 Goose",
            Option::<&str>::None,
            "finished",
            1_i64,
            r#"{"source":"legacy_fixture"}"#,
        ],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO algorithm_definitions (
            algorithm_id,
            version,
            metric_family,
            input_schema,
            output_schema,
            params_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            "legacy.metric.algorithm",
            "1.0.0",
            "recovery",
            "{}",
            "{}",
            "{}",
        ],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO algorithm_runs (
            run_id,
            algorithm_id,
            version,
            start_time,
            end_time,
            output_json,
            quality_flags_json,
            provenance_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            "legacy.metric.run.1",
            "legacy.metric.algorithm",
            "1.0.0",
            "2026-05-27T12:00:00Z",
            "2026-05-27T12:10:00Z",
            "{}",
            "[]",
            r#"{"source":"legacy_fixture"}"#,
        ],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO metric_values (
            metric_value_id,
            run_id,
            metric_family,
            name,
            value,
            unit,
            start_time,
            end_time
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            "legacy.metric.value.1",
            "legacy.metric.run.1",
            "recovery",
            "recovery_score",
            82.5_f64,
            "score_0_to_100",
            "2026-05-27T12:00:00Z",
            "2026-05-27T12:10:00Z",
        ],
    )
    .unwrap();

    conn.execute(
        r#"
        INSERT INTO metric_components (
            metric_component_id,
            run_id,
            component_name,
            value,
            unit,
            contribution_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            "legacy.metric.component.1",
            "legacy.metric.run.1",
            "readiness",
            0.82_f64,
            "fraction",
            "{}",
        ],
    )
    .unwrap();
}

#[test]
fn migrates_fresh_database_to_current_schema() {
    let store = GooseStore::open_in_memory().unwrap();

    assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(store.table_count("raw_evidence").unwrap(), 0);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 0);
    assert_eq!(store.table_count("capture_sessions").unwrap(), 0);
    assert_eq!(store.table_count("activity_sessions").unwrap(), 0);
    assert_eq!(store.table_count("activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("hourly_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("daily_recovery_metrics").unwrap(), 0);
    assert_eq!(store.table_count("metric_provenance").unwrap(), 0);
    assert_eq!(store.table_count("metric_debug_features").unwrap(), 0);
    assert_eq!(store.table_count("step_counter_samples").unwrap(), 0);
    assert_eq!(store.table_count("activity_intervals").unwrap(), 0);
    assert_eq!(store.table_count("activity_labels").unwrap(), 0);
    assert_eq!(store.table_count("external_sleep_sessions").unwrap(), 0);
    assert_eq!(store.table_count("external_sleep_stages").unwrap(), 0);
}

#[test]
fn migrates_legacy_capture_metric_database_and_keeps_activity_tables_empty() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    seed_legacy_capture_metric_database(&db_path);

    let store = GooseStore::open(&db_path).unwrap();

    assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(
        store.table_count("goose_schema_migrations").unwrap(),
        CURRENT_SCHEMA_VERSION
    );
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("capture_sessions").unwrap(), 1);
    assert_eq!(store.table_count("algorithm_definitions").unwrap(), 1);
    assert_eq!(store.table_count("algorithm_runs").unwrap(), 1);
    assert_eq!(store.table_count("metric_values").unwrap(), 1);
    assert_eq!(store.table_count("metric_components").unwrap(), 1);
    assert_eq!(store.table_count("activity_sessions").unwrap(), 0);
    assert_eq!(store.table_count("activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("hourly_activity_metrics").unwrap(), 0);
    assert_eq!(store.table_count("daily_recovery_metrics").unwrap(), 0);
    assert_eq!(store.table_count("metric_provenance").unwrap(), 0);
    assert_eq!(store.table_count("metric_debug_features").unwrap(), 0);
    assert_eq!(store.table_count("step_counter_samples").unwrap(), 0);
    assert_eq!(store.table_count("activity_intervals").unwrap(), 0);
    assert_eq!(store.table_count("activity_labels").unwrap(), 0);
    assert_eq!(store.table_count("external_sleep_sessions").unwrap(), 0);
    assert_eq!(store.table_count("external_sleep_stages").unwrap(), 0);
    assert!(
        store
            .table_columns("raw_evidence")
            .unwrap()
            .contains("capture_session_id")
    );
    assert!(
        store
            .table_columns("algorithm_definitions")
            .unwrap()
            .contains("display_name")
    );

    let raw = store.raw_evidence("legacy.raw.1").unwrap().unwrap();
    assert_eq!(raw.payload_hex, GET_HELLO_FRAME);
    assert_eq!(raw.capture_session_id, None);
    assert_eq!(raw.sensitivity, "public-test-fixture");

    let capture_session = store
        .capture_session("legacy.capture.session.1")
        .unwrap()
        .unwrap();
    assert_eq!(capture_session.source, "legacy.capture.source");
    assert_eq!(capture_session.status, "finished");
    assert_eq!(capture_session.frame_count, 1);

    let metric_values = store.metric_values_for_run("legacy.metric.run.1").unwrap();
    assert_eq!(metric_values.len(), 1);
    assert_eq!(metric_values[0].name, "recovery_score");
    assert_eq!(metric_values[0].value, 82.5);

    let metric_components = store
        .metric_components_for_run("legacy.metric.run.1")
        .unwrap();
    assert_eq!(metric_components.len(), 1);
    assert_eq!(metric_components[0].component_name, "readiness");
    assert_eq!(metric_components[0].unit, "fraction");
}

#[test]
fn stores_raw_evidence_and_decoded_frame_with_provenance_link() {
    let store = GooseStore::open_in_memory().unwrap();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap();
    let parsed = parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME).unwrap();
    store
        .start_capture_session(CaptureSessionInput {
            session_id: "capture-test-session",
            source: "synthetic.fixture",
            started_at_unix_ms: 1770000000000,
            device_model: "WHOOP 5.0 Goose",
            active_device_id: None,
            provenance_json: "{}",
        })
        .unwrap();

    let inserted = store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "synthetic-frame-1",
            source: "synthetic.fixture",
            captured_at: "2026-05-27T00:00:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &raw,
            sensitivity: "public-test-fixture",
            capture_session_id: Some("capture-test-session"),
        })
        .unwrap();
    assert!(inserted);

    let frame_inserted = store
        .insert_decoded_frame(DecodedFrameInput {
            frame_id: "frame-1",
            evidence_id: "synthetic-frame-1",
            parsed: &parsed,
            parser_version: "goose-core/0.1.0",
        })
        .unwrap();
    assert!(frame_inserted);

    let row = store.raw_evidence("synthetic-frame-1").unwrap().unwrap();
    assert_eq!(row.payload_hex, GET_HELLO_FRAME);
    assert_eq!(row.sha256.len(), 64);
    assert_eq!(
        row.capture_session_id.as_deref(),
        Some("capture-test-session")
    );
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 1);

    let decoded = store
        .decoded_frames_between("2026-05-27T00:00:00Z", "2026-05-28T00:00:00Z")
        .unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].packet_type_name.as_deref(), Some("COMMAND"));
    assert!(decoded[0].parsed_payload_json.contains("GET_HELLO"));
}

#[test]
fn raw_evidence_insert_is_idempotent_for_same_checksum() {
    let store = GooseStore::open_in_memory().unwrap();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap();
    let input = RawEvidenceInput {
        evidence_id: "synthetic-frame-1",
        source: "synthetic.fixture",
        captured_at: "2026-05-27T00:00:00Z",
        device_model: "WHOOP 5.0 Goose",
        payload: &raw,
        sensitivity: "public-test-fixture",
        capture_session_id: None,
    };

    assert!(store.insert_raw_evidence(input.clone()).unwrap());
    assert!(!store.insert_raw_evidence(input).unwrap());
    assert_eq!(store.table_count("raw_evidence").unwrap(), 1);
}

#[test]
fn raw_evidence_rejects_same_id_with_different_payload() {
    let store = GooseStore::open_in_memory().unwrap();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap();

    store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "synthetic-frame-1",
            source: "synthetic.fixture",
            captured_at: "2026-05-27T00:00:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: &raw,
            sensitivity: "public-test-fixture",
            capture_session_id: None,
        })
        .unwrap();

    let error = store
        .insert_raw_evidence(RawEvidenceInput {
            evidence_id: "synthetic-frame-1",
            source: "synthetic.fixture",
            captured_at: "2026-05-27T00:00:00Z",
            device_model: "WHOOP 5.0 Goose",
            payload: b"different",
            sensitivity: "public-test-fixture",
            capture_session_id: None,
        })
        .unwrap_err();

    assert!(error.to_string().contains("different checksum"));
}

#[test]
fn raw_evidence_payload_compaction_keeps_decoded_rows() {
    let store = GooseStore::open_in_memory().unwrap();
    let raw = hex::decode(GET_HELLO_FRAME).unwrap();
    let parsed = parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME).unwrap();
    let payload_bytes = raw.len() as i64;

    for (evidence_id, captured_at) in [
        ("synthetic-frame-old", "2026-05-27T00:00:00Z"),
        ("synthetic-frame-new", "2026-05-27T00:00:01Z"),
    ] {
        store
            .insert_raw_evidence(RawEvidenceInput {
                evidence_id,
                source: "synthetic.fixture",
                captured_at,
                device_model: "WHOOP 5.0 Goose",
                payload: &raw,
                sensitivity: "public-test-fixture",
                capture_session_id: None,
            })
            .unwrap();
        store
            .insert_decoded_frame(DecodedFrameInput {
                frame_id: &format!("{evidence_id}.frame.0"),
                evidence_id,
                parsed: &parsed,
                parser_version: "goose-core/0.1.0",
            })
            .unwrap();
    }

    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "retention-daily-activity",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1_779_842_400_000,
                end_time_unix_ms: 1_779_928_800_000,
                steps: Some(4_200),
                active_kcal: Some(345.0),
                resting_kcal: Some(1_620.0),
                total_kcal: Some(1_965.0),
                average_cadence_spm: Some(88.0),
                source_kind: "local_estimate",
                confidence: 0.74,
                inputs_json: r#"{"source":"retention_regression"}"#,
                quality_flags_json: r#"["formatted_metric_should_survive_raw_compaction"]"#,
                provenance_json: r#"{"algorithm":"retention.activity.v0"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_hourly_activity_metric(HourlyActivityMetricInput {
                hourly_metric_id: "retention-hourly-activity",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1_779_842_400_000,
                end_time_unix_ms: 1_779_846_000_000,
                steps: Some(420),
                active_kcal: Some(34.5),
                resting_kcal: Some(67.5),
                total_kcal: Some(102.0),
                average_cadence_spm: Some(84.0),
                source_kind: "local_estimate",
                confidence: 0.70,
                inputs_json: r#"{"source":"retention_regression"}"#,
                quality_flags_json: r#"["formatted_metric_should_survive_raw_compaction"]"#,
                provenance_json: r#"{"algorithm":"retention.hourly_activity.v0"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "retention-daily-recovery",
                date_key: "2026-05-27",
                timezone: "Europe/London",
                start_time_unix_ms: 1_779_842_400_000,
                end_time_unix_ms: 1_779_928_800_000,
                resting_hr_bpm: Some(54.0),
                hrv_rmssd_ms: None,
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.88,
                inputs_json: r#"{"source":"retention_regression"}"#,
                quality_flags_json: r#"["formatted_metric_should_survive_raw_compaction"]"#,
                provenance_json: r#"{"algorithm":"retention.recovery.v0"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "retention-activity-provenance",
                metric_scope: "daily_activity",
                metric_id: "retention-daily-activity",
                source_kind: "local_estimate",
                source_detail: "retention regression activity metric",
                confidence: Some(0.74),
                inputs_json: "{}",
                quality_flags_json: "[]",
                provenance_json: r#"{"source":"retention_regression"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "retention-hourly-activity-provenance",
                metric_scope: "hourly_activity",
                metric_id: "retention-hourly-activity",
                source_kind: "local_estimate",
                source_detail: "retention regression hourly activity metric",
                confidence: Some(0.70),
                inputs_json: "{}",
                quality_flags_json: "[]",
                provenance_json: r#"{"source":"retention_regression"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_metric_provenance(MetricProvenanceInput {
                provenance_id: "retention-recovery-provenance",
                metric_scope: "daily_recovery",
                metric_id: "retention-daily-recovery",
                source_kind: "device_sensor",
                source_detail: "retention regression recovery metric",
                confidence: Some(0.88),
                inputs_json: "{}",
                quality_flags_json: "[]",
                provenance_json: r#"{"source":"retention_regression"}"#,
            })
            .unwrap()
    );

    assert_eq!(
        store.raw_evidence_payload_bytes().unwrap(),
        payload_bytes * 2
    );

    let report = store
        .compact_raw_evidence_payloads_to_limit(payload_bytes)
        .unwrap();

    assert_eq!(report.before_bytes, payload_bytes * 2);
    assert_eq!(report.after_bytes, payload_bytes);
    assert_eq!(report.compacted_rows, 1);
    assert_eq!(report.freed_bytes, payload_bytes);
    assert_eq!(
        store
            .raw_evidence("synthetic-frame-old")
            .unwrap()
            .unwrap()
            .payload_hex,
        ""
    );
    assert_eq!(
        store
            .raw_evidence("synthetic-frame-new")
            .unwrap()
            .unwrap()
            .payload_hex,
        GET_HELLO_FRAME
    );

    let decoded = store
        .decoded_frames_between("2026-05-27T00:00:00Z", "2026-05-28T00:00:00Z")
        .unwrap();
    assert_eq!(decoded.len(), 2);
    let activity = store
        .daily_activity_metric("retention-daily-activity")
        .unwrap()
        .unwrap();
    assert_eq!(activity.steps, Some(4_200));
    assert_eq!(activity.source_kind, "local_estimate");
    let hourly_activity = store
        .hourly_activity_metric("retention-hourly-activity")
        .unwrap()
        .unwrap();
    assert_eq!(hourly_activity.steps, Some(420));
    assert_eq!(hourly_activity.source_kind, "local_estimate");
    let recovery = store
        .daily_recovery_metric("retention-daily-recovery")
        .unwrap()
        .unwrap();
    assert_eq!(recovery.resting_hr_bpm, Some(54.0));
    assert_eq!(recovery.source_kind, "device_sensor");
    assert_eq!(
        store
            .metric_provenance_for_metric("daily_activity", "retention-daily-activity")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .metric_provenance_for_metric("hourly_activity", "retention-hourly-activity")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .metric_provenance_for_metric("daily_recovery", "retention-daily-recovery")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.table_count("raw_evidence").unwrap(), 2);
    assert_eq!(store.table_count("decoded_frames").unwrap(), 2);
    assert_eq!(store.table_count("daily_activity_metrics").unwrap(), 1);
    assert_eq!(store.table_count("hourly_activity_metrics").unwrap(), 1);
    assert_eq!(store.table_count("daily_recovery_metrics").unwrap(), 1);
    assert_eq!(store.table_count("metric_provenance").unwrap(), 3);
}

#[test]
fn decoded_frame_requires_existing_raw_evidence() {
    let store = GooseStore::open_in_memory().unwrap();
    let parsed = parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME).unwrap();

    let error = store
        .insert_decoded_frame(DecodedFrameInput {
            frame_id: "frame-1",
            evidence_id: "missing-evidence",
            parsed: &parsed,
            parser_version: "goose-core/0.1.0",
        })
        .unwrap_err();

    assert!(error.to_string().contains("FOREIGN KEY"));
}

#[test]
fn capture_sessions_persist_start_finish_and_window_query() {
    let store = GooseStore::open_in_memory().unwrap();
    let input = CaptureSessionInput {
        session_id: "capture-live-1",
        source: "ios_core_bluetooth.live_notifications",
        started_at_unix_ms: 1770000000000,
        device_model: "WHOOP 5.0 Goose",
        active_device_id: Some("test-device"),
        provenance_json: r#"{"owner":"user","capture_kind":"live_ble_notification"}"#,
    };

    assert!(store.start_capture_session(input.clone()).unwrap());
    assert!(!store.start_capture_session(input).unwrap());

    let active = store.capture_session("capture-live-1").unwrap().unwrap();
    assert_eq!(active.status, "active");
    assert_eq!(active.frame_count, 0);
    assert_eq!(active.active_device_id.as_deref(), Some("test-device"));

    let finished = store
        .finish_capture_session("capture-live-1", 1770000001234, 3)
        .unwrap();
    assert_eq!(finished.status, "finished");
    assert_eq!(finished.ended_at_unix_ms, Some(1770000001234));
    assert_eq!(finished.frame_count, 3);

    let rows = store
        .capture_sessions_between(1769999999999, 1770000002000)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id, "capture-live-1");

    let error = store
        .finish_capture_session("capture-live-1", 1769999999999, 3)
        .unwrap_err();
    assert!(error.to_string().contains("ended_at_unix_ms"));
}

#[test]
fn activity_storage_round_trips_generic_sessions_metrics_intervals_and_labels() {
    let store = GooseStore::open_in_memory().unwrap();
    let session = ActivitySessionInput {
        session_id: "activity-session-1",
        source: "synthetic.activity",
        start_time_unix_ms: 1_770_000_000_000,
        end_time_unix_ms: 1_770_003_600_000,
        activity_type: "running",
        external_activity_type_code: Some("run"),
        external_activity_type_name: Some("Run"),
        custom_label: Some("Morning tempo run"),
        confidence: 0.92,
        detection_method: "heuristic_motion",
        sync_status: "candidate",
        provenance_json: r#"{"capture_session_id":"capture-1","owner":"user","source":"heuristic"}"#,
    };

    assert!(store.insert_activity_session(session.clone()).unwrap());
    assert!(!store.insert_activity_session(session).unwrap());

    let saved = store
        .activity_session("activity-session-1")
        .unwrap()
        .unwrap();
    assert_eq!(saved.activity_type, "running");
    assert_eq!(saved.custom_label.as_deref(), Some("Morning tempo run"));
    assert_eq!(saved.sync_status, "candidate");

    assert_eq!(
        store
            .activity_sessions_by_source("synthetic.activity")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.activity_sessions_by_type("running").unwrap().len(), 1);
    assert_eq!(
        store
            .activity_sessions_by_custom_label("Morning tempo run")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .activity_sessions_by_external_activity_type_code("run")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .activity_sessions_by_external_activity_type_name("Run")
            .unwrap()
            .len(),
        1
    );

    assert!(
        store
            .insert_activity_metric(ActivityMetricInput {
                metric_id: "metric-distance-1",
                activity_session_id: "activity-session-1",
                metric_name: "distance",
                value: 5.42,
                unit: "km",
                start_time_unix_ms: 1_770_000_000_000,
                end_time_unix_ms: 1_770_000_600_000,
                quality_flags_json: r#"["trusted"]"#,
                provenance_json: r#"{"frame_ids":["frame-1"],"source":"decoded_packets"}"#,
            })
            .unwrap()
    );
    assert!(
        store
            .insert_activity_metric(ActivityMetricInput {
                metric_id: "metric-heart-rate-1",
                activity_session_id: "activity-session-1",
                metric_name: "heart_rate",
                value: 154.0,
                unit: "bpm",
                start_time_unix_ms: 1_770_001_200_000,
                end_time_unix_ms: 1_770_001_260_000,
                quality_flags_json: r#"["smoothed","accepted"]"#,
                provenance_json: r#"{"frame_ids":["frame-2"],"source":"decoded_packets"}"#,
            })
            .unwrap()
    );
    assert_eq!(
        store
            .activity_metrics_for_session("activity-session-1")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store.activity_metrics_by_name("heart_rate").unwrap().len(),
        1
    );
    assert_eq!(
        store
            .activity_metrics_for_session_in_window(
                "activity-session-1",
                1_770_001_100_000,
                1_770_001_300_000,
            )
            .unwrap()
            .len(),
        1
    );

    assert!(
        store
            .insert_activity_interval(ActivityIntervalInput {
                interval_id: "interval-lap-1",
                activity_session_id: "activity-session-1",
                interval_type: "lap",
                start_time_unix_ms: 1_770_000_300_000,
                end_time_unix_ms: 1_770_000_360_000,
                sequence: 1,
                metadata_json: r#"{"lap_number":1,"label":"opening segment"}"#,
                provenance_json: r#"{"source":"manual_split"}"#,
            })
            .unwrap()
    );
    assert_eq!(
        store
            .activity_intervals_for_session("activity-session-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .activity_intervals_in_window(1_770_000_200_000, 1_770_000_400_000)
            .unwrap()
            .len(),
        1
    );

    assert!(
        store
            .insert_activity_label(ActivityLabelInput {
                label_id: "label-1",
                activity_session_id: "activity-session-1",
                label_type: "user",
                value: "Morning tempo run",
                source: "manual_entry",
                confidence: Some(1.0),
                provenance_json: r#"{"source":"typed_by_user"}"#,
            })
            .unwrap()
    );
    assert_eq!(
        store
            .activity_labels_for_session("activity-session-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.activity_label("label-1").unwrap().unwrap().value,
        "Morning tempo run"
    );
}

#[test]
fn daily_metric_rollups_round_trip_with_source_kind_provenance() {
    let store = GooseStore::open_in_memory().unwrap();

    let activity = DailyActivityMetricInput {
        daily_metric_id: "daily-activity-2026-06-02-device",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        steps: Some(8_421),
        active_kcal: Some(612.4),
        resting_kcal: Some(1_780.0),
        total_kcal: Some(2_392.4),
        average_cadence_spm: Some(96.2),
        source_kind: "device_counter",
        confidence: 0.91,
        inputs_json: r#"{"packet_families":["K11","K21"],"hr_samples":120}"#,
        quality_flags_json: r#"["counter_delta","hr_motion_gated"]"#,
        provenance_json: r#"{"owner":"user","algorithm":"goose.activity_totals.v0"}"#,
    };
    assert!(
        store
            .insert_daily_activity_metric(activity.clone())
            .unwrap()
    );
    assert!(!store.insert_daily_activity_metric(activity).unwrap());

    let saved_activity = store
        .daily_activity_metric("daily-activity-2026-06-02-device")
        .unwrap()
        .unwrap();
    assert_eq!(saved_activity.steps, Some(8_421));
    assert_eq!(saved_activity.source_kind, "device_counter");
    assert_eq!(
        store
            .daily_activity_metrics_between(1_780_355_000_000, 1_780_442_000_000)
            .unwrap()
            .len(),
        1
    );

    let hourly_activity = HourlyActivityMetricInput {
        hourly_metric_id: "hourly-activity-2026-06-02-10-device",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_387_200_000,
        end_time_unix_ms: 1_780_390_800_000,
        steps: Some(842),
        active_kcal: Some(61.2),
        resting_kcal: Some(74.0),
        total_kcal: Some(135.2),
        average_cadence_spm: Some(94.0),
        source_kind: "device_counter",
        confidence: 0.90,
        inputs_json: r#"{"packet_families":["K11"],"sample_count":3}"#,
        quality_flags_json: r#"["counter_delta"]"#,
        provenance_json: r#"{"owner":"user","algorithm":"goose.activity_hourly.v0"}"#,
    };
    assert!(
        store
            .insert_hourly_activity_metric(hourly_activity.clone())
            .unwrap()
    );
    assert!(
        !store
            .insert_hourly_activity_metric(hourly_activity)
            .unwrap()
    );

    let saved_hourly_activity = store
        .hourly_activity_metric("hourly-activity-2026-06-02-10-device")
        .unwrap()
        .unwrap();
    assert_eq!(saved_hourly_activity.steps, Some(842));
    assert_eq!(saved_hourly_activity.source_kind, "device_counter");
    assert_eq!(
        store
            .hourly_activity_metrics_between(1_780_387_000_000, 1_780_391_000_000)
            .unwrap()
            .len(),
        1
    );

    let recovery = DailyRecoveryMetricInput {
        daily_metric_id: "daily-recovery-2026-06-02-local",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        resting_hr_bpm: Some(51.4),
        hrv_rmssd_ms: Some(68.2),
        respiratory_rate_rpm: None,
        oxygen_saturation_percent: None,
        skin_temperature_delta_c: None,
        source_kind: "local_estimate",
        confidence: 0.72,
        inputs_json: r#"{"hr_windows":42,"rr_interval_chunks":8}"#,
        quality_flags_json: r#"["sleep_window_inferred"]"#,
        provenance_json: r#"{"owner":"user","algorithm":"goose.recovery_daily.v0"}"#,
    };
    assert!(
        store
            .insert_daily_recovery_metric(recovery.clone())
            .unwrap()
    );
    assert!(!store.insert_daily_recovery_metric(recovery).unwrap());

    let saved_recovery = store
        .daily_recovery_metric("daily-recovery-2026-06-02-local")
        .unwrap()
        .unwrap();
    assert_eq!(saved_recovery.resting_hr_bpm, Some(51.4));
    assert_eq!(saved_recovery.source_kind, "local_estimate");
    assert_eq!(
        store
            .daily_recovery_metrics_between(1_780_355_000_000, 1_780_442_000_000)
            .unwrap()
            .len(),
        1
    );

    let provenance = MetricProvenanceInput {
        provenance_id: "prov-daily-activity-2026-06-02-device",
        metric_scope: "daily_activity",
        metric_id: "daily-activity-2026-06-02-device",
        source_kind: "device_counter",
        source_detail: "ICM45686 pedometer counter",
        confidence: Some(0.91),
        inputs_json: r#"{"fields":["steps","cadence","activity"]}"#,
        quality_flags_json: r#"["counter_delta"]"#,
        provenance_json: r#"{"owner":"user","capture_kind":"local_packet_decode"}"#,
    };
    assert!(store.insert_metric_provenance(provenance.clone()).unwrap());
    assert!(!store.insert_metric_provenance(provenance).unwrap());
    assert_eq!(
        store
            .metric_provenance_for_metric("daily_activity", "daily-activity-2026-06-02-device")
            .unwrap()
            .len(),
        1
    );
    let hourly_provenance = MetricProvenanceInput {
        provenance_id: "prov-hourly-activity-2026-06-02-10-device",
        metric_scope: "hourly_activity",
        metric_id: "hourly-activity-2026-06-02-10-device",
        source_kind: "device_counter",
        source_detail: "ICM45686 pedometer counter hourly rollup",
        confidence: Some(0.90),
        inputs_json: r#"{"fields":["steps","cadence"]}"#,
        quality_flags_json: r#"["counter_delta"]"#,
        provenance_json: r#"{"owner":"user","capture_kind":"local_packet_decode"}"#,
    };
    assert!(
        store
            .insert_metric_provenance(hourly_provenance.clone())
            .unwrap()
    );
    assert!(!store.insert_metric_provenance(hourly_provenance).unwrap());
    assert_eq!(
        store
            .metric_provenance_for_metric("hourly_activity", "hourly-activity-2026-06-02-10-device")
            .unwrap()
            .len(),
        1
    );

    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-2026-06-02-unavailable",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1_780_355_200_000,
                end_time_unix_ms: 1_780_441_600_000,
                steps: None,
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: None,
                source_kind: "unavailable",
                confidence: 0.0,
                inputs_json: r#"{"reason":"device_counter_not_decoded"}"#,
                quality_flags_json: r#"["activity_steps_unavailable"]"#,
                provenance_json: r#"{"owner":"user","algorithm":"goose.activity.unavailable_status.v0"}"#,
            })
            .unwrap()
    );
    let confident_unavailable_provenance = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-daily-activity-2026-06-02-unavailable",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-2026-06-02-unavailable",
            source_kind: "unavailable",
            source_detail: "activity steps unavailable status",
            confidence: Some(0.4),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(
        confident_unavailable_provenance
            .to_string()
            .contains("confidence 0.0")
    );

    let invalid = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-invalid-source",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-2026-06-02-device",
            source_kind: "healthkit",
            source_detail: "not allowed",
            confidence: Some(0.5),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(invalid.to_string().contains("source_kind"));

    let mismatched_source = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-daily-activity-source-mismatch",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-2026-06-02-device",
            source_kind: "local_estimate",
            source_detail: "wrong source for this metric",
            confidence: Some(0.5),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(
        mismatched_source
            .to_string()
            .contains("source_kind must match daily_activity")
    );

    let missing_metric = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-daily-activity-missing-metric",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-missing",
            source_kind: "device_counter",
            source_detail: "missing metric",
            confidence: Some(0.5),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(
        missing_metric
            .to_string()
            .contains("must reference existing daily_activity metric")
    );

    let unsupported_scope = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-unsupported-scope",
            metric_scope: "activity_session",
            metric_id: "daily-activity-2026-06-02-device",
            source_kind: "device_counter",
            source_detail: "unsupported scope",
            confidence: Some(0.5),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(unsupported_scope.to_string().contains("metric_scope"));
}

#[test]
fn formatted_metrics_reject_empty_available_rows_and_valued_unavailable_rows() {
    let store = GooseStore::open_in_memory().unwrap();

    let empty_available_activity = store
        .insert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: "daily-activity-empty-local",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: None,
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "local_estimate",
            confidence: 0.5,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        empty_available_activity
            .to_string()
            .contains("must include steps or calorie values")
    );

    let cadence_only_activity = store
        .insert_hourly_activity_metric(HourlyActivityMetricInput {
            hourly_metric_id: "hourly-activity-cadence-only-device",
            date_key: "2026-06-02T10:00",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: None,
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: Some(90.0),
            source_kind: "device_counter",
            confidence: 0.5,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        cadence_only_activity
            .to_string()
            .contains("must include steps or calorie values")
    );

    let valued_unavailable_activity = store
        .insert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: "daily-activity-valued-unavailable",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: Some(1),
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "unavailable",
            confidence: 0.0,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        valued_unavailable_activity
            .to_string()
            .contains("must not carry metric values")
    );

    let confident_unavailable_activity = store
        .insert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: "daily-activity-confident-unavailable",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: None,
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "unavailable",
            confidence: 0.1,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        confident_unavailable_activity
            .to_string()
            .contains("confidence 0.0")
    );

    let empty_available_recovery = store
        .insert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: "daily-recovery-empty-device",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "device_sensor",
            confidence: 0.5,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        empty_available_recovery
            .to_string()
            .contains("must include at least one recovery value")
    );

    let valued_unavailable_recovery = store
        .insert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: "daily-recovery-valued-unavailable",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            resting_hr_bpm: Some(52.0),
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "unavailable",
            confidence: 0.0,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        valued_unavailable_recovery
            .to_string()
            .contains("must not carry metric values")
    );
}

#[test]
fn formatted_metrics_reject_official_whoop_labels_as_metric_sources() {
    let store = GooseStore::open_in_memory().unwrap();

    let label_sourced_activity = store
        .insert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: "daily-activity-whoop-label",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: Some(1234),
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "device_counter",
            confidence: 0.8,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"source":"official_whoop_app"}"#,
        })
        .unwrap_err();
    assert!(
        label_sourced_activity
            .to_string()
            .contains("official WHOOP label markers")
    );

    let label_sourced_hourly = store
        .insert_hourly_activity_metric(HourlyActivityMetricInput {
            hourly_metric_id: "hourly-activity-whoop-label",
            date_key: "2026-06-02T10:00",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: Some(100),
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "device_counter",
            confidence: 0.8,
            inputs_json: r#"{"source":"whoop_backend"}"#,
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        label_sourced_hourly
            .to_string()
            .contains("official WHOOP label markers")
    );

    let label_sourced_recovery = store
        .insert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: "daily-recovery-whoop-label",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            resting_hr_bpm: Some(54.0),
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "device_sensor",
            confidence: 0.8,
            inputs_json: "{}",
            quality_flags_json: r#"["official_whoop_label"]"#,
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        label_sourced_recovery
            .to_string()
            .contains("official WHOOP label markers")
    );

    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-local-for-provenance",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1,
                end_time_unix_ms: 2,
                steps: Some(1234),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: None,
                source_kind: "device_counter",
                confidence: 0.8,
                inputs_json: "{}",
                quality_flags_json: "[]",
                provenance_json: r#"{"source":"local_packet_decode"}"#,
            })
            .unwrap()
    );

    let label_sourced_provenance = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-whoop-label-source",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-local-for-provenance",
            source_kind: "device_counter",
            source_detail: "official_whoop_app",
            confidence: Some(0.8),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        label_sourced_provenance
            .to_string()
            .contains("official WHOOP labels")
    );

    let label_provenance_json = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-whoop-label-json",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-local-for-provenance",
            source_kind: "device_counter",
            source_detail: "local packet decode",
            confidence: Some(0.8),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"official_whoop_label":true}"#,
        })
        .unwrap_err();
    assert!(
        label_provenance_json
            .to_string()
            .contains("official WHOOP label markers")
    );
}

#[test]
fn formatted_metrics_reject_platform_imports_as_metric_sources() {
    let store = GooseStore::open_in_memory().unwrap();

    let platform_sourced_activity = store
        .insert_daily_activity_metric(DailyActivityMetricInput {
            daily_metric_id: "daily-activity-healthkit-steps",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: Some(1234),
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "device_counter",
            confidence: 0.8,
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"source":"healthkit_step_count"}"#,
        })
        .unwrap_err();
    assert!(
        platform_sourced_activity
            .to_string()
            .contains("platform-import markers")
    );

    let platform_sourced_hourly = store
        .insert_hourly_activity_metric(HourlyActivityMetricInput {
            hourly_metric_id: "hourly-activity-health-connect",
            date_key: "2026-06-02T10:00",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            steps: Some(100),
            active_kcal: None,
            resting_kcal: None,
            total_kcal: None,
            average_cadence_spm: None,
            source_kind: "device_counter",
            confidence: 0.8,
            inputs_json: r#"{"source":"health_connect_steps"}"#,
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        platform_sourced_hourly
            .to_string()
            .contains("platform-import markers")
    );

    let platform_sourced_recovery = store
        .insert_daily_recovery_metric(DailyRecoveryMetricInput {
            daily_metric_id: "daily-recovery-platform-import",
            date_key: "2026-06-02",
            timezone: "Europe/London",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            resting_hr_bpm: Some(54.0),
            hrv_rmssd_ms: None,
            respiratory_rate_rpm: None,
            oxygen_saturation_percent: None,
            skin_temperature_delta_c: None,
            source_kind: "device_sensor",
            confidence: 0.8,
            inputs_json: "{}",
            quality_flags_json: r#"["platform_import_not_syncable"]"#,
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        platform_sourced_recovery
            .to_string()
            .contains("platform-import markers")
    );

    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-local-profile-weight",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1,
                end_time_unix_ms: 2,
                steps: None,
                active_kcal: Some(120.0),
                resting_kcal: Some(1500.0),
                total_kcal: Some(1620.0),
                average_cadence_spm: None,
                source_kind: "local_estimate",
                confidence: 0.7,
                inputs_json: r#"{"profile_weight_kg":82.0,"profile_weight_source":"healthkit"}"#,
                quality_flags_json: "[]",
                provenance_json: r#"{"source":"packet_hr_motion","profile_weight_source":"HealthKit body mass profile autofill"}"#,
            })
            .unwrap()
    );

    let platform_sourced_provenance = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-healthkit-source",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-local-profile-weight",
            source_kind: "local_estimate",
            source_detail: "HealthKit step count import",
            confidence: Some(0.7),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        platform_sourced_provenance
            .to_string()
            .contains("platform imports as a formatted metric source")
    );

    let platform_provenance_json = store
        .insert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-health-connect-json",
            metric_scope: "daily_activity",
            metric_id: "daily-activity-local-profile-weight",
            source_kind: "local_estimate",
            source_detail: "packet HR/motion local estimate",
            confidence: Some(0.7),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"platform":"health_connect"}"#,
        })
        .unwrap_err();
    assert!(
        platform_provenance_json
            .to_string()
            .contains("platform-import markers")
    );
}

#[test]
fn migrates_v12_daily_activity_source_unique_table_for_separate_local_metrics() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA user_version = 12;
        CREATE TABLE daily_activity_metrics (
            daily_metric_id TEXT PRIMARY KEY,
            date_key TEXT NOT NULL,
            timezone TEXT NOT NULL,
            start_time_unix_ms INTEGER NOT NULL,
            end_time_unix_ms INTEGER NOT NULL,
            steps INTEGER,
            active_kcal REAL,
            resting_kcal REAL,
            total_kcal REAL,
            average_cadence_spm REAL,
            source_kind TEXT NOT NULL,
            confidence REAL NOT NULL,
            inputs_json TEXT NOT NULL DEFAULT '{}',
            quality_flags_json TEXT NOT NULL DEFAULT '[]',
            provenance_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(date_key, timezone, source_kind)
        );
        INSERT INTO daily_activity_metrics (
            daily_metric_id,
            date_key,
            timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            steps,
            active_kcal,
            resting_kcal,
            total_kcal,
            average_cadence_spm,
            source_kind,
            confidence,
            inputs_json,
            quality_flags_json,
            provenance_json
        ) VALUES (
            'daily-activity-energy-2026-06-02-local',
            '2026-06-02',
            'Europe/London',
            1780355200000,
            1780441600000,
            NULL,
            18.0,
            60.0,
            78.0,
            NULL,
            'local_estimate',
            0.66,
            '{"heart_rate_sample_count":8}',
            '["local_energy_estimate"]',
            '{"algorithm":"goose.energy.local_estimate.v0"}'
        );
        "#,
    )
    .unwrap();
    drop(conn);

    let store = GooseStore::open(&db_path).unwrap();
    assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-steps-2026-06-02-local",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1_780_355_200_000,
                end_time_unix_ms: 1_780_441_600_000,
                steps: Some(4_200),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(96.0),
                source_kind: "local_estimate",
                confidence: 0.55,
                inputs_json: r#"{"raw_motion_frames":120}"#,
                quality_flags_json: r#"["raw_motion_step_estimate"]"#,
                provenance_json: r#"{"algorithm":"goose.steps.raw_motion_estimate.v0"}"#,
            })
            .unwrap()
    );
    let local_rows = store
        .daily_activity_metrics_between(1_780_355_000_000, 1_780_442_000_000)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_kind == "local_estimate")
        .collect::<Vec<_>>();
    assert_eq!(local_rows.len(), 2);
    assert!(local_rows.iter().any(|row| row.active_kcal == Some(18.0)));
    assert!(local_rows.iter().any(|row| row.steps == Some(4_200)));
}

#[test]
fn migrates_v12_daily_recovery_source_unique_table_for_separate_device_metrics() {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("goose.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA user_version = 12;
        CREATE TABLE daily_recovery_metrics (
            daily_metric_id TEXT PRIMARY KEY,
            date_key TEXT NOT NULL,
            timezone TEXT NOT NULL,
            start_time_unix_ms INTEGER NOT NULL,
            end_time_unix_ms INTEGER NOT NULL,
            resting_hr_bpm REAL,
            hrv_rmssd_ms REAL,
            respiratory_rate_rpm REAL,
            oxygen_saturation_percent REAL,
            skin_temperature_delta_c REAL,
            source_kind TEXT NOT NULL,
            confidence REAL NOT NULL,
            inputs_json TEXT NOT NULL DEFAULT '{}',
            quality_flags_json TEXT NOT NULL DEFAULT '[]',
            provenance_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(date_key, timezone, source_kind)
        );
        INSERT INTO daily_recovery_metrics (
            daily_metric_id,
            date_key,
            timezone,
            start_time_unix_ms,
            end_time_unix_ms,
            resting_hr_bpm,
            hrv_rmssd_ms,
            respiratory_rate_rpm,
            oxygen_saturation_percent,
            skin_temperature_delta_c,
            source_kind,
            confidence,
            inputs_json,
            quality_flags_json,
            provenance_json
        ) VALUES (
            'daily-recovery-rhr-2026-06-02-device',
            '2026-06-02',
            'Europe/London',
            1780355200000,
            1780441600000,
            56.0,
            NULL,
            NULL,
            NULL,
            NULL,
            'device_sensor',
            0.82,
            '{"heart_rate_sample_count":8}',
            '["daily_rhr_lowest_quartile_hr"]',
            '{"algorithm":"goose.resting_heart_rate.device_sensor.v0"}'
        );
        "#,
    )
    .unwrap();
    drop(conn);

    let store = GooseStore::open(&db_path).unwrap();
    assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert!(
        store
            .insert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "daily-recovery-hrv-2026-06-02-device",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1_780_355_200_000,
                end_time_unix_ms: 1_780_441_600_000,
                resting_hr_bpm: None,
                hrv_rmssd_ms: Some(68.2),
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.61,
                inputs_json: r#"{"rr_interval_chunks":8}"#,
                quality_flags_json: r#"["rr_interval_scale_unvalidated"]"#,
                provenance_json: r#"{"algorithm":"goose.hrv.device_sensor.v0"}"#,
            })
            .unwrap()
    );
    let device_rows = store
        .daily_recovery_metrics_between(1_780_355_000_000, 1_780_442_000_000)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_kind == "device_sensor")
        .collect::<Vec<_>>();
    assert_eq!(device_rows.len(), 2);
    assert!(
        device_rows
            .iter()
            .any(|row| row.resting_hr_bpm == Some(56.0))
    );
    assert!(device_rows.iter().any(|row| row.hrv_rmssd_ms == Some(68.2)));
}

#[test]
fn daily_recovery_metric_upsert_refreshes_same_day_rollup() {
    let store = GooseStore::open_in_memory().unwrap();

    let first = DailyRecoveryMetricInput {
        daily_metric_id: "daily-recovery-rhr-2026-06-02-device",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        resting_hr_bpm: Some(58.0),
        hrv_rmssd_ms: None,
        respiratory_rate_rpm: None,
        oxygen_saturation_percent: None,
        skin_temperature_delta_c: None,
        source_kind: "device_sensor",
        confidence: 0.74,
        inputs_json: r#"{"heart_rate_sample_count":2}"#,
        quality_flags_json: r#"["daily_rhr_lowest_quartile_hr"]"#,
        provenance_json: r#"{"algorithm":"goose.resting_heart_rate.device_sensor.v0"}"#,
    };
    assert!(store.upsert_daily_recovery_metric(first.clone()).unwrap());
    assert!(!store.upsert_daily_recovery_metric(first).unwrap());

    let refreshed = DailyRecoveryMetricInput {
        daily_metric_id: "daily-recovery-rhr-2026-06-02-device",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        resting_hr_bpm: Some(56.0),
        hrv_rmssd_ms: None,
        respiratory_rate_rpm: None,
        oxygen_saturation_percent: None,
        skin_temperature_delta_c: None,
        source_kind: "device_sensor",
        confidence: 0.82,
        inputs_json: r#"{"heart_rate_sample_count":8}"#,
        quality_flags_json: r#"["daily_rhr_lowest_quartile_hr"]"#,
        provenance_json: r#"{"algorithm":"goose.resting_heart_rate.device_sensor.v0","refreshed":true}"#,
    };
    assert!(store.upsert_daily_recovery_metric(refreshed).unwrap());

    let saved = store
        .daily_recovery_metric("daily-recovery-rhr-2026-06-02-device")
        .unwrap()
        .unwrap();
    assert_eq!(saved.resting_hr_bpm, Some(56.0));
    assert_eq!(saved.confidence, 0.82);
    assert_eq!(saved.inputs_json, r#"{"heart_rate_sample_count":8}"#);

    assert!(
        store
            .insert_daily_recovery_metric(DailyRecoveryMetricInput {
                daily_metric_id: "daily-recovery-hrv-2026-06-02-device",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1_780_355_200_000,
                end_time_unix_ms: 1_780_441_600_000,
                resting_hr_bpm: None,
                hrv_rmssd_ms: Some(68.2),
                respiratory_rate_rpm: None,
                oxygen_saturation_percent: None,
                skin_temperature_delta_c: None,
                source_kind: "device_sensor",
                confidence: 0.61,
                inputs_json: r#"{"rr_interval_chunks":8}"#,
                quality_flags_json: r#"["rr_interval_scale_unvalidated"]"#,
                provenance_json: r#"{"algorithm":"goose.hrv.device_sensor.v0"}"#,
            })
            .unwrap()
    );
    let device_rows = store
        .daily_recovery_metrics_between(1_780_355_000_000, 1_780_442_000_000)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_kind == "device_sensor")
        .collect::<Vec<_>>();
    assert_eq!(device_rows.len(), 2);
    assert!(
        device_rows
            .iter()
            .any(|row| row.resting_hr_bpm == Some(56.0))
    );
    assert!(device_rows.iter().any(|row| row.hrv_rmssd_ms == Some(68.2)));

    let provenance = MetricProvenanceInput {
        provenance_id: "prov-daily-recovery-rhr-2026-06-02-device",
        metric_scope: "daily_recovery",
        metric_id: "daily-recovery-rhr-2026-06-02-device",
        source_kind: "device_sensor",
        source_detail: "WHOOP packet-derived heart-rate samples",
        confidence: Some(0.74),
        inputs_json: r#"{"heart_rate_sample_count":2}"#,
        quality_flags_json: "[]",
        provenance_json: r#"{"algorithm":"goose.resting_heart_rate.device_sensor.v0"}"#,
    };
    assert!(store.upsert_metric_provenance(provenance).unwrap());
    let refreshed_provenance = MetricProvenanceInput {
        provenance_id: "prov-daily-recovery-rhr-2026-06-02-device",
        metric_scope: "daily_recovery",
        metric_id: "daily-recovery-rhr-2026-06-02-device",
        source_kind: "device_sensor",
        source_detail: "WHOOP packet-derived heart-rate samples",
        confidence: Some(0.82),
        inputs_json: r#"{"heart_rate_sample_count":8}"#,
        quality_flags_json: "[]",
        provenance_json: r#"{"algorithm":"goose.resting_heart_rate.device_sensor.v0","refreshed":true}"#,
    };
    assert!(
        store
            .upsert_metric_provenance(refreshed_provenance)
            .unwrap()
    );
    let saved_provenance = store
        .metric_provenance("prov-daily-recovery-rhr-2026-06-02-device")
        .unwrap()
        .unwrap();
    assert_eq!(saved_provenance.confidence, Some(0.82));
    assert_eq!(
        saved_provenance.inputs_json,
        r#"{"heart_rate_sample_count":8}"#
    );

    let mismatched_source = store
        .upsert_metric_provenance(MetricProvenanceInput {
            provenance_id: "prov-daily-recovery-rhr-2026-06-02-mismatch",
            metric_scope: "daily_recovery",
            metric_id: "daily-recovery-rhr-2026-06-02-device",
            source_kind: "local_estimate",
            source_detail: "wrong source for this metric",
            confidence: Some(0.5),
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: r#"{"owner":"user"}"#,
        })
        .unwrap_err();
    assert!(
        mismatched_source
            .to_string()
            .contains("source_kind must match daily_recovery")
    );
}

#[test]
fn daily_activity_metric_upsert_refreshes_same_day_energy_rollup() {
    let store = GooseStore::open_in_memory().unwrap();

    let first = DailyActivityMetricInput {
        daily_metric_id: "daily-activity-energy-2026-06-02-local",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        steps: None,
        active_kcal: Some(12.4),
        resting_kcal: Some(44.0),
        total_kcal: Some(56.4),
        average_cadence_spm: None,
        source_kind: "local_estimate",
        confidence: 0.52,
        inputs_json: r#"{"heart_rate_sample_count":2}"#,
        quality_flags_json: r#"["local_energy_estimate"]"#,
        provenance_json: r#"{"algorithm":"goose.energy.local_estimate.v0"}"#,
    };
    assert!(store.upsert_daily_activity_metric(first.clone()).unwrap());
    assert!(!store.upsert_daily_activity_metric(first).unwrap());

    let refreshed = DailyActivityMetricInput {
        daily_metric_id: "daily-activity-energy-2026-06-02-local",
        date_key: "2026-06-02",
        timezone: "Europe/London",
        start_time_unix_ms: 1_780_355_200_000,
        end_time_unix_ms: 1_780_441_600_000,
        steps: None,
        active_kcal: Some(18.0),
        resting_kcal: Some(60.0),
        total_kcal: Some(78.0),
        average_cadence_spm: None,
        source_kind: "local_estimate",
        confidence: 0.66,
        inputs_json: r#"{"heart_rate_sample_count":8}"#,
        quality_flags_json: r#"["local_energy_estimate"]"#,
        provenance_json: r#"{"algorithm":"goose.energy.local_estimate.v0","refreshed":true}"#,
    };
    assert!(store.upsert_daily_activity_metric(refreshed).unwrap());

    let saved = store
        .daily_activity_metric("daily-activity-energy-2026-06-02-local")
        .unwrap()
        .unwrap();
    assert_eq!(saved.source_kind, "local_estimate");
    assert_eq!(saved.active_kcal, Some(18.0));
    assert_eq!(saved.resting_kcal, Some(60.0));
    assert_eq!(saved.total_kcal, Some(78.0));
    assert_eq!(saved.confidence, 0.66);

    assert!(
        store
            .insert_daily_activity_metric(DailyActivityMetricInput {
                daily_metric_id: "daily-activity-steps-2026-06-02-local",
                date_key: "2026-06-02",
                timezone: "Europe/London",
                start_time_unix_ms: 1_780_355_200_000,
                end_time_unix_ms: 1_780_441_600_000,
                steps: Some(4_200),
                active_kcal: None,
                resting_kcal: None,
                total_kcal: None,
                average_cadence_spm: Some(96.0),
                source_kind: "local_estimate",
                confidence: 0.55,
                inputs_json: r#"{"raw_motion_frames":120}"#,
                quality_flags_json: r#"["raw_motion_step_estimate"]"#,
                provenance_json: r#"{"algorithm":"goose.steps.raw_motion_estimate.v0"}"#,
            })
            .unwrap()
    );
    let local_rows = store
        .daily_activity_metrics_between(1_780_355_000_000, 1_780_442_000_000)
        .unwrap()
        .into_iter()
        .filter(|row| row.source_kind == "local_estimate")
        .collect::<Vec<_>>();
    assert_eq!(local_rows.len(), 2);
    assert!(local_rows.iter().any(|row| row.active_kcal == Some(18.0)));
    assert!(local_rows.iter().any(|row| row.steps == Some(4_200)));
}

#[test]
fn metric_debug_features_round_trip_with_source_kind_and_confidence() {
    let store = GooseStore::open_in_memory().unwrap();

    let feature = MetricDebugFeatureInput {
        feature_id: "debug-step-discovery-2026-06-02-k11",
        metric_family: "steps",
        feature_name: "decoded_step_counter_candidate",
        start_time_unix_ms: 1_780_398_000_000,
        end_time_unix_ms: 1_780_398_300_000,
        source_kind: "device_counter",
        confidence: Some(0.84),
        feature_json: r#"{"json_path":"$.body_summary.step_count","delta":100,"monotonic":true}"#,
        inputs_json: r#"{"packet_families":["K11/raw_stream_counted"],"frame_count":2}"#,
        quality_flags_json: r#"["controlled_capture_label_match"]"#,
        provenance_json: r#"{"owner":"user","report":"goose.step-capture-validation-report.v1"}"#,
    };
    assert!(store.insert_metric_debug_feature(feature.clone()).unwrap());
    assert!(!store.insert_metric_debug_feature(feature).unwrap());

    let saved = store
        .metric_debug_feature("debug-step-discovery-2026-06-02-k11")
        .unwrap()
        .unwrap();
    assert_eq!(saved.metric_family, "steps");
    assert_eq!(saved.feature_name, "decoded_step_counter_candidate");
    assert_eq!(saved.source_kind, "device_counter");
    assert_eq!(saved.confidence, Some(0.84));
    assert!(saved.feature_json.contains("step_count"));
    assert_eq!(
        store
            .metric_debug_features_between("steps", 1_780_397_900_000, 1_780_398_400_000)
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .metric_debug_features_between("hrv", 1_780_397_900_000, 1_780_398_400_000)
            .unwrap()
            .is_empty()
    );

    let invalid_source = store
        .insert_metric_debug_feature(MetricDebugFeatureInput {
            feature_id: "debug-invalid-source",
            metric_family: "steps",
            feature_name: "bad_source",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            source_kind: "healthkit",
            confidence: Some(0.5),
            feature_json: "{}",
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(invalid_source.to_string().contains("source_kind"));

    let invalid_confidence = store
        .insert_metric_debug_feature(MetricDebugFeatureInput {
            feature_id: "debug-invalid-confidence",
            metric_family: "steps",
            feature_name: "bad_confidence",
            start_time_unix_ms: 1,
            end_time_unix_ms: 2,
            source_kind: "local_estimate",
            confidence: Some(1.5),
            feature_json: "{}",
            inputs_json: "{}",
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(invalid_confidence.to_string().contains("confidence"));
}

#[test]
fn step_counter_samples_round_trip_and_reject_non_device_counter_sources() {
    let store = GooseStore::open_in_memory().unwrap();

    let sample = StepCounterSampleInput {
        sample_id: "step-counter-k11-2026-06-02-001",
        sample_time_unix_ms: 1_780_398_000_000,
        counter_value: 4_200,
        cadence_spm: Some(96.5),
        activity_state: Some("walking"),
        source_kind: "device_counter",
        packet_family: "K11/raw_stream_counted",
        json_path: "$.body_summary.step_count",
        frame_id: Some("frame-1"),
        evidence_id: Some("evidence-1"),
        capture_session_id: Some("capture-1"),
        quality_flags_json: "[]",
        provenance_json: r#"{"owner":"user","decode":"step_count"}"#,
    };
    assert!(store.insert_step_counter_sample(sample.clone()).unwrap());
    assert!(!store.insert_step_counter_sample(sample).unwrap());

    let saved = store
        .step_counter_sample("step-counter-k11-2026-06-02-001")
        .unwrap()
        .unwrap();
    assert_eq!(saved.counter_value, 4_200);
    assert_eq!(saved.cadence_spm, Some(96.5));
    assert_eq!(saved.activity_state.as_deref(), Some("walking"));
    assert_eq!(saved.source_kind, "device_counter");
    assert_eq!(saved.packet_family, "K11/raw_stream_counted");
    assert_eq!(saved.json_path, "$.body_summary.step_count");
    assert_eq!(saved.frame_id.as_deref(), Some("frame-1"));
    assert_eq!(
        store
            .step_counter_samples_between(1_780_397_900_000, 1_780_398_400_000)
            .unwrap()
            .len(),
        1
    );

    let invalid_source = store
        .insert_step_counter_sample(StepCounterSampleInput {
            sample_id: "step-counter-healthkit",
            sample_time_unix_ms: 1,
            counter_value: 1,
            cadence_spm: None,
            activity_state: None,
            source_kind: "healthkit",
            packet_family: "HealthKit",
            json_path: "$.steps",
            frame_id: None,
            evidence_id: None,
            capture_session_id: None,
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(invalid_source.to_string().contains("source_kind"));

    let invalid_estimate = store
        .insert_step_counter_sample(StepCounterSampleInput {
            sample_id: "step-counter-local-estimate",
            sample_time_unix_ms: 1,
            counter_value: 1,
            cadence_spm: None,
            activity_state: None,
            source_kind: "local_estimate",
            packet_family: "motion",
            json_path: "$.estimate",
            frame_id: None,
            evidence_id: None,
            capture_session_id: None,
            quality_flags_json: "[]",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(
        invalid_estimate
            .to_string()
            .contains("must be device_counter")
    );
}

#[test]
fn activity_corrections_round_trip_generic_sessions_and_provenance_history() {
    let store = GooseStore::open_in_memory().unwrap();
    let plans = activity_session_correction_plans();

    let change_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::ChangeActivityType)
        .unwrap();
    let trim_start_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::TrimStart)
        .unwrap();
    let trim_end_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::TrimEnd)
        .unwrap();
    let split_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::Split)
        .unwrap();
    let merge_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::Merge)
        .unwrap();
    let false_positive_plan = plans
        .iter()
        .find(|plan| plan.kind == ActivitySessionCorrectionKind::FalsePositive)
        .unwrap();

    let base_provenance = json!({
        "source": "manual_draft",
        "activity_session_id": "activity-session-correction-1",
        "kind": "draft",
    });
    let base_provenance_json = base_provenance.to_string();
    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: "activity-session-correction-1",
                source: "manual_draft",
                start_time_unix_ms: 1_770_000_000_000,
                end_time_unix_ms: 1_770_000_360_000,
                activity_type: "walking",
                external_activity_type_code: Some("walk"),
                external_activity_type_name: Some("Walk"),
                custom_label: Some("Lunch walk"),
                confidence: 0.54,
                detection_method: "manual_annotation",
                sync_status: "candidate",
                provenance_json: &base_provenance_json,
            })
            .unwrap()
    );

    let change_provenance = append_activity_session_correction_history(
        &base_provenance,
        ActivitySessionCorrectionKind::ChangeActivityType,
        json!({
            "activity_session_id": "activity-session-correction-1",
            "activity_type": "cycling",
        }),
    );
    let change_provenance_json = change_provenance.to_string();
    assert!(
        store
            .update_activity_session(ActivitySessionInput {
                session_id: "activity-session-correction-1",
                source: "manual_draft",
                start_time_unix_ms: 1_770_000_000_000,
                end_time_unix_ms: 1_770_000_360_000,
                activity_type: "cycling",
                external_activity_type_code: Some("bike"),
                external_activity_type_name: Some("Cycling"),
                custom_label: Some("Lunch ride"),
                confidence: 0.67,
                detection_method: &change_plan.detection_method,
                sync_status: &change_plan.sync_status,
                provenance_json: &change_provenance_json,
            })
            .unwrap()
    );

    let trim_start_provenance = append_activity_session_correction_history(
        &change_provenance,
        ActivitySessionCorrectionKind::TrimStart,
        json!({
            "activity_session_id": "activity-session-correction-1",
            "start_time_unix_ms": 1_770_000_180_000_i64,
        }),
    );
    let trim_start_provenance_json = trim_start_provenance.to_string();
    assert!(
        store
            .update_activity_session(ActivitySessionInput {
                session_id: "activity-session-correction-1",
                source: "manual_draft",
                start_time_unix_ms: 1_770_000_180_000,
                end_time_unix_ms: 1_770_000_360_000,
                activity_type: "cycling",
                external_activity_type_code: Some("bike"),
                external_activity_type_name: Some("Cycling"),
                custom_label: Some("Lunch ride"),
                confidence: 0.67,
                detection_method: &trim_start_plan.detection_method,
                sync_status: &trim_start_plan.sync_status,
                provenance_json: &trim_start_provenance_json,
            })
            .unwrap()
    );

    let trim_end_provenance = append_activity_session_correction_history(
        &trim_start_provenance,
        ActivitySessionCorrectionKind::TrimEnd,
        json!({
            "activity_session_id": "activity-session-correction-1",
            "end_time_unix_ms": 1_770_000_300_000_i64,
        }),
    );
    let trim_end_provenance_json = trim_end_provenance.to_string();
    assert!(
        store
            .update_activity_session(ActivitySessionInput {
                session_id: "activity-session-correction-1",
                source: "manual_draft",
                start_time_unix_ms: 1_770_000_180_000,
                end_time_unix_ms: 1_770_000_300_000,
                activity_type: "cycling",
                external_activity_type_code: Some("bike"),
                external_activity_type_name: Some("Cycling"),
                custom_label: Some("Lunch ride"),
                confidence: 0.67,
                detection_method: &trim_end_plan.detection_method,
                sync_status: &trim_end_plan.sync_status,
                provenance_json: &trim_end_provenance_json,
            })
            .unwrap()
    );

    let corrected = store
        .activity_session("activity-session-correction-1")
        .unwrap()
        .unwrap();
    assert_eq!(corrected.activity_type, "cycling");
    assert_eq!(corrected.start_time_unix_ms, 1_770_000_180_000);
    assert_eq!(corrected.end_time_unix_ms, 1_770_000_300_000);
    assert_eq!(corrected.duration_ms, 120_000);
    assert_eq!(corrected.detection_method, trim_end_plan.detection_method);
    assert_eq!(corrected.sync_status, trim_end_plan.sync_status);
    let corrected_provenance: serde_json::Value =
        serde_json::from_str(&corrected.provenance_json).unwrap();
    let correction_history = corrected_provenance["correction_history"]
        .as_array()
        .unwrap();
    assert_eq!(correction_history.len(), 3);
    assert_eq!(correction_history[0]["kind"], "change_activity_type");
    assert_eq!(correction_history[1]["kind"], "trim_start");
    assert_eq!(correction_history[2]["kind"], "trim_end");
    assert_eq!(corrected_provenance["manually_corrected"], true);

    let false_positive_base = json!({
        "source": "manual_correction",
        "activity_session_id": "activity-session-false-positive-1",
        "kind": "draft",
    });
    let false_positive_base_json = false_positive_base.to_string();
    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: "activity-session-false-positive-1",
                source: "manual_correction",
                start_time_unix_ms: 1_770_001_200_000,
                end_time_unix_ms: 1_770_001_260_000,
                activity_type: "other",
                external_activity_type_code: None,
                external_activity_type_name: None,
                custom_label: Some("Bad capture"),
                confidence: 0.18,
                detection_method: "manual_annotation",
                sync_status: "candidate",
                provenance_json: &false_positive_base_json,
            })
            .unwrap()
    );
    let false_positive_provenance = append_activity_session_correction_history(
        &false_positive_base,
        ActivitySessionCorrectionKind::FalsePositive,
        json!({
            "activity_session_id": "activity-session-false-positive-1",
            "reason": "duplicate capture",
        }),
    );
    let false_positive_provenance_json = false_positive_provenance.to_string();
    assert!(
        store
            .update_activity_session(ActivitySessionInput {
                session_id: "activity-session-false-positive-1",
                source: "manual_correction",
                start_time_unix_ms: 1_770_001_200_000,
                end_time_unix_ms: 1_770_001_260_000,
                activity_type: "unknown",
                external_activity_type_code: None,
                external_activity_type_name: None,
                custom_label: None,
                confidence: 0.0,
                detection_method: &false_positive_plan.detection_method,
                sync_status: &false_positive_plan.sync_status,
                provenance_json: &false_positive_provenance_json,
            })
            .unwrap()
    );

    let split_provenance = append_activity_session_correction_history(
        &json!({
            "source": "manual_correction",
            "activity_session_id": "activity-session-split-1",
            "kind": "draft",
        }),
        ActivitySessionCorrectionKind::Split,
        json!({
            "activity_session_id": "activity-session-split-1",
            "source_session_ids": ["activity-session-split-source"],
        }),
    );
    let split_provenance_json = split_provenance.to_string();
    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: "activity-session-split-1",
                source: "manual_correction",
                start_time_unix_ms: 1_770_002_400_000,
                end_time_unix_ms: 1_770_002_760_000,
                activity_type: "walking",
                external_activity_type_code: None,
                external_activity_type_name: None,
                custom_label: Some("Walk segment A"),
                confidence: 0.82,
                detection_method: &split_plan.detection_method,
                sync_status: &split_plan.sync_status,
                provenance_json: &split_provenance_json,
            })
            .unwrap()
    );

    let merge_provenance = append_activity_session_correction_history(
        &json!({
            "source": "manual_correction",
            "activity_session_id": "activity-session-merge-1",
            "kind": "draft",
        }),
        ActivitySessionCorrectionKind::Merge,
        json!({
            "activity_session_id": "activity-session-merge-1",
            "source_session_ids": [
                "activity-session-merge-left",
                "activity-session-merge-right"
            ],
        }),
    );
    let merge_provenance_json = merge_provenance.to_string();
    assert!(
        store
            .insert_activity_session(ActivitySessionInput {
                session_id: "activity-session-merge-1",
                source: "manual_correction",
                start_time_unix_ms: 1_770_003_000_000,
                end_time_unix_ms: 1_770_003_420_000,
                activity_type: "walking",
                external_activity_type_code: None,
                external_activity_type_name: None,
                custom_label: Some("Walk segment merge"),
                confidence: 0.88,
                detection_method: &merge_plan.detection_method,
                sync_status: &merge_plan.sync_status,
                provenance_json: &merge_provenance_json,
            })
            .unwrap()
    );

    assert_eq!(store.activity_sessions_by_type("cycling").unwrap().len(), 1);
    assert_eq!(store.activity_sessions_by_type("walking").unwrap().len(), 2);
    assert_eq!(
        store
            .activity_sessions_by_source("manual_correction")
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        store
            .activity_sessions_by_sync_status("discarded")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn activity_storage_rejects_inverted_windows_and_orphans() {
    let store = GooseStore::open_in_memory().unwrap();

    let inverted_session = store
        .insert_activity_session(ActivitySessionInput {
            session_id: "activity-session-2",
            source: "synthetic.activity",
            start_time_unix_ms: 1_770_003_600_000,
            end_time_unix_ms: 1_770_000_000_000,
            activity_type: "running",
            external_activity_type_code: Some("run"),
            external_activity_type_name: Some("Run"),
            custom_label: None,
            confidence: 0.5,
            detection_method: "heuristic_motion",
            sync_status: "candidate",
            provenance_json: r#"{"source":"heuristic"}"#,
        })
        .unwrap_err();
    assert!(
        inverted_session
            .to_string()
            .contains("greater than start_time_unix_ms")
    );

    store
        .insert_activity_session(ActivitySessionInput {
            session_id: "activity-session-2",
            source: "synthetic.activity",
            start_time_unix_ms: 1_770_000_000_000,
            end_time_unix_ms: 1_770_003_600_000,
            activity_type: "running",
            external_activity_type_code: Some("run"),
            external_activity_type_name: Some("Run"),
            custom_label: None,
            confidence: 0.5,
            detection_method: "heuristic_motion",
            sync_status: "candidate",
            provenance_json: r#"{"source":"heuristic"}"#,
        })
        .unwrap();

    let inverted_metric = store
        .insert_activity_metric(ActivityMetricInput {
            metric_id: "metric-window-error",
            activity_session_id: "activity-session-2",
            metric_name: "heart_rate",
            value: 150.0,
            unit: "bpm",
            start_time_unix_ms: 1_770_001_200_000,
            end_time_unix_ms: 1_770_001_100_000,
            quality_flags_json: "[]",
            provenance_json: r#"{"source":"decoded_packets"}"#,
        })
        .unwrap_err();
    assert!(
        inverted_metric
            .to_string()
            .contains("greater than start_time_unix_ms")
    );

    let orphan_metric = store
        .insert_activity_metric(ActivityMetricInput {
            metric_id: "metric-orphan",
            activity_session_id: "missing-session",
            metric_name: "heart_rate",
            value: 150.0,
            unit: "bpm",
            start_time_unix_ms: 1_770_001_100_000,
            end_time_unix_ms: 1_770_001_160_000,
            quality_flags_json: "[]",
            provenance_json: r#"{"source":"decoded_packets"}"#,
        })
        .unwrap_err();
    assert!(orphan_metric.to_string().contains("not found"));

    let inverted_interval = store
        .insert_activity_interval(ActivityIntervalInput {
            interval_id: "interval-window-error",
            activity_session_id: "activity-session-2",
            interval_type: "pause",
            start_time_unix_ms: 1_770_001_200_000,
            end_time_unix_ms: 1_770_001_100_000,
            sequence: 1,
            metadata_json: "{}",
            provenance_json: r#"{"source":"decoded_packets"}"#,
        })
        .unwrap_err();
    assert!(
        inverted_interval
            .to_string()
            .contains("greater than start_time_unix_ms")
    );

    let orphan_interval = store
        .insert_activity_interval(ActivityIntervalInput {
            interval_id: "interval-orphan",
            activity_session_id: "missing-session",
            interval_type: "pause",
            start_time_unix_ms: 1_770_001_100_000,
            end_time_unix_ms: 1_770_001_160_000,
            sequence: 1,
            metadata_json: "{}",
            provenance_json: r#"{"source":"decoded_packets"}"#,
        })
        .unwrap_err();
    assert!(orphan_interval.to_string().contains("not found"));

    let orphan_label = store
        .insert_activity_label(ActivityLabelInput {
            label_id: "label-orphan",
            activity_session_id: "missing-session",
            label_type: "candidate",
            value: "Possible run",
            source: "heuristic",
            confidence: Some(0.4),
            provenance_json: r#"{"source":"heuristic"}"#,
        })
        .unwrap_err();
    assert!(orphan_label.to_string().contains("not found"));
}

#[test]
fn algorithm_preferences_select_primary_algorithms_by_scope_and_family() {
    let store = GooseStore::open_in_memory().unwrap();
    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    for preference in built_in_default_algorithm_preferences() {
        store.set_algorithm_preference(&preference).unwrap();
    }

    let recovery = store
        .algorithm_preference("global", "recovery")
        .unwrap()
        .unwrap();
    assert_eq!(recovery.algorithm_id, "goose.recovery.v0");
    assert_eq!(recovery.version, "0.1.0");

    let preferences = store.algorithm_preferences(Some("global")).unwrap();
    assert_eq!(preferences.len(), 5);
    assert_eq!(preferences[0].metric_family, "hrv");
    assert_eq!(preferences[1].metric_family, "recovery");
    assert_eq!(preferences[2].metric_family, "sleep");

    let override_preference = AlgorithmPreferenceRecord {
        scope: "debug-comparison".to_string(),
        metric_family: "sleep".to_string(),
        algorithm_id: "goose.sleep.v0".to_string(),
        version: "0.1.0".to_string(),
    };
    store
        .set_algorithm_preference(&override_preference)
        .unwrap();
    assert_eq!(
        store
            .algorithm_preferences(None)
            .unwrap()
            .iter()
            .filter(|preference| preference.metric_family == "sleep")
            .count(),
        2
    );
}

#[test]
fn algorithm_preference_rejects_missing_or_wrong_family_algorithm() {
    let store = GooseStore::open_in_memory().unwrap();
    let missing = store
        .set_algorithm_preference(&AlgorithmPreferenceRecord {
            scope: "global".to_string(),
            metric_family: "sleep".to_string(),
            algorithm_id: "goose.sleep.v0".to_string(),
            version: "0.1.0".to_string(),
        })
        .unwrap_err();
    assert!(missing.to_string().contains("must exist"));

    for definition in built_in_algorithm_definitions() {
        store.upsert_algorithm_definition(&definition).unwrap();
    }
    let wrong_family = store
        .set_algorithm_preference(&AlgorithmPreferenceRecord {
            scope: "global".to_string(),
            metric_family: "recovery".to_string(),
            algorithm_id: "goose.sleep.v0".to_string(),
            version: "0.1.0".to_string(),
        })
        .unwrap_err();
    assert!(
        wrong_family
            .to_string()
            .contains("belongs to metric family sleep")
    );
}

#[test]
fn external_sleep_history_round_trips_platform_sessions_and_stages() {
    let store = GooseStore::open_in_memory().unwrap();
    let sleep = ExternalSleepSessionInput {
        sleep_id: "external-sleep-1",
        source: "healthkit.sleep_analysis",
        platform: "healthkit",
        platform_record_id: Some("hk-sleep-record-1"),
        start_time_unix_ms: 1_770_000_000_000,
        end_time_unix_ms: 1_770_028_800_000,
        timezone: Some("Europe/London"),
        stage_summary_json: r#"{"minutes_by_stage":{"asleep":420.0,"awake":60.0,"unknown":0.0,"not_applicable":0.0}}"#,
        confidence: 0.86,
        provenance_json: r#"{"owner":"user","source":"healthkit","sample_type":"sleepAnalysis"}"#,
    };

    assert!(store.insert_external_sleep_session(sleep.clone()).unwrap());
    assert!(!store.insert_external_sleep_session(sleep).unwrap());

    let saved = store
        .external_sleep_session("external-sleep-1")
        .unwrap()
        .unwrap();
    assert_eq!(saved.platform, "healthkit");
    assert_eq!(
        saved.platform_record_id.as_deref(),
        Some("hk-sleep-record-1")
    );
    assert_eq!(saved.duration_ms, 28_800_000);
    assert_eq!(saved.timezone.as_deref(), Some("Europe/London"));
    assert_eq!(saved.confidence, 0.86);

    assert_eq!(
        store
            .external_sleep_sessions_between(1_769_999_000_000, 1_770_029_000_000)
            .unwrap()
            .len(),
        1
    );

    let stage = ExternalSleepStageInput {
        stage_id: "external-sleep-stage-1",
        sleep_id: "external-sleep-1",
        stage_kind: "deep",
        start_time_unix_ms: 1_770_003_600_000,
        end_time_unix_ms: 1_770_007_200_000,
        confidence: 0.80,
        provenance_json: r#"{"owner":"user","source":"healthkit","value":"asleep_deep"}"#,
    };
    assert!(store.insert_external_sleep_stage(stage.clone()).unwrap());
    assert!(!store.insert_external_sleep_stage(stage).unwrap());

    let stages = store
        .external_sleep_stages_for_session("external-sleep-1")
        .unwrap();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].stage_kind, "deep");
    assert_eq!(stages[0].duration_ms, 3_600_000);
}

#[test]
fn external_sleep_history_rejects_invalid_platform_and_orphan_stages() {
    let store = GooseStore::open_in_memory().unwrap();
    let error = store
        .insert_external_sleep_session(ExternalSleepSessionInput {
            sleep_id: "external-sleep-bad-platform",
            source: "healthkit.sleep_analysis",
            platform: "private_api",
            platform_record_id: Some("record-1"),
            start_time_unix_ms: 1_770_000_000_000,
            end_time_unix_ms: 1_770_028_800_000,
            timezone: None,
            stage_summary_json: "{}",
            confidence: 0.86,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(error.to_string().contains("platform must be one of"));

    let error = store
        .insert_external_sleep_session(ExternalSleepSessionInput {
            sleep_id: "external-sleep-bad-stage-summary",
            source: "healthkit.sleep_analysis",
            platform: "healthkit",
            platform_record_id: Some("record-bad-stage-summary"),
            start_time_unix_ms: 1_770_000_000_000,
            end_time_unix_ms: 1_770_028_800_000,
            timezone: None,
            stage_summary_json: r#"{"asleep":420.0,"awake":60.0}"#,
            confidence: 0.86,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("stage_summary_json must contain minutes_by_stage object")
    );

    let error = store
        .insert_external_sleep_session(ExternalSleepSessionInput {
            sleep_id: "external-sleep-bad-stage-minutes",
            source: "healthkit.sleep_analysis",
            platform: "healthkit",
            platform_record_id: Some("record-bad-stage-minutes"),
            start_time_unix_ms: 1_770_000_000_000,
            end_time_unix_ms: 1_770_028_800_000,
            timezone: None,
            stage_summary_json: r#"{"minutes_by_stage":{"asleep":-1.0}}"#,
            confidence: 0.86,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must be finite and non-negative")
    );

    let error = store
        .insert_external_sleep_session(ExternalSleepSessionInput {
            sleep_id: "external-sleep-unknown-stage-summary",
            source: "healthkit.sleep_analysis",
            platform: "healthkit",
            platform_record_id: Some("record-unknown-stage-summary"),
            start_time_unix_ms: 1_770_000_000_000,
            end_time_unix_ms: 1_770_028_800_000,
            timezone: None,
            stage_summary_json: r#"{"minutes_by_stage":{"deeep":42.0}}"#,
            confidence: 0.86,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("stage_summary_json minutes_by_stage.deeep stage must be recognized")
    );

    let error = store
        .insert_external_sleep_stage(ExternalSleepStageInput {
            stage_id: "external-sleep-orphan-stage",
            sleep_id: "missing-sleep",
            stage_kind: "deep",
            start_time_unix_ms: 1_770_003_600_000,
            end_time_unix_ms: 1_770_007_200_000,
            confidence: 0.80,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("external sleep session missing-sleep not found")
    );

    let valid_session = ExternalSleepSessionInput {
        sleep_id: "external-sleep-stage-parent",
        source: "healthkit.sleep_analysis",
        platform: "healthkit",
        platform_record_id: Some("record-2"),
        start_time_unix_ms: 1_770_000_000_000,
        end_time_unix_ms: 1_770_028_800_000,
        timezone: None,
        stage_summary_json: "{}",
        confidence: 0.86,
        provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
    };
    assert!(store.insert_external_sleep_session(valid_session).unwrap());

    let error = store
        .insert_external_sleep_stage(ExternalSleepStageInput {
            stage_id: "external-sleep-out-of-session-stage",
            sleep_id: "external-sleep-stage-parent",
            stage_kind: "deep",
            start_time_unix_ms: 1_769_999_000_000,
            end_time_unix_ms: 1_770_007_200_000,
            confidence: 0.80,
            provenance_json: r#"{"owner":"user","source":"healthkit"}"#,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must be within parent sleep session")
    );
}

#[test]
fn calibration_labels_persist_user_owned_labels_with_provenance() {
    let store = GooseStore::open_in_memory().unwrap();
    let label = CalibrationLabelInput {
        label_id: "manual.recovery.2026-05-28",
        metric_family: "recovery",
        label_source: "manual",
        captured_at: "2026-05-28T06:00:00Z",
        value: 82.0,
        unit: "score_0_to_100",
        provenance_json: r#"{"entry":"typed_by_user","official_labels_are_labels":true}"#,
    };

    assert!(store.insert_calibration_label(label.clone()).unwrap());
    assert!(!store.insert_calibration_label(label).unwrap());

    let saved = store
        .calibration_label("manual.recovery.2026-05-28")
        .unwrap()
        .unwrap();
    assert_eq!(saved.metric_family, "recovery");
    assert_eq!(saved.label_source, "manual");
    assert_eq!(saved.value, 82.0);
    assert!(saved.provenance_json.contains("typed_by_user"));

    let labels = store
        .calibration_labels_between("2026-05-28T00:00:00Z", "2026-05-29T00:00:00Z")
        .unwrap();
    assert_eq!(labels, vec![saved]);
    assert!(store.table_count("calibration_labels").unwrap() == 1);
}

#[test]
fn calibration_labels_reject_private_api_sources_and_empty_provenance() {
    let store = GooseStore::open_in_memory().unwrap();
    let rejected_source = store
        .insert_calibration_label(CalibrationLabelInput {
            label_id: "bad.recovery.private-api",
            metric_family: "recovery",
            label_source: "private_api_replay",
            captured_at: "2026-05-28T06:00:00Z",
            value: 82.0,
            unit: "score_0_to_100",
            provenance_json: r#"{"source":"not_allowed"}"#,
        })
        .unwrap_err();
    assert!(
        rejected_source
            .to_string()
            .contains("unsupported label_source")
    );

    let empty_provenance = store
        .insert_calibration_label(CalibrationLabelInput {
            label_id: "bad.recovery.empty-provenance",
            metric_family: "recovery",
            label_source: "manual",
            captured_at: "2026-05-28T06:00:00Z",
            value: 82.0,
            unit: "score_0_to_100",
            provenance_json: "{}",
        })
        .unwrap_err();
    assert!(empty_provenance.to_string().contains("must not be empty"));
}

#[test]
fn command_validation_records_upsert_query_and_list_direct_send_status() {
    let store = GooseStore::open_in_memory().unwrap();
    let report = validate_commands(&[CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some("user_owned_official_capture".to_string()),
        provenance_json: Some(
            r#"{"capture_app":"whoop_official","capture_kind":"passive_ble_observation","owner":"user"}"#
                .to_string(),
        ),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        official_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        official_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        official_response_frame_hex: Some(GET_HELLO_RESPONSE_FRAME.to_string()),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    }]);
    let ready = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();

    store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: ready.command.clone(),
            risk_gate: "read_only".to_string(),
            direct_send_ready: ready.direct_send_ready,
            report_json: serde_json::to_string(ready).unwrap(),
        })
        .unwrap();

    let stored_ready = store
        .command_validation_record("get_hello")
        .unwrap()
        .unwrap();
    assert_eq!(stored_ready.command, "get_hello");
    assert!(stored_ready.direct_send_ready);
    assert_eq!(stored_ready.risk_gate, "read_only");
    assert!(
        stored_ready
            .report_json
            .contains("\"command\":\"get_hello\"")
    );

    let mut updated_result = ready.clone();
    updated_result.direct_send_ready = false;
    updated_result
        .missing_requirements
        .push("response_parser".to_string());
    store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: ready.command.clone(),
            risk_gate: "read_only".to_string(),
            direct_send_ready: updated_result.direct_send_ready,
            report_json: serde_json::to_string(&updated_result).unwrap(),
        })
        .unwrap();

    let updated = store
        .command_validation_record("get_hello")
        .unwrap()
        .unwrap();
    assert!(!updated.direct_send_ready);
    assert!(updated.report_json.contains("\"command\":\"get_hello\""));
    assert!(updated.report_json.contains("response_parser"));

    let records = store.command_validation_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0], updated);

    let ready_mismatch = store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: "get_hello".to_string(),
            risk_gate: "read_only".to_string(),
            direct_send_ready: false,
            report_json: serde_json::to_string(ready).unwrap(),
        })
        .unwrap_err();
    assert!(
        ready_mismatch
            .to_string()
            .contains("does not match record direct_send_ready false")
    );

    let invalid = store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: "run_alarm".to_string(),
            risk_gate: "user_visible_state_change".to_string(),
            direct_send_ready: false,
            report_json: "{".to_string(),
        })
        .unwrap_err();
    assert!(
        invalid
            .to_string()
            .contains("report_json must be valid JSON")
    );

    let mismatch = store
        .upsert_command_validation_record(&CommandValidationRecord {
            command: "run_alarm".to_string(),
            risk_gate: "user_visible_state_change".to_string(),
            direct_send_ready: ready.direct_send_ready,
            report_json: serde_json::to_string(ready).unwrap(),
        })
        .unwrap_err();
    assert!(
        mismatch
            .to_string()
            .contains("does not match record command run_alarm")
    );
}

#[test]
fn debug_stream_rows_persist_with_session_command_and_event_ordering() {
    let store = GooseStore::open_in_memory().unwrap();
    let session = DebugSessionRow {
        session_id: "debug-session-store".to_string(),
        started_at_unix_ms: 1779840000000,
        bridge_url: "ws://127.0.0.1:49152/goose-debug/stream?token=test".to_string(),
        bind_host: "127.0.0.1".to_string(),
        token_required: true,
        token_present: true,
        remote_bind_enabled: false,
        visible_remote_bind_toggle: false,
    };

    assert!(store.insert_debug_session(&session).unwrap());
    assert!(!store.insert_debug_session(&session).unwrap());
    assert_eq!(
        store.debug_session("debug-session-store").unwrap().unwrap(),
        session
    );

    let command = DebugCommandRow {
        command_id: "cmd-debug-store".to_string(),
        session_id: "debug-session-store".to_string(),
        schema: "goose.debug.command.v1".to_string(),
        command: "storage.check".to_string(),
        args_json: r#"{"self_test":true}"#.to_string(),
        dry_run: true,
        received_at_unix_ms: 1779840000100,
    };
    assert!(store.insert_debug_command(&command).unwrap());
    assert_eq!(
        store
            .debug_commands_for_session("debug-session-store")
            .unwrap(),
        vec![command]
    );
    assert_eq!(
        store
            .next_debug_event_sequence("debug-session-store")
            .unwrap(),
        1
    );

    let first_event = DebugEventRow {
        session_id: "debug-session-store".to_string(),
        sequence: 1,
        schema: "goose.debug.event.v1".to_string(),
        time_unix_ms: 1779840000100,
        source: "command".to_string(),
        level: "info".to_string(),
        topic: "command.started".to_string(),
        message: "storage.check accepted".to_string(),
        command_id: Some("cmd-debug-store".to_string()),
        data_json: r#"{"dry_run":true}"#.to_string(),
    };
    assert!(store.insert_debug_event(&first_event).unwrap());
    assert_eq!(
        store
            .next_debug_event_sequence("debug-session-store")
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .debug_events_for_session("debug-session-store")
            .unwrap(),
        vec![first_event]
    );

    let mut backwards_time = DebugEventRow {
        session_id: "debug-session-store".to_string(),
        sequence: 2,
        schema: "goose.debug.event.v1".to_string(),
        time_unix_ms: 1779840000099,
        source: "app".to_string(),
        level: "info".to_string(),
        topic: "app.backwards".to_string(),
        message: "time moved backwards".to_string(),
        command_id: None,
        data_json: "{}".to_string(),
    };
    assert!(
        store
            .insert_debug_event(&backwards_time)
            .unwrap_err()
            .to_string()
            .contains("before previous event time")
    );
    backwards_time.time_unix_ms = 1779840000200;
    backwards_time.data_json = "[]".to_string();
    assert!(
        store
            .insert_debug_event(&backwards_time)
            .unwrap_err()
            .to_string()
            .contains("JSON object")
    );
}
