use goose_core::commands::{
    COMMAND_DEFINITIONS, CommandDefinition, CommandDirectSendPreflightInput,
    CommandEmulatorLogEvidenceOptions, CommandEvidence, CommandLocalFrameCandidate,
    CommandRiskGate, command_capture_plan_from_results, command_evidence_from_emulator_log,
    command_evidence_from_emulator_log_text, command_evidence_template,
    command_evidence_with_local_frame_matches, direct_send_gate_from_result,
    direct_send_preflight_from_gate, load_command_evidence, load_command_local_frame_candidates,
    validate_commands,
};
use goose_core::protocol::{
    COMMAND_GET_HELLO, DeviceType, PACKET_TYPE_COMMAND_RESPONSE, ParsedPayload,
    build_v5_command_frame, build_v5_payload_frame, parse_frame_hex,
};

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";
const COMMAND_SERVICE_UUID: &str = "61080001-0000-1000-8000-00805f9b34fb";
const COMMAND_CHARACTERISTIC_UUID: &str = "61080002-0000-1000-8000-00805f9b34fb";
const COMMAND_WRITE_TYPE: &str = "with_response";
const TRUSTED_COMMAND_EVIDENCE_SOURCE: &str = "user_owned_official_capture";
const TRUSTED_COMMAND_PROVENANCE_JSON: &str =
    r#"{"capture_app":"whoop_official","capture_kind":"passive_ble_observation","owner":"user"}"#;

#[test]
fn command_template_covers_direct_control_families() {
    let template = command_evidence_template();
    let commands: Vec<_> = template
        .evidence
        .iter()
        .map(|evidence| evidence.command.as_str())
        .collect();

    for expected in [
        "set_clock",
        "get_clock",
        "send_historical_data",
        "abort_historical_transmits",
        "enter_high_freq_sync",
        "exit_high_freq_sync",
        "get_alarm_time",
        "run_alarm",
        "get_all_haptics_pattern",
        "select_wrist",
        "get_body_location_and_status",
        "toggle_realtime_hr",
        "start_raw_data",
        "toggle_imu_mode_historical",
        "set_dp_type",
        "force_dp_type",
        "set_device_config_value",
        "start_device_config_key_exchange",
        "set_feature_flag_value",
        "start_feature_flag_key_exchange",
        "verify_firmware_image",
        "start_firmware_load_new",
        "start_firmware_load",
        "reboot_strap",
        "force_trim",
        "toggle_persistent_r20",
    ] {
        assert!(commands.contains(&expected), "missing {expected}");
    }
}

#[test]
fn command_definitions_cover_apk_static_reference_rows_with_expected_gates() {
    let by_id: std::collections::BTreeMap<_, _> = COMMAND_DEFINITIONS
        .iter()
        .map(|definition| (definition.id, definition))
        .collect();
    let ids: std::collections::BTreeSet<_> = COMMAND_DEFINITIONS
        .iter()
        .map(|definition| definition.id)
        .collect();
    assert_eq!(
        ids.len(),
        COMMAND_DEFINITIONS.len(),
        "command ids must stay unique"
    );

    for (id, number, family, risk_gate) in [
        ("get_clock", 11, "clock_sync", CommandRiskGate::ReadOnly),
        (
            "set_clock",
            10,
            "clock_sync",
            CommandRiskGate::UserVisibleStateChange,
        ),
        (
            "get_alarm_time",
            67,
            "alarm_haptics",
            CommandRiskGate::ReadOnly,
        ),
        (
            "enter_high_freq_sync",
            96,
            "historical_sync",
            CommandRiskGate::UserVisibleStateChange,
        ),
        (
            "start_device_config_key_exchange",
            115,
            "device_config",
            CommandRiskGate::CriticalStateChange,
        ),
        (
            "send_next_feature_flag",
            118,
            "feature_flags",
            CommandRiskGate::CriticalStateChange,
        ),
        (
            "verify_firmware_image",
            83,
            "firmware_dfu",
            CommandRiskGate::CriticalStateChange,
        ),
        (
            "set_dp_type",
            52,
            "data_packet_config",
            CommandRiskGate::CriticalStateChange,
        ),
        (
            "force_dp_type",
            53,
            "data_packet_config",
            CommandRiskGate::CriticalStateChange,
        ),
        (
            "toggle_persistent_r21",
            154,
            "persistent_sensor_config",
            CommandRiskGate::CriticalStateChange,
        ),
    ] {
        let definition = by_id.get(id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(definition.command_number, Some(number), "{id}");
        assert_eq!(definition.family, family, "{id}");
        assert_eq!(definition.risk_gate, risk_gate, "{id}");
    }
}

#[test]
fn command_definitions_cover_generated_protocol_command_map_ids() {
    // The generated protocol command map is produced by the documentation
    // pipeline and is not committed to this repository. Read it at runtime and
    // skip the coverage assertions when it is absent so the suite stays green in
    // checkouts (and CI) that do not vendor the generated docs.
    let generated_protocol_map_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/generated/protocol-command-map.md");
    let Ok(generated_protocol_map) = std::fs::read_to_string(&generated_protocol_map_path) else {
        eprintln!(
            "skipping command_definitions_cover_generated_protocol_command_map_ids: {} not present",
            generated_protocol_map_path.display()
        );
        return;
    };
    let generated_ids: std::collections::BTreeSet<u16> = generated_protocol_map
        .lines()
        .filter_map(|line| {
            let columns: Vec<_> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            columns.first().and_then(|id| id.parse::<u16>().ok())
        })
        .collect();
    assert!(
        !generated_ids.is_empty(),
        "generated protocol command map parsed no command ids"
    );

    let goose_ids: std::collections::BTreeSet<u16> = COMMAND_DEFINITIONS
        .iter()
        .filter_map(|definition| definition.command_number)
        .collect();
    let missing: Vec<_> = generated_ids.difference(&goose_ids).copied().collect();
    assert!(
        missing.is_empty(),
        "missing generated protocol command ids: {missing:?}"
    );
}

#[test]
fn load_command_evidence_accepts_exported_top_level_json_report() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("goose-command-evidence.json");
    let payload = serde_json::json!({
        "schema": "goose.command-evidence.v1",
        "generated_by": "whoop-reversing.goose_command_evidence",
        "source_capture": "captures/android/whoop-ble.jsonl",
        "evidence_count": 1,
        "evidence": [ready_get_hello_evidence()],
    });
    std::fs::write(&path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();

    let evidence = load_command_evidence(&path).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].command, "get_hello");

    let report = validate_commands(&evidence);
    let get_hello = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        get_hello.direct_send_ready,
        "{:?}",
        get_hello.missing_requirements
    );
}

