---
plan: 67-01
phase: 67
status: complete
completed: 2026-06-12
---

# Plan 67-01 Summary: R22 WHOOP 5.0 Realtime Parser (BLE5-01)

## What Was Built

Added full R22 packet type support to the Goose Rust core, fixing the silent gap where WHOOP 5.0 devices streaming type `0x10` produced no realtime metrics.

**Files modified:**
- `Rust/core/src/protocol.rs` — added `PACKET_TYPE_R22_REALTIME_DATA: u8 = 0x10`, `DataPacketBodySummary::R22Whoop5Hr` variant, `parse_r22_payload()` function, routing arms in `parse_payload()`, `packet_type_name()`, and `is_partial_data_packet_type_allowed()`
- `Rust/core/src/bridge.rs` — added `R22Whoop5Hr` match arm in body_summary_kind dispatch and decoded-stream extraction (pushes HR into the same realtime pipeline as R17)
- `Rust/core/src/capture_correlation.rs` — added `R22Whoop5Hr` match arm
- `Rust/core/src/export.rs` — added `R22Whoop5Hr` match arm
- `Rust/core/src/metric_features.rs` — added `"r22_whoop5_hr"` to `trusted_frames_for_summary_kinds` alongside `"r17_optical_or_labrador_filtered"` (R22 priority in same-second dedup)
- `Rust/core/tests/protocol_tests.rs` — added 3 R22 fixture tests (4-byte variant, 6-byte variant with extra bytes, zero-HR edge case)

## Verification

- `cargo test --test protocol_tests` — 19 passed, 0 failed
- No Swift files changed (confirmed via git diff)
- `PACKET_TYPE_R22_REALTIME_DATA` constant at 0x10, `packet_type_name` returns `"R22_REALTIME_DATA"`, `domain = "r22_whoop5_hr"` in DataPacket output

## Deviations

None. All locked decisions from CONTEXT.md implemented exactly:
- battery_pct + HR from 4-byte variant ✓
- extra bytes kept raw as `Option<[u8; 2]>` ✓
- `"r22_whoop5_hr"` body_summary_kind ✓
- R22 priority over R17 via trusted_frames ✓

Note: The "too-short" early-return guard in `parse_r22_payload` (len < 4) cannot be triggered via `build_v5_payload_frame` because the builder always pads to 4-byte alignment. The guard remains as defensive code for corrupted BLE frames. Tests cover the 4-byte (minimum valid) and 6-byte (with extra) variants instead.
