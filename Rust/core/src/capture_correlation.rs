use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    GooseError, GooseResult,
    capture_import::CapturedFrameInput,
    fixtures::{CAPTURED_FRAME_BATCH_SCHEMA, FRAME_HEX_SCHEMA, FixtureIndexReport, IndexedFixture},
    protocol::{
        DataPacketBodySummary, DeviceType, ParsedFrame, ParsedPayload, build_v5_payload_frame,
        decode_hex_with_whitespace, parse_frame,
    },
    store::{DecodedFrameRow, GooseStore, RawEvidenceRow},
};

pub const CAPTURE_CORRELATION_REPORT_SCHEMA: &str = "goose.capture-correlation-report.v1";
pub const DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY: usize = 2;

#[derive(Debug, Clone, Copy)]
pub struct CaptureCorrelationOptions {
    pub min_owned_captures_per_summary: usize,
    pub require_owned_captures: bool,
}

impl Default for CaptureCorrelationOptions {
    fn default() -> Self {
        Self {
            min_owned_captures_per_summary: DEFAULT_MIN_OWNED_CAPTURES_PER_SUMMARY,
            require_owned_captures: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCorrelationReport {
    pub schema: String,
    pub generated_by: String,
    pub fixture_root: String,
    pub pass: bool,
    pub min_owned_captures_per_summary: usize,
    pub require_owned_captures: bool,
    pub observations: Vec<CaptureCorrelationObservation>,
    pub summaries: Vec<CaptureCorrelationSummary>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_capture_actions: Vec<CaptureCorrelationNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCorrelationObservation {
    pub fixture_id: String,
    pub evidence_id: String,
    pub path: String,
    pub kind: String,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub synthetic: bool,
    pub owned_capture: bool,
    pub packet_type_name: Option<String>,
    pub packet_k: Option<u8>,
    pub domain: Option<String>,
    pub body_summary_kind: String,
    pub warning_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCorrelationSummary {
    pub body_summary_kind: String,
    pub observation_count: usize,
    pub owned_capture_count: usize,
    pub synthetic_count: usize,
    pub warning_count: usize,
    pub trusted_metric_ready: bool,
    pub blocker_reasons: Vec<String>,
    #[serde(default)]
    pub next_capture_actions: Vec<CaptureCorrelationNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureCorrelationNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapturedFrameBatchFixtureFile {
    pub schema: String,
    pub frames: Vec<CapturedFrameInput>,
}

#[derive(Debug, Default)]
struct SummaryAccumulator {
    observation_count: usize,
    owned_capture_count: usize,
    synthetic_count: usize,
    warning_count: usize,
}

pub fn run_capture_correlation(
    root: &Path,
    index: &FixtureIndexReport,
    options: CaptureCorrelationOptions,
) -> CaptureCorrelationReport {
    let mut observations = Vec::new();
    let mut issues = Vec::new();

    if options.min_owned_captures_per_summary == 0 {
        issues.push("min_owned_captures_per_summary must be greater than zero".to_string());
    }

    for fixture in &index.fixtures {
        match fixture.schema.as_str() {
            FRAME_HEX_SCHEMA => {
                observe_frame_fixture(root, fixture, &mut observations, &mut issues)
            }
            crate::fixtures::PAYLOAD_HEX_SCHEMA => {
                observe_payload_fixture(root, fixture, &mut observations, &mut issues)
            }
            CAPTURED_FRAME_BATCH_SCHEMA => {
                observe_captured_frame_batch_fixture(root, fixture, &mut observations, &mut issues)
            }
            _ => {}
        }
    }

    if observations.is_empty() {
        issues.push("no packet/event summaries found for capture correlation".to_string());
    }

    let summaries = summarize_observations(
        &observations,
        options.min_owned_captures_per_summary,
        options.require_owned_captures,
        &mut issues,
    );
    let next_capture_actions =
        report_next_capture_actions(&summaries, observations.is_empty(), &options);

    CaptureCorrelationReport {
        schema: CAPTURE_CORRELATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-capture-correlation".to_string(),
        fixture_root: root.display().to_string(),
        pass: issues.is_empty(),
        min_owned_captures_per_summary: options.min_owned_captures_per_summary,
        require_owned_captures: options.require_owned_captures,
        observations,
        summaries,
        issues,
        next_capture_actions,
    }
}

pub fn run_capture_correlation_for_store(
    store: &GooseStore,
    evidence_scope: &str,
    start: &str,
    end: &str,
    options: CaptureCorrelationOptions,
) -> GooseResult<CaptureCorrelationReport> {
    if start.trim().is_empty() {
        return Err(GooseError::message("start is required"));
    }
    if end.trim().is_empty() {
        return Err(GooseError::message("end is required"));
    }
    if start >= end {
        return Err(GooseError::message("start must be earlier than end"));
    }
    let raw_rows = store.raw_evidence_between(start, end)?;
    let decoded_rows = store.decoded_frames_between(start, end)?;
    Ok(run_capture_correlation_for_rows(
        evidence_scope,
        &raw_rows,
        &decoded_rows,
        options,
    ))
}

pub fn run_capture_correlation_for_rows(
    evidence_scope: &str,
    raw_rows: &[RawEvidenceRow],
    decoded_rows: &[DecodedFrameRow],
    options: CaptureCorrelationOptions,
) -> CaptureCorrelationReport {
    let mut observations = Vec::new();
    let mut issues = Vec::new();

    if options.min_owned_captures_per_summary == 0 {
        issues.push("min_owned_captures_per_summary must be greater than zero".to_string());
    }

    let raw_by_id = raw_rows
        .iter()
        .map(|row| (row.evidence_id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    for row in decoded_rows {
        let Some(raw) = raw_by_id.get(row.evidence_id.as_str()) else {
            issues.push(format!(
                "{} decoded frame has no raw evidence row {}",
                row.frame_id, row.evidence_id
            ));
            continue;
        };
        let parsed_payload: Option<ParsedPayload> =
            match serde_json::from_str(&row.parsed_payload_json) {
                Ok(parsed_payload) => parsed_payload,
                Err(source) => {
                    issues.push(format!(
                        "{} parsed_payload_json invalid: {source}",
                        row.frame_id
                    ));
                    continue;
                }
            };
        push_decoded_frame_observation(&mut observations, raw, row, parsed_payload.as_ref());
    }

    if observations.is_empty() {
        issues.push("no packet/event summaries found for capture correlation".to_string());
    }

    let summaries = summarize_observations(
        &observations,
        options.min_owned_captures_per_summary,
        options.require_owned_captures,
        &mut issues,
    );
    let next_capture_actions =
        report_next_capture_actions(&summaries, observations.is_empty(), &options);

    CaptureCorrelationReport {
        schema: CAPTURE_CORRELATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-capture-correlation".to_string(),
        fixture_root: evidence_scope.to_string(),
        pass: issues.is_empty(),
        min_owned_captures_per_summary: options.min_owned_captures_per_summary,
        require_owned_captures: options.require_owned_captures,
        observations,
        summaries,
        issues,
        next_capture_actions,
    }
}

fn observe_frame_fixture(
    root: &Path,
    fixture: &IndexedFixture,
    observations: &mut Vec<CaptureCorrelationObservation>,
    issues: &mut Vec<String>,
) {
    let path = root.join(&fixture.path);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            issues.push(format!("{} cannot be read: {source}", fixture.id));
            return;
        }
    };
    let device_type = fixture
        .expected
        .as_ref()
        .and_then(expected_device_type)
        .unwrap_or(DeviceType::Goose);
    let parsed =
        match decode_hex_with_whitespace(&raw).and_then(|bytes| parse_frame(device_type, &bytes)) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(format!("{} failed parsing: {error}", fixture.id));
                return;
            }
        };

    push_correlatable_observation(observations, fixture, &fixture.id, &fixture.path, &parsed);
}

fn observe_payload_fixture(
    root: &Path,
    fixture: &IndexedFixture,
    observations: &mut Vec<CaptureCorrelationObservation>,
    issues: &mut Vec<String>,
) {
    let path = root.join(&fixture.path);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            issues.push(format!("{} cannot be read: {source}", fixture.id));
            return;
        }
    };
    let device_type = fixture
        .expected
        .as_ref()
        .and_then(expected_device_type)
        .unwrap_or(DeviceType::Goose);
    let parsed = match decode_hex_with_whitespace(&raw)
        .map(|payload| build_v5_payload_frame(&payload))
        .and_then(|frame| parse_frame(device_type, &frame))
    {
        Ok(parsed) => parsed,
        Err(error) => {
            issues.push(format!("{} failed parsing: {error}", fixture.id));
            return;
        }
    };

    push_correlatable_observation(observations, fixture, &fixture.id, &fixture.path, &parsed);
}

fn observe_captured_frame_batch_fixture(
    root: &Path,
    fixture: &IndexedFixture,
    observations: &mut Vec<CaptureCorrelationObservation>,
    issues: &mut Vec<String>,
) {
    let path = root.join(&fixture.path);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            issues.push(format!("{} cannot be read: {source}", fixture.id));
            return;
        }
    };
    let batch: CapturedFrameBatchFixtureFile = match serde_json::from_str(&raw) {
        Ok(batch) => batch,
        Err(source) => {
            issues.push(format!(
                "{} cannot parse captured frame batch: {source}",
                fixture.id
            ));
            return;
        }
    };
    if batch.schema != CAPTURED_FRAME_BATCH_SCHEMA {
        issues.push(format!(
            "{} embedded schema must be {CAPTURED_FRAME_BATCH_SCHEMA}",
            fixture.id
        ));
        return;
    }
    if batch.frames.is_empty() {
        issues.push(format!("{} must include at least one frame", fixture.id));
        return;
    }

