# Phase 67: WHOOP 5.0 Protocol Fixes - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 3 (protocol.rs modify, historical_sync.rs modify, protocol_tests.rs modify/extend)
**Analogs found:** 3 / 3

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `Rust/core/src/protocol.rs` | parser/protocol | transform | self (existing PACKET_TYPE_* + parse_v24_body_summary pattern) | exact |
| `Rust/core/src/historical_sync.rs` | service | batch/transform | self (existing timestamp evidence pipeline) | exact |
| `Rust/core/tests/protocol_tests.rs` | test | batch | `tests/protocol_tests.rs` existing synthetic fixture pattern | exact |

## Pattern Assignments

---

### `Rust/core/src/protocol.rs` — BLE5-01: R22 constant + parser

**Analog:** Existing `PACKET_TYPE_*` block (lines 5–21) and `parse_v24_body_summary` (lines 680–779)

**Constant definition pattern** (lines 5–21):
```rust
pub const PACKET_TYPE_COMMAND: u8 = 35;
pub const PACKET_TYPE_REALTIME_DATA: u8 = 40;
pub const PACKET_TYPE_HISTORICAL_DATA: u8 = 47;
pub const PACKET_TYPE_EVENT: u8 = 48;
// ...
```
New constant follows the same pattern — add immediately after line 21:
```rust
pub const PACKET_TYPE_R22_REALTIME_DATA: u8 = 0x10;  // = 16; WHOOP 5.0 BLE handle 0x0022
```

**`packet_type_name` arm pattern** (lines 398–418):
```rust
pub fn packet_type_name(packet_type: u8) -> Option<&'static str> {
    Some(match packet_type {
        PACKET_TYPE_COMMAND => "COMMAND",
        PACKET_TYPE_REALTIME_DATA => "REALTIME_DATA",
        // ...
        PACKET_TYPE_PUFFIN_METADATA => "PUFFIN_METADATA",
        _ => return None,
    })
}
```
Add before `_ => return None`:
```rust
PACKET_TYPE_R22_REALTIME_DATA => "R22_REALTIME_DATA",
```

**`parse_payload` match arm pattern** (lines 431–452):
```rust
fn parse_payload(payload: &[u8]) -> Option<ParsedPayload> {
    let packet_type = *payload.first()?;
    match packet_type {
        PACKET_TYPE_REALTIME_DATA
        | PACKET_TYPE_REALTIME_RAW_DATA
        | PACKET_TYPE_HISTORICAL_DATA
        | PACKET_TYPE_REALTIME_IMU_DATA_STREAM
        | PACKET_TYPE_HISTORICAL_IMU_DATA_STREAM => Some(parse_data_packet_payload(payload)),
        _ => Some(ParsedPayload::Raw { ... }),
    }
}
```
R22 needs its own arm (not routed through `parse_data_packet_payload` — it has a different header layout). Add before `_ =>`:
```rust
PACKET_TYPE_R22_REALTIME_DATA => Some(parse_r22_payload(payload)),
```

**`is_partial_data_packet_type_allowed` pattern** (lines 454–463):
```rust
fn is_partial_data_packet_type_allowed(packet_type: u8) -> bool {
    matches!(
        packet_type,
        PACKET_TYPE_REALTIME_DATA
            | PACKET_TYPE_REALTIME_RAW_DATA
            | PACKET_TYPE_HISTORICAL_DATA
            | PACKET_TYPE_REALTIME_IMU_DATA_STREAM
            | PACKET_TYPE_HISTORICAL_IMU_DATA_STREAM
    )
}
```
Add `| PACKET_TYPE_R22_REALTIME_DATA` to the `matches!` arms.

