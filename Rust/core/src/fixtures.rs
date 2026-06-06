use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    GooseError, GooseResult,
    protocol::{
        DeviceType, ParsedFrame, build_v5_payload_frame, decode_hex_with_whitespace, parse_frame,
    },
};

pub const FIXTURE_INDEX_SCHEMA: &str = "goose.fixture-index.v1";
pub const FRAME_HEX_SCHEMA: &str = "goose.frame.hex.v1";
pub const CAPTURED_FRAME_BATCH_SCHEMA: &str = "goose.captured-frame-batch.v1";
pub const PAYLOAD_HEX_SCHEMA: &str = "goose.payload.hex.v1";
pub const ACTIVITY_SESSION_FIXTURE_SCHEMA: &str = "goose.activity-session-fixtures.v1";
pub const OPENWHOOP_REFERENCE_FIXTURE_SCHEMA: &str = "goose.openwhoop-reference-fixture.v1";
pub const COMMAND_VALIDATION_FIXTURE_SCHEMA: &str = "goose.command-validation-fixtures.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureMetadata {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub device_firmware: String,
    pub app_version: String,
    pub schema: String,
    pub consent: String,
    pub sensitivity: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFixture {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub device_firmware: String,
    pub app_version: String,
    pub schema: String,
    pub consent: String,
    pub sensitivity: String,
    pub notes: String,
    pub checksum_sha256: String,
    pub byte_len: u64,
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureIndexReport {
    pub schema: String,
    pub generated_by: String,
    pub fixture_root: String,
    pub pass: bool,
    pub fixtures: Vec<IndexedFixture>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<FixtureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserFixtureReport {
    pub schema: String,
    pub generated_by: String,
    pub fixture_root: String,
    pub pass: bool,
    pub fixtures: Vec<ParserFixtureResult>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<FixtureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserFixtureResult {
    pub id: String,
    pub path: String,
    pub schema: String,
    pub pass: bool,
    pub parsed: Option<ParsedFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parsed_frames: Vec<ParsedFrame>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<FixtureNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FixtureNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapturedFrameBatchFixture {
    pub schema: String,
    pub frames: Vec<CapturedFrameFixtureFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapturedFrameFixtureFrame {
    pub evidence_id: String,
    #[serde(default)]
    pub frame_id: Option<String>,
    pub source: String,
    pub captured_at: String,
    pub device_model: String,
    pub frame_hex: String,
    pub sensitivity: String,
    #[serde(default = "default_device_type")]
    pub device_type: DeviceType,
}

pub fn build_fixture_index(root: &Path) -> GooseResult<FixtureIndexReport> {
    let mut sidecars = Vec::new();
    collect_sidecars(root, &mut sidecars)?;
    sidecars.sort();

    let mut fixtures = Vec::new();
    let mut issues = Vec::new();
    let mut ids = BTreeSet::new();
    let mut covered_paths = BTreeSet::new();

    for sidecar in sidecars {
        let raw =
            fs::read_to_string(&sidecar).map_err(|source| GooseError::io(&sidecar, source))?;
        let metadata: FixtureMetadata =
            serde_json::from_str(&raw).map_err(|source| GooseError::json(&sidecar, source))?;
        validate_metadata(&metadata, &sidecar, &mut issues);

        if !ids.insert(metadata.id.clone()) {
            issues.push(format!("duplicate fixture id: {}", metadata.id));
        }

        let relative_path = PathBuf::from(&metadata.path);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            issues.push(format!(
                "{} has unsafe relative fixture path {}",
                metadata.id, metadata.path
            ));
            continue;
        }

        let content_path = root.join(&relative_path);
        covered_paths.insert(relative_path);
        let bytes = match fs::read(&content_path) {
            Ok(bytes) => bytes,
            Err(source) => {
                issues.push(format!(
                    "{} content file cannot be read at {}: {source}",
                    metadata.id,
                    content_path.display()
                ));
                Vec::new()
            }
        };

        fixtures.push(IndexedFixture {
            id: metadata.id,
            path: metadata.path,
            kind: metadata.kind,
            source: metadata.source,
            captured_at: metadata.captured_at,
            device_model: metadata.device_model,
            device_firmware: metadata.device_firmware,
            app_version: metadata.app_version,
            schema: metadata.schema,
            consent: metadata.consent,
            sensitivity: metadata.sensitivity,
            notes: metadata.notes,
            checksum_sha256: sha256_hex(&bytes),
            byte_len: bytes.len() as u64,
            expected: metadata.expected,
        });
    }

    let mut data_files = Vec::new();
    collect_data_files(root, &mut data_files)?;
    for data_file in data_files {
        let relative = relative_path(root, &data_file);
        if !covered_paths.contains(&relative) {
            issues.push(format!(
                "fixture data file has no .fixture.json sidecar: {}",
                relative.display()
            ));
        }
    }

    let next_actions = fixture_index_next_actions(&issues);

    Ok(FixtureIndexReport {
        schema: FIXTURE_INDEX_SCHEMA.to_string(),
        generated_by: "goose-fixture-index".to_string(),
        fixture_root: root.display().to_string(),
        pass: issues.is_empty(),
        fixtures,
        issues,
        next_actions,
    })
}

pub fn run_parser_fixtures(root: &Path, index: &FixtureIndexReport) -> ParserFixtureReport {
    let mut results = Vec::new();
    let mut issues = Vec::new();

    for fixture in &index.fixtures {
        let result = match fixture.schema.as_str() {
            FRAME_HEX_SCHEMA => Some(run_frame_fixture(root, fixture)),
            PAYLOAD_HEX_SCHEMA => Some(run_payload_fixture(root, fixture)),
            CAPTURED_FRAME_BATCH_SCHEMA => Some(run_captured_frame_batch_fixture(root, fixture)),
            _ => None,
        };
        let Some(result) = result else {
            continue;
        };
        if !result.pass {
            issues.push(format!("{} failed parser validation", fixture.id));
        }
        results.push(result);
    }

    if results.is_empty() {
        issues.push("no goose.frame.hex.v1 fixtures found".to_string());
    }

    let next_actions = parser_fixture_report_next_actions(&issues, &results);

    ParserFixtureReport {
        schema: "goose.parser-fixture-report.v1".to_string(),
        generated_by: "goose-parser-fixture-runner".to_string(),
        fixture_root: root.display().to_string(),
        pass: issues.is_empty(),
        fixtures: results,
        issues,
        next_actions,
    }
}

pub fn load_fixture_index(path: &Path) -> GooseResult<FixtureIndexReport> {
    let raw = fs::read_to_string(path).map_err(|source| GooseError::io(path, source))?;
    serde_json::from_str(&raw).map_err(|source| GooseError::json(path, source))
}

fn run_frame_fixture(root: &Path, fixture: &IndexedFixture) -> ParserFixtureResult {
    let mut issues = Vec::new();
    let mut parsed = None;
    let path = root.join(&fixture.path);

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            let issues = vec![format!("cannot read fixture file: {source}")];
            return ParserFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                schema: fixture.schema.clone(),
                pass: false,
                parsed: None,
                parsed_frames: Vec::new(),
                next_actions: fixture_next_actions(&fixture.id, &issues),
                issues,
            };
        }
    };

    let expected = fixture.expected.clone().unwrap_or_default();
    let device_type = expected_device_type(&expected).unwrap_or(DeviceType::Goose);

    match decode_hex_with_whitespace(&raw).and_then(|bytes| parse_frame(device_type, &bytes)) {
        Ok(frame) => {
            compare_expected_frame(&fixture.id, &frame, &expected, &mut issues);
            parsed = Some(frame);
        }
        Err(error) => {
            issues.push(error.to_string());
        }
    }

    ParserFixtureResult {
        id: fixture.id.clone(),
        path: fixture.path.clone(),
        schema: fixture.schema.clone(),
        pass: issues.is_empty(),
        parsed,
        parsed_frames: Vec::new(),
        next_actions: fixture_next_actions(&fixture.id, &issues),
        issues,
    }
}

fn run_payload_fixture(root: &Path, fixture: &IndexedFixture) -> ParserFixtureResult {
    let mut issues = Vec::new();
    let mut parsed = None;
    let path = root.join(&fixture.path);

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            let issues = vec![format!("cannot read fixture file: {source}")];
            return ParserFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                schema: fixture.schema.clone(),
                pass: false,
                parsed: None,
                parsed_frames: Vec::new(),
                next_actions: fixture_next_actions(&fixture.id, &issues),
                issues,
            };
        }
    };

    let expected = fixture.expected.clone().unwrap_or_default();
    let device_type = expected_device_type(&expected).unwrap_or(DeviceType::Goose);

    match decode_hex_with_whitespace(&raw)
        .map(|payload| build_v5_payload_frame(&payload))
        .and_then(|frame| parse_frame(device_type, &frame))
    {
        Ok(frame) => {
            compare_expected_frame(&fixture.id, &frame, &expected, &mut issues);
            parsed = Some(frame);
        }
        Err(error) => {
            issues.push(error.to_string());
        }
    }

    ParserFixtureResult {
        id: fixture.id.clone(),
        path: fixture.path.clone(),
        schema: fixture.schema.clone(),
        pass: issues.is_empty(),
        parsed,
        parsed_frames: Vec::new(),
        next_actions: fixture_next_actions(&fixture.id, &issues),
        issues,
    }
}

