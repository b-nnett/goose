//! Generic activity candidate classification for pre-device testing.
//!
//! The gravity-stability reference below is inspired by the OpenWhoop activity
//! heuristic discussed in `docs/pre-whoop-readiness-todo.md` and the linked
//! OpenWhoop snapshot. This implementation is independent and conservative: it
//! recomputes a stability score from supplied gravity samples instead of
//! porting the reference code or assuming a specific sport such as running.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::activity_sessions::{
    ACTIVITY_SESSION_CORRECTION_SCOPE, activity_session_correction_plans,
};
use crate::health_sync::{ActivitySyncCandidate, ActivitySyncMetric, HealthSyncSessionKind};

pub const ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA: &str =
    "goose.activity-candidate-classifier-input.v1";
pub const ACTIVITY_CANDIDATE_CLASSIFIER_REPORT_SCHEMA: &str =
    "goose.activity-candidate-classifier-report.v1";
pub const ACTIVITY_CANDIDATE_UNKNOWN_ACTIVITY_TYPE: &str = "unknown";
pub const ACTIVITY_CANDIDATE_HEURISTIC_MOTION: &str = "heuristic_motion";
pub const ACTIVITY_CANDIDATE_HEURISTIC_HR_MOTION: &str = "heuristic_hr_motion";
pub const ACTIVITY_CANDIDATE_GENERATED_BY: &str = "goose-activity-candidate-classifier";
pub const ACTIVITY_SESSION_PACKET_DERIVED_METRIC_PLAN_REPORT_SCHEMA: &str =
    "goose.activity-session-packet-derived-metric-plan-report.v1";
