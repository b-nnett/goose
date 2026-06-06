//! Historical sync dry-run planning.
//!
//! This module models the historical-sync state machine and command plan
//! without issuing any BLE writes. It keeps the repo's safety-gate wording
//! while staying close to the OpenWhoop reference flow:
//! connect, optional `GetDataRange`, `SendHistoricalData`, consume metadata and
//! readings, ack `HistoryEnd`, stop on `HistoryComplete`, and retry once after
//! idle with `AbortHistoricalTransmits`.

use serde::{Deserialize, Serialize};

pub const HISTORICAL_SYNC_DRY_RUN_SCHEMA: &str = "goose.historical-sync-dry-run.v1";
pub const HISTORICAL_SYNC_DRY_RUN_REPORT_SCHEMA: &str = "goose.historical-sync-dry-run-report.v1";
pub const HISTORICAL_SYNC_PHYSICAL_VALIDATION_SCHEMA: &str =
    "goose.historical-sync-physical-validation.v1";
pub const HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA: &str =
    "goose.historical-sync-physical-validation-report.v1";
pub const HISTORICAL_SYNC_PHYSICAL_EVIDENCE_TEMPLATE_SCHEMA: &str =
    "goose.historical-sync-physical-evidence-template.v1";
pub const HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY: &str =
    "historical_sync_physical_requires_current_flow_event_order_and_timestamp_integrity";
