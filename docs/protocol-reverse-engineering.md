# Goose Protocol Reverse Engineering — How It Works

This document summarises how Goose connects to a WHOOP 5.0 band, extracts health data, and turns raw BLE packets into health metrics. It covers the BLE layer, the custom framing protocol, each data-collection mode, and the Swift↔Rust pipeline.

---

## What "Jailbreak" Means Here

There is no firmware exploit or iOS jailbreak involved. "Reverse engineering the WHOOP" means discovering WHOOP's proprietary Bluetooth LE protocol — frame structure, command numbers, CRC algorithms, and the connection handshake — and re-implementing it from scratch.

Two primary sources were used:

1. **WHOOP Android APK disassembly** — command numbers, parser names (e.g., "APK parser `nh0.l`"), and the command map were extracted by decompiling the Android app.
2. **OpenWhoop open-source project** (`github.com/bWanShiTong/openwhoop`, commit `55c5c1e`) — used as a behavioural reference for Gen4/Gen5 BLE protocol layout. Cited in `Rust/core/src/openwhoop_reference.rs`.

---

## BLE Connection Layer (`GooseBLEClient.swift`)

The app scans for BLE peripherals advertising either of two proprietary WHOOP services:

| UUID | Generation |
|------|------------|
| `fd4b0001-cce1-4033-93ce-002d5875f58a` | Gen5 (WHOOP 5.0) |
| `61080001-8d6d-82b8-614a-1c8cb0f8dcc6` | Gen4 (WHOOP 4.0) |

On top of those, standard GATT services are also used: `180D` (heart rate), `180F` (battery), `180A` (device info — firmware, hardware, model number).

Each service has a corresponding set of **characteristics** that map to roles:

| Role | Gen5 UUID | Gen4 UUID |
|------|-----------|-----------|
| Command write (to strap) | `fd4b0002` | `61080002` |
| Command response (from strap) | `fd4b0003` | `61080003` |
| Events from strap | `fd4b0004` | `61080004` |
| Data stream from strap | `fd4b0005` | `61080005` |
| Debug/Memfault | `fd4b0007` | `61080007` |

Device generation is detected in Swift by checking the characteristic UUID prefix (`610800*` → Gen4, everything else → Gen5/Goose). This `rustDeviceType` string is passed to the Rust parser on every frame.

---

## Frame Protocol (`GooseBLEClient+Parsing.swift`, `Rust/core/src/protocol.rs`)

Every packet on either generation uses a framed binary format starting with `0xAA`.

### Gen5 (WHOOP 5.0) — 8-byte header

```
[0xAA] [0x01] [LEN_LO] [LEN_HI] [0x00] [0x01] [CRC16_LO] [CRC16_HI]
[PAYLOAD bytes...]
[CRC32_B0] [CRC32_B1] [CRC32_B2] [CRC32_B3]
```

- **Header CRC**: CRC-16/Modbus over bytes 0–5.
- **Payload CRC**: CRC-32 (standard) over the payload.

### Gen4 (WHOOP 4.0) — 4-byte header

```
[0xAA] [LEN_LO] [LEN_HI] [CRC8]
[PAYLOAD bytes...]
[CRC32_B0] [CRC32_B1] [CRC32_B2] [CRC32_B3]
```

- **Header CRC**: CRC-8 over bytes 1–2.
- **Payload CRC**: CRC-32 (same algorithm as Gen5).

### Payload structure (both generations)

```
[packet_type: u8] [sequence: u8] [command_or_event: u8] [body...]
```

Known packet types:

| Byte | Constant name |
|------|---------------|
| 35 | COMMAND |
| 36 | COMMAND_RESPONSE |
| 37 | PUFFIN_COMMAND |
| 38 | PUFFIN_COMMAND_RESPONSE |
| 40 | REALTIME_DATA |
| 43 | REALTIME_RAW_DATA |
| 47 | HISTORICAL_DATA |
| 48 | EVENT |
| 49 | METADATA |
| 51 | REALTIME_IMU_DATA_STREAM |
| 52 | HISTORICAL_IMU_DATA_STREAM |
| 56 | PUFFIN_METADATA |

---

## Connection Handshake (`GooseHello.swift`)

After GATT service and characteristic discovery, the app writes a hardcoded **client hello frame** to the command characteristic:

```
aa 01 08 00 00 01 e6 71 23 01 91 01 36 3e 5c 8d
```

This is command `0x91` (145 = `GET_HELLO`), sequence `0x23`, built as a Gen5 V5 frame. The strap transitions to a "ready" state and begins accepting further commands. This exact byte sequence was captured from official WHOOP app Bluetooth traffic.

---

## Live Physiology Capture

To start continuous sensor streaming, the app sends a sequence of seven sensor commands to the strap. Each is a V5 command frame with a specific command number and boolean payload.

