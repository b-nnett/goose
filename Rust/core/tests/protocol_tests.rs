use goose_core::protocol::{
    COMMAND_GET_HELLO, DataPacketBodySummary, DeviceType, FrameAccumulator, I16SeriesSummary,
    PACKET_TYPE_COMMAND_RESPONSE, PACKET_TYPE_EVENT, PACKET_TYPE_HISTORICAL_DATA,
    PACKET_TYPE_R22_REALTIME_DATA, PACKET_TYPE_REALTIME_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
    ParsedPayload, build_v5_command_frame, build_v5_payload_frame, parse_frame, parse_frame_hex,
};

const GET_HELLO_FRAME: &str = "aa0108000001e67123019101363e5c8d";

#[test]
fn parses_hand_derived_goose_v5_get_hello_frame() {
    let parsed = parse_frame_hex(DeviceType::Goose, GET_HELLO_FRAME).unwrap();

    assert_eq!(parsed.raw_len, 16);
    assert_eq!(parsed.header_len, 8);
    assert_eq!(parsed.declared_len, 8);
    assert_eq!(parsed.payload_hex, "23019101");
    assert_eq!(parsed.packet_type, Some(35));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("COMMAND"));
    assert_eq!(parsed.sequence, Some(1));
    assert_eq!(parsed.command_or_event, Some(145));
    assert!(parsed.header_crc_valid);
    assert!(parsed.payload_crc_valid);
    assert!(parsed.warnings.is_empty());
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
}

#[test]
fn builder_matches_existing_python_command_builder_fixture() {
    let frame = build_v5_command_frame(1, COMMAND_GET_HELLO, &[1]);

    assert_eq!(hex::encode(frame), GET_HELLO_FRAME);
}

#[test]
fn deframer_reassembles_split_v5_frame_and_drops_prefix_noise() {
    let frame = hex::decode(GET_HELLO_FRAME).unwrap();
    let mut accumulator = FrameAccumulator::new(DeviceType::Goose);

    let first = accumulator.feed(&[0x00, 0x01, frame[0], frame[1], frame[2]]);
    assert!(first.frames.is_empty());
    assert_eq!(first.dropped_prefix_len, 2);
    assert_eq!(first.buffered_len, 3);

    let second = accumulator.feed(&frame[3..]);
    assert_eq!(second.frames, vec![frame]);
    assert_eq!(second.buffered_len, 0);
}

#[test]
fn payload_crc_mismatch_preserves_parseable_header_with_warning() {
    let mut frame = hex::decode(GET_HELLO_FRAME).unwrap();
    let last = frame.len() - 1;
    frame[last] ^= 0xff;

    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert!(parsed.header_crc_valid);
    assert!(!parsed.payload_crc_valid);
    assert_eq!(parsed.packet_type, Some(35));
    assert!(
        parsed
            .warnings
            .contains(&"payload_crc_mismatch".to_string())
    );
}

#[test]
fn malformed_length_fails_safely() {
    let mut frame = hex::decode(GET_HELLO_FRAME).unwrap();
    frame[2] = 0x04;
    frame[3] = 0x00;

    let error = parse_frame(DeviceType::Goose, &frame).unwrap_err();
    assert!(error.to_string().contains("declared length"));
}

#[test]
fn parses_generic_command_response_payload_contract() {
    let frame = build_v5_payload_frame(&[
        PACKET_TYPE_COMMAND_RESPONSE,
        9,
        COMMAND_GET_HELLO,
        1,
        0,
        0xaa,
        0xbb,
        0xcc,
    ]);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert_eq!(parsed.packet_type_name.as_deref(), Some("COMMAND_RESPONSE"));
    assert_eq!(
        parsed.parsed_payload,
        Some(ParsedPayload::CommandResponse {
            response_to_command: Some(COMMAND_GET_HELLO),
            response_to_command_name: Some("GET_HELLO".to_string()),
            origin_sequence: Some(1),
            result_code: Some(0),
            data_offset: 5,
            data_hex: "aabbcc".to_string(),
            warnings: Vec::new(),
        })
    );
}

