// Tests locking in WHOOP 4.0 (Gen4) BLE support and the recent fixes:
//   * "GEN4" (no underscore) accepted as DeviceType::Gen4 in the bridge.
//   * Gen4 4-byte header (len in bytes[1..3], crc8 at byte 3) + payload CRC32.
//   * Gen4 vs Gen5 header_len / expected_frame_len.
//   * Console-log / event / metadata / command-response Gen4 packet classification.
//   * Panic safety across the FFI boundary on malformed / truncated / garbage input.
//
// Complements (does not replace) gen4_outbound_verification.rs, which checks the
// exact outbound command frames the Swift client builds. Here we additionally
// build inbound-style Gen4 frames by hand and exercise the parser/bridge surface.
use goose_core::bridge::{BridgeResponse, handle_bridge_request_json};
use goose_core::protocol::{
    DeviceType, PACKET_TYPE_COMMAND, PACKET_TYPE_COMMAND_RESPONSE, PACKET_TYPE_CONSOLE_LOGS,
    PACKET_TYPE_EVENT, PACKET_TYPE_METADATA, ParsedFrame, ParsedPayload, crc8, parse_frame,
    parse_frame_hex,
};

const BRIDGE_REQUEST_SCHEMA: &str = "goose.bridge.request.v1";

/// Known-good outbound Gen4 GET_HELLO frame (mirrors the Swift client).
const GEN4_HELLO_HEX: &str = "aa0800a823002300ada86a2d";

/// Build a structurally valid Gen4 frame around an arbitrary payload:
/// `[0xaa, len_lo, len_hi, crc8(len)] + payload + crc32_le(payload)`.
/// The declared length covers `payload.len() + 4` (the trailing CRC32).
fn gen4_frame(payload: &[u8]) -> Vec<u8> {
    let declared_len = (payload.len() + 4) as u16;
    let len_bytes = declared_len.to_le_bytes();
    let header_crc = crc8(&len_bytes);
    let mut frame = Vec::with_capacity(4 + payload.len() + 4);
    frame.push(0xaa);
    frame.extend_from_slice(&len_bytes);
    frame.push(header_crc);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    frame
}

fn parse_gen4(frame: &[u8]) -> ParsedFrame {
    parse_frame(DeviceType::Gen4, frame).expect("valid gen4 frame should parse")
}

// ---------------------------------------------------------------------------
// 1. DEVICE TYPE PARSING (via the public bridge round-trip)
// ---------------------------------------------------------------------------

fn parse_frame_via_bridge(device_type: &str, frame_hex: &str) -> BridgeResponse {
    let request = serde_json::json!({
        "schema": BRIDGE_REQUEST_SCHEMA,
        "request_id": "gen4-device-type",
        "method": "protocol.parse_frame_hex",
        "args": { "device_type": device_type, "frame_hex": frame_hex },
    });
    serde_json::from_str(&handle_bridge_request_json(&request.to_string()))
        .expect("bridge must return parseable JSON")
}

#[test]
fn bridge_accepts_gen4_without_underscore() {
    // The fix: "GEN4" (no underscore) must resolve to DeviceType::Gen4.
    let response = parse_frame_via_bridge("GEN4", GEN4_HELLO_HEX);
    assert!(
        response.ok,
        "GEN4 should be accepted, got error: {:?}",
        response.error
    );
    let result = response.result.expect("ok response carries a result");
    // DeviceType::Gen4 serializes SCREAMING_SNAKE_CASE -> "GEN4".
    assert_eq!(result["device_type"], "GEN4");
    assert_eq!(result["header_len"], 4);
    assert_eq!(result["header_crc_valid"], true);
    assert_eq!(result["payload_crc_valid"], true);
}

#[test]
fn bridge_accepts_gen4_with_underscore() {
    let response = parse_frame_via_bridge("GEN_4", GEN4_HELLO_HEX);
    assert!(
        response.ok,
        "GEN_4 should be accepted, got error: {:?}",
        response.error
    );
    assert_eq!(
        response.result.expect("result")["header_len"],
        4,
        "Gen4 header is 4 bytes"
    );
}

#[test]
fn bridge_gen4_and_gen_4_resolve_to_the_same_device() {
    let underscore = parse_frame_via_bridge("GEN_4", GEN4_HELLO_HEX)
        .result
        .expect("GEN_4 result");
    let no_underscore = parse_frame_via_bridge("GEN4", GEN4_HELLO_HEX)
        .result
        .expect("GEN4 result");
    assert_eq!(
        underscore, no_underscore,
        "GEN4 and GEN_4 must parse identically"
    );
}

#[test]
fn bridge_rejects_unknown_device_type() {
    let response = parse_frame_via_bridge("WHOOP_99", GEN4_HELLO_HEX);
    assert!(!response.ok, "unknown device_type must not succeed");
    let error = response.error.expect("error present");
    assert!(
        error.message.contains("unsupported device_type"),
        "expected unsupported device_type error, got: {}",
        error.message
    );
}

