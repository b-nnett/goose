# WHOOP 4.0 (Gen4) support

This document describes the changes that add **WHOOP 4.0 (Gen4)** Bluetooth
support to Goose, plus a set of stability/performance fixes found while testing
against a real WHOOP 4.0 band. Upstream targeted WHOOP 5.0 only.

Protocol details were verified byte-for-byte against the
[openwhoop](https://github.com/bWanShiTong/openwhoop) reference (commit
`55c5c1e`), which the repo already cites in `Rust/core/src/openwhoop_reference.rs`.

## Background

The **inbound** parser was already generation-aware (`Rust/core/src/protocol.rs`):
Gen4 uses a 4-byte frame header + CRC8, Gen5 an 8-byte header + CRC16-Modbus, and
the inner payload is identical across generations. What was missing was the
**outbound** half — the handshake and commands were hardcoded to the Gen5 frame
and gated on the `fd4b0002` command characteristic. So a WHOOP 4.0 would connect
over BLE but never complete the handshake, leaving every screen empty.

Key insight: the payload is generation-independent — only the frame wrapper and a
few command data bytes differ. So the port is mostly: add a Gen4 frame
builder/deframer, make the hello/gating/opcodes generation-aware, and reuse the
existing inbound parser.

## WHOOP 4.0 support

- **Gen4 frame builder + deframer** (`GooseBLEClient+Parsing.swift`):
  `crc8` (poly `0x07`), `buildGen4CommandFrame` (`[0xAA][len_lo][len_hi][crc8(len)]
  + payload + crc32(payload) LE`, no padding), a generation-aware `buildCommandFrame`
  dispatcher, and `gen4Frames`/`gen4Payload` + `strapFrames`/`strapPayload` for the
  inbound command/response state machines (clock/alarm/sensor/historical), which
  previously assumed the 8-byte Gen5 header.
- **Generation detection / gating** (`GooseBLEClient+Commands.swift`):
  `CommandGeneration`, `activeCommandGeneration` (by command-characteristic UUID
  prefix `61080002` vs `fd4b0002`), and `supportsStrapCommands` replacing the four
  `fd4b0002`-only `supportsV5*` gates.
- **Generation-aware hello** (`GooseBLEClient+UserActions.swift`): Gen4 sends
  `GetHelloHarvard` (cmd 35, data `[0x00]`); Gen5 keeps `GetHello` (cmd 145, `[0x01]`).
  Verified: the Gen4 hello frame is `aa0800a823002300ada86a2d`.
- **Realtime heart rate** (`GooseBLEClient.swift`): on Gen4, live HR is delivered
  over the standard BLE Heart Rate service (180D/2A37), so the realtime set sends
  only `TOGGLE_REALTIME_HR` (cmd 3). It deliberately does **not** send
  `SEND_R10_R11_REALTIME` (cmd 63), whose raw K10/K11 motion firehose bloated
  on-device storage to hundreds of MB in minutes and is not needed for HR.
- **Gen4 historical sync** (`GooseBLEClient+HistoricalCommands.swift`): preamble
  `set_time` → `get_name` → `history_start` (Gen4 `[0x00]` data, skipping
  `GET_DATA_RANGE`). Gen4 history-start is **fire-and-forget** — the band returns no
  command response, it just streams — so the sync waits on the data stream + idle
  completion (otherwise it times out with "SEND_HISTORICAL_DATA timed out").
  **`enter_high_freq_sync` (cmd 96) is deliberately NOT sent**: with it, a real WHOOP
  4.0 streams the high-frequency raw-motion path (REALTIME_RAW_DATA k10/k11) and never
  the normal-history records; without it the band returns `HISTORICAL_DATA` (type 47,
  `normal_history`) carrying the per-sample heart-rate markers. Verified on hardware:
  removing cmd 96 took type-47 frames from 0 to 750 in one short sync.

## Fixes (found against a real WHOOP 4.0)

- **`unsupported device_type: GEN4`** — `bridge.rs::parse_device_type` only accepted
  the string `"GEN_4"`, but Swift sends `"GEN4"`, so every proprietary Gen4 frame
  was rejected (1959 failures in one capture). Now `"GEN4"` is accepted, plus the
  three `expected_device_type()` helpers in `capture_correlation.rs`,
  `capture_import.rs`, `fixtures.rs`.
- **FFI panic safety** — the C-FFI dispatch (`goose_bridge_handle_json`) had no
  `catch_unwind` and the release profile used `panic = "abort"`, so any panic
  crashed the whole app (or was UB across `extern "C"`). The dispatch is now wrapped
  in `catch_unwind` and the release profile uses `panic = "unwind"`, turning a panic
  into a structured JSON error.
