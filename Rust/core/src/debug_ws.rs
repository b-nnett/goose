use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    GooseError, GooseResult,
    store::{DebugCommandRow, DebugEventRow, DebugSessionRow, GooseStore},
};

pub const DEBUG_WS_CONTRACT_SCHEMA: &str = "goose.debug-ws-contract.v1";
pub const DEBUG_EVENT_SCHEMA: &str = "goose.debug.event.v1";
pub const DEBUG_COMMAND_SCHEMA: &str = "goose.debug.command.v1";
pub const DEBUG_EVENT_TOPIC_ACTIVITY_FEATURE_WINDOW_CREATED: &str =
    "activity.feature.window.created";
pub const DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CREATED: &str = "activity.candidate.created";
pub const DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_PROMOTED: &str = "activity.candidate.promoted";
pub const DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CORRECTED: &str = "activity.candidate.corrected";
pub const DEBUG_EVENT_TOPIC_ACTIVITY_SESSION_STATS_DISPLAYED: &str =
    "activity.session.stats.displayed";
pub const DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_PLANNED: &str = "export.raw_timeframe.planned";
pub const DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_COMPLETED: &str = "export.raw_timeframe.completed";
pub const DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_PLANNED: &str = "health_sync.activity.planned";
pub const DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_BLOCKED: &str = "health_sync.activity.blocked";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugWsContractInput {
    pub schema: String,
    pub bridge: DebugBridgeConfig,
    pub commands: Vec<DebugCommandEnvelope>,
    pub events: Vec<DebugEventEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugBridgeConfig {
    pub url: String,
    pub bind_host: String,
    pub token_required: bool,
    pub token_present: bool,
    #[serde(default)]
    pub remote_bind_enabled: bool,
    #[serde(default)]
    pub visible_remote_bind_toggle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugCommandEnvelope {
    pub schema: String,
    pub command_id: String,
    pub command: String,
    pub args: serde_json::Value,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugEventEnvelope {
    pub schema: String,
    pub session_id: String,
    pub time_unix_ms: u64,
    pub sequence: u64,
    pub source: String,
    pub level: String,
    pub topic: String,
    pub message: String,
    #[serde(default)]
    pub command_id: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugWsContractReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub bridge_valid: bool,
    #[serde(default)]
    pub commands_valid: bool,
    #[serde(default)]
    pub events_valid: bool,
    #[serde(default)]
    pub stream_order_valid: bool,
    #[serde(default)]
    pub command_references_valid: bool,
    #[serde(default)]
    pub command_results_correlated: bool,
    #[serde(default)]
    pub contract_ready: bool,
    pub command_count: usize,
    pub event_count: usize,
    pub command_results: Vec<DebugCommandResultStatus>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<DebugWsNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DebugWsNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugCommandResultStatus {
    pub command_id: String,
    pub command: String,
    pub started: bool,
    pub result: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugSessionStartInput {
    pub session_id: String,
    pub started_at_unix_ms: u64,
    pub bridge: DebugBridgeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugCommandStartInput {
    pub session_id: String,
    pub received_at_unix_ms: u64,
    pub command: DebugCommandEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugCommandFinishInput {
    pub session_id: String,
    pub time_unix_ms: u64,
    pub command_id: String,
    pub ok: bool,
    pub message: String,
    #[serde(default = "empty_object")]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugEventInput {
    pub session_id: String,
    pub time_unix_ms: u64,
    pub source: String,
    pub level: String,
    pub topic: String,
    pub message: String,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default = "empty_object")]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugSessionSnapshot {
    pub schema: String,
    pub session_id: String,
    pub bridge: DebugBridgeConfig,
    pub commands: Vec<DebugCommandEnvelope>,
    pub events: Vec<DebugEventEnvelope>,
    pub contract_report: DebugWsContractReport,
}

pub fn validate_debug_ws_contract(input: &DebugWsContractInput) -> DebugWsContractReport {
    let mut issues = Vec::new();

    if input.schema != DEBUG_WS_CONTRACT_SCHEMA {
        issues.push(format!("unsupported_schema:{}", input.schema));
    }
    validate_bridge(&input.bridge, &mut issues);

    let mut command_ids = BTreeSet::new();
    let mut command_by_id = BTreeMap::new();
    for command in &input.commands {
        validate_command(command, &mut command_ids, &mut issues);
        if !command.command_id.trim().is_empty() {
            command_by_id.insert(command.command_id.as_str(), command);
        }
    }

    let mut previous_sequence = None;
    let mut previous_time = None;
    let mut command_topics: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for event in &input.events {
        validate_event(
            event,
            &command_by_id,
            &mut previous_sequence,
            &mut previous_time,
            &mut command_topics,
            &mut issues,
        );
    }

    let command_results = input
        .commands
        .iter()
        .map(|command| {
            let topics = command_topics
                .get(command.command_id.as_str())
                .cloned()
                .unwrap_or_default();
            let started = topics.contains("command.started");
            let result = topics.contains("command.result");
            if !started {
                issues.push(format!(
                    "command_missing_started_event:{}",
                    command.command_id
                ));
            }
            if !result {
                issues.push(format!(
                    "command_missing_result_event:{}",
                    command.command_id
                ));
            }
            DebugCommandResultStatus {
                command_id: command.command_id.clone(),
                command: command.command.clone(),
                started,
                result,
            }
        })
        .collect();

    issues.sort();
    issues.dedup();
    let next_actions = debug_ws_next_actions(&issues);
    let input_valid = !issues
        .iter()
        .any(|issue| issue.starts_with("unsupported_schema"));
    let bridge_valid = !issues.iter().any(|issue| debug_bridge_issue(issue));
    let commands_valid = !issues.iter().any(|issue| debug_command_schema_issue(issue));
    let events_valid = !issues.iter().any(|issue| debug_event_schema_issue(issue));
    let stream_order_valid = !issues.iter().any(|issue| debug_stream_order_issue(issue));
    let command_references_valid = !issues
        .iter()
        .any(|issue| debug_command_reference_issue(issue));
    let command_results_correlated = !issues.iter().any(|issue| debug_command_result_issue(issue));
    let contract_ready = input_valid
        && bridge_valid
        && commands_valid
        && events_valid
        && stream_order_valid
        && command_references_valid
        && command_results_correlated
        && issues.is_empty();
    let pass = contract_ready;

    DebugWsContractReport {
        schema: "goose.debug-ws-contract-report.v1".to_string(),
        generated_by: "goose-debug-ws-contract".to_string(),
        pass,
        input_valid,
        bridge_valid,
        commands_valid,
        events_valid,
        stream_order_valid,
        command_references_valid,
        command_results_correlated,
        contract_ready,
        command_count: input.commands.len(),
        event_count: input.events.len(),
        command_results,
        issues,
        next_actions,
    }
}

fn debug_bridge_issue(issue: &str) -> bool {
    issue.starts_with("bridge_") || issue == "remote_bind_requires_visible_toggle"
}

fn debug_command_schema_issue(issue: &str) -> bool {
    issue.starts_with("command_schema_invalid:")
        || issue == "command_id_required"
        || issue.starts_with("duplicate_command_id:")
        || issue.starts_with("command_name_required:")
        || issue.starts_with("command_args_must_be_object:")
}

fn debug_event_schema_issue(issue: &str) -> bool {
    issue.starts_with("event_schema_invalid:")
        || issue.starts_with("event_session_id_required:")
        || issue.starts_with("event_source_invalid:")
        || issue.starts_with("event_level_invalid:")
        || issue.starts_with("event_topic_required:")
        || issue.starts_with("event_message_required:")
        || issue.starts_with("event_data_must_be_object:")
}

fn debug_stream_order_issue(issue: &str) -> bool {
    issue.starts_with("event_sequence_not_strictly_increasing:")
        || issue.starts_with("event_time_decreased:")
}

fn debug_command_reference_issue(issue: &str) -> bool {
    issue.starts_with("command_event_command_id_required:")
        || issue.starts_with("command_event_unknown_command_id:")
        || issue.starts_with("event_unknown_command_id:")
}

fn debug_command_result_issue(issue: &str) -> bool {
    issue.starts_with("command_missing_started_event:")
        || issue.starts_with("command_missing_result_event:")
}

pub fn start_debug_session(
    store: &GooseStore,
    input: &DebugSessionStartInput,
) -> GooseResult<DebugSessionSnapshot> {
    validate_required("session_id", &input.session_id)?;
    let bridge_issues = debug_bridge_config_issues(&input.bridge);
    if !bridge_issues.is_empty() {
        return Err(GooseError::message(format!(
            "invalid debug bridge config: {}",
            bridge_issues.join(", ")
        )));
    }
    store.insert_debug_session(&DebugSessionRow {
        session_id: input.session_id.clone(),
        started_at_unix_ms: u64_to_i64("started_at_unix_ms", input.started_at_unix_ms)?,
        bridge_url: input.bridge.url.clone(),
        bind_host: input.bridge.bind_host.clone(),
        token_required: input.bridge.token_required,
        token_present: input.bridge.token_present,
        remote_bind_enabled: input.bridge.remote_bind_enabled,
        visible_remote_bind_toggle: input.bridge.visible_remote_bind_toggle,
    })?;
    debug_session_snapshot(store, &input.session_id)
}

pub fn start_debug_command(
    store: &GooseStore,
    input: &DebugCommandStartInput,
) -> GooseResult<DebugSessionSnapshot> {
    validate_required("session_id", &input.session_id)?;
    if store.debug_session(&input.session_id)?.is_none() {
        return Err(GooseError::message(format!(
            "debug session {} not found",
            input.session_id
        )));
    }
    let command_issues = debug_command_envelope_issues(&input.command);
    if !command_issues.is_empty() {
        return Err(GooseError::message(format!(
            "invalid debug command: {}",
            command_issues.join(", ")
        )));
    }
    let args_json = serde_json::to_string(&input.command.args)
        .map_err(|error| GooseError::message(format!("cannot serialize command args: {error}")))?;
    store.insert_debug_command(&DebugCommandRow {
        command_id: input.command.command_id.clone(),
        session_id: input.session_id.clone(),
        schema: input.command.schema.clone(),
        command: input.command.command.clone(),
        args_json,
        dry_run: input.command.dry_run,
        received_at_unix_ms: u64_to_i64("received_at_unix_ms", input.received_at_unix_ms)?,
    })?;

    append_debug_event(
        store,
        &DebugEventInput {
            session_id: input.session_id.clone(),
            time_unix_ms: input.received_at_unix_ms,
            source: "command".to_string(),
            level: "info".to_string(),
            topic: "command.started".to_string(),
            message: format!("{} accepted", input.command.command),
            command_id: Some(input.command.command_id.clone()),
            data: json!({"dry_run": input.command.dry_run}),
        },
    )?;
    debug_session_snapshot(store, &input.session_id)
}

pub fn finish_debug_command(
    store: &GooseStore,
    input: &DebugCommandFinishInput,
) -> GooseResult<DebugSessionSnapshot> {
    validate_required("session_id", &input.session_id)?;
    validate_required("command_id", &input.command_id)?;
    validate_required("message", &input.message)?;
    let command = store.debug_command(&input.command_id)?.ok_or_else(|| {
        GooseError::message(format!("debug command {} not found", input.command_id))
    })?;
    if command.session_id != input.session_id {
        return Err(GooseError::message(format!(
            "debug command {} belongs to session {}, not {}",
            input.command_id, command.session_id, input.session_id
        )));
    }
    let mut data = object_value(input.data.clone(), "data")?;
    data.as_object_mut()
        .expect("object_value returns an object")
        .insert("ok".to_string(), serde_json::Value::Bool(input.ok));

    append_debug_event(
        store,
        &DebugEventInput {
            session_id: input.session_id.clone(),
            time_unix_ms: input.time_unix_ms,
            source: "command".to_string(),
            level: if input.ok { "info" } else { "error" }.to_string(),
            topic: "command.result".to_string(),
            message: input.message.clone(),
            command_id: Some(input.command_id.clone()),
            data,
        },
    )?;
    debug_session_snapshot(store, &input.session_id)
}

pub fn append_debug_event(
    store: &GooseStore,
    input: &DebugEventInput,
) -> GooseResult<DebugEventEnvelope> {
    validate_required("session_id", &input.session_id)?;
    if store.debug_session(&input.session_id)?.is_none() {
        return Err(GooseError::message(format!(
            "debug session {} not found",
            input.session_id
        )));
    }
    if let Some(command_id) = input.command_id.as_deref()
        && !command_id.trim().is_empty()
    {
        let command = store
            .debug_command(command_id)?
            .ok_or_else(|| GooseError::message(format!("debug command {command_id} not found")))?;
        if command.session_id != input.session_id {
            return Err(GooseError::message(format!(
                "debug command {command_id} belongs to session {}, not {}",
                command.session_id, input.session_id
            )));
        }
    }

    let sequence = store.next_debug_event_sequence(&input.session_id)?;
    let event = DebugEventEnvelope {
        schema: DEBUG_EVENT_SCHEMA.to_string(),
        session_id: input.session_id.clone(),
        time_unix_ms: input.time_unix_ms,
        sequence: i64_to_u64("sequence", sequence)?,
        source: input.source.clone(),
        level: input.level.clone(),
        topic: input.topic.clone(),
        message: input.message.clone(),
        command_id: input.command_id.clone(),
        data: object_value(input.data.clone(), "data")?,
    };
    let event_issues = debug_event_shape_issues(&event);
    if !event_issues.is_empty() {
        return Err(GooseError::message(format!(
            "invalid debug event: {}",
            event_issues.join(", ")
        )));
    }
    let data_json = serde_json::to_string(&event.data)
        .map_err(|error| GooseError::message(format!("cannot serialize event data: {error}")))?;
    store.insert_debug_event(&DebugEventRow {
        session_id: event.session_id.clone(),
        sequence: u64_to_i64("sequence", event.sequence)?,
        schema: event.schema.clone(),
        time_unix_ms: u64_to_i64("time_unix_ms", event.time_unix_ms)?,
        source: event.source.clone(),
        level: event.level.clone(),
        topic: event.topic.clone(),
        message: event.message.clone(),
        command_id: event.command_id.clone(),
        data_json,
    })?;
    Ok(event)
}

pub fn debug_session_snapshot(
    store: &GooseStore,
    session_id: &str,
) -> GooseResult<DebugSessionSnapshot> {
    validate_required("session_id", session_id)?;
    let session = store
        .debug_session(session_id)?
        .ok_or_else(|| GooseError::message(format!("debug session {session_id} not found")))?;
    let bridge = DebugBridgeConfig {
        url: session.bridge_url,
        bind_host: session.bind_host,
        token_required: session.token_required,
        token_present: session.token_present,
        remote_bind_enabled: session.remote_bind_enabled,
        visible_remote_bind_toggle: session.visible_remote_bind_toggle,
    };
    let commands = store
        .debug_commands_for_session(session_id)?
        .into_iter()
        .map(command_from_row)
        .collect::<GooseResult<Vec<_>>>()?;
    let events = store
        .debug_events_for_session(session_id)?
        .into_iter()
        .map(debug_event_envelope_from_row)
        .collect::<GooseResult<Vec<_>>>()?;
    let contract_input = DebugWsContractInput {
        schema: DEBUG_WS_CONTRACT_SCHEMA.to_string(),
        bridge: bridge.clone(),
        commands: commands.clone(),
        events: events.clone(),
    };
    let contract_report = validate_debug_ws_contract(&contract_input);
    Ok(DebugSessionSnapshot {
        schema: "goose.debug-session-snapshot.v1".to_string(),
        session_id: session_id.to_string(),
        bridge,
        commands,
        events,
        contract_report,
    })
}

pub fn debug_bridge_config_issues(bridge: &DebugBridgeConfig) -> Vec<String> {
    let mut issues = Vec::new();
    validate_bridge(bridge, &mut issues);
    sorted_unique_issues(issues)
}

pub fn debug_command_envelope_issues(command: &DebugCommandEnvelope) -> Vec<String> {
    let mut issues = Vec::new();
    let mut command_ids = BTreeSet::new();
    validate_command(command, &mut command_ids, &mut issues);
    sorted_unique_issues(issues)
}

pub fn debug_event_shape_issues(event: &DebugEventEnvelope) -> Vec<String> {
    let mut issues = Vec::new();
    validate_event_shape(event, &mut issues);
    sorted_unique_issues(issues)
}

fn validate_bridge(bridge: &DebugBridgeConfig, issues: &mut Vec<String>) {
    if !(bridge.url.starts_with("ws://") || bridge.url.starts_with("wss://")) {
        issues.push("bridge_url_must_be_websocket".to_string());
    }
    if !bridge.url.contains("/goose-debug/stream") {
        issues.push("bridge_url_missing_debug_stream_path".to_string());
    }
    if !bridge.token_required {
        issues.push("bridge_token_required".to_string());
    }
    if !bridge.token_present {
        issues.push("bridge_token_missing".to_string());
    }
    if !is_loopback_host(&bridge.bind_host) {
        issues.push("bridge_bind_host_must_be_loopback".to_string());
    }
    if bridge.remote_bind_enabled && !bridge.visible_remote_bind_toggle {
        issues.push("remote_bind_requires_visible_toggle".to_string());
    }
}

fn validate_command(
    command: &DebugCommandEnvelope,
    command_ids: &mut BTreeSet<String>,
    issues: &mut Vec<String>,
) {
    if command.schema != DEBUG_COMMAND_SCHEMA {
        issues.push(format!(
            "command_schema_invalid:{}:{}",
            command.command_id, command.schema
        ));
    }
    if command.command_id.trim().is_empty() {
        issues.push("command_id_required".to_string());
    } else if !command_ids.insert(command.command_id.clone()) {
        issues.push(format!("duplicate_command_id:{}", command.command_id));
    }
    if command.command.trim().is_empty() {
        issues.push(format!("command_name_required:{}", command.command_id));
    }
    if !command.args.is_object() {
        issues.push(format!(
            "command_args_must_be_object:{}",
            command.command_id
        ));
    }
}

fn validate_event<'a>(
    event: &'a DebugEventEnvelope,
    command_by_id: &BTreeMap<&'a str, &'a DebugCommandEnvelope>,
    previous_sequence: &mut Option<u64>,
    previous_time: &mut Option<u64>,
    command_topics: &mut BTreeMap<&'a str, BTreeSet<&'a str>>,
    issues: &mut Vec<String>,
) {
    validate_event_shape(event, issues);
    if previous_sequence.is_some_and(|sequence| event.sequence <= sequence) {
        issues.push(format!(
            "event_sequence_not_strictly_increasing:{}",
            event.sequence
        ));
    }
    if previous_time.is_some_and(|time| event.time_unix_ms < time) {
        issues.push(format!("event_time_decreased:{}", event.sequence));
    }
    *previous_sequence = Some(event.sequence);
    *previous_time = Some(event.time_unix_ms);

    if event.source == "command" {
        match event.command_id.as_deref() {
            Some(command_id) if command_by_id.contains_key(command_id) => {
                command_topics
                    .entry(command_id)
                    .or_default()
                    .insert(event.topic.as_str());
            }
            Some(command_id) if command_id.trim().is_empty() => {
                issues.push(format!(
                    "command_event_command_id_required:{}",
                    event.sequence
                ));
            }
            Some(command_id) => {
                issues.push(format!("command_event_unknown_command_id:{}", command_id));
            }
            None => issues.push(format!(
                "command_event_command_id_required:{}",
                event.sequence
            )),
        }
    } else if let Some(command_id) = event.command_id.as_deref()
        && !command_id.trim().is_empty()
        && !command_by_id.contains_key(command_id)
    {
        issues.push(format!("event_unknown_command_id:{}", command_id));
    }
}

fn validate_event_shape(event: &DebugEventEnvelope, issues: &mut Vec<String>) {
    if event.schema != DEBUG_EVENT_SCHEMA {
        issues.push(format!(
            "event_schema_invalid:{}:{}",
            event.sequence, event.schema
        ));
    }
    if event.session_id.trim().is_empty() {
        issues.push(format!("event_session_id_required:{}", event.sequence));
    }
    if !is_allowed_source(&event.source) {
        issues.push(format!(
            "event_source_invalid:{}:{}",
            event.sequence, event.source
        ));
    }
    if !is_allowed_level(&event.level) {
        issues.push(format!(
            "event_level_invalid:{}:{}",
            event.sequence, event.level
        ));
    }
    if event.topic.trim().is_empty() {
        issues.push(format!("event_topic_required:{}", event.sequence));
    }
    if event.message.trim().is_empty() {
        issues.push(format!("event_message_required:{}", event.sequence));
    }
    if !event.data.is_object() {
        issues.push(format!("event_data_must_be_object:{}", event.sequence));
    }
    if event.source == "command" {
        match event.command_id.as_deref() {
            Some(command_id) if command_id.trim().is_empty() => {
                issues.push(format!(
                    "command_event_command_id_required:{}",
                    event.sequence
                ));
            }
            None => issues.push(format!(
                "command_event_command_id_required:{}",
                event.sequence
            )),
            _ => {}
        }
    }
}

fn debug_ws_next_actions(issues: &[String]) -> Vec<DebugWsNextAction> {
    issues
        .iter()
        .map(|issue| debug_ws_next_action(issue))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn debug_ws_next_action(issue: &str) -> DebugWsNextAction {
    let parts = issue.split(':').collect::<Vec<_>>();
    let reason = parts.first().copied().unwrap_or(issue);
    let scope = if parts.len() > 1 {
        parts[1..].join(":")
    } else {
        "debug_ws".to_string()
    };
    let action = match reason {
        "unsupported_schema" => {
            "Update the debug stream contract input to goose.debug-ws-contract.v1, then rerun the contract validator."
        }
        "bridge_url_must_be_websocket" => {
            "Use a ws:// or wss:// URL for the local debug stream, then rerun the contract validator."
        }
        "bridge_url_missing_debug_stream_path" => {
            "Bind the debug stream on /goose-debug/stream so agents and tests connect to the expected path."
        }
        "bridge_token_required" => {
            "Require a per-session token for the debug stream before accepting clients."
        }
        "bridge_token_missing" => {
            "Generate and attach a per-session debug stream token before starting or validating the stream."
        }
        "bridge_bind_host_must_be_loopback" => {
            "Bind the debug stream to 127.0.0.1, localhost, or ::1 unless a visible remote-bind review path is added."
        }
        "remote_bind_requires_visible_toggle" => {
            "Expose a visible remote-bind toggle and approval state before enabling non-loopback debug access."
        }
        "command_schema_invalid" => {
            "Emit debug commands with schema goose.debug.command.v1, then rerun the contract validator."
        }
        "command_id_required" => {
            "Assign a non-empty stable command_id before recording the debug command."
        }
        "duplicate_command_id" => {
            "Give each debug command in the session a unique command_id before recording events."
        }
        "command_name_required" => {
            "Set the bridge method or debug command name before recording the command."
        }
        "command_args_must_be_object" => {
            "Serialize command args as a JSON object, using an empty object when the command has no parameters."
        }
        "event_schema_invalid" => {
            "Emit debug events with schema goose.debug.event.v1, then rerun the contract validator."
        }
        "event_session_id_required" => {
            "Attach the active debug session id to every persisted debug event."
        }
        "event_source_invalid" => {
            "Use an allowed event source: ble, app, rust, sqlite, metric, command, or debug."
        }
        "event_level_invalid" => "Use an allowed event level: trace, debug, info, warn, or error.",
        "event_topic_required" => {
            "Set a non-empty topic for the debug event so agents can route it."
        }
        "event_message_required" => "Set a non-empty human-readable debug event message.",
        "event_data_must_be_object" => {
            "Serialize debug event data as a JSON object, using an empty object when there is no payload."
        }
        "event_sequence_not_strictly_increasing" => {
            "Append events through Goose storage so sequence numbers strictly increase before streaming."
        }
        "event_time_decreased" => {
            "Use non-decreasing event timestamps within a debug session before rerunning the validator."
        }
        "command_event_command_id_required" => {
            "Attach the command_id to command.started and command.result events."
        }
        "command_event_unknown_command_id" | "event_unknown_command_id" => {
            "Record the referenced debug command before emitting events for it, or clear command_id on unrelated events."
        }
        "command_missing_started_event" => {
            "Wrap the action with debug.start_command so a command.started event is persisted before work begins."
        }
        "command_missing_result_event" => {
            "Finish the debug action with debug.finish_command so a command.result event is persisted."
        }
        _ => {
            "Resolve the debug stream contract issue, then rerun goose-debug-ws-contract or refresh the Debug snapshot."
        }
    };
    DebugWsNextAction {
        scope,
        reason: reason.to_string(),
        action: action.to_string(),
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host.trim(), "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

fn is_allowed_source(source: &str) -> bool {
    matches!(
        source,
        "ble" | "app" | "rust" | "sqlite" | "metric" | "command" | "debug"
    )
}

fn is_allowed_level(level: &str) -> bool {
    matches!(level, "trace" | "debug" | "info" | "warn" | "error")
}

fn command_from_row(row: DebugCommandRow) -> GooseResult<DebugCommandEnvelope> {
    let args = serde_json::from_str::<serde_json::Value>(&row.args_json)
        .map_err(|error| GooseError::message(format!("cannot parse command args JSON: {error}")))?;
    Ok(DebugCommandEnvelope {
        schema: row.schema,
        command_id: row.command_id,
        command: row.command,
        args,
        dry_run: row.dry_run,
    })
}

pub fn debug_event_envelope_from_row(row: DebugEventRow) -> GooseResult<DebugEventEnvelope> {
    let data = serde_json::from_str::<serde_json::Value>(&row.data_json)
        .map_err(|error| GooseError::message(format!("cannot parse event data JSON: {error}")))?;
    Ok(DebugEventEnvelope {
        schema: row.schema,
        session_id: row.session_id,
        time_unix_ms: i64_to_u64("time_unix_ms", row.time_unix_ms)?,
        sequence: i64_to_u64("sequence", row.sequence)?,
        source: row.source,
        level: row.level,
        topic: row.topic,
        message: row.message,
        command_id: row.command_id,
        data,
    })
}

fn validate_required(name: &str, value: &str) -> GooseResult<()> {
    if value.trim().is_empty() {
        Err(GooseError::message(format!("{name} is required")))
    } else {
        Ok(())
    }
}

fn object_value(value: serde_json::Value, name: &str) -> GooseResult<serde_json::Value> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(GooseError::message(format!("{name} must be a JSON object")))
    }
}

fn u64_to_i64(name: &str, value: u64) -> GooseResult<i64> {
    i64::try_from(value).map_err(|_| GooseError::message(format!("{name} is too large")))
}

fn i64_to_u64(name: &str, value: i64) -> GooseResult<u64> {
    u64::try_from(value).map_err(|_| GooseError::message(format!("{name} is negative")))
}

fn sorted_unique_issues(mut issues: Vec<String>) -> Vec<String> {
    issues.sort();
    issues.dedup();
    issues
}

fn empty_object() -> serde_json::Value {
    json!({})
}
