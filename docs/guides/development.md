<!-- generated-by: gsd-doc-writer -->
# Development Guide

This guide covers the day-to-day development workflow for Goose. It assumes you have already completed the first-run setup from [Getting Started](getting-started.md). It is structured around the three development surfaces: the iOS app (Swift/SwiftUI), the Rust core library, and the self-hosted server (FastAPI + TimescaleDB).

---

## Project Structure

```
GooseSwift/                    iOS app source — Swift/SwiftUI
GooseWorkoutLiveActivityExtension/  ActivityKit Live Activity widget
Rust/core/src/                 Rust static library source
Rust/core/include/             C bridge header (goose_core_bridge.h)
Rust/core/tests/               Rust integration tests (42 files)
Rust/iphoneos/                 Staged libgoose_core.a for device builds
Rust/iphonesimulator/          Staged libgoose_core.a for simulator builds
Scripts/build_ios_rust.sh      Xcode build phase — cross-compiles Rust to iOS
server/ingest/app/             FastAPI ingest service
server/ingest/app/analysis/    Daily analysis pipeline (sleep/recovery/strain)
server/db/init.sql             TimescaleDB schema (hypertables)
server/docker-compose.yml      goose-db + goose-ingest services
GooseSwift.xcodeproj           Xcode project
```

---

## iOS App Development

### Build commands

Open `GooseSwift.xcodeproj` in Xcode and build the `GooseSwift` scheme, or use the command line.

**Simulator build:**

```bash
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath /tmp/goose-swift-deriveddata \
  build
```

**Physical device build:**

```bash
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS,id=<device-id>' \
  -derivedDataPath /tmp/goose-swift-deriveddata-device \
  -allowProvisioningUpdates \
  build
```

List connected devices:

```bash
xcrun devicectl list devices
```

The Rust build step runs automatically as an Xcode build phase before compiling Swift. Set `GOOSE_SKIP_RUST_CORE_BUILD=1` only when the `.a` archive is already current for the active platform.

### Swift code conventions

**Extension-per-concern pattern.** Large types are split across files by domain area. Each extension file owns a coherent slice of behaviour; all extensions share state on the parent class.

```
GooseAppModel.swift                         Core @Published state + owned objects
GooseAppModel+NotificationPipeline.swift    BLE notification handling
GooseAppModel+ActivityRecording.swift       Activity session lifecycle
GooseAppModel+ActivityTimeline.swift        Activity timeline refresh
GooseAppModel+HealthCapture.swift           Health packet capture sessions
GooseAppModel+Lifecycle.swift               App lifecycle events
GooseAppModel+OvernightRun.swift            Overnight guard
GooseAppModel+OvernightRecovery.swift       Overnight recovery state
GooseAppModel+OvernightState.swift          Overnight guard state transitions
GooseAppModel+PacketPublishing.swift        BLE packet publishing to pipeline
GooseAppModel+Upload.swift                  Server upload trigger

GooseBLEClient.swift                        @Published BLE state + callback vars
GooseBLEClient+CentralDelegate.swift        CBCentralManagerDelegate
GooseBLEClient+PeripheralDelegate.swift     CBPeripheralDelegate
GooseBLEClient+Commands.swift               WHOOP command writes
GooseBLEClient+Parsing.swift                Packet framing helpers
GooseBLEClient+HistoricalCommands.swift     Historical sync command dispatch
GooseBLEClient+HistoricalHandlers.swift     Historical sync response handling
GooseBLEClient+HRMonitor.swift              HR monitor peripheral support
GooseBLEClient+DebugAndSync.swift           Debug session + sync utilities
GooseBLEClient+UserActions.swift            User-facing BLE actions
GooseBLEClient+VitalsAndLogging.swift       Vitals forwarding and BLE logging

HealthDataStore.swift                       Metric query coordinator
HealthDataStore+Sleep.swift                 Sleep metric queries
HealthDataStore+Cardio.swift                Cardio load queries
HealthDataStore+PacketInputs.swift          Packet input readiness
HealthDataStore+Snapshots.swift             Summary snapshot queries
HealthDataStore+StaticSnapshots.swift       Static/cached snapshot queries
HealthDataStore+ActivitySnapshots.swift     Activity snapshot queries
HealthDataStore+CoachSummaries.swift        Coach summary queries
HealthDataStore+StressEnergy.swift          Stress and energy metric queries
HealthDataStore+Trends.swift                Trend computation queries
HealthDataStore+Utilities.swift             Shared query helpers
HealthDataStore+Vitals.swift                Vitals metric queries
```