- **No auto-capture on connect** — connecting auto-started a 12-hour, full-rate
  packet capture that persisted every frame to SQLite (`GooseAppModel+Lifecycle.swift`),
  the dominant source of UI lag and unbounded DB growth. It is now opt-in.
- **Lightweight default export** — the default raw export pulled raw bytes and the
  large `sensor_samples` table fully into memory, causing OOM crashes
  (`MoreDataStore.swift`). Defaults now exclude raw bytes and `sensor_samples`; the
  full export remains available explicitly.
- **Bounded storage** — `DEFAULT_RAW_EVIDENCE_PAYLOAD_RETENTION_LIMIT_BYTES` was
  512 MB and the live write path passed `compact_raw_payloads: false`, so a WHOOP
  history backlog (pulled oldest-first) could grow the DB into the gigabytes. The
  limit is now 24 MB, the live write path compacts, and a single sync is capped at
  `historicalSyncPacketCap` (6000) packets per pass.

## What works / what doesn't on Gen4

With cmd 96 removed (above) the band streams its `normal_history` (type 47) records.
Decoding the **V12/V24** body of those frames showed they carry far more than heart
rate: the strap runs its own DSP and embeds RR intervals, SpO2 red/ir, skin-temperature
and respiratory values in every frame (offsets per the openwhoop reference, confirmed by
the heart-rate marker). All of the below was verified by pulling the app's own database
off a device (`devicectl copy appDataContainer`) and replaying the exact bridge calls
the UI makes.

- **Decoded and user-visible (no calibration needed):**
  - **HRV (RMSSD ~62 ms)** — from the device's own beat-to-beat RR intervals. The metric
    pipeline re-parses the raw `payload_hex` (parser-version independent), and RMSSD is
    computed *segment-aware* — successive differences only within a capture window, never
    across — which fixes a 452 ms → 42 ms error and needs no calibration (RR ms → RMSSD ms).
  - **Respiratory rate (~15.4 rpm)** — V24 `body[63]`, device-native `÷200` scale,
    self-validated as physiological; promoted with an honest `no_reference` provenance flag.
  - **Resting HR (~80 bpm)** — HR marker + low-motion filter.
  - **Strain (~12/21)** — from HR zones; needs no multi-day baseline, so it works from a
    single session. (Behind the "Run Packet-Derived Scores" trigger, now auto-started.)
- **Decoded but intentionally gated (need a reference the test device lacked):**
  - **SpO2** — only the stable optical DC levels are present (no AC/pulsatile ratio);
    an absolute % needs a factory ratio-of-ratios calibration curve.
  - **Skin temperature** — the candidate field is the *most* variable in the frame and the
    raw→°C scale can't be validated without a WHOOP reference; openwhoop itself marks its
    units unverified.
- **Needs more data, not more code:**
  - **Recovery** is wired (`goose_recovery_v0`) but baseline-relative — it activates once a
    few days of HRV/RHR baseline accumulate.
  - **Sleep staging** needs an overnight capture plus a classifier with reference labels.

### Storage / performance

The cached `parsed_payload_json` carried a `body_hex` field that exactly duplicated the
`payload_hex` stored next to it — ~43 MB of a 147 MB database on the high-volume raw-motion
stream, scaling with wear time. `insert_decoded_frame` now drops it for large frames
(metrics are byte-identical; only the debug timeline used it, for small frames), which
bounds multi-day captures and makes the metric batch ~27% faster.

## Tests

- `gen4_outbound_verification.rs` + `gen4_protocol_tests.rs` (24 tests): Gen4 frame
  round-trip with CRC validation, `"GEN4"`/`"GEN_4"` acceptance via the bridge, 4- vs
  8-byte header geometry, packet-type classification, panic-safety on malformed/truncated/
  garbage input, structured bridge errors.
- `gen4_v24_decode_test.rs`: the V12/V24 DSP-sensor decode (SpO2, skin temp, respiratory,
  signal quality) against a real hardware frame.
- `hrv_segment_rmssd_tests.rs`: `rmssd_segment_aware` (300/2000 ms band, Malik 20% rule,
  never-difference-across-windows) and V24 RR-interval decode from a real owned capture.
- `store_tests.rs::large_cached_body_hex_is_dropped_small_is_kept`: the body_hex storage
  compaction.

Run: `cd Rust/core && cargo test`