fn run_captured_frame_batch_fixture(root: &Path, fixture: &IndexedFixture) -> ParserFixtureResult {
    let mut issues = Vec::new();
    let mut parsed_frames = Vec::new();
    let path = root.join(&fixture.path);

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(source) => {
            let issues = vec![format!("cannot read fixture file: {source}")];
            return ParserFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                schema: fixture.schema.clone(),
                pass: false,
                parsed: None,
                parsed_frames,
                next_actions: fixture_next_actions(&fixture.id, &issues),
                issues,
            };
        }
    };

    let batch: CapturedFrameBatchFixture = match serde_json::from_str(&raw) {
        Ok(batch) => batch,
        Err(source) => {
            let issues = vec![format!("cannot parse captured frame batch: {source}")];
            return ParserFixtureResult {
                id: fixture.id.clone(),
                path: fixture.path.clone(),
                schema: fixture.schema.clone(),
                pass: false,
                parsed: None,
                parsed_frames,
                next_actions: fixture_next_actions(&fixture.id, &issues),
                issues,
            };
        }
    };

    if batch.schema != CAPTURED_FRAME_BATCH_SCHEMA {
        issues.push(format!(
            "{} embedded schema must be {CAPTURED_FRAME_BATCH_SCHEMA}",
            fixture.id
        ));
    }
    if batch.frames.is_empty() {
        issues.push(format!("{} must include at least one frame", fixture.id));
    }

    for frame in &batch.frames {
        match decode_hex_with_whitespace(&frame.frame_hex)
            .and_then(|bytes| parse_frame(frame.device_type, &bytes))
        {
            Ok(parsed) => parsed_frames.push(parsed),
            Err(error) => issues.push(format!("{} failed parsing: {error}", frame.evidence_id)),
        }
    }

    let expected = fixture.expected.clone().unwrap_or_default();
    compare_expected_batch(&fixture.id, &parsed_frames, &expected, &mut issues);

    ParserFixtureResult {
        id: fixture.id.clone(),
        path: fixture.path.clone(),
        schema: fixture.schema.clone(),
        pass: issues.is_empty(),
        parsed: None,
        parsed_frames,
        next_actions: fixture_next_actions(&fixture.id, &issues),
        issues,
    }
}

