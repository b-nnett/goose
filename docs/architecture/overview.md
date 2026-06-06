<!-- generated-by: gsd-doc-writer -->
# Architecture Overview

Goose is a two-tier biometric platform. An iOS app captures raw biometric data from a WHOOP wearable over Bluetooth Low Energy and persists it locally in SQLite via a Rust core library. A self-hosted server (FastAPI + TimescaleDB, deployed via Docker Compose) receives decoded biometric streams from the app and provides a read API and a static dashboard. The two tiers are loosely coupled: the iOS app operates fully offline and uploads opportunistically when a server URL and API key are configured.

---

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│ iOS App (GooseSwift)                                                │
│                                                                     │
│  WHOOP Device                                                       │
│      │ BLE GATT notifications                                       │
│      ▼                                                              │
│  GooseBLEClient  ──onNotification──►  GooseAppModel                │
│                                           │                         │
│                              notificationIngestQueue                │
│                                           │                         │
│                                           ▼                         │
│                                 NotificationFrameParser             │
│                                    (Rust: frame.parse)              │
│                                           │ frames                  │
│                                           ▼                         │
│                               CaptureFrameWriteQueue                │
│                                    (Rust: capture.import)           │
│                                           │ SQLite write            │
│                                           ▼                         │
│                                  goose.sqlite (local)               │
│                                           │                         │
│                              ┌────────────┴───────────┐            │
│                              │                        │             │
│                              ▼                        ▼             │
│                       HealthDataStore          GooseUploadService   │
│                       (Rust: metrics.*)         (uploadQueue)       │
│                       @MainActor scores         │                   │
│                                                 │ POST /v1/ingest-  │
│                                                 │ decoded + Bearer  │
└─────────────────────────────────────────────────┼───────────────────┘
                                                  │ HTTPS
┌─────────────────────────────────────────────────▼───────────────────┐
│ Self-Hosted Server (Docker Compose)                                 │
│                                                                     │
│  goose-ingest (FastAPI, port 8770)                                  │
│      │ store.upsert_streams → daily.compute_day                     │
│      ▼                                                              │
│  goose-db (TimescaleDB / PostgreSQL 16)                             │
│      hypertables: hr_samples, rr_intervals, events, battery,       │
│      spo2_samples, skin_temp_samples, resp_samples, gravity_samples │
│      plain tables: sleep_sessions, exercise_sessions, daily_metrics │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Data Flow

### Primary real-time BLE → SQLite path

1. **GooseBLEClient** receives raw BLE characteristic notification bytes on its `notificationIngestQueue`. The `onNotification` callback is set by `GooseAppModel`.
2. **GooseAppModel.handleNotification** dispatches work to `notificationIngestQueue`. `NotificationFrameParser` calls the Rust bridge (`GooseRustBridge`) to reassemble multi-packet frames via `protocol.parse_frame_hex`.
3. Parsed frames are handed to **CaptureFrameWriteQueue**, which batches rows and calls the Rust bridge method `capture.import_frame_batch` on its own dedicated queue. Rust writes decoded samples to `goose.sqlite` at `ApplicationSupport/GooseSwift/goose.sqlite`.
4. When a write batch succeeds, `GooseAppModel.triggerUpload` is called, which dispatches `GooseUploadService.upload` on the upload queue.

### Upload path (iOS → server)

1. **GooseUploadService** runs entirely on Swift concurrency detached tasks (`Task.detached(priority: .utility)`) — never on `@MainActor`.
2. It calls the Rust bridge method `upload.get_recent_decoded_streams` to fetch the last ~30 seconds of decoded streams from SQLite.
3. It POSTs a `DecodedBatch` JSON payload to `POST /v1/ingest-decoded` with a `Bearer` token loaded from the iOS Keychain (`RemoteServerKeychain`). The server URL is stored in `UserDefaults` under the key `goose.remote.serverURL`.
4. Retry logic: up to 3 attempts with 1 s / 2 s / 4 s backoff. Silent failure after 3 attempts — raw data is already in local SQLite.
5. Upload status (`lastUploadAt`, `pendingBatchCount`) is published back to `@MainActor` via `DispatchQueue.main.async`.

### Metric score path (on-demand)

`HealthDataStore` (a `@MainActor ObservableObject`) holds its own `GooseRustBridge` instance. It queries Rust `metrics.*` methods on its `packetInputQueue` and `heartRateTimelineQueue` dispatch queues, then publishes results as `@Published` properties consumed by SwiftUI views.

### Server daily analysis path

When `POST /v1/ingest-decoded` is received, the server calls `daily.compute_day` for each calendar day touched by the batch (throttled: at most once per device/day per 120 s; single-flight). `compute_day` reads the raw stream hypertables, runs the sleep → recovery → strain → exercise pipeline (modules in `server/ingest/app/analysis/`), and persists results idempotently to `sleep_sessions`, `exercise_sessions`, and `daily_metrics`.