| # | Cmd | Name | Effect |
|---|-----|------|--------|
| 1 | 3 | `TOGGLE_REALTIME_HR_ON` | Start heart rate stream |
| 2 | 63 | `SEND_R10_R11_REALTIME_ON` | R10/R11 optical realtime packets |
| 3 | 106 | `TOGGLE_IMU_MODE_ON` | IMU (accelerometer/gyroscope) stream |
| 4 | 154 | `TOGGLE_PERSISTENT_R21_ON` | Persistent R21 optical summary |
| 5 | 107 | `ENABLE_OPTICAL_DATA_ON` | Raw PPG optical data |
| 6 | 108 | `TOGGLE_OPTICAL_MODE_ON` | Optical streaming mode |
| 7 | 153 | `TOGGLE_PERSISTENT_R20_ON` | Persistent R20 optical summary |

Stop is the same commands in reverse order with `false` payloads. Command numbers were extracted from the WHOOP APK command map.

The standard GATT heart rate characteristic (`2A37` on service `180D`) is also subscribed to. Heart rate measurements are parsed using the Bluetooth SIG standard format (flags byte, BPM, optional RR-intervals at 1/1024 s resolution).

HRV is computed locally from RR-intervals: RMSSD over a sliding 120-interval window, chunked into 30-interval chunks, averaged across the last 12 chunks.

---

## Historical Data Sync

When the app initiates a sync, it runs a three-phase protocol to download all data stored on the strap that has not been acknowledged yet.

**Phase 1 — GET_DATA_RANGE (cmd 34)**
Query the strap's page pointers: current page, oldest unread page, and end page. The response contains six `u32` words from which `pages_behind` is calculated to show how much unread history exists.

**Phase 2 — SEND_HISTORICAL_DATA (cmd 22)**
Instruct the strap to begin streaming. The strap sends:
- `HistoryStart` metadata (kind 1)
- Bursts of `HISTORICAL_DATA` (type 47) and `HISTORICAL_IMU_DATA_STREAM` (type 52) packets
- `HistoryEnd` metadata (kind 2) — contains the ACK payload seed
- `HistoryComplete` metadata (kind 3)

**Phase 3 — HISTORICAL_DATA_RESULT (cmd 23)**
ACK back to the strap. The ACK payload is constructed from bytes at offsets 13–20 of the `HistoryEnd` metadata payload, prefixed with a success byte. Without this ACK the strap re-sends the same data on the next sync.

A **high-frequency sync mode** (cmd 96/97) is also supported, which makes the strap push historical data at a shorter configurable interval during an active session.

---

## Additional Reverse-Engineered Commands

| Cmd | Name | Source |
|-----|------|--------|
| 10/11 | SET_CLOCK / GET_CLOCK | APK; auto-corrects >5 s drift |
| 66–69 | SET/GET/RUN/DISABLE_ALARM | APK; includes haptics pattern format |
| 40/42/44 | LED drive / TIA gain / bias offset | APK; optical sensor hardware reads |
| 84 | Body location and status | APK render-only command |
| 96/98 | Extended / pack battery info | APK |
| 121 | Device config value (keyed) | APK parser `nh0.o` |
| 128 | Feature flag value (keyed) | APK parser `nh0.p` |
| 132 | Research packet | APK command map |

---

## Swift ↔ Rust Data Pipeline

All packet parsing and health metric computation runs in a Rust static library (`libgoose_core.a`) compiled for iOS (`aarch64-apple-ios`, `aarch64-apple-ios-sim`). Swift calls into it via a JSON-over-C bridge (`GooseRustBridge`):

```swift
rust.request(method: "protocol.parse_frame_hex", args: ["device_type": "GOOSE", "frame_hex": hex])
```

Key Rust modules:

| Module | Role |
|--------|------|
| `protocol.rs` | Frame deframing, CRC validation, payload parsing |
| `metrics.rs` | HRV, recovery, strain, stress computations |
| `sleep_validation.rs` | Sleep stage detection from IMU + optical data |
| `recovery_rollup.rs` | Daily recovery score aggregation |
| `energy_rollup.rs` | Energy expenditure rollup |
| `calibration.rs` | Sensor calibration routines |
| `activity_candidates.rs` | Workout / activity detection from motion packets |
| `historical_sync.rs` | Historical packet ingestion pipeline |
| `health_sync.rs` | HealthKit read/write integration |
| `openwhoop_reference.rs` | Gen4/Gen5 UUID and characteristic role reference |
| `store.rs` | SQLite persistence layer |

Captured frames are written to SQLite via `CaptureFrameWriteQueue` (batched, off the BLE thread). All data is local-only by default.

---

## Sources

- WHOOP Android APK (decompiled) — command numbers and parser names
- OpenWhoop project (`github.com/bWanShiTong/openwhoop`) — protocol layout reference
- Bluetooth SIG GATT specifications — standard service and characteristic UUIDs
- Bluetooth traffic capture from the official WHOOP iOS app — hello frame bytes
