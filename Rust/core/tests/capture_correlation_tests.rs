use std::{collections::BTreeSet, fs, path::Path};

use goose_core::{
    capture_correlation::run_capture_correlation_for_store,
    capture_correlation::{CaptureCorrelationOptions, run_capture_correlation},
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    fixtures::build_fixture_index,
    protocol::DeviceType,
    store::GooseStore,
};

const K10_FRAME: &str = "aa0164000001fb212b0a010000000000000000000000000000480000000000000000000000000000 00000000000000000000000000000000000000000000000000000000000000000000000000000000 000000000000000000000000000100feff0300000000000068cc8271";

#[test]
fn correlation_report_promotes_distinct_owned_history_and_motion_evidence() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    let report = run_capture_correlation(
        root,
        &index,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 2,
            require_owned_captures: false,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.observations.len() >= 13);
    let raw_motion = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "raw_motion_k10")
        .unwrap();
    assert_eq!(raw_motion.observation_count, 4);
    assert_eq!(raw_motion.owned_capture_count, 2);
    assert_eq!(raw_motion.synthetic_count, 2);
    assert_eq!(raw_motion.warning_count, 18);
    assert!(raw_motion.trusted_metric_ready);
    assert!(raw_motion.blocker_reasons.is_empty());
    assert!(raw_motion.next_capture_actions.is_empty());

    let k21_motion = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "raw_motion_k21")
        .unwrap();
    assert_eq!(k21_motion.observation_count, 3);
    assert_eq!(k21_motion.owned_capture_count, 2);
    assert_eq!(k21_motion.synthetic_count, 1);
    assert_eq!(k21_motion.warning_count, 5);
    assert!(k21_motion.trusted_metric_ready);
    assert!(k21_motion.blocker_reasons.is_empty());
    assert!(k21_motion.next_capture_actions.is_empty());

    // Phase 27 reclassified packet_k==24 frames from NormalHistory → V24History.
    // Phase 67 reclassified packet_k==18 frames from NormalHistory → V18History.
    // The synthetic K18 fixture is now v18_history (1 observation, 0 owned, 1 synthetic).
    let v18_history = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "v18_history")
        .unwrap();
    assert_eq!(v18_history.observation_count, 1);
    assert_eq!(v18_history.owned_capture_count, 0);
    assert_eq!(v18_history.synthetic_count, 1);
    assert!(!v18_history.trusted_metric_ready); // 0 owned < 2 required

    // V24History captures (2 user-owned K24 packets from Phase 27)
    let v24_history = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "v24_history")
        .unwrap();
    assert_eq!(v24_history.observation_count, 2);
    assert_eq!(v24_history.owned_capture_count, 2);
    assert_eq!(v24_history.synthetic_count, 0);
    assert!(v24_history.trusted_metric_ready); // 2 owned >= 2 required

    let temperature = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "event_temperature_level")
        .unwrap();
    assert_eq!(temperature.observation_count, 2);
    assert_eq!(temperature.owned_capture_count, 1);
    assert_eq!(temperature.synthetic_count, 1);
    assert_eq!(temperature.warning_count, 0);
    assert!(!temperature.trusted_metric_ready);
    assert!(
        temperature
            .blocker_reasons
            .iter()
            .any(|reason| reason == "owned_capture_count 1 below required 2")
    );
    assert!(temperature.next_capture_actions.iter().any(|action| {
        action.scope == "event_temperature_level"
            && action.action.contains("Capture 1 more user-owned")
            && action.action.contains("TEMPERATURE_LEVEL event 17")
    }));

    assert_eq!(
        owned_sources_for(&report, "raw_motion_k10"),
        BTreeSet::from([
            "android_btsnoop_full_snoop_history_complete_20260528T1748Z".to_string(),
            "android_btsnoop_live_identity_check_20260528T2002Z".to_string(),
        ])
    );
    assert_eq!(
        owned_sources_for(&report, "raw_motion_k21"),
        BTreeSet::from([
            "android_btsnoop_full_snoop_history_complete_20260528T1748Z".to_string(),
            "android_btsnoop_live_identity_check_20260528T2002Z".to_string(),
        ])
    );
    // K24 owned sources moved to v24_history; normal_history now has no owned sources
    assert_eq!(
        owned_sources_for(&report, "normal_history"),
        BTreeSet::new()
    );
    assert_eq!(
        owned_sources_for(&report, "v24_history"),
        BTreeSet::from([
            "android_btsnoop_full_snoop_history_complete_20260528T1748Z".to_string(),
            "android_btsnoop_live_identity_check_20260528T2002Z".to_string(),
        ])
    );

    assert!(
        report
            .next_capture_actions
            .iter()
            .all(|action| action.scope != "raw_motion_k10"
                && action.scope != "raw_motion_k21"
                && action.scope != "v24_history"),
        "{:?}",
        report.next_capture_actions
    );
    assert!(
        report.next_capture_actions.iter().any(|action| {
            action.scope == "r17_optical_or_labrador_filtered"
                && action.action.contains("Capture 2 more user-owned")
                && action
                    .action
                    .contains("official optical/ECG raw-stream session")
        }),
        "{:?}",
        report.next_capture_actions
    );
    assert!(
        report.next_capture_actions.iter().any(|action| {
            action.scope == "event_temperature_level"
                && action.action.contains("Capture 1 more user-owned")
        }),
        "{:?}",
        report.next_capture_actions
    );
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "synthetic.sanitized.corebluetooth.k10_motion"
            && observation.body_summary_kind == "raw_motion_k10"
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "synthetic.goose.v5.temperature_event"
            && observation.body_summary_kind == "event_temperature_level"
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.history_complete.temperature_level_event_payload"
            && observation.body_summary_kind == "event_temperature_level"
            && observation.owned_capture
            && observation.warning_count == 0
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.live_identity.k24_normal_history_payload"
            && observation.body_summary_kind == "v24_history" // reclassified by Phase 27
            && observation.owned_capture
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.live_identity.k10_motion_payload"
            && observation.body_summary_kind == "raw_motion_k10"
            && observation.owned_capture
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.live_identity.k21_motion_payload"
            && observation.body_summary_kind == "raw_motion_k21"
            && observation.owned_capture
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.history_complete.k24_normal_history_payload"
            && observation.body_summary_kind == "v24_history" // reclassified by Phase 27
            && observation.owned_capture
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.history_complete.k10_motion_payload"
            && observation.body_summary_kind == "raw_motion_k10"
            && observation.owned_capture
            && observation.warning_count == 0
    }));
    assert!(report.observations.iter().any(|observation| {
        observation.evidence_id == "owned.history_complete.k21_motion_payload"
            && observation.body_summary_kind == "raw_motion_k21"
            && observation.owned_capture
            && observation.warning_count == 0
    }));
}

