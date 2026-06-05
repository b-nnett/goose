// Verify the V12/V24 normal_history DSP sensor decode (SpO2, skin temp, respiratory,
// RR intervals, signal quality) against a REAL WHOOP 4.0 history frame captured on
// hardware. Field layout follows the openwhoop reference; offsets confirmed by the
// heart-rate marker matching.
use goose_core::protocol::{parse_frame_hex, DataPacketBodySummary, DeviceType, ParsedPayload};

#[test]
fn gen4_v24_normal_history_decodes_dsp_sensor_fields() {
    // Real GEN_4 k24 normal_history frame: hr=92, spo2_red=551, spo2_ir=617,
    // skin_temp_raw=774, respiratory_rate_raw=3073, signal_quality=3074, no RR.
    let hex = "aa6400a12f18053ffead0148b1216af822805454015c0000000000000000000071ec05d080c5c53cf600b03ec31dd7beece9633f00009dc5f600b03ec31dd7beece9633f2702690206036e0255015002010c020c010000000046000186060000000000005fd78f0e";
    let frame = parse_frame_hex(DeviceType::Gen4, hex).expect("parse");
    assert!(frame.header_crc_valid, "header crc");
    assert!(frame.payload_crc_valid, "payload crc");

    let payload = frame.parsed_payload.expect("payload");
    let ParsedPayload::DataPacket {
        body_summary: Some(summary),
        ..
    } = payload
    else {
        panic!("expected a data packet with a body summary");
    };

    match summary {
        DataPacketBodySummary::NormalHistory {
            marker_value,
            rr_intervals_ms,
            spo2_red,
            spo2_ir,
            skin_temp_raw,
            respiratory_rate_raw,
            signal_quality,
            ..
        } => {
            assert_eq!(marker_value, Some(92), "heart-rate marker");
            assert_eq!(rr_intervals_ms, Vec::<u16>::new(), "no RR in this frame");
            assert_eq!(spo2_red, Some(551));
            assert_eq!(spo2_ir, Some(617));
            assert_eq!(skin_temp_raw, Some(774));
            assert_eq!(respiratory_rate_raw, Some(3073));
            assert_eq!(signal_quality, Some(3074));
        }
        other => panic!("expected NormalHistory, got {other:?}"),
    }
}