    for frame in &batch.frames {
        let parsed = match decode_hex_with_whitespace(&frame.frame_hex)
            .and_then(|bytes| parse_frame(frame.device_type, &bytes))
        {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(format!("{} failed parsing: {error}", frame.evidence_id));
                continue;
            }
        };
        let path = format!(
            "{}#{}",
            fixture.path,
            frame.frame_id.as_deref().unwrap_or("frame")
        );
        push_correlatable_observation(observations, fixture, &frame.evidence_id, &path, &parsed);
    }
}

fn push_correlatable_observation(
    observations: &mut Vec<CaptureCorrelationObservation>,
    fixture: &IndexedFixture,
    evidence_id: &str,
    path: &str,
    parsed: &ParsedFrame,
) {
    let synthetic = is_synthetic_fixture(fixture);
    let owned_capture = is_owned_capture_fixture(fixture, synthetic);
    match parsed.parsed_payload.as_ref() {
        Some(ParsedPayload::DataPacket {
            packet_k,
            domain,
            body_summary: Some(body_summary),
            warnings,
            ..
        }) => {
            observations.push(CaptureCorrelationObservation {
                fixture_id: fixture.id.clone(),
                evidence_id: evidence_id.to_string(),
                path: path.to_string(),
                kind: fixture.kind.clone(),
                source: fixture.source.clone(),
                captured_at: fixture.captured_at.clone(),
                device_model: fixture.device_model.clone(),
                synthetic,
                owned_capture,
                packet_type_name: parsed.packet_type_name.clone(),
                packet_k: *packet_k,
                domain: domain.clone(),
                body_summary_kind: body_summary_kind(body_summary).to_string(),
                warning_count: warnings.len(),
                warnings: warnings.clone(),
            });
        }
        Some(ParsedPayload::Event {
            event_id,
            event_name,
            warnings,
            ..
        }) => {
            let Some(summary_kind) = event_summary_kind(*event_id, event_name.as_deref()) else {
                return;
            };
            observations.push(CaptureCorrelationObservation {
                fixture_id: fixture.id.clone(),
                evidence_id: evidence_id.to_string(),
                path: path.to_string(),
                kind: fixture.kind.clone(),
                source: fixture.source.clone(),
                captured_at: fixture.captured_at.clone(),
                device_model: fixture.device_model.clone(),
                synthetic,
                owned_capture,
                packet_type_name: parsed.packet_type_name.clone(),
                packet_k: None,
                domain: Some("event".to_string()),
                body_summary_kind: summary_kind,
                warning_count: warnings.len(),
                warnings: warnings.clone(),
            });
        }
        _ => {}
    }
}

