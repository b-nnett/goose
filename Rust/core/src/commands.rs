use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    GooseError, GooseResult,
    protocol::{DeviceType, ParsedPayload, parse_frame_hex},
};

pub const COMMAND_CAPTURE_PLAN_REPORT_SCHEMA: &str = "goose.command-capture-plan-report.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandRiskGate {
    ReadOnly,
    UserVisibleStateChange,
    CriticalStateChange,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CommandDefinition {
    pub id: &'static str,
    pub command_number: Option<u16>,
    pub family: &'static str,
    pub risk_gate: CommandRiskGate,
    pub description: &'static str,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandEvidence {
    #[serde(alias = "command_id", alias = "id")]
    pub command: String,
    #[serde(default, alias = "officialCaptureCount")]
    pub official_capture_count: u32,
    #[serde(
        default,
        alias = "evidenceSource",
        alias = "source_kind",
        alias = "sourceKind"
    )]
    pub evidence_source: Option<String>,
    #[serde(
        default,
        alias = "provenance",
        alias = "provenanceJson",
        deserialize_with = "deserialize_optional_provenance_json"
    )]
    pub provenance_json: Option<String>,
    #[serde(default, alias = "officialFrameHex")]
    pub official_frame_hex: Option<String>,
    #[serde(default, alias = "localFrameHex")]
    pub local_frame_hex: Option<String>,
    #[serde(
        default,
        alias = "officialServiceUuid",
        alias = "official_service",
        alias = "service_uuid",
        alias = "serviceUuid"
    )]
    pub official_service_uuid: Option<String>,
    #[serde(
        default,
        alias = "localServiceUuid",
        alias = "local_service",
        alias = "local_service_uuid"
    )]
    pub local_service_uuid: Option<String>,
    #[serde(
        default,
        alias = "officialCharacteristicUuid",
        alias = "official_characteristic",
        alias = "characteristic_uuid",
        alias = "characteristicUuid"
    )]
    pub official_characteristic_uuid: Option<String>,
    #[serde(
        default,
        alias = "localCharacteristicUuid",
        alias = "local_characteristic",
        alias = "local_characteristic_uuid"
    )]
    pub local_characteristic_uuid: Option<String>,
    #[serde(
        default,
        alias = "officialWriteType",
        alias = "official_write",
        alias = "write_type",
        alias = "writeType"
    )]
    pub official_write_type: Option<String>,
    #[serde(
        default,
        alias = "localWriteType",
        alias = "local_write",
        alias = "local_write_type"
    )]
    pub local_write_type: Option<String>,
    #[serde(default, alias = "officialResponseFrameHex")]
    pub official_response_frame_hex: Option<String>,
    #[serde(default, alias = "officialFailureResponseFrameHex")]
    pub official_failure_response_frame_hex: Option<String>,
    #[serde(
        default,
        alias = "triggeringUiAction",
        alias = "official_ui_action",
        alias = "officialUiAction",
        alias = "ui_action",
        alias = "uiAction"
    )]
    pub triggering_ui_action: Option<String>,
    #[serde(default, alias = "responseParser")]
    pub response_parser: bool,
    #[serde(default, alias = "failureParser")]
    pub failure_parser: bool,
    #[serde(default, alias = "visibleUserIntent")]
    pub visible_user_intent: bool,
    #[serde(default, alias = "visibleConfirmation")]
    pub visible_confirmation: bool,
    #[serde(default, alias = "eventLogging")]
    pub logging: bool,
    #[serde(default, alias = "timeoutBehavior")]
    pub timeout_behavior: bool,
    #[serde(default, alias = "rollbackPlan")]
    pub rollback_plan: bool,
    #[serde(default, alias = "explicitApproval")]
    pub explicit_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandValidationReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    #[serde(default)]
    pub evidence_valid: bool,
    #[serde(default)]
    pub all_direct_sends_ready: bool,
    pub direct_send_ready_count: usize,
    pub blocked_count: usize,
    #[serde(default)]
    pub evidence_source_summary: Vec<CommandEvidenceSourceSummary>,
    pub commands: Vec<CommandValidationResult>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidenceSourceSummary {
    pub evidence_source: String,
    #[serde(default)]
    pub capture_kind: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    pub count: usize,
    pub trusted_for_promotion_count: usize,
    pub blocked_for_source_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandValidationResult {
    pub command: String,
    pub command_number: Option<u16>,
    pub family: String,
    pub risk_gate: CommandRiskGate,
    pub description: String,
    pub direct_send_ready: bool,
    pub missing_requirements: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub next_capture_actions: Vec<CommandNextCaptureAction>,
    #[serde(default)]
    pub validated_local_frame_hex: Option<String>,
    #[serde(default)]
    pub validated_official_frame_hex: Option<String>,
    #[serde(default)]
    pub validated_service_uuid: Option<String>,
    #[serde(default)]
    pub validated_characteristic_uuid: Option<String>,
    #[serde(default)]
    pub validated_write_type: Option<String>,
    #[serde(default)]
    pub validated_evidence_source: Option<String>,
    #[serde(default)]
    pub validated_capture_kind: Option<String>,
    #[serde(default)]
    pub validated_owner: Option<String>,
    #[serde(default)]
    pub validated_provenance_json: Option<String>,
    #[serde(default)]
    pub validated_triggering_ui_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandNextCaptureAction {
    pub requirement: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandCapturePlanReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub requested_commands_valid: bool,
    pub validation_records_valid: bool,
    pub all_selected_gates_ready: bool,
    pub critical_gates_ready: bool,
    pub capture_actions_ready: bool,
    pub command_count: usize,
    pub ready_count: usize,
    pub locked_count: usize,
    pub critical_locked_count: usize,
    pub action_count: usize,
    pub family_summaries: Vec<CommandCapturePlanFamilySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command_focus: Option<CommandCapturePlanAction>,
    pub actions: Vec<CommandCapturePlanAction>,
    pub gates: BTreeMap<String, CommandDirectSendGate>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandCapturePlanFamilySummary {
    pub family: String,
    pub command_count: usize,
    pub ready_count: usize,
    pub locked_count: usize,
    pub critical_locked_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandCapturePlanAction {
    pub command: String,
    pub family: String,
    pub risk_gate: CommandRiskGate,
    pub requirement: String,
    pub action: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default)]
struct CommandCapturePlanFamilyAccumulator {
    command_count: usize,
    ready_count: usize,
    locked_count: usize,
    critical_locked_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDirectSendGate {
    pub schema: String,
    pub command: String,
    #[serde(default)]
    pub command_number: Option<u16>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub risk_gate: Option<CommandRiskGate>,
    pub direct_send_allowed: bool,
    pub missing_requirements: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub next_capture_actions: Vec<CommandNextCaptureAction>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub validated_local_frame_hex: Option<String>,
    #[serde(default)]
    pub validated_service_uuid: Option<String>,
    #[serde(default)]
    pub validated_characteristic_uuid: Option<String>,
    #[serde(default)]
    pub validated_write_type: Option<String>,
    #[serde(default)]
    pub validated_evidence_source: Option<String>,
    #[serde(default)]
    pub validated_capture_kind: Option<String>,
    #[serde(default)]
    pub validated_owner: Option<String>,
    #[serde(default)]
    pub validated_provenance_json: Option<String>,
    #[serde(default)]
    pub validated_triggering_ui_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDirectSendPreflightInput {
    pub command: String,
    pub now_unix_ms: u64,
    #[serde(default)]
    pub override_expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub visible_user_intent: bool,
    #[serde(default)]
    pub dry_run_bytes_shown: bool,
    #[serde(default)]
    pub dry_run_frame_hex: Option<String>,
    #[serde(default)]
    pub dry_run_service_uuid: Option<String>,
    #[serde(default)]
    pub dry_run_characteristic_uuid: Option<String>,
    #[serde(default)]
    pub dry_run_write_type: Option<String>,
    #[serde(default)]
    pub session_log_ready: bool,
    #[serde(default)]
    pub connection_state: Option<String>,
    #[serde(default)]
    pub active_device_id: Option<String>,
    #[serde(default)]
    pub critical_visible_confirmation: bool,
    #[serde(default)]
    pub critical_explicit_approval: bool,
    #[serde(default)]
    pub critical_rollback_or_restore_acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDirectSendPreflight {
    pub schema: String,
    pub command: String,
    pub direct_send_allowed: bool,
    pub gate: CommandDirectSendGate,
    pub missing_requirements: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub override_expires_in_ms: Option<u64>,
    #[serde(default)]
    pub dry_run_frame_hex: Option<String>,
    #[serde(default)]
    pub dry_run_service_uuid: Option<String>,
    #[serde(default)]
    pub dry_run_characteristic_uuid: Option<String>,
    #[serde(default)]
    pub dry_run_write_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvidenceTemplate {
    pub schema: String,
    pub evidence: Vec<CommandEvidence>,
}

#[derive(Debug, Clone)]
pub struct CommandEmulatorLogEvidenceOptions {
    pub write_type: String,
    pub visible_user_intent: bool,
    pub triggering_ui_action: Option<String>,
    pub visible_confirmation: bool,
    pub rollback_plan: bool,
    pub explicit_approval: bool,
    pub mirror_local_frame: bool,
    pub capture_app: String,
    pub capture_kind: String,
    pub owner: String,
}

impl Default for CommandEmulatorLogEvidenceOptions {
    fn default() -> Self {
        Self {
            write_type: "with_response".to_string(),
            visible_user_intent: false,
            triggering_ui_action: None,
            visible_confirmation: false,
            rollback_plan: false,
            explicit_approval: false,
            mirror_local_frame: false,
            capture_app: "whoop_official".to_string(),
            capture_kind: "official_app_to_macos_emulator".to_string(),
            owner: "user".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEmulatorLogEvidenceReport {
    pub schema: String,
    pub generated_by: String,
    #[serde(default)]
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub log_lines_ready: bool,
    #[serde(default)]
    pub official_writes_parsed: bool,
    #[serde(default)]
    pub responses_paired: bool,
    #[serde(default)]
    pub trusted_capture_context: bool,
    #[serde(default)]
    pub official_capture_ready: bool,
    #[serde(default)]
    pub local_frame_match_ready: bool,
    #[serde(default)]
    pub direct_validation_ready: bool,
    pub source_capture: String,
    pub source_log: String,
    pub device_type: String,
    pub evidence_source: String,
    pub capture_kind: String,
    pub owner: String,
    pub line_count: usize,
    pub transaction_count: usize,
    pub evidence_count: usize,
    pub evidence: Vec<CommandEvidence>,
    pub issues: Vec<String>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CommandEmulatorLogNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEmulatorLogNextAction {
    pub requirement: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLocalFrameCandidate {
    #[serde(
        alias = "id",
        default,
        deserialize_with = "deserialize_command_identifier"
    )]
    pub command: String,
    #[serde(
        default,
        alias = "commandId",
        deserialize_with = "deserialize_optional_command_identifier",
        skip_serializing_if = "Option::is_none"
    )]
    pub command_id: Option<String>,
    #[serde(
        default,
        alias = "localFrameHex",
        alias = "dry_run_frame_hex",
        alias = "dryRunFrameHex",
        alias = "frame_hex",
        alias = "frameHex"
    )]
    pub local_frame_hex: Option<String>,
    #[serde(
        default,
        alias = "localServiceUuid",
        alias = "dry_run_service_uuid",
        alias = "dryRunServiceUuid",
        alias = "service_uuid",
        alias = "serviceUuid"
    )]
    pub local_service_uuid: Option<String>,
    #[serde(
        default,
        alias = "localCharacteristicUuid",
        alias = "dry_run_characteristic_uuid",
        alias = "dryRunCharacteristicUuid",
        alias = "characteristic_uuid",
        alias = "characteristicUuid"
    )]
    pub local_characteristic_uuid: Option<String>,
    #[serde(
        default,
        alias = "localWriteType",
        alias = "dry_run_write_type",
        alias = "dryRunWriteType",
        alias = "write_type",
        alias = "writeType"
    )]
    pub local_write_type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(
        default,
        alias = "provenance",
        alias = "provenanceJson",
        deserialize_with = "deserialize_optional_provenance_json"
    )]
    pub provenance_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CommandLocalFrameCandidateFile {
    candidates: Option<Vec<CommandLocalFrameCandidate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLocalFrameMatchReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub comparisons_ready: bool,
    #[serde(default)]
    pub all_frames_matched: bool,
    #[serde(default)]
    pub promotion_ready: bool,
    pub evidence_count: usize,
    pub candidate_count: usize,
    pub matched_count: usize,
    pub blocked_count: usize,
    pub evidence: Vec<CommandEvidence>,
    pub comparisons: Vec<CommandLocalFrameComparison>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<CommandLocalFrameNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLocalFrameComparison {
    pub command: String,
    pub matched: bool,
    pub reason: String,
    #[serde(default)]
    pub official_frame_hex: Option<String>,
    #[serde(default)]
    pub local_frame_hex: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandLocalFrameNextAction {
    #[serde(default)]
    pub command: String,
    pub requirement: String,
    pub action: String,
}

pub const COMMAND_DEFINITIONS: &[CommandDefinition] = &[
    CommandDefinition {
        id: "link_valid",
        command_number: Some(1),
        family: "identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read or confirm the link-valid protocol state.",
    },
    CommandDefinition {
        id: "get_max_protocol_version",
        command_number: Some(2),
        family: "identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read the maximum supported strap protocol version.",
    },
    CommandDefinition {
        id: "toggle_realtime_hr",
        command_number: Some(3),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle realtime heart-rate packets.",
    },
    CommandDefinition {
        id: "report_version_info",
        command_number: Some(7),
        family: "identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap version information.",
    },
    CommandDefinition {
        id: "set_clock",
        command_number: Some(10),
        family: "clock_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Set strap RTC seconds and subseconds.",
    },
    CommandDefinition {
        id: "get_clock",
        command_number: Some(11),
        family: "clock_sync",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap RTC seconds and subseconds.",
    },
    CommandDefinition {
        id: "toggle_generic_hr_profile",
        command_number: Some(14),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle the generic BLE heart-rate profile path.",
    },
    CommandDefinition {
        id: "toggle_r7_data_collection",
        command_number: Some(16),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle R7 data collection.",
    },
    CommandDefinition {
        id: "run_haptic_pattern_maverick",
        command_number: Some(19),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Run a Maverick haptic pattern.",
    },
    CommandDefinition {
        id: "abort_historical_transmits",
        command_number: Some(20),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Abort active historical transmissions.",
    },
    CommandDefinition {
        id: "get_hello",
        command_number: Some(145),
        family: "identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap identity and protocol hello.",
    },
    CommandDefinition {
        id: "get_battery_level",
        command_number: Some(26),
        family: "battery",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap battery level.",
    },
    CommandDefinition {
        id: "get_data_range",
        command_number: Some(34),
        family: "historical_sync",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read available historical data range.",
    },
    CommandDefinition {
        id: "send_historical_data",
        command_number: Some(22),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Request historical data transfer.",
    },
    CommandDefinition {
        id: "historical_data_result",
        command_number: Some(23),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Send historical data transfer result or acknowledgement.",
    },
    CommandDefinition {
        id: "set_read_pointer",
        command_number: Some(33),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Move the strap historical read pointer.",
    },
    CommandDefinition {
        id: "get_hello_harvard",
        command_number: Some(35),
        family: "identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read legacy Harvard/Gen4 hello information.",
    },
    CommandDefinition {
        id: "start_firmware_load",
        command_number: Some(36),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Start legacy firmware image load.",
    },
    CommandDefinition {
        id: "load_firmware_data",
        command_number: Some(37),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Send a legacy firmware image data chunk.",
    },
    CommandDefinition {
        id: "process_firmware_image",
        command_number: Some(38),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Ask strap to process a legacy firmware image.",
    },
    CommandDefinition {
        id: "set_led_drive",
        command_number: Some(39),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write optical LED drive configuration.",
    },
    CommandDefinition {
        id: "get_led_drive",
        command_number: Some(40),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read optical LED drive configuration.",
    },
    CommandDefinition {
        id: "set_tia_gain",
        command_number: Some(41),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write optical TIA gain configuration.",
    },
    CommandDefinition {
        id: "get_tia_gain",
        command_number: Some(42),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read optical TIA gain configuration.",
    },
    CommandDefinition {
        id: "set_bias_offset",
        command_number: Some(43),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write optical bias offset configuration.",
    },
    CommandDefinition {
        id: "get_bias_offset",
        command_number: Some(44),
        family: "optical_afe_config",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read optical bias offset configuration.",
    },
    CommandDefinition {
        id: "set_dp_type",
        command_number: Some(52),
        family: "data_packet_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Set historical data-packet type selection.",
    },
    CommandDefinition {
        id: "force_dp_type",
        command_number: Some(53),
        family: "data_packet_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Force historical data-packet type selection.",
    },
    CommandDefinition {
        id: "send_r10_r11_realtime",
        command_number: Some(63),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle or request R10/R11 realtime raw packets.",
    },
    CommandDefinition {
        id: "set_alarm_time",
        command_number: Some(66),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Set strap alarm time.",
    },
    CommandDefinition {
        id: "get_alarm_time",
        command_number: Some(67),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap alarm configuration.",
    },
    CommandDefinition {
        id: "run_alarm",
        command_number: Some(68),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Trigger an alarm pattern.",
    },
    CommandDefinition {
        id: "disable_alarm",
        command_number: Some(69),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Disable strap alarm.",
    },
    CommandDefinition {
        id: "run_haptics_pattern",
        command_number: Some(79),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Run a selected haptic pattern.",
    },
    CommandDefinition {
        id: "get_advertising_name_harvard",
        command_number: Some(76),
        family: "device_identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read legacy Harvard advertising name.",
    },
    CommandDefinition {
        id: "set_advertising_name_harvard",
        command_number: Some(77),
        family: "device_identity",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Set legacy Harvard advertising name.",
    },
    CommandDefinition {
        id: "stop_haptics",
        command_number: Some(122),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Stop active haptics.",
    },
    CommandDefinition {
        id: "get_all_haptics_pattern",
        command_number: Some(80),
        family: "alarm_haptics",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read available strap haptic patterns.",
    },
    CommandDefinition {
        id: "select_wrist",
        command_number: Some(123),
        family: "wrist_selection",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Change left/right wrist selection.",
    },
    CommandDefinition {
        id: "start_raw_data",
        command_number: Some(81),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Start realtime raw data stream.",
    },
    CommandDefinition {
        id: "stop_raw_data",
        command_number: Some(82),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Stop realtime raw data stream.",
    },
    CommandDefinition {
        id: "verify_firmware_image",
        command_number: Some(83),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Verify a firmware image write/read step.",
    },
    CommandDefinition {
        id: "get_body_location_and_status",
        command_number: Some(84),
        family: "wrist_selection",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read body-location and strap status.",
    },
    CommandDefinition {
        id: "enter_high_freq_sync",
        command_number: Some(96),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Enter high-frequency sync mode.",
    },
    CommandDefinition {
        id: "exit_high_freq_sync",
        command_number: Some(97),
        family: "historical_sync",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Exit high-frequency sync mode.",
    },
    CommandDefinition {
        id: "get_extended_battery_info",
        command_number: Some(98),
        family: "battery",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read extended battery and fuel-gauge information.",
    },
    CommandDefinition {
        id: "toggle_imu_mode_historical",
        command_number: Some(105),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle historical IMU data stream mode.",
    },
    CommandDefinition {
        id: "toggle_imu_mode",
        command_number: Some(106),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle realtime IMU stream mode.",
    },
    CommandDefinition {
        id: "enable_optical_data",
        command_number: Some(107),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Enable realtime optical R20 data.",
    },
    CommandDefinition {
        id: "toggle_optical_mode",
        command_number: Some(108),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle optical stream mode.",
    },
    CommandDefinition {
        id: "start_device_config_key_exchange",
        command_number: Some(115),
        family: "device_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Start persistent device-config key exchange.",
    },
    CommandDefinition {
        id: "send_next_device_config",
        command_number: Some(116),
        family: "device_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Send the next persistent device-config key/value.",
    },
    CommandDefinition {
        id: "start_feature_flag_key_exchange",
        command_number: Some(117),
        family: "feature_flags",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Start feature-flag key exchange.",
    },
    CommandDefinition {
        id: "send_next_feature_flag",
        command_number: Some(118),
        family: "feature_flags",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Send the next feature-flag key/value.",
    },
    CommandDefinition {
        id: "set_device_config_value",
        command_number: Some(119),
        family: "device_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write a device configuration value.",
    },
    CommandDefinition {
        id: "set_feature_flag_value",
        command_number: Some(120),
        family: "feature_flags",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write a feature flag value.",
    },
    CommandDefinition {
        id: "get_device_config_value",
        command_number: Some(121),
        family: "device_config",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read a device configuration value.",
    },
    CommandDefinition {
        id: "toggle_labrador_data_generation",
        command_number: Some(124),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle raw ECG/Labrador packet generation.",
    },
    CommandDefinition {
        id: "toggle_labrador_raw_save",
        command_number: Some(125),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle raw ECG/Labrador save behavior.",
    },
    CommandDefinition {
        id: "get_feature_flag_value",
        command_number: Some(128),
        family: "feature_flags",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read a feature flag value.",
    },
    CommandDefinition {
        id: "set_research_packet",
        command_number: Some(131),
        family: "research_packet",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Write research packet configuration.",
    },
    CommandDefinition {
        id: "get_research_packet",
        command_number: Some(132),
        family: "research_packet",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read research packet configuration.",
    },
    CommandDefinition {
        id: "toggle_labrador_filtered",
        command_number: Some(139),
        family: "sensor_stream",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Toggle filtered ECG/Labrador data stream.",
    },
    CommandDefinition {
        id: "set_advertising_name",
        command_number: Some(140),
        family: "device_identity",
        risk_gate: CommandRiskGate::UserVisibleStateChange,
        description: "Set strap advertising name.",
    },
    CommandDefinition {
        id: "get_advertising_name",
        command_number: Some(141),
        family: "device_identity",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read strap advertising name.",
    },
    CommandDefinition {
        id: "start_firmware_load_new",
        command_number: Some(142),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Start a firmware image load.",
    },
    CommandDefinition {
        id: "load_firmware_data_new",
        command_number: Some(143),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Send a firmware image data chunk.",
    },
    CommandDefinition {
        id: "process_firmware_image_new",
        command_number: Some(144),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Ask strap to process a loaded firmware image.",
    },
    CommandDefinition {
        id: "get_battery_pack_info",
        command_number: Some(151),
        family: "battery",
        risk_gate: CommandRiskGate::ReadOnly,
        description: "Read battery-pack information.",
    },
    CommandDefinition {
        id: "toggle_persistent_r20",
        command_number: Some(153),
        family: "persistent_sensor_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Toggle persistent optical R20 configuration.",
    },
    CommandDefinition {
        id: "toggle_persistent_r21",
        command_number: Some(154),
        family: "persistent_sensor_config",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Toggle persistent IMU R21 configuration.",
    },
    CommandDefinition {
        id: "enter_ble_dfu",
        command_number: Some(45),
        family: "firmware_dfu",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Enter BLE DFU mode.",
    },
    CommandDefinition {
        id: "reboot_strap",
        command_number: Some(29),
        family: "reboot_maintenance",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Reboot the strap.",
    },
    CommandDefinition {
        id: "power_cycle_strap",
        command_number: Some(32),
        family: "reboot_maintenance",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Power-cycle the strap.",
    },
    CommandDefinition {
        id: "force_trim",
        command_number: Some(25),
        family: "reboot_maintenance",
        risk_gate: CommandRiskGate::CriticalStateChange,
        description: "Force storage trim.",
    },
];

const TRUSTED_COMMAND_EVIDENCE_SOURCES: &[&str] =
    &["user_owned_official_capture", "passive_official_capture"];
const TRUSTED_COMMAND_EVIDENCE_SOURCE_ALIASES: &[&str] =
    &["official_app_capture", "official_app_to_macos_emulator"];
const TRUSTED_COMMAND_PROVENANCE_CAPTURE_KINDS: &[&str] = &[
    "official_app_to_macos_emulator",
    "passive_ble_observation",
    "user_owned_official_capture",
    "owned_device_passive_capture",
];
const GOOSE_COMMAND_SERVICE_UUID: &str = "fd4b0001-cce1-4033-93ce-002d5875f58a";
const GOOSE_COMMAND_TO_STRAP_UUID: &str = "fd4b0002-cce1-4033-93ce-002d5875f58a";
const GOOSE_COMMAND_FROM_STRAP_UUID: &str = "fd4b0003-cce1-4033-93ce-002d5875f58a";
const MAX_DIRECT_SEND_OVERRIDE_WINDOW_MS: u64 = 30_000;

pub fn load_command_evidence(path: &Path) -> GooseResult<Vec<CommandEvidence>> {
    let raw = fs::read_to_string(path).map_err(|source| GooseError::io(path, source))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(template) = serde_json::from_str::<CommandEvidenceTemplate>(&raw) {
        return Ok(template.evidence);
    }
    if let Ok(evidence) = serde_json::from_str::<Vec<CommandEvidence>>(&raw) {
        return Ok(evidence);
    }
    if let Ok(evidence) = serde_json::from_str::<CommandEvidence>(&raw) {
        return Ok(vec![evidence]);
    }
    load_command_evidence_jsonl(path, &raw)
}

pub fn load_command_local_frame_candidates(
    path: &Path,
) -> GooseResult<Vec<CommandLocalFrameCandidate>> {
    if path.is_dir() {
        return load_command_local_frame_candidates_dir(path);
    }
    load_command_local_frame_candidates_file(path)
}

fn load_command_local_frame_candidates_dir(
    path: &Path,
) -> GooseResult<Vec<CommandLocalFrameCandidate>> {
    let mut files = fs::read_dir(path)
        .map_err(|source| GooseError::io(path, source))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| GooseError::io(path, source))?;
    files.sort();

    let mut candidates = Vec::new();
    for file in files {
        if !file.is_file() || !command_candidate_file_extension(&file) {
            continue;
        }
        candidates.extend(load_command_local_frame_candidates_file(&file)?);
    }
    Ok(candidates)
}

fn command_candidate_file_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "json" | "jsonl"))
        .unwrap_or(false)
}