#[test]
fn official_app_emulator_fixture_promotes_validated_shortcut_commands() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/command-evidence/whoop-emulator-command-evidence.json");
    if !path.exists() {
        eprintln!(
            "skipping official_app_emulator_fixture_promotes_validated_shortcut_commands: {} not present",
            path.display()
        );
        return;
    }
    let evidence = load_command_evidence(&path).unwrap();
    assert_eq!(evidence.len(), 20);
    assert!(evidence.iter().any(|row| {
        row.provenance_json
            .as_deref()
            .unwrap_or("")
            .contains(r#""capture_kind":"official_app_to_macos_emulator""#)
    }));

    let report = validate_commands(&evidence);
    assert_eq!(report.direct_send_ready_count, 13);
    for command in [
        "toggle_realtime_hr",
        "abort_historical_transmits",
        "get_data_range",
        "send_historical_data",
        "historical_data_result",
        "set_alarm_time",
        "disable_alarm",
        "enter_high_freq_sync",
        "exit_high_freq_sync",
        "toggle_imu_mode",
    ] {
        let result = report
            .commands
            .iter()
            .find(|result| result.command == command)
            .unwrap();
        assert!(
            result.direct_send_ready,
            "{command}: {:?}",
            result.missing_requirements
        );
    }

    let haptic = report
        .commands
        .iter()
        .find(|result| result.command == "run_haptic_pattern_maverick")
        .unwrap();
    assert!(!haptic.direct_send_ready);
    assert!(
        haptic
            .missing_requirements
            .contains(&"response_parser".to_string())
    );
    assert!(
        haptic
            .missing_requirements
            .contains(&"timeout_behavior".to_string())
    );
    assert!(haptic.next_capture_actions.iter().any(|action| {
        action.requirement == "response_parser" && action.action.contains("success response parser")
    }));
    assert!(haptic.next_capture_actions.iter().any(|action| {
        action.requirement == "timeout_behavior" && action.action.contains("timeout/retry behavior")
    }));

    for command in ["set_feature_flag_value", "start_firmware_load_new"] {
        let result = report
            .commands
            .iter()
            .find(|result| result.command == command)
            .unwrap();
        assert!(!result.direct_send_ready);
        assert!(
            result.validated_local_frame_hex.is_some(),
            "{command} should reproduce local bytes without bypassing critical gates"
        );
        assert!(
            !result
                .missing_requirements
                .contains(&"local_frame_matches_official_frame".to_string()),
            "{command}: {:?}",
            result.missing_requirements
        );
        assert!(
            result
                .next_capture_actions
                .iter()
                .any(|action| action.requirement == "explicit_approval"
                    && action
                        .action
                        .contains("short-lived explicit runtime approval")),
            "{command}: {:?}",
            result.next_capture_actions
        );
    }

    let get_data_range_frame = hex::encode(build_v5_command_frame(7, 34, &[]));
    let get_data_range = evidence
        .iter()
        .find(|row| row.command == "get_data_range")
        .unwrap();
    assert_eq!(
        get_data_range.official_frame_hex.as_deref(),
        Some(get_data_range_frame.as_str())
    );
    assert_eq!(
        get_data_range.local_frame_hex.as_deref(),
        Some(get_data_range_frame.as_str())
    );

    let send_historical_data_frame = hex::encode(build_v5_command_frame(8, 22, &[]));
    let send_historical_data = evidence
        .iter()
        .find(|row| row.command == "send_historical_data")
        .unwrap();
    assert_eq!(
        send_historical_data.official_frame_hex.as_deref(),
        Some(send_historical_data_frame.as_str())
    );
    assert_eq!(
        send_historical_data.local_frame_hex.as_deref(),
        Some(send_historical_data_frame.as_str())
    );

    let historical_data_result_frame = hex::encode(build_v5_command_frame(9, 23, &[0, 0, 0, 0]));
    let historical_data_result = evidence
        .iter()
        .find(|row| row.command == "historical_data_result")
        .unwrap();
    assert_eq!(
        historical_data_result.official_frame_hex.as_deref(),
        Some(historical_data_result_frame.as_str())
    );
    assert_eq!(
        historical_data_result.local_frame_hex.as_deref(),
        Some(historical_data_result_frame.as_str())
    );
    let parsed_failure_ack = parse_frame_hex(
        DeviceType::Goose,
        historical_data_result
            .official_response_frame_hex
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert!(parsed_failure_ack.header_crc_valid);
    assert!(parsed_failure_ack.payload_crc_valid);
    match parsed_failure_ack.parsed_payload {
        Some(ParsedPayload::CommandResponse {
            response_to_command,
            result_code,
            ..
        }) => {
            assert_eq!(response_to_command, Some(23));
            assert_eq!(result_code, Some(1));
        }
        other => panic!("expected historical_data_result failure ack, got {other:?}"),
    }
}

#[test]
fn command_validation_pass_requires_every_direct_send_gate_ready() {
    let report = validate_commands(&[ready_get_hello_evidence()]);

    assert!(report.evidence_valid, "{:?}", report.issues);
    assert!(!report.all_direct_sends_ready);
    assert!(!report.pass);
    assert_eq!(report.direct_send_ready_count, 1);
    assert_eq!(report.blocked_count, COMMAND_DEFINITIONS.len() - 1);
}

#[test]
fn command_validation_passes_when_all_command_gates_are_ready() {
    let evidence = COMMAND_DEFINITIONS
        .iter()
        .map(ready_command_evidence_for_definition)
        .collect::<Vec<_>>();

    let report = validate_commands(&evidence);

    assert!(report.evidence_valid, "{:?}", report.issues);
    assert!(report.all_direct_sends_ready);
    assert!(report.pass);
    assert_eq!(report.direct_send_ready_count, COMMAND_DEFINITIONS.len());
    assert_eq!(report.blocked_count, 0);

    let plan = command_capture_plan_from_results(&report.commands, &[]);
    assert!(plan.pass, "{:?}", plan.issues);
    assert!(plan.requested_commands_valid);
    assert!(plan.validation_records_valid);
    assert!(plan.all_selected_gates_ready);
    assert!(plan.critical_gates_ready);
    assert!(plan.capture_actions_ready);
    assert_eq!(plan.command_count, COMMAND_DEFINITIONS.len());
    assert_eq!(plan.ready_count, COMMAND_DEFINITIONS.len());
    assert_eq!(plan.locked_count, 0);
    assert!(plan.next_command_focus.is_none());
}

#[test]
fn command_capture_plan_summarizes_emulator_evidence_promotion_work() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/command-evidence/whoop-emulator-command-evidence.json");
    if !path.exists() {
        eprintln!(
            "skipping command_capture_plan_summarizes_emulator_evidence_promotion_work: {} not present",
            path.display()
        );
        return;
    }
    let evidence = load_command_evidence(&path).unwrap();
    let report = validate_commands(&evidence);
    let requested = [
        "toggle_realtime_hr",
        "run_haptic_pattern_maverick",
        "start_firmware_load_new",
        "reboot_strap",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();

    let plan = command_capture_plan_from_results(&report.commands, &requested);

    assert_eq!(plan.schema, "goose.command-capture-plan-report.v1");
    assert!(!plan.pass);
    assert!(plan.requested_commands_valid);
    assert!(plan.validation_records_valid);
    assert!(!plan.all_selected_gates_ready);
    assert!(!plan.critical_gates_ready);
    assert!(!plan.capture_actions_ready);
    assert_eq!(plan.command_count, 4);
    assert_eq!(plan.ready_count, 1);
    assert_eq!(plan.locked_count, 3);
    assert_eq!(plan.critical_locked_count, 2);
    assert!(
        plan.gates
            .get("toggle_realtime_hr")
            .unwrap()
            .direct_send_allowed
    );
    assert!(plan.actions.iter().any(|action| {
        action.command == "run_haptic_pattern_maverick"
            && action.requirement == "response_parser"
            && action.action.contains("success response parser")
    }));
    assert!(plan.actions.iter().any(|action| {
        action.command == "reboot_strap"
            && action.requirement == "official_capture_evidence"
            && action.action.contains("real strap or macOS BLE emulator")
    }));
    let focus = plan.next_command_focus.as_ref().unwrap();
    assert_eq!(focus.command, "start_firmware_load_new");
    assert_eq!(focus.family, "firmware_dfu");
    assert_eq!(focus.risk_gate, CommandRiskGate::CriticalStateChange);
    assert!(plan.family_summaries.iter().any(|family| {
        family.family == "sensor_stream" && family.ready_count == 1 && family.locked_count == 0
    }));
}

#[test]
fn command_validator_cli_can_emit_capture_plan_for_selected_commands() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/command-evidence/whoop-emulator-command-evidence.json");
    if !path.exists() {
        eprintln!(
            "skipping command_validator_cli_can_emit_capture_plan_for_selected_commands: {} not present",
            path.display()
        );
        return;
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-command-validator"))
        .arg("--evidence")
        .arg(path)
        .arg("--capture-plan")
        .arg("--commands")
        .arg("toggle_realtime_hr,start_firmware_load_new")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["schema"], "goose.command-capture-plan-report.v1");
    assert_eq!(plan["command_count"], 2);
    assert_eq!(plan["requested_commands_valid"], true);
    assert_eq!(plan["validation_records_valid"], true);
    assert_eq!(plan["all_selected_gates_ready"], false);
    assert_eq!(plan["critical_gates_ready"], false);
    assert_eq!(plan["capture_actions_ready"], false);
    assert_eq!(plan["ready_count"], 1);
    assert_eq!(plan["locked_count"], 1);
    assert_eq!(plan["critical_locked_count"], 1);
    assert_eq!(
        plan["gates"]["toggle_realtime_hr"]["direct_send_allowed"],
        true
    );
    assert!(
        plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["command"] == "start_firmware_load_new"
                && action["requirement"] == "explicit_approval")
    );
}