// ---------------------------------------------------------------------------
// 2. GEN4 FRAME ROUND-TRIP
// ---------------------------------------------------------------------------

#[test]
fn gen4_known_hello_frame_round_trips() {
    let parsed = parse_frame_hex(DeviceType::Gen4, GEN4_HELLO_HEX).expect("hello parses");
    assert_eq!(parsed.header_len, 4);
    assert_eq!(parsed.declared_len, 8); // 4-byte payload + 4-byte CRC32
    assert!(parsed.header_crc_valid, "header crc8 must be valid");
    assert!(parsed.payload_crc_valid, "payload crc32 must be valid");
    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_COMMAND));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("COMMAND"));
    assert!(parsed.warnings.is_empty());
}

#[test]
fn hand_built_gen4_command_frame_validates_both_crcs() {
    // packet_type=COMMAND(35), sequence=7, command=145 (GET_HELLO), data=0x01
    let payload = [PACKET_TYPE_COMMAND, 7, 145, 0x01];
    let frame = gen4_frame(&payload);
    let parsed = parse_gen4(&frame);

    assert_eq!(parsed.header_len, 4);
    assert_eq!(parsed.raw_len, frame.len());
    assert_eq!(parsed.declared_len, payload.len() + 4);
    assert!(parsed.header_crc_valid);
    assert!(parsed.payload_crc_valid);
    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_COMMAND));
    assert_eq!(parsed.sequence, Some(7));
    assert_eq!(parsed.command_or_event, Some(145));
    assert_eq!(
        parsed.parsed_payload,
        Some(ParsedPayload::Command {
            command: Some(145),
            command_name: Some("GET_HELLO".to_string()),
            data_offset: 3,
            data_hex: "01".to_string(),
            warnings: Vec::new(),
        })
    );
    assert!(parsed.warnings.is_empty());
}

#[test]
fn gen4_header_crc_uses_only_the_two_length_bytes() {
    // Independently confirm the header CRC contract: crc8 over bytes[1..3] == byte[3].
    let payload = [PACKET_TYPE_COMMAND, 1, 145];
    let frame = gen4_frame(&payload);
    assert_eq!(
        crc8(&frame[1..3]),
        frame[3],
        "byte[3] must be crc8 of the two length bytes"
    );
    assert!(parse_gen4(&frame).header_crc_valid);
}

#[test]
fn gen4_corrupted_header_crc_is_flagged_not_panicked() {
    let payload = [PACKET_TYPE_COMMAND, 1, 145, 0x02];
    let mut frame = gen4_frame(&payload);
    frame[3] ^= 0xff; // corrupt the header crc8 byte
    let parsed = parse_gen4(&frame);
    assert!(
        !parsed.header_crc_valid,
        "corrupted header crc must be reported invalid"
    );
    assert!(parsed.warnings.contains(&"header_crc_mismatch".to_string()));
}

#[test]
fn gen4_corrupted_payload_crc_is_flagged_not_panicked() {
    let payload = [PACKET_TYPE_COMMAND, 1, 145, 0x03];
    let mut frame = gen4_frame(&payload);
    let last = frame.len() - 1;
    frame[last] ^= 0xff; // corrupt the trailing payload crc32
    let parsed = parse_gen4(&frame);
    assert!(parsed.header_crc_valid, "header should still be valid");
    assert!(
        !parsed.payload_crc_valid,
        "corrupted payload crc must be reported invalid"
    );
    assert!(parsed.warnings.contains(&"payload_crc_mismatch".to_string()));
}

// ---------------------------------------------------------------------------
// 3. GEN4 vs GEN5 HEADER GEOMETRY
// ---------------------------------------------------------------------------

#[test]
fn gen4_header_is_four_bytes_and_gen5_headers_are_eight() {
    assert_eq!(DeviceType::Gen4.header_len(), 4);
    assert_eq!(DeviceType::Maverick.header_len(), 8);
    assert_eq!(DeviceType::Puffin.header_len(), 8);
    assert_eq!(DeviceType::Goose.header_len(), 8);
}