fn load_command_local_frame_candidates_file(
    path: &Path,
) -> GooseResult<Vec<CommandLocalFrameCandidate>> {
    let raw = fs::read_to_string(path).map_err(|source| GooseError::io(path, source))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(file) = serde_json::from_str::<CommandLocalFrameCandidateFile>(&raw)
        && let Some(candidates) = file.candidates
    {
        return Ok(normalize_command_local_frame_candidates(candidates));
    }
    if let Ok(candidates) = serde_json::from_str::<Vec<CommandLocalFrameCandidate>>(&raw) {
        return Ok(normalize_command_local_frame_candidates(candidates));
    }
    if let Ok(candidate) = serde_json::from_str::<CommandLocalFrameCandidate>(&raw) {
        return Ok(normalize_command_local_frame_candidates(vec![candidate]));
    }
    load_command_local_frame_candidates_jsonl(path, &raw)
}

fn load_command_local_frame_candidates_jsonl(
    path: &Path,
    raw: &str,
) -> GooseResult<Vec<CommandLocalFrameCandidate>> {
    let mut candidates = Vec::new();
    for (line_index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = serde_json::from_str::<CommandLocalFrameCandidate>(line).map_err(|error| {
            GooseError::message(format!(
                "{} line {} is not valid command-local-frame-candidate JSONL: {error}",
                path.display(),
                line_index + 1
            ))
        })?;
        candidates.push(row);
    }
    Ok(normalize_command_local_frame_candidates(candidates))
}

