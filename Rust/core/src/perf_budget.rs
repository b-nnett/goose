use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GooseError, GooseResult,
    capture_import::{CapturedFrameBatchOptions, CapturedFrameInput, import_captured_frame_batch},
    export::{RawExportOptions, export_raw_timeframe},
    metrics::{
        HrvInput, RecoveryInput, SleepInput, StrainInput, StressInput, goose_hrv_v0,
        goose_recovery_v0, goose_sleep_v0, goose_strain_v0, goose_stress_v0,
    },
    protocol::{DeviceType, FrameAccumulator, build_v5_payload_frame, parse_frame},
    store::GooseStore,
};

pub const PERF_BUDGET_REPORT_SCHEMA: &str = "goose.perf-budget-report.v1";
pub const DEFAULT_PERF_SCALE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfBudgetOptions {
    pub scale: usize,
    pub budgets: PerfBudgets,
}

impl Default for PerfBudgetOptions {
    fn default() -> Self {
        Self {
            scale: DEFAULT_PERF_SCALE,
            budgets: PerfBudgets::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerfBudgets {
    pub parser_max_duration_ms: u64,
    pub deframer_max_duration_ms: u64,
    pub algorithms_max_duration_ms: u64,
    pub export_max_duration_ms: u64,
    pub parser_max_estimated_peak_bytes: u64,
    pub deframer_max_estimated_peak_bytes: u64,
    pub algorithms_max_estimated_peak_bytes: u64,
    pub export_max_estimated_peak_bytes: u64,
}

impl Default for PerfBudgets {
    fn default() -> Self {
        Self {
            parser_max_duration_ms: 1_500,
            deframer_max_duration_ms: 1_500,
            algorithms_max_duration_ms: 1_500,
            export_max_duration_ms: 5_000,
            parser_max_estimated_peak_bytes: mib(8),
            deframer_max_estimated_peak_bytes: mib(8),
            algorithms_max_estimated_peak_bytes: mib(8),
            export_max_estimated_peak_bytes: mib(64),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfBudgetReport {
    pub schema: String,
    pub generated_by: String,
    pub scale: usize,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub parser_workload_ready: bool,
    #[serde(default)]
    pub deframer_workload_ready: bool,
    #[serde(default)]
    pub score_workload_ready: bool,
    #[serde(default)]
    pub export_workload_ready: bool,
    #[serde(default)]
    pub duration_budget_ready: bool,
    #[serde(default)]
    pub memory_budget_ready: bool,
    #[serde(default)]
    pub correctness_ready: bool,
    #[serde(default)]
    pub all_workloads_ready: bool,
    #[serde(default)]
    pub perf_budget_ready: bool,
    pub budgets: PerfBudgets,
    pub workloads: Vec<PerfWorkloadReport>,
    pub total_duration_ms: u64,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<PerfBudgetNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerfWorkloadReport {
    pub name: String,
    pub pass: bool,
    pub cases: usize,
    pub checks: usize,
    pub duration_ms: u64,
    pub max_duration_ms: u64,
    pub estimated_peak_bytes: u64,
    pub max_estimated_peak_bytes: u64,
    pub bytes_processed: u64,
    pub details: Value,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<PerfBudgetNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PerfBudgetNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

pub fn run_perf_budget(options: PerfBudgetOptions) -> GooseResult<PerfBudgetReport> {
    if options.scale == 0 {
        return Err(GooseError::message("scale must be greater than 0"));
    }

    let suite_started = Instant::now();
    let workloads = vec![
        parser_workload(options.scale, &options.budgets),
        deframer_workload(options.scale, &options.budgets),
        algorithms_workload(options.scale, &options.budgets),
        export_workload(options.scale, &options.budgets)?,
    ];
    let total_duration_ms = duration_ms(suite_started.elapsed());

    Ok(perf_budget_report_from_workloads(
        options.scale,
        options.budgets,
        workloads,
        total_duration_ms,
    ))
}

pub fn perf_budget_report_from_workloads(
    scale: usize,
    budgets: PerfBudgets,
    workloads: Vec<PerfWorkloadReport>,
    total_duration_ms: u64,
) -> PerfBudgetReport {
    let input_valid = scale > 0;
    let parser_workload_ready = perf_workload_ready(&workloads, "parser_frame_batch");
    let deframer_workload_ready = perf_workload_ready(&workloads, "deframer_split_stream");
    let score_workload_ready = perf_workload_ready(&workloads, "goose_score_batch");
    let export_workload_ready = perf_workload_ready(&workloads, "raw_export_bundle");
    let duration_budget_ready = perf_budget_reason_ready(&workloads, "duration_budget_exceeded");
    let memory_budget_ready = perf_budget_reason_ready(&workloads, "memory_budget_exceeded");
    let correctness_ready = workloads.iter().all(|workload| {
        workload.issues.iter().all(|issue| {
            matches!(
                perf_budget_issue_reason(issue),
                "duration_budget_exceeded" | "memory_budget_exceeded"
            )
        })
    });
    let all_workloads_ready = parser_workload_ready
        && deframer_workload_ready
        && score_workload_ready
        && export_workload_ready
        && workloads.iter().all(|workload| workload.pass);
    let issues = workloads
        .iter()
        .filter(|workload| !workload.pass)
        .map(|workload| format!("{} failed budget", workload.name))
        .collect::<Vec<_>>();
    let next_actions = perf_budget_report_next_actions(&workloads);
    let perf_budget_ready = input_valid
        && all_workloads_ready
        && duration_budget_ready
        && memory_budget_ready
        && correctness_ready
        && issues.is_empty();
    let pass = perf_budget_ready;

    PerfBudgetReport {
        schema: PERF_BUDGET_REPORT_SCHEMA.to_string(),
        generated_by: "goose-perf-budget".to_string(),
        scale,
        pass,
        input_valid,
        parser_workload_ready,
        deframer_workload_ready,
        score_workload_ready,
        export_workload_ready,
        duration_budget_ready,
        memory_budget_ready,
        correctness_ready,
        all_workloads_ready,
        perf_budget_ready,
        budgets,
        workloads,
        total_duration_ms,
        issues,
        next_actions,
    }
}

fn perf_workload_ready(workloads: &[PerfWorkloadReport], name: &str) -> bool {
    workloads
        .iter()
        .any(|workload| workload.name == name && workload.pass)
}

fn perf_budget_reason_ready(workloads: &[PerfWorkloadReport], reason: &str) -> bool {
    workloads.iter().all(|workload| {
        workload
            .issues
            .iter()
            .all(|issue| perf_budget_issue_reason(issue) != reason)
    })
}

fn parser_workload(scale: usize, budgets: &PerfBudgets) -> PerfWorkloadReport {
    let mut frames = Vec::with_capacity(scale);
    for index in 0..scale {
        frames.push(build_v5_payload_frame(&synthetic_payload(index)));
    }

    let started = Instant::now();
    let mut checks = 0usize;
    let mut bytes_processed = 0u64;
    let mut parse_failures = 0usize;
    for frame in &frames {
        bytes_processed += frame.len() as u64;
        checks += 1;
        match parse_frame(DeviceType::Goose, frame) {
            Ok(parsed) if parsed.header_crc_valid && parsed.payload_crc_valid => {}
            _ => parse_failures += 1,
        }
    }
    let duration_ms = duration_ms(started.elapsed());
    let estimated_peak_bytes = frames.iter().map(|frame| frame.len() as u64).sum::<u64>();
    finish_workload(WorkloadFinish {
        name: "parser_frame_batch",
        cases: scale,
        checks,
        duration_ms,
        max_duration_ms: budgets.parser_max_duration_ms,
        estimated_peak_bytes,
        max_estimated_peak_bytes: budgets.parser_max_estimated_peak_bytes,
        bytes_processed,
        details: json!({
            "device_type": "GOOSE",
            "parse_failures": parse_failures,
            "payload_family": "mixed_command_event_data_packet"
        }),
        extra_issues: if parse_failures == 0 {
            Vec::new()
        } else {
            vec![format!("{parse_failures} built frames failed to parse")]
        },
    })
}

fn deframer_workload(scale: usize, budgets: &PerfBudgets) -> PerfWorkloadReport {
    let mut stream = Vec::new();
    let mut expected_frames = Vec::new();
    let mut expected_dropped_prefix_bytes = 0usize;
    for index in 0..scale {
        let prefix_len = index % 3;
        expected_dropped_prefix_bytes += prefix_len;
        for byte_index in 0..prefix_len {
            stream.push(1 + ((index + byte_index) % 0xa8) as u8);
        }
        let frame = build_v5_payload_frame(&synthetic_payload(index));
        stream.extend_from_slice(&frame);
        expected_frames.push(frame);
    }

    let started = Instant::now();
    let mut accumulator = FrameAccumulator::new(DeviceType::Goose);
    let mut extracted = Vec::new();
    let mut dropped_prefix_bytes = 0usize;
    for chunk in stream.chunks(7) {
        let result = accumulator.feed(chunk);
        dropped_prefix_bytes += result.dropped_prefix_len;
        extracted.extend(result.frames);
    }
    let duration_ms = duration_ms(started.elapsed());
    let bytes_processed = stream.len() as u64;
    let estimated_peak_bytes = stream.len() as u64
        + extracted
            .iter()
            .map(|frame| frame.len() as u64)
            .sum::<u64>();
    let mut issues = Vec::new();
    if extracted != expected_frames {
        issues.push(format!(
            "expected {} deframed frames, got {}",
            expected_frames.len(),
            extracted.len()
        ));
    }
    if dropped_prefix_bytes != expected_dropped_prefix_bytes {
        issues.push(format!(
            "expected {expected_dropped_prefix_bytes} dropped prefix bytes, got {dropped_prefix_bytes}"
        ));
    }

    finish_workload(WorkloadFinish {
        name: "deframer_split_stream",
        cases: scale,
        checks: scale * 2,
        duration_ms,
        max_duration_ms: budgets.deframer_max_duration_ms,
        estimated_peak_bytes,
        max_estimated_peak_bytes: budgets.deframer_max_estimated_peak_bytes,
        bytes_processed,
        details: json!({
            "chunk_size": 7,
            "extracted_frames": extracted.len(),
            "expected_dropped_prefix_bytes": expected_dropped_prefix_bytes,
            "actual_dropped_prefix_bytes": dropped_prefix_bytes
        }),
        extra_issues: issues,
    })
}

fn algorithms_workload(scale: usize, budgets: &PerfBudgets) -> PerfWorkloadReport {
    let started = Instant::now();
    let mut checks = 0usize;
    let mut output_failures = 0usize;
    let mut bytes_processed = 0u64;

    for index in 0..scale {
        let hrv_input = hrv_input(index);
        bytes_processed += hrv_input.rr_intervals_ms.len() as u64 * 8;
        output_failures += goose_hrv_v0(&hrv_input).output.is_none() as usize;
        checks += 1;

        output_failures += goose_sleep_v0(&sleep_input(index)).output.is_none() as usize;
        output_failures += goose_strain_v0(&strain_input(index)).output.is_none() as usize;
        output_failures += goose_recovery_v0(&recovery_input(index)).output.is_none() as usize;
        output_failures += goose_stress_v0(&stress_input(index)).output.is_none() as usize;
        checks += 4;
    }

    let duration_ms = duration_ms(started.elapsed());
    finish_workload(WorkloadFinish {
        name: "goose_score_batch",
        cases: scale * 5,
        checks,
        duration_ms,
        max_duration_ms: budgets.algorithms_max_duration_ms,
        estimated_peak_bytes: scale as u64 * 64 * 8,
        max_estimated_peak_bytes: budgets.algorithms_max_estimated_peak_bytes,
        bytes_processed,
        details: json!({
            "families": ["hrv", "sleep", "strain", "recovery", "stress"],
            "output_failures": output_failures
        }),
        extra_issues: if output_failures == 0 {
            Vec::new()
        } else {
            vec![format!(
                "{output_failures} generated score inputs produced no output"
            )]
        },
    })
}

fn export_workload(scale: usize, budgets: &PerfBudgets) -> GooseResult<PerfWorkloadReport> {
    let workspace = PerfWorkspace::new()?;
    let db_path = workspace.path.join("goose.sqlite");
    let output_dir = workspace.path.join("export.goosebundle");
    let zip_path = workspace.path.join("export.goosebundle.zip");

    let store = GooseStore::open(&db_path)?;
    let frames = (0..scale)
        .map(|index| CapturedFrameInput {
            evidence_id: format!("perf.raw.{index:04}"),
            frame_id: Some(format!("perf.frame.{index:04}")),
            source: "perf-budget.synthetic".to_string(),
            captured_at: captured_at(index),
            device_model: "WHOOP 5.0 Goose synthetic".to_string(),
            frame_hex: hex::encode(build_v5_payload_frame(&synthetic_payload(index))),
            sensitivity: "synthetic-no-user-data".to_string(),
            capture_session_id: None,
            device_type: DeviceType::Goose,
        })
        .collect::<Vec<_>>();
    let import_report = import_captured_frame_batch(
        &store,
        &frames,
        CapturedFrameBatchOptions {
            parser_version: "goose-core/perf-budget",
            active_device_id: None,
        },
    )?;
    if !import_report.pass {
        return Err(GooseError::message(format!(
            "perf export setup import failed: {:?}",
            import_report.issues
        )));
    }

    let started = Instant::now();
    let export_report = export_raw_timeframe(
        &store,
        RawExportOptions {
            output_dir: &output_dir,
            start: "2026-05-01T00:00:00Z",
            end: "2026-05-29T00:00:00Z",
            app_version: "goose-app/perf-budget",
            core_version: "goose-core/perf-budget",
            data_families: Vec::new(),
            filters: Default::default(),
            sqlite_source_path: Some(&db_path),
            zip_output_path: Some(&zip_path),
        },
    )?;
    let duration_ms = duration_ms(started.elapsed());
    let output_bytes = directory_size(&output_dir)? + file_size(&zip_path)?;
    let db_bytes = file_size(&db_path)?;

    Ok(finish_workload(WorkloadFinish {
        name: "raw_export_bundle",
        cases: scale,
        checks: 3,
        duration_ms,
        max_duration_ms: budgets.export_max_duration_ms,
        estimated_peak_bytes: output_bytes + db_bytes,
        max_estimated_peak_bytes: budgets.export_max_estimated_peak_bytes,
        bytes_processed: output_bytes,
        details: json!({
            "raw_rows": export_report.raw_rows,
            "decoded_frame_rows": export_report.decoded_frame_rows,
            "packet_timeline_rows": export_report.packet_timeline_rows,
            "manifest_file_count": export_report.manifest.files.len(),
            "zip_output": true,
            "output_bytes": output_bytes,
            "database_bytes": db_bytes
        }),
        extra_issues: if export_report.pass
            && export_report.raw_rows == scale
            && export_report.decoded_frame_rows == scale
        {
            Vec::new()
        } else {
            vec![format!(
                "raw export report did not match expected rows: pass={}, raw={}, decoded={}, issues={:?}",
                export_report.pass,
                export_report.raw_rows,
                export_report.decoded_frame_rows,
                export_report.issues
            )]
        },
    }))
}

struct WorkloadFinish<'a> {
    name: &'a str,
    cases: usize,
    checks: usize,
    duration_ms: u64,
    max_duration_ms: u64,
    estimated_peak_bytes: u64,
    max_estimated_peak_bytes: u64,
    bytes_processed: u64,
    details: Value,
    extra_issues: Vec<String>,
}

fn finish_workload(input: WorkloadFinish<'_>) -> PerfWorkloadReport {
    let mut issues = input.extra_issues;
    if input.duration_ms > input.max_duration_ms {
        issues.push(format!(
            "duration {}ms exceeds budget {}ms",
            input.duration_ms, input.max_duration_ms
        ));
    }
    if input.estimated_peak_bytes > input.max_estimated_peak_bytes {
        issues.push(format!(
            "estimated peak {} bytes exceeds budget {} bytes",
            input.estimated_peak_bytes, input.max_estimated_peak_bytes
        ));
    }
    let next_actions = perf_budget_workload_next_actions(input.name, &issues);

    PerfWorkloadReport {
        name: input.name.to_string(),
        pass: issues.is_empty(),
        cases: input.cases,
        checks: input.checks,
        duration_ms: input.duration_ms,
        max_duration_ms: input.max_duration_ms,
        estimated_peak_bytes: input.estimated_peak_bytes,
        max_estimated_peak_bytes: input.max_estimated_peak_bytes,
        bytes_processed: input.bytes_processed,
        details: input.details,
        issues,
        next_actions,
    }
}

fn perf_budget_report_next_actions(workloads: &[PerfWorkloadReport]) -> Vec<PerfBudgetNextAction> {
    dedupe_perf_next_actions(
        workloads
            .iter()
            .flat_map(|workload| workload.next_actions.iter().cloned())
            .collect(),
    )
}

fn perf_budget_workload_next_actions(
    workload_name: &str,
    issues: &[String],
) -> Vec<PerfBudgetNextAction> {
    dedupe_perf_next_actions(
        issues
            .iter()
            .map(|issue| {
                let reason = perf_budget_issue_reason(issue);
                PerfBudgetNextAction {
                    scope: workload_name.to_string(),
                    reason: reason.to_string(),
                    action: perf_budget_action(workload_name, reason).to_string(),
                }
            })
            .collect(),
    )
}

fn perf_budget_issue_reason(issue: &str) -> &'static str {
    if issue.contains("duration") && issue.contains("exceeds budget") {
        "duration_budget_exceeded"
    } else if issue.contains("estimated peak") && issue.contains("exceeds budget") {
        "memory_budget_exceeded"
    } else if issue.contains("built frames failed to parse") {
        "parser_correctness_failure"
    } else if issue.contains("deframed frames") {
        "deframer_frame_mismatch"
    } else if issue.contains("dropped prefix bytes") {
        "deframer_prefix_mismatch"
    } else if issue.contains("generated score inputs produced no output") {
        "algorithm_output_failure"
    } else if issue.contains("raw export report did not match expected rows") {
        "raw_export_row_mismatch"
    } else {
        "workload_issue"
    }
}

fn perf_budget_action(workload_name: &str, reason: &str) -> &'static str {
    match reason {
        "duration_budget_exceeded" => match workload_name {
            "parser_frame_batch" => {
                "Profile parser frame decoding at this scale, then remove repeated allocations or parsing passes before raising the mobile budget."
            }
            "deframer_split_stream" => {
                "Profile deframer chunk handling at this scale, then reduce buffer copies and retained stream state before raising the mobile budget."
            }
            "goose_score_batch" => {
                "Profile Goose score calculations at this scale, then cache shared feature work or simplify repeated math before raising the mobile budget."
            }
            "raw_export_bundle" => {
                "Profile raw export writes at this scale, then stream rows and zip output instead of buffering before raising the mobile budget."
            }
            _ => {
                "Profile the workload at this scale and reduce repeated work before raising the mobile budget."
            }
        },
        "memory_budget_exceeded" => match workload_name {
            "parser_frame_batch" => {
                "Reduce parser frame retention or reuse buffers so batch parsing stays inside the mobile memory budget."
            }
            "deframer_split_stream" => {
                "Reduce deframer buffer growth and copied frame retention so streaming capture stays inside the mobile memory budget."
            }
            "goose_score_batch" => {
                "Reduce temporary vectors in Goose score inputs/outputs or process windows incrementally before raising the mobile memory budget."
            }
            "raw_export_bundle" => {
                "Stream export rows, SQLite reads, and zip writes so raw timeframe export stays inside the mobile memory budget."
            }
            _ => {
                "Reduce retained buffers or process the workload incrementally before raising the mobile memory budget."
            }
        },
        "parser_correctness_failure" => {
            "Fix the local frame builder/parser invariant and add a regression fixture before trusting parser performance numbers."
        }
        "deframer_frame_mismatch" | "deframer_prefix_mismatch" => {
            "Fix deframer correctness against split-stream fixtures before using this performance report for mobile readiness."
        }
        "algorithm_output_failure" => {
            "Fix Goose score input generation or quality gates so generated valid inputs produce score outputs before benchmarking."
        }
        "raw_export_row_mismatch" => {
            "Fix raw export row accounting or report generation before treating export performance as mobile-ready."
        }
        _ => {
            "Inspect the workload issue and add a targeted regression before treating this performance report as trusted."
        }
    }
}

fn dedupe_perf_next_actions(actions: Vec<PerfBudgetNextAction>) -> Vec<PerfBudgetNextAction> {
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

fn synthetic_payload(index: usize) -> Vec<u8> {
    match index % 4 {
        0 => vec![35, (index % 255) as u8, 145, 1],
        1 => {
            let mut payload = vec![48, (index % 255) as u8, 17, 0];
            payload.extend_from_slice(&(index as u32).to_le_bytes());
            payload.extend_from_slice(&(index as u16).to_le_bytes());
            payload.extend_from_slice(&[0, 0, 0xde, 0xad, 0xbe, 0xef]);
            payload
        }
        2 => {
            let mut payload = vec![47, 18, 1];
            payload.extend_from_slice(&(index as u32).to_le_bytes());
            payload.extend_from_slice(&(0x1122_3344u32 + index as u32).to_le_bytes());
            payload.extend_from_slice(&(index as u16).to_le_bytes());
            payload.extend_from_slice(&[0xaa, 0x4d, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
            payload
        }
        _ => {
            let mut payload = vec![43, 10, 1];
            payload.extend_from_slice(&(index as u32).to_le_bytes());
            payload.extend_from_slice(&(0x5566_7788u32 + index as u32).to_le_bytes());
            payload.extend_from_slice(&(index as u16).to_le_bytes());
            payload.resize(96, 0);
            payload[17] = 72;
            for sample_index in 0..5 {
                let value = (sample_index as i16) - 2;
                let offset = 85 + sample_index * 2;
                payload[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
            payload
        }
    }
}

fn hrv_input(index: usize) -> HrvInput {
    HrvInput {
        start_time: "2026-05-28T00:00:00Z".to_string(),
        end_time: "2026-05-28T00:05:00Z".to_string(),
        rr_intervals_ms: (0..64)
            .map(|offset| 780.0 + ((index + offset) % 40) as f64)
            .collect(),
        input_ids: Vec::new(),
    }
}

fn sleep_input(index: usize) -> SleepInput {
    SleepInput {
        start_time: "2026-05-27T22:30:00Z".to_string(),
        end_time: "2026-05-28T06:30:00Z".to_string(),
        sleep_duration_minutes: 390.0 + (index % 60) as f64,
        sleep_need_minutes: 480.0,
        time_in_bed_minutes: 500.0,
        midpoint_deviation_minutes: (index % 90) as f64,
        disturbance_count: (index % 8) as u32,
        input_ids: Vec::new(),
        ..Default::default()
    }
}

fn strain_input(index: usize) -> StrainInput {
    StrainInput {
        start_time: "2026-05-28T12:00:00Z".to_string(),
        end_time: "2026-05-28T13:00:00Z".to_string(),
        duration_minutes: 60.0,
        resting_hr_bpm: 58.0,
        average_hr_bpm: 105.0 + (index % 30) as f64,
        max_hr_bpm: 185.0,
        hr_zone_minutes: vec![
            20.0,
            15.0,
            10.0 + (index % 5) as f64,
            10.0,
            5.0 - (index % 5) as f64,
        ],
        input_ids: Vec::new(),
    }
}

fn recovery_input(index: usize) -> RecoveryInput {
    RecoveryInput {
        start_time: "2026-05-28T06:00:00Z".to_string(),
        end_time: "2026-05-28T06:05:00Z".to_string(),
        hrv_rmssd_ms: 45.0 + (index % 20) as f64,
        hrv_baseline_rmssd_ms: 50.0,
        resting_hr_bpm: 56.0 + (index % 8) as f64,
        resting_hr_baseline_bpm: 58.0,
        respiratory_rate_rpm: 14.0,
        respiratory_rate_baseline_rpm: 14.0,
        skin_temp_delta_c: ((index % 5) as f64 - 2.0) * 0.1,
        sleep_score_0_to_100: 75.0,
        prior_strain_0_to_21: 8.0,
        input_ids: Vec::new(),
    }
}

fn stress_input(index: usize) -> StressInput {
    StressInput {
        start_time: "2026-05-28T12:00:00Z".to_string(),
        end_time: "2026-05-28T12:05:00Z".to_string(),
        heart_rate_bpm: 85.0 + (index % 40) as f64,
        resting_hr_bpm: 58.0,
        hrv_rmssd_ms: 35.0 + (index % 10) as f64,
        hrv_baseline_rmssd_ms: 50.0,
        motion_intensity_0_to_1: (index % 10) as f64 / 10.0,
        input_ids: Vec::new(),
    }
}

fn captured_at(index: usize) -> String {
    let day = 1 + (index / 86_400) % 20;
    let second_of_day = index % 86_400;
    let hour = second_of_day / 3_600;
    let minute = (second_of_day / 60) % 60;
    let second = second_of_day % 60;
    format!("2026-05-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn directory_size(path: &Path) -> GooseResult<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|source| GooseError::io(path, source))? {
        let entry = entry.map_err(|source| GooseError::io(path, source))?;
        let entry_path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|source| GooseError::io(&entry_path, source))?;
        if metadata.is_dir() {
            total += directory_size(&entry_path)?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn file_size(path: &Path) -> GooseResult<u64> {
    Ok(fs::metadata(path)
        .map_err(|source| GooseError::io(path, source))?
        .len())
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

const fn mib(value: u64) -> u64 {
    value * 1024 * 1024
}

struct PerfWorkspace {
    path: PathBuf,
}

impl PerfWorkspace {
    fn new() -> GooseResult<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                GooseError::message(format!("system clock before unix epoch: {error}"))
            })?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("goose-perf-budget-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).map_err(|source| GooseError::io(&path, source))?;
        Ok(Self { path })
    }
}

impl Drop for PerfWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