fn owned_sources_for(
    report: &goose_core::capture_correlation::CaptureCorrelationReport,
    body_summary_kind: &str,
) -> BTreeSet<String> {
    report
        .observations
        .iter()
        .filter(|observation| {
            observation.body_summary_kind == body_summary_kind && observation.owned_capture
        })
        .map(|observation| observation.source.clone())
        .collect()
}

#[test]
fn correlation_report_can_fail_when_owned_capture_evidence_is_required() {
    let root = Path::new("fixtures");
    let index = build_fixture_index(root).unwrap();

    let report = run_capture_correlation(
        root,
        &index,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: true,
        },
    );

    assert!(!report.pass);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("r17_optical_or_labrador_filtered is not trusted"))
    );
}

#[test]
fn owned_sanitized_capture_can_satisfy_metric_promotion_gate() {
    let tempdir = tempfile::tempdir().unwrap();
    fs::write(
        tempdir.path().join("owned_batch.json"),
        format!(
            r#"{{
  "schema": "goose.captured-frame-batch.v1",
  "frames": [
    {{
      "evidence_id": "owned.capture.k10",
      "frame_id": "owned.capture.k10.frame.0",
      "source": "ios.corebluetooth.notification",
      "captured_at": "2026-05-27T00:00:00Z",
      "device_model": "WHOOP 5.0 Goose",
      "frame_hex": "{K10_FRAME}",
      "sensitivity": "user-owned-capture",
      "device_type": "GOOSE"
    }}
  ]
}}"#
        ),
    )
    .unwrap();
    fs::write(
        tempdir.path().join("owned_batch.fixture.json"),
        r#"{
  "id": "owned.capture.batch",
  "path": "owned_batch.json",
  "kind": "sanitized_capture",
  "source": "owned_corebluetooth_capture",
  "captured_at": "2026-05-27T00:00:00Z",
  "device_model": "WHOOP 5.0 Goose",
  "device_firmware": "owned",
  "app_version": "goose-fixture",
  "schema": "goose.captured-frame-batch.v1",
  "consent": "user-owned-capture",
  "sensitivity": "user-owned-capture",
  "notes": "Owned sanitized capture fixture used to prove the correlation trust gate."
}
"#,
    )
    .unwrap();
    let index = build_fixture_index(tempdir.path()).unwrap();

    let report = run_capture_correlation(
        tempdir.path(),
        &index,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: true,
        },
    );

    assert!(report.pass, "{:?}", report.issues);
    let raw_motion = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "raw_motion_k10")
        .unwrap();
    assert_eq!(raw_motion.observation_count, 1);
    assert_eq!(raw_motion.owned_capture_count, 1);
    assert_eq!(raw_motion.synthetic_count, 0);
    assert!(raw_motion.trusted_metric_ready);
    assert!(raw_motion.blocker_reasons.is_empty());
    assert!(raw_motion.next_capture_actions.is_empty());
    assert!(report.next_capture_actions.is_empty());
}