fn normalize_command_local_frame_candidates(
    mut candidates: Vec<CommandLocalFrameCandidate>,
) -> Vec<CommandLocalFrameCandidate> {
    for candidate in &mut candidates {
        if let Some(command) = normalize_command_identifier(&candidate.command).or_else(|| {
            candidate
                .command_id
                .as_deref()
                .and_then(normalize_command_identifier)
        }) {
            candidate.command = command;
        } else {
            candidate.command = candidate.command.trim().to_string();
        }
    }
    candidates
}

fn normalize_command_identifier(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(command_number) = trimmed.parse::<u16>() {
        return COMMAND_DEFINITIONS
            .iter()
            .find(|definition| definition.command_number == Some(command_number))
            .map(|definition| definition.id.to_string());
    }
    let snake = trimmed
        .trim_matches(|character: char| character == '"' || character.is_whitespace())
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");
    COMMAND_DEFINITIONS
        .iter()
        .find(|definition| {
            definition.id == snake
                || definition.id.eq_ignore_ascii_case(trimmed)
                || definition.id.eq_ignore_ascii_case(trimmed)
        })
        .map(|definition| definition.id.to_string())
}

fn load_command_evidence_jsonl(path: &Path, raw: &str) -> GooseResult<Vec<CommandEvidence>> {
    let mut evidence = Vec::new();
    for (line_index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = serde_json::from_str::<CommandEvidence>(line).map_err(|error| {
            GooseError::message(format!(
                "{} line {} is not valid command-evidence JSONL: {error}",
                path.display(),
                line_index + 1
            ))
        })?;
        evidence.push(row);
    }
    Ok(evidence)
}

pub fn command_evidence_from_emulator_log(
    path: &Path,
    options: &CommandEmulatorLogEvidenceOptions,
) -> GooseResult<CommandEmulatorLogEvidenceReport> {
    let raw = fs::read_to_string(path).map_err(|source| GooseError::io(path, source))?;
    command_evidence_from_emulator_log_text(&path.display().to_string(), &raw, options)
}

pub fn command_evidence_from_emulator_log_text(
    source_log: &str,
    raw: &str,
    options: &CommandEmulatorLogEvidenceOptions,
) -> GooseResult<CommandEmulatorLogEvidenceReport> {
    let lines = emulator_log_lines(raw);
    let mut issues = Vec::new();
    let mut pending_writes = Vec::<PendingEmulatorCommandWrite>::new();
    let mut accumulators = BTreeMap::<String, EmulatorCommandEvidenceAccumulator>::new();
    let mut seen_responses = BTreeSet::<String>::new();

    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        let message = emulator_log_message(line);
        if let Some(write) = emulator_command_write_record(line_no, message, options) {
            let parsed = match parse_frame_hex(DeviceType::Goose, &write.frame_hex) {
                Ok(parsed) => parsed,
                Err(error) => {
                    issues.push(format!(
                        "emulator_write_parse_failed:{}:{error}",
                        write.line_no
                    ));
                    continue;
                }
            };
            let Some((command_number, sequence)) = parsed_command_write(&parsed) else {
                issues.push(format!(
                    "emulator_write_not_command_payload:{}:{}",
                    write.line_no,
                    parsed.packet_type_name.as_deref().unwrap_or("unknown")
                ));
                continue;
            };
            let Some(definition) = command_definition_by_number(command_number) else {
                issues.push(format!(
                    "emulator_write_unknown_command:{}:{command_number}",
                    write.line_no
                ));
                continue;
            };
            let accumulator = accumulators
                .entry(definition.id.to_string())
                .or_insert_with(|| {
                    EmulatorCommandEvidenceAccumulator::new(definition, options, &write)
                });
            accumulator.record_write(&write, options);
            pending_writes.push(PendingEmulatorCommandWrite {
                command: definition.id.to_string(),
                command_number,
                sequence,
            });
            continue;
        }

        let Some(response) = emulator_command_response_record(line_no, message) else {
            continue;
        };
        let response_key = normalize_hex(&response.frame_hex);
        if !seen_responses.insert(response_key) {
            continue;
        }
        let parsed = match parse_frame_hex(DeviceType::Goose, &response.frame_hex) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(format!(
                    "emulator_response_parse_failed:{}:{error}",
                    response.line_no
                ));
                continue;
            }
        };
        let Some((response_to_command, origin_sequence, result_code)) =
            parsed_command_response(&parsed)
        else {
            issues.push(format!(
                "emulator_response_not_command_response:{}:{}",
                response.line_no,
                parsed.packet_type_name.as_deref().unwrap_or("unknown")
            ));
            continue;
        };
        let Some(pending_index) = pending_writes.iter().position(|write| {
            write.command_number == response_to_command && write.sequence == origin_sequence
        }) else {
            issues.push(format!(
                "emulator_response_unpaired:{}:command={response_to_command}:sequence={origin_sequence}",
                response.line_no
            ));
            continue;
        };
        let pending = pending_writes.remove(pending_index);
        let Some(accumulator) = accumulators.get_mut(&pending.command) else {
            issues.push(format!(
                "emulator_response_missing_accumulator:{}:{}",
                response.line_no, pending.command
            ));
            continue;
        };
        accumulator.record_response(&response.frame_hex, result_code);
    }

    let pending_write_count = pending_writes.len();
    let evidence = accumulators
        .into_values()
        .map(|accumulator| accumulator.into_evidence(options, source_log))
        .collect::<Vec<_>>();
    let input_valid = !source_log.trim().is_empty() && !raw.trim().is_empty();
    let log_lines_ready = !lines.is_empty();
    let official_writes_parsed = !evidence.is_empty()
        && evidence
            .iter()
            .all(|row| row.official_capture_count > 0 && row.official_frame_hex.is_some());
    let responses_paired = pending_write_count == 0
        && !evidence.is_empty()
        && evidence.iter().all(|row| {
            row.official_response_frame_hex.is_some()
                || row.official_failure_response_frame_hex.is_some()
        });
    let trusted_capture_context = !evidence.is_empty()
        && evidence.iter().all(|row| {
            let provenance = row.provenance_json.as_deref().and_then(|raw| {
                match serde_json::from_str::<serde_json::Value>(raw) {
                    Ok(serde_json::Value::Object(object)) => Some(object),
                    _ => None,
                }
            });
            trusted_command_evidence_source(
                row.evidence_source.as_deref().unwrap_or(""),
                provenance.as_ref(),
            )
        });
    let official_capture_ready = input_valid
        && log_lines_ready
        && official_writes_parsed
        && responses_paired
        && trusted_capture_context
        && issues.is_empty();
    let local_frame_match_ready = !evidence.is_empty() && evidence.iter().all(frames_match);
    let direct_validation_ready = official_capture_ready && local_frame_match_ready;
    let next_actions = emulator_log_next_actions(
        input_valid,
        log_lines_ready,
        official_writes_parsed,
        responses_paired,
        trusted_capture_context,
        official_capture_ready,
        local_frame_match_ready,
        &issues,
    );

    Ok(CommandEmulatorLogEvidenceReport {
        schema: "goose.command-evidence.v1".to_string(),
        generated_by: "goose-command-validator emulator-log".to_string(),
        pass: official_capture_ready,
        input_valid,
        log_lines_ready,
        official_writes_parsed,
        responses_paired,
        trusted_capture_context,
        official_capture_ready,
        local_frame_match_ready,
        direct_validation_ready,
        source_capture: source_log.to_string(),
        source_log: source_log.to_string(),
        device_type: "GOOSE".to_string(),
        evidence_source: "user_owned_official_capture".to_string(),
        capture_kind: options.capture_kind.clone(),
        owner: options.owner.clone(),
        line_count: lines.len(),
        transaction_count: evidence
            .iter()
            .map(|row| row.official_capture_count as usize)
            .sum(),
        evidence_count: evidence.len(),
        evidence,
        issues,
        notes: vec![
            "Rows were parsed from a local macOS WHOOP BLE peripheral emulator log.".to_string(),
            "The official app produced these writes; Goose did not send BLE commands during conversion.".to_string(),
            "local_frame_hex is populated only when --emulator-mirror-local-frame is used after a separate byte-match/replay comparison.".to_string(),
        ],
        next_actions,
    })
}

fn emulator_log_next_actions(
    input_valid: bool,
    log_lines_ready: bool,
    official_writes_parsed: bool,
    responses_paired: bool,
    trusted_capture_context: bool,
    official_capture_ready: bool,
    local_frame_match_ready: bool,
    issues: &[String],
) -> Vec<CommandEmulatorLogNextAction> {
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    if !input_valid {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "command_emulator_log_input_required",
            "Pick a non-empty macOS BLE emulator log captured while the official WHOOP app performed the command.",
        );
    }
    if input_valid && !log_lines_ready {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "command_emulator_log_lines_required",
            "Capture emulator log lines that include official app command writes and strap responses.",
        );
    }
    if log_lines_ready && !official_writes_parsed {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "official_command_writes_required",
            "Capture official app writes to the command characteristic with full frame bytes.",
        );
    }
    if official_writes_parsed && !responses_paired {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "official_command_responses_required",
            "Capture and pair strap command responses for each official app write before promotion.",
        );
    }
    if official_writes_parsed && !trusted_capture_context {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "official_capture_provenance_trusted",
            "Use user-owned WHOOP official-app emulator provenance with owner=user, capture_app=whoop_official, and an accepted capture_kind.",
        );
    }
    for issue in issues {
        let (requirement, action) = emulator_log_issue_next_action(issue);
        push_emulator_log_next_action(&mut actions, &mut seen, requirement, &action);
    }
    if official_capture_ready && !local_frame_match_ready {
        push_emulator_log_next_action(
            &mut actions,
            &mut seen,
            "local_frame_matches_official_frame",
            "Import Goose dry-run local frame candidates and run local-frame promotion before validating direct sends.",
        );
    }
    actions
}

fn push_emulator_log_next_action(
    actions: &mut Vec<CommandEmulatorLogNextAction>,
    seen: &mut BTreeSet<String>,
    requirement: &str,
    action: &str,
) {
    let key = format!("{requirement}|{action}");
    if !seen.insert(key) {
        return;
    }
    actions.push(CommandEmulatorLogNextAction {
        requirement: requirement.to_string(),
        action: action.to_string(),
    });
}

fn emulator_log_issue_next_action(issue: &str) -> (&'static str, String) {
    if issue.starts_with("emulator_write_parse_failed:") {
        (
            "official_write_frame_parseable",
            "Recapture or repair the official app write frame so Goose can parse it.".to_string(),
        )
    } else if issue.starts_with("emulator_write_not_command_payload:") {
        (
            "official_command_write_frame_required",
            "Filter the emulator log to official command writes, not notifications or data packets."
                .to_string(),
        )
    } else if issue.starts_with("emulator_write_unknown_command:") {
        (
            "command_definition_required",
            "Add or correct the APK/firmware command definition for this captured command number."
                .to_string(),
        )
    } else if issue.starts_with("emulator_response_parse_failed:") {
        (
            "official_response_frame_parseable",
            "Recapture or repair the strap response frame so Goose can parse it.".to_string(),
        )
    } else if issue.starts_with("emulator_response_not_command_response:") {
        (
            "official_command_response_frame_required",
            "Capture command-response frames for the official app write, not unrelated notifications."
                .to_string(),
        )
    } else if issue.starts_with("emulator_response_unpaired:")
        || issue.starts_with("emulator_response_missing_accumulator:")
    {
        (
            "official_response_pairs_with_write",
            "Capture the write and response in the same emulator session so sequence and command number pair correctly."
                .to_string(),
        )
    } else {
        (
            "command_emulator_log_issue",
            format!("Resolve emulator log issue: {issue}"),
        )
    }
}

pub fn command_evidence_template() -> CommandEvidenceTemplate {
    CommandEvidenceTemplate {
        schema: "goose.command-evidence.v1".to_string(),
        evidence: COMMAND_DEFINITIONS
            .iter()
            .map(|definition| CommandEvidence {
                command: definition.id.to_string(),
                ..CommandEvidence::default()
            })
            .collect(),
    }
}