pub const HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY: &str =
    "service_characteristics_notifications_auth_commands_event_order_and_timestamp_fields";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncGeneration {
    Gen4,
    Gen5,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncState {
    #[default]
    Idle,
    Connected,
    RangeRequested,
    Transferring,
    AckPending,
    Complete,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncSafetyGate {
    ReadOnly,
    UserVisibleStateChange,
    CriticalStateChange,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncAckDisposition {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncPayloadExpectation {
    Empty,
    ZeroByte,
    HistoryEndAck {
        disposition: HistoricalSyncAckDisposition,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalSyncPlanStepKind {
    Connect,
    GetDataRange,
    SendHistoricalData,
    ConsumeMetadata,
    ConsumeReading,
    HistoricalDataResult,
    AbortHistoricalTransmits,
    ResumeRequested,
    Blocked,
    Failed,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoricalSyncFakeEvent {
    HistoryStart,
    HistoryEnd,
    HistoryComplete,
    Metadata { name: String },
    Reading { name: String },
    IdleTimeout,
    CancelRequested,
    ResumeRequested,
    MalformedResponse { detail: String },
    DuplicateTransfer { detail: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncRetryPlan {
    #[serde(default = "default_true")]
    pub retry_after_idle: bool,
    #[serde(default = "default_retry_budget")]
    pub max_idle_retries: u8,
}

impl Default for HistoricalSyncRetryPlan {
    fn default() -> Self {
        Self {
            retry_after_idle: true,
            max_idle_retries: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncTimeoutPlan {
    pub idle_timeout_ms: u64,
    pub transfer_timeout_ms: u64,
    pub ack_timeout_ms: u64,
}

impl Default for HistoricalSyncTimeoutPlan {
    fn default() -> Self {
        Self {
            idle_timeout_ms: 30_000,
            transfer_timeout_ms: 120_000,
            ack_timeout_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HistoricalSyncCancelPlan {
    #[serde(default)]
    pub requested: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HistoricalSyncResumePlan {
    #[serde(default)]
    pub requested: bool,
    #[serde(default)]
    pub resume_from_state: Option<HistoricalSyncState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncDryRunInput {
    pub schema: String,
    pub generation: HistoricalSyncGeneration,
    #[serde(default = "default_true")]
    pub device_connected: bool,
    #[serde(default = "default_true")]
    pub safety_gate_ready: bool,
    #[serde(default = "default_true")]
    pub request_data_range: bool,
    #[serde(default)]
    pub retry: HistoricalSyncRetryPlan,
    #[serde(default)]
    pub timeout: HistoricalSyncTimeoutPlan,
    #[serde(default)]
    pub cancel: HistoricalSyncCancelPlan,
    #[serde(default)]
    pub resume: HistoricalSyncResumePlan,
    #[serde(default)]
    pub fake_events: Vec<HistoricalSyncFakeEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncPlanStep {
    pub kind: HistoricalSyncPlanStepKind,
    pub state_before: HistoricalSyncState,
    pub state_after: HistoricalSyncState,
    #[serde(default)]
    pub safety_gate: Option<HistoricalSyncSafetyGate>,
    #[serde(default)]
    pub command_number: Option<u16>,
    #[serde(default)]
    pub payload_expectation: Option<HistoricalSyncPayloadExpectation>,
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncDryRunReport {
    pub schema: String,
    pub generated_by: String,
    pub generation: HistoricalSyncGeneration,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub device_connected: bool,
    #[serde(default)]
    pub safety_gate_ready: bool,
    #[serde(default)]
    pub request_data_range: bool,
    pub state: HistoricalSyncState,
    pub state_trace: Vec<HistoricalSyncState>,
    pub steps: Vec<HistoricalSyncPlanStep>,
    pub planned_command_count: usize,
    pub blocked_count: usize,
    pub failed_count: usize,
    pub retry_count: usize,
    pub timeout_count: usize,
    pub cancel_count: usize,
    pub resume_count: usize,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<HistoricalSyncNextAction>,
    #[serde(default)]
    pub retry: HistoricalSyncRetryPlan,
    #[serde(default)]
    pub timeout: HistoricalSyncTimeoutPlan,
    #[serde(default)]
    pub cancel: HistoricalSyncCancelPlan,
    #[serde(default)]
    pub resume: HistoricalSyncResumePlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncPhysicalValidationInput {
    pub schema: String,
    pub generation: HistoricalSyncGeneration,
    pub capture_session_id: String,
    #[serde(default)]
    pub service_uuids: Vec<String>,
    #[serde(default)]
    pub characteristics: Vec<HistoricalSyncCharacteristicEvidence>,
    #[serde(default)]
    pub notification_subscriptions: Vec<HistoricalSyncNotificationEvidence>,
    #[serde(default)]
    pub auth_events: Vec<HistoricalSyncObservedEvent>,
    #[serde(default)]
    pub command_events: Vec<HistoricalSyncObservedCommand>,
    #[serde(default)]
    pub metadata_events: Vec<HistoricalSyncObservedEvent>,
    #[serde(default)]
    pub timestamp_evidence: Vec<HistoricalSyncTimestampEvidence>,
    #[serde(default)]
    pub raw_evidence_anchors: Vec<HistoricalSyncRawEvidenceAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncRawEvidenceAnchor {
    pub evidence_id: String,
    pub sha256: String,
    pub observation_kind: String,
    pub observation_name: String,
    #[serde(default)]
    pub sequence: Option<u32>,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncCharacteristicEvidence {
    pub service_uuid: String,
    pub characteristic_uuid: String,
    pub role: String,
    #[serde(default)]
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncNotificationEvidence {
    pub characteristic_uuid: String,
    pub enabled: bool,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncObservedEvent {
    pub name: String,
    pub sequence: u32,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncObservedCommand {
    pub command: String,
    pub sequence: u32,
    #[serde(default)]
    pub response_observed: bool,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncTimestampEvidence {
    pub packet_kind: String,
    #[serde(default)]
    pub source_signal: String,
    pub captured_at: String,
    #[serde(default)]
    pub sample_time: Option<String>,
    #[serde(default)]
    pub sample_time_source: Option<String>,
    #[serde(default)]
    pub device_timestamp_seconds: Option<u32>,
    #[serde(default)]
    pub device_timestamp_subseconds: Option<u16>,
    #[serde(default)]
    pub capture_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncPhysicalValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub generation: HistoricalSyncGeneration,
    pub capture_session_id: String,
    pub service_uuid_confirmed: bool,
    pub characteristic_roles_confirmed: bool,
    pub notification_behavior_confirmed: bool,
    pub auth_session_handshake_confirmed: bool,
    pub command_flow_confirmed: bool,
    #[serde(default)]
    pub event_order_confirmed: bool,
    #[serde(default)]
    pub evidence_session_confirmed: bool,
    #[serde(default)]
    pub raw_evidence_anchored: bool,
    pub timestamp_fields_confirmed: bool,
    #[serde(default)]
    pub service_uuid_count: usize,
    #[serde(default)]
    pub characteristic_count: usize,
    #[serde(default)]
    pub notification_subscription_count: usize,
    #[serde(default)]
    pub auth_event_count: usize,
    #[serde(default)]
    pub command_event_count: usize,
    #[serde(default)]
    pub metadata_event_count: usize,
    #[serde(default)]
    pub timestamp_evidence_count: usize,
    #[serde(default)]
    pub raw_evidence_anchor_count: usize,
    #[serde(default)]
    pub motion_timestamp_evidence_count: usize,
    #[serde(default)]
    pub heart_rate_timestamp_evidence_count: usize,
    pub issues: Vec<String>,
    pub quality_flags: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<HistoricalSyncNextAction>,
    #[serde(default)]
    pub provenance: serde_json::Value,
    #[serde(default)]
    pub acceptance_summary: HistoricalSyncPhysicalAcceptanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HistoricalSyncPhysicalAcceptanceSummary {
    pub policy: String,
    pub physical_sync_ready: bool,
    pub generation: String,
    pub capture_session_id: String,
    pub service_uuid_confirmed: bool,
    pub characteristic_roles_confirmed: bool,
    pub notification_behavior_confirmed: bool,
    pub auth_session_handshake_confirmed: bool,
    pub command_flow_confirmed: bool,
    pub event_order_confirmed: bool,
    pub evidence_session_confirmed: bool,
    pub raw_evidence_anchored: bool,
    pub timestamp_fields_confirmed: bool,
    pub service_uuid_count: usize,
    pub characteristic_count: usize,
    pub notification_subscription_count: usize,
    pub auth_event_count: usize,
    pub command_event_count: usize,
    pub metadata_event_count: usize,
    pub timestamp_evidence_count: usize,
    pub raw_evidence_anchor_count: usize,
    pub motion_timestamp_evidence_count: usize,
    pub heart_rate_timestamp_evidence_count: usize,
    pub issue_count: usize,
    pub quality_flag_count: usize,
    pub error_count: usize,
    pub next_action_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalSyncPhysicalEvidenceTemplate {
    pub schema: String,
    pub generated_by: String,
    pub generation: HistoricalSyncGeneration,
    pub capture_session_id: String,
    pub expected_service_uuid: String,
    pub input: HistoricalSyncPhysicalValidationInput,
    pub required_observations: Vec<HistoricalSyncNextAction>,
}

pub fn run_historical_sync_dry_run(
    input: &HistoricalSyncDryRunInput,
) -> HistoricalSyncDryRunReport {
    let mut issues = Vec::new();
    let input_valid = validate_input(input, &mut issues);

    let mut state = HistoricalSyncState::Idle;
    let mut state_trace = vec![state];
    let mut steps = Vec::new();
    let mut planned_command_count = 0usize;
    let mut blocked_count = 0usize;
    let mut failed_count = 0usize;
    let mut retry_count = 0usize;
    let mut timeout_count = 0usize;
    let mut cancel_count = 0usize;
    let mut resume_count = 0usize;

    if !input_valid {
        push_failed(
            &mut steps,
            &mut state_trace,
            &mut state,
            "invalid historical sync dry-run input",
            &mut failed_count,
        );
        return finish_report(
            input,
            input_valid,
            state,
            state_trace,
            steps,
            planned_command_count,
            blocked_count,
            failed_count,
            retry_count,
            timeout_count,
            cancel_count,
            resume_count,
            issues,
        );
    }

    if !input.device_connected {
        push_blocked(
            &mut steps,
            &mut state_trace,
            &mut state,
            "device_disconnected",
            &mut blocked_count,
        );
        push_issue_once(&mut issues, "device_disconnected");
        return finish_report(
            input,
            input_valid,
            state,
            state_trace,
            steps,
            planned_command_count,
            blocked_count,
            failed_count,
            retry_count,
            timeout_count,
            cancel_count,
            resume_count,
            issues,
        );
    }

    if !input.safety_gate_ready {
        push_blocked(
            &mut steps,
            &mut state_trace,
            &mut state,
            "safety_gate_locked",
            &mut blocked_count,
        );
        push_issue_once(&mut issues, "safety_gate_locked");
        return finish_report(
            input,
            input_valid,
            state,
            state_trace,
            steps,
            planned_command_count,
            blocked_count,
            failed_count,
            retry_count,
            timeout_count,
            cancel_count,
            resume_count,
            issues,
        );
    }

    let mut current_attempt_history_end_seen = false;
    let mut current_attempt_history_complete_seen = false;
    start_transfer_attempt(
        input,
        &mut state,
        &mut state_trace,
        &mut steps,
        &mut planned_command_count,
        "initial_connection",
    );

    for event in &input.fake_events {
        if state == HistoricalSyncState::Failed || state == HistoricalSyncState::Complete {
            break;
        }
        if state == HistoricalSyncState::Blocked
            && !matches!(event, HistoricalSyncFakeEvent::ResumeRequested)
        {
            push_issue_once(&mut issues, "event_while_blocked");
            push_failed(
                &mut steps,
                &mut state_trace,
                &mut state,
                "event_while_blocked",
                &mut failed_count,
            );
            break;
        }

        match event {
            HistoricalSyncFakeEvent::HistoryStart => {
                let next_state = state;
                consume_metadata(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    "HistoryStart",
                    "history_start",
                    next_state,
                );
            }
            HistoricalSyncFakeEvent::HistoryEnd => {
                handle_history_end(
                    input,
                    &mut state,
                    &mut state_trace,
                    &mut steps,
                    &mut planned_command_count,
                    &mut current_attempt_history_end_seen,
                    &mut issues,
                    &mut failed_count,
                );
            }
            HistoricalSyncFakeEvent::HistoryComplete => {
                if current_attempt_history_end_seen {
                    consume_metadata(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "HistoryComplete",
                        "history_complete",
                        HistoricalSyncState::AckPending,
                    );
                    push_complete(&mut steps, &mut state_trace, &mut state);
                    current_attempt_history_complete_seen = true;
                    break;
                }
                push_issue_once(&mut issues, "history_end_missing_before_history_complete");
                push_failed(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    "history_complete_without_history_end",
                    &mut failed_count,
                );
                break;
            }
            HistoricalSyncFakeEvent::Metadata { name } => {
                let marker = normalize_history_marker(name);
                if marker == "history_start" {
                    let next_state = state;
                    consume_metadata(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "HistoryStart",
                        "history_start",
                        next_state,
                    );
                } else if marker == "history_end" {
                    handle_history_end(
                        input,
                        &mut state,
                        &mut state_trace,
                        &mut steps,
                        &mut planned_command_count,
                        &mut current_attempt_history_end_seen,
                        &mut issues,
                        &mut failed_count,
                    );
                } else if marker == "history_complete" {
                    if current_attempt_history_end_seen {
                        consume_metadata(
                            &mut steps,
                            &mut state_trace,
                            &mut state,
                            "HistoryComplete",
                            "history_complete",
                            HistoricalSyncState::AckPending,
                        );
                        push_complete(&mut steps, &mut state_trace, &mut state);
                        current_attempt_history_complete_seen = true;
                        break;
                    }
                    push_issue_once(&mut issues, "history_end_missing_before_history_complete");
                    push_failed(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "history_complete_without_history_end",
                        &mut failed_count,
                    );
                    break;
                } else {
                    let next_state = state;
                    consume_metadata(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        name.as_str(),
                        name.as_str(),
                        next_state,
                    );
                }
            }
            HistoricalSyncFakeEvent::Reading { name } => {
                consume_reading(&mut steps, &mut state_trace, &mut state, name);
            }
            HistoricalSyncFakeEvent::IdleTimeout => {
                timeout_count += 1;
                if input.retry.retry_after_idle
                    && retry_count < usize::from(input.retry.max_idle_retries)
                {
                    push_command_step(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        HistoricalSyncPlanStepKind::AbortHistoricalTransmits,
                        HistoricalSyncState::Idle,
                        "retry_after_idle",
                        input.generation,
                        None,
                    );
                    retry_count += 1;
                    current_attempt_history_end_seen = false;
                    current_attempt_history_complete_seen = false;
                    start_transfer_attempt(
                        input,
                        &mut state,
                        &mut state_trace,
                        &mut steps,
                        &mut planned_command_count,
                        "retry_after_idle",
                    );
                } else {
                    push_issue_once(&mut issues, "idle_timeout_retry_exhausted");
                    push_failed(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "idle_timeout_retry_exhausted",
                        &mut failed_count,
                    );
                    break;
                }
            }
            HistoricalSyncFakeEvent::CancelRequested => {
                cancel_count += 1;
                let cancel_reason = input.cancel.reason.as_deref().unwrap_or("cancel_requested");
                push_command_step(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    HistoricalSyncPlanStepKind::AbortHistoricalTransmits,
                    HistoricalSyncState::Idle,
                    cancel_reason,
                    input.generation,
                    None,
                );
                push_blocked(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    cancel_reason,
                    &mut blocked_count,
                );
            }
            HistoricalSyncFakeEvent::ResumeRequested => {
                if !input.resume.requested {
                    push_issue_once(&mut issues, "resume_not_enabled");
                    push_failed(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "resume_not_enabled",
                        &mut failed_count,
                    );
                    break;
                }
                if state != HistoricalSyncState::Blocked {
                    push_issue_once(&mut issues, "resume_without_block");
                    push_failed(
                        &mut steps,
                        &mut state_trace,
                        &mut state,
                        "resume_without_block",
                        &mut failed_count,
                    );
                    break;
                }
                resume_count += 1;
                let resume_reason = input
                    .resume
                    .resume_from_state
                    .map(|value| format!("resume_from_{value:?}"))
                    .unwrap_or_else(|| "resume_requested".to_string());
                push_step(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    HistoricalSyncPlanStepKind::ResumeRequested,
                    HistoricalSyncState::Idle,
                    None,
                    None,
                    None,
                    None,
                    resume_reason,
                );
                current_attempt_history_end_seen = false;
                current_attempt_history_complete_seen = false;
                start_transfer_attempt(
                    input,
                    &mut state,
                    &mut state_trace,
                    &mut steps,
                    &mut planned_command_count,
                    "resume_after_cancel",
                );
            }
            HistoricalSyncFakeEvent::MalformedResponse { detail } => {
                push_issue_once(&mut issues, "malformed_response");
                push_failed(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    detail,
                    &mut failed_count,
                );
                break;
            }
            HistoricalSyncFakeEvent::DuplicateTransfer { detail } => {
                push_issue_once(&mut issues, "duplicate_transfer");
                push_failed(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    detail,
                    &mut failed_count,
                );
                break;
            }
        }
    }

    if state != HistoricalSyncState::Complete && state != HistoricalSyncState::Failed {
        if state == HistoricalSyncState::Blocked {
            push_issue_once(&mut issues, "history_sync_cancelled");
        } else if current_attempt_history_end_seen {
            if !current_attempt_history_complete_seen {
                push_issue_once(&mut issues, "history_complete_missing");
                push_failed(
                    &mut steps,
                    &mut state_trace,
                    &mut state,
                    "history_complete_missing",
                    &mut failed_count,
                );
            }
        } else {
            push_issue_once(&mut issues, "history_end_missing");
            push_failed(
                &mut steps,
                &mut state_trace,
                &mut state,
                "history_end_missing",
                &mut failed_count,
            );
        }
    }

    finish_report(
        input,
        input_valid,
        state,
        state_trace,
        steps,
        planned_command_count,
        blocked_count,
        failed_count,
        retry_count,
        timeout_count,
        cancel_count,
        resume_count,
        issues,
    )
}

pub fn historical_sync_physical_evidence_template(
    generation: HistoricalSyncGeneration,
    capture_session_id: impl Into<String>,
) -> HistoricalSyncPhysicalEvidenceTemplate {
    let capture_session_id = capture_session_id.into();
    let expected_service_uuid = expected_historical_service_uuid(generation).to_string();
    HistoricalSyncPhysicalEvidenceTemplate {
        schema: HISTORICAL_SYNC_PHYSICAL_EVIDENCE_TEMPLATE_SCHEMA.to_string(),
        generated_by: "goose-historical-sync-physical-validator".to_string(),
        generation,
        capture_session_id: capture_session_id.clone(),
        expected_service_uuid: expected_service_uuid.clone(),
        input: HistoricalSyncPhysicalValidationInput {
            schema: HISTORICAL_SYNC_PHYSICAL_VALIDATION_SCHEMA.to_string(),
            generation,
            capture_session_id: capture_session_id.clone(),
            service_uuids: vec![expected_service_uuid.clone()],
            characteristics: vec![
                HistoricalSyncCharacteristicEvidence {
                    service_uuid: expected_service_uuid.to_string(),
                    characteristic_uuid: String::new(),
                    role: "command_to_strap".to_string(),
                    properties: vec!["write_without_response".to_string()],
                },
                HistoricalSyncCharacteristicEvidence {
                    service_uuid: expected_service_uuid.to_string(),
                    characteristic_uuid: String::new(),
                    role: "data_from_strap".to_string(),
                    properties: vec!["notify".to_string()],
                },
                HistoricalSyncCharacteristicEvidence {
                    service_uuid: expected_service_uuid.to_string(),
                    characteristic_uuid: String::new(),
                    role: "event_from_strap".to_string(),
                    properties: vec!["notify".to_string()],
                },
            ],
            notification_subscriptions: vec![
                HistoricalSyncNotificationEvidence {
                    characteristic_uuid: String::new(),
                    enabled: false,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncNotificationEvidence {
                    characteristic_uuid: String::new(),
                    enabled: false,
                    capture_session_id: Some(capture_session_id.clone()),
                },
            ],
            auth_events: vec![
                HistoricalSyncObservedEvent {
                    name: "connected".to_string(),
                    sequence: 1,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncObservedEvent {
                    name: "authenticated".to_string(),
                    sequence: 2,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncObservedEvent {
                    name: "subscribed".to_string(),
                    sequence: 3,
                    capture_session_id: Some(capture_session_id.clone()),
                },
            ],
            command_events: vec![
                HistoricalSyncObservedCommand {
                    command: "send_historical_data".to_string(),
                    sequence: 4,
                    response_observed: false,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncObservedCommand {
                    command: "historical_data_result".to_string(),
                    sequence: 8,
                    response_observed: false,
                    capture_session_id: Some(capture_session_id.clone()),
                },
            ],
            metadata_events: vec![
                HistoricalSyncObservedEvent {
                    name: "HistoryStart".to_string(),
                    sequence: 5,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncObservedEvent {
                    name: "HistoryEnd".to_string(),
                    sequence: 6,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncObservedEvent {
                    name: "HistoryComplete".to_string(),
                    sequence: 7,
                    capture_session_id: Some(capture_session_id.clone()),
                },
            ],
            timestamp_evidence: vec![
                HistoricalSyncTimestampEvidence {
                    packet_kind: "raw_motion_k21".to_string(),
                    source_signal: "raw_motion_k21".to_string(),
                    captured_at: String::new(),
                    sample_time: None,
                    sample_time_source: Some("device_timestamp".to_string()),
                    device_timestamp_seconds: None,
                    device_timestamp_subseconds: None,
                    capture_session_id: Some(capture_session_id.clone()),
                },
                HistoricalSyncTimestampEvidence {
                    packet_kind: "normal_history".to_string(),
                    source_signal: "heart_rate".to_string(),
                    captured_at: String::new(),
                    sample_time: None,
                    sample_time_source: Some("device_timestamp".to_string()),
                    device_timestamp_seconds: None,
                    device_timestamp_subseconds: None,
                    capture_session_id: Some(capture_session_id.clone()),
                },
            ],
            raw_evidence_anchors: Vec::new(),
        },
        required_observations: physical_validation_next_actions(&[
            "historical_service_uuid_missing".to_string(),
            "historical_characteristic_roles_incomplete".to_string(),
            "historical_notification_behavior_missing".to_string(),
            "auth_session_handshake_missing".to_string(),
            "historical_command_flow_incomplete".to_string(),
            "historical_motion_timestamp_fields_unproven".to_string(),
            "historical_heart_rate_timestamp_fields_unproven".to_string(),
            "historical_timestamp_fields_unproven".to_string(),
            "historical_raw_evidence_fingerprints_missing".to_string(),
        ]),
    }
}

pub fn validate_historical_sync_physical_evidence(
    input: &HistoricalSyncPhysicalValidationInput,
) -> HistoricalSyncPhysicalValidationReport {
    let mut issues = Vec::new();
    if input.schema != HISTORICAL_SYNC_PHYSICAL_VALIDATION_SCHEMA {
        push_issue_once(&mut issues, format!("unsupported schema {}", input.schema));
    }
    if input.capture_session_id.trim().is_empty() {
        push_issue_once(&mut issues, "capture_session_id_required");
    }

    let expected_service_uuid = expected_historical_service_uuid(input.generation);
    let service_uuid_confirmed = input
        .service_uuids
        .iter()
        .any(|uuid| normalize_uuid(uuid) == expected_service_uuid);
    if !service_uuid_confirmed {
        push_issue_once(&mut issues, "historical_service_uuid_missing");
    }

    let characteristic_roles_confirmed = has_characteristic_role(input, "command_to_strap")
        && has_characteristic_role(input, "data_from_strap")
        && has_characteristic_role(input, "event_from_strap");
    if !characteristic_roles_confirmed {
        push_issue_once(&mut issues, "historical_characteristic_roles_incomplete");
    }

    let notification_behavior_confirmed =
        input.notification_subscriptions.iter().any(|notification| {
            notification.enabled
                && characteristic_for_role(input, "data_from_strap")
                    .is_some_and(|uuid| normalize_uuid(&notification.characteristic_uuid) == uuid)
        }) && input.notification_subscriptions.iter().any(|notification| {
            notification.enabled
                && characteristic_for_role(input, "event_from_strap")
                    .is_some_and(|uuid| normalize_uuid(&notification.characteristic_uuid) == uuid)
        });
    if !notification_behavior_confirmed {
        push_issue_once(&mut issues, "historical_notification_behavior_missing");
    }

    let auth_session_handshake_confirmed = observed_event(&input.auth_events, "connected")
        && observed_event(&input.auth_events, "authenticated")
        && observed_event(&input.auth_events, "subscribed");
    if !auth_session_handshake_confirmed {
        push_issue_once(&mut issues, "auth_session_handshake_missing");
    }

    let command_flow_confirmed = observed_command(&input.command_events, "send_historical_data")
        && observed_command(&input.command_events, "historical_data_result")
        && observed_metadata(&input.metadata_events, "history_start")
        && observed_metadata(&input.metadata_events, "history_end")
        && observed_metadata(&input.metadata_events, "history_complete");
    if !command_flow_confirmed {
        push_issue_once(&mut issues, "historical_command_flow_incomplete");
    }
    let event_order_confirmed = physical_event_order_confirmed(input);
    if !event_order_confirmed {
        push_issue_once(&mut issues, "historical_event_order_unproven");
    }
    let evidence_session_confirmed = physical_evidence_session_confirmed(input);
    if !evidence_session_confirmed {
        push_issue_once(&mut issues, "historical_evidence_session_mismatch");
    }
    let raw_evidence_anchored = physical_raw_evidence_anchored(input);
    if !raw_evidence_anchored {
        push_issue_once(&mut issues, "historical_raw_evidence_fingerprints_missing");
    }

    let motion_timestamp_evidence_count =
        timestamp_packet_confirmed_rows(&input.timestamp_evidence, "motion").len();
    let timestamp_motion_fields_confirmed = motion_timestamp_evidence_count > 0;
    if !timestamp_motion_fields_confirmed {
        push_issue_once(&mut issues, "historical_motion_timestamp_fields_unproven");
    }
    let heart_rate_timestamp_evidence_count =
        timestamp_packet_confirmed_rows(&input.timestamp_evidence, "heart_rate").len();
    let timestamp_heart_rate_fields_confirmed = heart_rate_timestamp_evidence_count > 0;
    if !timestamp_heart_rate_fields_confirmed {
        push_issue_once(
            &mut issues,
            "historical_heart_rate_timestamp_fields_unproven",
        );
    }
    let timestamp_fields_confirmed = timestamp_packets_confirmed(&input.timestamp_evidence);
    if !timestamp_fields_confirmed {
        push_issue_once(&mut issues, "historical_timestamp_fields_unproven");
    }

    let pass = issues.is_empty()
        && service_uuid_confirmed
        && characteristic_roles_confirmed
        && notification_behavior_confirmed
        && auth_session_handshake_confirmed
        && command_flow_confirmed
        && event_order_confirmed
        && evidence_session_confirmed
        && raw_evidence_anchored
        && timestamp_fields_confirmed;
    let mut report = HistoricalSyncPhysicalValidationReport {
        schema: HISTORICAL_SYNC_PHYSICAL_VALIDATION_REPORT_SCHEMA.to_string(),
        generated_by: "goose-historical-sync-physical-validator".to_string(),
        pass,
        generation: input.generation,
        capture_session_id: input.capture_session_id.clone(),
        service_uuid_confirmed,
        characteristic_roles_confirmed,
        notification_behavior_confirmed,
        auth_session_handshake_confirmed,
        command_flow_confirmed,
        event_order_confirmed,
        evidence_session_confirmed,
        raw_evidence_anchored,
        timestamp_fields_confirmed,
        service_uuid_count: input.service_uuids.len(),
        characteristic_count: input.characteristics.len(),
        notification_subscription_count: input.notification_subscriptions.len(),
        auth_event_count: input.auth_events.len(),
        command_event_count: input.command_events.len(),
        metadata_event_count: input.metadata_events.len(),
        timestamp_evidence_count: input.timestamp_evidence.len(),
        raw_evidence_anchor_count: input.raw_evidence_anchors.len(),
        motion_timestamp_evidence_count,
        heart_rate_timestamp_evidence_count,
        quality_flags: Vec::new(),
        errors: Vec::new(),
        next_actions: physical_validation_next_actions(&issues),
        issues,
        provenance: serde_json::json!({
            "report_integrity_policy": HISTORICAL_SYNC_PHYSICAL_REPORT_INTEGRITY_POLICY,
            "validation_policy": HISTORICAL_SYNC_PHYSICAL_VALIDATION_POLICY,
        }),
        acceptance_summary: HistoricalSyncPhysicalAcceptanceSummary::default(),
    };
    report.acceptance_summary = historical_sync_physical_acceptance_summary(&report);
    report
}

pub fn historical_sync_physical_acceptance_summary(
    report: &HistoricalSyncPhysicalValidationReport,
) -> HistoricalSyncPhysicalAcceptanceSummary {
    HistoricalSyncPhysicalAcceptanceSummary {
        policy: "historical_sync_physical_must_match_current_flow_timestamp_and_evidence_contract"
            .to_string(),
        physical_sync_ready: report.pass,
        generation: match report.generation {
            HistoricalSyncGeneration::Gen4 => "gen4",
            HistoricalSyncGeneration::Gen5 => "gen5",
        }
        .to_string(),
        capture_session_id: report.capture_session_id.clone(),
        service_uuid_confirmed: report.service_uuid_confirmed,
        characteristic_roles_confirmed: report.characteristic_roles_confirmed,
        notification_behavior_confirmed: report.notification_behavior_confirmed,
        auth_session_handshake_confirmed: report.auth_session_handshake_confirmed,
        command_flow_confirmed: report.command_flow_confirmed,
        event_order_confirmed: report.event_order_confirmed,
        evidence_session_confirmed: report.evidence_session_confirmed,
        raw_evidence_anchored: report.raw_evidence_anchored,
        timestamp_fields_confirmed: report.timestamp_fields_confirmed,
        service_uuid_count: report.service_uuid_count,
        characteristic_count: report.characteristic_count,
        notification_subscription_count: report.notification_subscription_count,
        auth_event_count: report.auth_event_count,
        command_event_count: report.command_event_count,
        metadata_event_count: report.metadata_event_count,
        timestamp_evidence_count: report.timestamp_evidence_count,
        raw_evidence_anchor_count: report.raw_evidence_anchor_count,
        motion_timestamp_evidence_count: report.motion_timestamp_evidence_count,
        heart_rate_timestamp_evidence_count: report.heart_rate_timestamp_evidence_count,
        issue_count: report.issues.len(),
        quality_flag_count: report.quality_flags.len(),
        error_count: report.errors.len(),
        next_action_count: report.next_actions.len(),
    }
}

fn validate_input(input: &HistoricalSyncDryRunInput, issues: &mut Vec<String>) -> bool {
    let mut valid = true;
    if input.schema != HISTORICAL_SYNC_DRY_RUN_SCHEMA {
        push_issue_once(issues, format!("unsupported schema {}", input.schema));
        valid = false;
    }
    for (name, value) in [
        ("idle_timeout_ms", input.timeout.idle_timeout_ms),
        ("transfer_timeout_ms", input.timeout.transfer_timeout_ms),
        ("ack_timeout_ms", input.timeout.ack_timeout_ms),
    ] {
        if value == 0 {
            push_issue_once(issues, format!("{name}_must_be_positive"));
            valid = false;
        }
    }
    valid
}

fn start_transfer_attempt(
    input: &HistoricalSyncDryRunInput,
    state: &mut HistoricalSyncState,
    state_trace: &mut Vec<HistoricalSyncState>,
    steps: &mut Vec<HistoricalSyncPlanStep>,
    planned_command_count: &mut usize,
    note: &str,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::Connect,
        HistoricalSyncState::Connected,
        None,
        None,
        None,
        None,
        note.to_string(),
    );

    let mut current_state = *state;
    if input.request_data_range {
        push_command_step(
            steps,
            state_trace,
            &mut current_state,
            HistoricalSyncPlanStepKind::GetDataRange,
            HistoricalSyncState::RangeRequested,
            "optional_get_data_range",
            input.generation,
            None,
        );
        *planned_command_count += 1;
    }

    push_command_step(
        steps,
        state_trace,
        &mut current_state,
        HistoricalSyncPlanStepKind::SendHistoricalData,
        HistoricalSyncState::Transferring,
        note,
        input.generation,
        None,
    );
    *planned_command_count += 1;
    *state = current_state;
}

fn handle_history_end(
    input: &HistoricalSyncDryRunInput,
    state: &mut HistoricalSyncState,
    state_trace: &mut Vec<HistoricalSyncState>,
    steps: &mut Vec<HistoricalSyncPlanStep>,
    planned_command_count: &mut usize,
    history_end_seen: &mut bool,
    issues: &mut Vec<String>,
    failed_count: &mut usize,
) {
    match *state {
        HistoricalSyncState::Transferring | HistoricalSyncState::AckPending => {
            consume_metadata(
                steps,
                state_trace,
                state,
                "HistoryEnd",
                "history_end",
                HistoricalSyncState::AckPending,
            );
            *history_end_seen = true;
            push_command_step(
                steps,
                state_trace,
                state,
                HistoricalSyncPlanStepKind::HistoricalDataResult,
                HistoricalSyncState::AckPending,
                "ack_history_end_success",
                input.generation,
                Some(HistoricalSyncAckDisposition::Success),
            );
            *planned_command_count += 1;
        }
        _ => {
            push_issue_once(issues, "history_end_out_of_order");
            push_failed(
                steps,
                state_trace,
                state,
                "history_end_out_of_order",
                failed_count,
            );
        }
    }
}

fn consume_metadata(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    event_name: &str,
    note: &str,
    next_state: HistoricalSyncState,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::ConsumeMetadata,
        next_state,
        None,
        None,
        None,
        Some(event_name.to_string()),
        note.to_string(),
    );
}

fn consume_reading(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    event_name: &str,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::ConsumeReading,
        *state,
        None,
        None,
        None,
        Some(event_name.to_string()),
        "consume_reading".to_string(),
    );
}

fn push_command_step(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    kind: HistoricalSyncPlanStepKind,
    next_state: HistoricalSyncState,
    note: &str,
    generation: HistoricalSyncGeneration,
    ack_disposition: Option<HistoricalSyncAckDisposition>,
) {
    let (command_number, safety_gate, payload_expectation) = match kind {
        HistoricalSyncPlanStepKind::GetDataRange => (
            Some(34),
            Some(HistoricalSyncSafetyGate::ReadOnly),
            Some(payload_expectation_for_generation(generation)),
        ),
        HistoricalSyncPlanStepKind::SendHistoricalData => (
            Some(22),
            Some(HistoricalSyncSafetyGate::UserVisibleStateChange),
            Some(payload_expectation_for_generation(generation)),
        ),
        HistoricalSyncPlanStepKind::HistoricalDataResult => (
            Some(23),
            Some(HistoricalSyncSafetyGate::UserVisibleStateChange),
            Some(HistoricalSyncPayloadExpectation::HistoryEndAck {
                disposition: ack_disposition.unwrap_or(HistoricalSyncAckDisposition::Success),
            }),
        ),
        HistoricalSyncPlanStepKind::AbortHistoricalTransmits => (
            Some(20),
            Some(HistoricalSyncSafetyGate::UserVisibleStateChange),
            Some(HistoricalSyncPayloadExpectation::Empty),
        ),
        _ => (None, None, None),
    };

    push_step(
        steps,
        state_trace,
        state,
        kind,
        next_state,
        safety_gate,
        command_number,
        payload_expectation,
        None,
        note.to_string(),
    );
}

fn payload_expectation_for_generation(
    generation: HistoricalSyncGeneration,
) -> HistoricalSyncPayloadExpectation {
    match generation {
        HistoricalSyncGeneration::Gen5 => HistoricalSyncPayloadExpectation::Empty,
        HistoricalSyncGeneration::Gen4 => HistoricalSyncPayloadExpectation::ZeroByte,
    }
}

fn push_step(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    kind: HistoricalSyncPlanStepKind,
    next_state: HistoricalSyncState,
    safety_gate: Option<HistoricalSyncSafetyGate>,
    command_number: Option<u16>,
    payload_expectation: Option<HistoricalSyncPayloadExpectation>,
    event_name: Option<String>,
    note: String,
) {
    steps.push(HistoricalSyncPlanStep {
        kind,
        state_before: *state,
        state_after: next_state,
        safety_gate,
        command_number,
        payload_expectation,
        event_name,
        note,
    });
    *state = next_state;
    state_trace.push(next_state);
}

fn push_blocked(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    reason: &str,
    blocked_count: &mut usize,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::Blocked,
        HistoricalSyncState::Blocked,
        None,
        None,
        None,
        None,
        reason.to_string(),
    );
    *blocked_count += 1;
}

fn push_failed(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
    reason: &str,
    failed_count: &mut usize,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::Failed,
        HistoricalSyncState::Failed,
        None,
        None,
        None,
        None,
        reason.to_string(),
    );
    *failed_count += 1;
}

fn push_complete(
    steps: &mut Vec<HistoricalSyncPlanStep>,
    state_trace: &mut Vec<HistoricalSyncState>,
    state: &mut HistoricalSyncState,
) {
    push_step(
        steps,
        state_trace,
        state,
        HistoricalSyncPlanStepKind::Complete,
        HistoricalSyncState::Complete,
        None,
        None,
        None,
        None,
        "history_complete".to_string(),
    );
}

fn finish_report(
    input: &HistoricalSyncDryRunInput,
    input_valid: bool,
    state: HistoricalSyncState,
    state_trace: Vec<HistoricalSyncState>,
    steps: Vec<HistoricalSyncPlanStep>,
    planned_command_count: usize,
    blocked_count: usize,
    failed_count: usize,
    retry_count: usize,
    timeout_count: usize,
    cancel_count: usize,
    resume_count: usize,
    mut issues: Vec<String>,
) -> HistoricalSyncDryRunReport {
    if state == HistoricalSyncState::Blocked {
        push_issue_once(&mut issues, "history_sync_cancelled");
    }
    let next_actions = next_actions_for_issues(&issues);
    HistoricalSyncDryRunReport {
        schema: HISTORICAL_SYNC_DRY_RUN_REPORT_SCHEMA.to_string(),
        generated_by: "goose-historical-sync-dry-run".to_string(),
        generation: input.generation,
        pass: input_valid && state == HistoricalSyncState::Complete && issues.is_empty(),
        input_valid,
        device_connected: input.device_connected,
        safety_gate_ready: input.safety_gate_ready,
        request_data_range: input.request_data_range,
        state,
        state_trace,
        steps,
        planned_command_count,
        blocked_count,
        failed_count,
        retry_count,
        timeout_count,
        cancel_count,
        resume_count,
        issues,
        next_actions,
        retry: input.retry,
        timeout: input.timeout,
        cancel: input.cancel.clone(),
        resume: input.resume,
    }
}

fn next_actions_for_issues(issues: &[String]) -> Vec<HistoricalSyncNextAction> {
    let mut actions = Vec::new();
    for issue in issues {
        actions.push(HistoricalSyncNextAction {
            scope: "historical_sync_report".to_string(),
            reason: issue.clone(),
            action: issue_action(issue),
        });
    }
    dedupe_actions(actions)
}

fn issue_action(issue: &str) -> String {
    match issue {
        "unsupported schema goose.historical-sync-dry-run.v1" => {
            "Use goose.historical-sync-dry-run.v1 input before planning historical sync."
                .to_string()
        }
        "device_disconnected" => {
            "Connect the strap before running the historical sync dry run.".to_string()
        }
        "safety_gate_locked" => {
            "Unlock the historical sync safety gate, then rerun the dry run.".to_string()
        }
        "idle_timeout_retry_exhausted" => {
            "Inspect the transfer timeout and retry budget, then rerun from idle.".to_string()
        }
        "history_end_missing" => {
            "Capture HistoryEnd metadata before expecting the ack path.".to_string()
        }
        "history_end_missing_before_history_complete" => {
            "Capture HistoryEnd before HistoryComplete, then rerun the plan.".to_string()
        }
        "history_complete_missing" => {
            "Capture HistoryComplete metadata before treating the transfer as complete.".to_string()
        }
        "history_sync_cancelled" => {
            "Resume the historical sync or keep the plan blocked until the user restarts it."
                .to_string()
        }
        "resume_not_enabled" => {
            "Enable the resume plan before accepting ResumeRequested events.".to_string()
        }
        "resume_without_block" => "Resume only after the plan is blocked or cancelled.".to_string(),
        "malformed_response" => {
            "Fix the response parser or capture the malformed frame again.".to_string()
        }
        "duplicate_transfer" => {
            "Abort the duplicate transfer and restart from idle once.".to_string()
        }
        value if value.ends_with("_must_be_positive") => {
            "Set positive idle, transfer, and ack timeout values before planning the sync."
                .to_string()
        }
        value if value.starts_with("unsupported schema ") => {
            "Use goose.historical-sync-dry-run.v1 input before planning historical sync."
                .to_string()
        }
        _ => format!("Resolve historical sync issue {issue}, then rerun the dry run."),
    }
}

pub(crate) fn physical_validation_next_actions(issues: &[String]) -> Vec<HistoricalSyncNextAction> {
    dedupe_actions(
        issues
            .iter()
            .map(|issue| HistoricalSyncNextAction {
                scope: "historical_sync_physical_validation".to_string(),
                reason: issue.clone(),
                action: physical_validation_issue_action(issue),
            })
            .collect(),
    )
}

fn physical_validation_issue_action(issue: &str) -> String {
    match issue {
        "capture_session_id_required" => {
            "Attach the validation bundle to a concrete physical capture session id.".to_string()
        }
        "historical_service_uuid_missing" => {
            "Capture a GATT dump that includes the expected WHOOP historical sync service UUID for this generation.".to_string()
        }
        "historical_characteristic_roles_incomplete" => {
            "Annotate command, data notification, and event notification characteristic roles from the physical GATT dump.".to_string()
        }
        "historical_notification_behavior_missing" => {
            "Subscribe to the data and event characteristics and record enabled notification state before historical transfer.".to_string()
        }
        "auth_session_handshake_missing" => {
            "Record connect, authenticated, and subscribed session transitions before sending historical commands.".to_string()
        }
        "historical_command_flow_incomplete" => {
            "Capture SendHistoricalData, HistoryStart, HistoryEnd, HistoryComplete, and HistoricalDataResult in order.".to_string()
        }
        "historical_event_order_unproven" => {
            "Attach ordered connected, authenticated, subscribed, SendHistoricalData, HistoryStart, HistoryEnd, HistoryComplete, and HistoricalDataResult observations from one physical sync.".to_string()
        }
        "historical_evidence_session_mismatch" => {
            "Set every physical notification, session event, command, metadata event, timestamp row, and raw evidence anchor to the same capture_session_id as the validation bundle.".to_string()
        }
        "historical_raw_evidence_fingerprints_missing" => {
            "Anchor every physical notification, session event, command, metadata event, and timestamp row to a captured evidence id with a valid SHA-256 fingerprint.".to_string()
        }
        "historical_motion_timestamp_fields_unproven" => {
            "Capture historical motion packets with device timestamp fields whose UTC sample times match the device timestamp values and differ from import time.".to_string()
        }
        "historical_heart_rate_timestamp_fields_unproven" => {
            "Capture historical heart-rate packets with device timestamp fields whose UTC sample times match the device timestamp values and differ from import time.".to_string()
        }
        "historical_timestamp_fields_unproven" => {
            "Capture historical motion and heart-rate packets with device timestamps whose UTC sample times match the device timestamp fields and differ from import time.".to_string()
        }
        value if value.starts_with("unsupported schema ") => {
            "Use goose.historical-sync-physical-validation.v1 for physical evidence bundles.".to_string()
        }
        _ => format!("Resolve physical historical sync validation issue {issue}."),
    }
}

fn expected_historical_service_uuid(generation: HistoricalSyncGeneration) -> &'static str {
    match generation {
        HistoricalSyncGeneration::Gen4 => "610800018d6d82b8614a1c8cb0f8dcc6",
        HistoricalSyncGeneration::Gen5 => "fd4b0001cce1403393ce002d5875f58a",
    }
}

fn has_characteristic_role(input: &HistoricalSyncPhysicalValidationInput, role: &str) -> bool {
    input.characteristics.iter().any(|characteristic| {
        normalize_role(&characteristic.role) == role
            && normalize_uuid(&characteristic.service_uuid)
                == expected_historical_service_uuid(input.generation)
            && !characteristic.characteristic_uuid.trim().is_empty()
            && characteristic_properties_match_role(characteristic, role)
    })
}

fn characteristic_for_role(
    input: &HistoricalSyncPhysicalValidationInput,
    role: &str,
) -> Option<String> {
    input
        .characteristics
        .iter()
        .find(|characteristic| {
            normalize_role(&characteristic.role) == role
                && normalize_uuid(&characteristic.service_uuid)
                    == expected_historical_service_uuid(input.generation)
                && !characteristic.characteristic_uuid.trim().is_empty()
        })
        .map(|characteristic| normalize_uuid(&characteristic.characteristic_uuid))
}

fn characteristic_properties_match_role(
    characteristic: &HistoricalSyncCharacteristicEvidence,
    role: &str,
) -> bool {
    match role {
        "command_to_strap" => characteristic_has_any_property(
            characteristic,
            &["write", "write_without_response", "writewithresponse"],
        ),
        "data_from_strap" | "event_from_strap" => {
            characteristic_has_any_property(characteristic, &["notify", "indicate"])
        }
        _ => true,
    }
}

fn characteristic_has_any_property(
    characteristic: &HistoricalSyncCharacteristicEvidence,
    expected: &[&str],
) -> bool {
    characteristic.properties.iter().any(|property| {
        let normalized = normalize_history_marker(property);
        expected.iter().any(|expected| normalized == *expected)
    })
}

fn observed_event(events: &[HistoricalSyncObservedEvent], name: &str) -> bool {
    events
        .iter()
        .any(|event| normalize_history_marker(&event.name) == name)
}

fn observed_metadata(events: &[HistoricalSyncObservedEvent], name: &str) -> bool {
    observed_event(events, name)
}

fn observed_command(commands: &[HistoricalSyncObservedCommand], command: &str) -> bool {
    commands
        .iter()
        .any(|event| normalize_history_marker(&event.command) == command && event.response_observed)
}

fn physical_event_order_confirmed(input: &HistoricalSyncPhysicalValidationInput) -> bool {
    let Some(connected) = event_sequence(&input.auth_events, "connected") else {
        return false;
    };
    let Some(authenticated) = event_sequence(&input.auth_events, "authenticated") else {
        return false;
    };
    let Some(subscribed) = event_sequence(&input.auth_events, "subscribed") else {
        return false;
    };
    let Some(send_historical_data) =
        command_sequence(&input.command_events, "send_historical_data", true)
    else {
        return false;
    };
    let Some(history_start) = event_sequence(&input.metadata_events, "history_start") else {
        return false;
    };
    let Some(history_end) = event_sequence(&input.metadata_events, "history_end") else {
        return false;
    };
    let Some(history_complete) = event_sequence(&input.metadata_events, "history_complete") else {
        return false;
    };
    let Some(historical_data_result) =
        command_sequence(&input.command_events, "historical_data_result", true)
    else {
        return false;
    };
    connected < authenticated
        && authenticated < subscribed
        && subscribed < send_historical_data
        && send_historical_data < history_start
        && history_start < history_end
        && history_end < history_complete
        && history_complete < historical_data_result
}

fn physical_evidence_session_confirmed(input: &HistoricalSyncPhysicalValidationInput) -> bool {
    let expected = input.capture_session_id.trim();
    if expected.is_empty() {
        return false;
    }
    input
        .notification_subscriptions
        .iter()
        .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
        && input
            .auth_events
            .iter()
            .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
        && input
            .command_events
            .iter()
            .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
        && input
            .metadata_events
            .iter()
            .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
        && input
            .timestamp_evidence
            .iter()
            .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
        && input
            .raw_evidence_anchors
            .iter()
            .all(|row| capture_session_matches(row.capture_session_id.as_deref(), expected))
}

fn capture_session_matches(observed: Option<&str>, expected: &str) -> bool {
    observed.is_some_and(|value| value.trim() == expected)
}

fn physical_raw_evidence_anchored(input: &HistoricalSyncPhysicalValidationInput) -> bool {
    let expected_capture_session = input.capture_session_id.trim();
    if expected_capture_session.is_empty() {
        return false;
    }
    let anchors_valid = !input.raw_evidence_anchors.is_empty()
        && input.raw_evidence_anchors.iter().all(|anchor| {
            !anchor.evidence_id.trim().is_empty()
                && is_sha256_hex(&anchor.sha256)
                && !anchor.observation_kind.trim().is_empty()
                && !anchor.observation_name.trim().is_empty()
                && capture_session_matches(
                    anchor.capture_session_id.as_deref(),
                    expected_capture_session,
                )
        });
    anchors_valid
        && input
            .notification_subscriptions
            .iter()
            .filter(|row| row.enabled)
            .all(|row| {
                raw_anchor_exists(
                    input,
                    "notification_subscription",
                    &normalize_uuid(&row.characteristic_uuid),
                    None,
                )
            })
        && input.auth_events.iter().all(|row| {
            raw_anchor_exists(
                input,
                "auth_event",
                &normalize_history_marker(&row.name),
                Some(row.sequence),
            )
        })
        && input.command_events.iter().all(|row| {
            raw_anchor_exists(
                input,
                "command_event",
                &normalize_history_marker(&row.command),
                Some(row.sequence),
            )
        })
        && input.metadata_events.iter().all(|row| {
            raw_anchor_exists(
                input,
                "metadata_event",
                &normalize_history_marker(&row.name),
                Some(row.sequence),
            )
        })
        && input.timestamp_evidence.iter().all(|row| {
            raw_anchor_exists(
                input,
                "timestamp_evidence",
                &timestamp_anchor_name(row),
                None,
            )
        })
}

fn raw_anchor_exists(
    input: &HistoricalSyncPhysicalValidationInput,
    observation_kind: &str,
    observation_name: &str,
    sequence: Option<u32>,
) -> bool {
    input.raw_evidence_anchors.iter().any(|anchor| {
        normalize_history_marker(&anchor.observation_kind) == observation_kind
            && normalize_history_marker(&anchor.observation_name)
                == normalize_history_marker(observation_name)
            && anchor.sequence == sequence
    })
}

fn timestamp_anchor_name(row: &HistoricalSyncTimestampEvidence) -> String {
    format!(
        "{}:{}",
        normalize_history_marker(&row.packet_kind),
        normalize_history_marker(&row.source_signal)
    )
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn event_sequence(events: &[HistoricalSyncObservedEvent], name: &str) -> Option<u32> {
    events
        .iter()
        .filter(|event| normalize_history_marker(&event.name) == name)
        .map(|event| event.sequence)
        .min()
}

fn command_sequence(
    commands: &[HistoricalSyncObservedCommand],
    command: &str,
    require_response: bool,
) -> Option<u32> {
    commands
        .iter()
        .filter(|event| {
            normalize_history_marker(&event.command) == command
                && (!require_response || event.response_observed)
        })
        .map(|event| event.sequence)
        .min()
}

fn timestamp_packets_confirmed(evidence: &[HistoricalSyncTimestampEvidence]) -> bool {
    let motion_rows = timestamp_packet_confirmed_rows(evidence, "motion");
    let heart_rate_rows = timestamp_packet_confirmed_rows(evidence, "heart_rate");
    motion_rows.iter().any(|motion_index| {
        heart_rate_rows
            .iter()
            .any(|hr_index| hr_index != motion_index)
    })
}

fn timestamp_packet_confirmed_rows(
    evidence: &[HistoricalSyncTimestampEvidence],
    required_signal: &str,
) -> Vec<usize> {
    evidence
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let packet_kind = normalize_history_marker(&row.packet_kind);
            let source_signal = normalize_history_marker(&row.source_signal);
            let Some(device_timestamp_seconds) = row.device_timestamp_seconds else {
                return None;
            };
            let Some(sample_time) = row.sample_time.as_deref() else {
                return None;
            };
            let Some(sample_time_unix_ms) = parse_rfc3339_utc_unix_ms(sample_time) else {
                return None;
            };
            let Some(captured_at_unix_ms) = parse_rfc3339_utc_unix_ms(&row.captured_at) else {
                return None;
            };
            let device_timestamp_subseconds = row.device_timestamp_subseconds.unwrap_or(0);
            if device_timestamp_subseconds > 999 {
                return None;
            }
            let device_timestamp_unix_ms = i64::from(device_timestamp_seconds) * 1_000
                + i64::from(device_timestamp_subseconds);
            (timestamp_row_matches_signal(&packet_kind, &source_signal, required_signal)
                && plausible_unix_timestamp_seconds(device_timestamp_seconds)
                && row.sample_time_source.as_deref() == Some("device_timestamp")
                && sample_time_unix_ms == device_timestamp_unix_ms
                && sample_time_unix_ms != captured_at_unix_ms)
                .then_some(index)
        })
        .collect()
}

fn timestamp_row_matches_signal(
    packet_kind: &str,
    source_signal: &str,
    required_signal: &str,
) -> bool {
    let has_motion = packet_kind.contains("motion") || source_signal.contains("motion");
    let has_heart_rate = packet_kind.contains("heart_rate") || source_signal.contains("heart_rate");
    let source_has_motion = source_signal.contains("motion");
    let source_has_heart_rate = source_signal.contains("heart_rate");
    match required_signal {
        "motion" => source_has_motion && has_motion && !has_heart_rate,
        "heart_rate" => source_has_heart_rate && has_heart_rate && !has_motion,
        _ => source_signal.contains(required_signal),
    }
}

fn plausible_unix_timestamp_seconds(seconds: u32) -> bool {
    (946_684_800..=4_102_444_800).contains(&seconds)
}

fn parse_rfc3339_utc_unix_ms(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let seconds_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let (second_text, fraction_text) = seconds_part
        .split_once('.')
        .map_or((seconds_part, ""), |(seconds, fraction)| {
            (seconds, fraction)
        });
    let second = second_text.parse::<u32>().ok()?;
    let millis = parse_millis_fraction(fraction_text)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    days.checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)?
        .checked_add(i64::from(minute) * 60_000)?
        .checked_add(i64::from(second) * 1_000)?
        .checked_add(i64::from(millis))
}

fn parse_millis_fraction(value: &str) -> Option<u32> {
    if value.is_empty() {
        return Some(0);
    }
    if !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let mut millis = 0_u32;
    let mut factor = 100_u32;
    for character in value.chars().take(3) {
        millis += character.to_digit(10)? * factor;
        factor /= 10;
    }
    Some(millis)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

fn normalize_uuid(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn normalize_role(value: &str) -> String {
    normalize_history_marker(value)
}

fn dedupe_actions(actions: Vec<HistoricalSyncNextAction>) -> Vec<HistoricalSyncNextAction> {
    let mut deduped = Vec::new();
    for action in actions {
        if !deduped.iter().any(|existing| existing == &action) {
            deduped.push(action);
        }
    }
    deduped
}

fn push_issue_once(issues: &mut Vec<String>, issue: impl Into<String>) {
    let issue = issue.into();
    if !issues.iter().any(|existing| existing == &issue) {
        issues.push(issue);
    }
}

fn normalize_history_marker(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_separator = true;
    let mut previous_was_lower_or_digit = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase()
                && previous_was_lower_or_digit
                && !previous_was_separator
            {
                normalized.push('_');
            }
            normalized.push(character.to_ascii_lowercase());
            previous_was_separator = false;
            previous_was_lower_or_digit =
                character.is_ascii_lowercase() || character.is_ascii_digit();
        } else {
            if !previous_was_separator && !normalized.is_empty() {
                normalized.push('_');
            }
            previous_was_separator = true;
            previous_was_lower_or_digit = false;
        }
    }
    normalized.trim_matches('_').to_string()
}

fn default_true() -> bool {
    true
}

fn default_retry_budget() -> u8 {
    1
}