---

## Key Abstractions

| Abstraction | File | Description |
|---|---|---|
| `GooseAppModel` | `GooseSwift/GooseAppModel.swift` + `GooseAppModel+*.swift` | Central `@MainActor` coordinator; owns BLE client, Rust bridge, all notification queues, upload service. Split across 10 extension files by concern. |
| `GooseBLEClient` | `GooseSwift/GooseBLEClient.swift` + `GooseBLEClient+*.swift` | CoreBluetooth central manager; WHOOP GATT connection and proprietary frame framing; command writes. |
| `GooseRustBridge` | `GooseSwift/GooseRustBridge.swift` | JSON-RPC envelope over `goose_bridge_handle_json` / `goose_bridge_free_string` (C FFI). Schema: `goose.bridge.request.v1`. Stateless — multiple instances are normal. |
| `HealthDataStore` | `GooseSwift/HealthDataStore.swift` + `HealthDataStore+*.swift` | `@MainActor` metric query layer. Holds its own `GooseRustBridge`; publishes scored health metrics to SwiftUI views. |
| `GooseUploadService` | `GooseSwift/GooseUploadService.swift` | Fetches recent decoded streams from Rust, POSTs to `POST /v1/ingest-decoded`. Runs on a dedicated utility queue; never touches `@MainActor` inline. |
| `CaptureFrameWriteQueue` | `GooseSwift/CaptureFrameWriteQueue.swift` | Batches parsed BLE frames and writes them to SQLite via Rust bridge `capture.import_frame_batch`. |
| `NotificationFrameParser` | `GooseSwift/NotificationFrameParsing.swift` | Delegates raw BLE bytes to Rust for frame reassembly and compact summary extraction. |
| `OvernightSQLiteMirrorQueue` | `GooseSwift/OvernightSQLiteMirrorQueue.swift` | During overnight guard mode, queues raw notification rows for Rust bridge SQLite insert. |
| Rust core (`libgoose_core.a`) | `Rust/core/src/bridge.rs` | 58+ dispatched methods: protocol parsing, SQLite persistence, metric algorithms, BLE frame import, export. Entry point: `bridge.rs`. |
| FastAPI ingest service | `server/ingest/app/main.py` | Bearer-gated REST API: `POST /v1/ingest-decoded`, read endpoints, daily compute. No OpenAPI schema exposed publicly. |

---

## Directory Structure

```
goose/
├── GooseSwift/                 iOS app source (Swift/SwiftUI)
│   ├── GooseAppModel*.swift    Central coordinator + extensions
│   ├── GooseBLEClient*.swift   CoreBluetooth + WHOOP protocol
│   ├── GooseRustBridge.swift   C FFI bridge (JSON-RPC)
│   ├── HealthDataStore*.swift  Metric query layer
│   ├── GooseUploadService.swift Server upload (utility queue)
│   └── *Views.swift / *Screen.swift  SwiftUI UI
├── GooseWorkoutLiveActivityExtension/
│   └── GooseWorkoutLiveActivityWidget.swift  ActivityKit / Dynamic Island
├── Rust/core/src/              Rust library (libgoose_core)
│   ├── bridge.rs               FFI dispatch table (58+ methods)
│   ├── protocol.rs             WHOOP BLE frame parsing
│   ├── store.rs                SQLite schema helpers
│   ├── metrics.rs              Health algorithm implementations
│   ├── metric_features.rs      Feature extraction
│   └── ...                     40+ additional modules
├── server/
│   ├── ingest/app/             FastAPI ingest service
│   │   ├── main.py             Route definitions
│   │   ├── ingest.py           Raw-frame batch pipeline
│   │   ├── store.py            Idempotent DB upserts
│   │   ├── read.py             Read queries
│   │   └── analysis/           Daily pipeline (sleep/recovery/strain)
│   ├── db/init.sql             TimescaleDB schema (hypertables)
│   └── docker-compose.yml      goose-db + goose-ingest services
├── Scripts/build_ios_rust.sh   Cross-compile Rust → iOS static libs
└── GooseSwift.xcodeproj        Xcode project
```

---

## Threading Model

| Thread / Queue | Owner | Used For |
|---|---|---|
| `@MainActor` (main thread) | Swift runtime | All `@Published` state mutations, SwiftUI rendering, `GooseAppModel` and `HealthDataStore` methods |
| `com.goose.swift.notification-ingest` | `GooseAppModel` | Initial BLE notification receipt and frame boundary detection |
| `com.goose.swift.notification-parse` | `GooseAppModel` | Rust frame parsing calls (blocking FFI) |
| `com.goose.swift.capture-frame-row-build` | `GooseAppModel` | Building SQLite row structs from parsed frames |
| Swift concurrency detached task (`.utility`) | `GooseUploadService` | Rust bridge `upload.get_recent_decoded_streams` + HTTP upload |
| `com.goose.swift.health.packet-inputs` | `HealthDataStore` | Metric score queries via Rust bridge |
| `com.goose.swift.health.heart-rate-timeline` | `HealthDataStore` | Heart rate timeline refresh |
| `CBCentralManager` queue | CoreBluetooth | BLE delegate callbacks from `GooseBLEClient` |