**`parse_r22_payload` — new function, model on `parse_r17_body_summary`** (lines 583–611):
```rust
fn parse_r17_body_summary(payload: &[u8]) -> (Option<DataPacketBodySummary>, Vec<String>) {
    let flags = read_u16_le(payload, 13);
    // ...
    (Some(DataPacketBodySummary::R17OpticalOrLabradorFiltered { ... }), warnings)
}
```
New function uses `ParsedPayload::Raw` or a new `DataPacketBodySummary` variant. Copy the guard-then-extract pattern:
```rust
fn parse_r22_payload(payload: &[u8]) -> ParsedPayload {
    let mut warnings = Vec::new();
    if payload.len() < 4 {
        warnings.push("r22_payload_too_short".to_string());
        return ParsedPayload::Raw {
            data_offset: payload.len(),
            data_hex: hex::encode(payload),
            warnings,
        };
    }
    let battery_pct = payload[1];
    let hr_milli_bpm = u16::from_le_bytes([payload[2], payload[3]]);
    let hr_bpm = hr_milli_bpm as f32 / 10.0;
    let extra = if payload.len() >= 6 {
        Some([payload[4], payload[5]])
    } else {
        None
    };
    // Return as DataPacket or a new ParsedPayload variant per planner decision
    // body_summary_kind: "r22_whoop5_hr"
    // ...
}
```

**`data_packet_domain` — add R22 domain string** (lines 906–919):
```rust
fn data_packet_domain(packet_k: u8) -> Option<&'static str> {
    Some(match packet_k {
        17 => "r17_optical_or_labrador_filtered",
        // ...
    })
}
```
Not applicable for R22 since it does not go through `parse_data_packet_payload` / `packet_k`. Domain string `"r22_whoop5_hr"` lives in the body summary kind directly.

**v18 split — model on `parse_v24_body_summary`** (lines 680–779):
```rust
fn parse_v24_body_summary(payload: &[u8]) -> (Option<DataPacketBodySummary>, Vec<String>) {
    let data = payload.get(3..).unwrap_or(&[]);  // skip 3-byte data-packet header
    let mut warnings = Vec::new();

    if data.len() < 77 {
        warnings.push("v24_payload_too_short".to_string());
        return (Some(DataPacketBodySummary::V24History { hr: None, ... }), warnings);
    }

    let hr = data.get(14).copied();
    let rr_count = data.get(15).copied().unwrap_or(0) as usize;
    let rr_count = rr_count.min(4);
    let rr_intervals_ms = (0..rr_count)
        .filter_map(|i| {
            let o = 16 + 2 * i;
            read_u16_le(data, o)
        })
        .filter(|&v| v != 0)
        .collect::<Vec<u16>>();

    let gravity_x = read_f32_le(data, 33);
    let gravity_y = read_f32_le(data, 37);
    let gravity_z = read_f32_le(data, 41);
    let skin_temp_raw = read_u16_le(data, 65);
    // ...
}
```
`parse_v18_body` follows the **same structure** but with v18-specific offsets from CONTEXT.md:
- `data = payload.get(3..).unwrap_or(&[])` (same 3-byte skip — packet_type + packet_k + status)
- Minimum length guard → push warning + return early with None fields
- HR at offset 22, rr_count at 23, RR at 24+i×2 (cap at 4, skip zeros)
- gravity_x/y/z at offsets 45/49/53 (f32 LE via `read_f32_le`)
- step_motion_counter at offset 57 (u16 LE via `read_u16_le`)
- skin_temp_raw at offset 73 (u16 LE via `read_u16_le`); convert: `raw as f32 / 128.0`; gate: `5.0..=45.0`

**`parse_data_packet_body_summary` match arm** (lines 566–581):
```rust
match packet_k {
    7 | 9 | 12 | 18 => (
        Some(DataPacketBodySummary::NormalHistory {
            hr_present: hr_present_marker.map(|marker| marker != 0),
            marker_offset: hr_marker_offset,
            marker_value: hr_present_marker,
        }),
        Vec::new(),
    ),
    17 => parse_r17_body_summary(payload),
    24 => parse_v24_body_summary(payload),
    _ => (None, Vec::new()),
}
```
Split `18` out of the `7 | 9 | 12 | 18` arm:
```rust
7 | 9 | 12 => (Some(DataPacketBodySummary::NormalHistory { ... }), Vec::new()),
18 => parse_v18_body(payload),
```