When adding a new concern to `GooseAppModel`, `GooseBLEClient`, or `HealthDataStore`, create a new `+<Concern>.swift` extension file rather than growing the primary file.

**Threading rules.** All `@Published` state mutations and SwiftUI rendering happen on `@MainActor`. Each subsystem has a named `DispatchQueue`:

| Queue label | Owner | Purpose |
|---|---|---|
| `com.goose.swift.notification-ingest` | `GooseAppModel` | Initial BLE notification receipt |
| `com.goose.swift.notification-parse` | `GooseAppModel` | Rust frame parsing (blocking FFI) |
| `com.goose.swift.capture-frame-row-build` | `GooseAppModel` | Building SQLite row structs |
| `com.goose.swift.capture-frame-writes` | `CaptureFrameWriteQueue` | SQLite batch writes via Rust |
| `com.goose.swift.health.packet-inputs` | `HealthDataStore` | Metric score queries |
| `com.goose.swift.health.heart-rate-timeline` | `HealthDataStore` | HR timeline refresh |

`GooseUploadService` uses the Swift cooperative thread pool (async/await Tasks) rather than a named `DispatchQueue`.

The Rust bridge (`GooseRustBridge.request(...)`) is a **blocking synchronous call** — it calls `goose_bridge_handle_json` via C FFI and waits for the response. Never call it from `@MainActor` inline for anything expensive. Always dispatch to a background `DispatchQueue` first.

**Multiple bridge instances are intentional.** `GooseAppModel`, `HealthDataStore`, `CaptureFrameWriteQueue`, `OvernightSQLiteMirrorQueue`, and `GooseUploadService` each hold their own `GooseRustBridge()` instance. The Rust library is stateless across calls; all state lives in `goose.sqlite`.

**Database path convention.** The SQLite file is always resolved via `HealthDataStore.defaultDatabasePath()` (resolves to `ApplicationSupport/GooseSwift/goose.sqlite`). Every bridge call that accesses storage must include `"database_path"` in its `args` dictionary.

**Naming conventions:**
- File names: PascalCase matching the primary type (`GooseBLEClient.swift`); extensions use `+` notation (`GooseBLEClient+Commands.swift`)
- Properties: camelCase; booleans prefixed `is`, `can`, `has`, `should`
- `UserDefaults` keys: dot-namespaced reverse-DNS strings as `static let` (`"goose.remote.serverURL"`)
- `DispatchQueue` labels: reverse-DNS format (`"com.goose.swift.capture-frame-writes"`)

**Code style.** 2-space indentation throughout. Opening braces on the same line. One blank line between methods within a type. `private` used heavily for internal state. `@unchecked Sendable` on queue-protected types.

---

## Rust Core Development

### Running tests

The Rust core has 42 integration test files in `Rust/core/tests/`. Run them with Cargo from the `Rust/core` directory:

```bash
cd Rust/core
cargo test
```

Run a specific test file:

```bash
cargo test --test bridge_tests
```

Run a single test by name:

```bash
cargo test --test bridge_tests test_version
```

Unit tests within `src/` follow standard Rust conventions (`#[cfg(test)]` blocks). The Cargo target directory defaults to `build/rust-target/goose-core` to keep build products outside the source tree.

### Bridge architecture

The bridge is the sole interface between Swift and Rust. The C header (`Rust/core/include/goose_core_bridge.h`) exposes three symbols:

```c
char *goose_bridge_handle_json(const char *request_json);
void goose_bridge_free_string(char *value);
char *goose_core_version_json(void);
```

`goose_bridge_handle_json` deserialises a JSON request, dispatches to `handle_bridge_request_inner` in `bridge.rs`, and returns a JSON response string that the caller must free with `goose_bridge_free_string`.

**Request schema:**

```json
{
  "schema": "goose.bridge.request.v1",
  "request_id": "goose-swift-<timestamp>-<counter>",
  "method": "metrics.goose_hrv_v0",
  "args": { "database_path": "/path/to/goose.sqlite" }
}
```