**Critical constraint:** `GooseRustBridge.request(...)` is a blocking synchronous call (it calls `goose_bridge_handle_json` via C FFI and waits for a response). It must never be called from `@MainActor` inline for any expensive method. Always dispatch to a background queue first.

---

## Persistence Boundaries

| Store | Location | Owner | Contains |
|---|---|---|---|
| `goose.sqlite` | `ApplicationSupport/GooseSwift/goose.sqlite` | Rust core (via `rusqlite`) | All captured BLE frames, decoded biometric samples, metric scores, activity sessions |
| `UserDefaults` | iOS system | Swift | Onboarding state, device identity, HR estimates, server URL (`goose.remote.serverURL`), upload enabled flag (`goose.remote.uploadEnabled`) |
| iOS Keychain | iOS system | `RemoteServerKeychain` | Server API token (service: `goose.remote`, account: `apiKey`) |
| TimescaleDB | Docker volume `goose-db-data` | Server | Hypertables for HR, RR, events, battery, SpO2, skin temp, respiration, gravity; derived tables for sleep/exercise/daily metrics |
| Raw frame archive | Docker volume `goose-raw-data` (`/data/raw`) | Server | Archived raw BLE frame batches (hex, by device/date) |

---

## Server API Summary

All `/v1` routes require `Authorization: Bearer <GOOSE_API_KEY>`. The OpenAPI schema is intentionally disabled (`docs_url=None`) to avoid advertising the API surface publicly.

| Method | Path | Description |
|---|---|---|
| `GET` | `/healthz` | DB connectivity check (no auth required) |
| `POST` | `/v1/ingest-decoded` | Ingest a decoded biometric stream batch from the iOS app |
| `POST` | `/v1/ingest` | Ingest a raw BLE frame batch (legacy / reference) |
| `GET` | `/v1/devices` | List known devices |
| `GET` | `/v1/streams/{kind}` | Query a decoded stream (hr, rr, events, battery, spo2, skin_temp, resp, gravity) |
| `GET` | `/v1/batches` | List raw batch records for a device |
| `GET` | `/v1/batches/{batch_id}/frames` | Retrieve raw BLE frames for a specific batch |
| `GET` | `/v1/summary` | Stream row counts for a device/time range |
| `GET` | `/v1/daily` | Daily metric rows for a date range |
| `GET` | `/v1/today` | Most recent daily metric row for a device |
| `GET` | `/v1/sleep` | Sleep sessions for a date |
| `GET` | `/v1/workouts` | Exercise sessions for a date range |
| `POST` | `/v1/compute-daily` | Force recompute daily metrics for a device/date |
| `POST` | `/v1/backfill-workouts` | Recompute exercise sessions over a date range |
| `GET` | `/v1/profile` | Retrieve user profile (height/weight/age/sex) |
| `POST` | `/v1/profile` | Create or update user profile |
| `GET` | `/` | Static dashboard SPA |

---

## Architectural Constraints

- **Rust bridge is synchronous.** `goose_bridge_handle_json` blocks the calling thread. All bridge calls for expensive operations (capture import, metric computation, upload fetch) must happen on a background `DispatchQueue`.
- **Multiple bridge instances are intentional.** `GooseAppModel`, `HealthDataStore`, `OvernightSQLiteMirrorQueue`, `CaptureFrameWriteQueue`, and `GooseUploadService` each hold their own `GooseRustBridge` instance. The Rust library is stateless across calls; state lives in SQLite.
- **Database path convention.** The SQLite file is always resolved via `HealthDataStore.defaultDatabasePath()`. Every bridge call that accesses storage must pass `database_path` in its args.
- **Upload is opt-in.** `GooseUploadService` checks `UserDefaults` key `goose.remote.uploadEnabled` before every upload attempt. An unconfigured or disabled server URL results in a silent no-op — local SQLite is unaffected.
- **Server ingest is idempotent.** All `store.upsert_*` calls use `ON CONFLICT DO UPDATE` or `DO NOTHING`. The iOS app may upload the same 30 s window multiple times; the server deduplicates by `(device_id, ts)` primary keys on each hypertable.
- **No circular imports.** The `GooseWorkoutLiveActivityExtension` target shares only `WorkoutLiveActivityAttributes.swift` with the main app. It has no access to `GooseAppModel`, `GooseRustBridge`, or any SQLite layer.
