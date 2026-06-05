// Temporary verification: parse the exact Gen4 command frames the Swift client
// now builds for WHOOP 4.0 through the strap-mirroring parser and assert their
// CRCs and command decoding are valid.
use goose_core::protocol::{parse_frame_hex, DeviceType, PACKET_TYPE_COMMAND};

fn check(label: &str, hex: &str, expected_cmd: u8) {
    let frame = parse_frame_hex(DeviceType::Gen4, hex)
        .unwrap_or_else(|e| panic!("{label}: parse failed: {e:?}"));
    assert!(frame.header_crc_valid, "{label}: header CRC8 invalid");
    assert!(frame.payload_crc_valid, "{label}: payload CRC32 invalid");
    assert_eq!(frame.header_len, 4, "{label}: header must be 4 bytes (Gen4)");
    assert_eq!(
        frame.packet_type,
        Some(PACKET_TYPE_COMMAND),
        "{label}: packet type must be COMMAND(35)"
    );
    assert_eq!(
        frame.command_or_event,
        Some(expected_cmd),
        "{label}: wrong command opcode"
    );
}

#[test]
fn gen4_outbound_frames_are_accepted_by_the_strap_parser() {
    check("HELLO", "aa0800a823002300ada86a2d", 35); // GetHelloHarvard
    check("TOGGLE_REALTIME_HR_ON", "aa0800a823b4030155eabe0d", 3);
    check("SET_CLOCK", "aa10005723390a30835e6600000000007116bd0e", 10);
    check("GET_NAME", "aa0800a8233a4c0083bff3e6", 76);
    check("ENTER_HIGH_FREQ_SYNC", "aa07006b233b60cbaf4ab4", 96);
    check("HISTORY_START", "aa0800a8233c1600ef762aa2", 22);
    check("HISTORY_ACK", "aa100057233d17010000000000000000ae478d25", 23);
}