fn push_decoded_frame_observation(
    observations: &mut Vec<CaptureCorrelationObservation>,
    raw: &RawEvidenceRow,
    row: &DecodedFrameRow,
    parsed_payload: Option<&ParsedPayload>,
) {
    let synthetic = is_synthetic_values(&["decoded_frame", &raw.source, &raw.sensitivity]);
    let owned_capture =
        is_owned_capture_values("decoded_frame", &raw.source, &raw.sensitivity, synthetic);
    match parsed_payload {
        Some(ParsedPayload::DataPacket {
            packet_k,
            domain,
            body_summary: Some(body_summary),
            warnings,
            ..
        }) => {
            observations.push(CaptureCorrelationObservation {
                fixture_id: format!("sqlite:{}", row.frame_id),
                evidence_id: row.evidence_id.clone(),
                path: row.frame_id.clone(),
                kind: "decoded_frame".to_string(),
                source: raw.source.clone(),
                captured_at: raw.captured_at.clone(),
                device_model: raw.device_model.clone(),
                synthetic,
                owned_capture,
                packet_type_name: row.packet_type_name.clone(),
                packet_k: *packet_k,
                domain: domain.clone(),
                body_summary_kind: body_summary_kind(body_summary).to_string(),
                warning_count: warnings.len(),
                warnings: warnings.clone(),
            });
        }
        Some(ParsedPayload::Event {
            event_id,
            event_name,
            warnings,
            ..
        }) => {
            let Some(summary_kind) = event_summary_kind(*event_id, event_name.as_deref()) else {
                return;
            };
            observations.push(CaptureCorrelationObservation {
                fixture_id: format!("sqlite:{}", row.frame_id),
                evidence_id: row.evidence_id.clone(),
                path: row.frame_id.clone(),
                kind: "decoded_frame".to_string(),
                source: raw.source.clone(),
                captured_at: raw.captured_at.clone(),
                device_model: raw.device_model.clone(),
                synthetic,
                owned_capture,
                packet_type_name: row.packet_type_name.clone(),
                packet_k: None,
                domain: Some("event".to_string()),
                body_summary_kind: summary_kind,
                warning_count: warnings.len(),
                warnings: warnings.clone(),
            });
        }
        _ => {}
    }
}

