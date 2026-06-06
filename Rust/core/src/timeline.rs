use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    GooseError, GooseResult,
    debug_ws::{
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CORRECTED,
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CREATED,
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_PROMOTED,
        DEBUG_EVENT_TOPIC_ACTIVITY_FEATURE_WINDOW_CREATED,
        DEBUG_EVENT_TOPIC_ACTIVITY_SESSION_STATS_DISPLAYED,
        DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_COMPLETED,
        DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_PLANNED,
        DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_BLOCKED,
        DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_PLANNED,
    },
    protocol::ParsedPayload,
    store::{DebugEventRow, DecodedFrameRow, GooseStore, RawEvidenceRow},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PacketTimelineRow {
    pub timeline_id: String,
    pub frame_id: String,
    pub evidence_id: String,
    pub captured_at: String,
    pub category: String,
    pub title: String,
    pub packet_type_name: Option<String>,
    pub sequence: Option<i64>,
    pub command_or_event: Option<i64>,
    #[serde(default)]
    pub device_timestamp_seconds: Option<u32>,
    #[serde(default)]
    pub device_timestamp_subseconds: Option<u16>,
    #[serde(default)]
    pub body_hex: Option<String>,
    pub summary: serde_json::Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityStage {
    CaptureSession,
    RawFrame,
    DecodedPacket,
    FeatureWindow,
    ActivityCandidate,
    CandidateCorrection,
    PromotedSession,
    DisplayedStats,
    ExportPlanning,
    ExportCompleted,
    HealthSyncPlanning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservabilityTimelineRow {
    pub timeline_id: String,
    pub stage: ObservabilityStage,
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub parent_timeline_id: Option<String>,
    #[serde(default)]
    pub raw_evidence_id: Option<String>,
    #[serde(default)]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub feature_window_id: Option<String>,
    #[serde(default)]
    pub candidate_id: Option<String>,
    #[serde(default)]
    pub activity_session_id: Option<String>,
    #[serde(default)]
    pub stat_id: Option<String>,
    #[serde(default)]
    pub capture_session_id: Option<String>,
    #[serde(default)]
    pub capture_session_action_key: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub captured_at: Option<String>,
    #[serde(default)]
    pub time_unix_ms: Option<i64>,
    pub summary: serde_json::Value,
    pub warnings: Vec<String>,
}

pub fn packet_timeline_between(
    store: &GooseStore,
    start: &str,
    end: &str,
) -> GooseResult<Vec<PacketTimelineRow>> {
    let decoded_rows = store.decoded_frames_between(start, end)?;
    packet_timeline_from_decoded_frames(&decoded_rows)
}

pub fn packet_timeline_from_decoded_frames(
    decoded_rows: &[DecodedFrameRow],
) -> GooseResult<Vec<PacketTimelineRow>> {
    decoded_rows
        .iter()
        .map(timeline_row_from_decoded_frame)
        .collect()
}

pub fn observability_timeline_from_rows(
    raw_rows: &[RawEvidenceRow],
    packet_rows: &[PacketTimelineRow],
    debug_rows: &[DebugEventRow],
) -> GooseResult<Vec<ObservabilityTimelineRow>> {
    let capture_session_ids = capture_session_ids_from_debug_rows(debug_rows)?;
    let raw_capture_session_ids = raw_rows
        .iter()
        .map(|row| (row.evidence_id.clone(), row.capture_session_id.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut rows = raw_rows
        .iter()
        .map(|row| observability_row_from_raw_evidence(row, &capture_session_ids))
        .collect::<Vec<_>>();

    rows.extend(
        packet_rows
            .iter()
            .map(|row| {
                observability_row_from_packet_timeline(
                    row,
                    raw_capture_session_ids
                        .get(&row.evidence_id)
                        .cloned()
                        .flatten(),
                )
            })
            .collect::<Vec<_>>(),
    );

    for row in debug_rows
        .iter()
        .map(observability_row_from_debug_event)
        .collect::<GooseResult<Vec<_>>>()?
        .into_iter()
        .flatten()
    {
        rows.push(row);
    }

    rows.sort_by(|left, right| {
        observability_stage_rank(&left.stage)
            .cmp(&observability_stage_rank(&right.stage))
            .then_with(|| observability_sort_key(left).cmp(&observability_sort_key(right)))
            .then_with(|| left.timeline_id.cmp(&right.timeline_id))
    });

    Ok(rows)
}

fn timeline_row_from_decoded_frame(row: &DecodedFrameRow) -> GooseResult<PacketTimelineRow> {
    let parsed_payload: Option<ParsedPayload> = serde_json::from_str(&row.parsed_payload_json)
        .map_err(|error| {
            GooseError::message(format!(
                "{} parsed_payload_json invalid: {error}",
                row.frame_id
            ))
        })?;
    let warnings = parse_warnings(row)?;

    let (category, title, device_timestamp_seconds, device_timestamp_subseconds, body_hex, summary) =
        match parsed_payload {
            Some(ParsedPayload::Command {
                command,
                command_name,
                data_offset,
                data_hex,
                ..
            }) => {
                let label = command_name
                    .clone()
                    .or_else(|| command.map(|value| format!("command_{value}")))
                    .unwrap_or_else(|| "unknown_command".to_string());
                (
                    "command".to_string(),
                    format!("Command {label}"),
                    None,
                    None,
                    non_empty(data_hex.clone()),
                    json!({
                        "command": command,
                        "command_name": command_name,
                        "data_offset": data_offset,
                        "data_hex": data_hex,
                    }),
                )
            }
            Some(ParsedPayload::CommandResponse {
                response_to_command,
                response_to_command_name,
                origin_sequence,
                result_code,
                data_offset,
                data_hex,
                ..
            }) => {
                let label = response_to_command_name
                    .clone()
                    .or_else(|| response_to_command.map(|value| format!("command_{value}")))
                    .unwrap_or_else(|| "unknown_command".to_string());
                (
                    "command_response".to_string(),
                    format!(
                        "Command response {label} result {}",
                        result_code.unwrap_or(255)
                    ),
                    None,
                    None,
                    non_empty(data_hex.clone()),
                    json!({
                        "response_to_command": response_to_command,
                        "response_to_command_name": response_to_command_name,
                        "origin_sequence": origin_sequence,
                        "result_code": result_code,
                        "data_offset": data_offset,
                        "data_hex": data_hex,
                    }),
                )
            }
            Some(ParsedPayload::Event {
                event_id,
                event_name,
                timestamp_seconds,
                timestamp_subseconds,
                data_offset,
                data_hex,
                ..
            }) => {
                let label = event_name
                    .clone()
                    .or_else(|| event_id.map(|value| format!("event_{value}")))
                    .unwrap_or_else(|| "unknown_event".to_string());
                (
                    "event".to_string(),
                    format!("Event {label}"),
                    timestamp_seconds,
                    timestamp_subseconds,
                    non_empty(data_hex.clone()),
                    json!({
                        "event_id": event_id,
                        "event_name": event_name,
                        "data_offset": data_offset,
                        "data_hex": data_hex,
                    }),
                )
            }
            Some(ParsedPayload::DataPacket {
                packet_k,
                domain,
                status_or_stream,
                counter_or_page,
                timestamp_seconds,
                timestamp_subseconds,
                hr_marker_offset,
                hr_present_marker,
                body_offset,
                body_hex,
                body_summary,
                ..
            }) => {
                let label = domain
                    .clone()
                    .or_else(|| packet_k.map(|value| format!("k{value}")))
                    .unwrap_or_else(|| "unknown_data_packet".to_string());
                (
                    "data_packet".to_string(),
                    format!("Data packet {label}"),
                    timestamp_seconds,
                    timestamp_subseconds,
                    non_empty(body_hex.clone()),
                    json!({
                        "packet_k": packet_k,
                        "domain": domain,
                        "status_or_stream": status_or_stream,
                        "counter_or_page": counter_or_page,
                        "hr_marker_offset": hr_marker_offset,
                        "hr_present_marker": hr_present_marker,
                        "body_offset": body_offset,
                        "body_hex": body_hex,
                        "body_summary": body_summary,
                    }),
                )
            }
            Some(ParsedPayload::Raw {
                data_offset,
                data_hex,
                ..
            }) => (
                "raw".to_string(),
                "Raw packet payload".to_string(),
                None,
                None,
                non_empty(data_hex.clone()),
                json!({
                    "data_offset": data_offset,
                    "data_hex": data_hex,
                }),
            ),
            None => (
                "raw".to_string(),
                "Unclassified packet payload".to_string(),
                None,
                None,
                non_empty(row.payload_hex.clone()),
                json!({
                    "payload_hex": row.payload_hex,
                }),
            ),
        };

    Ok(PacketTimelineRow {
        timeline_id: format!("{}.timeline", row.frame_id),
        frame_id: row.frame_id.clone(),
        evidence_id: row.evidence_id.clone(),
        captured_at: row.captured_at.clone(),
        category,
        title,
        packet_type_name: row.packet_type_name.clone(),
        sequence: row.sequence,
        command_or_event: row.command_or_event,
        device_timestamp_seconds,
        device_timestamp_subseconds,
        body_hex,
        summary,
        warnings,
    })
}

fn parse_warnings(row: &DecodedFrameRow) -> GooseResult<Vec<String>> {
    serde_json::from_str(&row.warnings_json).map_err(|error| {
        GooseError::message(format!("{} warnings_json invalid: {error}", row.frame_id))
    })
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

fn observability_row_from_raw_evidence(
    row: &RawEvidenceRow,
    capture_session_ids: &BTreeSet<String>,
) -> ObservabilityTimelineRow {
    let capture_session_id = row.capture_session_id.clone();
    let parent_timeline_id = capture_session_id.as_ref().and_then(|session_id| {
        capture_session_ids
            .contains(session_id)
            .then(|| format!("capture-session.{session_id}.scan"))
    });
    ObservabilityTimelineRow {
        timeline_id: format!("raw.{}", row.evidence_id),
        stage: ObservabilityStage::RawFrame,
        category: "raw".to_string(),
        title: format!("Raw frame {}", row.evidence_id),
        parent_timeline_id,
        raw_evidence_id: Some(row.evidence_id.clone()),
        frame_id: None,
        feature_window_id: None,
        candidate_id: None,
        activity_session_id: None,
        stat_id: None,
        capture_session_id,
        capture_session_action_key: None,
        source: None,
        level: None,
        topic: None,
        message: None,
        command_id: None,
        captured_at: Some(row.captured_at.clone()),
        time_unix_ms: None,
        summary: json!({
            "source": row.source,
            "device_model": row.device_model,
            "sensitivity": row.sensitivity,
            "capture_session_id": row.capture_session_id,
            "sha256": row.sha256,
        }),
        warnings: Vec::new(),
    }
}

fn observability_row_from_packet_timeline(
    row: &PacketTimelineRow,
    capture_session_id: Option<String>,
) -> ObservabilityTimelineRow {
    ObservabilityTimelineRow {
        timeline_id: row.timeline_id.clone(),
        stage: ObservabilityStage::DecodedPacket,
        category: row.category.clone(),
        title: row.title.clone(),
        parent_timeline_id: Some(format!("raw.{}", row.evidence_id)),
        raw_evidence_id: Some(row.evidence_id.clone()),
        frame_id: Some(row.frame_id.clone()),
        feature_window_id: None,
        candidate_id: None,
        activity_session_id: None,
        stat_id: None,
        capture_session_id,
        capture_session_action_key: None,
        source: None,
        level: None,
        topic: None,
        message: None,
        command_id: None,
        captured_at: Some(row.captured_at.clone()),
        time_unix_ms: None,
        summary: row.summary.clone(),
        warnings: row.warnings.clone(),
    }
}

fn capture_session_ids_from_debug_rows(
    debug_rows: &[DebugEventRow],
) -> GooseResult<BTreeSet<String>> {
    let mut session_ids = BTreeSet::new();
    for row in debug_rows
        .iter()
        .filter(|row| row.topic.starts_with("capture.session."))
    {
        let data: serde_json::Value = serde_json::from_str(&row.data_json).map_err(|error| {
            GooseError::message(format!("{} data_json invalid: {error}", row.sequence))
        })?;
        if let Some(session_id) = observability_capture_session_id(&data) {
            session_ids.insert(session_id);
        }
    }
    Ok(session_ids)
}

fn observability_row_from_debug_event(
    row: &DebugEventRow,
) -> GooseResult<Option<ObservabilityTimelineRow>> {
    let data: serde_json::Value = serde_json::from_str(&row.data_json).map_err(|error| {
        GooseError::message(format!("{} data_json invalid: {error}", row.sequence))
    })?;

    let Some(stage) = observability_stage_for_topic(&row.topic) else {
        return Ok(None);
    };

    let timeline_id = observability_timeline_id(&stage, &data);
    let parent_timeline_id = observability_parent_timeline_id(&stage, &data);
    let (raw_evidence_id, frame_id, feature_window_id, candidate_id, activity_session_id, stat_id) =
        observability_link_ids(&data);
    let capture_session_id = observability_capture_session_id(&data);
    let capture_session_action_key = observability_capture_session_action_key(&data);
    let title = observability_title(&stage, &data, row);
    let category = observability_category(&stage).to_string();

    Ok(Some(ObservabilityTimelineRow {
        timeline_id,
        stage,
        category,
        title,
        parent_timeline_id,
        raw_evidence_id,
        frame_id,
        feature_window_id,
        candidate_id,
        activity_session_id,
        stat_id,
        capture_session_id,
        capture_session_action_key,
        source: Some(row.source.clone()),
        level: Some(row.level.clone()),
        topic: Some(row.topic.clone()),
        message: Some(row.message.clone()),
        command_id: row.command_id.clone(),
        captured_at: None,
        time_unix_ms: Some(row.time_unix_ms),
        summary: json!({
            "source": row.source,
            "level": row.level,
            "topic": row.topic,
            "message": row.message,
            "command_id": row.command_id,
            "data": data,
        }),
        warnings: Vec::new(),
    }))
}

fn observability_stage_for_topic(topic: &str) -> Option<ObservabilityStage> {
    match topic {
        t if t.starts_with("capture.session.") => Some(ObservabilityStage::CaptureSession),
        DEBUG_EVENT_TOPIC_ACTIVITY_FEATURE_WINDOW_CREATED => {
            Some(ObservabilityStage::FeatureWindow)
        }
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CREATED => Some(ObservabilityStage::ActivityCandidate),
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_CORRECTED => {
            Some(ObservabilityStage::CandidateCorrection)
        }
        DEBUG_EVENT_TOPIC_ACTIVITY_CANDIDATE_PROMOTED => Some(ObservabilityStage::PromotedSession),
        DEBUG_EVENT_TOPIC_ACTIVITY_SESSION_STATS_DISPLAYED => {
            Some(ObservabilityStage::DisplayedStats)
        }
        DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_PLANNED => Some(ObservabilityStage::ExportPlanning),
        DEBUG_EVENT_TOPIC_EXPORT_RAW_TIMEFRAME_COMPLETED => {
            Some(ObservabilityStage::ExportCompleted)
        }
        DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_PLANNED
        | DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_BLOCKED => {
            Some(ObservabilityStage::HealthSyncPlanning)
        }
        _ => None,
    }
}

fn observability_category(stage: &ObservabilityStage) -> &'static str {
    match stage {
        ObservabilityStage::CaptureSession => "capture",
        ObservabilityStage::RawFrame => "raw",
        ObservabilityStage::DecodedPacket => "packet",
        ObservabilityStage::FeatureWindow
        | ObservabilityStage::ActivityCandidate
        | ObservabilityStage::CandidateCorrection
        | ObservabilityStage::PromotedSession
        | ObservabilityStage::DisplayedStats => "activity",
        ObservabilityStage::ExportPlanning | ObservabilityStage::ExportCompleted => "export",
        ObservabilityStage::HealthSyncPlanning => "health_sync",
    }
}

fn observability_timeline_id(stage: &ObservabilityStage, data: &serde_json::Value) -> String {
    if matches!(stage, ObservabilityStage::CaptureSession) {
        return capture_session_timeline_id(data).unwrap_or_else(|| {
            format!(
                "capture-session.{}",
                data.get("capture_session_action_key")
                    .and_then(|value| value.as_str())
                    .or_else(|| data.get("action_key").and_then(|value| value.as_str()))
                    .unwrap_or("event")
            )
        });
    }
    let subject_id = observability_subject_id(stage, data).unwrap_or_else(|| {
        data.get("window_id")
            .and_then(|value| value.as_str())
            .or_else(|| data.get("candidate_id").and_then(|value| value.as_str()))
            .or_else(|| {
                data.get("activity_session_id")
                    .and_then(|value| value.as_str())
            })
            .or_else(|| data.get("stat_id").and_then(|value| value.as_str()))
            .or_else(|| data.get("export_job_id").and_then(|value| value.as_str()))
            .or_else(|| data.get("plan_id").and_then(|value| value.as_str()))
            .unwrap_or("event")
            .to_string()
    });
    format!("{}.{}", observability_stage_prefix(stage), subject_id)
}

fn observability_parent_timeline_id(
    stage: &ObservabilityStage,
    data: &serde_json::Value,
) -> Option<String> {
    match stage {
        ObservabilityStage::CaptureSession => {
            let session_id = observability_capture_session_id(data)?;
            let action_key = observability_capture_session_action_key(data)?;
            capture_session_parent_action_key(&action_key).map(|parent_action_key| {
                format!("capture-session.{session_id}.{parent_action_key}")
            })
        }
        ObservabilityStage::FeatureWindow => observability_frame_id(data)
            .map(|frame_id| format!("{}.timeline", frame_id))
            .or_else(|| {
                observability_raw_evidence_id(data)
                    .map(|evidence_id| format!("raw.{}", evidence_id))
            }),
        ObservabilityStage::ActivityCandidate => observability_feature_window_id(data)
            .map(|window_id| format!("feature-window.{}", window_id))
            .or_else(|| {
                observability_activity_session_id(data)
                    .map(|session_id| format!("promoted-session.{}", session_id))
            })
            .or_else(|| {
                observability_frame_id(data).map(|frame_id| format!("{}.timeline", frame_id))
            }),
        ObservabilityStage::CandidateCorrection => observability_candidate_id(data)
            .map(|candidate_id| format!("activity-candidate.{}", candidate_id))
            .or_else(|| {
                observability_feature_window_id(data)
                    .map(|window_id| format!("feature-window.{}", window_id))
            })
            .or_else(|| {
                observability_activity_session_id(data)
                    .map(|session_id| format!("promoted-session.{}", session_id))
            })
            .or_else(|| {
                observability_frame_id(data).map(|frame_id| format!("{}.timeline", frame_id))
            }),
        ObservabilityStage::PromotedSession | ObservabilityStage::DisplayedStats => {
            observability_activity_session_id(data)
                .map(|session_id| format!("promoted-session.{}", session_id))
                .or_else(|| {
                    observability_candidate_id(data)
                        .map(|candidate_id| format!("activity-candidate.{}", candidate_id))
                })
        }
        ObservabilityStage::ExportPlanning | ObservabilityStage::ExportCompleted => {
            observability_activity_session_id(data)
                .map(|session_id| format!("promoted-session.{}", session_id))
                .or_else(|| {
                    observability_candidate_id(data)
                        .map(|candidate_id| format!("activity-candidate.{}", candidate_id))
                })
        }
        ObservabilityStage::HealthSyncPlanning => observability_activity_session_id(data)
            .map(|session_id| format!("promoted-session.{}", session_id))
            .or_else(|| {
                observability_candidate_id(data)
                    .map(|candidate_id| format!("activity-candidate.{}", candidate_id))
            })
            .or_else(|| {
                observability_export_job_id(data)
                    .map(|export_job_id| format!("export-plan.{}", export_job_id))
            }),
        ObservabilityStage::RawFrame | ObservabilityStage::DecodedPacket => None,
    }
}

fn observability_link_ids(
    data: &serde_json::Value,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    (
        observability_raw_evidence_id(data),
        observability_frame_id(data),
        observability_feature_window_id(data),
        observability_candidate_id(data),
        observability_activity_session_id(data),
        observability_stat_id(data),
    )
}

fn observability_title(
    stage: &ObservabilityStage,
    data: &serde_json::Value,
    row: &DebugEventRow,
) -> String {
    match stage {
        ObservabilityStage::FeatureWindow => {
            let id =
                observability_feature_window_id(data).unwrap_or_else(|| row.sequence.to_string());
            format!("Feature window {}", id)
        }
        ObservabilityStage::CaptureSession => {
            let session_id =
                observability_capture_session_id(data).unwrap_or_else(|| row.sequence.to_string());
            let action_label = observability_capture_session_action_label(data)
                .unwrap_or_else(|| "session".to_string());
            format!("Capture session {action_label} {session_id}")
        }
        ObservabilityStage::ActivityCandidate => {
            let id = observability_candidate_id(data)
                .or_else(|| observability_feature_window_id(data))
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Activity candidate {}", id)
        }
        ObservabilityStage::CandidateCorrection => {
            let id = observability_candidate_id(data)
                .or_else(|| observability_activity_session_id(data))
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Candidate correction {}", id)
        }
        ObservabilityStage::PromotedSession => {
            let id = observability_activity_session_id(data)
                .or_else(|| observability_candidate_id(data))
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Promoted session {}", id)
        }
        ObservabilityStage::DisplayedStats => {
            let id = observability_stat_id(data)
                .or_else(|| observability_activity_session_id(data))
                .or_else(|| {
                    data.get("metric_name")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Displayed stat {}", id)
        }
        ObservabilityStage::ExportPlanning => {
            let id = observability_export_job_id(data)
                .or_else(|| row.command_id.clone())
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Export plan {}", id)
        }
        ObservabilityStage::ExportCompleted => {
            let id = observability_export_job_id(data)
                .or_else(|| row.command_id.clone())
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Export complete {}", id)
        }
        ObservabilityStage::HealthSyncPlanning => {
            let id = observability_health_sync_plan_id(data)
                .or_else(|| observability_activity_session_id(data))
                .or_else(|| row.command_id.clone())
                .unwrap_or_else(|| row.sequence.to_string());
            if row.topic.as_str() == DEBUG_EVENT_TOPIC_HEALTH_SYNC_ACTIVITY_BLOCKED {
                format!("Health sync blocked {}", id)
            } else {
                format!("Health sync plan {}", id)
            }
        }
        ObservabilityStage::RawFrame => {
            let id = observability_raw_evidence_id(data)
                .or_else(|| row.command_id.clone())
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Raw frame {}", id)
        }
        ObservabilityStage::DecodedPacket => {
            let id = observability_frame_id(data)
                .or_else(|| row.command_id.clone())
                .unwrap_or_else(|| row.sequence.to_string());
            format!("Decoded packet {}", id)
        }
    }
}

fn observability_subject_id(
    stage: &ObservabilityStage,
    data: &serde_json::Value,
) -> Option<String> {
    match stage {
        ObservabilityStage::CaptureSession => capture_session_subject_id(data),
        ObservabilityStage::RawFrame => observability_raw_evidence_id(data),
        ObservabilityStage::DecodedPacket => observability_frame_id(data),
        ObservabilityStage::FeatureWindow => observability_feature_window_id(data),
        ObservabilityStage::ActivityCandidate | ObservabilityStage::CandidateCorrection => {
            observability_candidate_id(data).or_else(|| observability_feature_window_id(data))
        }
        ObservabilityStage::PromotedSession | ObservabilityStage::DisplayedStats => {
            observability_activity_session_id(data).or_else(|| observability_candidate_id(data))
        }
        ObservabilityStage::ExportPlanning | ObservabilityStage::ExportCompleted => {
            observability_export_job_id(data).or_else(|| observability_activity_session_id(data))
        }
        ObservabilityStage::HealthSyncPlanning => observability_health_sync_plan_id(data)
            .or_else(|| observability_activity_session_id(data))
            .or_else(|| observability_candidate_id(data)),
    }
}

fn observability_stage_prefix(stage: &ObservabilityStage) -> &'static str {
    match stage {
        ObservabilityStage::CaptureSession => "capture-session",
        ObservabilityStage::RawFrame => "raw",
        ObservabilityStage::DecodedPacket => "packet",
        ObservabilityStage::FeatureWindow => "feature-window",
        ObservabilityStage::ActivityCandidate => "activity-candidate",
        ObservabilityStage::CandidateCorrection => "candidate-correction",
        ObservabilityStage::PromotedSession => "promoted-session",
        ObservabilityStage::DisplayedStats => "displayed-stats",
        ObservabilityStage::ExportPlanning => "export-plan",
        ObservabilityStage::ExportCompleted => "export-complete",
        ObservabilityStage::HealthSyncPlanning => "health-sync",
    }
}

fn observability_stage_rank(stage: &ObservabilityStage) -> u8 {
    match stage {
        ObservabilityStage::CaptureSession => 0,
        ObservabilityStage::RawFrame => 1,
        ObservabilityStage::DecodedPacket => 2,
        ObservabilityStage::FeatureWindow => 3,
        ObservabilityStage::ActivityCandidate => 4,
        ObservabilityStage::CandidateCorrection => 5,
        ObservabilityStage::PromotedSession => 6,
        ObservabilityStage::DisplayedStats => 7,
        ObservabilityStage::ExportPlanning => 8,
        ObservabilityStage::ExportCompleted => 9,
        ObservabilityStage::HealthSyncPlanning => 10,
    }
}

fn observability_sort_key(row: &ObservabilityTimelineRow) -> String {
    row.captured_at
        .clone()
        .or_else(|| row.time_unix_ms.map(|value| format!("{value:020}")))
        .unwrap_or_else(|| row.timeline_id.clone())
}

fn observability_raw_evidence_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["raw_evidence_id", "evidence_id"])
}

fn observability_capture_session_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["capture_session_id"])
}

fn observability_capture_session_action_key(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["capture_session_action_key", "action_key"])
}

