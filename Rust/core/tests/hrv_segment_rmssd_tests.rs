// Committable regression tests for the WHOOP 4.0 HRV path:
//   1. rmssd_segment_aware — the physiological RMSSD that only differences beats
//      within a capture window (never across) and applies a Malik artifact filter.
//   2. V24 normal_history RR-interval decode from a real owned-capture payload.
// No private database is required; all inputs are inline.
use goose_core::metrics::rmssd_segment_aware;
use goose_core::protocol::{parse_payload, DataPacketBodySummary, ParsedPayload};

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.01, "expected {b}, got {a}");
}

#[test]
fn segment_rmssd_single_clean_segment() {
    // diffs: -10, +20, -15 -> 100 + 400 + 225 = 725; mean 241.6667; sqrt = 15.5456
    let rmssd = rmssd_segment_aware(&[vec![820.0, 810.0, 830.0, 815.0]], 1).unwrap();
    approx(rmssd, (725.0_f64 / 3.0).sqrt());
}

#[test]
fn segment_rmssd_never_crosses_segment_boundaries() {
    // Two windows. Only the within-window pairs (-10 and -15) count; the 810->830
    // jump across windows must be ignored. 100 + 225 = 325; mean 162.5; sqrt 12.7475.
    let two = rmssd_segment_aware(&[vec![820.0, 810.0], vec![830.0, 815.0]], 1).unwrap();
    approx(two, (325.0_f64 / 2.0).sqrt());
    // The same beats as one contiguous window would (wrongly) include the cross jump.
    let one = rmssd_segment_aware(&[vec![820.0, 810.0, 830.0, 815.0]], 1).unwrap();
    assert!(one > two, "concatenating windows must not lower the diff count");
}

#[test]
fn segment_rmssd_rejects_malik_artifacts_and_out_of_range() {
    // >20% successive change (missed/double beat) -> the only pair is dropped -> None.
    assert!(rmssd_segment_aware(&[vec![800.0, 1700.0]], 1).is_none());
    // Interval outside the physiological 300..=2000 ms band -> dropped -> None.
    assert!(rmssd_segment_aware(&[vec![250.0, 260.0]], 1).is_none());
    // A clean pair survives alongside a rejected one.
    let rmssd = rmssd_segment_aware(&[vec![800.0, 1700.0], vec![900.0, 890.0]], 1).unwrap();
    approx(rmssd, 10.0); // only |890-900| = 10 survives
}

#[test]
fn segment_rmssd_needs_at_least_one_pair() {
    assert!(rmssd_segment_aware(&[vec![800.0]], 1).is_none());
    assert!(rmssd_segment_aware(&[], 1).is_none());
}

#[test]
fn v24_normal_history_decodes_rr_intervals() {
    // Real GEN_4 V24 normal_history payload captured over CoreBluetooth (HR marker 94,
    // three RR intervals). Proves the device ships beat-to-beat RR inside history frames.
    let payload_hex = "2f18055404ae011eb9216af86180544a015e034d011d048b0200000061640dff00d0e83c662635bd5c87f23e669a6d3f00003f46662635bd5c87f23e669a6d3f50027a02a0037e0252015002010c020c01000000002700010000000000000000";
    let bytes = hex::decode(payload_hex).expect("hex");
    let parsed = parse_payload(&bytes).expect("payload parses");
    let ParsedPayload::DataPacket {
        body_summary: Some(DataPacketBodySummary::NormalHistory {
            marker_value,
            rr_intervals_ms,
            ..
        }),
        ..
    } = parsed
    else {
        panic!("expected a NormalHistory data packet");
    };
    assert_eq!(marker_value, Some(94), "heart-rate marker");
    assert_eq!(rr_intervals_ms, vec![333, 1053, 651], "decoded RR intervals (ms)");
}