**Response schema:**

```json
{
  "schema": "goose.bridge.response.v1",
  "request_id": "goose-swift-...",
  "ok": true,
  "result": { ... },
  "timing": { "method": "metrics.goose_hrv_v0", "method_elapsed_us": 1234 }
}
```

On failure `ok` is `false` and `error: { "code": "method_error", "message": "..." }` is present.

### Adding a new bridge method

The complete method list is in `BRIDGE_METHODS` (a `&[&str]` constant in `bridge.rs`) and verified by the test `bridge_methods_constant_matches_dispatcher`. When adding a new method:

1. Add the method name string to `BRIDGE_METHODS` in alphabetical order within its namespace prefix.
2. Add a `#[derive(Debug, Clone, Deserialize)]` args struct (e.g. `struct MyMethodArgs { database_path: String, ... }`).
3. Add a match arm in `handle_bridge_request_inner`:

```rust
"my.new_method" => request_args::<MyMethodArgs>(&request)
    .and_then(my_new_method_bridge)
    .map(|value| bridge_ok(&request.request_id, value))
    .unwrap_or_else(|error| bridge_error(&request.request_id, "method_error", error)),
```

4. Implement the bridge function returning `GooseResult<serde_json::Value>`.
5. Run `cargo test --test bridge_tests` — the constant-matches-dispatcher test will catch any drift between `BRIDGE_METHODS` and the match arms.

### Build artifacts

`Scripts/build_ios_rust.sh` builds `Rust/core` for the active Xcode platform and stages the output:

```
Rust/iphoneos/libgoose_core.a          device target (aarch64-apple-ios)
Rust/iphonesimulator/libgoose_core.a   simulator target (aarch64-apple-ios-sim or x86_64-apple-ios)
```

The script uses `find ... -newer libgoose_core.a` to skip rebuilds when source has not changed since the last staged archive. Manual builds (e.g., to test on a different target without Xcode):

```bash
# Simulator — Apple Silicon
PLATFORM_NAME=iphonesimulator CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh

# Physical device
PLATFORM_NAME=iphoneos CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh
```

**Build environment variables:**

| Variable | Default | Effect |
|---|---|---|
| `GOOSE_SKIP_RUST_CORE_BUILD` | `0` | Set to `1` to skip the entire build phase (archive must already exist) |
| `GOOSE_RUST_RELEASE` | `0` | Set to `1` to force a Cargo release build regardless of platform or Xcode configuration |
| `GOOSE_RUST_DEBUG_BUILD` | `0` | Set to `1` to force a Cargo debug build even for `iphoneos` (device builds default to release) |

The Cargo target directory is `build/rust-target/goose-core` (configurable via `CARGO_TARGET_DIR`).

---

## BLE Pipeline Development

### Notification pipeline

BLE bytes flow through a three-queue pipeline before being written to SQLite:

```
GooseBLEClient (CBCentralManagerDelegate callbacks)
  │ raw notification bytes
  ▼ notificationIngestQueue
GooseAppModel.handleNotification
  │ hex bytes + device context
  ▼ notificationParseQueue
NotificationFrameParser → Rust bridge: protocol.parse_frame_hex_batch
  │ parsed frames
  ▼ captureFrameRowBuildQueue
CaptureFrameWriteQueue → Rust bridge: capture.import_frame_batch
  │ sqlite write
  ▼ goose.sqlite
  │ (on success)
  ▼ GooseAppModel.triggerUpload → GooseUploadService
```

**`GooseBLEClient`** (`GooseBLEClient.swift` + extensions) exposes callback properties set by `GooseAppModel`:

```swift
var onNotification: ((GooseNotificationEvent) -> Void)?
var onRawNotification: ((GooseNotificationEvent) -> Void)?
var onRawNotificationWithContext: ((GooseNotificationEvent, GooseBLENotificationContext) -> Void)?
```

When adding support for a new GATT characteristic or packet type, add a handler in `GooseBLEClient+PeripheralDelegate.swift` (for `didUpdateValueFor`) or `GooseBLEClient+Parsing.swift` (for frame reassembly helpers).