#[test]
fn expected_frame_len_matches_header_geometry() {
    // Gen4: declared length lives in bytes[1..3], frame = declared + 4-byte header.
    let gen4 = gen4_frame(&[PACKET_TYPE_COMMAND, 1, 145, 0xab, 0xcd]);
    assert_eq!(
        DeviceType::Gen4.expected_frame_len(&gen4),
        Some(gen4.len()),
        "Gen4 expected_frame_len must equal the actual frame length"
    );

    // Gen5 family: declared length lives in bytes[2..4], frame = declared + 8-byte header.
    // Construct a header that declares a 12-byte body+crc tail.
    let mut gen5 = vec![0xaa, 0x01];
    gen5.extend_from_slice(&12u16.to_le_bytes()); // declared length at bytes[2..4]
    gen5.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // remaining header bytes
    assert_eq!(
        DeviceType::Goose.expected_frame_len(&gen5),
        Some(8 + 12),
        "Gen5 expected_frame_len = 8-byte header + declared length"
    );
    assert_eq!(DeviceType::Maverick.expected_frame_len(&gen5), Some(20));
    assert_eq!(DeviceType::Puffin.expected_frame_len(&gen5), Some(20));
}

#[test]
fn expected_frame_len_none_when_buffer_too_short_for_header() {
    assert_eq!(DeviceType::Gen4.expected_frame_len(&[0xaa, 0x08]), None);
    assert_eq!(
        DeviceType::Goose.expected_frame_len(&[0xaa, 0x01, 0x08, 0x00]),
        None
    );
}

// ---------------------------------------------------------------------------
// 4. CONSOLE_LOGS / EVENT / METADATA / COMMAND_RESPONSE classification (Gen4)
// ---------------------------------------------------------------------------

#[test]
fn gen4_console_log_frame_is_classified() {
    // type 50 CONSOLE_LOGS, payload carries an ASCII-ish body.
    let payload = [
        PACKET_TYPE_CONSOLE_LOGS,
        0x01,
        b'b',
        b'o',
        b'o',
        b't',
        b'\n',
    ];
    let parsed = parse_gen4(&gen4_frame(&payload));
    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_CONSOLE_LOGS));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("CONSOLE_LOGS"));
    assert!(parsed.header_crc_valid && parsed.payload_crc_valid);
    // Console logs fall through to a Raw payload (no dedicated decoder), never panic.
    assert!(matches!(
        parsed.parsed_payload,
        Some(ParsedPayload::Raw { .. })
    ));
}

#[test]
fn gen4_event_frame_is_classified_and_decoded() {
    // type 48 EVENT, event_id=15 (BOOT) at bytes[2..4], 32-bit ts at [4..8], 16-bit subsec at [8..10].
    // The event header is 12 bytes (data starts at offset 12), so bytes 10..12 are header
    // padding and the event body begins at offset 12.
    let mut payload = vec![PACKET_TYPE_EVENT, 0x02];
    payload.extend_from_slice(&15u16.to_le_bytes()); // event_id BOOT (offset 2..4)
    payload.extend_from_slice(&0x11223344u32.to_le_bytes()); // timestamp (offset 4..8)
    payload.extend_from_slice(&0x5566u16.to_le_bytes()); // subseconds (offset 8..10)
    payload.extend_from_slice(&[0x00, 0x00]); // header padding (offset 10..12)
    payload.extend_from_slice(&[0xde, 0xad]); // body (offset 12..)
    let parsed = parse_gen4(&gen4_frame(&payload));

    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_EVENT));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("EVENT"));
    match parsed.parsed_payload.expect("event payload") {
        ParsedPayload::Event {
            event_id,
            event_name,
            timestamp_seconds,
            timestamp_subseconds,
            data_hex,
            ..
        } => {
            assert_eq!(event_id, Some(15));
            assert_eq!(event_name.as_deref(), Some("BOOT"));
            assert_eq!(timestamp_seconds, Some(0x11223344));
            assert_eq!(timestamp_subseconds, Some(0x5566));
            assert_eq!(data_hex, "dead");
        }
        other => panic!("expected Event payload, got {other:?}"),
    }
}

#[test]
fn gen4_metadata_frame_is_classified() {
    // type 49 METADATA has no dedicated decoder -> Raw payload, but must classify the name.
    let payload = [PACKET_TYPE_METADATA, 0x03, 0xaa, 0xbb, 0xcc, 0xdd];
    let parsed = parse_gen4(&gen4_frame(&payload));
    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_METADATA));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("METADATA"));
    assert!(parsed.header_crc_valid && parsed.payload_crc_valid);
    assert!(matches!(
        parsed.parsed_payload,
        Some(ParsedPayload::Raw { .. })
    ));
}

#[test]
fn gen4_command_response_frame_is_decoded() {
    // type 36 COMMAND_RESPONSE: response_to_command at [2], origin_seq at [3], result at [4].
    let payload = [
        PACKET_TYPE_COMMAND_RESPONSE,
        0x09,
        145, // GET_HELLO
        1,   // origin sequence
        0,   // result code
        0xaa,
        0xbb,
    ];
    let parsed = parse_gen4(&gen4_frame(&payload));
    assert_eq!(parsed.packet_type, Some(PACKET_TYPE_COMMAND_RESPONSE));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("COMMAND_RESPONSE"));
    assert_eq!(
        parsed.parsed_payload,
        Some(ParsedPayload::CommandResponse {
            response_to_command: Some(145),
            response_to_command_name: Some("GET_HELLO".to_string()),
            origin_sequence: Some(1),
            result_code: Some(0),
            data_offset: 5,
            data_hex: "aabb".to_string(),
            warnings: Vec::new(),
        })
    );
}