pub fn command_evidence_with_local_frame_matches(
    evidence: &[CommandEvidence],
    candidates: &[CommandLocalFrameCandidate],
) -> CommandLocalFrameMatchReport {
    let mut candidates_by_command = BTreeMap::<String, Vec<&CommandLocalFrameCandidate>>::new();
    for candidate in candidates {
        let command = normalize_command_identifier(&candidate.command)
            .or_else(|| {
                candidate
                    .command_id
                    .as_deref()
                    .and_then(normalize_command_identifier)
            })
            .unwrap_or_else(|| candidate.command.trim().to_string());
        candidates_by_command
            .entry(command)
            .or_default()
            .push(candidate);
    }

    let mut promoted = Vec::new();
    let mut comparisons = Vec::new();
    let mut issues = Vec::new();

    for row in evidence {
        let Some(definition) = COMMAND_DEFINITIONS
            .iter()
            .find(|definition| definition.id == row.command)
        else {
            issues.push(format!("unknown command evidence: {}", row.command));
            promoted.push(row.clone());
            comparisons.push(CommandLocalFrameComparison {
                command: row.command.clone(),
                matched: false,
                reason: "unknown_command".to_string(),
                official_frame_hex: row.official_frame_hex.as_deref().map(normalize_hex),
                local_frame_hex: row.local_frame_hex.as_deref().map(normalize_hex),
                source: None,
                warnings: vec!["command is not in Goose command definitions".to_string()],
            });
            continue;
        };

        if frames_match(row) {
            promoted.push(row.clone());
            comparisons.push(CommandLocalFrameComparison {
                command: row.command.clone(),
                matched: true,
                reason: "already_matched".to_string(),
                official_frame_hex: row.official_frame_hex.as_deref().map(normalize_hex),
                local_frame_hex: row.local_frame_hex.as_deref().map(normalize_hex),
                source: None,
                warnings: Vec::new(),
            });
            continue;
        }

        let Some(command_candidates) = candidates_by_command.get(row.command.as_str()) else {
            promoted.push(row.clone());
            comparisons.push(CommandLocalFrameComparison {
                command: row.command.clone(),
                matched: false,
                reason: "local_candidate_missing".to_string(),
                official_frame_hex: row.official_frame_hex.as_deref().map(normalize_hex),
                local_frame_hex: None,
                source: None,
                warnings: vec![
                    "import a Goose dry-run/local frame candidate for this command".to_string(),
                ],
            });
            continue;
        };

        let mut selected: Option<(CommandEvidence, CommandLocalFrameComparison)> = None;
        let mut last_comparison = None;
        for candidate in command_candidates {
            match promote_local_frame_candidate(definition, row, candidate) {
                Ok((promoted_row, matched_comparison)) => {
                    selected = Some((promoted_row, matched_comparison));
                    break;
                }
                Err(blocked_comparison) => {
                    last_comparison = Some(blocked_comparison);
                }
            }
        }

        if let Some((promoted_row, comparison)) = selected {
            promoted.push(promoted_row);
            comparisons.push(comparison);
        } else {
            promoted.push(row.clone());
            let comparison = last_comparison.unwrap_or_else(|| CommandLocalFrameComparison {
                command: row.command.clone(),
                matched: false,
                reason: "local_candidate_missing".to_string(),
                official_frame_hex: row.official_frame_hex.as_deref().map(normalize_hex),
                local_frame_hex: None,
                source: None,
                warnings: Vec::new(),
            });
            issues.push(format!("{}:{}", comparison.command, comparison.reason));
            comparisons.push(comparison);
        }
    }

    let matched_count = comparisons
        .iter()
        .filter(|comparison| comparison.matched)
        .count();
    let blocked_count = comparisons.len().saturating_sub(matched_count);
    let input_valid = !evidence.is_empty() && !candidates.is_empty();
    let comparisons_ready = comparisons.len() == evidence.len() && promoted.len() == evidence.len();
    let all_frames_matched = input_valid && matched_count == evidence.len() && blocked_count == 0;
    let promotion_ready =
        input_valid && comparisons_ready && all_frames_matched && issues.is_empty();
    let next_actions = local_frame_match_next_actions(evidence, candidates, &comparisons);

    CommandLocalFrameMatchReport {
        schema: "goose.command-local-frame-match-report.v1".to_string(),
        generated_by: "goose-command-local-frame-match".to_string(),
        pass: promotion_ready,
        input_valid,
        comparisons_ready,
        all_frames_matched,
        promotion_ready,
        evidence_count: promoted.len(),
        candidate_count: candidates.len(),
        matched_count,
        blocked_count,
        evidence: promoted,
        comparisons,
        issues,
        next_actions,
    }
}

fn local_frame_match_next_actions(
    evidence: &[CommandEvidence],
    candidates: &[CommandLocalFrameCandidate],
    comparisons: &[CommandLocalFrameComparison],
) -> Vec<CommandLocalFrameNextAction> {
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    if evidence.is_empty() {
        push_local_frame_next_action(
            &mut actions,
            &mut seen,
            "",
            "official_command_evidence_required",
            "Import official-app command evidence from the real strap or macOS BLE emulator before matching Goose dry-run bytes.",
        );
    }
    if candidates.is_empty() {
        push_local_frame_next_action(
            &mut actions,
            &mut seen,
            "",
            "local_frame_candidates_required",
            "Import Goose dry-run local frame candidates from the APK/firmware-derived builder or whoop-rev command builder.",
        );
    }
    for comparison in comparisons {
        if comparison.matched {
            continue;
        }
        push_local_frame_next_action(
            &mut actions,
            &mut seen,
            &comparison.command,
            &comparison.reason,
            &local_frame_match_action_for_reason(&comparison.command, &comparison.reason),
        );
    }
    actions
}

fn push_local_frame_next_action(
    actions: &mut Vec<CommandLocalFrameNextAction>,
    seen: &mut BTreeSet<String>,
    command: &str,
    requirement: &str,
    action: &str,
) {
    let key = format!("{command}|{requirement}|{action}");
    if !seen.insert(key) {
        return;
    }
    actions.push(CommandLocalFrameNextAction {
        command: command.to_string(),
        requirement: requirement.to_string(),
        action: action.to_string(),
    });
}

fn local_frame_match_action_for_reason(command: &str, reason: &str) -> String {
    match reason {
        "unknown_command" => {
            "Map this evidence row to a known Goose command definition before comparing bytes."
                .to_string()
        }
        "local_candidate_missing" | "local_frame_missing" => format!(
            "Build or import a Goose dry-run frame candidate for {command} before promotion."
        ),
        "official_frame_missing" => format!(
            "Import the official app write frame for {command} before comparing local bytes."
        ),
        "command_number_missing" => format!(
            "Add the static APK/firmware command number for {command} before local byte promotion."
        ),
        "official_frame_parse_failed" | "official_frame_crc_invalid" => format!(
            "Recapture or repair the official {command} frame so Goose can parse it with valid CRCs."
        ),
        "official_frame_not_command_payload" => format!(
            "Use the official command write frame for {command}, not a notification or data packet."
        ),
        "official_frame_command_number_mismatch" => {
            format!("Re-check the static command map; the official frame must parse as {command}.")
        }
        "local_frame_parse_failed" | "local_frame_crc_invalid" => format!(
            "Regenerate the Goose dry-run frame for {command} so it parses with valid CRCs."
        ),
        "local_frame_not_command_payload" => {
            format!("Regenerate the local candidate for {command} as a command write frame.")
        }
        "local_frame_command_number_mismatch" => format!(
            "Fix the Goose dry-run builder so the local frame parses as the same command number as {command}."
        ),
        "frame_bytes_differ" => format!(
            "Replay or adjust the Goose dry-run builder until the local {command} bytes exactly match the official frame."
        ),
        "local_service_uuid_mismatch" => format!(
            "Set the local dry-run service UUID for {command} to the official captured service UUID."
        ),
        "local_characteristic_uuid_mismatch" => format!(
            "Set the local dry-run characteristic UUID for {command} to the official captured characteristic UUID."
        ),
        "local_write_type_mismatch" | "local_write_type_invalid" => format!(
            "Set the local dry-run write type for {command} to the official captured write type."
        ),
        _ => format!("Resolve local-frame comparison blocker {reason} for {command}."),
    }
}

pub fn validate_commands(evidence: &[CommandEvidence]) -> CommandValidationReport {
    let evidence_by_command: BTreeMap<&str, &CommandEvidence> = evidence
        .iter()
        .map(|record| (record.command.as_str(), record))
        .collect();
    let mut issues = Vec::new();
    let evidence_source_summary = command_evidence_source_summary(evidence);

    for record in evidence {
        if !COMMAND_DEFINITIONS
            .iter()
            .any(|definition| definition.id == record.command)
        {
            issues.push(format!("unknown command evidence: {}", record.command));
        }
    }

    let commands: Vec<CommandValidationResult> = COMMAND_DEFINITIONS
        .iter()
        .map(|definition| {
            let evidence = evidence_by_command.get(definition.id).copied();
            validate_command(definition, evidence)
        })
        .collect();

    let direct_send_ready_count = commands
        .iter()
        .filter(|command| command.direct_send_ready)
        .count();
    let blocked_count = commands.len() - direct_send_ready_count;
    let evidence_valid = issues.is_empty();
    let all_direct_sends_ready = blocked_count == 0;

    CommandValidationReport {
        schema: "goose.command-validation-report.v1".to_string(),
        generated_by: "goose-command-validator".to_string(),
        pass: evidence_valid && all_direct_sends_ready,
        evidence_valid,
        all_direct_sends_ready,
        direct_send_ready_count,
        blocked_count,
        evidence_source_summary,
        commands,
        issues,
    }
}

pub fn command_result_from_report_json(report_json: &str) -> GooseResult<CommandValidationResult> {
    serde_json::from_str::<CommandValidationResult>(report_json)
        .map_err(|error| GooseError::message(format!("invalid command validation JSON: {error}")))
}

pub fn direct_send_gate_from_result(
    command: &str,
    result: Option<&CommandValidationResult>,
) -> CommandDirectSendGate {
    match result {
        Some(result) => CommandDirectSendGate {
            schema: "goose.command-direct-send-gate.v1".to_string(),
            command: result.command.clone(),
            command_number: result.command_number,
            family: Some(result.family.clone()),
            risk_gate: Some(result.risk_gate),
            direct_send_allowed: result.direct_send_ready,
            missing_requirements: result.missing_requirements.clone(),
            warnings: result.warnings.clone(),
            next_capture_actions: result.next_capture_actions.clone(),
            issues: Vec::new(),
            validated_local_frame_hex: result.validated_local_frame_hex.clone(),
            validated_service_uuid: result.validated_service_uuid.clone(),
            validated_characteristic_uuid: result.validated_characteristic_uuid.clone(),
            validated_write_type: result.validated_write_type.clone(),
            validated_evidence_source: result.validated_evidence_source.clone(),
            validated_capture_kind: result.validated_capture_kind.clone(),
            validated_owner: result.validated_owner.clone(),
            validated_provenance_json: result.validated_provenance_json.clone(),
            validated_triggering_ui_action: result.validated_triggering_ui_action.clone(),
        },
        None => CommandDirectSendGate {
            schema: "goose.command-direct-send-gate.v1".to_string(),
            command: command.to_string(),
            command_number: None,
            family: None,
            risk_gate: None,
            direct_send_allowed: false,
            missing_requirements: vec!["command_validation_record".to_string()],
            warnings: Vec::new(),
            next_capture_actions: vec![CommandNextCaptureAction {
                requirement: "command_validation_record".to_string(),
                action: "Import or validate official command evidence so Goose can persist a validation record for this command.".to_string(),
            }],
            issues: vec!["command validation record not found".to_string()],
            validated_local_frame_hex: None,
            validated_service_uuid: None,
            validated_characteristic_uuid: None,
            validated_write_type: None,
            validated_evidence_source: None,
            validated_capture_kind: None,
            validated_owner: None,
            validated_provenance_json: None,
            validated_triggering_ui_action: None,
        },
    }
}

pub fn command_capture_plan_from_results(
    results: &[CommandValidationResult],
    requested_commands: &[String],
) -> CommandCapturePlanReport {
    let mut results_by_command = BTreeMap::new();
    let mut issues = Vec::new();
    for result in results {
        results_by_command.insert(result.command.clone(), result.clone());
    }

    let definitions = command_capture_plan_definitions(requested_commands, &mut issues);
    let mut gates = BTreeMap::new();
    let mut family_accumulators = BTreeMap::<String, CommandCapturePlanFamilyAccumulator>::new();
    let mut actions = Vec::new();
    let mut seen_actions = BTreeSet::new();
    let mut ready_count = 0usize;
    let mut locked_count = 0usize;
    let mut critical_locked_count = 0usize;

    for definition in definitions {
        let result = results_by_command.get(definition.id);
        let gate = direct_send_gate_from_result(definition.id, result);
        let family = definition.family.to_string();
        let accumulator = family_accumulators.entry(family.clone()).or_default();
        accumulator.command_count += 1;
        if gate.direct_send_allowed {
            ready_count += 1;
            accumulator.ready_count += 1;
        } else {
            locked_count += 1;
            accumulator.locked_count += 1;
            if definition.risk_gate == CommandRiskGate::CriticalStateChange {
                critical_locked_count += 1;
                accumulator.critical_locked_count += 1;
            }
            push_command_capture_plan_actions(
                &mut actions,
                &mut seen_actions,
                definition,
                &gate.next_capture_actions,
                &gate.missing_requirements,
            );
        }
        gates.insert(definition.id.to_string(), gate);
    }

    for command in results_by_command.keys() {
        if !COMMAND_DEFINITIONS
            .iter()
            .any(|definition| definition.id == command)
        {
            issues.push(format!("unknown_command_validation_record:{command}"));
        }
    }

    let family_summaries = family_accumulators
        .into_iter()
        .map(|(family, accumulator)| CommandCapturePlanFamilySummary {
            family,
            command_count: accumulator.command_count,
            ready_count: accumulator.ready_count,
            locked_count: accumulator.locked_count,
            critical_locked_count: accumulator.critical_locked_count,
        })
        .collect::<Vec<_>>();
    let requested_commands_valid = !issues
        .iter()
        .any(|issue| issue.starts_with("unknown_requested_command:"));
    let validation_records_valid = !issues
        .iter()
        .any(|issue| issue.starts_with("unknown_command_validation_record:"));
    let all_selected_gates_ready = locked_count == 0;
    let critical_gates_ready = critical_locked_count == 0;
    let capture_actions_ready = actions.is_empty();
    let next_command_focus = next_command_focus_from_actions(&actions);
    let pass = requested_commands_valid
        && validation_records_valid
        && all_selected_gates_ready
        && critical_gates_ready
        && capture_actions_ready
        && issues.is_empty();

    CommandCapturePlanReport {
        schema: COMMAND_CAPTURE_PLAN_REPORT_SCHEMA.to_string(),
        generated_by: "goose-command-capture-plan".to_string(),
        pass,
        requested_commands_valid,
        validation_records_valid,
        all_selected_gates_ready,
        critical_gates_ready,
        capture_actions_ready,
        command_count: ready_count + locked_count,
        ready_count,
        locked_count,
        critical_locked_count,
        action_count: actions.len(),
        family_summaries,
        next_command_focus,
        actions,
        gates,
        issues,
    }
}

