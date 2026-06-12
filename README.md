<!-- generated-by: gsd-doc-writer -->
> **Disclaimer — Unofficial Project / Personal Data Research**
>
> Goose is an independent, unofficial project not affiliated with, endorsed by, or supported by WHOOP, Inc. This project accesses biometric data exclusively over Bluetooth Low Energy from the user's own hardware — it does not touch WHOOP's servers or APIs. It was built for personal research and data portability purposes only, grounded in GDPR Art. 20 (right to data portability) and EU Directive 2009/24/EC Art. 6 (interoperability exception).
>
> See [DISCLAIMER.md](DISCLAIMER.md) for the full legal statement.

# Goose - Local Companion for WHOOP Devices

> **Active fork of [b-nnett/goose](https://github.com/b-nnett/goose).** This fork extends the original with a self-hosted FastAPI + TimescaleDB server, automatic biometric upload from iOS, and an expanded health metrics pipeline. The upstream project appears inactive; development continues here.

**Alpha proof of concept. This build is for developers to evaluate whether a project of this scope is viable. It is not ready to use as an app for tracking personal health data yet.**

If you don't know what Xcode is, or how to build the Rust core, this build is not for you. Come back on 13 June 2026 for the first public beta on TestFlight.

![Goose app hero showing a connected WHOOP 5.0 device](docs/assets/readme-hero.png)

This prototype targets WHOOP 5.0 and WHOOP 4.0. Other WHOOP generations are not supported in this build.

The app and backend have had very little attention put into performance. The app will lag, very considerably. Performance PRs are welcome, or you can wait until I address it in due course.

Goose is a local-first WHOOP data and health metrics project. The iOS app connects to WHOOP bands, routes packet data through the Goose Rust core, and turns that data into daily health, recovery, sleep, strain, stress, cardio, energy, coach, and debug views. An optional self-hosted server lets you persist decoded biometric streams outside the device.

## What Shipped in v5.0

v5.0 is the first milestone where the full biometric pipeline is closed end to end.

**Algorithms and metrics**

- HRV accuracy: BLE-gap aware RMSSD computation with ectopic beat filter.
- Sleep staging: Cole-Kripke activity-count classifier and 4-class AASM stage model (Wake / REM / Light / Deep).
- Strain and calories: Ghidra-confirmed WHOOP coefficients for strain score and calorie expenditure.
- Readiness Engine v1: ACWR (acute:chronic workload ratio) with Foster monotony over a 28-day strain window; outputs a readiness level and zone (optimal / rundown / primed).

**Biometric decode**

- V24 packet decode for SpO2, skin temperature, respiration rate, and gravity2 streams.
- Exercise detection from the decoded biometric stream.

**Upload sync infrastructure**

- 6 stream tables carry a `synced` flag: `battery`, `events`, `exercise_sessions`, `gravity2_samples`, `hr_samples`, `rr_intervals`.
- Pending-upload queries and mark-synced operations are available on all 6 tables.

**Rust core**

- SQLite schema v19.
- 45 integration test files in `Rust/core/tests/`.

## Project Layout

```text
GooseSwift/                         SwiftUI app source
GooseSwiftTests/                    XCTest suite for Swift components
GooseWorkoutLiveActivityExtension/  Live Activity widget extension
Rust/                               iOS static library, headers, per-platform outputs
Scripts/build_ios_rust.sh           Xcode build phase for the Goose Rust core
server/                             Self-hosted FastAPI+TimescaleDB server (Docker)
docs/guides/                        Getting started, development, testing, configuration guides
docs/architecture/                  System overview and component diagrams
docs/api/                           Server API reference
GooseSwift.xcodeproj                Xcode project
```

Key Swift entry points:

- `GooseSwiftApp.swift`: app lifecycle and deep-link handling.
- `RootView.swift`: onboarding gate and global sync toast host.
- `AppShellView.swift`: tab shell and shared health store wiring.
- `GooseAppModel.swift`: app state, BLE ownership, lifecycle, and bridge summaries.
- `GooseBLEClient.swift`: Bluetooth scan/connect/sync logic.
- `GooseRustBridge.swift`: Swift wrapper around the Rust C bridge.
- `HealthView.swift` and `Health*` files: health dashboards, metric pages, trends, and sheets.
- `CoachView.swift` and `Coach*` files: coach UI and chat support.
- `MoreView.swift`: operational/debug/settings surfaces.

This is an active prototype. Because the data pipeline is still evolving, some metrics appear as empty or unavailable until the app has a source for them.

## Independence

Goose is an independent project and is not affiliated with WHOOP. This repository does not include or reference source code owned by WHOOP. The app communicates with WHOOP bands over Bluetooth using services and data exposed by the device, then parses and stores that local data through the Goose Rust core. Product names are used only to describe compatibility.

## Design Credit

The current health metric UI draws heavily from [Bevel](https://www.bevel.health/), especially the Sleep, Recovery, Strain, Stress, and trend-detail surfaces. Bevel is not affiliated with Goose; this credit is here because their product design has been a major visual reference.

## Acknowledgements

This fork is built on top of [b-nnett/goose](https://github.com/b-nnett/goose), the original Goose iOS project. The iOS app shell, BLE protocol work, Rust core architecture, and WHOOP packet parsing are derived from that upstream codebase.

The BLE pairing model, on-wrist detection approach, and background sync architecture draw from [Noop](https://github.com/NoopApp/noop), an offline WHOOP companion that pioneered local-first WHOOP data access on iOS. Noop is not affiliated with Goose; this credit is here because it established many of the patterns this project builds on.

The self-hosted server and biometric algorithm pipeline are adapted from [my-whoop](https://github.com/tigercraft4/my-whoop), a prior personal project for storing and analysing WHOOP data on a self-hosted FastAPI + TimescaleDB stack, itself forked from [johnmiddleton12/wearable](https://github.com/johnmiddleton12/wearable). The server in `server/ingest/` maintains API compatibility with my-whoop so existing deployments continue to work.

## Current Scope

- SwiftUI app shell with Home, Health, Coach, and More tabs.
- Onboarding and persisted profile state.
- CoreBluetooth scan/connect flows for WHOOP 5.0 and WHOOP 4.0 devices.
- JSON-over-C bridge into the Goose Rust core.
- Self-hosted server (`server/`): FastAPI + TimescaleDB, Dockerized; supports both device generations via `device_generation` field.
- Automatic upload of decoded biometric data from iOS to server (10 stream tables with `synced` flag).
- Health metric surfaces for Sleep, Recovery, Strain, Stress, Cardio Load, Energy Bank, Health Monitor, Packet Inputs, Algorithms, References, and Calibration.
- HealthKit sleep import and workout write support.
- Coach surfaces that summarize local metrics and explain missing data.
- More/Debug operational surfaces for device state, capture, sync, algorithms, storage, privacy, and support.
- Workout Live Activity extension.

## Requirements

- macOS with Xcode installed.
- iOS 26.0 SDK and an iOS 26.0 capable simulator/device.
- Apple Developer signing configured for the `com.tigercraft4.goose` bundle identifier.
- Rust and Cargo for building the Goose Rust core from the committed `Rust/core` source.
- iOS Rust targets installed with `rustup`; see the Rust Core Bridge section below.
- Docker (for the self-hosted server — optional).

Built Rust `.a` archives (`Rust/iphoneos/libgoose_core.a` and `Rust/iphonesimulator/libgoose_core.a`) are committed to the repository as pre-built artifacts. Set `GOOSE_SKIP_RUST_CORE_BUILD=1` to skip rebuilding when the committed archives are already valid for the active Xcode platform.

## Build

Clone the repository first:

```bash
git clone https://github.com/tigercraft4/goose.git
cd goose
```

Open `GooseSwift.xcodeproj` in Xcode and build the `GooseSwift` scheme, or build from the command line.

Simulator build:

```sh
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath /tmp/goose-swift-deriveddata \
  build
```

Physical device build:

```sh
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

```sh
xcrun devicectl list devices
```

## Reinstall On A Device

After a successful physical-device build, reinstall and launch:

```sh
xcrun devicectl device uninstall app \
  --device <device-id> \
  com.tigercraft4.goose

xcrun devicectl device install app \
  --device <device-id> \
  /tmp/goose-swift-deriveddata-device/Build/Products/Debug-iphoneos/GooseSwift.app

xcrun devicectl device process launch \
  --device <device-id> \
  --terminate-existing \
  com.tigercraft4.goose
```

## Self-Hosted Server

The `server/` directory contains an optional FastAPI + TimescaleDB backend. The iOS app works standalone without it.

```bash
cd server
cp .env.example .env
# Set GOOSE_API_KEY and GOOSE_DB_PASSWORD in .env
docker compose up -d --build
```

Check it started: `curl -s localhost:8770/healthz` → `{"status":"ok"}`

Configure the server URL and Bearer token in the iOS app under More > Server Settings. See `server/README.md` for API details and the full list of environment variables.

## Rust Core Bridge

The Rust bridge source is committed in `Rust/core`. Do not commit built `.a`
archives; Xcode generates them locally through `Scripts/build_ios_rust.sh`.

Prerequisites:

- Xcode command line tools.
- Rust via `rustup`.
- iOS Rust targets:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

`Scripts/build_ios_rust.sh` builds `Rust/core` for the active Xcode platform:

- `iphoneos` -> `aarch64-apple-ios`
- `iphonesimulator` on Apple Silicon -> `aarch64-apple-ios-sim`
- `iphonesimulator` on Intel -> `x86_64-apple-ios`

Outputs are staged into:

```text
Rust/iphoneos/libgoose_core.a
Rust/iphonesimulator/libgoose_core.a
```

The Swift target links `Rust/$(PLATFORM_NAME)/libgoose_core.a` and reads the C
bridge header from `Rust/core/include/goose_core_bridge.h`. The default Cargo
target directory is `build/rust-target/goose-core`, so Rust build products stay
outside the committed source tree.

Manual builds:

```bash
# Simulator on Apple Silicon
PLATFORM_NAME=iphonesimulator CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh

# Physical iPhone
PLATFORM_NAME=iphoneos CURRENT_ARCH=arm64 Scripts/build_ios_rust.sh
```

You normally do not need to run these by hand; the Xcode build phase runs the
script before compiling Swift.

## Data And Privacy

- Metric views show empty, stale, or unavailable states when a source is missing.
- Metric rows and trend sheets show where values came from when that information is available.
- Raw packet payloads stay in debug/export flows rather than everyday health views.
- Coach responses use the same local metric summaries shown in the app.
- Health and fitness data is local by default. Any future backend or AI feature will need its own consent flow and privacy notes.

## Documentation

Guides and reference docs:

- `docs/guides/getting-started.md`: prerequisites, clone, first run, and common setup issues.
- `docs/guides/development.md`: local setup, build commands, code style, and PR process.
- `docs/guides/testing.md`: Rust test suite, coverage, and CI integration.
- `docs/guides/configuration.md`: environment variables and server configuration.
- `docs/architecture/overview.md`: system overview, component diagram, and data flow.
- `docs/api/reference.md`: server API endpoints, request/response formats, and authentication.

## Contributing

This project moves quickly, so small focused changes are easiest to review.

Want to talk to other contributors? [Join the discussion on GitHub](https://github.com/tigercraft4/goose/discussions).

- Keep changes close to the feature or bug you are working on.
- Match the existing SwiftUI style before introducing new patterns.
- Build after touching Swift, Rust bridge, project, or signing settings.
- Check both empty and populated states for metric UI when possible.
- Keep user-facing health copy plain and careful. Avoid medical claims.
- Put debug tooling, packet details, and raw export behavior under More or Debug surfaces.
- Update the relevant MVP doc when a change completes or changes an open task.
- Mention any build warnings, skipped checks, or device-only assumptions in the PR notes.

See [CONTRIBUTING.md](CONTRIBUTING.md) for full guidelines including code style, Rust bridge conventions, and the PR checklist.

## Development Notes

- Prefer small, typed Swift models over displaying raw summary strings.
- Keep Home, Health, Coach, and More routes modular enough to work independently.
- Metric pages should still look polished when data is missing.
- Before installing to a device, run a simulator or device build and check that the Rust library target matches the destination platform.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
