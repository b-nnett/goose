<!-- generated-by: gsd-doc-writer -->
# API Reference

This document covers the two API surfaces in the Goose platform:

1. **Server REST API** — a FastAPI service that receives biometric data from the iOS app and exposes read endpoints for the dashboard.
2. **Rust Bridge FFI API** — a JSON-over-C-FFI interface embedded in `libgoose_core.a`, called by the iOS app directly in-process.

---

## Authentication

### Server REST API

All `/v1/*` endpoints require a Bearer token. The token is configured server-side via the `GOOSE_API_KEY` environment variable.

Include the header in every request:

```
Authorization: Bearer <your-api-key>
```

A missing or incorrect token returns `401 Unauthorized`:

```json
{"detail": "unauthorized"}
```

The server uses `secrets.compare_digest` for constant-time comparison, preventing timing attacks.

### Rust Bridge FFI

The bridge does not use network authentication. It is called in-process via a C function pair:

- `goose_bridge_handle_json(request: *const c_char) -> *mut c_char`
- `goose_bridge_free_string(ptr: *mut c_char)`

All security is at the iOS app/OS level; the bridge is not exposed over any network interface.

---

## Server REST API

### Base URL

<!-- VERIFY: production base URL for the self-hosted server -->

The server runs as a Docker container on the user's personal server. The exact host and port depend on the deployment configuration.