fn next_command_focus_from_actions(
    actions: &[CommandCapturePlanAction],
) -> Option<CommandCapturePlanAction> {
    for risk_gate in [
        CommandRiskGate::CriticalStateChange,
        CommandRiskGate::UserVisibleStateChange,
        CommandRiskGate::ReadOnly,
    ] {
        if let Some(action) = actions
            .iter()
            .find(|action| action.risk_gate == risk_gate)
            .cloned()
        {
            return Some(action);
        }
    }
    None
}

fn command_capture_plan_definitions(
    requested_commands: &[String],
    issues: &mut Vec<String>,
) -> Vec<&'static CommandDefinition> {
    if requested_commands.is_empty() {
        return COMMAND_DEFINITIONS.iter().collect();
    }

    let mut definitions = Vec::new();
    let mut seen = BTreeSet::new();
    for command in requested_commands {
        let command = command.trim();
        if command.is_empty() {
            continue;
        }
        if !seen.insert(command.to_string()) {
            continue;
        }
        match COMMAND_DEFINITIONS
            .iter()
            .find(|definition| definition.id == command)
        {
            Some(definition) => definitions.push(definition),
            None => issues.push(format!("unknown_requested_command:{command}")),
        }
    }
    definitions
}

fn push_command_capture_plan_actions(
    actions: &mut Vec<CommandCapturePlanAction>,
    seen: &mut BTreeSet<String>,
    definition: &CommandDefinition,
    next_capture_actions: &[CommandNextCaptureAction],
    missing_requirements: &[String],
) {
    if next_capture_actions.is_empty() {
        for requirement in missing_requirements {
            push_command_capture_plan_action(
                actions,
                seen,
                definition,
                requirement,
                &format!(
                    "Resolve validation requirement {requirement} for {}.",
                    definition.id
                ),
            );
        }
        return;
    }

    for action in next_capture_actions {
        push_command_capture_plan_action(
            actions,
            seen,
            definition,
            &action.requirement,
            &action.action,
        );
    }
}

fn push_command_capture_plan_action(
    actions: &mut Vec<CommandCapturePlanAction>,
    seen: &mut BTreeSet<String>,
    definition: &CommandDefinition,
    requirement: &str,
    action: &str,
) {
    let key = format!("{}|{requirement}|{action}", definition.id);
    if !seen.insert(key) {
        return;
    }
    let summary = if requirement.is_empty() {
        action.to_string()
    } else {
        format!("{requirement}: {action}")
    };
    actions.push(CommandCapturePlanAction {
        command: definition.id.to_string(),
        family: definition.family.to_string(),
        risk_gate: definition.risk_gate,
        requirement: requirement.to_string(),
        action: action.to_string(),
        summary,
    });
}

pub fn direct_send_preflight_from_gate(
    input: &CommandDirectSendPreflightInput,
    gate: CommandDirectSendGate,
) -> CommandDirectSendPreflight {
    let mut missing = Vec::new();
    let mut warnings = gate.warnings.clone();

    if gate.command != input.command {
        missing.push("command_gate_matches_request".to_string());
    }
    if !gate.direct_send_allowed {
        missing.push("direct_send_gate_ready".to_string());
        missing.extend(gate.missing_requirements.clone());
    }
    if !input.visible_user_intent {
        missing.push("visible_user_intent".to_string());
    }
    if !input.dry_run_bytes_shown {
        missing.push("dry_run_bytes_shown".to_string());
    }
    validate_preflight_dry_run_frame(input, &gate, &mut missing);
    validate_preflight_endpoint(input, &gate, &mut missing, &mut warnings);
    if !input.session_log_ready {
        missing.push("session_log_entry".to_string());
    }
    if input.connection_state.as_deref().map(str::trim) != Some("connected") {
        missing.push("connected_device".to_string());
    }
    if input
        .active_device_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        missing.push("active_device_id".to_string());
    }
    validate_critical_runtime_preflight(input, &gate, &mut missing);

    let override_expires_in_ms = validate_short_lived_override(input, &mut missing, &mut warnings);

    missing.sort();
    missing.dedup();
    warnings.sort();
    warnings.dedup();

    CommandDirectSendPreflight {
        schema: "goose.command-direct-send-preflight.v1".to_string(),
        command: input.command.clone(),
        direct_send_allowed: missing.is_empty(),
        gate,
        missing_requirements: missing,
        warnings,
        override_expires_in_ms,
        dry_run_frame_hex: input
            .dry_run_frame_hex
            .as_deref()
            .map(normalize_hex)
            .filter(|value| !value.is_empty()),
        dry_run_service_uuid: input
            .dry_run_service_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value)
            .filter(|value| !value.is_empty()),
        dry_run_characteristic_uuid: input
            .dry_run_characteristic_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value)
            .filter(|value| !value.is_empty()),
        dry_run_write_type: input
            .dry_run_write_type
            .as_deref()
            .and_then(normalize_write_type),
    }
}

fn validate_critical_runtime_preflight(
    input: &CommandDirectSendPreflightInput,
    gate: &CommandDirectSendGate,
    missing: &mut Vec<String>,
) {
    if gate.risk_gate != Some(CommandRiskGate::CriticalStateChange) {
        return;
    }
    if !input.critical_visible_confirmation {
        missing.push("critical_visible_confirmation".to_string());
    }
    if !input.critical_explicit_approval {
        missing.push("critical_explicit_approval".to_string());
    }
    if !input.critical_rollback_or_restore_acknowledged {
        missing.push("critical_rollback_or_restore_acknowledged".to_string());
    }
}

fn validate_preflight_dry_run_frame(
    input: &CommandDirectSendPreflightInput,
    gate: &CommandDirectSendGate,
    missing: &mut Vec<String>,
) {
    let Some(shown_frame) = input
        .dry_run_frame_hex
        .as_deref()
        .map(normalize_hex)
        .filter(|value| !value.is_empty())
    else {
        missing.push("dry_run_frame_hex".to_string());
        return;
    };
    let Some(validated_frame) = gate
        .validated_local_frame_hex
        .as_deref()
        .map(normalize_hex)
        .filter(|value| !value.is_empty())
    else {
        missing.push("validated_local_frame_hex".to_string());
        return;
    };
    if shown_frame != validated_frame {
        missing.push("dry_run_frame_matches_validated_local_frame".to_string());
    }
}

fn validate_preflight_endpoint(
    input: &CommandDirectSendPreflightInput,
    gate: &CommandDirectSendGate,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    compare_preflight_endpoint_field(
        "dry_run_service_uuid",
        "validated_service_uuid",
        "dry_run_service_uuid_matches_validated_endpoint",
        input
            .dry_run_service_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value),
        gate.validated_service_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value),
        EndpointFieldKind::Identifier,
        missing,
        warnings,
    );
    compare_preflight_endpoint_field(
        "dry_run_characteristic_uuid",
        "validated_characteristic_uuid",
        "dry_run_characteristic_uuid_matches_validated_endpoint",
        input
            .dry_run_characteristic_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value),
        gate.validated_characteristic_uuid
            .as_deref()
            .map(normalize_ble_endpoint_value),
        EndpointFieldKind::Identifier,
        missing,
        warnings,
    );
    compare_preflight_endpoint_field(
        "dry_run_write_type",
        "validated_write_type",
        "dry_run_write_type_matches_validated_endpoint",
        input
            .dry_run_write_type
            .as_deref()
            .and_then(normalize_write_type),
        gate.validated_write_type
            .as_deref()
            .and_then(normalize_write_type),
        EndpointFieldKind::WriteType,
        missing,
        warnings,
    );
}

#[derive(Debug, Clone, Copy)]
enum EndpointFieldKind {
    Identifier,
    WriteType,
}

fn compare_preflight_endpoint_field(
    dry_run_requirement: &str,
    validated_requirement: &str,
    match_requirement: &str,
    dry_run_value: Option<String>,
    validated_value: Option<String>,
    field_kind: EndpointFieldKind,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let Some(dry_run_value) = dry_run_value.filter(|value| !value.is_empty()) else {
        missing.push(dry_run_requirement.to_string());
        return;
    };
    let Some(validated_value) = validated_value.filter(|value| !value.is_empty()) else {
        missing.push(validated_requirement.to_string());
        return;
    };

    let matches = match field_kind {
        EndpointFieldKind::Identifier => {
            normalize_ble_identifier_for_compare(&dry_run_value)
                == normalize_ble_identifier_for_compare(&validated_value)
        }
        EndpointFieldKind::WriteType => dry_run_value == validated_value,
    };
    if !matches {
        missing.push(match_requirement.to_string());
        warnings.push(format!(
            "{dry_run_requirement} {dry_run_value} did not match {validated_requirement} {validated_value}"
        ));
    }
}

fn validate_short_lived_override(
    input: &CommandDirectSendPreflightInput,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<u64> {
    let Some(expires_at) = input.override_expires_at_unix_ms else {
        missing.push("short_lived_user_override".to_string());
        return None;
    };
    if expires_at <= input.now_unix_ms {
        missing.push("short_lived_user_override_fresh".to_string());
        return None;
    }
    let remaining = expires_at - input.now_unix_ms;
    if remaining > MAX_DIRECT_SEND_OVERRIDE_WINDOW_MS {
        missing.push("short_lived_user_override_short_lived".to_string());
        warnings.push(format!(
            "override window {remaining}ms exceeds {MAX_DIRECT_SEND_OVERRIDE_WINDOW_MS}ms"
        ));
    }
    Some(remaining)
}

#[derive(Debug, Clone)]
struct PendingEmulatorCommandWrite {
    command: String,
    command_number: u8,
    sequence: u8,
}

#[derive(Debug, Clone)]
struct EmulatorCommandWriteRecord {
    line_no: usize,
    frame_hex: String,
    service_uuid: String,
    characteristic_uuid: String,
    write_type: String,
}

#[derive(Debug, Clone)]
struct EmulatorCommandResponseRecord {
    line_no: usize,
    frame_hex: String,
}

#[derive(Debug, Clone)]
struct EmulatorCommandEvidenceAccumulator {
    evidence: CommandEvidence,
    command_number: u8,
    command_name: String,
    transaction_lines: Vec<usize>,
    matched_count: usize,
    success_response_count: usize,
    failure_response_count: usize,
}

impl EmulatorCommandEvidenceAccumulator {
    fn new(
        definition: &CommandDefinition,
        options: &CommandEmulatorLogEvidenceOptions,
        write: &EmulatorCommandWriteRecord,
    ) -> Self {
        let triggering_ui_action = triggering_ui_action_for_command(definition, options);
        Self {
            evidence: CommandEvidence {
                command: definition.id.to_string(),
                official_capture_count: 0,
                evidence_source: Some("user_owned_official_capture".to_string()),
                provenance_json: None,
                official_frame_hex: Some(normalize_hex(&write.frame_hex)),
                local_frame_hex: options
                    .mirror_local_frame
                    .then(|| normalize_hex(&write.frame_hex)),
                official_service_uuid: Some(write.service_uuid.clone()),
                local_service_uuid: Some(write.service_uuid.clone()),
                official_characteristic_uuid: Some(write.characteristic_uuid.clone()),
                local_characteristic_uuid: Some(write.characteristic_uuid.clone()),
                official_write_type: Some(write.write_type.clone()),
                local_write_type: Some(write.write_type.clone()),
                official_response_frame_hex: None,
                official_failure_response_frame_hex: None,
                triggering_ui_action: Some(triggering_ui_action),
                response_parser: false,
                failure_parser: false,
                visible_user_intent: options.visible_user_intent,
                visible_confirmation: options.visible_confirmation,
                logging: true,
                timeout_behavior: false,
                rollback_plan: options.rollback_plan,
                explicit_approval: options.explicit_approval,
            },
            command_number: definition.command_number.unwrap_or_default() as u8,
            command_name: command_name_for_provenance(definition),
            transaction_lines: Vec::new(),
            matched_count: 0,
            success_response_count: 0,
            failure_response_count: 0,
        }
    }

    fn record_write(
        &mut self,
        write: &EmulatorCommandWriteRecord,
        options: &CommandEmulatorLogEvidenceOptions,
    ) {
        self.evidence.official_capture_count += 1;
        self.transaction_lines.push(write.line_no);
        if self.evidence.official_frame_hex.is_none() {
            self.evidence.official_frame_hex = Some(normalize_hex(&write.frame_hex));
        }
        if options.mirror_local_frame && self.evidence.local_frame_hex.is_none() {
            self.evidence.local_frame_hex = Some(normalize_hex(&write.frame_hex));
        }
    }

    fn record_response(&mut self, response_hex: &str, result_code: u8) {
        self.matched_count += 1;
        self.evidence.timeout_behavior = true;
        if result_code == 0 || result_code == 1 {
            self.success_response_count += 1;
            self.evidence.response_parser = true;
            if self.evidence.official_response_frame_hex.is_none() {
                self.evidence.official_response_frame_hex = Some(normalize_hex(response_hex));
            }
        } else {
            self.failure_response_count += 1;
            self.evidence.failure_parser = true;
            if self.evidence.official_failure_response_frame_hex.is_none() {
                self.evidence.official_failure_response_frame_hex =
                    Some(normalize_hex(response_hex));
            }
        }
    }

    fn into_evidence(
        mut self,
        options: &CommandEmulatorLogEvidenceOptions,
        source_log: &str,
    ) -> CommandEvidence {
        let mut transaction_statuses = serde_json::Map::new();
        transaction_statuses.insert("matched".to_string(), serde_json::json!(self.matched_count));
        if self.success_response_count > 0 {
            transaction_statuses.insert(
                "success_response".to_string(),
                serde_json::json!(self.success_response_count),
            );
        }
        if self.failure_response_count > 0 {
            transaction_statuses.insert(
                "failure_response".to_string(),
                serde_json::json!(self.failure_response_count),
            );
        }
        let mut provenance = serde_json::json!({
            "capture_app": options.capture_app.clone(),
            "capture_kind": options.capture_kind.clone(),
            "owner": options.owner.clone(),
            "source_capture": source_log,
            "device_type": "GOOSE",
            "command_name": self.command_name,
            "command_number": self.command_number,
            "transaction_lines": self.transaction_lines,
            "transaction_statuses": transaction_statuses,
        });
        if let Some(triggering_ui_action) = self.evidence.triggering_ui_action.as_deref() {
            provenance["triggering_ui_action"] = serde_json::json!(triggering_ui_action);
        }
        self.evidence.provenance_json = Some(provenance.to_string());
        self.evidence
    }
}

fn emulator_log_lines(raw: &str) -> Vec<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        let mut lines = Vec::new();
        collect_emulator_log_lines(&value, &mut lines);
        if !lines.is_empty() {
            return lines;
        }
    }
    raw.lines().map(str::to_string).collect()
}

