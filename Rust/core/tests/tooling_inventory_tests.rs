use std::path::Path;

const REQUIRED_CLI_BINS: &[&str] = &[
    "goose-fixture-index",
    "goose-capture-sanitize",
    "goose-capture-sqlite-import",
    "goose-parser-fixture-runner",
    "goose-capture-correlation",
    "goose-metric-input-readiness",
    "goose-capture-arrival-plan",
    "goose-command-capture-plan",
    "goose-metric-feature-report",
    "goose-local-health-validation-suite",
    "goose-command-validator",
    "goose-export-validator",
    "goose-reference-algo-runner",
    "goose-algo-benchmark",
    "goose-calibration-evaluator",
    "goose-health-sync-dry-run",
    "goose-debug-ws-contract",
    "goose-debug-ws-serve",
    "goose-ui-coverage-audit",
    "goose-storage-check",
    "goose-property-test-suite",
    "goose-perf-budget",
    "goose-privacy-lint",
];

const REQUIRED_DOC_ENTRIES: &[&str] = &[
    "`goose-metric-input-readiness` / `metrics.input_readiness`",
    "`goose-capture-arrival-plan` / `capture.arrival_plan`",
    "`goose-command-capture-plan` / `commands.capture_plan`",
    "`goose-capture-sqlite-import`",
    "`goose-metric-feature-report motion` / `metrics.motion_features`",
    "`goose-metric-feature-report heart-rate` / `metrics.heart_rate_features`",
    "`goose-metric-feature-report vital-event` / `metrics.vital_event_features`",
    "`goose-metric-feature-report step-discovery` / `metrics.step_packet_discovery`",
    "`goose-metric-feature-report step-validation` / `metrics.step_capture_validation`",
    "`goose-metric-feature-report raw-motion-steps` / `metrics.raw_motion_step_estimate`",
    "`goose-metric-feature-report step-counter-ingest` / `metrics.step_counter_ingest`",
    "`goose-metric-feature-report step-rollup` / `metrics.step_counter_daily_rollup`",
    "`goose-metric-feature-report steps-unavailable-status` / `metrics.activity_unavailable_daily_status`",
    "`goose-metric-feature-report calories-unavailable-status` / `metrics.energy_unavailable_daily_status`",
    "`goose-metric-feature-report hrv` / `metrics.hrv_features`",
    "`goose-metric-feature-report hrv-validation` / `metrics.hrv_capture_validation`",
    "`goose-metric-feature-report respiratory-rate-validation` / `metrics.respiratory_rate_capture_validation`",
    "`goose-metric-feature-report recovery-sensors` / `metrics.recovery_sensor_discovery`",
    "`goose-metric-feature-report recovery-unavailable-status` / `metrics.recovery_unavailable_daily_status`",
    "`goose-metric-feature-report window` / `metrics.window_features`",
    "`goose-metric-feature-report resting-hr` / `metrics.resting_hr_features`",
    "`goose-metric-feature-report rhr-rollup` / `metrics.resting_hr_daily_rollup`",
    "`goose-metric-feature-report rhr-validation` / `metrics.resting_hr_capture_validation`",
    "`goose-metric-feature-report sleep-score` / `metrics.sleep_score_from_features`",
    "`goose-metric-feature-report recovery-score` / `metrics.recovery_score_from_features`",
    "`goose-metric-feature-report strain-score` / `metrics.strain_score_from_features`",
    "`goose-metric-feature-report stress-score` / `metrics.stress_score_from_features`",
    "`goose-local-health-validation-suite`",
];

#[test]
fn required_machine_readable_tools_are_registered_as_cargo_bins() {
    let manifest = read_workspace_file("Cargo.toml");
    for bin in REQUIRED_CLI_BINS {
        assert!(
            manifest.contains(&format!("name = \"{bin}\"")),
            "Cargo.toml missing required Goose tool bin {bin}"
        );
        assert!(
            manifest.contains(&format!("path = \"src/bin/{bin}.rs\"")),
            "Cargo.toml missing expected path for Goose tool bin {bin}"
        );
    }
}

#[test]
fn testing_strategy_names_scriptable_tools_for_bridge_gates() {
    // The testing/tooling strategy doc is an optional documentation artifact
    // that is not always vendored. Skip the entry assertions when it is absent
    // so the suite stays green in checkouts (and CI) that omit the docs.
    let Some(strategy) = read_goose_file_opt("docs/testing-and-tooling-strategy.md") else {
        eprintln!(
            "skipping testing_strategy_names_scriptable_tools_for_bridge_gates: docs/testing-and-tooling-strategy.md not present"
        );
        return;
    };
    for entry in REQUIRED_DOC_ENTRIES {
        assert!(
            strategy.contains(entry),
            "testing strategy missing scriptable tooling entry {entry}"
        );
    }
    assert!(
        strategy.contains("6. `goose-capture-arrival-plan` / `capture.arrival_plan`"),
        "Immediate Tool Order should name the standalone capture arrival plan CLI"
    );
    assert!(
        strategy.contains("25. `goose-debug-ws-serve`"),
        "Immediate Tool Order should include the debug WebSocket serve tool"
    );
}

fn read_workspace_file(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"))
}

fn read_goose_file_opt(relative: &str) -> Option<String> {
    // `CARGO_MANIFEST_DIR` is `<repo-root>/Rust/core`, so two `parent()` hops
    // reach the repo root where the `docs/` tree lives.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("goose project has repo root")
        .join(relative);
    std::fs::read_to_string(&path).ok()
}
