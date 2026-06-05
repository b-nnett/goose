# Goose - Local Companion for WHOOP 5.0

**Alpha proof of concept. This build is for developers to evaluate whether a project of this scope is viable. It is not ready to use as an app for tracking personal health data yet.**

If you don't know what Xcode is, or how to build the Rust core, this build is not for you. Come back on 13 June 2026 for the first public beta on TestFlight.

![Goose app hero showing a connected WHOOP 5.0 device](docs/assets/readme-hero.png)

This prototype targets WHOOP 5.0. **Experimental WHOOP 4.0 (Gen4) support** has been added (see the WHOOP 4.0 section below and [`docs/WHOOP_4.0_GEN4.md`](docs/WHOOP_4.0_GEN4.md)): a 4.0 band connects, completes the handshake, streams live heart rate, and decodes HRV, respiratory rate, resting HR and strain from its history (SpO2/skin-temp need a reference calibration; recovery/sleep need more days of data).

The app and backend have had very little attention put into performance. The app will lag, very considerably. Performance PRs are welcome, or you can wait until I address it in due course.

Goose is a local-first WHOOP 5.0 data and health metrics project. The iOS app connects to WHOOP 5.0 bands, routes packet data through the Goose Rust core, and turns that data into daily health, recovery, sleep, strain, stress, cardio, energy, coach, and debug views.

## WHOOP 4.0 (Gen4)

Experimental support for the WHOOP 4.0 band. The inbound parser was already
generation-aware; this adds the missing outbound half (Gen4 framing, handshake,
and commands) plus several stability/performance fixes found on real hardware.
Full write-up: [`docs/WHOOP_4.0_GEN4.md`](docs/WHOOP_4.0_GEN4.md).

What was added:

- **Gen4 BLE framing** — a 4-byte-header + CRC8 frame builder/deframer and a
  generation-aware command dispatcher (Gen4 `61080002` vs Gen5 `fd4b0002`), with a
  Gen4 hello (`GetHelloHarvard`, cmd 35) and a Gen4 historical-sync sequence.
- **Live heart rate on Gen4** via the standard BLE Heart Rate service (180D/2A37).

What was fixed (mostly surfaced while bringing up Gen4, but general):

- **`unsupported device_type: GEN4`** — the Rust bridge only accepted the string
  `"GEN_4"`, so every Gen4 frame was rejected; now `"GEN4"` is accepted.
- **FFI panic safety** — the C-FFI dispatch now uses `catch_unwind` and the release
  profile uses `panic = "unwind"`, so a Rust panic returns a JSON error instead of
  crashing the app.
- **Lag** — a 12-hour, full-rate packet capture that auto-started on every connect
  (persisting every frame to SQLite) is no longer automatic; it's opt-in.
- **Export OOM crash** — the default raw export no longer pulls raw bytes and the
  huge `sensor_samples` table fully into memory.
- **Unbounded database growth** — raw-payload retention is capped (24 MB), live
  writes compact, and a single history sync pass is bounded, so a WHOOP history
  backlog can't balloon the on-device database.

Recovery metrics now decode from the band's own history. The V12/V24 `normal_history`
frames carry the strap's DSP output, so **HRV (RMSSD), respiratory rate, resting HR and
strain** are decoded and user-visible on Gen4 (verified against a real on-device capture).
**SpO2** and **skin temperature** stay intentionally gated — both need a factory/reference
calibration the test device (no WHOOP app) couldn't provide. **Recovery** is wired and
activates once a few days of baseline data accumulate; **sleep staging** needs an overnight
capture plus a reference-labelled classifier. See [`docs/WHOOP_4.0_GEN4.md`](docs/WHOOP_4.0_GEN4.md).

Tests for the Gen4 work live in `Rust/core/tests/gen4_protocol_tests.rs` and
`gen4_outbound_verification.rs`.

## Project Layout