#[test]
fn load_command_evidence_accepts_jsonl_rows_from_file_pickers() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("goose-command-evidence.jsonl");
    let select_wrist_frame = hex::encode(build_v5_command_frame(1, 123, &[1]));
    let rows = [
        serde_json::to_string(&ready_get_hello_evidence()).unwrap(),
        format!(
            r#"{{"command_id":"select_wrist","officialCaptureCount":1,"evidenceSource":"user_owned_official_capture","provenance":{{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user"}},"triggeringUiAction":"Settings > Device > Wrist > Right","officialFrameHex":"{frame}","localFrameHex":"{frame}","officialServiceUuid":"{service}","localServiceUuid":"{service}","officialCharacteristicUuid":"{characteristic}","localCharacteristicUuid":"{characteristic}","officialWriteType":"{write_type}","localWriteType":"{write_type}","officialResponseFrameHex":"{response}","responseParser":true,"visibleUserIntent":true,"eventLogging":true,"timeoutBehavior":true}}"#,
            frame = select_wrist_frame,
            service = COMMAND_SERVICE_UUID,
            characteristic = COMMAND_CHARACTERISTIC_UUID,
            write_type = COMMAND_WRITE_TYPE,
            response = command_response_frame_hex(123)
        ),
    ];
    std::fs::write(&path, format!("{}\n{}\n", rows[0], rows[1])).unwrap();

    let evidence = load_command_evidence(&path).unwrap();
    assert_eq!(evidence.len(), 2);
    let report = validate_commands(&evidence);
    for command in ["get_hello", "select_wrist"] {
        let result = report
            .commands
            .iter()
            .find(|result| result.command == command)
            .unwrap();
        assert!(
            result.direct_send_ready,
            "{command}: {:?}",
            result.missing_requirements
        );
    }
}

#[test]
fn load_command_local_frame_candidates_accepts_wrapper_and_jsonl_aliases() {
    let tempdir = tempfile::tempdir().unwrap();
    let wrapper_path = tempdir
        .path()
        .join("goose-command-local-frame-candidates.json");
    let jsonl_path = tempdir
        .path()
        .join("goose-command-local-frame-candidates.jsonl");
    let whoop_rev_path = tempdir.path().join("whoop-rev-build-command.json");
    let numeric_id_path = tempdir.path().join("numeric-command-id-candidate.json");
    let frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    std::fs::write(
        &wrapper_path,
        format!(
            r#"{{
              "schema":"goose.command-local-frame-candidates.v1",
              "candidates":[{{
                "command_id":"get_hello",
                "dryRunFrameHex":"{frame}",
                "dryRunServiceUuid":"fd4b0001-cce1-4033-93ce-002d5875f58a",
                "dryRunCharacteristicUuid":"fd4b0002-cce1-4033-93ce-002d5875f58a",
                "dryRunWriteType":"with_response",
                "source":"whoop-rev build-command GET_HELLO --frame",
                "provenance":{{"builder":"whoop-rev","dry_run":true}}
              }}]
            }}"#
        ),
    )
    .unwrap();
    std::fs::write(
        &jsonl_path,
        format!(
            r#"{{"id":"get_hello","frame_hex":"{frame}","service_uuid":"fd4b0001-cce1-4033-93ce-002d5875f58a","characteristic_uuid":"fd4b0002-cce1-4033-93ce-002d5875f58a","write_type":"with_response"}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        &whoop_rev_path,
        format!(
            r#"{{"command":"GET_HELLO","command_id":145,"sequence":1,"payload_hex":"23019101","frame_hex":"{frame}","device_type":"GOOSE","send_allowed":false}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        &numeric_id_path,
        format!(r#"{{"command_id":145,"frame_hex":"{frame}"}}"#),
    )
    .unwrap();

    let wrapper_candidates = load_command_local_frame_candidates(&wrapper_path).unwrap();
    assert_eq!(wrapper_candidates.len(), 1);
    assert_eq!(wrapper_candidates[0].command, "get_hello");
    assert_eq!(
        wrapper_candidates[0].local_frame_hex.as_deref(),
        Some(frame.as_str())
    );
    assert!(
        wrapper_candidates[0]
            .provenance_json
            .as_deref()
            .unwrap()
            .contains("whoop-rev")
    );

    let jsonl_candidates = load_command_local_frame_candidates(&jsonl_path).unwrap();
    assert_eq!(jsonl_candidates.len(), 1);
    assert_eq!(
        jsonl_candidates[0].local_characteristic_uuid.as_deref(),
        Some("fd4b0002-cce1-4033-93ce-002d5875f58a")
    );

    let whoop_rev_candidates = load_command_local_frame_candidates(&whoop_rev_path).unwrap();
    assert_eq!(whoop_rev_candidates.len(), 1);
    assert_eq!(whoop_rev_candidates[0].command, "get_hello");
    assert_eq!(
        whoop_rev_candidates[0].local_frame_hex.as_deref(),
        Some(frame.as_str())
    );

    let numeric_id_candidates = load_command_local_frame_candidates(&numeric_id_path).unwrap();
    assert_eq!(numeric_id_candidates.len(), 1);
    assert_eq!(numeric_id_candidates[0].command, "get_hello");
}

#[test]
fn emulator_log_evidence_conversion_keeps_local_frame_match_explicit() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("emulator-log.json");
    let get_hello_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let response = command_response_frame_hex(COMMAND_GET_HELLO);
    let payload = serde_json::json!({
        "emulator_log_tail": {
            "lines": [
                format!("[1.000s] Write 16 bytes to command_to_strap: {get_hello_frame}"),
                "[1.001s] Command getHello seq=1 body=01",
                format!("[1.010s] Notify command_from_strap queued=true: {response}")
            ]
        }
    });
    std::fs::write(&path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();

    let evidence_report =
        command_evidence_from_emulator_log(&path, &CommandEmulatorLogEvidenceOptions::default())
            .unwrap();

    assert_eq!(evidence_report.schema, "goose.command-evidence.v1");
    assert!(evidence_report.pass, "{:?}", evidence_report.issues);
    assert!(evidence_report.input_valid);
    assert!(evidence_report.log_lines_ready);
    assert!(evidence_report.official_writes_parsed);
    assert!(evidence_report.responses_paired);
    assert!(evidence_report.trusted_capture_context);
    assert!(evidence_report.official_capture_ready);
    assert!(!evidence_report.local_frame_match_ready);
    assert!(!evidence_report.direct_validation_ready);
    assert_eq!(evidence_report.evidence_count, 1);
    assert!(
        evidence_report.issues.is_empty(),
        "{:?}",
        evidence_report.issues
    );
    assert!(evidence_report.next_actions.iter().any(|action| {
        action.requirement == "local_frame_matches_official_frame"
            && action.action.contains("local frame candidates")
    }));
    let row = &evidence_report.evidence[0];
    assert_eq!(row.command, "get_hello");
    assert_eq!(
        row.official_frame_hex.as_deref(),
        Some(get_hello_frame.as_str())
    );
    assert_eq!(row.local_frame_hex, None);
    assert_eq!(
        row.official_characteristic_uuid.as_deref(),
        Some("fd4b0002-cce1-4033-93ce-002d5875f58a")
    );
    assert!(row.response_parser);
    assert!(row.timeout_behavior);
    assert!(
        row.provenance_json
            .as_deref()
            .unwrap_or("")
            .contains(r#""capture_kind":"official_app_to_macos_emulator""#)
    );

    let validation = validate_commands(&evidence_report.evidence);
    let get_hello = validation
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(!get_hello.direct_send_ready);
    assert!(
        get_hello
            .missing_requirements
            .contains(&"local_frame_matches_official_frame".to_string()),
        "{:?}",
        get_hello.missing_requirements
    );
}

#[test]
fn emulator_log_evidence_accepts_structured_command_capture_jsonl_rows() {
    let get_hello_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let response = command_response_frame_hex_for_sequence(COMMAND_GET_HELLO, 1, 0);
    let write = serde_json::json!({
        "schema": "whoop-reversing.emulator-command-capture-row.v1",
        "kind": "ble_write_characteristic",
        "direction": "phone_to_device",
        "characteristic_uuid": "fd4b0002-cce1-4033-93ce-002d5875f58a",
        "characteristic_uuid_label": {"role": "command_to_strap"},
        "source": "macos_ble_peripheral_log",
        "source_log": "captures/ble/whoop-emulator.log",
        "source_line": 22,
        "value_hex": get_hello_frame,
        "write_type": "withResponse"
    });
    let notify = serde_json::json!({
        "schema": "whoop-reversing.emulator-command-capture-row.v1",
        "kind": "ble_characteristic_changed",
        "direction": "device_to_phone",
        "characteristic_uuid": "fd4b0003-cce1-4033-93ce-002d5875f58a",
        "characteristic_uuid_label": {"role": "command_from_strap"},
        "notify_queued": true,
        "source": "macos_ble_peripheral_log",
        "source_log": "captures/ble/whoop-emulator.log",
        "source_line": 24,
        "value_hex": response
    });
    let raw_jsonl = format!("{write}\n{notify}\n");

    let evidence_report = command_evidence_from_emulator_log_text(
        "captures/ble/emulator-command-capture.jsonl",
        &raw_jsonl,
        &CommandEmulatorLogEvidenceOptions {
            write_type: "without_response".to_string(),
            ..CommandEmulatorLogEvidenceOptions::default()
        },
    )
    .unwrap();

    assert!(evidence_report.pass, "{:?}", evidence_report.issues);
    assert!(evidence_report.official_capture_ready);
    assert!(!evidence_report.local_frame_match_ready);
    assert_eq!(evidence_report.line_count, 2);
    assert_eq!(evidence_report.evidence_count, 1);
    let row = &evidence_report.evidence[0];
    assert_eq!(row.command, "get_hello");
    assert_eq!(row.official_capture_count, 1);
    assert_eq!(row.official_write_type.as_deref(), Some("with_response"));
    assert_eq!(row.local_write_type.as_deref(), Some("with_response"));
    assert_eq!(
        row.official_characteristic_uuid.as_deref(),
        Some("fd4b0002-cce1-4033-93ce-002d5875f58a")
    );
    assert!(
        row.provenance_json
            .as_deref()
            .unwrap_or("")
            .contains(r#""transaction_lines":[22]"#)
    );

    let wrapped = serde_json::json!({
        "schema": "whoop-reversing.emulator-command-capture.v1",
        "rows": [write, notify]
    });
    let wrapped_report = command_evidence_from_emulator_log_text(
        "captures/ble/emulator-command-capture-report.json",
        &wrapped.to_string(),
        &CommandEmulatorLogEvidenceOptions::default(),
    )
    .unwrap();
    assert_eq!(wrapped_report.evidence_count, 1);
    assert!(wrapped_report.official_capture_ready);
}

#[test]
fn emulator_log_evidence_can_validate_after_explicit_byte_match_replay_gate() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("emulator.log");
    let select_wrist_frame = hex::encode(build_v5_command_frame(7, 123, &[1]));
    let response = command_response_frame_hex_for_sequence(123, 7, 0);
    std::fs::write(
        &path,
        format!(
            "[10.000s] Write 16 bytes to command_to_strap: {select_wrist_frame}\n\
             [10.001s] Command selectWrist seq=7 body=01\n\
             [10.020s] Notify command_from_strap queued=true: {response}\n"
        ),
    )
    .unwrap();

    let evidence_report = command_evidence_from_emulator_log(
        &path,
        &CommandEmulatorLogEvidenceOptions {
            visible_user_intent: true,
            triggering_ui_action: Some("Settings > Device > Wrist > Right".to_string()),
            mirror_local_frame: true,
            ..CommandEmulatorLogEvidenceOptions::default()
        },
    )
    .unwrap();

    assert!(evidence_report.pass, "{:?}", evidence_report.issues);
    assert!(evidence_report.official_capture_ready);
    assert!(evidence_report.local_frame_match_ready);
    assert!(evidence_report.direct_validation_ready);
    assert!(evidence_report.next_actions.is_empty());

    let validation = validate_commands(&evidence_report.evidence);
    let select_wrist = validation
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();
    assert!(
        select_wrist.direct_send_ready,
        "{:?}",
        select_wrist.missing_requirements
    );
    assert_eq!(
        select_wrist.validated_triggering_ui_action.as_deref(),
        Some("Settings > Device > Wrist > Right")
    );
    assert_eq!(
        select_wrist.validated_local_frame_hex.as_deref(),
        Some(select_wrist_frame.as_str())
    );
}

#[test]
fn emulator_log_evidence_reports_next_action_when_no_command_writes_are_found() {
    let evidence_report = command_evidence_from_emulator_log_text(
        "empty-emulator.log",
        "[1.000s] central subscribed but no writes were captured",
        &CommandEmulatorLogEvidenceOptions::default(),
    )
    .unwrap();

    assert!(!evidence_report.pass);
    assert!(evidence_report.input_valid);
    assert!(evidence_report.log_lines_ready);
    assert!(!evidence_report.official_writes_parsed);
    assert_eq!(evidence_report.evidence_count, 0);
    assert!(evidence_report.next_actions.iter().any(|action| {
        action.requirement == "official_command_writes_required"
            && action.action.contains("command characteristic")
    }));
}

#[test]
fn command_validator_cli_can_ingest_emulator_log_and_write_evidence_artifact() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("emulator.log");
    let evidence_output = tempdir.path().join("emulator-evidence.json");
    let get_hello_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let response = command_response_frame_hex(COMMAND_GET_HELLO);
    std::fs::write(
        &path,
        format!(
            "[1.000s] Write 16 bytes to command_to_strap: {get_hello_frame}\n\
             [1.010s] Notify command_from_strap queued=true: {response}\n"
        ),
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-command-validator"))
        .arg("--emulator-log")
        .arg(&path)
        .arg("--emulator-evidence-output")
        .arg(&evidence_output)
        .arg("--emulator-mirror-local-frame")
        .arg("--visible-user-intent")
        .arg("--commands")
        .arg("get_hello")
        .arg("--capture-plan")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["gates"]["get_hello"]["direct_send_allowed"], true);
    assert!(
        evidence_output.exists(),
        "expected CLI to write generated evidence"
    );
    let generated = load_command_evidence(&evidence_output).unwrap();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].command, "get_hello");
}