fn compare_expected_batch(
    id: &str,
    frames: &[ParsedFrame],
    expected: &serde_json::Value,
    issues: &mut Vec<String>,
) {
    if let Some(expected_frame_count) = expected.get("frame_count").and_then(|value| value.as_u64())
        && frames.len() != expected_frame_count as usize
    {
        issues.push(format!(
            "{id} expected frame_count={expected_frame_count}, got {}",
            frames.len()
        ));
    }

    compare_expected_string_sequence(
        id,
        "packet_type_names",
        frames
            .iter()
            .map(|frame| frame.packet_type_name.as_deref().unwrap_or(""))
            .collect(),
        expected,
        issues,
    );
    compare_expected_string_sequence(
        id,
        "parsed_payload_kinds",
        frames
            .iter()
            .map(|frame| {
                frame
                    .parsed_payload
                    .as_ref()
                    .map(parsed_payload_kind)
                    .unwrap_or_else(|| "none".to_string())
            })
            .collect(),
        expected,
        issues,
    );
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

fn compare_expected_frame(
    id: &str,
    frame: &ParsedFrame,
    expected: &serde_json::Value,
    issues: &mut Vec<String>,
) {
    let expected_fields: BTreeMap<&str, Option<u8>> = BTreeMap::from([
        ("packet_type", frame.packet_type),
        ("sequence", frame.sequence),
        ("command_or_event", frame.command_or_event),
    ]);

    for (field, actual) in expected_fields {
        let Some(expected_value) = expected.get(field).and_then(|value| value.as_u64()) else {
            issues.push(format!("{id} missing expected.{field}"));
            continue;
        };
        if actual != Some(expected_value as u8) {
            issues.push(format!(
                "{id} expected {field}={expected_value}, got {:?}",
                actual
            ));
        }
    }

    compare_expected_bool(
        id,
        "header_crc_valid",
        frame.header_crc_valid,
        expected,
        issues,
    );
    compare_expected_bool(
        id,
        "payload_crc_valid",
        frame.payload_crc_valid,
        expected,
        issues,
    );

    if let Some(expected_payload) = expected.get("payload_hex").and_then(|value| value.as_str())
        && frame.payload_hex != expected_payload
    {
        issues.push(format!(
            "{id} expected payload_hex={expected_payload}, got {}",
            frame.payload_hex
        ));
    }
    if let Some(expected_packet_type_name) = expected
        .get("packet_type_name")
        .and_then(|value| value.as_str())
        && frame.packet_type_name.as_deref() != Some(expected_packet_type_name)
    {
        issues.push(format!(
            "{id} expected packet_type_name={expected_packet_type_name}, got {:?}",
            frame.packet_type_name
        ));
    }
    if let Some(expected_payload) = expected.get("parsed_payload") {
        let actual_payload =
            serde_json::to_value(&frame.parsed_payload).unwrap_or(serde_json::Value::Null);
        compare_expected_json_subset(
            id,
            "parsed_payload",
            &actual_payload,
            expected_payload,
            issues,
        );
    }
}

fn compare_expected_json_subset(
    id: &str,
    path: &str,
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    issues: &mut Vec<String>,
) {
    match expected {
        serde_json::Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                issues.push(format!(
                    "{id} expected {path} to be an object, got {actual}"
                ));
                return;
            };
            for (key, expected_value) in expected_map {
                let child_path = format!("{path}.{key}");
                match actual_map.get(key) {
                    Some(actual_value) => compare_expected_json_subset(
                        id,
                        &child_path,
                        actual_value,
                        expected_value,
                        issues,
                    ),
                    None => issues.push(format!("{id} missing {child_path}")),
                }
            }
        }
        _ => {
            if actual != expected {
                issues.push(format!("{id} expected {path}={expected}, got {actual}"));
            }
        }
    }
}