```text
GooseSwift/                         SwiftUI app source
GooseWorkoutLiveActivityExtension/  Live Activity widget extension
Rust/                               iOS static library, headers, per-platform outputs
Scripts/build_ios_rust.sh           Xcode build phase for the Goose Rust core
docs/goose-swift-mvp/               MVP plans, contracts, and data-readiness docs
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

Goose is an independent project and is not affiliated with WHOOP. This repository does not include or reference source code owned by WHOOP. The app communicates with WHOOP 5.0 bands over Bluetooth using services and data exposed by the device, then parses and stores that local data through the Goose Rust core. Product names are used only to describe compatibility.

## Design Credit

The current health metric UI draws heavily from [Bevel](https://www.bevel.health/), especially the Sleep, Recovery, Strain, Stress, and trend-detail surfaces. Bevel is not affiliated with Goose; this credit is here because their product design has been a major visual reference.

## Current Scope

- SwiftUI app shell with Home, Health, Coach, and More tabs.
- Onboarding and persisted profile state.
- CoreBluetooth scan/connect flows for WHOOP 5.0 devices.
- JSON-over-C bridge into the Goose Rust core.
- Health metric surfaces for Sleep, Recovery, Strain, Stress, Cardio Load, Energy Bank, Health Monitor, Packet Inputs, Algorithms, References, and Calibration.
- HealthKit sleep import and workout write support.
- Coach surfaces that summarize local metrics and explain missing data.
- More/Debug operational surfaces for device state, capture, sync, algorithms, storage, privacy, and support.
- Workout Live Activity extension.

## Requirements

- macOS with Xcode installed.
- iOS 26 SDK and an iOS 26 capable simulator/device.
- Apple Developer signing configured for the `com.goose.swift` bundle identifier.
- Rust and Cargo for building the Goose Rust core from the committed `Rust/core` source.
- iOS Rust targets installed with `rustup`; see the Rust Core Bridge section below.

Built Rust `.a` archives are generated locally during Xcode builds and are not committed. Set `GOOSE_SKIP_RUST_CORE_BUILD=1` only when the matching local archive already exists for the active Xcode platform.

## Build

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
  com.goose.swift

xcrun devicectl device install app \
  --device <device-id> \
  /tmp/goose-swift-deriveddata-device/Build/Products/Debug-iphoneos/GooseSwift.app

xcrun devicectl device process launch \
  --device <device-id> \
  --terminate-existing \
  com.goose.swift
```

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

Detailed implementation plans live in `docs/goose-swift-mvp/`:

- `Home.md`: Home tab contract and remaining work.
- `Health.md`: Health surfaces, metric pages, packet inputs, trends, and acceptance checks.
- `Coach.md`: Coach tab plan and chat architecture notes.
- `More.md`: operational settings/debug/capture/privacy surfaces.
- `CodexCoachServer.md`: viability notes for a future Codex-powered coach.
- `RemainingDataTodo.md`: unresolved data-source and persistence work.

Recovery-specific follow-up work is tracked in `recovery-todo.md`.

## Contributing

This project moves quickly, so small focused changes are easiest to review.

Want to talk to other contributors? [Join the group here](https://x.com/i/chat/group_join/g2061785795330019536/3SHQtt2O8f).

- Keep changes close to the feature or bug you are working on.
- Match the existing SwiftUI style before introducing new patterns.
- Build after touching Swift, Rust bridge, project, or signing settings.
- Check both empty and populated states for metric UI when possible.
- Keep user-facing health copy plain and careful. Avoid medical claims.
- Put debug tooling, packet details, and raw export behavior under More or Debug surfaces.
- Update the relevant MVP doc when a change completes or changes an open task.
- Mention any build warnings, skipped checks, or device-only assumptions in the PR notes.

## Development Notes

- Prefer small, typed Swift models over displaying raw summary strings.
- Keep Home, Health, Coach, and More routes modular enough to work independently.
- Metric pages should still look polished when data is missing.
- Before installing to a device, run a simulator or device build and check that the Rust library target matches the destination platform.