**`history_hr_marker_offset` — note** (lines 921–928):
```rust
fn history_hr_marker_offset(packet_k: u8) -> Option<usize> {
    match packet_k {
        7 => Some(27),
        9 | 12 | 24 => Some(17),
        18 => Some(14),
        _ => None,
    }
}
```
Keep `18 => Some(14)` — v18 still has an HR marker at offset 14 even though a full `parse_v18_body` is added.

**`read_f32_le` helper** (lines 675–678):
```rust
fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
```
Already present — use directly in `parse_v18_body`.

---

### `Rust/core/src/historical_sync.rs` — BLE5-02: stale-clock dedup + EVENT type-48 bypass

**Analog:** Existing timestamp evidence pipeline (lines 312–1910 range); timestamp converter implicit in sample_time field assignment.

The file uses `sample_time` (RFC3339 string) derived from device timestamps. The stale-clock fix and EVENT bypass need to be located at the point where device_timestamp_seconds is converted to a wall-clock time.

**Pattern to search for the conversion site:**
```bash
grep -n "device_timestamp_seconds\|sample_time_source.*device_timestamp\|to_rfc3339\|unix.*timestamp\|timestamp.*offset" \
  Rust/core/src/historical_sync.rs
```

**Stale-clock guard pattern** — model on existing guard patterns throughout the file:
```rust
// Guard before applying offset:
if (wall_clock_ref as i64 - device_clock_ref as i64).unsigned_abs() > 86_400 {
    // Snap to 300-second grid
    let snapped = (device_timestamp_seconds / 300) * 300;
    // use snapped as the wall-clock seconds
}
```

**EVENT type-48 bypass pattern** — model on the `packet_type` checks already present. At the timestamp conversion point, add a branch:
```rust
// EVENT packets carry native RTC unix seconds — bypass the device-epoch offset.
if packet_type == PACKET_TYPE_EVENT {
    // Use device_timestamp_seconds directly as wall-clock unix seconds
} else {
    // Normal offset conversion
}
```
`PACKET_TYPE_EVENT` is already defined in `protocol.rs` as `48u8` and imported throughout the codebase.

---

### `Rust/core/tests/protocol_tests.rs` — synthetic fixture tests for R22 and v18

**Analog:** Existing test pattern for `normal_history` (lines ~203–231) and `parses_r17_optical_body_offsets_and_signed_sample_stats` (lines 233–296).

**Synthetic payload construction pattern** (lines 203–230):
```rust
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
        ParsedPayload::DataPacket { body_summary, warnings, .. } => {
            assert!(warnings.is_empty());
            assert_eq!(body_summary, Some(DataPacketBodySummary::NormalHistory { ... }));
        }
        other => panic!("expected data packet, got {other:?}"),
    }
}
```

**R22 fixture test** — use `build_v5_payload_frame` with a hand-crafted payload matching the BTSnoop sample from CONTEXT.md:
```rust
#[test]
fn parses_r22_4byte_realtime_sample() {
    // BTSnoop confirmed: 10 50 31 05 = battery 80%, HR 132.9 BPM
    let payload = vec![0x10u8, 0x50, 0x31, 0x05];
    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();
    // assert battery_pct == 80, hr_bpm ≈ 132.9
}
```

**v18 fixture test** — same approach, build a synthetic payload with known field values at v18 offsets:
```rust
#[test]
fn parses_v18_historical_body_fields() {
    // payload[0] = PACKET_TYPE_HISTORICAL_DATA, payload[1] = 18, payload[2] = status
    // Set HR at body offset 22, rr_count at 23, gravity at 45/49/53, skin_temp at 73
    let mut payload = vec![0u8; 90];  // large enough for all v18 fields
    payload[0] = PACKET_TYPE_HISTORICAL_DATA;
    payload[1] = 18;
    payload[3 + 22] = 75;  // HR = 75 BPM  (body starts at payload[3])
    // ...
    let parsed = parse_frame(DeviceType::Goose, &build_v5_payload_frame(&payload)).unwrap();
    // assert V18History fields match expected values
}
```