fn collect_emulator_log_lines(value: &serde_json::Value, lines: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            for line in text.lines() {
                if line.contains("command_to_strap") || line.contains("command_from_strap") {
                    lines.push(line.to_string());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_emulator_log_lines(item, lines);
            }
        }
        serde_json::Value::Object(object) => {
            if is_structured_emulator_command_row(value) {
                lines.push(value.to_string());
                return;
            }
            for item in object.values() {
                collect_emulator_log_lines(item, lines);
            }
        }
        _ => {}
    }
}

fn emulator_log_message(line: &str) -> &str {
    let trimmed = line.trim();
    let scoped = trimmed
        .find('[')
        .map(|index| &trimmed[index..])
        .unwrap_or(trimmed);
    if let Some(close_index) = scoped.find("] ") {
        return &scoped[close_index + 2..];
    }
    scoped
}

fn emulator_command_write_frame_hex(message: &str) -> Option<String> {
    let marker = " to command_to_strap:";
    if !message.starts_with("Write ") || !message.contains(marker) {
        return None;
    }
    let (_, value) = message.split_once(marker)?;
    normalize_emulator_hex(value)
}

fn emulator_command_write_record(
    line_no: usize,
    message: &str,
    options: &CommandEmulatorLogEvidenceOptions,
) -> Option<EmulatorCommandWriteRecord> {
    if let Some(record) = structured_emulator_command_write_record(line_no, message, options) {
        return Some(record);
    }
    let frame_hex = emulator_command_write_frame_hex(message)?;
    Some(EmulatorCommandWriteRecord {
        line_no,
        frame_hex,
        service_uuid: GOOSE_COMMAND_SERVICE_UUID.to_string(),
        characteristic_uuid: GOOSE_COMMAND_TO_STRAP_UUID.to_string(),
        write_type: normalize_write_type(&options.write_type)
            .unwrap_or_else(|| options.write_type.trim().to_string()),
    })
}

fn emulator_command_response_record(
    line_no: usize,
    message: &str,
) -> Option<EmulatorCommandResponseRecord> {
    if let Some(record) = structured_emulator_command_response_record(line_no, message) {
        return Some(record);
    }
    let frame_hex = emulator_command_response_frame_hex(message)?;
    Some(EmulatorCommandResponseRecord { line_no, frame_hex })
}

fn emulator_command_response_frame_hex(message: &str) -> Option<String> {
    if !message.contains("Notify command_from_strap ") || !message.contains("queued=true:") {
        return None;
    }
    let (_, value) = message.rsplit_once(':')?;
    normalize_emulator_hex(value)
}

fn normalize_emulator_hex(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .take_while(|ch| {
            ch.is_ascii_hexdigit() || ch.is_ascii_whitespace() || *ch == ':' || *ch == '-'
        })
        .filter(|ch| ch.is_ascii_hexdigit())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if normalized.is_empty() || normalized.len() % 2 != 0 {
        return None;
    }
    Some(normalized)
}

fn structured_emulator_command_write_record(
    line_no: usize,
    message: &str,
    options: &CommandEmulatorLogEvidenceOptions,
) -> Option<EmulatorCommandWriteRecord> {
    let value = serde_json::from_str::<serde_json::Value>(message).ok()?;
    if !structured_emulator_role_matches(&value, "command_to_strap", GOOSE_COMMAND_TO_STRAP_UUID) {
        return None;
    }
    if structured_emulator_direction(&value).as_deref() == Some("device_to_phone") {
        return None;
    }
    let frame_hex = structured_emulator_value_hex(&value)?;
    let line_no = structured_emulator_source_line(&value).unwrap_or(line_no);
    let service_uuid = structured_emulator_string_field(
        &value,
        &["service_uuid", "serviceUuid", "gatt_service_uuid"],
    )
    .map(normalize_ble_endpoint_value)
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| GOOSE_COMMAND_SERVICE_UUID.to_string());
    let characteristic_uuid = structured_emulator_string_field(
        &value,
        &[
            "characteristic_uuid",
            "characteristicUuid",
            "gatt_characteristic_uuid",
        ],
    )
    .map(normalize_ble_endpoint_value)
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| GOOSE_COMMAND_TO_STRAP_UUID.to_string());
    let write_type = structured_emulator_string_field(&value, &["write_type", "writeType"])
        .and_then(normalize_write_type)
        .or_else(|| normalize_write_type(&options.write_type))
        .unwrap_or_else(|| options.write_type.trim().to_string());
    Some(EmulatorCommandWriteRecord {
        line_no,
        frame_hex,
        service_uuid,
        characteristic_uuid,
        write_type,
    })
}

fn structured_emulator_command_response_record(
    line_no: usize,
    message: &str,
) -> Option<EmulatorCommandResponseRecord> {
    let value = serde_json::from_str::<serde_json::Value>(message).ok()?;
    if !structured_emulator_role_matches(
        &value,
        "command_from_strap",
        GOOSE_COMMAND_FROM_STRAP_UUID,
    ) {
        return None;
    }
    if structured_emulator_direction(&value).as_deref() == Some("phone_to_device") {
        return None;
    }
    if value
        .get("notify_queued")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return None;
    }
    let frame_hex = structured_emulator_value_hex(&value)?;
    let line_no = structured_emulator_source_line(&value).unwrap_or(line_no);
    Some(EmulatorCommandResponseRecord { line_no, frame_hex })
}

fn is_structured_emulator_command_row(value: &serde_json::Value) -> bool {
    structured_emulator_value_hex(value).is_some()
        && (structured_emulator_role_matches(
            value,
            "command_to_strap",
            GOOSE_COMMAND_TO_STRAP_UUID,
        ) || structured_emulator_role_matches(
            value,
            "command_from_strap",
            GOOSE_COMMAND_FROM_STRAP_UUID,
        ))
}

fn structured_emulator_role_matches(
    value: &serde_json::Value,
    expected_role: &str,
    expected_characteristic_uuid: &str,
) -> bool {
    structured_emulator_role(value).as_deref() == Some(expected_role)
        || structured_emulator_string_field(
            value,
            &[
                "characteristic_uuid",
                "characteristicUuid",
                "gatt_characteristic_uuid",
            ],
        )
        .map(|uuid| {
            normalize_ble_identifier_for_compare(uuid)
                == normalize_ble_identifier_for_compare(expected_characteristic_uuid)
        })
        .unwrap_or(false)
}

fn structured_emulator_role(value: &serde_json::Value) -> Option<String> {
    structured_emulator_string_field(value, &["role", "characteristic_role", "label"])
        .or_else(|| {
            value
                .get("characteristic_uuid_label")
                .and_then(|label| structured_emulator_string_field(label, &["role", "name"]))
        })
        .map(|role| role.trim().to_ascii_lowercase())
        .filter(|role| !role.is_empty())
}

fn structured_emulator_direction(value: &serde_json::Value) -> Option<String> {
    structured_emulator_string_field(value, &["direction"])
        .map(|direction| direction.trim().to_ascii_lowercase())
        .filter(|direction| !direction.is_empty())
}

fn structured_emulator_value_hex(value: &serde_json::Value) -> Option<String> {
    structured_emulator_string_field(
        value,
        &[
            "value_hex",
            "valueHex",
            "frame_hex",
            "frameHex",
            "payload_hex",
        ],
    )
    .and_then(normalize_emulator_hex)
}

fn structured_emulator_source_line(value: &serde_json::Value) -> Option<usize> {
    value
        .get("source_line")
        .or_else(|| value.get("sourceLine"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|line| usize::try_from(line).ok())
}

fn structured_emulator_string_field<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn parsed_command_write(parsed: &crate::protocol::ParsedFrame) -> Option<(u8, u8)> {
    match parsed.parsed_payload.as_ref()? {
        ParsedPayload::Command { command, .. } => Some(((*command)?, parsed.sequence?)),
        _ => None,
    }
}

fn parsed_command_response(parsed: &crate::protocol::ParsedFrame) -> Option<(u8, u8, u8)> {
    match parsed.parsed_payload.as_ref()? {
        ParsedPayload::CommandResponse {
            response_to_command,
            origin_sequence,
            result_code,
            ..
        } => Some((
            (*response_to_command)?,
            (*origin_sequence)?,
            (*result_code)?,
        )),
        _ => None,
    }
}

fn command_definition_by_number(command_number: u8) -> Option<&'static CommandDefinition> {
    COMMAND_DEFINITIONS
        .iter()
        .find(|definition| definition.command_number == Some(u16::from(command_number)))
}

fn command_name_for_provenance(definition: &CommandDefinition) -> String {
    definition.id.to_ascii_uppercase()
}

fn triggering_ui_action_for_command(
    definition: &CommandDefinition,
    options: &CommandEmulatorLogEvidenceOptions,
) -> String {
    options
        .triggering_ui_action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("official app emulator capture > {}", definition.id))
}

fn validate_command(
    definition: &CommandDefinition,
    evidence: Option<&CommandEvidence>,
) -> CommandValidationResult {
    let mut missing = Vec::new();
    let mut warnings = Vec::new();

    let Some(evidence) = evidence else {
        return CommandValidationResult {
            command: definition.id.to_string(),
            command_number: definition.command_number,
            family: definition.family.to_string(),
            risk_gate: definition.risk_gate,
            description: definition.description.to_string(),
            direct_send_ready: false,
            missing_requirements: vec!["official_capture_evidence".to_string()],
            warnings,
            next_capture_actions: next_capture_actions_for_missing(
                definition,
                &["official_capture_evidence".to_string()],
            ),
            validated_local_frame_hex: None,
            validated_official_frame_hex: None,
            validated_service_uuid: None,
            validated_characteristic_uuid: None,
            validated_write_type: None,
            validated_evidence_source: None,
            validated_capture_kind: None,
            validated_owner: None,
            validated_provenance_json: None,
            validated_triggering_ui_action: None,
        };
    };
    let evidence_source = command_evidence_source(evidence);
    let evidence_provenance_json = command_evidence_provenance_json(evidence);
    let evidence_provenance = parse_command_provenance_object(evidence_provenance_json.as_deref());
    let evidence_capture_kind = evidence_provenance
        .as_ref()
        .and_then(|object| provenance_string(object, "capture_kind"))
        .map(str::to_string);
    let evidence_owner = evidence_provenance
        .as_ref()
        .and_then(|object| provenance_string(object, "owner"))
        .map(str::to_string);
    let evidence_triggering_ui_action =
        command_evidence_triggering_ui_action(evidence, evidence_provenance.as_ref());

    let required_capture_count = match definition.risk_gate {
        CommandRiskGate::ReadOnly | CommandRiskGate::UserVisibleStateChange => 1,
        CommandRiskGate::CriticalStateChange => 2,
    };

    if evidence.official_capture_count < required_capture_count {
        missing.push(format!("official_capture_count>={required_capture_count}"));
    }
    validate_evidence_provenance(evidence, &mut missing, &mut warnings);
    let validated_endpoint = validate_command_endpoint(evidence, &mut missing, &mut warnings);
    if !frames_match(evidence) {
        missing.push("local_frame_matches_official_frame".to_string());
    }
    validate_official_command_frame(definition, evidence, &mut missing, &mut warnings);
    if !evidence.response_parser {
        missing.push("response_parser".to_string());
    } else {
        validate_official_response_frame(definition, evidence, &mut missing, &mut warnings);
    }
    if !evidence.visible_user_intent {
        missing.push("visible_user_intent".to_string());
    }
    if !matches!(definition.risk_gate, CommandRiskGate::ReadOnly)
        && evidence_triggering_ui_action.is_none()
    {
        missing.push("official_triggering_ui_action".to_string());
    }
    if !evidence.logging {
        missing.push("event_logging".to_string());
    }
    if !evidence.timeout_behavior {
        missing.push("timeout_behavior".to_string());
    }

    if matches!(definition.risk_gate, CommandRiskGate::CriticalStateChange) {
        if !evidence.failure_parser {
            missing.push("failure_parser".to_string());
        } else {
            validate_official_failure_response_frame(
                definition,
                evidence,
                &mut missing,
                &mut warnings,
            );
        }
        if !evidence.visible_confirmation {
            missing.push("visible_confirmation".to_string());
        }
        if !evidence.rollback_plan {
            missing.push("rollback_or_restore_plan".to_string());
        }
        if !evidence.explicit_approval {
            missing.push("explicit_approval".to_string());
        }
    }

    if evidence.official_frame_hex.is_some() ^ evidence.local_frame_hex.is_some() {
        warnings.push("only one frame hex value was provided".to_string());
    }

    CommandValidationResult {
        command: definition.id.to_string(),
        command_number: definition.command_number,
        family: definition.family.to_string(),
        risk_gate: definition.risk_gate,
        description: definition.description.to_string(),
        direct_send_ready: missing.is_empty(),
        next_capture_actions: next_capture_actions_for_missing(definition, &missing),
        missing_requirements: missing,
        warnings,
        validated_local_frame_hex: evidence.local_frame_hex.as_deref().map(normalize_hex),
        validated_official_frame_hex: evidence.official_frame_hex.as_deref().map(normalize_hex),
        validated_service_uuid: validated_endpoint
            .as_ref()
            .map(|endpoint| endpoint.service_uuid.clone()),
        validated_characteristic_uuid: validated_endpoint
            .as_ref()
            .map(|endpoint| endpoint.characteristic_uuid.clone()),
        validated_write_type: validated_endpoint.map(|endpoint| endpoint.write_type),
        validated_evidence_source: evidence_source,
        validated_capture_kind: evidence_capture_kind,
        validated_owner: evidence_owner,
        validated_provenance_json: evidence_provenance_json,
        validated_triggering_ui_action: evidence_triggering_ui_action,
    }
}