#[test]
fn correlation_report_gives_next_action_when_no_summaries_exist() {
    let tempdir = tempfile::tempdir().unwrap();
    let index = build_fixture_index(tempdir.path()).unwrap();

    let report = run_capture_correlation(
        tempdir.path(),
        &index,
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: true,
        },
    );

    assert!(!report.pass);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue == "no packet/event summaries found for capture correlation")
    );
    assert!(
        report.next_capture_actions.iter().any(|action| {
            action.scope == "capture_correlation"
                && action.action.contains("Import owned WHOOP frames")
        }),
        "{:?}",
        report.next_capture_actions
    );
}

#[test]
fn store_correlation_counts_owned_app_imports() {
    let store = GooseStore::open_in_memory().unwrap();
    let frames = vec![CapturedFrameInput {
        evidence_id: "app.owned.k10".to_string(),
        frame_id: Some("app.owned.k10.frame.0".to_string()),
        source: "ios.corebluetooth.notification".to_string(),
        captured_at: "2026-05-27T00:00:00Z".to_string(),
        device_model: "WHOOP 5.0 Goose".to_string(),
        frame_hex: K10_FRAME.to_string(),
        sensitivity: "user-owned-live-notification".to_string(),
        capture_session_id: None,
        device_type: DeviceType::Goose,
        device_uuid: None,
    }];
    let import = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/test",
            active_device_id: None,
        },
    )
    .unwrap();
    assert!(import.pass, "{:?}", import.issues);

    let report = run_capture_correlation_for_store(
        &store,
        "test-db",
        "2026-05-27T00:00:00Z",
        "2026-05-28T00:00:00Z",
        CaptureCorrelationOptions {
            min_owned_captures_per_summary: 1,
            require_owned_captures: true,
        },
    )
    .unwrap();

    assert!(report.pass, "{:?}", report.issues);
    let raw_motion = report
        .summaries
        .iter()
        .find(|summary| summary.body_summary_kind == "raw_motion_k10")
        .unwrap();
    assert_eq!(raw_motion.owned_capture_count, 1);
    assert!(raw_motion.trusted_metric_ready);
}