fn compare_expected_string_sequence(
    id: &str,
    field: &str,
    actual: Vec<impl AsRef<str>>,
    expected: &serde_json::Value,
    issues: &mut Vec<String>,
) {
    let Some(expected_values) = expected.get(field).and_then(|value| value.as_array()) else {
        issues.push(format!("{id} missing expected.{field}"));
        return;
    };
    let expected_strings = expected_values
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if expected_strings.len() != expected_values.len() {
        issues.push(format!("{id} expected.{field} must contain only strings"));
        return;
    }
    let actual_strings = actual
        .iter()
        .map(|value| value.as_ref().to_string())
        .collect::<Vec<_>>();
    if actual_strings != expected_strings {
        issues.push(format!(
            "{id} expected {field}={expected_strings:?}, got {actual_strings:?}"
        ));
    }
}

fn compare_expected_bool(
    id: &str,
    field: &str,
    actual: bool,
    expected: &serde_json::Value,
    issues: &mut Vec<String>,
) {
    let Some(expected_value) = expected.get(field).and_then(|value| value.as_bool()) else {
        issues.push(format!("{id} missing expected.{field}"));
        return;
    };
    if actual != expected_value {
        issues.push(format!(
            "{id} expected {field}={expected_value}, got {actual}"
        ));
    }
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

fn validate_metadata(metadata: &FixtureMetadata, sidecar: &Path, issues: &mut Vec<String>) {
    let required = [
        ("id", metadata.id.as_str()),
        ("path", metadata.path.as_str()),
        ("kind", metadata.kind.as_str()),
        ("source", metadata.source.as_str()),
        ("captured_at", metadata.captured_at.as_str()),
        ("device_model", metadata.device_model.as_str()),
        ("device_firmware", metadata.device_firmware.as_str()),
        ("app_version", metadata.app_version.as_str()),
        ("schema", metadata.schema.as_str()),
        ("consent", metadata.consent.as_str()),
        ("sensitivity", metadata.sensitivity.as_str()),
    ];

    for (field, value) in required {
        if value.trim().is_empty() {
            issues.push(format!(
                "{} missing required field {field}",
                sidecar.display()
            ));
        }
    }
}

fn fixture_index_next_actions(issues: &[String]) -> Vec<FixtureNextAction> {
    dedupe_fixture_next_actions(
        issues
            .iter()
            .map(|issue| {
                let (reason, action) = fixture_index_issue_action(issue);
                FixtureNextAction {
                    scope: fixture_index_issue_scope(issue),
                    reason: reason.to_string(),
                    action: action.to_string(),
                }
            })
            .collect(),
    )
}

fn fixture_index_issue_scope(issue: &str) -> String {
    if let Some(scope) = issue.split(" missing required field ").next()
        && issue.contains(" missing required field ")
    {
        return scope.to_string();
    }
    if let Some(id) = issue.strip_prefix("duplicate fixture id: ") {
        return id.to_string();
    }
    if let Some((id, _)) = issue.split_once(" has unsafe relative fixture path ") {
        return id.to_string();
    }
    if let Some((id, _)) = issue.split_once(" content file cannot be read at ") {
        return id.to_string();
    }
    if let Some(path) = issue.strip_prefix("fixture data file has no .fixture.json sidecar: ") {
        return path.to_string();
    }
    "fixture_index".to_string()
}

fn fixture_index_issue_action(issue: &str) -> (&'static str, &'static str) {
    if issue.contains(" missing required field ") {
        (
            "missing_metadata_field",
            "Fill every required fixture sidecar metadata field before trusting the fixture.",
        )
    } else if issue.starts_with("duplicate fixture id: ") {
        (
            "duplicate_fixture_id",
            "Assign a globally unique fixture id so parser and import reports can be traced.",
        )
    } else if issue.contains(" has unsafe relative fixture path ") {
        (
            "unsafe_fixture_path",
            "Change the fixture sidecar path to a relative path inside the fixture root.",
        )
    } else if issue.contains(" content file cannot be read at ") {
        (
            "content_file_unreadable",
            "Restore the referenced fixture content file or update the sidecar path.",
        )
    } else if issue.starts_with("fixture data file has no .fixture.json sidecar: ") {
        (
            "missing_sidecar",
            "Add a .fixture.json sidecar with source, capture date, device/app version, schema, consent, sensitivity, and checksum context.",
        )
    } else {
        (
            "fixture_index_issue",
            "Inspect the fixture index issue, repair the sidecar or content file, and regenerate the fixture index.",
        )
    }
}

fn parser_fixture_report_next_actions(
    issues: &[String],
    results: &[ParserFixtureResult],
) -> Vec<FixtureNextAction> {
    let mut actions = Vec::new();
    for result in results {
        if !result.pass {
            actions.extend(result.next_actions.iter().cloned());
        }
    }
    for issue in issues {
        if issue == "no goose.frame.hex.v1 fixtures found" {
            actions.push(FixtureNextAction {
                scope: "parser_fixtures".to_string(),
                reason: "no_parser_fixtures".to_string(),
                action: "Add at least one goose.frame.hex.v1, goose.payload.hex.v1, or goose.captured-frame-batch.v1 fixture and regenerate the fixture index.".to_string(),
            });
        } else if issue.ends_with(" failed parser validation") {
            let fixture_id = issue
                .strip_suffix(" failed parser validation")
                .unwrap_or("parser_fixtures");
            if !results
                .iter()
                .any(|result| result.id == fixture_id && !result.next_actions.is_empty())
            {
                actions.push(FixtureNextAction {
                    scope: fixture_id.to_string(),
                    reason: "parser_fixture_failed".to_string(),
                    action: "Inspect the fixture-level parser issues and add the missing parser, expected field, or fixture repair.".to_string(),
                });
            }
        } else {
            actions.push(FixtureNextAction {
                scope: "parser_fixtures".to_string(),
                reason: "parser_fixture_report_issue".to_string(),
                action: "Inspect the parser fixture report issue and repair the fixture/index inputs before trusting parser coverage.".to_string(),
            });
        }
    }
    dedupe_fixture_next_actions(actions)
}

fn fixture_next_actions(id: &str, issues: &[String]) -> Vec<FixtureNextAction> {
    dedupe_fixture_next_actions(
        issues
            .iter()
            .map(|issue| {
                let (reason, action) = parser_fixture_issue_action(issue);
                FixtureNextAction {
                    scope: id.to_string(),
                    reason: reason.to_string(),
                    action: action.to_string(),
                }
            })
            .collect(),
    )
}

fn parser_fixture_issue_action(issue: &str) -> (&'static str, &'static str) {
    if issue.starts_with("cannot read fixture file:") {
        (
            "fixture_file_unreadable",
            "Restore the fixture file or update the indexed path, then regenerate the fixture index.",
        )
    } else if issue.starts_with("cannot parse captured frame batch:") {
        (
            "captured_frame_batch_json_invalid",
            "Regenerate the sanitized captured-frame batch as valid JSON with preserved frame bytes.",
        )
    } else if issue.contains(" embedded schema must be ") {
        (
            "captured_frame_batch_schema_invalid",
            "Update the captured-frame batch schema to goose.captured-frame-batch.v1 or add a migration before trusting it.",
        )
    } else if issue.contains(" must include at least one frame") {
        (
            "captured_frame_batch_empty",
            "Add at least one captured frame with provenance before using this batch as parser evidence.",
        )
    } else if issue.contains("hex decode error") {
        (
            "frame_hex_invalid",
            "Repair the fixture hex string or regenerate the sanitized capture without altering the original evidence.",
        )
    } else if issue.contains("frame does not start with 0xaa")
        || issue.contains("frame shorter than")
        || issue.contains("declared length")
        || issue.contains("frame length")
        || issue.contains(" failed parsing:")
    {
        (
            "frame_parse_failed",
            "Compare the raw bytes against the official capture, then fix the parser or fixture without dropping unknown bytes.",
        )
    } else if issue.contains("expected.") && issue.contains("must contain only strings") {
        (
            "expected_sequence_invalid",
            "Rewrite the expected sequence as an array of strings derived from trusted parser evidence.",
        )
    } else if issue.contains(" missing parsed_payload")
        || issue.contains(" expected parsed_payload")
    {
        (
            "parsed_payload_subset_missing",
            "Fix the parsed_payload subset expectation or the parser output using trusted capture evidence.",
        )
    } else if issue.contains(" missing expected.") {
        (
            "expected_field_missing",
            "Add the missing expected field from a hand-derived case or trusted captured fixture, not from a failing run alone.",
        )
    } else if issue.contains(" expected ") && issue.contains(", got ") {
        (
            "expected_value_mismatch",
            "Resolve the expected-value mismatch by checking the parser against trusted evidence before changing expected output.",
        )
    } else {
        (
            "parser_fixture_issue",
            "Inspect the parser fixture issue and add the missing parser, expected field, or fixture repair.",
        )
    }
}

fn dedupe_fixture_next_actions(actions: Vec<FixtureNextAction>) -> Vec<FixtureNextAction> {
    actions
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_sidecars(root: &Path, sidecars: &mut Vec<PathBuf>) -> GooseResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|source| GooseError::io(root, source))? {
        let entry = entry.map_err(|source| GooseError::io(root, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_sidecars(&path, sidecars)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".fixture.json"))
        {
            sidecars.push(path);
        }
    }
    Ok(())
}

fn collect_data_files(root: &Path, files: &mut Vec<PathBuf>) -> GooseResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|source| GooseError::io(root, source))? {
        let entry = entry.map_err(|source| GooseError::io(root, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_data_files(&path, files)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                !name.ends_with(".fixture.json") && name != "index.json" && name != "README.md"
            })
        {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
