<!-- generated-by: gsd-doc-writer -->
# Testing

Goose has three independent test surfaces: the **Rust core** (42 integration test files, runs on Linux and macOS), the **server stack** (pytest suite against FastAPI + TimescaleDB), and the **iOS app** (no automated test target; verified manually with a physical WHOOP device). There is no Swift/XCTest target in the project.

---

## Rust Core

### Test framework

Cargo's built-in test runner. Integration tests live in `Rust/core/tests/` (42 files). Unit tests are collocated with source modules in `Rust/core/src/`. The crate ships many binaries used by integration tests as CLI fixtures, including `goose-fixture-index`, `goose-parser-fixture-runner`, `goose-capture-import`, `goose-capture-sqlite-import`, `goose-local-health-validation-suite`, `goose-reference-algo-runner`, `goose-property-test-suite`, and others defined in `Rust/core/Cargo.toml`.

A `Rust/core/fixtures/` directory provides golden hex frames and synthetic capture data consumed by tests such as `fixture_tests.rs`, `capture_import_tests.rs`, and `protocol_tests.rs`.

Several integration tests (notably `reference_tests.rs` and `reference_runner_cli_tests.rs`) shell out to `python3` for the external reference adapters in `Rust/core/tools/reference/`. These adapters fall back to hand-derived values when optional science packages (neurokit2, pyHRV, pyactigraphy) are absent, so `python3` must be on `PATH` but a full Python environment is not required.

### Running tests

From the `Rust/core` directory:

```bash
# Full suite — all integration and unit tests
cargo test --locked

# Keep going on failure instead of stopping at the first error
cargo test --locked --no-fail-fast

# Library unit tests only (no integration test files)
cargo test --lib --locked

# A single integration test file
cargo test --test bridge_tests --locked

# A single test by name (substring match)
cargo test --locked bridge_returns_core_version_payload
```

Build all targets (library + binaries + tests) without running:

```bash
cargo build --all-targets --locked
```

### Writing new tests

- Integration tests go in `Rust/core/tests/` following the `<domain>_tests.rs` naming pattern (e.g., `store_tests.rs`, `protocol_tests.rs`).
- Unit tests go inside `src/<module>.rs` under a `#[cfg(test)]` block.
- Tests that exercise the C bridge use the two-symbol FFI pair: `goose_bridge_handle_json` / `goose_bridge_free_string`. See `tests/bridge_tests.rs` for the pattern.
- Tests requiring a real SQLite database use `tempfile::NamedTempFile` (already a dev dependency) to create a throwaway path.
- Hex frame fixtures go in `Rust/core/fixtures/`; document them in `fixtures/README.md`.

### Coverage

No coverage threshold is configured. CI runs `cargo test --locked --no-fail-fast` and `cargo test --lib --locked` as blocking gates.

---

## Server (FastAPI + TimescaleDB)

### Test framework

pytest >= 8.0 with httpx >= 0.27 as the FastAPI `TestClient` transport. Test files are in `server/ingest/tests/`. Install test dependencies separately from the production image:

```bash
cd server/ingest
pip install -r requirements-dev.txt   # includes -r requirements.txt
```

`requirements-dev.txt` adds `pytest>=8` and `httpx>=0.27` on top of the production stack.

### Running tests

From `server/ingest`:

```bash
# Full suite
pytest tests/

# A single file
pytest tests/test_ingest_decoded_api.py

# A single test by name
pytest tests/test_store.py::test_upsert_hr_sample -v

# Skip tests that need Docker (unit-only run)
pytest tests/ -m "not docker"
```

### Integration tests and Docker

Tests that exercise the real TimescaleDB schema (test files such as `test_e2e.py`, `test_ingest_decoded_api.py`, `test_read_api.py`, `test_store.py`) use the `timescale_dsn` and `clean_db` fixtures defined in `tests/conftest.py`. These fixtures:

1. Pull and start `timescale/timescaledb:2.17.2-pg16` in a throwaway container.
2. Apply `server/db/init.sql` to create the schema.
3. Truncate all data tables before each test for isolation.
4. Remove the container on teardown.

Tests that require Docker are decorated with `@requires_docker` and are skipped automatically if `docker` is not on `PATH` or `docker info` returns a non-zero exit code. You do not need to start the server manually — `TestClient` runs the FastAPI app in-process.

Docker must be running for the integration suite:

```bash
docker info   # verify Docker daemon is reachable
pytest tests/ # integration tests self-manage the TimescaleDB container
```

### Test file reference

| File | What it covers |
|------|---------------|
| `test_ingest_decoded_api.py` | `/v1/ingest-decoded` endpoint contract |
| `test_ingest_api.py` | General ingest endpoint contract including auth and archive-only mode |
| `test_read_api.py` | Read endpoints, pagination, filtering |
| `test_read.py` | Lower-level read helpers: device listing, stream queries, downsampling |
| `test_ingest_pipeline.py` | Full ingest pipeline including stream routing |
| `test_e2e.py` | End-to-end: decode → ingest → compute → read → idempotency |
| `test_store.py` | Database upsert and hypertable row counts |
| `test_daily.py`, `test_daily_alg.py` | `/v1/compute-daily` and derived metrics |
| `test_sleep.py`, `test_recovery.py`, `test_strain.py` | Metric-specific endpoint responses |
| `test_hrv.py`, `test_baselines.py`, `test_calories_rmr.py` | Algorithm accuracy checks |
| `test_validation.py` | Schema validation and error response shapes |
| `test_archive.py` | Raw frame archive storage and retrieval |
| `test_units.py` | Unit-level helpers (no Docker required) |
| `test_activity.py`, `test_exercise.py` | Activity and exercise session endpoints |
| `test_whoop_api.py` | WHOOP OAuth client: offline mock transport, token refresh lifecycle |
| `test_profile_calories_workouts.py` | Profile storage, `/v1/profile`, and calorie estimation for bouts |
| `../client/test_uploader.py` | `server/client/uploader.py` frame loading and batching logic |

### Writing new tests

- Place new files in `server/ingest/tests/` using the `test_<area>.py` naming convention.
- Unit tests (no DB needed) can run without the `clean_db` fixture.
- Integration tests that touch TimescaleDB must use the `client` fixture (which calls `clean_db`) so the container is set up and data is truncated before each test.
- Set required environment variables via `monkeypatch.setenv` — do not rely on a real `.env` file in tests.
- Synthetic fixture data lives in `server/ingest/tests/fixtures/`; the `hist_biometric.bin` binary is a 762-record synthetic capture used by `test_e2e.py`.

---

## iOS App

There is no Swift test target in `GooseSwift.xcodeproj`. iOS functionality is verified manually.

### Build verification

The minimum check before a PR is a clean Xcode build for the target destination:

```bash
# Simulator
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath /tmp/goose-swift-deriveddata \
  build

# Physical device
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS,id=<device-id>' \
  -derivedDataPath /tmp/goose-swift-deriveddata-device \
  -allowProvisioningUpdates \
  build
```

### Manual BLE and upload testing

Testing BLE capture and server upload requires a physical WHOOP device (5.0 or 4.0) and a physical iPhone running iOS 26.

**Device connection**

1. Build and install the app on a physical device (see README.md for `xcrun devicectl` commands).
2. Open the app and complete onboarding.
3. Navigate to the Home tab; tap **Scan** to start a CoreBluetooth scan.
4. Select your WHOOP device from the discovered list.
5. Verify the connection state indicator turns active and heart rate values appear in real time.

**BLE capture verification**

1. Navigate to **More > Debug** after connecting.
2. Confirm that live packet counts are incrementing.
3. Use **More > Capture** to start a capture session. Let it run for at least 60 seconds.
4. Verify the capture session status shows received frame counts.