**`NotificationFrameParser`** (`NotificationFrameParsing.swift`) calls `protocol.parse_frame_hex_batch` on the Rust bridge. Each parsed frame includes a compact summary (`NotificationFrameCompactSummary`) used for real-time UI metrics without blocking on full decoding.

**`CaptureFrameWriteQueue`** (`CaptureFrameWriteQueue.swift`) is `@unchecked Sendable`, guarded by `NSLock`. It:
- Batches rows with a 50 ms coalesce delay (`coalesceDelay: TimeInterval = 0.05`)
- Calls Rust bridge method `capture.import_frame_batch` with the `database_path` and an array of `CapturedFrameWriteRow` objects
- Back-pressures: frames are dropped when `queuedRowCount >= maxQueuedRows` and the drop count is surfaced in `CaptureFrameWriteEnqueueResult`
- Coalesces completion callbacks (1 s delay, `completionCoalesceDelay`) to avoid flooding `@MainActor`

### Adding a new WHOOP packet type

1. Add frame parsing logic in `Rust/core/src/protocol.rs` (parse the new packet family into a `ParsedPayload` variant).
2. Update `capture.import_frame_batch` in `Rust/core/src/capture_import.rs` to persist the new decoded fields to SQLite (add column migrations in `store.rs` if the schema changes).
3. Add the compact summary extraction to `Rust/core/src/bridge.rs` if the packet needs real-time UI feedback.
4. Add a `GooseBLEClient+` extension handler in Swift to route the new characteristic notifications into `onNotification`.
5. Add a Rust integration test in `Rust/core/tests/` exercising the new parse path.

---

## Server Development

### Starting the server for development

```bash
cd server
cp .env.example .env
# Set GOOSE_API_KEY and GOOSE_DB_PASSWORD in .env
docker compose up -d --build
```

Verify it started:

```bash
curl -s localhost:8770/healthz
# → {"status":"ok"}
```

The server exposes two services via Docker Compose:
- `goose-db` — TimescaleDB 2.17.2 on PostgreSQL 16; data in Docker volume `goose-db-data`
- `goose-ingest` — FastAPI on port 8770 (configurable via `GOOSE_INGEST_PORT`); raw BLE archives in Docker volume `goose-raw-data`

### Adding a new API route

Routes are defined in `server/ingest/app/main.py`. All `/v1` routes are Bearer-gated via the `require_auth` dependency:

```python
@app.get("/v1/my-new-route", dependencies=[Depends(require_auth)])
def my_new_route(device: str):
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.my_new_query(conn, device)
```

The OpenAPI schema is intentionally disabled (`docs_url=None, redoc_url=None, openapi_url=None`) — do not re-enable it without reviewing whether the endpoint surface should be public.

**File responsibilities:**
- `main.py` — route definitions and request/response models (Pydantic `BaseModel`)
- `store.py` — idempotent DB upserts (`ON CONFLICT DO UPDATE / DO NOTHING`)
- `read.py` — read queries returning dicts/lists
- `ingest.py` — raw BLE frame batch pipeline (legacy path)
- `db.py` — schema bootstrap (`bootstrap_schema` re-applies `init.sql` idempotently on every startup)
- `config.py` — reads `GOOSE_API_KEY`, `GOOSE_DB_DSN`, `GOOSE_RAW_ROOT` from environment
- `analysis/daily.py` — `compute_day(conn, device_id, day)` orchestrates the daily sleep → recovery → strain → exercise pipeline

### TimescaleDB schema

The schema lives in `server/db/init.sql` and is applied idempotently by `db.bootstrap_schema` on startup. Hypertables (partitioned by `ts`):

| Table | Key columns | Notes |
|---|---|---|
| `hr_samples` | `(device_id, ts)` | 1 Hz heart rate in BPM |
| `rr_intervals` | `(device_id, ts, rr_ms)` | R-R intervals from WHOOP |
| `events` | `(device_id, ts, kind)` | WHOOP lifecycle events |
| `battery` | `(device_id, ts)` | State of charge, mV, charging flag |
| `spo2_samples` | `(device_id, ts)` | Raw ADC (red + IR LED) |
| `skin_temp_samples` | `(device_id, ts)` | Raw ADC |
| `resp_samples` | `(device_id, ts)` | Raw ADC |
| `gravity_samples` | `(device_id, ts)` | Accel-derived gravity vector in g |