#[test]
fn command_validator_cli_fails_when_unrequested_command_gates_remain_locked() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("partial-evidence.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&vec![ready_get_hello_evidence()]).unwrap(),
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-command-validator"))
        .arg("--evidence")
        .arg(&path)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pass"], false);
    assert_eq!(report["evidence_valid"], true);
    assert_eq!(report["all_direct_sends_ready"], false);
    assert_eq!(report["direct_send_ready_count"], 1);
    assert_eq!(report["blocked_count"], COMMAND_DEFINITIONS.len() - 1);
}

#[test]
fn command_validator_cli_promotes_emulator_log_with_local_frame_candidates() {
    let tempdir = tempfile::tempdir().unwrap();
    let log_path = tempdir.path().join("emulator.log");
    let candidates_path = tempdir.path().join("local-frame-candidates.json");
    let match_output = tempdir.path().join("local-frame-match-report.json");
    let get_hello_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let response = command_response_frame_hex(COMMAND_GET_HELLO);
    std::fs::write(
        &log_path,
        format!(
            "[1.000s] Write 16 bytes to command_to_strap: {get_hello_frame}\n\
             [1.010s] Notify command_from_strap queued=true: {response}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        &candidates_path,
        format!(
            r#"{{
              "command":"GET_HELLO",
              "command_id":145,
              "sequence":1,
              "payload_hex":"23019101",
              "frame_hex":"{get_hello_frame}",
              "device_type":"GOOSE",
              "send_allowed":false
            }}"#
        ),
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-command-validator"))
        .arg("--emulator-log")
        .arg(&log_path)
        .arg("--local-frame-candidates")
        .arg(&candidates_path)
        .arg("--local-frame-match-output")
        .arg(&match_output)
        .arg("--visible-user-intent")
        .arg("--commands")
        .arg("get_hello")
        .arg("--capture-plan")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["gates"]["get_hello"]["direct_send_allowed"], true);
    assert_eq!(
        plan["gates"]["get_hello"]["validated_local_frame_hex"],
        get_hello_frame
    );
    let match_report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&match_output).unwrap()).unwrap();
    assert_eq!(match_report["matched_count"], 1);
    assert_eq!(
        match_report["comparisons"][0]["reason"],
        "local_frame_matches_official_frame"
    );
}

#[test]
fn command_validator_cli_accepts_builder_directory_for_local_frame_candidates() {
    let tempdir = tempfile::tempdir().unwrap();
    let log_path = tempdir.path().join("emulator.log");
    let candidates_dir = tempdir.path().join("015-safe-read-baseline-builders");
    let match_output = tempdir.path().join("local-frame-match-report.json");
    std::fs::create_dir(&candidates_dir).unwrap();

    let get_hello_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let get_battery_level_frame = hex::encode(build_v5_command_frame(2, 26, &[]));
    let get_hello_response = command_response_frame_hex_for_sequence(COMMAND_GET_HELLO, 1, 0);
    let get_battery_response = command_response_frame_hex_for_sequence(26, 2, 0);
    std::fs::write(
        &log_path,
        format!(
            "[1.000s] Write 16 bytes to command_to_strap: {get_hello_frame}\n\
             [1.010s] Notify command_from_strap queued=true: {get_hello_response}\n\
             [2.000s] Write 16 bytes to command_to_strap: {get_battery_level_frame}\n\
             [2.010s] Notify command_from_strap queued=true: {get_battery_response}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        candidates_dir.join("001-get-hello.json"),
        format!(
            r#"{{"command":"GET_HELLO","command_id":145,"sequence":1,"frame_hex":"{get_hello_frame}","device_type":"GOOSE","send_allowed":false}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        candidates_dir.join("002-get-battery-level.json"),
        format!(
            r#"{{"command":"GET_BATTERY_LEVEL","command_id":26,"sequence":2,"frame_hex":"{get_battery_level_frame}","device_type":"GOOSE","send_allowed":false}}"#
        ),
    )
    .unwrap();
    std::fs::write(candidates_dir.join("README.txt"), "ignored").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_goose-command-validator"))
        .arg("--emulator-log")
        .arg(&log_path)
        .arg("--local-frame-candidates")
        .arg(&candidates_dir)
        .arg("--local-frame-match-output")
        .arg(&match_output)
        .arg("--visible-user-intent")
        .arg("--commands")
        .arg("get_hello,get_battery_level")
        .arg("--capture-plan")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["gates"]["get_hello"]["direct_send_allowed"], true);
    assert_eq!(
        plan["gates"]["get_battery_level"]["direct_send_allowed"],
        true
    );
    let match_report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&match_output).unwrap()).unwrap();
    assert_eq!(match_report["matched_count"], 2);
    assert_eq!(match_report["candidate_count"], 2);
}