fn summarize_observations(
    observations: &[CaptureCorrelationObservation],
    min_owned_captures_per_summary: usize,
    require_owned_captures: bool,
    issues: &mut Vec<String>,
) -> Vec<CaptureCorrelationSummary> {
    let mut accumulators = BTreeMap::<String, SummaryAccumulator>::new();
    for observation in observations {
        let accumulator = accumulators
            .entry(observation.body_summary_kind.clone())
            .or_default();
        accumulator.observation_count += 1;
        accumulator.warning_count += observation.warning_count;
        if observation.owned_capture {
            accumulator.owned_capture_count += 1;
        }
        if observation.synthetic {
            accumulator.synthetic_count += 1;
        }
    }

    accumulators
        .into_iter()
        .map(|(body_summary_kind, accumulator)| {
            let mut blocker_reasons = Vec::new();
            if accumulator.owned_capture_count < min_owned_captures_per_summary {
                blocker_reasons.push(format!(
                    "owned_capture_count {} below required {}",
                    accumulator.owned_capture_count, min_owned_captures_per_summary
                ));
            }
            let trusted_metric_ready = blocker_reasons.is_empty();
            if require_owned_captures && !trusted_metric_ready {
                issues.push(format!(
                    "{body_summary_kind} is not trusted for metric promotion: {}",
                    blocker_reasons.join("; ")
                ));
            }
            let next_capture_actions = next_capture_actions_for_summary(
                &body_summary_kind,
                accumulator.owned_capture_count,
                min_owned_captures_per_summary,
            );
            CaptureCorrelationSummary {
                body_summary_kind,
                observation_count: accumulator.observation_count,
                owned_capture_count: accumulator.owned_capture_count,
                synthetic_count: accumulator.synthetic_count,
                warning_count: accumulator.warning_count,
                trusted_metric_ready,
                blocker_reasons,
                next_capture_actions,
            }
        })
        .collect()
}

fn next_capture_actions_for_summary(
    body_summary_kind: &str,
    owned_capture_count: usize,
    min_owned_captures_per_summary: usize,
) -> Vec<CaptureCorrelationNextAction> {
    if owned_capture_count >= min_owned_captures_per_summary {
        return Vec::new();
    }
    let missing = min_owned_captures_per_summary - owned_capture_count;
    vec![CaptureCorrelationNextAction {
        scope: body_summary_kind.to_string(),
        reason: format!(
            "owned_capture_count {owned_capture_count} below required {min_owned_captures_per_summary}"
        ),
        action: capture_action_text(body_summary_kind, missing),
    }]
}

