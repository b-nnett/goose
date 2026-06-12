---
plan: 67-02
phase: 67
status: complete
completed: 2026-06-12
---

# Plan 67-02 Summary: v18 Historical Decode + Stale-Clock Fix (BLE5-02)

## What Was Built

**Task 1 — protocol.rs v18 body parser:**
- Split `18` out of the `7 | 9 | 12 | 18` NormalHistory arm → `18 => parse_v18_body(payload)`
- New `DataPacketBodySummary::V18History` variant: `hr: Option<u8>`, `rr_intervals_ms: Vec<u16>`, `gravity_x/y/z: Option<f32>`, `skin_temp_raw: Option<u16>`, `step_motion_counter: Option<u16>`, `warnings: Vec<String>`
- `parse_v18_body()`: skips 3-byte header, reads all fields at confirmed NOOP offsets, guards rr_count ≤ 4, applies minimum-length check (75 body bytes needed for skin_temp at data[73])

**Task 2 — bridge.rs + ecosystem persistence:**
- `body_summary_kind()` arm: `V18History { .. } => "v18_history"`
- Main decode match: V18History arm pushing HR, RR, gravity, skin_temp (with 5–45°C gate), step_counter to existing stream vecs
- `capture_correlation.rs`, `export.rs`, `metric_features.rs` — all required match arms added

**Task 3 — historical_sync.rs stale-clock + EVENT bypass + tests:**
- `timestamp_packet_confirmed_rows()`: stale-clock guard — when `|captured_at - device_timestamp| > 86_400s`, snaps device_timestamp_seconds to 300-second grid before comparison
- EVENT packet bypass: `packet_kind.contains("event")` exempts EVENT packets from the grid snap (EVENT timestamps are native RTC unix seconds)
- 2 new tests: `parses_v18_historical_body_fields` (full field decode), `v18_too_short_yields_warning`

## Files Modified

- `Rust/core/src/protocol.rs` — V18History variant + parse_v18_body()
- `Rust/core/src/bridge.rs` — V18History persistence arm
- `Rust/core/src/capture_correlation.rs` — V18History match arm
- `Rust/core/src/export.rs` — V18History match arm
- `Rust/core/src/metric_features.rs` — V18History wiring
- `Rust/core/src/historical_sync.rs` — stale-clock guard + EVENT bypass
- `Rust/core/tests/protocol_tests.rs` — v18 fixture tests (21 total, all pass)

## Verification

- `cargo test --test protocol_tests` — 21 passed, 0 failed ✓
- `cargo build` — clean, no warnings ✓
- No Swift files changed ✓
- `grep -n "86_400" historical_sync.rs` → lines 1907 + 1914 ✓
- `grep -c "fn parses_v18\|fn v18_too_short" protocol_tests.rs` → 2 ✓

## Deviations

- EVENT bypass uses `packet_kind.contains("event")` instead of `PACKET_TYPE_EVENT` constant — `HistoricalSyncTimestampEvidence` has no `packet_type: u8` field; packet_kind (String) is the correct discriminator at this layer.
- `timestamp_packet_confirmed_rows` is a validation function, not a write-path function; the stale-clock guard bounds the influence of corrupt RTC on the confirmation logic (prevents stale frames from being "confirmed" with wrong timestamps).
- Existing test `parses_history_packet_stable_header_and_hr_marker` updated: now correctly expects V18History (v18 split from NormalHistory arm) with v18_payload_too_short warning for the short test payload.