**Server upload flow**

Prerequisites: the self-hosted server must be running (see `server/README.md`).

1. Configure the server URL and Bearer token in **More > Remote Server**.
   - URL example: `http://goose.local:8770` (use a hostname, not a bare IP address).
   - Token: the `GOOSE_API_KEY` value from your server `.env`.
   - Enable the **Upload** toggle.
2. With a WHOOP device connected, data should upload automatically after a sync.
3. Verify delivery by checking the server health and querying a read endpoint:

   ```bash
   curl -s localhost:8770/healthz
   # → {"status":"ok"}

   curl -s -H "Authorization: Bearer <your-key>" \
     "localhost:8770/v1/daily?device_id=<device-id>&limit=1"
   ```

4. Check the More > Remote Server status indicator in the app for upload confirmation or error messages.

**Empty and error state verification**

- Check each metric view (Sleep, Recovery, Strain, Stress, Cardio, Energy) in both populated and empty states.
- Verify that metric rows and trend sheets show the data source label when available.
- Disconnect the WHOOP device and confirm the UI gracefully shows unavailable or stale states rather than crashing.

---

## CI

Two workflows run the automated test gates on every push and pull request that touches `Rust/` or `Rust/core/`.

### `rust-core-ci.yml` — Build, test, and lint

Runs on `ubuntu-latest`. Triggered by pushes and PRs that touch `Rust/core/**` or the workflow file itself, and on `workflow_dispatch`.

Steps:
1. Install stable Rust toolchain with clippy.
2. Confirm `python3` is available (needed by reference adapter tests).
3. Cache `~/.cargo/registry`, `~/.cargo/git`, and `Rust/core/target`.
4. `cargo build --all-targets --locked`
5. `cargo test --locked --no-fail-fast` (blocking)
6. `cargo clippy --all-targets --locked || true` (non-blocking — surfaces warnings without failing the build)

### `rust-core.yml` — Format, build, test (MSRV matrix)

Runs on `ubuntu-latest` and `macos-15` against Rust 1.96 (MSRV). Triggered by pushes and PRs that touch `Rust/**`.

Jobs:
- **fmt** — `cargo fmt --all -- --check` (blocking)
- **build-test** (matrix: ubuntu-latest, macos-15):
  1. Install Rust 1.96 via rustup.
  2. `cargo build --lib --verbose`
  3. `cargo test --lib --verbose`
- **clippy** — `cargo clippy --lib --no-deps -- -D warnings` (advisory, `continue-on-error: true`)

### `security.yml` — Dependency audit

Runs on a weekly schedule (Mondays, 07:00 UTC) and on pushes/PRs that touch dependency manifests.

- **cargo-audit** — audits `Rust/core/Cargo.lock` against the RustSec advisory database; fails on any known-vulnerable dependency.
- **trivy** — filesystem scan for vulnerable dependencies, secrets, and misconfigurations across Rust, Python, and workflow files; fails on HIGH or CRITICAL findings.

### `codeql.yml` — Static analysis

Runs on every push and PR to `main`, and weekly (Mondays, 08:00 UTC).

- Covers Swift (`GooseSwift/`) and Python (`server/`) source.
- Rust is excluded from CodeQL; advisories are handled by `cargo-audit`.

The iOS app is not built or tested in CI — it requires macOS with Xcode and a physical device, neither of which the current workflow matrix provisions.

### `server-ci.yml` — Server pytest suite

Runs on `ubuntu-latest`. Triggered by pushes and PRs that touch `server/**`.

Steps:
1. Set up Python 3.12.
2. Install `server/packages/whoop-protocol` as a local editable package.
3. Install `server/ingest/requirements-dev.txt`.
4. Confirm the Docker daemon is reachable (`docker info`).
5. `pytest tests/ -v --tb=short` — all tests; the `conftest.py` fixtures self-manage the TimescaleDB container.