#[test]
fn local_frame_match_promotion_unlocks_exact_goose_dry_run_bytes() {
    let frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let response = command_response_frame_hex(COMMAND_GET_HELLO);
    let official = CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some(TRUSTED_COMMAND_EVIDENCE_SOURCE.to_string()),
        provenance_json: Some(TRUSTED_COMMAND_PROVENANCE_JSON.to_string()),
        official_frame_hex: Some(frame.clone()),
        local_frame_hex: None,
        official_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        official_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        official_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        official_response_frame_hex: Some(response),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    };

    let report = command_evidence_with_local_frame_matches(
        &[official],
        &[CommandLocalFrameCandidate {
            command: "get_hello".to_string(),
            command_id: None,
            local_frame_hex: Some(frame.clone()),
            local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
            local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            source: Some("whoop-rev build-command GET_HELLO --frame".to_string()),
            provenance_json: Some(r#"{"builder":"whoop-rev","dry_run":true}"#.to_string()),
        }],
    );

    assert!(report.pass, "{:?}", report.issues);
    assert!(report.input_valid);
    assert!(report.comparisons_ready);
    assert!(report.all_frames_matched);
    assert!(report.promotion_ready);
    assert_eq!(report.matched_count, 1);
    assert!(report.next_actions.is_empty());
    assert_eq!(
        report.evidence[0].local_frame_hex.as_deref(),
        Some(frame.as_str())
    );
    assert!(
        report.evidence[0]
            .provenance_json
            .as_deref()
            .unwrap()
            .contains("local_frame_match")
    );

    let validation = validate_commands(&report.evidence);
    let get_hello = validation
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        get_hello.direct_send_ready,
        "{:?}",
        get_hello.missing_requirements
    );
}

#[test]
fn local_frame_match_promotion_normalizes_whoop_rev_command_ids() {
    let frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let official = with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_frame_hex: Some(frame.clone()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    });

    for (command, command_id) in [("GET_HELLO", None), ("", Some("145".to_string()))] {
        let report = command_evidence_with_local_frame_matches(
            std::slice::from_ref(&official),
            &[CommandLocalFrameCandidate {
                command: command.to_string(),
                command_id,
                local_frame_hex: Some(frame.clone()),
                local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
                local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
                local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
                source: Some("whoop-rev build-command GET_HELLO --frame".to_string()),
                provenance_json: None,
            }],
        );
        assert!(report.pass, "{command}: {:?}", report.issues);
        assert_eq!(report.matched_count, 1);
        assert_eq!(
            report.evidence[0].local_frame_hex.as_deref(),
            Some(frame.as_str())
        );
    }
}

#[test]
fn local_frame_match_promotion_blocks_different_command_bytes() {
    let official_frame = hex::encode(build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]));
    let wrong_local_frame = hex::encode(build_v5_command_frame(1, 123, &[1]));
    let official = with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_frame_hex: Some(official_frame),
        local_frame_hex: None,
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    });

    let report = command_evidence_with_local_frame_matches(
        &[official],
        &[CommandLocalFrameCandidate {
            command: "get_hello".to_string(),
            command_id: None,
            local_frame_hex: Some(wrong_local_frame),
            local_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
            local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            local_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            source: Some("test wrong command builder".to_string()),
            provenance_json: None,
        }],
    );

    assert!(!report.pass);
    assert!(report.input_valid);
    assert!(report.comparisons_ready);
    assert!(!report.all_frames_matched);
    assert!(!report.promotion_ready);
    assert_eq!(report.matched_count, 0);
    assert_eq!(report.blocked_count, 1);
    assert_eq!(
        report.comparisons[0].reason,
        "local_frame_command_number_mismatch"
    );
    assert!(report.next_actions.iter().any(|action| {
        action.command == "get_hello"
            && action.requirement == "local_frame_command_number_mismatch"
            && action.action.contains("same command number")
    }));
    assert_eq!(report.evidence[0].local_frame_hex, None);
}

#[test]
fn local_frame_match_report_fails_closed_without_evidence_or_candidates() {
    let report = command_evidence_with_local_frame_matches(&[], &[]);

    assert!(!report.pass);
    assert!(!report.input_valid);
    assert!(report.comparisons_ready);
    assert!(!report.all_frames_matched);
    assert!(!report.promotion_ready);
    assert_eq!(report.evidence_count, 0);
    assert_eq!(report.candidate_count, 0);
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| { action.requirement == "official_command_evidence_required" })
    );
    assert!(
        report
            .next_actions
            .iter()
            .any(|action| { action.requirement == "local_frame_candidates_required" })
    );
}

#[test]
fn read_command_requires_official_capture_frame_match_and_response_parser() {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        result.direct_send_ready,
        "{:?}",
        result.missing_requirements
    );
    assert_eq!(result.risk_gate, CommandRiskGate::ReadOnly);
}

