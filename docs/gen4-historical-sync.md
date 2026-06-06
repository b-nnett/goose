# Gen4 (WHOOP 4.0) Historical Sync — Implementation & Approach

## Goal

Bring Goose's BLE historical-sync flow to parity for WHOOP 4.0 straps.
Before this work, the app could discover, connect, and stream live HR/IMU
from a Gen4 device, but every historical-sync path failed silently because
the wire protocol and state machine differ from Gen5 in subtle ways.

For Gen5/protocol background see [`protocol-reverse-engineering.md`](./protocol-reverse-engineering.md).

---

## Wire-level differences (Gen5 vs Gen4)

| Layer | Gen5 (WHOOP 5.0) | Gen4 (WHOOP 4.0) |
|---|---|---|
| Command characteristic | `fd4b0002` | `61080002` |
| Response characteristic | `fd4b0003` | `61080003` |
| Data stream characteristic | `fd4b0005` | `61080005` |
| Frame header length | 8 bytes (`aa | rev | LE16 len | reserved×2 | CRC-16`) | 4 bytes (`aa | LE16 len | CRC-8`) |
| Hello frame | static captured frame | `GET_HELLO` (cmd 145) in Gen4 framing |
| Historical setup command | `GET_DATA_RANGE` (cmd 34, args = `[]`) | `GET_DATA_RANGE` (cmd 34, args = `[0x00]`) |
| Historical start command | `SEND_HISTORICAL_DATA` (cmd 22, args = `[]`) | `SEND_HISTORICAL_DATA` (cmd 22, args = `[0x00]`) |
| Per-page request | derived from `historyEnd` metadata body | `cmd 23` with `[0x01, LE32 page_seq, LE32 16]` |
| cmd 22 response | `<echoed> 01 …` = success | `<echoed> 02 0b 00 00` = success (the `0x02` is NOT Gen5 PENDING) |
| Page-end ack | computed from `historyEnd` metadata body | local counter, request page_seq + 1 |
| Success signal | last cmd 23 ack with `historyComplete=true` | `historyComplete` metadata (data-stream packets are *not* counted by Swift) |

---

## State machine

The Gen4 sync follows a strict `34 → 22 → 23 → 23 → … → complete` order
discovered from PacketLogger captures of the official iOS app:

```
beginHistoricalSync(gen4)
   │
   ▼
writeHistoricalCommand(.getDataRange)        ← cmd 34, args=[0x00]
   │
   ▼
CMD_RSP cmd 34 → parse payload[10..14]       ← LE32 last_synced
   │   gen4HistoricalPageSeq = last_synced + 1
   ▼
writeHistoricalCommand(.sendHistoricalData)  ← cmd 22, args=[0x00]
   │
   ▼
CMD_RSP cmd 22 → Gen4 short-circuit          ← byte 4 = 0x02 == success ack
   │   (skip Gen5 result-code interpretation)
   ▼
writeHistoricalCommand(.historicalDataResult) ← cmd 23 page_seq = N
   │
   │ ── HistoryStart metadata ──┐
   │ ── HIST_DATA on 61080005 ──│ (reassembled by Rust pipeline)
   │ ── HistoryEnd metadata ────┘
   ▼
gen4HistoricalPageSeq += 1
queue next cmd 23, repeat
   │
   ▼
…until HistoryComplete metadata arrives
   │
   ▼
idle timer fires → completeHistoricalSync
```

The strap chooses when to stop. Each `cmd 23` is fire-and-forget; we never
match it to a `CMD_RSP` (its response on `61080003` is just a 5-byte ack
we ignore).

---

## Implementation map

All changes are confined to the BLE client and one app-level callback.
Each entry points to the file and the *why*, not the *what*.

### Detection & framing

