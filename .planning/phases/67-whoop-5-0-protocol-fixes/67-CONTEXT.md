# Phase 67: WHOOP 5.0 Protocol Fixes - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix the two silent protocol gaps that prevent WHOOP 5.0 users from receiving any realtime metrics or historical data. Both fixes are purely in Rust — no Swift changes required.

1. **BLE5-01 (R22 realtime):** Add `PACKET_TYPE_R22_REALTIME_DATA: u8 = 0x10` (= 16 decimal) to `protocol.rs`. Parse the 4-byte variant (battery_pct + HR milli-bpm ÷10) and 6-byte variant (same + extra bytes kept raw). Map to body_summary_kind `"r22_whoop5_hr"`. Add to `trusted_frames_for_summary_kinds` alongside R17. R22 has priority over R17 when both are received in the same second (WHOOP 5.0 streams R17 on 0x0027 AND R22 on 0x0022 simultaneously — dedup by second-granularity window, R22 preferred).

2. **BLE5-02 (v18 historical + stale-clock):** Split `18` out of the `7 | 9 | 12 | 18` arm in `protocol.rs` → `parse_v18_body(payload)`. Decode all fields that have existing SQLite tables: HR (offset 22), RR intervals (offset 24+, count at 23), gravity_x/y/z (offsets 45/49/53), skin_temp_raw (offset 73, convert: `degC = raw / 128.0`), step_motion_counter (offset 57). Feed into `insert_skin_temp_sample`, `insert_rr_interval`, `insert_gravity_sample`, `insert_step_sample`. In `historical_sync.rs`, add stale-clock dedup: if `|wallClockRef - deviceClockRef| > 86400s`, snap timestamps to 300s grid. Also add EVENT type-48 timestamp bypass (EVENT timestamps are native RTC unix seconds — must not pass through the device-epoch→wall-clock offset converter).

</domain>

<decisions>
## Implementation Decisions

### R22 Parsing Scope
- Extract both battery_pct (Byte 1) and HR milli-bpm (Bytes 2-3 LE, ÷10 = BPM) from R22
- Leave 6-byte variant's `extra` field as raw bytes — purpose TBD, no interpretation now
- New constant: `PACKET_TYPE_R22_REALTIME_DATA: u8 = 0x10` (follows existing PACKET_TYPE_* naming convention)
- New body_summary_kind: `"r22_whoop5_hr"` (parallel to `"r17_optical_or_labrador_filtered"`)
- Add to `packet_type_name` match arm: `PACKET_TYPE_R22_REALTIME_DATA => "R22_REALTIME_DATA"`

### R17/R22 Dual-Stream Dedup
- R22 has priority over R17 when both arrive in the same second-granularity window
- Dedup at second-granularity (same unix timestamp window)
- Wire into same `trusted_frames_for_summary_kinds` pipeline — add `"r22_whoop5_hr"` alongside R17 variants

### v18 Historical Decode Scope
- Implement ALL fields that have existing SQLite tables: HR, RR, gravity_x/y/z, skin_temp, step_counter
- Stale-clock threshold: 86400s → snap to 300s grid (seed-confirmed value)
- EVENT type-48 timestamp bypass: included in this phase — add bypass in same `historical_sync.rs`
- Test fixtures: synthetic hexdump samples from BTSnoop data in seed (no real device capture required)

### Claude's Discretion
- Exact location of R17/R22 dedup logic within protocol.rs vs. bridge.rs dispatch — choose whichever avoids duplication
- Whether `parse_v18_body` is a pub fn or pub(crate) fn
- Test module structure (separate test file vs. inline `#[cfg(test)]` block)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `protocol.rs`: `packet_type_name()` function — add R22 arm here
- `protocol.rs`: `parse_payload()` match — add `PACKET_TYPE_R22_REALTIME_DATA` arm
- `protocol.rs`: `is_partial_data_packet_type_allowed()` — may need R22 added
- `protocol.rs`: existing `7 | 9 | 12 | 18` arm at line 566-579 — split 18 out
- `historical_sync.rs`: timestamp converter — add 86400s check and EVENT bypass
- Existing tables: `skin_temp_samples`, `rr_interval_samples`, `gravity2_samples`, `step_counter_samples` — feed v18 fields here
- `trusted_frames_for_summary_kinds`: add `"r22_whoop5_hr"` alongside R17 variants

### Established Patterns
- Packet type constants defined at top of `protocol.rs` (lines 6-21): `pub const PACKET_TYPE_*: u8 = N`
- `parse_payload()` uses match on first payload byte → `ParsedPayload` enum variants
- body_summary_kind strings used in bridge dispatch and `trusted_frames_for_summary_kinds`
- Tests live in `Rust/core/tests/protocol_tests.rs` — synthetic hex fixtures preferred

### Integration Points
- `NotificationFrameParser.swift` → Rust bridge → `parse_payload()` — R22 lands here
- `GooseBLEClient+HistoricalCommands.swift` → Rust bridge → historical decode pipeline — v18 lands here
- No Swift file changes needed in either case

</code_context>

<specifics>
## Specific Ideas

- R22 packet format confirmed via BTSnoop (issue #92 — darylbleach, WHOOP 5.0):
  - 4-byte: `[0x10, battery_pct, hr_lo, hr_hi]` — HR = (hr_hi << 8 | hr_lo) / 10.0
  - 6-byte: same + `[extra_lo, extra_hi]` — extra kept raw
  - Sample: `10 50 31 05` = battery 80%, HR 132.9 BPM ✓
- v18 field layout from NOOP cross-reference (Packages/WhoopProtocol/Sources/WhoopProtocol/Interpreter.swift):
  - Offset 15: u32 LE unix timestamp
  - Offset 22: u8 HR BPM
  - Offset 23: u8 rr_count (likely ≤4, add guard)
  - Offset 24+i×2: u16 LE RR milliseconds
  - Offset 41: f32 LE dynamic_acceleration (gate: 0.0..8.0 g)
  - Offsets 45/49/53: f32 LE gravity_x/y/z
  - Offset 57: u16 LE step_motion_counter
  - Offset 73: u16 LE skin_temp_raw (÷128.0 = °C, gate 5..45°C)
- skin_temp conversion is the ONLY client-side transform needed
- Do NOT decode v26 PPG (24×i16 waveform at offset 27) — NOOP skips it, not biometric data

</specifics>

<deferred>
## Deferred Ideas

- R22 6-byte `extra` field interpretation (HRV ms, SpO2, or secondary optical) — needs second BTSnoop capture with ground truth
- v18 fields without existing SQLite tables (resp_samples — check if table exists before implementing)
- R22 variant numbering (v2–v8 from config keys) — different payload layouts per variant — leave as TBD
- v26 PPG waveform decode — NOOP skips it, defer indefinitely

</deferred>