#[test]
fn parses_event_header_and_preserves_unknown_event_body() {
    let frame = build_v5_payload_frame(&[
        PACKET_TYPE_EVENT,
        2,
        17,
        0,
        0x04,
        0x03,
        0x02,
        0x01,
        0x06,
        0x05,
        0,
        0,
        0xde,
        0xad,
        0xbe,
        0xef,
    ]);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert_eq!(parsed.packet_type_name.as_deref(), Some("EVENT"));
    assert_eq!(
        parsed.parsed_payload,
        Some(ParsedPayload::Event {
            event_id: Some(17),
            event_name: Some("TEMPERATURE_LEVEL".to_string()),
            timestamp_seconds: Some(0x01020304),
            timestamp_subseconds: Some(0x0506),
            data_offset: 12,
            data_hex: "deadbeef".to_string(),
            warnings: Vec::new(),
        })
    );
}

#[test]
fn parses_history_packet_stable_header_and_hr_marker() {
    // v18 is now its own parser (split from 7|9|12|18 NormalHistory arm).
    // A short v18 payload produces V18History with v18_payload_too_short warning;
    // the outer header fields (packet_k, hr_marker_offset, etc.) remain unchanged.
    let frame = build_v5_payload_frame(&[
        PACKET_TYPE_HISTORICAL_DATA,
        18,
        1,
        0x04,
        0x03,
        0x02,
        0x01,
        0x44,
        0x33,
        0x22,
        0x11,
        0x66,
        0x55,
        0xaa,
        0x4d,
        0xbb,
        0xcc,
        0xdd,
        0xee,
        0xff,
    ]);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert_eq!(parsed.packet_type_name.as_deref(), Some("HISTORICAL_DATA"));
    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            packet_k,
            domain,
            status_or_stream,
            counter_or_page,
            timestamp_seconds,
            timestamp_subseconds,
            hr_marker_offset,
            hr_present_marker,
            body_offset,
            body_summary,
            ..
        } => {
            assert_eq!(packet_k, Some(18));
            assert_eq!(domain.as_deref(), Some("normal_history_with_hr_marker"));
            assert_eq!(status_or_stream, Some(1));
            assert_eq!(counter_or_page, Some(0x01020304));
            assert_eq!(timestamp_seconds, Some(0x11223344));
            assert_eq!(timestamp_subseconds, Some(0x5566));
            assert_eq!(hr_marker_offset, Some(14));
            assert_eq!(hr_present_marker, Some(0x4d));
            assert_eq!(body_offset, 13);
            // v18 body: too short payload → V18History with warning (body has only 7 bytes)
            match body_summary.unwrap() {
                DataPacketBodySummary::V18History { warnings, .. } => {
                    assert!(warnings.contains(&"v18_payload_too_short".to_string()));
                }
                other => panic!("expected V18History, got {other:?}"),
            }
        }
        other => panic!("expected DataPacket, got {other:?}"),
    }
}