fn observability_capture_session_action_label(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["capture_session_action", "action"])
}

fn capture_session_timeline_id(data: &serde_json::Value) -> Option<String> {
    let subject_id = capture_session_subject_id(data)?;
    Some(format!("capture-session.{subject_id}"))
}

fn capture_session_subject_id(data: &serde_json::Value) -> Option<String> {
    let session_id = observability_capture_session_id(data)?;
    let action_key = observability_capture_session_action_key(data)?;
    Some(format!("{session_id}.{action_key}"))
}

fn capture_session_parent_action_key(action_key: &str) -> Option<&'static str> {
    match action_key {
        "scan" => None,
        "connect" => Some("scan"),
        "subscribe" => Some("connect"),
        "capture_start" => Some("subscribe"),
        "import_notifications" => Some("subscribe"),
        "import_capture_file" => Some("import_notifications"),
        "capture_stop" => Some("import_capture_file"),
        _ => None,
    }
}

fn observability_frame_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["frame_id", "decoded_frame_id"])
}

fn observability_feature_window_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["feature_window_id", "window_id"])
}

fn observability_candidate_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["candidate_id"])
}

fn observability_activity_session_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["activity_session_id", "session_id"])
}

fn observability_stat_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["stat_id", "metric_name"])
}

fn observability_export_job_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["export_job_id"])
}

fn observability_health_sync_plan_id(data: &serde_json::Value) -> Option<String> {
    string_field(data, &["plan_id"])
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    })
}
