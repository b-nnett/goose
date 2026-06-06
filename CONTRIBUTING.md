<!-- generated-by: gsd-doc-writer -->
# Contributing to Goose

This project moves quickly. Small, focused changes are easiest to review.

Want to talk to other contributors? [Join the group chat here](https://x.com/i/chat/group_join/g2061785795330019536/3SHQtt2O8f).

---

## Development Environment Setup

See [Getting Started](docs/guides/getting-started.md) for prerequisites and first-run instructions, and [Development Guide](docs/guides/development.md) for the full local development workflow.

### Prerequisites

- macOS with Xcode installed (iOS 26 SDK required)
- Apple Developer account with signing configured for bundle ID `com.goose.swift`
- Rust toolchain via `rustup`
- iOS Rust targets:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

- Docker (optional — only needed for the self-hosted server in `server/`)

### Clone and open

```bash
git clone https://github.com/tigercraft4/goose.git
cd goose
open GooseSwift.xcodeproj
```

The Rust core is built automatically as an Xcode build phase via `Scripts/build_ios_rust.sh`. You do not need to run it manually.

For the self-hosted server:

```bash
cd server
cp .env.example .env
# Set GOOSE_API_KEY and GOOSE_DB_PASSWORD in .env
docker compose up -d --build
```

---

## Building

### Simulator

```bash
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath /tmp/goose-swift-deriveddata \
  build
```

### Physical device

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

List connected devices with `xcrun devicectl list devices`.

Always verify the Rust library target matches the destination platform before installing. The static library is built per-platform; simulator and device archives are separate files.

### Rust core only

```bash
# Simulator (Apple Silicon)
PLATFORM_NAME=iphonesimulator CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh

# Physical iPhone
PLATFORM_NAME=iphoneos CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh
```

Do not commit built `.a` archives. The paths `Rust/iphoneos/libgoose_core.a` and `Rust/iphonesimulator/libgoose_core.a` are local build outputs.

---

## Running Tests

### Rust core

The Rust test suite runs on any platform (including Linux/CI):

```bash
cd Rust/core
cargo test --locked --no-fail-fast
```

There are 40+ integration tests in `Rust/core/tests/`. CI runs these automatically on every pull request that touches `Rust/core/` via the `Rust Core CI` workflow (`.github/workflows/rust-core-ci.yml`).

There is no Swift test target in `GooseSwift.xcodeproj`. iOS behaviour is verified by building and running on simulator or device.

---

## Code Style

### Swift

- **Indentation:** 2 spaces throughout — no tabs.
- **Braces:** Opening brace on the same line as the declaration (K&R style).
- **Types:** PascalCase — `GooseBLEClient`, `HealthDataStore`, `ActivityModels`.
- **Methods and properties:** camelCase — `handleNotification`, `isScanning`, `liveHeartRateBPM`.
- **Booleans:** Prefix with `is`, `can`, `has`, or `should`.
- **File naming:** Match the primary type — `GooseBLEClient.swift`. Use `+` suffix files for concern-scoped extensions — `GooseBLEClient+Commands.swift`, `GooseAppModel+OvernightRun.swift`.
- **One blank line** between methods within a type; **two blank lines** between top-level declarations in an extension file.
- `private` for all internal state; `private(set)` for read-only `@Published` properties.
- No `///` doc comments — inline `//` comments explain non-obvious logic only.
- No TODO, FIXME, HACK, or XXX markers.

There is no formatter config file. Match the surrounding file's style exactly. Run a simulator build after any Swift change to confirm it compiles.

### Rust

- Edition 2024, MSRV 1.94.
- Run `cargo clippy` and `cargo fmt` before submitting. Clippy is non-blocking in CI but warnings should not be introduced.
- Follow the existing module structure under `Rust/core/src/`.

---

## Rust Bridge Conventions

These rules apply whenever touching `GooseRustBridge` or adding bridge call sites:

- **Always pass `database_path` in every bridge call that accesses storage.** The Rust side is stateless; all persistence goes through the path argument. Use `HealthDataStore.defaultDatabasePath()` to resolve it.
- **Never call `GooseRustBridge` from `@MainActor` inline.** `goose_bridge_handle_json` is synchronous and blocks the calling thread. Always dispatch to a background `DispatchQueue` first, then return to `@MainActor` via `Task { @MainActor in ... }`.
- **Do not create ad-hoc `GooseRustBridge()` instances per call site.** Use the instance already owned by the relevant coordinator (`GooseAppModel`, `HealthDataStore`, `OvernightSQLiteMirrorQueue`, `CaptureFrameWriteQueue`). If a new long-lived coordinator needs bridge access, give it its own instance — the bridge is intentionally not a singleton.

---

## PR Guidelines

- Keep changes close to the feature or bug you are working on. Avoid bundling unrelated fixes.
- Match the existing SwiftUI style before introducing new patterns.
- Build after touching Swift source, Rust bridge, the Xcode project, or signing settings.
- Check both empty and populated states for any metric UI you change. Metric pages must remain polished when data is missing.
- Keep user-facing health copy plain. Avoid medical claims.
- Put debug tooling, packet details, and raw export behaviour under More or Debug surfaces — not in everyday health views.
- Update the relevant MVP doc in `docs/goose-swift-mvp/` when a change completes or changes an open task.
- Mention any build warnings, skipped checks, or device-only assumptions in the PR description.
- For any change that touches the Rust core, confirm the Rust test suite still passes locally before opening the PR.

---

## Architecture Overview

Before making structural changes, read `docs/architecture/overview.md`. Key boundaries to respect:

- `GooseAppModel` is the single `@MainActor` coordinator. UI observes it via `@EnvironmentObject`. Do not introduce a second coordinator.
- BLE bytes flow inward through `GooseBLEClient` → frame reassembly → `GooseRustBridge` → `CaptureFrameWriteQueue`. Do not short-circuit this pipeline.
- `GooseWorkoutLiveActivityExtension` has no access to `GooseAppModel` or `GooseRustBridge`. Keep the extension target isolated.
- `HeartRateSeriesStore.shared` is the only module-level singleton. Do not introduce new ones.

---

## Issue Reporting

Open an issue on GitHub with:

1. What you expected to happen.
2. What actually happened (include any console output or crash log if applicable).
3. Steps to reproduce, including WHOOP device generation (5.0 or 4.0) and iOS version.
4. Whether the problem occurs on simulator, physical device, or both.

---

## License

By contributing you agree that your changes will be released under the project's [GPL-3.0-or-later](LICENSE) license.