#[test]
fn normal_state_change_requires_visible_intent_logging_timeout_and_matching_frame() {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "select_wrist".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some("aa01".to_string()),
        local_frame_hex: Some("aa02".to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(123)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();
    assert!(!result.direct_send_ready);
    assert!(
        result
            .missing_requirements
            .contains(&"local_frame_matches_official_frame".to_string())
    );
}

#[test]
fn state_changing_command_requires_official_triggering_ui_action() {
    let select_wrist_frame = hex::encode(build_v5_command_frame(1, 123, &[1]));
    let mut evidence = with_trusted_capture(CommandEvidence {
        command: "select_wrist".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(select_wrist_frame.clone()),
        local_frame_hex: Some(select_wrist_frame),
        official_response_frame_hex: Some(command_response_frame_hex(123)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    });
    evidence.triggering_ui_action = None;

    let report = validate_commands(&[evidence]);
    let result = report
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();

    assert!(!result.direct_send_ready);
    assert!(
        result
            .missing_requirements
            .contains(&"official_triggering_ui_action".to_string()),
        "{:?}",
        result.missing_requirements
    );
    assert!(result.next_capture_actions.iter().any(|action| {
        action.requirement == "official_triggering_ui_action"
            && action.action.contains("official app screen")
    }));
}

#[test]
fn matching_frame_bytes_must_also_match_expected_command_number() {
    let get_hello_frame = hex::encode(build_v5_command_frame(1, 145, &[]));
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "select_wrist".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(get_hello_frame.clone()),
        local_frame_hex: Some(get_hello_frame),
        official_response_frame_hex: Some(command_response_frame_hex(145)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();
    assert!(!result.direct_send_ready);
    assert!(
        result
            .missing_requirements
            .contains(&"official_frame_command_number_matches_definition".to_string()),
        "{:?}",
        result.missing_requirements
    );
}

#[test]
fn response_frame_must_pair_back_to_the_validated_command() {
    let select_wrist_frame = hex::encode(build_v5_command_frame(1, 123, &[1]));
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "select_wrist".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(select_wrist_frame.clone()),
        local_frame_hex: Some(select_wrist_frame),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();
    assert!(!result.direct_send_ready);
    assert!(
        result
            .missing_requirements
            .contains(&"official_response_matches_command_number".to_string()),
        "{:?}",
        result.missing_requirements
    );
}

#[test]
fn command_endpoint_must_match_official_capture_before_direct_send() {
    let get_hello_frame = hex::encode(build_v5_command_frame(1, 145, &[]));
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(get_hello_frame.clone()),
        local_frame_hex: Some(get_hello_frame),
        official_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        local_service_uuid: Some("61080003-0000-1000-8000-00805f9b34fb".to_string()),
        official_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        local_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        official_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        local_write_type: Some("without_response".to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(!result.direct_send_ready);
    for expected in [
        "ble_service_uuid_matches_official_capture",
        "ble_write_type_matches_official_capture",
    ] {
        assert!(
            result.missing_requirements.contains(&expected.to_string()),
            "{expected}: {:?}",
            result.missing_requirements
        );
    }
}

#[test]
fn critical_commands_require_multiple_captures_failure_parser_confirmation_and_rollback() {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "start_firmware_load_new".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some("aa01".to_string()),
        local_frame_hex: Some("aa01".to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(142)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert_eq!(result.risk_gate, CommandRiskGate::CriticalStateChange);
    for expected in [
        "official_capture_count>=2",
        "failure_parser",
        "visible_confirmation",
        "rollback_or_restore_plan",
        "explicit_approval",
    ] {
        assert!(
            result.missing_requirements.contains(&expected.to_string()),
            "missing requirement {expected}: {:?}",
            result.missing_requirements
        );
    }
    assert!(
        result
            .next_capture_actions
            .iter()
            .any(|action| action.requirement == "official_capture_count>=2"
                && action
                    .action
                    .contains("at least 2 official app transactions")),
        "{:?}",
        result.next_capture_actions
    );
    assert!(
        result
            .next_capture_actions
            .iter()
            .any(|action| action.requirement == "failure_parser"
                && action.action.contains("non-success official response")),
        "{:?}",
        result.next_capture_actions
    );
}

#[test]
fn blocked_gate_carries_next_capture_actions_to_app_preflight() {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "run_haptic_pattern_maverick".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(hex::encode(build_v5_command_frame(1, 19, &[1]))),
        local_frame_hex: Some(hex::encode(build_v5_command_frame(1, 19, &[1]))),
        visible_user_intent: true,
        logging: true,
        ..CommandEvidence::default()
    })]);
    let result = report
        .commands
        .iter()
        .find(|command| command.command == "run_haptic_pattern_maverick")
        .unwrap();
    let gate = direct_send_gate_from_result("run_haptic_pattern_maverick", Some(result));

    assert!(!gate.direct_send_allowed);
    assert!(
        gate.next_capture_actions
            .iter()
            .any(|action| action.requirement == "response_parser"
                && action.action.contains("success response parser")),
        "{:?}",
        gate.next_capture_actions
    );
    assert!(
        gate.next_capture_actions
            .iter()
            .any(|action| action.requirement == "timeout_behavior"
                && action.action.contains("timeout/retry behavior")),
        "{:?}",
        gate.next_capture_actions
    );
}

#[test]
fn critical_command_failure_parser_requires_captured_failure_response() {
    let critical_frame = hex::encode(build_v5_command_frame(1, 142, &[]));
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "start_firmware_load_new".to_string(),
        official_capture_count: 2,
        official_frame_hex: Some(critical_frame.clone()),
        local_frame_hex: Some(critical_frame),
        official_response_frame_hex: Some(command_response_frame_hex(142)),
        response_parser: true,
        failure_parser: true,
        visible_user_intent: true,
        visible_confirmation: true,
        logging: true,
        timeout_behavior: true,
        rollback_plan: true,
        explicit_approval: true,
        ..CommandEvidence::default()
    })]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert!(!result.direct_send_ready);
    assert_eq!(
        result.missing_requirements,
        vec!["official_failure_response_frame".to_string()]
    );
}

#[test]
fn critical_command_failure_response_must_be_non_success_for_same_command() {
    let critical_frame = hex::encode(build_v5_command_frame(1, 142, &[]));
    let success_report = validate_commands(&[critical_command_evidence(
        &critical_frame,
        command_response_frame_hex(142),
    )]);
    let success_result = success_report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert!(!success_result.direct_send_ready);
    assert!(
        success_result
            .missing_requirements
            .contains(&"official_failure_response_result_code_nonzero".to_string()),
        "{:?}",
        success_result.missing_requirements
    );

    let wrong_command_report = validate_commands(&[critical_command_evidence(
        &critical_frame,
        command_failure_response_frame_hex(COMMAND_GET_HELLO),
    )]);
    let wrong_command_result = wrong_command_report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert!(!wrong_command_result.direct_send_ready);
    assert!(
        wrong_command_result
            .missing_requirements
            .contains(&"official_failure_response_matches_command_number".to_string()),
        "{:?}",
        wrong_command_result.missing_requirements
    );
}

#[test]
fn critical_command_can_be_ready_with_success_and_failure_response_pairing() {
    let critical_frame = hex::encode(build_v5_command_frame(1, 142, &[]));
    let report = validate_commands(&[critical_command_evidence(
        &critical_frame,
        command_failure_response_frame_hex(142),
    )]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert!(
        result.direct_send_ready,
        "{:?}",
        result.missing_requirements
    );
}

#[test]
fn command_evidence_source_must_be_user_owned_official_capture_not_private_api() {
    let report = validate_commands(&[CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some("private_api_replay".to_string()),
        provenance_json: Some(TRUSTED_COMMAND_PROVENANCE_JSON.to_string()),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    }]);

    let result = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(!result.direct_send_ready);
    assert!(
        result
            .missing_requirements
            .contains(&"official_capture_source_trusted".to_string()),
        "{:?}",
        result.missing_requirements
    );
    assert!(
        result
            .warnings
            .contains(&"private_api_replay_not_allowed_for_command_validation".to_string()),
        "{:?}",
        result.warnings
    );
}

#[test]
fn official_app_emulator_source_alias_requires_user_owned_provenance() {
    let ready_report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some("official_app_capture".to_string()),
        provenance_json: Some(
            r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user"}"#.to_string(),
        ),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);
    let ready_result = ready_report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        ready_result.direct_send_ready,
        "{:?}",
        ready_result.missing_requirements
    );
    assert_eq!(
        ready_result.validated_evidence_source.as_deref(),
        Some("official_app_capture")
    );
    assert_eq!(
        ready_result.validated_capture_kind.as_deref(),
        Some("official_app_to_macos_emulator")
    );
    assert_eq!(ready_result.validated_owner.as_deref(), Some("user"));
    assert!(
        ready_result
            .validated_provenance_json
            .as_deref()
            .unwrap_or_default()
            .contains(r#""capture_app":"whoop_official""#)
    );

    let blocked_report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some("official_app_capture".to_string()),
        provenance_json: Some(
            r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"unknown"}"#.to_string(),
        ),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);
    let blocked_result = blocked_report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert_eq!(
        blocked_result.validated_evidence_source.as_deref(),
        Some("official_app_capture")
    );
    assert_eq!(
        blocked_result.validated_capture_kind.as_deref(),
        Some("official_app_to_macos_emulator")
    );
    assert_eq!(blocked_result.validated_owner.as_deref(), Some("unknown"));
    assert!(!blocked_result.direct_send_ready);
    assert!(
        blocked_result
            .missing_requirements
            .contains(&"official_capture_source_trusted".to_string()),
        "{:?}",
        blocked_result.missing_requirements
    );
}

#[test]
fn command_validation_reports_evidence_source_summary() {
    let report = validate_commands(&[
        with_trusted_capture(CommandEvidence {
            command: "get_hello".to_string(),
            official_capture_count: 1,
            evidence_source: Some("official_app_capture".to_string()),
            provenance_json: Some(
                r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"user"}"#.to_string(),
            ),
            official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
            local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
            official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
            response_parser: true,
            visible_user_intent: true,
            logging: true,
            timeout_behavior: true,
            ..CommandEvidence::default()
        }),
        with_trusted_capture(CommandEvidence {
            command: "select_wrist".to_string(),
            official_capture_count: 1,
            evidence_source: Some("official_app_capture".to_string()),
            provenance_json: Some(
                r#"{"capture_app":"whoop_official","capture_kind":"official_app_to_macos_emulator","owner":"unknown"}"#.to_string(),
            ),
            ..CommandEvidence::default()
        }),
        CommandEvidence {
            command: "run_alarm".to_string(),
            official_capture_count: 1,
            evidence_source: Some("private_api_replay".to_string()),
            provenance_json: Some(TRUSTED_COMMAND_PROVENANCE_JSON.to_string()),
            ..CommandEvidence::default()
        },
    ]);

    let trusted_alias = report
        .evidence_source_summary
        .iter()
        .find(|summary| {
            summary.evidence_source == "official_app_capture"
                && summary.capture_kind.as_deref() == Some("official_app_to_macos_emulator")
                && summary.owner.as_deref() == Some("user")
        })
        .unwrap();
    assert_eq!(trusted_alias.count, 1);
    assert_eq!(trusted_alias.trusted_for_promotion_count, 1);
    assert_eq!(trusted_alias.blocked_for_source_count, 0);

    let blocked_alias = report
        .evidence_source_summary
        .iter()
        .find(|summary| {
            summary.evidence_source == "official_app_capture"
                && summary.capture_kind.as_deref() == Some("official_app_to_macos_emulator")
                && summary.owner.as_deref() == Some("unknown")
        })
        .unwrap();
    assert_eq!(blocked_alias.count, 1);
    assert_eq!(blocked_alias.trusted_for_promotion_count, 0);
    assert_eq!(blocked_alias.blocked_for_source_count, 1);

    let private_api = report
        .evidence_source_summary
        .iter()
        .find(|summary| summary.evidence_source == "private_api_replay")
        .unwrap();
    assert_eq!(private_api.trusted_for_promotion_count, 0);
    assert_eq!(private_api.blocked_for_source_count, 1);
}