**Imports at top of test file** (lines 1–10):
```rust
use goose_core::protocol::{
    DataPacketBodySummary, DeviceType, I16SeriesSummary,
    PACKET_TYPE_HISTORICAL_DATA, PACKET_TYPE_REALTIME_DATA, PACKET_TYPE_REALTIME_RAW_DATA,
    ParsedPayload,
    build_v5_command_frame, build_v5_payload_frame, parse_frame, parse_frame_hex,
};
```
Add `PACKET_TYPE_R22_REALTIME_DATA` to the import list when writing R22 tests.

---

## Shared Patterns

### `read_u16_le` / `read_u32_le` / `read_f32_le` helpers
**Source:** `Rust/core/src/protocol.rs` lines 930–944, 675–678
**Apply to:** `parse_r22_payload`, `parse_v18_body`
```rust
fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]))
}
fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
```

### Payload too-short guard
**Source:** `Rust/core/src/protocol.rs` lines 684–710 (`parse_v24_body_summary`)
**Apply to:** `parse_r22_payload`, `parse_v18_body`
```rust
if data.len() < MINIMUM {
    warnings.push("vXX_payload_too_short".to_string());
    return (Some(DataPacketBodySummary::VXXHistory { hr: None, ... }), warnings);
}
```

### RR interval parse with cap and zero-filter
**Source:** `Rust/core/src/protocol.rs` lines 714–722 (`parse_v24_body_summary`)
**Apply to:** `parse_v18_body`
```rust
let rr_count = data.get(OFFSET).copied().unwrap_or(0) as usize;
let rr_count = rr_count.min(4);
let rr_intervals_ms = (0..rr_count)
    .filter_map(|i| read_u16_le(data, BASE + 2 * i))
    .filter(|&v| v != 0)
    .collect::<Vec<u16>>();
```

### body_summary_kind string used in trusted_frames pipeline
**Source:** `Rust/core/src/metric_features.rs` line 1867; `capture_correlation.rs` lines 576–584
**Apply to:** bridge dispatch and `trusted_frames_for_summary_kinds` call site
```rust
// In metric_features.rs where R17 is listed:
trusted_frames_for_summary_kinds(correlation, &["r17_optical_or_labrador_filtered"]);
// Add r22 alongside:
trusted_frames_for_summary_kinds(correlation, &["r17_optical_or_labrador_filtered", "r22_whoop5_hr"]);
```

### Store insert pattern for batch biometric data
**Source:** `Rust/core/src/store.rs` lines 6789–6808 (v24 batch insert)
**Apply to:** v18 field persistence from `parse_v18_body` caller in bridge.rs
```rust
store.conn.execute(
    "INSERT OR IGNORE INTO skin_temp_samples (device_id, ts, raw, contact) VALUES (?1, ?2, ?3, ?4)",
    params![device_id, ts, raw, contact],
)?;
// rr_intervals:
store.conn.execute(
    "INSERT OR IGNORE INTO rr_intervals (device_id, ts, interval_ms) VALUES (?1, ?2, ?3)",
    params![device_id, ts_for_rr, rr_ms],
)?;
```

## No Analog Found

None — all three files have direct analogs in the existing codebase.

## Metadata

**Analog search scope:** `Rust/core/src/protocol.rs`, `Rust/core/src/historical_sync.rs`, `Rust/core/src/metric_features.rs`, `Rust/core/src/capture_correlation.rs`, `Rust/core/src/store.rs`, `Rust/core/tests/protocol_tests.rs`
**Files scanned:** 6
**Pattern extraction date:** 2026-06-12
