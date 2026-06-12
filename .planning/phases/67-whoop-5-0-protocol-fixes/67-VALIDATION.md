---
phase: 67-whoop-5-0-protocol-fixes
nyquist_compliant: true
wave_0_complete: true
audited: 2026-06-12
---

# Phase 67 Validation: WHOOP 5.0 Protocol Fixes

## Test Infrastructure

| Framework | Config | Run Command |
|-----------|--------|-------------|
| Rust cargo test | Rust/core/Cargo.toml | `cargo test --manifest-path Rust/core/Cargo.toml` |

## Per-Requirement Coverage

| Requirement | Task | Test File | Test Name | Status |
|-------------|------|-----------|-----------|--------|
| BLE5-01 | R22 4-byte parse | protocol_tests.rs | r22_4byte_parses_battery_and_hr | COVERED |
| BLE5-01 | R22 6-byte parse | protocol_tests.rs | r22_6byte_parses_battery_hr_and_extra_raw | COVERED |
| BLE5-01 | R22 short payload | protocol_tests.rs | r22_zero_hr_bytes_parse_as_zero_not_error | COVERED |
| BLE5-01 | body_summary_kind routing | bridge_tests.rs | r22_import_produces_body_summary_kind_r22_whoop5_hr | COVERED |
| BLE5-01 | R22/R17 dedup | bridge_tests.rs | r22_shadows_r17_in_same_unix_second_for_hrv_features | ESCALATED — BLOCKER |
| BLE5-02 | v18 field decode | protocol_tests.rs | parses_v18_historical_body_fields | COVERED |
| BLE5-02 | v18 too-short | protocol_tests.rs | v18_too_short_yields_warning | COVERED |
| BLE5-02 | stale-clock guard | bridge_tests.rs | stale_device_clock_snaps_to_300s_grid_for_timestamp_confirmation | COVERED |
| BLE5-02 | EVENT type-48 bypass | bridge_tests.rs | event_packet_timestamp_bypasses_stale_clock_snap | COVERED |

## Manual-Only Items

None. All items are automated.

## Escalated — BLOCKER

### BLE5-01 R22/R17 same-second dedup

**Test:** `r22_shadows_r17_in_same_unix_second_for_hrv_features` in `bridge_tests.rs`

**Requirement:** When an R22 and an R17 frame share the same unix-second window, R22 is preferred and the R17 frame is dropped from the HRV trusted set.

**Actual behavior:** `trusted_feature_count` is 1 when it must be 0 — the R17 frame survives despite an R22 frame at the same captured_at second.

**Root cause:** `r22_shadowed_r17_frame_ids` in `metric_features.rs` (line 6371) calls `chrono_captured_at_to_unix` to extract the unix second from R22 frames' `captured_at` string. That function is a stub that unconditionally returns `Err(())` (lines 6416–6434), so `r22_seconds` is always empty and no R17 frame is ever suppressed.

**Fix applied (post-validation):**
1. `chrono_captured_at_to_unix` implemented using `crate::historical_sync::parse_rfc3339_utc_unix_ms` (made `pub(crate)`) — divides ms by 1000, casts to u32.
2. `r22_shadowed_r17_frame_ids` refactored to use `CaptureCorrelationReport.observations[].body_summary_kind` directly (avoids unreliable `parsed_payload_json` deserialization path) — signature changed to `(correlation: &CaptureCorrelationReport)`.
3. R17 dedup uses `captured_at` timestamp (R17 realtime frames have no device timestamp in payload).

**Test status:** PASSING — `r22_shadows_r17_in_same_unix_second_for_hrv_features` green.

## Sign-Off

Automated: 9 tests covering 9 requirements (4 in bridge_tests.rs + 5 in protocol_tests.rs)
Escalated: 0 — all gaps resolved