#[test]
fn command_evidence_requires_non_empty_provenance_object() {
    let missing_provenance_report = validate_commands(&[CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some(TRUSTED_COMMAND_EVIDENCE_SOURCE.to_string()),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    }]);
    let missing_result = missing_provenance_report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        missing_result
            .missing_requirements
            .contains(&"official_capture_provenance".to_string()),
        "{:?}",
        missing_result.missing_requirements
    );

    let invalid_provenance_report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        provenance_json: Some("not-json".to_string()),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);
    let invalid_result = invalid_provenance_report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        invalid_result
            .missing_requirements
            .contains(&"official_capture_provenance_json_object".to_string()),
        "{:?}",
        invalid_result.missing_requirements
    );

    let empty_object_report = validate_commands(&[CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        evidence_source: Some(TRUSTED_COMMAND_EVIDENCE_SOURCE.to_string()),
        provenance_json: Some("{}".to_string()),
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    }]);
    let empty_object_result = empty_object_report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    assert!(
        empty_object_result
            .missing_requirements
            .contains(&"official_capture_provenance_non_empty_object".to_string()),
        "{:?}",
        empty_object_result.missing_requirements
    );
}

#[test]
fn direct_send_gate_uses_validation_result_and_fails_closed_without_record() {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);
    let ready = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    let gate = direct_send_gate_from_result("get_hello", Some(ready));
    assert!(gate.direct_send_allowed);
    assert_eq!(gate.risk_gate, Some(CommandRiskGate::ReadOnly));

    let missing = direct_send_gate_from_result("run_alarm", None);
    assert!(!missing.direct_send_allowed);
    assert!(
        missing
            .missing_requirements
            .contains(&"command_validation_record".to_string())
    );
}

#[test]
fn direct_send_preflight_requires_fresh_visible_logged_connected_override() {
    let ready_gate = ready_get_hello_gate();
    let blocked = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "get_hello".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: None,
            visible_user_intent: false,
            dry_run_bytes_shown: false,
            dry_run_frame_hex: None,
            dry_run_service_uuid: None,
            dry_run_characteristic_uuid: None,
            dry_run_write_type: None,
            session_log_ready: false,
            connection_state: Some("disconnected".to_string()),
            active_device_id: None,
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        ready_gate,
    );

    assert!(!blocked.direct_send_allowed);
    for expected in [
        "active_device_id",
        "connected_device",
        "dry_run_frame_hex",
        "dry_run_bytes_shown",
        "session_log_entry",
        "short_lived_user_override",
        "visible_user_intent",
    ] {
        assert!(
            blocked.missing_requirements.contains(&expected.to_string()),
            "{expected}: {:?}",
            blocked.missing_requirements
        );
    }
}

#[test]
fn direct_send_preflight_allows_only_short_lived_ready_gate() {
    let too_long = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "get_hello".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: Some(61_001),
            visible_user_intent: true,
            dry_run_bytes_shown: true,
            dry_run_frame_hex: Some(GET_HELLO_FRAME.to_string()),
            dry_run_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
            dry_run_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            dry_run_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            session_log_ready: true,
            connection_state: Some("connected".to_string()),
            active_device_id: Some("strap-1".to_string()),
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        ready_get_hello_gate(),
    );
    assert!(!too_long.direct_send_allowed);
    assert!(
        too_long
            .missing_requirements
            .contains(&"short_lived_user_override_short_lived".to_string()),
        "{:?}",
        too_long.missing_requirements
    );

    let allowed = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "get_hello".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: Some(16_000),
            visible_user_intent: true,
            dry_run_bytes_shown: true,
            dry_run_frame_hex: Some(GET_HELLO_FRAME.to_string()),
            dry_run_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
            dry_run_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            dry_run_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            session_log_ready: true,
            connection_state: Some("connected".to_string()),
            active_device_id: Some("strap-1".to_string()),
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        ready_get_hello_gate(),
    );

    assert!(
        allowed.direct_send_allowed,
        "{:?}",
        allowed.missing_requirements
    );
    assert_eq!(allowed.override_expires_in_ms, Some(15_000));
    assert_eq!(allowed.dry_run_frame_hex.as_deref(), Some(GET_HELLO_FRAME));
}

#[test]
fn direct_send_preflight_requires_displayed_bytes_to_match_validated_frame() {
    let mismatched = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "get_hello".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: Some(16_000),
            visible_user_intent: true,
            dry_run_bytes_shown: true,
            dry_run_frame_hex: Some("aa0108000001e6712301910100000000".to_string()),
            dry_run_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
            dry_run_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            dry_run_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            session_log_ready: true,
            connection_state: Some("connected".to_string()),
            active_device_id: Some("strap-1".to_string()),
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        ready_get_hello_gate(),
    );

    assert!(!mismatched.direct_send_allowed);
    assert!(
        mismatched
            .missing_requirements
            .contains(&"dry_run_frame_matches_validated_local_frame".to_string()),
        "{:?}",
        mismatched.missing_requirements
    );
}

#[test]
fn direct_send_preflight_requires_displayed_endpoint_to_match_validated_capture() {
    let mismatched = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "get_hello".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: Some(16_000),
            visible_user_intent: true,
            dry_run_bytes_shown: true,
            dry_run_frame_hex: Some(GET_HELLO_FRAME.to_string()),
            dry_run_service_uuid: Some("61080003-0000-1000-8000-00805f9b34fb".to_string()),
            dry_run_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
            dry_run_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
            session_log_ready: true,
            connection_state: Some("connected".to_string()),
            active_device_id: Some("strap-1".to_string()),
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        ready_get_hello_gate(),
    );

    assert!(!mismatched.direct_send_allowed);
    assert!(
        mismatched
            .missing_requirements
            .contains(&"dry_run_service_uuid_matches_validated_endpoint".to_string()),
        "{:?}",
        mismatched.missing_requirements
    );
}

#[test]
fn critical_direct_send_preflight_requires_runtime_approval_even_when_gate_ready() {
    let critical_frame = hex::encode(build_v5_command_frame(1, 142, &[]));
    let blocked = direct_send_preflight_from_gate(
        &ready_preflight_input("start_firmware_load_new", &critical_frame),
        ready_start_firmware_gate(&critical_frame),
    );

    assert!(!blocked.direct_send_allowed);
    for expected in [
        "critical_explicit_approval",
        "critical_rollback_or_restore_acknowledged",
        "critical_visible_confirmation",
    ] {
        assert!(
            blocked.missing_requirements.contains(&expected.to_string()),
            "{expected}: {:?}",
            blocked.missing_requirements
        );
    }
}

#[test]
fn critical_direct_send_preflight_allows_with_runtime_approval_bundle() {
    let critical_frame = hex::encode(build_v5_command_frame(1, 142, &[]));
    let mut input = ready_preflight_input("start_firmware_load_new", &critical_frame);
    input.critical_visible_confirmation = true;
    input.critical_explicit_approval = true;
    input.critical_rollback_or_restore_acknowledged = true;

    let allowed =
        direct_send_preflight_from_gate(&input, ready_start_firmware_gate(&critical_frame));

    assert!(
        allowed.direct_send_allowed,
        "{:?}",
        allowed.missing_requirements
    );
}

#[test]
fn user_visible_state_change_preflight_does_not_require_critical_runtime_approval() {
    let select_wrist_frame = hex::encode(build_v5_command_frame(1, 123, &[1]));
    let allowed = direct_send_preflight_from_gate(
        &ready_preflight_input("select_wrist", &select_wrist_frame),
        ready_select_wrist_gate(&select_wrist_frame),
    );

    assert!(
        allowed.direct_send_allowed,
        "{:?}",
        allowed.missing_requirements
    );
}