// ---------------------------------------------------------------------------
// 5. PANIC SAFETY at the FFI boundary
// ---------------------------------------------------------------------------

#[test]
fn empty_and_garbage_inputs_never_panic() {
    // Empty buffer.
    assert!(parse_frame(DeviceType::Gen4, &[]).is_err());
    // Single 0xaa with no length bytes.
    assert!(parse_frame(DeviceType::Gen4, &[0xaa]).is_err());
    // Does not start with the frame marker.
    assert!(parse_frame(DeviceType::Gen4, &[0x00, 0x01, 0x02, 0x03]).is_err());
    // All-0xAA bytes: declared length 0xaaaa wildly exceeds the buffer.
    let all_aa = parse_frame(DeviceType::Gen4, &[0xaa; 16]);
    match all_aa {
        Ok(frame) => assert!(
            !frame.warnings.is_empty(),
            "an accepted suspicious frame must carry warnings"
        ),
        Err(_) => {} // erroring is also acceptable; the contract is "no panic".
    }
}

#[test]
fn declared_length_exceeding_buffer_does_not_panic() {
    // Build a header that claims a much larger payload than is present.
    let len_bytes = 200u16.to_le_bytes();
    let header_crc = crc8(&len_bytes);
    let frame = vec![0xaa, len_bytes[0], len_bytes[1], header_crc, PACKET_TYPE_EVENT];
    // Result must be Ok-with-warnings or Err, but never a panic.
    match parse_frame(DeviceType::Gen4, &frame) {
        Ok(parsed) => assert!(
            parsed.warnings.contains(&"frame_truncated".to_string()),
            "a truncated-but-accepted frame must warn it is truncated"
        ),
        Err(_) => {}
    }
}

#[test]
fn odd_length_and_invalid_hex_return_err_not_panic() {
    // Odd number of hex digits cannot decode to bytes.
    assert!(parse_frame_hex(DeviceType::Gen4, "aa080").is_err());
    // Non-hex characters.
    assert!(parse_frame_hex(DeviceType::Gen4, "zzzz").is_err());
    // Empty string.
    assert!(parse_frame_hex(DeviceType::Gen4, "").is_err());
}

#[test]
fn random_garbage_hex_buffers_never_panic_for_gen4_or_gen5() {
    // A spread of malformed/short/garbage inputs across both header geometries.
    let cases = [
        "aa",
        "aaaa",
        "aaaaaa",
        "aa00",
        "aa0000",
        "aa000000",
        "aaffffffffffffff",
        "00112233445566778899",
        "aabbccddeeff",
        "aa0800a8",                 // gen4 header only, no payload
        "aa0800a823",              // gen4 header + 1 payload byte (truncated)
        "deadbeefdeadbeefdeadbeef", // does not start with 0xaa
    ];
    for device in [
        DeviceType::Gen4,
        DeviceType::Maverick,
        DeviceType::Puffin,
        DeviceType::Goose,
    ] {
        for case in cases {
            // The only contract: this returns (Ok or Err) without unwinding.
            let _ = parse_frame_hex(device, case);
        }
    }
}

// ---------------------------------------------------------------------------
// 6. Bridge-level structured errors (no panic across FFI)
// ---------------------------------------------------------------------------

#[test]
fn bridge_invalid_json_returns_structured_error() {
    let response: BridgeResponse =
        serde_json::from_str(&handle_bridge_request_json("{not valid json"))
            .expect("bridge always returns valid JSON");
    assert!(!response.ok);
    let error = response.error.expect("error present");
    assert_eq!(error.code, "invalid_json");
}

#[test]
fn bridge_unknown_schema_returns_structured_error() {
    let request = serde_json::json!({
        "schema": "goose.bridge.request.WRONG",
        "request_id": "schema-1",
        "method": "protocol.parse_frame_hex",
        "args": { "device_type": "GEN4", "frame_hex": GEN4_HELLO_HEX },
    });
    let response: BridgeResponse =
        serde_json::from_str(&handle_bridge_request_json(&request.to_string())).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "unsupported_schema");
}

#[test]
fn bridge_gen4_garbage_frame_returns_structured_error_not_panic() {
    // A frame that does not start with 0xaa must surface as a method error, not a panic.
    let response = parse_frame_via_bridge("GEN4", "deadbeef");
    assert!(!response.ok, "garbage frame must not succeed");
    let error = response.error.expect("error present");
    assert_ne!(error.code, "panic", "must be a clean error, not a caught panic");
}