pub const ACTIVITY_SESSION_PACKET_DERIVED_METRIC_PLAN_GENERATED_BY: &str =
    "goose-activity-session-packet-derived-metric-planner";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityCandidateClassifierInput {
    pub schema: String,
    #[serde(default)]
    pub options: ActivityCandidateClassifierOptions,
    #[serde(default)]
    pub windows: Vec<ActivityFeatureWindowInput>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ActivityCandidateClassifierOptions {
    pub min_evidence_confidence_0_to_1: f64,
    pub min_gravity_stability_0_to_1: f64,
    pub min_promotion_confidence_0_to_1: f64,
    #[serde(default)]
    pub require_user_approval: bool,
}

impl Default for ActivityCandidateClassifierOptions {
    fn default() -> Self {
        Self {
            min_evidence_confidence_0_to_1: 0.75,
            min_gravity_stability_0_to_1: 0.80,
            min_promotion_confidence_0_to_1: 0.80,
            require_user_approval: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityFeatureWindowInput {
    pub window_id: String,
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub heart_rate: Option<ActivityHeartRateEvidence>,
    #[serde(default)]
    pub motion: Option<ActivityMotionEvidence>,
    #[serde(default)]
    pub command_sync: Option<ActivityCommandSyncEvidence>,
    #[serde(default)]
    pub approved_by_user: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityHeartRateEvidence {
    pub heart_rate_bpm: f64,
    pub confidence_0_to_1: f64,
    pub provenance: ActivityEvidenceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityMotionEvidence {
    pub gravity_samples: Vec<ActivityGravitySample>,
    pub confidence_0_to_1: f64,
    pub provenance: ActivityEvidenceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityGravitySample {
    pub gravity_x_g: f64,
    pub gravity_y_g: f64,
    pub gravity_z_g: f64,
    pub confidence_0_to_1: f64,
    pub provenance: ActivityEvidenceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityCommandSyncEvidence {
    pub synced: bool,
    pub confidence_0_to_1: f64,
    pub provenance: ActivityEvidenceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityEvidenceProvenance {
    pub source: String,
    #[serde(default)]
    pub evidence_id: Option<String>,
    #[serde(default)]
    pub capture_session_id: Option<String>,
    #[serde(default)]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "default_provenance_details")]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityCandidateClassifierReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub window_count: usize,
    pub candidate_window_count: usize,
    pub unknown_window_count: usize,
    pub blocked_window_count: usize,
    pub windows: Vec<ActivityCandidateWindowReport>,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ActivityCandidateNextAction>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityCandidateState {
    Unknown,
    Candidate,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityCandidateWindowReport {
    pub window_id: String,
    pub start_time: String,
    pub end_time: String,
    pub activity_type: String,
    pub state: ActivityCandidateState,
    pub confidence_0_to_1: f64,
    #[serde(default)]
    pub heart_rate_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub motion_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub gravity_stability_0_to_1: Option<f64>,
    #[serde(default)]
    pub command_sync_confidence_0_to_1: Option<f64>,
    #[serde(default)]
    pub approved_by_user: bool,
    pub readiness_reasons: Vec<String>,
    pub blocker_reasons: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<ActivityCandidateNextAction>,
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivityCandidateNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySessionPacketDerivedMetricPlanReport {
    pub schema: String,
    pub generated_by: String,
    pub pass: bool,
    pub session_id: String,
    pub activity_type: String,
    pub session_kind: HealthSyncSessionKind,
    pub source_kind: String,
    pub metric_count: usize,
    pub attached_metric_count: usize,
    pub ignored_metric_count: usize,
    pub metric_plans: Vec<ActivitySessionPacketDerivedMetricPlan>,
    pub ignored_metric_names: Vec<String>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivitySessionPacketDerivedMetricPlan {
    pub session_id: String,
    pub activity_type: String,
    pub session_kind: HealthSyncSessionKind,
    pub source_kind: String,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub quality_flags: Vec<String>,
    pub provenance: Value,
}

pub fn run_activity_candidate_classifier(
    input: &ActivityCandidateClassifierInput,
) -> ActivityCandidateClassifierReport {
    let mut issues = Vec::new();
    if input.schema != ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA {
        issues.push("activity_candidate_classifier_input_schema_mismatch".to_string());
    }
    if input.windows.is_empty() {
        issues.push("no_activity_feature_windows_provided".to_string());
    }

    let windows = input
        .windows
        .iter()
        .map(|window| classify_activity_feature_window(window, input.options))
        .collect::<Vec<_>>();

    let candidate_window_count = windows
        .iter()
        .filter(|window| window.state == ActivityCandidateState::Candidate)
        .count();
    let unknown_window_count = windows
        .iter()
        .filter(|window| window.state == ActivityCandidateState::Unknown)
        .count();
    let blocked_window_count = windows
        .iter()
        .filter(|window| window.state == ActivityCandidateState::Blocked)
        .count();

    let mut next_actions = Vec::new();
    if input.schema != ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA {
        next_actions.push(ActivityCandidateNextAction {
            scope: "activity_candidate_classifier".to_string(),
            reason: "activity_candidate_classifier_input_schema_mismatch".to_string(),
            action: format!(
                "Set schema to {} before rerunning the classifier.",
                ACTIVITY_CANDIDATE_CLASSIFIER_INPUT_SCHEMA
            ),
        });
    }
    if input.windows.is_empty() {
        next_actions.push(ActivityCandidateNextAction {
            scope: "activity_candidate_classifier".to_string(),
            reason: "no_activity_feature_windows_provided".to_string(),
            action:
                "Provide at least one HR or motion feature window before rerunning the classifier."
                    .to_string(),
        });
    }
    next_actions.extend(
        windows
            .iter()
            .flat_map(|window| window.next_actions.iter().cloned()),
    );
    let next_actions = dedupe_activity_next_actions(next_actions);

    let pass = issues.is_empty()
        && candidate_window_count == windows.len()
        && blocked_window_count == 0
        && unknown_window_count == 0
        && !windows.is_empty();

    ActivityCandidateClassifierReport {
        schema: ACTIVITY_CANDIDATE_CLASSIFIER_REPORT_SCHEMA.to_string(),
        generated_by: ACTIVITY_CANDIDATE_GENERATED_BY.to_string(),
        pass,
        window_count: windows.len(),
        candidate_window_count,
        unknown_window_count,
        blocked_window_count,
        windows,
        issues,
        next_actions,
    }
}

pub fn classify_activity_feature_window(
    window: &ActivityFeatureWindowInput,
    options: ActivityCandidateClassifierOptions,
) -> ActivityCandidateWindowReport {
    let hr = window.heart_rate.as_ref();
    let motion = window.motion.as_ref();
    let command_sync = window.command_sync.as_ref();
    let gravity_stability = motion.and_then(gravity_stability_reference);
    let combined_confidence =
        combined_confidence(hr, motion, command_sync, gravity_stability).clamp(0.0, 1.0);

    let mut readiness_reasons = BTreeSet::new();
    let mut blocker_reasons = BTreeSet::new();

    if let Some(heart_rate) = hr {
        readiness_reasons.insert("heart_rate_present".to_string());
        if !is_unit_interval(heart_rate.confidence_0_to_1)
            || heart_rate.confidence_0_to_1 < options.min_evidence_confidence_0_to_1
        {
            blocker_reasons.insert("low_confidence".to_string());
        }
    } else {
        blocker_reasons.insert("missing_heart_rate".to_string());
    }

    if let Some(motion) = motion {
        readiness_reasons.insert("motion_present".to_string());
        if !is_unit_interval(motion.confidence_0_to_1)
            || motion.confidence_0_to_1 < options.min_evidence_confidence_0_to_1
        {
            blocker_reasons.insert("low_confidence".to_string());
        }
        match gravity_stability {
            Some(score) => {
                readiness_reasons.insert(format!("gravity_stability_reference:{score:.3}"));
                if score < options.min_gravity_stability_0_to_1 {
                    blocker_reasons.insert("low_confidence".to_string());
                }
            }
            None => {
                readiness_reasons.insert("gravity_stability_reference:unavailable".to_string());
                blocker_reasons.insert("low_confidence".to_string());
            }
        }
    } else {
        blocker_reasons.insert("missing_motion".to_string());
    }

    if let Some(command_sync) = command_sync {
        if command_sync.synced {
            readiness_reasons.insert("command_sync_present".to_string());
            if !is_unit_interval(command_sync.confidence_0_to_1)
                || command_sync.confidence_0_to_1 < options.min_evidence_confidence_0_to_1
            {
                blocker_reasons.insert("low_confidence".to_string());
            }
        } else {
            readiness_reasons.insert("command_sync_present_but_not_synced".to_string());
            blocker_reasons.insert("missing_command_sync".to_string());
        }
    } else {
        blocker_reasons.insert("missing_command_sync".to_string());
    }

    if window.approved_by_user {
        readiness_reasons.insert("candidate_promotion_approved".to_string());
    }

    if !is_unit_interval(combined_confidence)
        || combined_confidence < options.min_promotion_confidence_0_to_1
    {
        blocker_reasons.insert("low_confidence".to_string());
    }

    let has_missing_or_quality_blocker = blocker_reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "missing_heart_rate" | "missing_motion" | "missing_command_sync" | "low_confidence"
        )
    });
    if !has_missing_or_quality_blocker && options.require_user_approval && !window.approved_by_user
    {
        blocker_reasons.insert("candidate_promotion_not_approved".to_string());
    }

    let state = if has_missing_or_quality_blocker {
        ActivityCandidateState::Unknown
    } else if options.require_user_approval && !window.approved_by_user {
        ActivityCandidateState::Blocked
    } else {
        ActivityCandidateState::Candidate
    };

    let blocker_reasons = blocker_reasons.into_iter().collect::<Vec<_>>();
    let readiness_reasons = readiness_reasons.into_iter().collect::<Vec<_>>();

    let mut next_actions = next_actions_for_blockers(&window.window_id, &blocker_reasons);
    if state == ActivityCandidateState::Candidate {
        next_actions.extend(activity_session_correction_plans().into_iter().map(|plan| {
            ActivityCandidateNextAction {
                scope: ACTIVITY_SESSION_CORRECTION_SCOPE.to_string(),
                reason: plan.kind.as_str().to_string(),
                action: plan.action,
            }
        }));
    }
    let next_actions = dedupe_activity_next_actions(next_actions);

    let provenance = json!({
        "classifier": ACTIVITY_CANDIDATE_GENERATED_BY,
        "window_id": window.window_id,
        "start_time": window.start_time,
        "end_time": window.end_time,
        "activity_type": ACTIVITY_CANDIDATE_UNKNOWN_ACTIVITY_TYPE,
        "state": state,
        "approved_by_user": window.approved_by_user,
        "heart_rate_provenance": window.heart_rate.as_ref().map(|e| e.provenance.clone()),
        "motion_provenance": window.motion.as_ref().map(|e| e.provenance.clone()),
        "command_sync_provenance": window.command_sync.as_ref().map(|e| e.provenance.clone()),
        "gravity_stability_reference": gravity_stability,
        "combined_confidence_0_to_1": combined_confidence,
        "readiness_reasons": readiness_reasons.clone(),
        "blocker_reasons": blocker_reasons.clone(),
    });

    ActivityCandidateWindowReport {
        window_id: window.window_id.clone(),
        start_time: window.start_time.clone(),
        end_time: window.end_time.clone(),
        activity_type: ACTIVITY_CANDIDATE_UNKNOWN_ACTIVITY_TYPE.to_string(),
        state,
        confidence_0_to_1: combined_confidence,
        heart_rate_confidence_0_to_1: hr.map(|e| e.confidence_0_to_1),
        motion_confidence_0_to_1: motion.map(|e| e.confidence_0_to_1),
        gravity_stability_0_to_1: gravity_stability,
        command_sync_confidence_0_to_1: command_sync.map(|e| e.confidence_0_to_1),
        approved_by_user: window.approved_by_user,
        readiness_reasons,
        blocker_reasons,
        next_actions,
        provenance,
    }
}

fn gravity_stability_reference(motion: &ActivityMotionEvidence) -> Option<f64> {
    // OpenWhoop describes gravity-vector stability as a useful activity/sleep
    // reference signal. This implementation recomputes that idea from the
    // supplied samples with pairwise alignment and conservative weighting.
    let samples = motion
        .gravity_samples
        .iter()
        .filter_map(|sample| {
            if !is_unit_interval(sample.confidence_0_to_1) {
                return None;
            }
            let magnitude = (sample.gravity_x_g * sample.gravity_x_g
                + sample.gravity_y_g * sample.gravity_y_g
                + sample.gravity_z_g * sample.gravity_z_g)
                .sqrt();
            if !magnitude.is_finite() || magnitude <= f64::EPSILON {
                return None;
            }
            let weight = sample.confidence_0_to_1.clamp(0.0, 1.0);
            Some((
                [
                    sample.gravity_x_g / magnitude,
                    sample.gravity_y_g / magnitude,
                    sample.gravity_z_g / magnitude,
                ],
                weight,
            ))
        })
        .collect::<Vec<_>>();

    if samples.len() < 2 {
        return None;
    }

    let mut pairwise_alignment_sum = 0.0;
    let mut pairwise_weight_sum = 0.0;
    for (index, (left, left_weight)) in samples.iter().enumerate() {
        for (right, right_weight) in samples.iter().skip(index + 1) {
            let dot = dot3(left, right).clamp(-1.0, 1.0);
            let alignment_0_to_1 = (dot + 1.0) / 2.0;
            let pair_weight = (*left_weight + *right_weight) / 2.0;
            pairwise_alignment_sum += alignment_0_to_1 * pair_weight;
            pairwise_weight_sum += pair_weight;
        }
    }

    if pairwise_weight_sum <= 0.0 {
        return None;
    }

    let pairwise_alignment_0_to_1 = pairwise_alignment_sum / pairwise_weight_sum;
    let average_sample_confidence_0_to_1 =
        samples.iter().map(|(_, weight)| *weight).sum::<f64>() / samples.len() as f64;
    let sample_factor_0_to_1 = (samples.len() as f64 / 4.0).min(1.0);

    Some(
        (pairwise_alignment_0_to_1 * 0.7
            + average_sample_confidence_0_to_1 * 0.2
            + sample_factor_0_to_1 * 0.1)
            .clamp(0.0, 1.0),
    )
}

fn combined_confidence(
    heart_rate: Option<&ActivityHeartRateEvidence>,
    motion: Option<&ActivityMotionEvidence>,
    command_sync: Option<&ActivityCommandSyncEvidence>,
    gravity_stability: Option<f64>,
) -> f64 {
    let mut scores = Vec::new();

    if let Some(heart_rate) = heart_rate
        && is_unit_interval(heart_rate.confidence_0_to_1)
    {
        scores.push(heart_rate.confidence_0_to_1.clamp(0.0, 1.0));
    }
    if let Some(motion) = motion
        && is_unit_interval(motion.confidence_0_to_1)
    {
        scores.push(motion.confidence_0_to_1.clamp(0.0, 1.0));
    }
    if let Some(command_sync) = command_sync
        && command_sync.synced
        && is_unit_interval(command_sync.confidence_0_to_1)
    {
        scores.push(command_sync.confidence_0_to_1.clamp(0.0, 1.0));
    }
    if let Some(gravity_stability) = gravity_stability
        && is_unit_interval(gravity_stability)
    {
        scores.push(gravity_stability.clamp(0.0, 1.0));
    }

    if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    }
}

fn next_actions_for_blockers(
    scope: &str,
    blocker_reasons: &[String],
) -> Vec<ActivityCandidateNextAction> {
    let mut actions = Vec::new();

    for reason in blocker_reasons {
        match reason.as_str() {
            "missing_heart_rate" => actions.push(ActivityCandidateNextAction {
                scope: scope.to_string(),
                reason: reason.clone(),
                action: "Capture trusted heart-rate evidence for this window and rerun the classifier.".to_string(),
            }),
            "missing_motion" => actions.push(ActivityCandidateNextAction {
                scope: scope.to_string(),
                reason: reason.clone(),
                action: "Capture motion evidence with gravity samples for this window and rerun the classifier.".to_string(),
            }),
            "missing_command_sync" => actions.push(ActivityCandidateNextAction {
                scope: scope.to_string(),
                reason: reason.clone(),
                action: "Carry command-sync evidence into the feature window before promoting it.".to_string(),
            }),
            "low_confidence" => actions.push(ActivityCandidateNextAction {
                scope: scope.to_string(),
                reason: reason.clone(),
                action: "Tighten the window or improve evidence quality until the confidence score clears the threshold.".to_string(),
            }),
            "candidate_promotion_not_approved" => actions.push(ActivityCandidateNextAction {
                scope: scope.to_string(),
                reason: reason.clone(),
                action: "Ask the user to approve candidate promotion before creating an activity session.".to_string(),
            }),
            _ => {}
        }
    }

    actions
}

fn dedupe_activity_next_actions(
    actions: Vec<ActivityCandidateNextAction>,
) -> Vec<ActivityCandidateNextAction> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();

    for action in actions {
        if seen.insert((
            action.scope.clone(),
            action.reason.clone(),
            action.action.clone(),
        )) {
            deduped.push(action);
        }
    }

    deduped
}

fn default_provenance_details() -> Value {
    json!({})
}

fn is_unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn dot3(left: &[f64; 3], right: &[f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

pub fn run_packet_derived_activity_metric_planner(
    session: &ActivitySyncCandidate,
) -> ActivitySessionPacketDerivedMetricPlanReport {
    let mut issues = Vec::new();
    if !matches!(
        session.session_kind,
        HealthSyncSessionKind::Activity | HealthSyncSessionKind::Workout
    ) {
        issues.push("unsupported_activity_session_kind".to_string());
    }
    if session.session_id.trim().is_empty() {
        issues.push("session_id_required".to_string());
    }
    if session.activity_type.trim().is_empty() {
        issues.push("activity_type_required".to_string());
    }
    if session.source_kind.trim().is_empty() {
        issues.push("source_kind_required".to_string());
    }

    let mut metric_plans = Vec::new();
    let mut ignored_metric_names = BTreeSet::new();

    for metric in &session.metrics {
        match packet_derived_activity_metric_plan(session, metric) {
            Some(plan) => metric_plans.push(plan),
            None => {
                ignored_metric_names.insert(metric.name.clone());
            }
        }
    }

    let ignored_metric_names = ignored_metric_names.into_iter().collect::<Vec<_>>();
    let attached_metric_count = metric_plans.len();
    let ignored_metric_count = session.metrics.len().saturating_sub(attached_metric_count);

    ActivitySessionPacketDerivedMetricPlanReport {
        schema: ACTIVITY_SESSION_PACKET_DERIVED_METRIC_PLAN_REPORT_SCHEMA.to_string(),
        generated_by: ACTIVITY_SESSION_PACKET_DERIVED_METRIC_PLAN_GENERATED_BY.to_string(),
        pass: issues.is_empty(),
        session_id: session.session_id.clone(),
        activity_type: session.activity_type.clone(),
        session_kind: session.session_kind,
        source_kind: session.source_kind.clone(),
        metric_count: session.metrics.len(),
        attached_metric_count,
        ignored_metric_count,
        metric_plans,
        ignored_metric_names,
        issues,
    }
}

fn packet_derived_activity_metric_plan(
    session: &ActivitySyncCandidate,
    metric: &ActivitySyncMetric,
) -> Option<ActivitySessionPacketDerivedMetricPlan> {
    let metric_name = normalized_marker(&metric.name);
    let metric_kind = match metric_name.as_str() {
        "load" => "load",
        "strain" => "strain",
        _ => return None,
    };
    if !is_packet_derived_metric(metric) {
        return None;
    }
    if !metric.value.is_finite() {
        return None;
    }

    Some(ActivitySessionPacketDerivedMetricPlan {
        session_id: session.session_id.clone(),
        activity_type: session.activity_type.clone(),
        session_kind: session.session_kind,
        source_kind: session.source_kind.clone(),
        metric_name: metric_kind.to_string(),
        value: metric.value,
        unit: metric.unit.clone(),
        start_time: metric.start_time.clone(),
        end_time: metric.end_time.clone(),
        quality_flags: metric.quality_flags.clone(),
        provenance: metric.provenance.clone(),
    })
}

fn is_packet_derived_metric(metric: &ActivitySyncMetric) -> bool {
    metric.quality_flags.iter().any(|flag| {
        let normalized = normalized_marker(flag);
        normalized == "packet_derived" || normalized.ends_with("_packet_derived")
    }) || metric
        .provenance
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| normalized_marker(source).contains("packet_derived"))
}

fn normalized_marker(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}