fn next_capture_actions_for_missing(
    definition: &CommandDefinition,
    missing: &[String],
) -> Vec<CommandNextCaptureAction> {
    let mut actions = Vec::new();
    for requirement in missing {
        actions.push(CommandNextCaptureAction {
            requirement: requirement.clone(),
            action: next_capture_action_for_requirement(definition, requirement),
        });
    }
    actions
}

fn next_capture_action_for_requirement(
    definition: &CommandDefinition,
    requirement: &str,
) -> String {
    let command = definition.id;
    if requirement.starts_with("official_capture_count>=") {
        let count = requirement.trim_start_matches("official_capture_count>=");
        return format!(
            "Capture at least {count} official app transactions for {command}; critical commands need independent captures before promotion."
        );
    }

    match requirement {
        "official_capture_evidence" => format!(
            "Use the official app against the real strap or macOS BLE emulator and capture the {command} write with bytes, endpoint, write type, timestamp, visible user action, and provenance."
        ),
        "official_capture_source" => {
            "Record the evidence source as user_owned_official_capture, passive_official_capture, or official_app_capture with user-owned official-app provenance.".to_string()
        }
        "official_capture_source_trusted" => {
            "Replace this row with user-owned official app or passive official capture evidence; private API replay cannot promote direct sends.".to_string()
        }
        "official_capture_provenance" => {
            "Attach provenance JSON describing capture app, capture kind, owner, device/app versions where known, and capture file path.".to_string()
        }
        "official_capture_provenance_json_object" => {
            "Fix provenance so it is a valid JSON object, not free text or another JSON type.".to_string()
        }
        "official_capture_provenance_non_empty_object" => {
            "Fill the provenance JSON object with non-empty capture metadata before trusting this row.".to_string()
        }
        "official_ble_service_uuid" => {
            "Record the BLE service UUID used by the official app write.".to_string()
        }
        "local_ble_service_uuid" => {
            "Set the Goose dry-run service UUID to the official captured command service.".to_string()
        }
        "official_ble_characteristic_uuid" => {
            "Record the BLE characteristic UUID used by the official app write.".to_string()
        }
        "local_ble_characteristic_uuid" => {
            "Set the Goose dry-run characteristic UUID to the official captured command characteristic.".to_string()
        }
        "official_ble_write_type" => {
            "Record whether the official app writes with response or without response.".to_string()
        }
        "official_ble_write_type_valid" => {
            "Normalize the captured official write type to with_response or without_response.".to_string()
        }
        "local_ble_write_type" => {
            "Set the Goose dry-run write type to the official captured write type.".to_string()
        }
        "local_ble_write_type_valid" => {
            "Normalize the Goose dry-run write type to with_response or without_response.".to_string()
        }
        "ble_service_uuid_matches_official_capture" => {
            "Update the Goose dry-run endpoint so its service UUID exactly matches the official capture.".to_string()
        }
        "ble_characteristic_uuid_matches_official_capture" => {
            "Update the Goose dry-run endpoint so its characteristic UUID exactly matches the official capture.".to_string()
        }
        "ble_write_type_matches_official_capture" => {
            "Update the Goose dry-run endpoint so its write type exactly matches the official capture.".to_string()
        }
        "local_frame_matches_official_frame" => format!(
            "Use the APK/firmware-derived builder or raw replay args until Goose dry-run bytes exactly match the official {command} frame."
        ),
        "official_frame_parseable" => {
            "Preserve the complete official app write frame and verify Goose can parse it.".to_string()
        }
        "official_frame_crc_valid" => {
            "Capture or reconstruct the full official frame with valid header and payload CRCs.".to_string()
        }
        "official_frame_is_command_payload" => {
            "Capture the command write frame rather than notifications, data packets, or unrelated payloads.".to_string()
        }
        "official_frame_command_number_matches_definition" => format!(
            "Re-check the command mapping; the captured write must parse as the {command} command number before promotion."
        ),
        "response_parser" => format!(
            "Implement or enable the success response parser, then prove it parses the official strap response for {command}."
        ),
        "official_response_frame" => format!(
            "Capture the strap-to-app success response for {command} after the official app write."
        ),
        "official_response_frame_parseable" => {
            "Preserve the complete success response frame and verify Goose can parse it.".to_string()
        }
        "official_response_frame_crc_valid" => {
            "Capture or reconstruct the full success response with valid header and payload CRCs.".to_string()
        }
        "official_response_frame_is_command_response" => {
            "Capture the command response frame for this write, not a sensor/data/event notification.".to_string()
        }
        "official_response_matches_command_number" => format!(
            "Pair the captured success response back to the {command} command number."
        ),
        "visible_user_intent" => {
            "Trigger the official app action from a visible UI path and mark visible_user_intent=true in the evidence.".to_string()
        }
        "official_triggering_ui_action" => {
            "Record the exact official app screen, button, or test action that produced this state-changing write.".to_string()
        }
        "event_logging" => {
            "Persist a Goose event-log entry for dry-run, preflight, and send attempts before enabling this command.".to_string()
        }
        "timeout_behavior" => {
            "Capture or document the official app timeout/retry behavior and encode the Goose timeout path for this command.".to_string()
        }
        "failure_parser" => {
            "Implement or enable the critical failure parser, then prove it parses a non-success official response.".to_string()
        }
        "official_failure_response_frame" => {
            "Capture a non-success official command response for this critical command.".to_string()
        }
        "official_failure_response_frame_parseable" => {
            "Preserve the complete failure response frame and verify Goose can parse it.".to_string()
        }
        "official_failure_response_frame_crc_valid" => {
            "Capture or reconstruct the full failure response with valid header and payload CRCs.".to_string()
        }
        "official_failure_response_frame_is_command_response" => {
            "Capture a command-response failure frame, not an unrelated notification or event.".to_string()
        }
        "official_failure_response_matches_command_number" => format!(
            "Pair the captured failure response back to the {command} command number."
        ),
        "official_failure_response_result_code_nonzero" => {
            "Use a real non-success response with a nonzero result code for critical failure handling.".to_string()
        }
        "visible_confirmation" => {
            "Add or verify a visible confirmation step before this critical command can be sent.".to_string()
        }
        "rollback_or_restore_plan" => match definition.risk_gate {
            CommandRiskGate::CriticalStateChange => {
                "Document and surface the rollback, restore, or safe-stop path for this critical command.".to_string()
            }
            _ => "Document the restore path before promotion.".to_string(),
        },
        "explicit_approval" => {
            "Require short-lived explicit runtime approval immediately before direct send.".to_string()
        }
        _ => format!("Resolve validation requirement {requirement} for {command}."),
    }
}

#[derive(Debug, Clone)]
struct ValidatedCommandEndpoint {
    service_uuid: String,
    characteristic_uuid: String,
    write_type: String,
}

fn validate_command_endpoint(
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<ValidatedCommandEndpoint> {
    let official_service = required_ble_endpoint_value(
        evidence.official_service_uuid.as_deref(),
        "official_ble_service_uuid",
        missing,
    );
    let local_service = required_ble_endpoint_value(
        evidence.local_service_uuid.as_deref(),
        "local_ble_service_uuid",
        missing,
    );
    let official_characteristic = required_ble_endpoint_value(
        evidence.official_characteristic_uuid.as_deref(),
        "official_ble_characteristic_uuid",
        missing,
    );
    let local_characteristic = required_ble_endpoint_value(
        evidence.local_characteristic_uuid.as_deref(),
        "local_ble_characteristic_uuid",
        missing,
    );
    let official_write_type = required_write_type(
        evidence.official_write_type.as_deref(),
        "official_ble_write_type",
        missing,
        warnings,
    );
    let local_write_type = required_write_type(
        evidence.local_write_type.as_deref(),
        "local_ble_write_type",
        missing,
        warnings,
    );

    if let (Some(official), Some(local)) = (&official_service, &local_service)
        && normalize_ble_identifier_for_compare(official)
            != normalize_ble_identifier_for_compare(local)
    {
        missing.push("ble_service_uuid_matches_official_capture".to_string());
        warnings.push(format!(
            "local BLE service {local} did not match official capture {official}"
        ));
    }
    if let (Some(official), Some(local)) = (&official_characteristic, &local_characteristic)
        && normalize_ble_identifier_for_compare(official)
            != normalize_ble_identifier_for_compare(local)
    {
        missing.push("ble_characteristic_uuid_matches_official_capture".to_string());
        warnings.push(format!(
            "local BLE characteristic {local} did not match official capture {official}"
        ));
    }
    if let (Some(official), Some(local)) = (&official_write_type, &local_write_type)
        && official != local
    {
        missing.push("ble_write_type_matches_official_capture".to_string());
        warnings.push(format!(
            "local BLE write type {local} did not match official capture {official}"
        ));
    }

    match (local_service, local_characteristic, local_write_type) {
        (Some(service_uuid), Some(characteristic_uuid), Some(write_type)) => {
            Some(ValidatedCommandEndpoint {
                service_uuid,
                characteristic_uuid,
                write_type,
            })
        }
        _ => None,
    }
}

fn required_ble_endpoint_value(
    value: Option<&str>,
    requirement: &str,
    missing: &mut Vec<String>,
) -> Option<String> {
    let normalized = value
        .map(normalize_ble_endpoint_value)
        .filter(|value| !value.is_empty());
    if normalized.is_none() {
        missing.push(requirement.to_string());
    }
    normalized
}

fn required_write_type(
    value: Option<&str>,
    requirement: &str,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        missing.push(requirement.to_string());
        return None;
    };
    let Some(normalized) = normalize_write_type(raw) else {
        missing.push(format!("{requirement}_valid"));
        warnings.push(format!("unsupported BLE write type: {raw}"));
        return None;
    };
    Some(normalized)
}

fn validate_evidence_provenance(
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let provenance_object = command_provenance_object(evidence, missing, warnings);
    let source = evidence
        .evidence_source
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if source.is_empty() {
        missing.push("official_capture_source".to_string());
    } else if !trusted_command_evidence_source(source, provenance_object.as_ref()) {
        missing.push("official_capture_source_trusted".to_string());
        if source == "private_api_replay" {
            warnings.push("private_api_replay_not_allowed_for_command_validation".to_string());
        } else {
            warnings.push(format!("untrusted command evidence source: {source}"));
        }
    }
}

fn command_provenance_object(
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let Some(provenance_json) = evidence
        .provenance_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        missing.push("official_capture_provenance".to_string());
        return None;
    };

    match serde_json::from_str::<serde_json::Value>(provenance_json) {
        Ok(serde_json::Value::Object(object)) if !object.is_empty() => Some(object),
        Ok(serde_json::Value::Object(_)) => {
            missing.push("official_capture_provenance_non_empty_object".to_string());
            None
        }
        Ok(_) => {
            missing.push("official_capture_provenance_json_object".to_string());
            None
        }
        Err(error) => {
            missing.push("official_capture_provenance_json_object".to_string());
            warnings.push(format!(
                "official capture provenance JSON parse failed: {error}"
            ));
            None
        }
    }
}

fn trusted_command_evidence_source(
    source: &str,
    provenance: Option<&serde_json::Map<String, serde_json::Value>>,
) -> bool {
    if TRUSTED_COMMAND_EVIDENCE_SOURCES.contains(&source) {
        return true;
    }
    if !TRUSTED_COMMAND_EVIDENCE_SOURCE_ALIASES.contains(&source) {
        return false;
    }
    let Some(provenance) = provenance else {
        return false;
    };
    provenance_string(provenance, "owner") == Some("user")
        && provenance_string(provenance, "capture_app") == Some("whoop_official")
        && provenance_string(provenance, "capture_kind")
            .is_some_and(|kind| TRUSTED_COMMAND_PROVENANCE_CAPTURE_KINDS.contains(&kind))
}

fn provenance_string<'a>(
    provenance: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    provenance
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn command_evidence_source_summary(
    evidence: &[CommandEvidence],
) -> Vec<CommandEvidenceSourceSummary> {
    let mut summaries: BTreeMap<
        (String, Option<String>, Option<String>),
        CommandEvidenceSourceSummary,
    > = BTreeMap::new();
    for row in evidence {
        let source = command_evidence_source(row).unwrap_or_else(|| "missing".to_string());
        let provenance = parse_command_provenance_object(row.provenance_json.as_deref());
        let capture_kind = provenance
            .as_ref()
            .and_then(|object| provenance_string(object, "capture_kind"))
            .map(str::to_string);
        let owner = provenance
            .as_ref()
            .and_then(|object| provenance_string(object, "owner"))
            .map(str::to_string);
        let trusted = trusted_command_evidence_source(&source, provenance.as_ref());
        let key = (source.clone(), capture_kind.clone(), owner.clone());
        let summary = summaries
            .entry(key)
            .or_insert_with(|| CommandEvidenceSourceSummary {
                evidence_source: source,
                capture_kind,
                owner,
                count: 0,
                trusted_for_promotion_count: 0,
                blocked_for_source_count: 0,
            });
        summary.count += 1;
        if trusted {
            summary.trusted_for_promotion_count += 1;
        } else {
            summary.blocked_for_source_count += 1;
        }
    }
    summaries.into_values().collect()
}