| File / symbol | Role |
|---|---|
| `GooseBLETypes.swift` — `WhoopGeneration` enum | Single source of truth: detect via UUID prefix, build the right hello frame and command frame. Add a new generation here, not in call sites. |
| `GooseBLEClient+Parsing.swift` — `gen4Frames(in:)` / `gen4Payload(in:)` / `buildGen4CommandFrame(...)` / `crc8(...)` | Per-notification Gen4 framing. CRC-8 polynomial `0x07`, init `0x00`. |
| `GooseBLEClient+Commands.swift` — `processDiscoveredCharacteristics` | Sets `activeDeviceGeneration` immediately when the command characteristic is bound, so subsequent frame builds pick the right format. |

### Historical-sync state machine

| File / symbol | Role |
|---|---|
| `GooseBLEClient+HistoricalCommands.swift` — `beginHistoricalSync` Gen4 branch | Forces the cmd 34 entry point even when callers pass `firstCommandOverride` (Gen4 ignores those overrides). |
| `GooseBLEClient+HistoricalCommands.swift` — `gen4PageRequestPayload(seq:)` | The 9-byte cmd 23 args (`[0x01][LE32 page_seq][LE32 16]`). |
| `GooseBLEClient+HistoricalCommands.swift` — `writeHistoricalCommand` payload override | Cmd 34 and cmd 22 carry one `0x00` byte on Gen4 (empty body on Gen5). |
| `GooseBLEClient+HistoricalHandlers.swift` — `handleHistoricalCommandResponse` Gen4 cmd 22 short-circuit | Bypasses the Gen5 result-code path because the Gen4 ack uses `0x02` in that slot. Immediately advances to cmd 23. |
| `GooseBLEClient+HistoricalHandlers.swift` — `handleHistoricalCommandResponse` Gen4 cmd 34 branch | Parses `payload[10..14]` for `last_synced` and sets `gen4HistoricalPageSeq = last_synced + 1`. |
| `GooseBLEClient+HistoricalHandlers.swift` — `historyEnd` Gen4 branch | Increments `gen4HistoricalPageSeq` and queues the next cmd 23. Gen5's `historicalDataResultPayload` derivation doesn't apply. |
| `GooseBLEClient+DebugAndSync.swift` — `retryHistoricalTransferAfterIdleIfNeeded` Gen4 skip | When `historyCompleteReceived`, trust the strap's signal and complete the sync. See "Known limitations" below. |

### Auto-extract after sync

| File / symbol | Role |
|---|---|
| `GooseAppModel.swift` — `onHistoricalSyncCompleted` callback | Decouples `GooseAppModel` (owns the BLE client) from `HealthDataStore` (owned by `AppShellView`). |
| `GooseAppModel+HealthCapture.swift` — `handleHistoricalSyncProgress` | Fires the callback on `progress.isTerminal && !progress.failed`. |
| `AppShellView.swift` — `.onAppear` | Wires the callback to `healthStore.runPacketInputs()` so metric extraction kicks off automatically. |

---

## Known limitations

### 1. Swift sync path doesn't count K24 frames

`handleHistoricalSyncValue` runs a per-notification frame extractor.
Gen4's `K24 normal_history` packets are often delivered in *fragmented*
ATT notifications, so the per-notification deframer can't reassemble them
and they never reach the switch in `handleHistoricalSyncFrame`. As a
result, `historicalPacketsReceivedThisSync` stays at zero throughout a
successful burst.

**Workaround:** trust `historyCompleteReceived` (see
`retryHistoricalTransferAfterIdleIfNeeded`). The Rust pipeline
(`GooseAppModel+NotificationPipeline.swift` → `gooseFrames(...)`) uses a
stateful deframer keyed by characteristic, so it captures every K24 frame
correctly and the `health.packet_capture` summary reflects the real count.

**Proper fix (not in this branch):** unify the deframers — either have the
sync path use the stateful version, or feed Rust's parsed events back into
the sync counter. Tracked separately.

### 2. Reassembly-drop noise on `61080005`

Gen4 emits small (4–8 byte) non-frame chunks between real frames on the
data stream characteristic. The reassembler counts these as drops. They
are harmless but used to flood logs at warn level.

