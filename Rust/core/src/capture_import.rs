use std::{collections::BTreeSet, fs, path::Path, time::Instant};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{
    GooseError, GooseResult,
    fixtures::{CAPTURED_FRAME_BATCH_SCHEMA, FRAME_HEX_SCHEMA, FixtureIndexReport, IndexedFixture},
    protocol::{DeviceType, decode_hex_with_whitespace, parse_frame},
    store::{
        CaptureSessionInput, DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES, DecodedFrameInput,
        GooseStore, RawEvidenceInput, RawEvidencePayloadRetentionReport,
    },
    timeline::{PacketTimelineRow, packet_timeline_from_decoded_frames},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureImportReport {
    pub schema: String,
    pub generated_by: String,
    pub fixture_root: String,
    pub database_path: String,
    pub pass: bool,
    pub raw_inserted: usize,
    pub raw_existing: usize,
    pub frames_inserted: usize,
    pub frames_existing: usize,
    pub fixtures: Vec<CaptureImportFixtureResult>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CaptureImportNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureImportFixtureResult {
    pub id: String,
    pub path: String,
    pub imported_raw: bool,
    pub imported_frame: bool,
    pub packet_type: Option<u8>,
    pub packet_type_name: Option<String>,
    pub sequence: Option<u8>,
    pub command_or_event: Option<u8>,
    pub parsed_payload_kind: Option<String>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CaptureImportNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct CaptureImportNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct CaptureImportOptions<'a> {
    pub fixture_root: &'a Path,
    pub database_path: &'a Path,
    pub parser_version: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedFrameInput {
    pub evidence_id: String,
    #[serde(default)]
    pub frame_id: Option<String>,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub frame_hex: String,
    pub sensitivity: String,
    #[serde(default)]
    pub capture_session_id: Option<String>,
    #[serde(default = "default_device_type")]
    pub device_type: DeviceType,
}

#[derive(Debug, Clone)]
pub struct CapturedFrameBatchOptions<'a> {
    pub parser_version: &'a str,
    /// Optional device id (CoreBluetooth peripheral UUID) for the session that produced
    /// this batch. When supplied, the value is written to capture_sessions.active_device_id
    /// for every session referenced by the batch frames. When None, existing session rows
    /// are not modified (backward-compatible).
    pub active_device_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct CapturedFrameBatchOutputOptions {
    pub include_timeline_rows: bool,
    pub compact_raw_payloads: bool,
    pub include_results: bool,
}

impl Default for CapturedFrameBatchOutputOptions {
    fn default() -> Self {
        Self {
            include_timeline_rows: true,
            compact_raw_payloads: true,
            include_results: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedFrameBatchImportReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub frame_count: usize,
    pub raw_inserted: usize,
    pub raw_existing: usize,
    pub frames_inserted: usize,
    pub frames_existing: usize,
    pub results: Vec<CapturedFrameImportResult>,
    pub timeline_rows: Vec<PacketTimelineRow>,
    pub raw_payload_retention: RawEvidencePayloadRetentionReport,
    pub timing: CapturedFrameBatchTimingReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CaptureImportNextAction>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CapturedFrameBatchTimingReport {
    pub total_us: u64,
    pub hex_decode_us: u64,
    pub raw_insert_us: u64,
    pub frame_parse_us: u64,
    pub decoded_insert_us: u64,
    pub timeline_us: u64,
    pub raw_compaction_us: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct CapturedFrameBatchTimingAccumulator {
    hex_decode_us: u64,
    raw_insert_us: u64,
    frame_parse_us: u64,
    decoded_insert_us: u64,
    timeline_us: u64,
    raw_compaction_us: u64,
}

impl CapturedFrameBatchTimingAccumulator {
    fn report(self, started: Instant) -> CapturedFrameBatchTimingReport {
        CapturedFrameBatchTimingReport {
            total_us: elapsed_us_u64(started),
            hex_decode_us: self.hex_decode_us,
            raw_insert_us: self.raw_insert_us,
            frame_parse_us: self.frame_parse_us,
            decoded_insert_us: self.decoded_insert_us,
            timeline_us: self.timeline_us,
            raw_compaction_us: self.raw_compaction_us,
        }
    }
}

fn elapsed_us_u64(started: Instant) -> u64 {
    let elapsed = started.elapsed().as_micros();
    if elapsed > u64::MAX as u128 {
        u64::MAX
    } else {
        elapsed as u64
    }
}

#[derive(Debug, Clone)]
pub struct CaptureSqliteImportOptions<'a> {
    pub source_database_path: &'a Path,
    pub target_database_path: &'a Path,
    pub session_id: &'a str,
    pub device_model: &'a str,
    pub sensitivity: &'a str,
    pub parser_version: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSqliteImportReport {
    pub schema: String,
    pub generated_by: String,
    pub source_database_path: String,
    pub target_database_path: String,
    pub session_id: String,
    pub session_started: bool,
    pub session_finished: bool,
    pub source_frame_count: usize,
    pub raw_inserted: usize,
    pub raw_existing: usize,
    pub frames_inserted: usize,
    pub frames_existing: usize,
    pub parse_failed_count: usize,
    pub raw_import_completed: bool,
    pub decode_pass: bool,
    pub pass: bool,
    pub frame_batch_import: CapturedFrameBatchImportReport,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CaptureImportNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapturedFrameBatchFixtureFile {
    pub schema: String,
    pub frames: Vec<CapturedFrameInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedFrameImportResult {
    pub evidence_id: String,
    pub frame_id: String,
    pub imported_raw: bool,
    pub imported_frame: bool,
    pub parse_ok: bool,
    pub packet_type: Option<u8>,
    pub packet_type_name: Option<String>,
    pub sequence: Option<u8>,
    pub command_or_event: Option<u8>,
    pub parsed_payload_kind: Option<String>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CaptureImportNextAction>,
}

pub fn import_captured_frame_batch(
    store: &GooseStore,
    frames: &[CapturedFrameInput],
    options: CapturedFrameBatchOptions<'_>,
) -> GooseResult<CapturedFrameBatchImportReport> {
    import_captured_frame_batch_with_output_options(
        store,
        frames,
        options,
        CapturedFrameBatchOutputOptions::default(),
    )
}

pub fn import_captured_frame_batch_with_output_options(
    store: &GooseStore,
    frames: &[CapturedFrameInput],
    options: CapturedFrameBatchOptions<'_>,
    output_options: CapturedFrameBatchOutputOptions,
) -> GooseResult<CapturedFrameBatchImportReport> {
    store.immediate_transaction(|store| {
        import_captured_frame_batch_with_output_options_in_transaction(
            store,
            frames,
            options,
            output_options,
        )
    })
}

fn import_captured_frame_batch_with_output_options_in_transaction(
    store: &GooseStore,
    frames: &[CapturedFrameInput],
    options: CapturedFrameBatchOptions<'_>,
    output_options: CapturedFrameBatchOutputOptions,
) -> GooseResult<CapturedFrameBatchImportReport> {
    let batch_started = Instant::now();
    let mut timing = CapturedFrameBatchTimingAccumulator::default();
    let mut results = Vec::new();
    let mut issues = Vec::new();
    let mut raw_inserted = 0;
    let mut raw_existing = 0;
    let mut frames_inserted = 0;
    let mut frames_existing = 0;
    let mut decoded_rows = Vec::new();

    if options.parser_version.trim().is_empty() {
        issues.push("parser_version is required".to_string());
    }
    if frames.is_empty() {
        issues.push("at least one captured frame is required".to_string());
    }

    for frame in frames {
        let result =
            import_captured_frame_timed(store, frame, options.parser_version, &mut timing)?;
        if result.imported_raw {
            raw_inserted += 1;
        } else if result
            .issues
            .iter()
            .all(|issue| !issue.contains("raw evidence"))
        {
            raw_existing += 1;
        }
        if result.imported_frame {
            frames_inserted += 1;
        } else if result.parse_ok && result.issues.is_empty() {
            frames_existing += 1;
        }
        if !result.issues.is_empty() {
            issues.push(format!("{} failed import", result.evidence_id));
        }
        if output_options.include_timeline_rows
            && result.parse_ok
            && let Some(row) = store.decoded_frame(&result.frame_id)?
        {
            decoded_rows.push(row);
        }
        if output_options.include_results || !result.issues.is_empty() {
            results.push(result);
        }
    }

    // FIX-01: propagate active_device_id from batch options to capture_sessions.
    // Only runs when active_device_id is supplied and there are frames with a session ref.
    // Uses a SET ... WHERE active_device_id IS NULL guard so already-tagged sessions are
    // left untouched (idempotent on repeated import of the same batch).
    if let Some(device_id) = options.active_device_id {
        let session_ids: BTreeSet<&str> = frames
            .iter()
            .filter_map(|f| f.capture_session_id.as_deref())
            .collect();
        for session_id in session_ids {
            store.set_capture_session_device_id(session_id, device_id)?;
        }
    }

    let timeline_started = Instant::now();
    let timeline_rows = if output_options.include_timeline_rows {
        packet_timeline_from_decoded_frames(&decoded_rows)?
    } else {
        Vec::new()
    };
    timing.timeline_us = timing
        .timeline_us
        .saturating_add(elapsed_us_u64(timeline_started));

    let raw_compaction_started = Instant::now();
    let raw_payload_retention = if output_options.compact_raw_payloads {
        store.compact_raw_evidence_payloads_to_limit(
            DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES,
        )?
    } else {
        RawEvidencePayloadRetentionReport {
            limit_bytes: DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES,
            before_bytes: 0,
            after_bytes: 0,
            compacted_rows: 0,
            freed_bytes: 0,
        }
    };
    timing.raw_compaction_us = timing
        .raw_compaction_us
        .saturating_add(elapsed_us_u64(raw_compaction_started));

    let next_actions = captured_frame_batch_next_actions(&issues, &results);
    let timing = timing.report(batch_started);

    Ok(CapturedFrameBatchImportReport {
        schema: "goose.captured-frame-batch-import-report.v1".to_string(),
        generated_by: "goose-capture-import".to_string(),
        pass: issues.is_empty(),
        frame_count: frames.len(),
        raw_inserted,
        raw_existing,
        frames_inserted,
        frames_existing,
        results,
        timeline_rows,
        raw_payload_retention,
        timing,
        issues,
        next_actions,
    })
}

pub fn import_capture_sqlite(
    store: &GooseStore,
    options: CaptureSqliteImportOptions<'_>,
) -> GooseResult<CaptureSqliteImportReport> {
    let mut issues = Vec::new();
    validate_capture_sqlite_options(&options, &mut issues);

    let rows = match load_capture_sqlite_frame_rows(options.source_database_path) {
        Ok(rows) => rows,
        Err(error) => {
            issues.push(format!("cannot read capture sqlite frames: {error}"));
            Vec::new()
        }
    };
    if rows.is_empty() {
        issues.push("capture sqlite has no framed records".to_string());
    }

    let timestamp_bounds = capture_sqlite_timestamp_bounds(&rows);
    if timestamp_bounds.is_none() && !rows.is_empty() {
        issues.push("capture sqlite framed records have no parseable timestamps".to_string());
    }
    let (started_at_unix_ms, ended_at_unix_ms) = timestamp_bounds.unwrap_or((0, 0));

    let can_import = !options.session_id.trim().is_empty()
        && !options.device_model.trim().is_empty()
        && !options.sensitivity.trim().is_empty()
        && !options.parser_version.trim().is_empty();

    let provenance_json = serde_json::json!({
        "source": "capture_sqlite",
        "source_database_path": options.source_database_path.display().to_string(),
        "parser_version": options.parser_version,
    })
    .to_string();
    let existing_session = if can_import {
        store.capture_session(options.session_id)?
    } else {
        None
    };
    let session_started = if can_import && existing_session.is_none() && !rows.is_empty() {
        store.start_capture_session(CaptureSessionInput {
            session_id: options.session_id,
            source: "capture.sqlite",
            started_at_unix_ms,
            device_model: options.device_model,
            active_device_id: None,
            provenance_json: &provenance_json,
        })?
    } else {
        false
    };

    let frames = if can_import {
        rows.iter()
            .map(|row| capture_sqlite_frame_input(row, &options))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let frame_batch_import = import_captured_frame_batch(
        store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: options.parser_version,
            active_device_id: None,
        },
    )?;

    let session_finished = if can_import && !rows.is_empty() {
        store
            .finish_capture_session(
                options.session_id,
                ended_at_unix_ms.max(started_at_unix_ms),
                rows.len() as i64,
            )
            .map(|_| true)?
    } else {
        false
    };

    let parse_failed_count = frame_batch_import
        .results
        .iter()
        .filter(|result| !result.parse_ok)
        .count();
    let raw_import_completed =
        frame_batch_import.raw_inserted + frame_batch_import.raw_existing == rows.len();
    let decode_pass = frame_batch_import.pass;
    if !raw_import_completed && !rows.is_empty() {
        issues.push("not all capture sqlite frames were preserved as raw evidence".to_string());
    }
    if parse_failed_count > 0 {
        issues.push(format!(
            "{parse_failed_count} capture sqlite frames preserved as raw evidence but not decoded"
        ));
    }
    let pass = issues.is_empty() && decode_pass;
    let next_actions = capture_sqlite_import_next_actions(&issues, &frame_batch_import);

    Ok(CaptureSqliteImportReport {
        schema: "goose.capture-sqlite-import-report.v1".to_string(),
        generated_by: "goose-capture-sqlite-import".to_string(),
        source_database_path: options.source_database_path.display().to_string(),
        target_database_path: options.target_database_path.display().to_string(),
        session_id: options.session_id.to_string(),
        session_started,
        session_finished,
        source_frame_count: rows.len(),
        raw_inserted: frame_batch_import.raw_inserted,
        raw_existing: frame_batch_import.raw_existing,
        frames_inserted: frame_batch_import.frames_inserted,
        frames_existing: frame_batch_import.frames_existing,
        parse_failed_count,
        raw_import_completed,
        decode_pass,
        pass,
        frame_batch_import,
        issues,
        next_actions,
    })
}

pub fn import_fixture_index(
    store: &GooseStore,
    index: &FixtureIndexReport,
    options: CaptureImportOptions<'_>,
) -> CaptureImportReport {
    let mut fixtures = Vec::new();
    let mut issues = Vec::new();
    let mut raw_inserted = 0;
    let mut raw_existing = 0;
    let mut frames_inserted = 0;
    let mut frames_existing = 0;

    for fixture in &index.fixtures {
        let fixture_results = match fixture.schema.as_str() {
            FRAME_HEX_SCHEMA => vec![import_frame_fixture(store, fixture, &options)],
            CAPTURED_FRAME_BATCH_SCHEMA => {
                import_captured_frame_batch_fixture(store, fixture, &options)
            }
            _ => continue,
        };

        for result in fixture_results {
            if result.imported_raw {
                raw_inserted += 1;
            } else if result.issues.is_empty() {
                raw_existing += 1;
            }
            if result.imported_frame {
                frames_inserted += 1;
            } else if result.issues.is_empty() {
                frames_existing += 1;
            }
            if !result.issues.is_empty() {
                issues.push(format!("{} failed import", result.id));
            }
            fixtures.push(result);
        }
    }

    if fixtures.is_empty() {
        issues.push("no importable frame fixtures found to import".to_string());
    }

    let next_actions = capture_import_report_next_actions(&issues, &fixtures);

    CaptureImportReport {
        schema: "goose.capture-import-report.v1".to_string(),
        generated_by: "goose-capture-import".to_string(),
        fixture_root: options.fixture_root.display().to_string(),
        database_path: options.database_path.display().to_string(),
        pass: issues.is_empty(),
        raw_inserted,
        raw_existing,
        frames_inserted,
        frames_existing,
        fixtures,
        issues,
        next_actions,
    }
}

fn import_captured_frame(
    store: &GooseStore,
    frame: &CapturedFrameInput,
    parser_version: &str,
) -> GooseResult<CapturedFrameImportResult> {
    let mut timing = CapturedFrameBatchTimingAccumulator::default();
    import_captured_frame_timed(store, frame, parser_version, &mut timing)
}

fn import_captured_frame_timed(
    store: &GooseStore,
    frame: &CapturedFrameInput,
    parser_version: &str,
    timing: &mut CapturedFrameBatchTimingAccumulator,
) -> GooseResult<CapturedFrameImportResult> {
    let mut issues = Vec::new();
    let frame_id = frame
        .frame_id
        .clone()
        .unwrap_or_else(|| format!("{}.frame.0", frame.evidence_id));

    let hex_decode_started = Instant::now();
    let raw_bytes = match decode_hex_with_whitespace(&frame.frame_hex) {
        Ok(raw_bytes) => raw_bytes,
        Err(error) => {
            timing.hex_decode_us = timing
                .hex_decode_us
                .saturating_add(elapsed_us_u64(hex_decode_started));
            return Ok(CapturedFrameImportResult {
                evidence_id: frame.evidence_id.clone(),
                frame_id,
                imported_raw: false,
                imported_frame: false,
                parse_ok: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(&frame.evidence_id, &[error.to_string()]),
                issues: vec![error.to_string()],
            });
        }
    };
    timing.hex_decode_us = timing
        .hex_decode_us
        .saturating_add(elapsed_us_u64(hex_decode_started));

    let raw_insert_started = Instant::now();
    let imported_raw = match store.insert_raw_evidence(RawEvidenceInput {
        evidence_id: &frame.evidence_id,
        source: &frame.source,
        captured_at: &frame.captured_at,
        device_model: &frame.device_model,
        payload: &raw_bytes,
        sensitivity: &frame.sensitivity,
        capture_session_id: frame.capture_session_id.as_deref(),
    }) {
        Ok(imported) => imported,
        Err(error) => {
            issues.push(format!("raw evidence insert failed: {error}"));
            if frame.capture_session_id.is_some() {
                match store.insert_raw_evidence(RawEvidenceInput {
                    evidence_id: &frame.evidence_id,
                    source: &frame.source,
                    captured_at: &frame.captured_at,
                    device_model: &frame.device_model,
                    payload: &raw_bytes,
                    sensitivity: &frame.sensitivity,
                    capture_session_id: None,
                }) {
                    Ok(imported) => {
                        issues.push(
                            "raw evidence inserted without capture_session_id after session-scoped insert failed"
                                .to_string(),
                        );
                        imported
                    }
                    Err(fallback_error) => {
                        issues.push(format!(
                            "raw evidence fallback insert without capture_session_id failed: {fallback_error}"
                        ));
                        false
                    }
                }
            } else {
                false
            }
        }
    };
    timing.raw_insert_us = timing
        .raw_insert_us
        .saturating_add(elapsed_us_u64(raw_insert_started));

    let frame_parse_started = Instant::now();

    // HR monitor (0x2A37 standard GATT) frames are NOT 0xAA-framed WHOOP frames;
    // parse_frame rejects them at FRAME_START. Bypass parsing and store raw GATT bytes
    // directly as a decoded_frames row so the upload bridge can read them.
    // Both CRC flags must be true: the upload bridge skips rows where either is false.
    if frame.device_type == DeviceType::HrMonitor {
        use crate::protocol::ParsedFrame;
        let hr_pseudo_frame = ParsedFrame {
            device_type: DeviceType::HrMonitor,
            raw_len: raw_bytes.len(),
            header_len: 0,
            declared_len: raw_bytes.len(),
            // Full GATT bytes as hex so the upload bridge can call parse_hr_measurement
            payload_hex: hex::encode(&raw_bytes),
            payload_crc_hex: String::new(),
            // Both true so the upload bridge's CRC-skip (bridge.rs ~line 3059) does not drop the row
            header_crc_valid: true,
            payload_crc_valid: true,
            packet_type: None,
            packet_type_name: None,
            sequence: None,
            command_or_event: None,
            parsed_payload: None,
            warnings: Vec::new(),
        };
        timing.frame_parse_us = timing
            .frame_parse_us
            .saturating_add(elapsed_us_u64(frame_parse_started));

        let decoded_insert_started = Instant::now();
        let imported_frame = match store.insert_decoded_frame(DecodedFrameInput {
            frame_id: &frame_id,
            evidence_id: &frame.evidence_id,
            parsed: &hr_pseudo_frame,
            parser_version,
        }) {
            Ok(imported) => imported,
            Err(error) => {
                issues.push(error.to_string());
                false
            }
        };
        timing.decoded_insert_us = timing
            .decoded_insert_us
            .saturating_add(elapsed_us_u64(decoded_insert_started));

        return Ok(CapturedFrameImportResult {
            evidence_id: frame.evidence_id.clone(),
            frame_id,
            imported_raw,
            imported_frame,
            parse_ok: issues.is_empty(),
            packet_type: None,
            packet_type_name: None,
            sequence: None,
            command_or_event: None,
            parsed_payload_kind: None,
            next_actions: capture_import_next_actions(&frame.evidence_id, &issues),
            issues,
        });
    }

    let parsed = match parse_frame(frame.device_type, &raw_bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            timing.frame_parse_us = timing
                .frame_parse_us
                .saturating_add(elapsed_us_u64(frame_parse_started));
            issues.push(error.to_string());
            return Ok(CapturedFrameImportResult {
                evidence_id: frame.evidence_id.clone(),
                frame_id,
                imported_raw,
                imported_frame: false,
                parse_ok: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(&frame.evidence_id, &issues),
                issues,
            });
        }
    };
    timing.frame_parse_us = timing
        .frame_parse_us
        .saturating_add(elapsed_us_u64(frame_parse_started));

    let decoded_insert_started = Instant::now();
    let imported_frame = match store.insert_decoded_frame(DecodedFrameInput {
        frame_id: &frame_id,
        evidence_id: &frame.evidence_id,
        parsed: &parsed,
        parser_version,
    }) {
        Ok(imported) => imported,
        Err(error) => {
            issues.push(error.to_string());
            false
        }
    };
    timing.decoded_insert_us = timing
        .decoded_insert_us
        .saturating_add(elapsed_us_u64(decoded_insert_started));

    Ok(CapturedFrameImportResult {
        evidence_id: frame.evidence_id.clone(),
        frame_id,
        imported_raw,
        imported_frame,
        parse_ok: issues.is_empty(),
        packet_type: parsed.packet_type,
        packet_type_name: parsed.packet_type_name.clone(),
        sequence: parsed.sequence,
        command_or_event: parsed.command_or_event,
        parsed_payload_kind: parsed.parsed_payload.as_ref().map(parsed_payload_kind),
        next_actions: capture_import_next_actions(&frame.evidence_id, &issues),
        issues,
    })
}

fn import_frame_fixture(
    store: &GooseStore,
    fixture: &IndexedFixture,
    options: &CaptureImportOptions<'_>,
) -> CaptureImportFixtureResult {
    let mut issues = Vec::new();
    let path = options.fixture_root.join(&fixture.path);

    let raw_text = match fs::read_to_string(&path) {
        Ok(raw_text) => raw_text,
        Err(source) => {
            return CaptureImportFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                imported_raw: false,
                imported_frame: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(
                    &fixture.id,
                    &[format!("cannot read fixture file: {source}")],
                ),
                issues: vec![format!("cannot read fixture file: {source}")],
            };
        }
    };

    let raw_bytes = match decode_hex_with_whitespace(&raw_text) {
        Ok(raw_bytes) => raw_bytes,
        Err(error) => {
            return CaptureImportFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                imported_raw: false,
                imported_frame: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(&fixture.id, &[error.to_string()]),
                issues: vec![error.to_string()],
            };
        }
    };

    let imported_raw = match store.insert_raw_evidence(RawEvidenceInput {
        evidence_id: &fixture.id,
        source: &fixture.source,
        captured_at: &fixture.captured_at,
        device_model: &fixture.device_model,
        payload: &raw_bytes,
        sensitivity: &fixture.sensitivity,
        capture_session_id: None,
    }) {
        Ok(imported) => imported,
        Err(error) => {
            issues.push(error.to_string());
            false
        }
    };

    let device_type = fixture
        .expected
        .as_ref()
        .and_then(expected_device_type)
        .unwrap_or(DeviceType::Goose);
    let parsed = match parse_frame(device_type, &raw_bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            issues.push(error.to_string());
            return CaptureImportFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                imported_raw,
                imported_frame: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(&fixture.id, &issues),
                issues,
            };
        }
    };

    let frame_id = format!("{}.frame.0", fixture.id);
    let imported_frame = match store.insert_decoded_frame(DecodedFrameInput {
        frame_id: &frame_id,
        evidence_id: &fixture.id,
        parsed: &parsed,
        parser_version: options.parser_version,
    }) {
        Ok(imported) => imported,
        Err(error) => {
            issues.push(error.to_string());
            false
        }
    };

    CaptureImportFixtureResult {
        id: fixture.id.clone(),
        path: fixture.path.clone(),
        imported_raw,
        imported_frame,
        packet_type: parsed.packet_type,
        packet_type_name: parsed.packet_type_name.clone(),
        sequence: parsed.sequence,
        command_or_event: parsed.command_or_event,
        parsed_payload_kind: parsed.parsed_payload.as_ref().map(parsed_payload_kind),
        next_actions: capture_import_next_actions(&fixture.id, &issues),
        issues,
    }
}

fn import_captured_frame_batch_fixture(
    store: &GooseStore,
    fixture: &IndexedFixture,
    options: &CaptureImportOptions<'_>,
) -> Vec<CaptureImportFixtureResult> {
    let path = options.fixture_root.join(&fixture.path);
    let raw_text = match fs::read_to_string(&path) {
        Ok(raw_text) => raw_text,
        Err(source) => {
            return vec![CaptureImportFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                imported_raw: false,
                imported_frame: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(
                    &fixture.id,
                    &[format!("cannot read captured frame batch: {source}")],
                ),
                issues: vec![format!("cannot read captured frame batch: {source}")],
            }];
        }
    };

    let batch: CapturedFrameBatchFixtureFile = match serde_json::from_str(&raw_text) {
        Ok(batch) => batch,
        Err(source) => {
            return vec![CaptureImportFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                imported_raw: false,
                imported_frame: false,
                packet_type: None,
                packet_type_name: None,
                sequence: None,
                command_or_event: None,
                parsed_payload_kind: None,
                next_actions: capture_import_next_actions(
                    &fixture.id,
                    &[format!("cannot parse captured frame batch: {source}")],
                ),
                issues: vec![format!("cannot parse captured frame batch: {source}")],
            }];
        }
    };

    if batch.schema != CAPTURED_FRAME_BATCH_SCHEMA {
        return vec![CaptureImportFixtureResult {
            id: fixture.id.clone(),
            path: fixture.path.clone(),
            imported_raw: false,
            imported_frame: false,
            packet_type: None,
            packet_type_name: None,
            sequence: None,
            command_or_event: None,
            parsed_payload_kind: None,
            next_actions: capture_import_next_actions(
                &fixture.id,
                &[format!(
                    "captured frame batch schema must be {CAPTURED_FRAME_BATCH_SCHEMA}"
                )],
            ),
            issues: vec![format!(
                "captured frame batch schema must be {CAPTURED_FRAME_BATCH_SCHEMA}"
            )],
        }];
    }
    if batch.frames.is_empty() {
        return vec![CaptureImportFixtureResult {
            id: fixture.id.clone(),
            path: fixture.path.clone(),
            imported_raw: false,
            imported_frame: false,
            packet_type: None,
            packet_type_name: None,
            sequence: None,
            command_or_event: None,
            parsed_payload_kind: None,
            next_actions: capture_import_next_actions(
                &fixture.id,
                &["captured frame batch must include at least one frame".to_string()],
            ),
            issues: vec!["captured frame batch must include at least one frame".to_string()],
        }];
    }

    batch
        .frames
        .iter()
        .map(
            |frame| match import_captured_frame(store, frame, options.parser_version) {
                Ok(result) => CaptureImportFixtureResult {
                    id: result.evidence_id,
                    path: format!("{}#{}", fixture.path, result.frame_id),
                    imported_raw: result.imported_raw,
                    imported_frame: result.imported_frame,
                    packet_type: result.packet_type,
                    packet_type_name: result.packet_type_name,
                    sequence: result.sequence,
                    command_or_event: result.command_or_event,
                    parsed_payload_kind: result.parsed_payload_kind,
                    next_actions: result.next_actions,
                    issues: result.issues,
                },
                Err(error) => CaptureImportFixtureResult {
                    id: frame.evidence_id.clone(),
                    path: fixture.path.clone(),
                    imported_raw: false,
                    imported_frame: false,
                    packet_type: None,
                    packet_type_name: None,
                    sequence: None,
                    command_or_event: None,
                    parsed_payload_kind: None,
                    next_actions: capture_import_next_actions(
                        &frame.evidence_id,
                        &[error.to_string()],
                    ),
                    issues: vec![error.to_string()],
                },
            },
        )
        .collect()
}

fn captured_frame_batch_next_actions(
    issues: &[String],
    results: &[CapturedFrameImportResult],
) -> Vec<CaptureImportNextAction> {
    let mut actions = issues
        .iter()
        .flat_map(|issue| {
            if issue == "parser_version is required"
                || issue == "at least one captured frame is required"
            {
                capture_import_next_actions("captured_frame_batch", std::slice::from_ref(issue))
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    actions.extend(
        results
            .iter()
            .flat_map(|result| result.next_actions.iter().cloned()),
    );
    dedupe_capture_import_next_actions(actions)
}

fn capture_import_report_next_actions(
    issues: &[String],
    fixtures: &[CaptureImportFixtureResult],
) -> Vec<CaptureImportNextAction> {
    let mut actions = issues
        .iter()
        .flat_map(|issue| {
            if issue == "no importable frame fixtures found to import" {
                capture_import_next_actions("fixture_index", std::slice::from_ref(issue))
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    actions.extend(
        fixtures
            .iter()
            .flat_map(|fixture| fixture.next_actions.iter().cloned()),
    );
    dedupe_capture_import_next_actions(actions)
}

#[derive(Debug, Clone)]
struct CaptureSqliteFrameRow {
    line_no: i64,
    decode_index: i64,
    captured_at: String,
    role: Option<String>,
    value_hex: String,
}

fn validate_capture_sqlite_options(
    options: &CaptureSqliteImportOptions<'_>,
    issues: &mut Vec<String>,
) {
    if options.session_id.trim().is_empty() {
        issues.push("session_id is required".to_string());
    }
    if options.device_model.trim().is_empty() {
        issues.push("device_model is required".to_string());
    }
    if options.sensitivity.trim().is_empty() {
        issues.push("sensitivity is required".to_string());
    }
    if options.parser_version.trim().is_empty() {
        issues.push("parser_version is required".to_string());
    }
}

fn load_capture_sqlite_frame_rows(path: &Path) -> GooseResult<Vec<CaptureSqliteFrameRow>> {
    let connection = Connection::open(path).map_err(|error| {
        GooseError::message(format!(
            "cannot open capture sqlite {}: {error}",
            path.display()
        ))
    })?;
    if !capture_sqlite_table_exists(&connection, "records")?
        || !capture_sqlite_table_exists(&connection, "packets")?
    {
        return Err(GooseError::message(
            "capture sqlite must contain records and packets tables",
        ));
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                records.line_no,
                MIN(packets.decode_index) AS decode_index,
                records.ts,
                records.role,
                records.value_hex
            FROM records
            INNER JOIN packets ON packets.record_id = records.id
            WHERE COALESCE(packets.is_frame, 0) = 1
              AND records.value_hex IS NOT NULL
              AND length(trim(records.value_hex)) > 0
            GROUP BY records.id, records.line_no, records.ts, records.role, records.value_hex
            ORDER BY records.line_no, decode_index
            "#,
        )
        .map_err(|error| GooseError::message(format!("cannot query capture sqlite: {error}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CaptureSqliteFrameRow {
                line_no: row.get(0)?,
                decode_index: row.get(1)?,
                captured_at: normalize_capture_sqlite_timestamp(row.get::<_, String>(2)?.as_str()),
                role: row.get(3)?,
                value_hex: row.get(4)?,
            })
        })
        .map_err(|error| GooseError::message(format!("cannot scan capture sqlite: {error}")))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(GooseError::from)
}

fn capture_sqlite_table_exists(connection: &Connection, table: &str) -> GooseResult<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|error| GooseError::message(format!("cannot inspect capture sqlite: {error}")))?;
    Ok(count > 0)
}

fn capture_sqlite_timestamp_bounds(rows: &[CaptureSqliteFrameRow]) -> Option<(i64, i64)> {
    let mut min_ms: Option<i64> = None;
    let mut max_ms: Option<i64> = None;
    for row in rows {
        let Some(ms) = parse_capture_rfc3339_unix_ms(&row.captured_at) else {
            continue;
        };
        min_ms = Some(min_ms.map_or(ms, |current| current.min(ms)));
        max_ms = Some(max_ms.map_or(ms, |current| current.max(ms)));
    }
    min_ms.zip(max_ms)
}

fn capture_sqlite_frame_input(
    row: &CaptureSqliteFrameRow,
    options: &CaptureSqliteImportOptions<'_>,
) -> CapturedFrameInput {
    let session_token = sanitize_capture_sqlite_token(options.session_id);
    let evidence_id = format!(
        "{session_token}.line-{}.decode-{}",
        row.line_no, row.decode_index
    );
    let source = row
        .role
        .as_ref()
        .map(|role| format!("capture.sqlite.{role}"))
        .unwrap_or_else(|| "capture.sqlite".to_string());
    CapturedFrameInput {
        frame_id: Some(format!("{evidence_id}.frame.0")),
        evidence_id,
        source,
        captured_at: row.captured_at.clone(),
        device_model: options.device_model.to_string(),
        frame_hex: row.value_hex.clone(),
        sensitivity: options.sensitivity.to_string(),
        capture_session_id: Some(options.session_id.to_string()),
        device_type: DeviceType::Goose,
    }
}

fn capture_sqlite_import_next_actions(
    issues: &[String],
    frame_batch_import: &CapturedFrameBatchImportReport,
) -> Vec<CaptureImportNextAction> {
    let mut actions = issues
        .iter()
        .map(|issue| {
            let (reason, action) = capture_sqlite_issue_action(issue);
            CaptureImportNextAction {
                scope: "capture_sqlite".to_string(),
                reason: reason.to_string(),
                action: action.to_string(),
            }
        })
        .collect::<Vec<_>>();
    actions.extend(frame_batch_import.next_actions.iter().cloned());
    dedupe_capture_import_next_actions(actions)
}

fn capture_sqlite_issue_action(issue: &str) -> (&'static str, &'static str) {
    if issue == "session_id is required" {
        (
            "session_id_missing",
            "Pass --session-id with the owned capture session identifier before importing capture.sqlite evidence.",
        )
    } else if issue == "capture sqlite has no framed records" {
        (
            "capture_sqlite_frames_missing",
            "Regenerate the HCI capture analysis database and confirm packets.is_frame rows are present before importing.",
        )
    } else if issue.contains("no parseable timestamps") {
        (
            "capture_sqlite_timestamps_unparseable",
            "Regenerate capture.sqlite with RFC3339 record timestamps so validation windows can bind to the owned session.",
        )
    } else if issue.contains("not all capture sqlite frames were preserved") {
        (
            "capture_sqlite_raw_import_incomplete",
            "Run storage.check, repair raw_evidence constraints, then rerun the capture.sqlite import.",
        )
    } else if issue.contains("preserved as raw evidence but not decoded") {
        (
            "capture_sqlite_decode_incomplete",
            "Use the raw-only rows as parser fixtures, fix the parser gaps, then rerun the import before relying on decoded metric evidence.",
        )
    } else if issue.starts_with("cannot read capture sqlite") {
        (
            "capture_sqlite_unreadable",
            "Repair the capture.sqlite path, schema, or permissions before importing owned packet evidence.",
        )
    } else {
        capture_import_issue_action(issue)
    }
}

fn normalize_capture_sqlite_timestamp(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(prefix) = trimmed.strip_suffix("+00:00") {
        format!("{prefix}Z")
    } else {
        trimmed.to_string()
    }
}

fn sanitize_capture_sqlite_token(value: &str) -> String {
    let mut token = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while token.contains("--") {
        token = token.replace("--", "-");
    }
    token.trim_matches('-').to_string()
}

fn parse_capture_rfc3339_unix_ms(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let (date, time_and_zone) = trimmed.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let (time, offset_seconds) = split_rfc3339_time_and_offset(time_and_zone)?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let (second_text, fraction_text) = second_part
        .split_once('.')
        .map_or((second_part, ""), |(second, fraction)| (second, fraction));
    let second = second_text.parse::<i64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let millis = parse_fraction_millis(fraction_text)?;
    let days = days_from_civil(year, month, day);
    Some(
        days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000 + millis
            - offset_seconds * 1_000,
    )
}

fn split_rfc3339_time_and_offset(value: &str) -> Option<(&str, i64)> {
    if let Some(time) = value.strip_suffix('Z') {
        return Some((time, 0));
    }
    let sign_index = value
        .char_indices()
        .skip(1)
        .find_map(|(index, ch)| (ch == '+' || ch == '-').then_some(index))?;
    let (time, offset) = value.split_at(sign_index);
    let sign = if offset.starts_with('+') { 1 } else { -1 };
    let mut parts = offset[1..].split(':');
    let hours = parts.next()?.parse::<i64>().ok()?;
    let minutes = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }
    Some((time, sign * (hours * 3600 + minutes * 60)))
}

fn parse_fraction_millis(fraction: &str) -> Option<i64> {
    if fraction.is_empty() {
        return Some(0);
    }
    if !fraction.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut digits = fraction.chars().take(3).collect::<String>();
    while digits.len() < 3 {
        digits.push('0');
    }
    digits.parse::<i64>().ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn capture_import_next_actions(scope: &str, issues: &[String]) -> Vec<CaptureImportNextAction> {
    dedupe_capture_import_next_actions(
        issues
            .iter()
            .map(|issue| {
                let (reason, action) = capture_import_issue_action(issue);
                CaptureImportNextAction {
                    scope: scope.to_string(),
                    reason: reason.to_string(),
                    action: action.to_string(),
                }
            })
            .collect(),
    )
}

fn capture_import_issue_action(issue: &str) -> (&'static str, &'static str) {
    let lower = issue.to_ascii_lowercase();
    if issue == "parser_version is required" {
        (
            "parser_version_missing",
            "Set a non-empty parser_version that identifies the app/core parser build before importing capture frames.",
        )
    } else if issue == "at least one captured frame is required"
        || issue == "captured frame batch must include at least one frame"
    {
        (
            "captured_frame_batch_empty",
            "Select or capture at least one user-owned frame, then rerun capture.import_frame_batch.",
        )
    } else if issue == "no importable frame fixtures found to import" {
        (
            "no_importable_fixtures",
            "Regenerate the fixture index or add frame-hex/captured-frame-batch fixtures before running capture import.",
        )
    } else if issue.starts_with("cannot read fixture file")
        || issue.starts_with("cannot read captured frame batch")
    {
        (
            "fixture_file_unreadable",
            "Repair the fixture path or file permissions, regenerate the fixture index, then rerun capture import.",
        )
    } else if issue.starts_with("cannot parse captured frame batch") {
        (
            "captured_frame_batch_json_invalid",
            "Regenerate the captured-frame batch as valid JSON with sanitized frame rows, then rerun capture import.",
        )
    } else if issue.starts_with("captured frame batch schema must be") {
        (
            "captured_frame_batch_schema_invalid",
            "Regenerate the captured-frame batch with the supported goose.captured-frame-batch schema before importing it.",
        )
    } else if lower.contains("hex") || lower.contains("invalid character") {
        (
            "frame_hex_invalid",
            "Re-export or recapture the frame as complete hex bytes, then add a malformed-frame regression if the source was trusted.",
        )
    } else if lower.contains("raw evidence insert failed")
        || lower.contains("raw_evidence")
        || lower.contains("raw evidence")
    {
        (
            "raw_evidence_insert_failed",
            "Run storage.check, repair raw_evidence schema/session references, then rerun capture import so raw bytes are preserved.",
        )
    } else if lower.contains("decoded frame insert")
        || lower.contains("decoded_frames")
        || lower.contains("foreign key")
        || lower.contains("unique constraint")
    {
        (
            "decoded_frame_insert_failed",
            "Run storage.check and repair decoded-frame constraints or evidence references before trusting parsed frame storage.",
        )
    } else if lower.contains("frame") || lower.contains("parse") || lower.contains("crc") {
        (
            "frame_parse_failed",
            "Preserve the raw evidence row, add this frame as a parser fixture with provenance, then implement or fix the parser path.",
        )
    } else {
        (
            "capture_import_issue",
            "Inspect this capture import issue, preserve the raw artifact, and add a targeted regression before trusting the imported frame.",
        )
    }
}

fn dedupe_capture_import_next_actions(
    actions: Vec<CaptureImportNextAction>,
) -> Vec<CaptureImportNextAction> {
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

fn default_device_type() -> DeviceType {
    DeviceType::Goose
}

fn parsed_payload_kind(payload: &crate::protocol::ParsedPayload) -> String {
    match payload {
        crate::protocol::ParsedPayload::Command { .. } => "command",
        crate::protocol::ParsedPayload::CommandResponse { .. } => "command_response",
        crate::protocol::ParsedPayload::Event { .. } => "event",
        crate::protocol::ParsedPayload::DataPacket { .. } => "data_packet",
        crate::protocol::ParsedPayload::Raw { .. } => "raw",
    }
    .to_string()
}

fn expected_device_type(expected: &serde_json::Value) -> Option<DeviceType> {
    let value = expected.get("device_type")?.as_str()?;
    match value {
        "GEN_4" => Some(DeviceType::Gen4),
        "MAVERICK" => Some(DeviceType::Maverick),
        "PUFFIN" => Some(DeviceType::Puffin),
        "GOOSE" => Some(DeviceType::Goose),
        _ => None,
    }
}

pub fn ensure_database_parent(path: &Path) -> GooseResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| GooseError::io(parent, source))?;
    }
    Ok(())
}