### Endpoints Overview

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/` | No | Serve the datastore dashboard SPA (`index.html`) |
| `GET` | `/architecture` | No | Serve the device-link architecture page |
| `GET` | `/healthz` | No | Health check — verifies database connectivity |
| `POST` | `/v1/ingest` | Yes | Ingest a raw BLE frame batch |
| `POST` | `/v1/ingest-decoded` | Yes | Ingest pre-decoded biometric streams |
| `GET` | `/v1/devices` | Yes | List all registered devices |
| `GET` | `/v1/batches` | Yes | List raw batches for a device |
| `GET` | `/v1/batches/{batch_id}/frames` | Yes | Get parsed frames for a raw batch |
| `GET` | `/v1/summary` | Yes | Count records per stream for a device and time window |
| `GET` | `/v1/streams/{kind}` | Yes | Query decoded time-series stream data |
| `POST` | `/v1/compute-daily` | Yes | Compute and persist daily metrics for a device/date |
| `GET` | `/v1/daily` | Yes | Query daily metrics over a date range |
| `GET` | `/v1/today` | Yes | Most-recent daily metrics row for a device |
| `GET` | `/v1/sleep` | Yes | Sleep sessions ending on a given date |
| `GET` | `/v1/workouts` | Yes | Exercise sessions over a date range |
| `POST` | `/v1/backfill-workouts` | Yes | Recompute exercise sessions over a historical date range |
| `GET` | `/v1/profile` | Yes | Get stored user profile for a device |
| `POST` | `/v1/profile` | Yes | Create or update user profile |

OpenAPI docs are disabled server-side (`docs_url=None`, `redoc_url=None`, `openapi_url=None`) — this reference is the canonical documentation.

---

### Health Check

#### `GET /healthz`

Verifies the server can reach the database. No auth required.

**Response — healthy (200):**
```json
{"status": "ok"}
```

**Response — unhealthy (503):**
```json
{"detail": "db unavailable: <error message>"}
```

---

### Ingest Endpoints

#### `POST /v1/ingest`

Ingest a batch of raw BLE frames from the device. The server stores and optionally decodes the frames.

**Request body:**
```json
{
  "batch_id": "uuid-string",
  "device": {
    "device_id": "device-identifier",
    "mac": "AA:BB:CC:DD:EE:FF",
    "name": "WHOOP 5.0"
  },
  "clock_ref": {
    "device": 1234567,
    "wall": 1700000000
  },
  "frames": [
    {"seq": 0, "hex": "aabbccdd..."},
    {"seq": 1, "hex": "aabbccdd..."}
  ],
  "decode_streams": true
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `batch_id` | string | Yes | Unique identifier for this batch |
| `device.device_id` | string | Yes | Stable device identifier |
| `device.mac` | string | No | BLE MAC address |
| `device.name` | string | No | Human-readable device name |
| `clock_ref.device` | int | Yes | Device clock tick at capture time |
| `clock_ref.wall` | int | Yes | Wall-clock Unix seconds at capture time |
| `frames[].seq` | int | No | Frame sequence number |
| `frames[].hex` | string | Yes | Raw BLE frame as a hex string |
| `decode_streams` | bool | No (default: `true`) | Whether to decode frames into typed streams |

**Response (200):** Result from the ingest processor (structure varies by implementation).

---

#### `POST /v1/ingest-decoded`

Ingest pre-decoded biometric streams directly. This is the primary upload path used by the iOS app — the Rust bridge decodes frames on-device, and the structured streams are sent here.

After ingestion, the server automatically recomputes daily metrics for the affected calendar days (throttled to at most once per 120 seconds per device/day pair, with single-flight protection).

**Request body:**
```json
{
  "device": {
    "id": "device-identifier",
    "mac": "AA:BB:CC:DD:EE:FF",
    "name": "WHOOP 5.0"
  },
  "streams": {
    "hr": [{"ts": 1700000000, "bpm": 72}],
    "rr": [{"ts": 1700000000, "rr_ms": 833}],
    "events": [{"ts": 1700000000, "kind": "string", "payload": {}}],
    "battery": [{"ts": 1700000000, "soc": 85, "mv": 4100, "charging": false}],
    "spo2": [{"ts": 1700000000, "red": 12345, "ir": 67890}],
    "skin_temp": [{"ts": 1700000000, "raw": 2048}],
    "resp": [{"ts": 1700000000, "raw": 512}],
    "gravity": [{"ts": 1700000000, "x": 0.01, "y": 0.02, "z": 0.98}]
  },
  "device_generation": "5.0"
}
```

| Stream | Value columns | Description |
|--------|--------------|-------------|
| `hr` | `bpm` | Heart rate samples |
| `rr` | `rr_ms` | RR interval in milliseconds |
| `events` | `kind`, `payload` | Device events |
| `battery` | `soc`, `mv`, `charging` | Battery state of charge (%), millivolts, charging flag |
| `spo2` | `red`, `ir` | Raw ADC photodiode counts |
| `skin_temp` | `raw` | Raw ADC skin temperature |
| `resp` | `raw` | Raw ADC respiratory rate |
| `gravity` | `x`, `y`, `z` | Accelerometer-derived gravity vector in g |

`spo2`, `skin_temp`, and `resp` store raw ADC values. The read API converts these to human units (`%`, `°C`, `bpm`) when queried.

`device_generation` defaults to `"5.0"` if omitted (for backward compatibility with older clients).

**Response (200):**
```json
{"upserted": {"hr": 120, "rr": 118, "events": 3, "battery": 5, "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0}}
```

---

### Read Endpoints

#### `GET /v1/devices`

List all registered devices.

**Response (200):**
```json
[
  {
    "device_id": "device-identifier",
    "mac": "AA:BB:CC:DD:EE:FF",
    "name": "WHOOP 5.0",
    "first_seen": "2024-01-01T00:00:00+00:00",
    "last_seen": "2024-06-01T12:00:00+00:00"
  }
]
```

---

#### `GET /v1/batches`

List raw batches for a device.

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `device` | string | Required | Device identifier |
| `limit` | int | `100` | Maximum number of batches to return |

**Response (200):** Array of batch records with `batch_id`, `device_id`, `received_at`, `start_ts`, `end_ts`, `packet_count`, `file_path`, `sha256`, `byte_size`.

---

#### `GET /v1/batches/{batch_id}/frames`

Get parsed frames from a stored raw batch archive. The archive is decompressed (zstd) and each frame is parsed via the whoop-protocol library.

**Path parameters:**

| Parameter | Description |
|-----------|-------------|
| `batch_id` | UUID of the batch |

**Response (200):** Array of frame objects:
```json
[
  {
    "seq": 0,
    "hex": "aabbccdd...",
    "type_name": "DataPacket",
    "crc_ok": true,
    "fields": [],
    "parsed": {}
  }
]
```

**Response (404):** `{"detail": "batch not found"}`

---

#### `GET /v1/summary`

Count records per decoded stream and raw batches for a device within a time window.

**Query parameters:**

| Parameter | Alias | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `device` | — | string | Required | Device identifier |
| `from` | `from_` | int | `0` | Start Unix timestamp (seconds) |
| `to` | `to` | int | `2000000000` | End Unix timestamp (seconds) |

**Response (200):**
```json
{
  "hr": 14400, "rr": 14350, "events": 42, "battery": 288,
  "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0,
  "batches": 17
}
```

---

#### `GET /v1/streams/{kind}`

Query decoded time-series stream data for a device.

**Path parameters:**

| Parameter | Values |
|-----------|--------|
| `kind` | `hr`, `rr`, `events`, `battery`, `spo2`, `skin_temp`, `resp`, `gravity` |

**Query parameters:**

| Parameter | Alias | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `device` | — | string | Required | Device identifier |
| `from` | `from_` | int | `0` | Start Unix timestamp (seconds) |
| `to` | `to` | int | `2000000000` | End Unix timestamp (seconds) |
| `limit` | — | int | `5000` | Hard cap on rows returned |
| `max_points` | — | int | `null` | When set, enables server-side time-bucket downsampling |

When `max_points` is set and the raw row count exceeds it, the server returns time-bucketed averages. The bucket width is derived from the actual data extent, not the nominal `from`/`to` sentinel window.

`spo2`, `skin_temp`, and `resp` rows include augmented human-unit fields alongside the raw ADC values:

- `spo2`: adds `value` (SpO2 %) computed via rolling window, `unit: "%"`
- `skin_temp`: adds `value` (°C), `unit: "°C"`
- `resp`: adds `value` (breaths per minute), `unit: "bpm"`

**Response (200):** Array of time-ordered row objects. Each row has a `ts` (ISO-8601 datetime) plus kind-specific value columns.

**Response (404):** `{"detail": "unknown stream kind: <kind>"}`

---

### Daily Metrics Endpoints

#### `POST /v1/compute-daily`

Compute and persist daily metrics for a specific device/date. Runs the full analysis pipeline including sleep staging.

**Request body:**
```json
{"device": "device-identifier", "date": "2024-06-01"}
```

**Response (200):** The computed daily summary object.

---

#### `GET /v1/daily`

Query daily metrics over an inclusive date range.

**Query parameters:**

| Parameter | Alias | Type | Required | Description |
|-----------|-------|------|----------|-------------|
| `device` | — | string | Yes | Device identifier |
| `from` | `from_` | string | Yes | Start date (`YYYY-MM-DD`) |
| `to` | `to` | string | Yes | End date (`YYYY-MM-DD`) |

**Response (200):** Array of daily metric rows. Each row contains:
`device_id`, `day`, `total_sleep_min`, `efficiency`, `deep_min`, `rem_min`, `light_min`, `disturbances`, `resting_hr`, `avg_hrv`, `recovery`, `strain`, `exercise_count`, `sleep_start`, `sleep_end`, `spo2_pct`, `skin_temp_dev_c`, `resp_rate_bpm`, `sleep_performance`, `training_state`, `sleep_needed_min`, `total_calories_kcal`, `computed_at`.

---

#### `GET /v1/today`

Most-recent daily metrics row for a device.

**Query parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `device` | string | Yes | Device identifier |

**Response (200):** A single daily metrics row (same shape as `/v1/daily` rows), or `null` if no rows exist.

---

#### `GET /v1/sleep`

Sleep sessions whose night ends on a given date.

**Query parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `device` | string | Yes | Device identifier |
| `date` | string | Yes | Date in `YYYY-MM-DD` format |

**Response (200):** Array of sleep session objects:
```json
[
  {
    "device_id": "device-identifier",
    "start_ts": "2024-05-31T23:30:00+00:00",
    "end_ts": "2024-06-01T07:15:00+00:00",
    "efficiency": 0.87,
    "resting_hr": 52.0,
    "avg_hrv": 68.5,
    "stages": []
  }
]
```

---

#### `GET /v1/workouts`

Exercise sessions whose start date (UTC) falls within a date range.

**Query parameters:**

| Parameter | Alias | Type | Required | Description |
|-----------|-------|------|----------|-------------|
| `device` | — | string | Yes | Device identifier |
| `from` | `from_` | string | Yes | Start date (`YYYY-MM-DD`) |
| `to` | `to` | string | Yes | End date (`YYYY-MM-DD`) |

**Response (200):** Array of exercise session objects with columns: `device_id`, `start_ts`, `end_ts`, `avg_hr`, `peak_hr`, `strain`, `kind`, `duration_s`, `zone_time_pct`, `avg_hrr_pct`, `hrmax`, `hrmax_source`, `calories_kcal`, `calories_kj`.

---

#### `POST /v1/backfill-workouts`

Recompute exercise sessions over a historical date range by replaying `compute_day` for each date. Idempotent and safe to re-run, but may be slow for large ranges.

**Request body:**
```json
{"device": "device-identifier", "from": "2024-01-01", "to": "2024-06-01"}
```

**Response (200):**
```json
{
  "recomputed": 152,
  "days": [
    {"date": "2024-01-01", "status": "ok", "exercises": []},
    {"date": "2024-01-02", "status": "error", "detail": "reason"}
  ]
}
```

---

### Profile Endpoints

#### `GET /v1/profile`

Get the stored user profile for a device.

**Query parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `device` | string | Yes | Device identifier |

**Response (200):** Profile object or `{}` if none exists:
```json
{
  "device_id": "device-identifier",
  "height_cm": 178.0,
  "weight_kg": 75.0,
  "age": 30,
  "sex": "male",
  "updated_at": "2024-06-01T10:00:00+00:00"
}
```

---

#### `POST /v1/profile`

Create or update the user profile for a device.

**Request body:**
```json
{
  "device": "device-identifier",
  "height_cm": 178.0,
  "weight_kg": 75.0,
  "age": 30,
  "sex": "male"
}
```

All fields except `device` are optional. `sex` must be one of `"male"`, `"female"`, or `"nonbinary"` (case-insensitive) or `null`.

**Response (200):** The full profile row after upsert.

**Response (422):** `{"detail": "sex must be one of ['female', 'male', 'nonbinary'] or null; got '...'"}`

---

### Error Responses

| Status | Condition |
|--------|-----------|
| `401` | Missing or invalid `Authorization` header |
| `400` | Invalid date format (expects `YYYY-MM-DD`) |
| `404` | Resource not found (batch, unknown stream kind) |
| `422` | Validation error (invalid field values) |
| `503` | Database unreachable (health check only) |

---

## Rust Bridge FFI API

The Rust bridge exposes all core logic — protocol parsing, metric computation, SQLite persistence — as a single JSON-over-FFI RPC layer. The Swift app calls it synchronously via three C symbols from `libgoose_core.a`.

### C Function Signatures

Declared in `Rust/core/include/goose_core_bridge.h` (included via `GooseSwift/GooseSwift-Bridging-Header.h`):

```c
const char *goose_bridge_handle_json(const char *request_json);
void        goose_bridge_free_string(char *value);
const char *goose_core_version_json(void);
```

`goose_bridge_handle_json` is synchronous and blocks the calling thread until complete. Never call it from the main thread or any `@MainActor` context for expensive methods — always dispatch to a background queue first.

Memory ownership: the caller must pass the returned pointer to `goose_bridge_free_string` after reading the response. Never free it with standard `free()`.

---

### Request Envelope

Every call uses the same JSON envelope:

```json
{
  "schema": "goose.bridge.request.v1",
  "request_id": "goose-swift-1700000000.0-1",
  "method": "core.version",
  "args": {}
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schema` | string | Yes | Must be `"goose.bridge.request.v1"` |
| `request_id` | string | Yes | Non-empty unique identifier for this call; echoed in the response |
| `method` | string | Yes | The RPC method name (see method catalogue below) |
| `args` | object | No (default: `{}`) | Method-specific arguments |

The `request_id` and `schema` fields are validated before dispatching. A missing or empty `request_id` returns an error immediately.

---

### Response Envelope

```json
{
  "schema": "goose.bridge.response.v1",
  "request_id": "goose-swift-1700000000.0-1",
  "ok": true,
  "result": { ... },
  "timing": {
    "method": "core.version",
    "method_elapsed_us": 42
  }
}
```

**On failure:**
```json
{
  "schema": "goose.bridge.response.v1",
  "request_id": "goose-swift-1700000000.0-1",
  "ok": false,
  "error": {
    "code": "method_error",
    "message": "human-readable description"
  }
}
```

| Field | Type | Always present | Description |
|-------|------|----------------|-------------|
| `schema` | string | Yes | `"goose.bridge.response.v1"` |
| `request_id` | string | Yes | Echoed from request |
| `ok` | bool | Yes | `true` on success |
| `result` | object | On success | Method-specific return value |
| `error` | object | On failure | Contains `code` (string) and `message` (string) |
| `timing` | object | On success | Contains `method` (string) and `method_elapsed_us` (uint64, microseconds) |

---

### Swift Usage Pattern

`GooseRustBridge` in `GooseSwift/GooseRustBridge.swift` wraps the FFI:

```swift
let bridge = GooseRustBridge()

// Successful call — returns the `result` dictionary
let result = try bridge.request(method: "core.version")

// Call with arguments
let value = try bridge.requestValue(
  method: "storage.check",
  args: ["database_path": databasePath, "self_test": false]
)
```

`request()` returns `[String: Any]`. `requestValue()` returns `Any` (for methods that return non-object types). Both throw `GooseRustBridgeError` on failure.

Every caller creates its own `GooseRustBridge` instance — the bridge is stateless and multiple instances are intentional. All methods that touch storage require `database_path` in `args`; the canonical path is resolved via `HealthDataStore.defaultDatabasePath()`.

---

### Method Catalogue

The bridge supports 121 RPC methods at compile time. The full live list is available at runtime via `core.list_methods`. Methods are grouped by namespace:

#### Core / Discovery

| Method | Args | Description |
|--------|------|-------------|
| `core.version` | _(none)_ | Returns crate version, schema IDs, storage schema version |
| `core.list_methods` | _(none)_ | Returns sorted list of all supported method names |

#### Protocol Parsing

| Method | Key Args | Description |
|--------|----------|-------------|
| `protocol.parse_frame_hex` | `frame_hex: string`, `device_type: string` | Parse a single BLE frame from hex |
| `protocol.parse_frame_hex_batch` | `frames: [string]`, `device_type: string`, `include_result: bool` | Parse multiple frames in a single call |

#### Storage

| Method | Key Args | Description |
|--------|----------|-------------|
| `storage.check` | `database_path`, `self_test: bool` | Verify the SQLite database is healthy |
| `storage.compact_raw_evidence` | `database_path`, `limit_bytes: i64` | Compact raw evidence storage, removing oldest frames until under the byte limit |

#### Settings / Algorithm Preferences

| Method | Key Args | Description |
|--------|----------|-------------|
| `settings.apply_default_algorithm_preferences` | `database_path`, `scope` | Seed the preferences table with built-in defaults |
| `settings.get_algorithm_preference` | `database_path`, `scope`, `metric_family` | Get the active algorithm for a metric family |
| `settings.list_algorithm_preferences` | `database_path`, `scope?` | List all stored algorithm preferences |
| `settings.set_algorithm_preference` | `database_path`, `scope`, `metric_family`, `algorithm_id`, `version`, `register_built_ins` | Set or update an algorithm preference |

#### Metrics — Score Algorithms

| Method | Key Args | Description |
|--------|----------|-------------|
| `metrics.goose_hrv_v0` | `HrvInput` fields | Run HRV algorithm v0 |
| `metrics.goose_sleep_v0` | `SleepInput` fields | Run sleep scoring algorithm v0 |
| `metrics.goose_sleep_v1` | `SleepV1Input` fields | Run sleep staging algorithm v1 |
| `metrics.goose_strain_v0` | `StrainInput` fields | Run strain scoring algorithm v0 |
| `metrics.goose_recovery_v0` | `RecoveryInput` fields | Run recovery scoring algorithm v0 |
| `metrics.goose_stress_v0` | `StressInput` fields | Run stress scoring algorithm v0 |
| `metrics.built_in_definitions` | _(none)_ | List all built-in algorithm definitions |
| `metrics.reference_definitions` | _(none)_ | List all reference algorithm definitions |
| `metrics.default_preferences` | _(none)_ | Get the default algorithm preferences |
| `metrics.reference_compare` | `family`, `input`, `reference_report?`, `goose_algorithm_id?` | Compare Goose output against a reference |

#### Metrics — Store-Backed Features

| Method | Key Args | Description |
|--------|----------|-------------|
| `metrics.input_readiness` | `database_path`, `start`, `end`, `min_owned_captures?` | Check metric pipeline readiness |
| `metrics.heart_rate_features` | `database_path`, `start`, `end` | Compute heart rate feature report |
| `metrics.hrv_features` | `database_path`, `start`, `end`, `min_rr_intervals_to_compute?` | Compute HRV features |
| `metrics.hrv_capture_validation` | `database_path`, `start`, `end` | Validate HRV capture quality |
| `metrics.motion_features` | `database_path`, `start`, `end` | Compute motion feature report |
| `metrics.vital_event_features` | `database_path`, `start`, `end` | Compute vital event features |
| `metrics.resting_hr_features` | `database_path`, `start`, `end` | Compute resting heart rate features |
| `metrics.window_features` | `database_path`, `start`, `end` | Compute metric window features |
| `metrics.recovery_score_from_features` | `database_path`, `start`, `end` | Compute recovery score from stored features |
| `metrics.sleep_score_from_features` | `database_path`, `start`, `end` | Compute sleep score from stored features |
| `metrics.strain_score_from_features` | `database_path`, `start`, `end` | Compute strain score from stored features |
| `metrics.stress_score_from_features` | `database_path`, `start`, `end` | Compute stress score from stored features |
| `metrics.recovery_sensor_discovery` | `database_path`, `start`, `end` | Discover which recovery sensors are available |
| `metrics.recovery_sensor_daily_rollup` | `database_path`, `date_key`, `timezone` | Roll up recovery sensor data for a day |
| `metrics.recovery_unavailable_daily_status` | `database_path`, `date_key`, `timezone` | Mark recovery as unavailable for a day |
| `metrics.resting_hr_daily_rollup` | `database_path`, `date_key`, `timezone` | Roll up resting HR for a day |
| `metrics.resting_hr_capture_validation` | `database_path`, `start`, `end` | Validate resting HR capture quality |
| `metrics.daily_recovery_metrics` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List daily recovery metric rows |

#### Metrics — Activity & Step Counting

| Method | Key Args | Description |
|--------|----------|-------------|
| `metrics.step_packet_discovery` | `database_path`, `start`, `end` | Discover step-count data in captured packets |
| `metrics.step_capture_validation` | `database_path`, `start`, `end` | Validate step capture quality |
| `metrics.raw_motion_step_estimate` | `database_path`, `start`, `end` | Estimate steps from raw motion |
| `metrics.step_counter_ingest` | `database_path`, `start`, `end` | Ingest step counter data |
| `metrics.step_counter_daily_rollup` | `database_path`, `date_key`, `timezone`, `start_time_unix_ms`, `end_time_unix_ms` | Roll up step count for a day |
| `metrics.step_counter_hourly_rollup` | `database_path`, `date_key`, `timezone`, `start_time_unix_ms`, `end_time_unix_ms` | Roll up step count for an hour |
| `metrics.daily_activity_metrics` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List daily activity metric rows |
| `metrics.hourly_activity_metrics` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List hourly activity metric rows |
| `metrics.activity_unavailable_daily_status` | `database_path`, `date_key`, `timezone`, `start_time_unix_ms`, `end_time_unix_ms` | Mark activity as unavailable for a day |

#### Metrics — Energy

| Method | Key Args | Description |
|--------|----------|-------------|
| `metrics.energy_daily_rollup` | `database_path`, `date_key`, `timezone`, `profile_weight_kg?`, `profile_age_years?` | Roll up energy expenditure for a day |
| `metrics.energy_hourly_rollup` | `database_path`, `date_key`, `timezone`, `start`, `end` | Roll up energy for an hour |
| `metrics.energy_capture_validation` | `database_path`, `date_key`, `timezone` | Validate energy capture data |
| `metrics.energy_unavailable_daily_status` | `database_path`, `date_key`, `timezone` | Mark energy as unavailable for a day |

#### Metrics — Sensor Validation

| Method | Key Args | Description |
|--------|----------|-------------|
| `metrics.respiratory_rate_capture_validation` | `database_path`, `start`, `end` | Validate respiratory rate capture quality |
| `metrics.oxygen_saturation_capture_validation` | `database_path`, `start`, `end` | Validate SpO2 capture quality |
| `metrics.temperature_capture_validation` | `database_path`, `start`, `end` | Validate skin temperature capture quality |

#### Capture

| Method | Key Args | Description |
|--------|----------|-------------|
| `capture.start_session` | `database_path`, `session_id`, `source`, `started_at_unix_ms`, `device_model` | Begin a new capture session |
| `capture.finish_session` | `database_path`, `session_id`, `ended_at_unix_ms`, `frame_count?` | End a capture session |
| `capture.list_sessions` | `database_path`, `start_unix_ms`, `end_unix_ms` | List capture sessions in a time window |
| `capture.import_frame_batch` | `database_path`, `frames: [CapturedFrameInput]`, `parser_version?` | Import a batch of captured frames into SQLite |
| `capture.timeline` | `database_path`, `start`, `end` | Build a packet timeline |
| `capture.observability_timeline` | `database_path`, `start`, `end`, `start_unix_ms`, `end_unix_ms` | Build an observability timeline |
| `capture.correlation_report` | `database_path`, `start`, `end`, `min_owned_captures?` | Assess correlation between owned and reference captures |
| `capture.arrival_plan` | `database_path`, `start`, `end`, `timezone?` | Generate a capture arrival plan and next actions |
| `capture.sanitize` | `input_path`, `output_path`, `salt?` | Sanitize a capture file (redact identifiers) |

#### Activity Sessions

| Method | Key Args | Description |
|--------|----------|-------------|
| `activity.create_session` | `database_path`, `session_id`, `source`, `start_time_unix_ms`, `end_time_unix_ms`, `activity_type`, `confidence`, `detection_method`, `sync_status` | Create or upsert an activity session |
| `activity.get_session` | `database_path`, `session_id` | Get a single activity session by ID |
| `activity.list_sessions` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List activity sessions in a time window |
| `activity.list_sessions_with_metrics` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List sessions with attached metrics |
| `activity.update_session` | `database_path`, `session_id`, ... | Update an existing activity session |
| `activity.delete_session` | `database_path`, `session_id` | Delete an activity session |
| `activity.apply_correction` | `database_path`, `session_id`, `kind`, `activity_type?`, `start_time_unix_ms?`, `end_time_unix_ms?` | Apply a user correction to an activity session |
| `activity.correction_plans` | `database_path`, `session_id` | Get available correction plans for a session |
| `activity.attach_metric` | `database_path`, `metric_id`, `activity_session_id`, `metric_name`, `value`, `unit`, `start_time_unix_ms`, `end_time_unix_ms` | Attach a single metric to an activity session |
| `activity.attach_metrics` | `database_path`, `metrics: [...]` | Batch-attach metrics to activity sessions |
| `activity.list_metrics` | `database_path`, `activity_session_id` | List metrics attached to a session |
| `activity.list_intervals` | `database_path`, `activity_session_id` | List intervals attached to a session |
| `activity.attach_interval` | `database_path`, `interval_id`, `activity_session_id`, `interval_type`, `start_time_unix_ms`, `end_time_unix_ms`, `sequence` | Attach an interval to an activity session |
| `activity.metrics_for_session_in_window` | `database_path`, `activity_session_id`, `start_time_unix_ms`, `end_time_unix_ms` | Get metrics for a session within a time window |

#### Sleep

| Method | Key Args | Description |
|--------|----------|-------------|
| `sleep.import_external_history` | `database_path`, `sessions: [...]`, `stages: [...]` | Import sleep sessions and stages from an external source (e.g. HealthKit) |
| `sleep.add_correction_label` | `database_path`, `label_id`, `label_type`, `start_time_unix_ms`, `end_time_unix_ms` | Add a correction label for a sleep session |
| `sleep.list_correction_labels` | `database_path`, `start_time_unix_ms`, `end_time_unix_ms` | List sleep correction labels |
| `sleep.validate_stage_labels` | `database_path`, `input`, `min_label_confidence?` | Validate sleep stage labels against model output |
| `sleep.validate_window_labels` | `database_path`, `start`, `end` | Validate sleep window labels |
| `sleep.validate_v1_evidence_folder` | `evidence_dir`, `expected_manifest_sha256?` | Validate a sleep v1 evidence folder |
| `sleep.validate_v1_explanation_stability` | `input`, `max_repeated_run_delta?` | Validate sleep v1 explanation stability |
| `sleep.validate_v1_release_gates` | `input` | Evaluate sleep v1 release gate criteria |

#### Calibration

| Method | Key Args | Description |
|--------|----------|-------------|
| `calibration.apply` | `database_path`, `metric_family`, `algorithm_id`, `algorithm_version`, `raw_score`, `score_min`, `score_max` | Apply a calibration record to a raw score |
| `calibration.evaluate_dataset` | `dataset`, `options`, `database_path?`, `persist?` | Evaluate a calibration dataset |
| `calibration.evaluate_stored_labels` | `database_path`, `start`, `end`, `options`, `persist?` | Evaluate calibration using stored labels |
| `calibration.import_labels` | `database_path`, `labels: [...]` | Import calibration labels |
| `calibration.list_labels` | `database_path`, `start`, `end` | List stored calibration labels |

#### Overnight / Sync

| Method | Key Args | Description |
|--------|----------|-------------|
| `overnight.mirror_batch` | `database_path`, `sessions: [...]`, `raw_notifications: [...]`, `historical_range_polls: [...]` | Persist overnight capture data from multiple sources |
| `overnight.mirror_counts` | `database_path`, `session_id` | Count rows mirrored in an overnight session |
| `upload.get_recent_decoded_streams` | `database_path` | Retrieve decoded streams for server upload |

#### Commands

| Method | Key Args | Description |
|--------|----------|-------------|
| `commands.definitions` | _(none)_ | List all known BLE command definitions |
| `commands.evidence_template` | _(none)_ | Get the template for command evidence |
| `commands.evidence_from_emulator_log` | `log_text`, `source_log?`, `write_type?` | Extract command evidence from emulator log text |
| `commands.validate_evidence` | `evidence: [...]`, `database_path?`, `persist?` | Validate command evidence |
| `commands.promote_local_frame_matches` | `evidence: [...]`, `candidates: [...]` | Promote matched local frames as evidence |
| `commands.capture_plan` | `database_path`, `commands?` | Generate a capture plan for commands |
| `commands.list_validation_records` | `database_path` | List stored command validation records |
| `commands.import_validation_records` | `database_path`, `records: [...]` | Import command validation records |
| `commands.direct_send_gate` | `database_path`, `command` | Check gate status for direct BLE command send |
| `commands.direct_send_preflight` | `database_path`, `command`, `now_unix_ms`, `visible_user_intent?` | Run pre-flight checks before sending a BLE command |

#### Debug / WebSocket Session

| Method | Key Args | Description |
|--------|----------|-------------|
| `debug.start_session` | `database_path`, `session_id`, `started_at_unix_ms`, `bridge: {url, bind_host, token_required, ...}` | Start a debug WebSocket session |
| `debug.start_command` | `database_path`, `session_id`, `received_at_unix_ms`, `command` | Record the start of a debug command |
| `debug.finish_command` | `database_path`, `session_id`, `time_unix_ms`, `command_id`, `ok`, `message` | Record the completion of a debug command |
| `debug.record_event` | `database_path`, `session_id`, `time_unix_ms`, `source`, `level`, `topic`, `message` | Append an event to a debug session |
| `debug.session_snapshot` | `database_path`, `session_id` | Get a snapshot of the current debug session state |

#### Export / Privacy

| Method | Key Args | Description |
|--------|----------|-------------|
| `export.raw_timeframe` | `database_path`, `output_dir`, `start`, `end`, `app_version?`, `include_sqlite?` | Export raw data for a time window into a directory |
| `export.validate_bundle` | `path` | Validate an export bundle |
| `privacy.lint` | `path` | Lint a path for privacy-sensitive content |

#### Historical Sync

| Method | Key Args | Description |
|--------|----------|-------------|
| `historical_sync.dry_run` | `HistoricalSyncDryRunInput` fields | Dry-run historical sync and report what would happen |
| `historical_sync.physical_evidence_template` | `generation`, `capture_session_id?` | Get physical evidence template for a device generation |
| `historical_sync.validate_physical_evidence` | `HistoricalSyncPhysicalValidationInput` fields | Validate physical capture evidence |

#### Health Sync

| Method | Key Args | Description |
|--------|----------|-------------|
| `health_sync.dry_run` | `HealthSyncDryRunInput` fields | Dry-run HealthKit sync and report the plan |
| `health_sync.activity_dry_run` | `ActivityHealthSyncDryRunInput` fields | Dry-run HealthKit activity sync |

#### Diagnostics

| Method | Key Args | Description |
|--------|----------|-------------|
| `diagnostics.perf_budget` | `scale?` | Run the performance budget suite |
| `diagnostics.property_suite` | `seed?`, `cases_per_group?` | Run property-based tests |

#### OpenWHOOP Reference

| Method | Args | Description |
|--------|------|-------------|
| `openwhoop.reference_report` | _(none)_ | Return the bundled OpenWHOOP protocol reference (service UUIDs, characteristic roles, history fields) |

#### Timeline

| Method | Key Args | Description |
|--------|----------|-------------|
| `timeline.from_decoded_frames` | `decoded_frames: [DecodedFrameRow]` | Build a packet timeline from an array of decoded frame rows |

#### UI Coverage

| Method | Key Args | Description |
|--------|----------|-------------|
| `ui_coverage.audit` | `coverage_map_path?` | Audit UI coverage against known screens and states |

---

### Common Argument Patterns

**`database_path`** — Required on all storage-backed methods. The canonical path is:
`<ApplicationSupport>/GooseSwift/goose.sqlite`

Resolved at runtime via `HealthDataStore.defaultDatabasePath()`. Pass it explicitly in every call — the bridge does not infer it.

**Time ranges** — Methods accept time boundaries as either:
- Unix epoch seconds (`start`, `end` as ISO-8601 strings for store queries)
- Unix milliseconds (`start_time_unix_ms`, `end_time_unix_ms` as `i64`)

Check the specific method's args struct for the exact field names and types.

**`scope`** — Algorithm preference scope. Defaults to `"device"` in the bridge.

---

## WebSocket Debug Endpoint

The iOS app and server support a local WebSocket debug interface used during live capture sessions.

**URL pattern:**
```
ws://127.0.0.1:8765/goose-debug/stream?token=<session-token>
```

The `debug.start_session` bridge method records the WebSocket bridge configuration:

```swift
bridge.request(
  method: "debug.start_session",
  args: [
    "database_path": databasePath,
    "session_id": debugSessionID,
    "started_at_unix_ms": ...,
    "bridge": [
      "url": "ws://127.0.0.1:8765",
      "bind_host": "127.0.0.1",
      "token_required": true,
      "token_present": false,
      "remote_bind_enabled": false,
      "visible_remote_bind_toggle": true,
    ],
  ]
)
```

The `server/dashboard/server.py` script binds the dashboard WebSocket server to `127.0.0.1:8765`. This is a development/debug tool — not part of the production ingest path.

The debug session lifecycle uses four bridge methods in sequence:
1. `debug.start_session` — opens the session and records bridge config
2. `debug.start_command` / `debug.finish_command` — bracket individual debug commands
3. `debug.record_event` — append arbitrary events to the session log
4. `debug.session_snapshot` — read the current session state at any time