Plain tables: `devices`, `raw_batches`, `sleep_sessions`, `exercise_sessions`, `daily_metrics`, `user_profiles`.

All `store.upsert_*` calls use parameterised queries — never string-interpolate device IDs or timestamps into SQL.

### Daily analysis pipeline

`POST /v1/ingest-decoded` triggers `daily.compute_day(conn, device_id, day)` for each calendar day touched by the uploaded batch. Recomputes are throttled: at most once per `(device, day)` per 120 seconds (`_RECOMPUTE_COOLDOWN_S`), and single-flighted (`_recompute_lock`). Force a recompute immediately:

```bash
curl -s -X POST localhost:8770/v1/compute-daily \
  -H "Authorization: Bearer <GOOSE_API_KEY>" \
  -H "Content-Type: application/json" \
  -d '{"device": "<device-id>", "date": "2026-06-03"}'
```

The pipeline in `analysis/` is composed of pure functions over stream dicts:
- `sleep.py` — sleep window detection, stage scoring
- `hrv.py` — nightly HRV RMSSD (last-SWS tiered)
- `recovery.py` — recovery score with EWMA baseline (`baselines.py`)
- `strain.py` + `exercise.py` — strain computation and exercise sessions
- `calories.py` — calorie estimates
- `units.py` — unit conversions

---

## Debug Surfaces

### More tab debug views

The More tab (`GooseSwift/MoreView.swift`) exposes all operational debug surfaces. `MoreDataStore` (`MoreDataStore.swift`) is the `@MainActor ObservableObject` backing these views. Key surfaces:

- **Storage** — runs `storage.check` bridge method; shows schema version and database path
- **Capture** — start/stop health packet capture sessions; shows frame counts and import status
- **Debug session** — starts a `debug.start_session` bridge call using WebSocket config `ws://127.0.0.1:8765`; refreshes via `debug.session_snapshot`
- **Validation** — runs property suite (`diagnostics.property_suite`), UI coverage audit (`ui_coverage.audit`), privacy lint (`privacy.lint`)
- **Export** — raw BLE frame export with date ranges via `export.raw_timeframe`

### WebSocket debug server

The Rust core includes a standalone WebSocket debug server (`Rust/core/src/debug_ws_server.rs`) used for external tooling to receive debug events emitted during bridge calls. The iOS app records the WebSocket contract URL (`ws://127.0.0.1:8765`, bind host `127.0.0.1`) when starting a debug session via `debug.start_session`. The actual server process is launched separately via the `goose-debug-ws-serve` binary built from `Rust/core/src/bin/goose-debug-ws-serve.rs`.

Run the debug server from `Rust/core`:

```bash
cargo run --bin goose-debug-ws-serve -- \
  --database-path /path/to/goose.sqlite \
  --session-id <session-id> \
  --port 8765
```

Debug events are structured as `goose.debug.event.v1` JSON envelopes and streamed to the WebSocket client as they occur.

### Calling bridge methods manually

During development you can call any bridge method directly from the Rust `core.list_methods` output:

```bash
cd Rust/core
echo '{"schema":"goose.bridge.request.v1","request_id":"dev-1","method":"core.list_methods","args":{}}' \
  | cargo run --bin goose-fixture-index 2>/dev/null
```

Or via a Swift playground call using `GooseRustBridge`:

```swift
let bridge = GooseRustBridge()
let result = try bridge.request(
  method: "storage.check",
  args: ["database_path": HealthDataStore.defaultDatabasePath()]
)
```

---

## Contributing

- Keep changes close to the feature or bug being addressed.
- Match the existing SwiftUI style before introducing new patterns.
- Build after touching Swift source, Rust bridge, project settings, or signing configuration.
- Check both empty and populated states for metric UI.
- Put debug tooling, packet details, and raw export behaviour under More or Debug surfaces — not in everyday health views.
- Update the relevant doc in `docs/guides/` when a change completes or changes an open task.
- Mention any build warnings, skipped checks, or device-only assumptions in the PR description.

See [GETTING-STARTED.md](getting-started.md) for prerequisites and first-run setup.