**Workaround:** drops under 32 bytes now log at debug; ≥ 32 bytes still
warn so genuine framing corruption stays visible. See
`GooseAppModel+NotificationPipeline.swift`.

### 3. Wrist Temperature card stays empty (project-wide, not Gen4-specific)

The Health Monitor's "Wrist Temperature" card reads
`dailyRecoveryMetricSnapshot(valueKey: "skin_temperature_delta_c", …)`,
populated by the Rust `metrics.recovery_sensor_daily_rollup` bridge call.
That writer filters candidates on:

```rust
feature.trusted_candidate_evidence
    && feature.resolved_metric_input
    && feature.value_semantics_verified
```

Both the Gen5 `event_temperature_level` and the Gen4 `K24
normal_history` feature constructors hardcode `resolved_metric_input:
false` and `value_semantics_verified: false`. The provenance JSON says
`"score_input_policy": "blocked_until_temperature_units_are_verified"` —
this is a deliberate gate awaiting unit verification, not a wiring bug.

The captured candidates *are* visible in **More → Debug** as
`latestSkinTemperatureCandidateStatus` / `latestHistoryTemperatureCandidateStatus`.

---

## What success looks like in the logs

After a clean run with this branch installed, expect roughly:

```
ble.sync historical_sync.started ... first=gen4_get_data_range
ble.sync historical_sync.command.sent GET_DATA_RANGE seq=N payload=00 aa07...
ble.sync historical_sync.command.raw_response GET_DATA_RANGE ... SUCCESS(1)
ble.sync historical_sync.gen4.range last_synced=NNNN next_seq=NNNN+1
ble.sync historical_sync.command.sent SEND_HISTORICAL_DATA seq=N+1 payload=00 aa07...
ble.sync historical_sync.gen4.transfer_ack seq=N+1 payload=...020b0000
ble.sync historical_sync.gen4.transfer_armed next_seq=NNNN+1
ble.sync historical_sync.command.sent HISTORICAL_DATA_RESULT seq=N+2 payload=01...
ble.sync historical_sync.metadata HistoryStart
ble.sync historical_sync.metadata HistoryEnd
ble.sync historical_sync.gen4.page_end next_seq=NNNN+2 packets=0
[ … repeat per page … ]
ble.sync historical_sync.metadata HistoryComplete
ble.sync historical_sync.gen4.retry_skipped history_complete=true
ble.sync historical_sync.completed reason=gen4_history_complete_metadata_only
```

And in the `health.packet_capture summary`:

- `data.k24.normal_history` growing to hundreds or thousands of frames
- `Skin Temp` matching the K24 count
- `data.k25.none` growing if pulse-info packets are present
- `K47 0` — historical-data-typed (Gen5) frames stay zero on Gen4

---

## How this was reverse-engineered

1. **PacketLogger capture** of the official WHOOP iOS app talking to a Gen4
   strap (`packetlogger_whoop.log`, `whoop_recording_packets.log` in
   `~/Downloads`). Parsed offline with a Python btsnoop reader.
2. **Critical detail**: Apple PacketLogger btsnoop is datalink type
   `1001` and **omits the HCI type byte** (vs the standard `1002` which
   includes it). All ATT offsets are shifted by 1 relative to typical
   btsnoop parsers — getting this wrong led the first analysis pass to
   misread cmd numbers entirely.
3. Compared the first ~30 ATT writes against decoded WHOOP frames to find
   the setup sequence (cmd 35/76/117/118/120/34/22) that precedes the
   first cmd 23 page request.
4. **Discarded most of the setup** — cmd 35/76/117/118/120 are feature-flag
   and config reads. Only cmd 34 (page-range query) and cmd 22 (enable
   transfer) are needed for sync to start; the rest were chatter from the
   official app's app-level state machine.
5. Decoded the cmd 34 response (69 bytes) by matching its `last_synced`
   field against the cmd 23 page_seq the official app sent next. Offset 7
   in the body (= payload offset 10) matched exactly.