fn capture_action_text(body_summary_kind: &str, missing: usize) -> String {
    let frame_plural = if missing == 1 { "frame" } else { "frames" };
    match body_summary_kind {
        "r17_optical_or_labrador_filtered" => format!(
            "Capture {missing} more user-owned {body_summary_kind} {frame_plural} from an official optical/ECG raw-stream session, or import a sanitized Files capture containing K17/R20 notifications, then rerun Capture Trust."
        ),
        "event_temperature_level" => format!(
            "Capture {missing} more user-owned {body_summary_kind} {frame_plural} during an official history or temperature-event sync, or import a sanitized Files capture containing TEMPERATURE_LEVEL event 17, then rerun Capture Trust."
        ),
        _ => format!(
            "Capture {missing} more user-owned {body_summary_kind} {frame_plural} through live BLE notification capture or Files import, then rerun Capture Trust."
        ),
    }
}

fn report_next_capture_actions(
    summaries: &[CaptureCorrelationSummary],
    no_observations: bool,
    options: &CaptureCorrelationOptions,
) -> Vec<CaptureCorrelationNextAction> {
    let mut actions = Vec::new();
    if options.min_owned_captures_per_summary == 0 {
        actions.push(CaptureCorrelationNextAction {
            scope: "options".to_string(),
            reason: "min_owned_captures_per_summary must be greater than zero".to_string(),
            action: "Set min_owned_captures_per_summary to at least 1 before trusting capture correlation.".to_string(),
        });
    }
    if no_observations {
        actions.push(CaptureCorrelationNextAction {
            scope: "capture_correlation".to_string(),
            reason: "no packet/event summaries found for capture correlation".to_string(),
            action: "Import owned WHOOP frames or sanitized capture fixtures that decode to data-packet/event summaries, then rerun Capture Trust.".to_string(),
        });
    }
    actions.extend(
        summaries
            .iter()
            .flat_map(|summary| summary.next_capture_actions.clone()),
    );
    actions
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

fn is_synthetic_fixture(fixture: &IndexedFixture) -> bool {
    is_synthetic_values(&[
        fixture.kind.as_str(),
        fixture.source.as_str(),
        fixture.consent.as_str(),
        fixture.sensitivity.as_str(),
    ])
}

fn is_owned_capture_fixture(fixture: &IndexedFixture, synthetic: bool) -> bool {
    is_owned_capture_values(
        &fixture.kind,
        &fixture.source,
        &fixture.sensitivity,
        synthetic,
    )
}

fn is_synthetic_values(values: &[&str]) -> bool {
    values
        .iter()
        .any(|value| value.to_ascii_lowercase().contains("synthetic"))
}

fn is_owned_capture_values(kind: &str, source: &str, sensitivity: &str, synthetic: bool) -> bool {
    if synthetic {
        return false;
    }
    let joined = format!("{kind} {source} {sensitivity}").to_ascii_lowercase();
    if joined.contains("private_api_replay") || joined.contains("private-api-replay") {
        return false;
    }
    joined.contains("user-owned")
        || joined.contains("owned")
        || joined.contains("corebluetooth")
        || joined.contains("notification")
}

fn body_summary_kind(summary: &DataPacketBodySummary) -> &'static str {
    match summary {
        DataPacketBodySummary::NormalHistory { .. } => "normal_history",
        DataPacketBodySummary::R17OpticalOrLabradorFiltered { .. } => {
            "r17_optical_or_labrador_filtered"
        }
        DataPacketBodySummary::RawMotionK10 { .. } => "raw_motion_k10",
        DataPacketBodySummary::RawMotionK21 { .. } => "raw_motion_k21",
        DataPacketBodySummary::V24History { .. } => "v24_history",
        DataPacketBodySummary::R22Whoop5Hr { .. } => "r22_whoop5_hr",
        DataPacketBodySummary::V18History { .. } => "v18_history",
    }
}

fn event_summary_kind(event_id: Option<u16>, event_name: Option<&str>) -> Option<String> {
    if event_id == Some(17) || event_name == Some("TEMPERATURE_LEVEL") {
        Some("event_temperature_level".to_string())
    } else {
        None
    }
}