#[test]
fn historical_sync_direct_writes_stay_behind_validation_and_runtime_preflight_gates() {
    let definition = COMMAND_DEFINITIONS
        .iter()
        .find(|definition| definition.id == "send_historical_data")
        .unwrap();
    let report = validate_commands(&[ready_command_evidence_for_definition(definition)]);
    let result = report
        .commands
        .iter()
        .find(|command| command.command == "send_historical_data")
        .unwrap();

    assert!(
        result.direct_send_ready,
        "{:?}",
        result.missing_requirements
    );
    assert_eq!(result.family, "historical_sync");
    assert_eq!(result.risk_gate, CommandRiskGate::UserVisibleStateChange);

    let gate = direct_send_gate_from_result("send_historical_data", Some(result));
    let blocked = direct_send_preflight_from_gate(
        &CommandDirectSendPreflightInput {
            command: "send_historical_data".to_string(),
            now_unix_ms: 1_000,
            override_expires_at_unix_ms: None,
            visible_user_intent: false,
            dry_run_bytes_shown: false,
            dry_run_frame_hex: None,
            dry_run_service_uuid: None,
            dry_run_characteristic_uuid: None,
            dry_run_write_type: None,
            session_log_ready: false,
            connection_state: Some("disconnected".to_string()),
            active_device_id: None,
            critical_visible_confirmation: false,
            critical_explicit_approval: false,
            critical_rollback_or_restore_acknowledged: false,
        },
        gate,
    );

    assert!(!blocked.direct_send_allowed);
    for expected in [
        "active_device_id",
        "connected_device",
        "dry_run_bytes_shown",
        "dry_run_frame_hex",
        "session_log_entry",
        "short_lived_user_override",
        "visible_user_intent",
    ] {
        assert!(
            blocked.missing_requirements.contains(&expected.to_string()),
            "{expected}: {:?}",
            blocked.missing_requirements
        );
    }

    let missing_validation_record = direct_send_gate_from_result("send_historical_data", None);
    assert!(!missing_validation_record.direct_send_allowed);
    assert!(
        missing_validation_record
            .missing_requirements
            .contains(&"command_validation_record".to_string())
    );
}

fn command_response_frame_hex(command: u8) -> String {
    command_response_frame_hex_with_result(command, 0)
}

fn command_failure_response_frame_hex(command: u8) -> String {
    command_response_frame_hex_with_result(command, 1)
}

fn command_response_frame_hex_with_result(command: u8, result_code: u8) -> String {
    command_response_frame_hex_for_sequence(command, 1, result_code)
}

fn command_response_frame_hex_for_sequence(
    command: u8,
    origin_sequence: u8,
    result_code: u8,
) -> String {
    hex::encode(build_v5_payload_frame(&[
        PACKET_TYPE_COMMAND_RESPONSE,
        9,
        command,
        origin_sequence,
        result_code,
    ]))
}

fn with_trusted_capture(mut evidence: CommandEvidence) -> CommandEvidence {
    if evidence.evidence_source.is_none() {
        evidence.evidence_source = Some(TRUSTED_COMMAND_EVIDENCE_SOURCE.to_string());
    }
    if evidence.provenance_json.is_none() {
        evidence.provenance_json = Some(TRUSTED_COMMAND_PROVENANCE_JSON.to_string());
    }
    if evidence.official_service_uuid.is_none() {
        evidence.official_service_uuid = Some(COMMAND_SERVICE_UUID.to_string());
    }
    if evidence.local_service_uuid.is_none() {
        evidence.local_service_uuid = Some(COMMAND_SERVICE_UUID.to_string());
    }
    if evidence.official_characteristic_uuid.is_none() {
        evidence.official_characteristic_uuid = Some(COMMAND_CHARACTERISTIC_UUID.to_string());
    }
    if evidence.local_characteristic_uuid.is_none() {
        evidence.local_characteristic_uuid = Some(COMMAND_CHARACTERISTIC_UUID.to_string());
    }
    if evidence.official_write_type.is_none() {
        evidence.official_write_type = Some(COMMAND_WRITE_TYPE.to_string());
    }
    if evidence.local_write_type.is_none() {
        evidence.local_write_type = Some(COMMAND_WRITE_TYPE.to_string());
    }
    if evidence.triggering_ui_action.is_none() {
        evidence.triggering_ui_action =
            Some(format!("official app test action for {}", evidence.command));
    }
    evidence
}

fn ready_get_hello_evidence() -> CommandEvidence {
    with_trusted_capture(CommandEvidence {
        command: "get_hello".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        local_frame_hex: Some(GET_HELLO_FRAME.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(COMMAND_GET_HELLO)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })
}

fn ready_command_evidence_for_definition(definition: &CommandDefinition) -> CommandEvidence {
    let command_number = definition
        .command_number
        .expect("command definitions used for direct-send gates need command numbers");
    let command =
        u8::try_from(command_number).expect("Goose command frame builder expects u8 command ids");
    let frame = hex::encode(build_v5_command_frame(1, command, &[1]));
    let critical = definition.risk_gate == CommandRiskGate::CriticalStateChange;
    with_trusted_capture(CommandEvidence {
        command: definition.id.to_string(),
        official_capture_count: if critical { 2 } else { 1 },
        official_frame_hex: Some(frame.clone()),
        local_frame_hex: Some(frame),
        official_response_frame_hex: Some(command_response_frame_hex(command)),
        official_failure_response_frame_hex: critical
            .then(|| command_failure_response_frame_hex(command)),
        response_parser: true,
        failure_parser: critical,
        visible_user_intent: true,
        visible_confirmation: critical,
        logging: true,
        timeout_behavior: true,
        rollback_plan: critical,
        explicit_approval: critical,
        ..CommandEvidence::default()
    })
}

fn ready_get_hello_gate() -> goose_core::commands::CommandDirectSendGate {
    let report = validate_commands(&[ready_get_hello_evidence()]);
    let ready = report
        .commands
        .iter()
        .find(|command| command.command == "get_hello")
        .unwrap();
    direct_send_gate_from_result("get_hello", Some(ready))
}

fn ready_select_wrist_gate(
    select_wrist_frame: &str,
) -> goose_core::commands::CommandDirectSendGate {
    let report = validate_commands(&[with_trusted_capture(CommandEvidence {
        command: "select_wrist".to_string(),
        official_capture_count: 1,
        official_frame_hex: Some(select_wrist_frame.to_string()),
        local_frame_hex: Some(select_wrist_frame.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(123)),
        response_parser: true,
        visible_user_intent: true,
        logging: true,
        timeout_behavior: true,
        ..CommandEvidence::default()
    })]);
    let ready = report
        .commands
        .iter()
        .find(|command| command.command == "select_wrist")
        .unwrap();
    direct_send_gate_from_result("select_wrist", Some(ready))
}

fn ready_start_firmware_gate(critical_frame: &str) -> goose_core::commands::CommandDirectSendGate {
    let report = validate_commands(&[critical_command_evidence(
        critical_frame,
        command_failure_response_frame_hex(142),
    )]);
    let ready = report
        .commands
        .iter()
        .find(|command| command.command == "start_firmware_load_new")
        .unwrap();
    assert!(ready.direct_send_ready, "{:?}", ready.missing_requirements);
    direct_send_gate_from_result("start_firmware_load_new", Some(ready))
}

fn ready_preflight_input(command: &str, frame: &str) -> CommandDirectSendPreflightInput {
    CommandDirectSendPreflightInput {
        command: command.to_string(),
        now_unix_ms: 1_000,
        override_expires_at_unix_ms: Some(16_000),
        visible_user_intent: true,
        dry_run_bytes_shown: true,
        dry_run_frame_hex: Some(frame.to_string()),
        dry_run_service_uuid: Some(COMMAND_SERVICE_UUID.to_string()),
        dry_run_characteristic_uuid: Some(COMMAND_CHARACTERISTIC_UUID.to_string()),
        dry_run_write_type: Some(COMMAND_WRITE_TYPE.to_string()),
        session_log_ready: true,
        connection_state: Some("connected".to_string()),
        active_device_id: Some("strap-1".to_string()),
        critical_visible_confirmation: false,
        critical_explicit_approval: false,
        critical_rollback_or_restore_acknowledged: false,
    }
}

fn critical_command_evidence(
    critical_frame: &str,
    failure_response_frame_hex: String,
) -> CommandEvidence {
    with_trusted_capture(CommandEvidence {
        command: "start_firmware_load_new".to_string(),
        official_capture_count: 2,
        official_frame_hex: Some(critical_frame.to_string()),
        local_frame_hex: Some(critical_frame.to_string()),
        official_response_frame_hex: Some(command_response_frame_hex(142)),
        official_failure_response_frame_hex: Some(failure_response_frame_hex),
        response_parser: true,
        failure_parser: true,
        visible_user_intent: true,
        visible_confirmation: true,
        logging: true,
        timeout_behavior: true,
        rollback_plan: true,
        explicit_approval: true,
        ..CommandEvidence::default()
    })
}