fn command_evidence_source(evidence: &CommandEvidence) -> Option<String> {
    evidence
        .evidence_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn command_evidence_provenance_json(evidence: &CommandEvidence) -> Option<String> {
    evidence
        .provenance_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn command_evidence_triggering_ui_action(
    evidence: &CommandEvidence,
    provenance: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    evidence
        .triggering_ui_action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let provenance = provenance?;
            ["triggering_ui_action", "ui_action", "official_ui_action"]
                .iter()
                .find_map(|key| provenance_string(provenance, key))
                .map(str::to_string)
        })
}

fn parse_command_provenance_object(
    provenance_json: Option<&str>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let provenance_json = provenance_json?.trim();
    if provenance_json.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(provenance_json).ok()? {
        serde_json::Value::Object(object) if !object.is_empty() => Some(object),
        _ => None,
    }
}

fn frames_match(evidence: &CommandEvidence) -> bool {
    let Some(official) = evidence.official_frame_hex.as_deref() else {
        return false;
    };
    let Some(local) = evidence.local_frame_hex.as_deref() else {
        return false;
    };
    normalize_hex(official) == normalize_hex(local) && !normalize_hex(official).is_empty()
}

fn promote_local_frame_candidate(
    definition: &CommandDefinition,
    evidence: &CommandEvidence,
    candidate: &CommandLocalFrameCandidate,
) -> Result<(CommandEvidence, CommandLocalFrameComparison), CommandLocalFrameComparison> {
    let command = evidence.command.clone();
    let source = candidate
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let official_frame_hex = evidence.official_frame_hex.as_deref().map(normalize_hex);
    let local_frame_hex = candidate.local_frame_hex.as_deref().map(normalize_hex);
    let mut warnings = Vec::new();

    let Some(official) = official_frame_hex
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "official_frame_missing".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    };
    let Some(local) = local_frame_hex.as_deref().filter(|value| !value.is_empty()) else {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "local_frame_missing".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    };

    let Some(expected_command_number) = definition.command_number.map(|value| value as u8) else {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "command_number_missing".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    };

    let official_parsed = match parse_frame_hex(DeviceType::Goose, official) {
        Ok(parsed) => parsed,
        Err(error) => {
            warnings.push(format!("official frame parse failed: {error}"));
            return Err(CommandLocalFrameComparison {
                command,
                matched: false,
                reason: "official_frame_parse_failed".to_string(),
                official_frame_hex,
                local_frame_hex,
                source,
                warnings,
            });
        }
    };
    if !official_parsed.header_crc_valid || !official_parsed.payload_crc_valid {
        warnings.push("official frame CRC did not validate".to_string());
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "official_frame_crc_invalid".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }
    let Some((official_command, _)) = parsed_command_write(&official_parsed) else {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "official_frame_not_command_payload".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    };
    if official_command != expected_command_number {
        warnings.push(format!(
            "official frame command {official_command} did not match expected {expected_command_number}"
        ));
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "official_frame_command_number_mismatch".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }

    let local_parsed = match parse_frame_hex(DeviceType::Goose, local) {
        Ok(parsed) => parsed,
        Err(error) => {
            warnings.push(format!("local frame parse failed: {error}"));
            return Err(CommandLocalFrameComparison {
                command,
                matched: false,
                reason: "local_frame_parse_failed".to_string(),
                official_frame_hex,
                local_frame_hex,
                source,
                warnings,
            });
        }
    };
    if !local_parsed.header_crc_valid || !local_parsed.payload_crc_valid {
        warnings.push("local frame CRC did not validate".to_string());
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "local_frame_crc_invalid".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }
    let Some((local_command, _)) = parsed_command_write(&local_parsed) else {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "local_frame_not_command_payload".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    };
    if local_command != expected_command_number {
        warnings.push(format!(
            "local frame command {local_command} did not match expected {expected_command_number}"
        ));
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "local_frame_command_number_mismatch".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }
    if official != local {
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason: "frame_bytes_differ".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }

    if let Err(reason) = candidate_endpoint_mismatch(evidence, candidate) {
        warnings.push("candidate endpoint did not match official capture".to_string());
        return Err(CommandLocalFrameComparison {
            command,
            matched: false,
            reason,
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        });
    }

    let mut promoted = evidence.clone();
    promoted.local_frame_hex = Some(local.to_string());
    if let Some(local_service_uuid) = candidate
        .local_service_uuid
        .as_deref()
        .map(normalize_ble_endpoint_value)
        .filter(|value| !value.is_empty())
    {
        promoted.local_service_uuid = Some(local_service_uuid);
    }
    if let Some(local_characteristic_uuid) = candidate
        .local_characteristic_uuid
        .as_deref()
        .map(normalize_ble_endpoint_value)
        .filter(|value| !value.is_empty())
    {
        promoted.local_characteristic_uuid = Some(local_characteristic_uuid);
    }
    if let Some(local_write_type) = candidate
        .local_write_type
        .as_deref()
        .and_then(normalize_write_type)
    {
        promoted.local_write_type = Some(local_write_type);
    }
    promoted.provenance_json = Some(merged_local_frame_match_provenance(evidence, candidate));

    Ok((
        promoted,
        CommandLocalFrameComparison {
            command,
            matched: true,
            reason: "local_frame_matches_official_frame".to_string(),
            official_frame_hex,
            local_frame_hex,
            source,
            warnings,
        },
    ))
}

fn candidate_endpoint_mismatch(
    evidence: &CommandEvidence,
    candidate: &CommandLocalFrameCandidate,
) -> Result<(), String> {
    if let (Some(official), Some(local)) = (
        evidence.official_service_uuid.as_deref(),
        candidate.local_service_uuid.as_deref(),
    ) && normalize_ble_identifier_for_compare(official)
        != normalize_ble_identifier_for_compare(local)
    {
        return Err("local_service_uuid_mismatch".to_string());
    }
    if let (Some(official), Some(local)) = (
        evidence.official_characteristic_uuid.as_deref(),
        candidate.local_characteristic_uuid.as_deref(),
    ) && normalize_ble_identifier_for_compare(official)
        != normalize_ble_identifier_for_compare(local)
    {
        return Err("local_characteristic_uuid_mismatch".to_string());
    }
    if let (Some(official), Some(local)) = (
        evidence.official_write_type.as_deref(),
        candidate.local_write_type.as_deref(),
    ) {
        match (normalize_write_type(official), normalize_write_type(local)) {
            (Some(official), Some(local)) if official == local => {}
            (Some(_), Some(_)) => return Err("local_write_type_mismatch".to_string()),
            _ => return Err("local_write_type_invalid".to_string()),
        }
    }
    Ok(())
}

fn merged_local_frame_match_provenance(
    evidence: &CommandEvidence,
    candidate: &CommandLocalFrameCandidate,
) -> String {
    let mut provenance =
        parse_command_provenance_object(evidence.provenance_json.as_deref()).unwrap_or_default();
    let candidate_provenance =
        parse_command_provenance_object(candidate.provenance_json.as_deref());
    let mut match_object = serde_json::Map::new();
    match_object.insert(
        "source".to_string(),
        serde_json::json!(
            candidate
                .source
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("goose_local_dry_run")
        ),
    );
    match_object.insert(
        "matched_official_frame".to_string(),
        serde_json::json!(true),
    );
    if let Some(candidate_provenance) = candidate_provenance {
        match_object.insert(
            "candidate_provenance".to_string(),
            serde_json::Value::Object(candidate_provenance),
        );
    }
    provenance.insert(
        "local_frame_match".to_string(),
        serde_json::Value::Object(match_object),
    );
    serde_json::Value::Object(provenance).to_string()
}

fn validate_official_command_frame(
    definition: &CommandDefinition,
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let Some(expected_command_number) = definition.command_number else {
        return;
    };
    let Some(official_frame_hex) = evidence.official_frame_hex.as_deref() else {
        if !missing
            .iter()
            .any(|requirement| requirement == "local_frame_matches_official_frame")
        {
            missing.push("local_frame_matches_official_frame".to_string());
        }
        return;
    };

    let parsed = match parse_frame_hex(DeviceType::Goose, official_frame_hex) {
        Ok(parsed) => parsed,
        Err(error) => {
            missing.push("official_frame_parseable".to_string());
            warnings.push(format!("official frame parse failed: {error}"));
            return;
        }
    };

    if !parsed.header_crc_valid || !parsed.payload_crc_valid {
        missing.push("official_frame_crc_valid".to_string());
    }

    let actual_command = match parsed.parsed_payload {
        Some(ParsedPayload::Command { command, .. }) => command,
        Some(other) => {
            missing.push("official_frame_is_command_payload".to_string());
            warnings.push(format!(
                "official frame payload was {}",
                payload_kind(&other)
            ));
            return;
        }
        None => {
            missing.push("official_frame_is_command_payload".to_string());
            warnings.push("official frame payload could not be classified".to_string());
            return;
        }
    };

    if actual_command.map(u16::from) != Some(expected_command_number) {
        missing.push("official_frame_command_number_matches_definition".to_string());
    }
}

fn validate_official_response_frame(
    definition: &CommandDefinition,
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let Some(expected_command_number) = definition.command_number else {
        return;
    };
    let Some(response_frame_hex) = evidence.official_response_frame_hex.as_deref() else {
        missing.push("official_response_frame".to_string());
        return;
    };

    let parsed = match parse_frame_hex(DeviceType::Goose, response_frame_hex) {
        Ok(parsed) => parsed,
        Err(error) => {
            missing.push("official_response_frame_parseable".to_string());
            warnings.push(format!("official response frame parse failed: {error}"));
            return;
        }
    };

    if !parsed.header_crc_valid || !parsed.payload_crc_valid {
        missing.push("official_response_frame_crc_valid".to_string());
    }

    let response_to_command = match parsed.parsed_payload {
        Some(ParsedPayload::CommandResponse {
            response_to_command,
            ..
        }) => response_to_command,
        Some(other) => {
            missing.push("official_response_frame_is_command_response".to_string());
            warnings.push(format!(
                "official response frame payload was {}",
                payload_kind(&other)
            ));
            return;
        }
        None => {
            missing.push("official_response_frame_is_command_response".to_string());
            warnings.push("official response frame payload could not be classified".to_string());
            return;
        }
    };

    if response_to_command.map(u16::from) != Some(expected_command_number) {
        missing.push("official_response_matches_command_number".to_string());
    }
}

fn validate_official_failure_response_frame(
    definition: &CommandDefinition,
    evidence: &CommandEvidence,
    missing: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let Some(expected_command_number) = definition.command_number else {
        return;
    };
    let Some(response_frame_hex) = evidence.official_failure_response_frame_hex.as_deref() else {
        missing.push("official_failure_response_frame".to_string());
        return;
    };

    let parsed = match parse_frame_hex(DeviceType::Goose, response_frame_hex) {
        Ok(parsed) => parsed,
        Err(error) => {
            missing.push("official_failure_response_frame_parseable".to_string());
            warnings.push(format!(
                "official failure response frame parse failed: {error}"
            ));
            return;
        }
    };

    if !parsed.header_crc_valid || !parsed.payload_crc_valid {
        missing.push("official_failure_response_frame_crc_valid".to_string());
    }

    let (response_to_command, result_code) = match parsed.parsed_payload {
        Some(ParsedPayload::CommandResponse {
            response_to_command,
            result_code,
            ..
        }) => (response_to_command, result_code),
        Some(other) => {
            missing.push("official_failure_response_frame_is_command_response".to_string());
            warnings.push(format!(
                "official failure response frame payload was {}",
                payload_kind(&other)
            ));
            return;
        }
        None => {
            missing.push("official_failure_response_frame_is_command_response".to_string());
            warnings.push(
                "official failure response frame payload could not be classified".to_string(),
            );
            return;
        }
    };

    if response_to_command.map(u16::from) != Some(expected_command_number) {
        missing.push("official_failure_response_matches_command_number".to_string());
    }
    if result_code == Some(0) || result_code.is_none() {
        missing.push("official_failure_response_result_code_nonzero".to_string());
    }
}

fn payload_kind(payload: &ParsedPayload) -> &'static str {
    match payload {
        ParsedPayload::Command { .. } => "command",
        ParsedPayload::CommandResponse { .. } => "command_response",
        ParsedPayload::Event { .. } => "event",
        ParsedPayload::DataPacket { .. } => "data_packet",
        ParsedPayload::Raw { .. } => "raw",
    }
}

fn normalize_hex(value: &str) -> String {
    value
        .chars()
        .filter(|char| !char.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_ble_endpoint_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_ble_identifier_for_compare(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_write_type(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "write"
        | "with_response"
        | "withresponse"
        | "write_with_response"
        | "writewithresponse" => Some("with_response".to_string()),
        "without_response"
        | "write_without_response"
        | "withoutresponse"
        | "writewithoutresponse"
        | "no_response"
        | "write_no_response" => Some("without_response".to_string()),
        _ => None,
    }
}

fn deserialize_optional_provenance_json<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        Some(value) => serde_json::to_string(&value)
            .map(Some)
            .map_err(de::Error::custom),
    }
}

fn deserialize_command_identifier<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        value => Err(de::Error::custom(format!(
            "command identifier must be a string or number, got {value}"
        ))),
    }
}

fn deserialize_optional_command_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(value)) => Ok(Some(value.to_string())),
        Some(serde_json::Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Some(value) => Err(de::Error::custom(format!(
            "command_id must be a string or number, got {value}"
        ))),
    }
}