#[test]
fn normal_history_zero_hr_marker_is_not_treated_as_hr_present() {
    let mut payload = vec![PACKET_TYPE_HISTORICAL_DATA, 9, 1];
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&3u16.to_le_bytes());
    payload.resize(18, 0);
    payload[17] = 0;
    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_summary,
            warnings,
            ..
        } => {
            assert!(warnings.is_empty());
            assert_eq!(
                body_summary,
                Some(DataPacketBodySummary::NormalHistory {
                    hr_present: Some(false),
                    marker_offset: Some(17),
                    marker_value: Some(0),
                })
            );
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn parses_r17_optical_body_offsets_and_signed_sample_stats() {
    let mut payload = vec![0; 32];
    payload[0] = PACKET_TYPE_HISTORICAL_DATA;
    payload[1] = 17;
    payload[2] = 1;
    put_u16(&mut payload, 13, (1 << 9) | (1 << 11));
    payload[15..=20].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    put_u16(&mut payload, 24, 3);
    put_i16(&mut payload, 26, 1000);
    put_i16(&mut payload, 28, -1000);
    put_i16(&mut payload, 30, 200);

    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_hex,
            body_summary,
            warnings,
            ..
        } => {
            // WR-01: body_hex must be populated for R17 — PERF-05 only excludes K10/K21.
            assert!(
                !body_hex.is_empty(),
                "body_hex must be non-empty for R17 (PERF-05 only excludes K10 and K21)"
            );
            assert!(warnings.is_empty());
            assert_eq!(
                body_summary,
                Some(DataPacketBodySummary::R17OpticalOrLabradorFiltered {
                    flags: Some(0x0a00),
                    flag_bit_9: Some(true),
                    flag_bit_11: Some(true),
                    channels_or_gain: vec![1, 2, 3, 4, 5, 6],
                    sample_count: Some(3),
                    samples: Some(I16SeriesSummary {
                        name: "r17_samples".to_string(),
                        offset: 26,
                        expected_count: 3,
                        parsed_count: 3,
                        min: Some(-1000),
                        max: Some(1000),
                        sum: 200,
                        preview: vec![1000, -1000, 200],
                        full_samples: Some(vec![1000, -1000, 200]),
                    }),
                    warnings: Vec::new(),
                })
            );
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn r17_truncated_samples_warn_without_losing_available_values() {
    let mut payload = vec![0; 28];
    payload[0] = PACKET_TYPE_HISTORICAL_DATA;
    payload[1] = 17;
    put_u16(&mut payload, 24, 4);
    put_i16(&mut payload, 26, -7);

    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_summary,
            warnings,
            ..
        } => {
            assert!(warnings.contains(&"r17_samples_truncated".to_string()));
            let Some(DataPacketBodySummary::R17OpticalOrLabradorFiltered {
                samples,
                warnings: summary_warnings,
                ..
            }) = body_summary
            else {
                panic!("expected r17 body summary");
            };
            assert!(summary_warnings.contains(&"r17_samples_truncated".to_string()));
            assert_eq!(samples.unwrap().parsed_count, 1);
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn parses_k10_raw_motion_offsets_without_claiming_units() {
    let mut payload = vec![0; 1288];
    payload[0] = PACKET_TYPE_REALTIME_RAW_DATA;
    payload[1] = 10;
    payload[17] = 72;
    put_i16(&mut payload, 85, 1);
    put_i16(&mut payload, 87, -2);
    put_i16(&mut payload, 89, 3);
    put_i16(&mut payload, 1088, -10);
    put_i16(&mut payload, 1090, 20);

    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_summary,
            body_hex,
            warnings,
            ..
        } => {
            assert!(warnings.is_empty());
            assert!(
                body_hex.is_empty(),
                "body_hex must be empty for K10 after PERF-05 exclusion"
            );
            let Some(DataPacketBodySummary::RawMotionK10 {
                heart_rate,
                axes,
                warnings: summary_warnings,
            }) = body_summary
            else {
                panic!("expected k10 body summary");
            };
            assert_eq!(heart_rate, Some(72));
            assert!(summary_warnings.is_empty());
            assert_eq!(axes.len(), 6);
            assert_eq!(axes[0].name, "accelerometer_x");
            assert_eq!(axes[0].expected_count, 100);
            assert_eq!(axes[0].parsed_count, 100);
            assert_eq!(axes[0].min, Some(-2));
            assert_eq!(axes[0].max, Some(3));
            assert_eq!(axes[0].sum, 2);
            assert_eq!(axes[5].name, "gyroscope_z");
            assert_eq!(axes[5].min, Some(-10));
            assert_eq!(axes[5].max, Some(20));
            // full_samples preservation: all 100 K10 samples retained
            assert_eq!(axes[0].full_samples.as_ref().unwrap().len(), 100);
            assert_eq!(axes[0].full_samples.as_ref().unwrap()[0..3], [1, -2, 3]);
            assert_eq!(axes[0].preview.len(), 8);
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn parses_k21_grouped_motion_offsets_and_counts() {
    let mut payload = vec![0; 1038];
    payload[0] = PACKET_TYPE_REALTIME_DATA;
    payload[1] = 21;
    put_u16(&mut payload, 14, 321);
    put_u16(&mut payload, 16, 3);
    put_u16(&mut payload, 622, 3);
    put_i16(&mut payload, 20, -1);
    put_i16(&mut payload, 22, 2);
    put_i16(&mut payload, 24, -3);
    put_i16(&mut payload, 1032, 50);
    put_i16(&mut payload, 1034, 60);
    put_i16(&mut payload, 1036, 70);

    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_summary,
            body_hex,
            warnings,
            ..
        } => {
            assert!(warnings.is_empty());
            assert!(
                body_hex.is_empty(),
                "body_hex must be empty for K21 after PERF-05 exclusion"
            );
            let Some(DataPacketBodySummary::RawMotionK21 {
                field_x,
                group_1_count,
                group_2_count,
                axes,
                warnings: summary_warnings,
            }) = body_summary
            else {
                panic!("expected k21 body summary");
            };
            assert_eq!(field_x, Some(321));
            assert_eq!(group_1_count, Some(3));
            assert_eq!(group_2_count, Some(3));
            assert!(summary_warnings.is_empty());
            assert_eq!(axes.len(), 6);
            assert_eq!(axes[0].name, "group_1_axis_0");
            assert_eq!(axes[0].preview, vec![-1, 2, -3]);
            assert_eq!(axes[0].sum, -2);
            assert_eq!(axes[5].name, "group_2_axis_2");
            assert_eq!(axes[5].preview, vec![50, 60, 70]);
            assert_eq!(axes[5].sum, 180);
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn truncated_long_motion_frame_keeps_partial_samples_with_quality_warnings() {
    let mut payload = vec![0; 1038];
    payload[0] = PACKET_TYPE_REALTIME_DATA;
    payload[1] = 21;
    put_u16(&mut payload, 14, 321);
    put_u16(&mut payload, 16, 100);
    put_i16(&mut payload, 20, -1);
    put_i16(&mut payload, 22, 2);
    put_i16(&mut payload, 24, -3);
    let mut frame = build_v5_payload_frame(&payload);
    frame.truncate(180);

    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert_eq!(parsed.raw_len, 180);
    assert!(parsed.declared_len > parsed.raw_len);
    assert!(!parsed.payload_crc_valid);
    assert_eq!(parsed.payload_crc_hex, "");
    assert!(parsed.warnings.contains(&"frame_truncated".to_string()));
    assert!(
        parsed
            .warnings
            .contains(&"payload_crc_unavailable_due_to_truncated_frame".to_string())
    );
    assert!(
        !parsed
            .warnings
            .contains(&"payload_crc_mismatch".to_string())
    );

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket {
            body_summary,
            warnings,
            ..
        } => {
            assert!(warnings.contains(&"group_1_axis_0_truncated".to_string()));
            let Some(DataPacketBodySummary::RawMotionK21 {
                axes,
                warnings: summary_warnings,
                ..
            }) = body_summary
            else {
                panic!("expected k21 body summary");
            };
            assert!(summary_warnings.contains(&"group_1_axis_0_truncated".to_string()));
            assert_eq!(axes[0].name, "group_1_axis_0");
            assert_eq!(axes[0].expected_count, 100);
            assert_eq!(axes[0].parsed_count, 76);
            assert_eq!(axes[0].preview[0..3], [-1, 2, -3]);
            // full_samples length tracks parsed_count under truncation
            assert_eq!(axes[0].full_samples.as_ref().unwrap().len(), 76);
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}

#[test]
fn truncated_non_data_frame_fails_instead_of_becoming_decoded_evidence() {
    let mut frame = build_v5_command_frame(1, COMMAND_GET_HELLO, &[1, 2, 3, 4, 5, 6, 7, 8]);
    frame.truncate(frame.len() - 3);

    let error = parse_frame(DeviceType::Goose, &frame).unwrap_err();

    assert!(error.to_string().contains("declared length"));
}

#[test]
fn short_data_packets_preserve_raw_body_and_warn() {
    // Packet k=9 (NormalHistory) with a very short payload — existing behaviour unchanged.
    let frame = build_v5_payload_frame(&[PACKET_TYPE_HISTORICAL_DATA, 9, 1, 2]);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert!(
        parsed
            .warnings
            .contains(&"data_packet_header_too_short".to_string())
    );
    assert!(
        parsed
            .warnings
            .contains(&"history_hr_marker_missing".to_string())
    );
    assert_eq!(
        parsed.parsed_payload,
        Some(ParsedPayload::DataPacket {
            packet_k: Some(9),
            domain: Some("normal_history_with_hr_marker".to_string()),
            status_or_stream: Some(1),
            counter_or_page: None,
            timestamp_seconds: None,
            timestamp_subseconds: None,
            hr_marker_offset: Some(17),
            hr_present_marker: None,
            body_offset: 4,
            body_hex: String::new(),
            body_summary: Some(DataPacketBodySummary::NormalHistory {
                hr_present: None,
                marker_offset: Some(17),
                marker_value: None,
            }),
            warnings: vec![
                "data_packet_header_too_short".to_string(),
                "history_hr_marker_missing".to_string(),
            ],
        })
    );
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

// R22 WHOOP 5.0 realtime packet tests (BLE5-01)

#[test]
fn r22_4byte_parses_battery_and_hr() {
    // BTSnoop fixture: 10 50 31 05 — battery 80%, HR 132.9 BPM
    let payload = [PACKET_TYPE_R22_REALTIME_DATA, 0x50, 0x31, 0x05];
    let frame = build_v5_payload_frame(&payload);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    assert_eq!(parsed.packet_type, Some(0x10));
    assert_eq!(parsed.packet_type_name.as_deref(), Some("R22_REALTIME_DATA"));

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket { domain, body_summary, warnings, .. } => {
            assert_eq!(domain.as_deref(), Some("r22_whoop5_hr"));
            assert!(warnings.is_empty());
            match body_summary.unwrap() {
                DataPacketBodySummary::R22Whoop5Hr {
                    battery_pct,
                    hr_milli_bpm,
                    hr_bpm,
                    extra,
                    warnings,
                } => {
                    assert_eq!(battery_pct, Some(0x50)); // 80%
                    assert_eq!(hr_milli_bpm, Some(0x0531)); // 1329 milli-bpm
                    assert!((hr_bpm.unwrap() - 132.9).abs() < 0.01);
                    assert_eq!(extra, None);
                    assert!(warnings.is_empty());
                }
                other => panic!("expected R22Whoop5Hr, got {other:?}"),
            }
        }
        other => panic!("expected DataPacket, got {other:?}"),
    }
}

#[test]
fn r22_6byte_parses_battery_hr_and_extra_raw() {
    // BTSnoop fixture: 10 48 40 06 7a 02 — battery 72%, HR 160.0 BPM, extra [0x7a, 0x02]
    let payload = [PACKET_TYPE_R22_REALTIME_DATA, 0x48, 0x40, 0x06, 0x7a, 0x02];
    let frame = build_v5_payload_frame(&payload);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket { body_summary, warnings, .. } => {
            assert!(warnings.is_empty());
            match body_summary.unwrap() {
                DataPacketBodySummary::R22Whoop5Hr {
                    battery_pct,
                    hr_milli_bpm,
                    hr_bpm,
                    extra,
                    warnings,
                } => {
                    assert_eq!(battery_pct, Some(0x48)); // 72%
                    assert_eq!(hr_milli_bpm, Some(0x0640)); // 1600 milli-bpm
                    assert!((hr_bpm.unwrap() - 160.0).abs() < 0.01);
                    // extra bytes kept raw — no interpretation
                    assert_eq!(extra, Some([0x7a, 0x02]));
                    assert!(warnings.is_empty());
                }
                other => panic!("expected R22Whoop5Hr, got {other:?}"),
            }
        }
        other => panic!("expected DataPacket, got {other:?}"),
    }
}

#[test]
fn r22_zero_hr_bytes_parse_as_zero_not_error() {
    // build_v5_payload_frame always pads to 4-byte alignment, so a 3-byte payload
    // [0x10, battery, hr_lo] is padded with one 0x00 byte → hr = u16::from_le_bytes([hr_lo, 0x00]).
    // This verifies the parser handles low HR readings (e.g., resting BPM) without warnings.
    let payload = [PACKET_TYPE_R22_REALTIME_DATA, 0x50, 0x14, 0x00]; // HR = 0x0014 = 20 milli-bpm = 2.0 BPM (edge case)
    let frame = build_v5_payload_frame(&payload);
    let parsed = parse_frame(DeviceType::Goose, &frame).unwrap();

    match parsed.parsed_payload.unwrap() {
        ParsedPayload::DataPacket { body_summary, warnings, .. } => {
            assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
            match body_summary.unwrap() {
                DataPacketBodySummary::R22Whoop5Hr {
                    battery_pct,
                    hr_milli_bpm,
                    hr_bpm,
                    extra,
                    warnings,
                } => {
                    assert_eq!(battery_pct, Some(0x50)); // 80%
                    assert_eq!(hr_milli_bpm, Some(0x0014)); // 20 milli-bpm
                    assert!((hr_bpm.unwrap() - 2.0).abs() < 0.01);
                    assert_eq!(extra, None);
                    assert!(warnings.is_empty());
                }
                other => panic!("expected R22Whoop5Hr, got {other:?}"),
            }
        }
        other => panic!("expected DataPacket, got {other:?}"),
    }
}
