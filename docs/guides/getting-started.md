<!-- generated-by: gsd-doc-writer -->
# Getting Started

This guide walks you from zero to a running Goose build with a connected WHOOP device. The iOS app is the core of Goose and works fully offline — the self-hosted server is optional.

---

## Prerequisites

### Required — iOS app

| Tool | Version | Notes |
|---|---|---|
| macOS with Xcode | Xcode with iOS 26 SDK | Required to build the app |
| iOS 26 SDK | 26.0 | Must be installed inside Xcode |
| Apple Developer account | Any (free or paid) | Required for signing; bundle ID is `com.tigercraft4.goose` |
| Rust toolchain | MSRV 1.96 | Install via [rustup.rs](https://rustup.rs) |
| Cargo | Comes with rustup | Used by the Xcode build phase |
| iOS Rust targets | See below | Three targets required |
| iOS device or simulator | iOS 26.0+ | WHOOP BLE pairing requires a physical device |

Install the three required Rust cross-compilation targets:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

### Optional — self-hosted server

| Tool | Notes |
|---|---|
| Docker with Docker Compose | Runs the FastAPI + TimescaleDB stack |

---

## Clone the repository

```bash
git clone https://github.com/b-nnett/goose.git
cd goose
```

---

## Build the iOS app

### Build in Xcode (recommended)

1. Open `GooseSwift.xcodeproj` in Xcode.
2. Select the `GooseSwift` scheme.
3. Choose a simulator or connected iOS 26 device as the run destination.
4. Press **Run** (⌘R).

The Xcode build phase `Scripts/build_ios_rust.sh` runs automatically before the Swift compile step. It cross-compiles `Rust/core` for the active platform and places the static library at `Rust/$(PLATFORM_NAME)/libgoose_core.a`. This takes a few minutes on the first build; subsequent builds are incremental.

### Build from the command line

Simulator (Apple Silicon Mac):

```bash
xcodebuild \
  -project GooseSwift.xcodeproj \
  -scheme GooseSwift \
  -configuration Debug \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath /tmp/goose-swift-deriveddata \
  build
```

Physical device (find your device ID first with `xcrun devicectl list devices`):

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

Install and launch on device after a successful build:

```bash
xcrun devicectl device install app \
  --device <device-id> \
  /tmp/goose-swift-deriveddata-device/Build/Products/Debug-iphoneos/GooseSwift.app

xcrun devicectl device process launch \
  --device <device-id> \
  --terminate-existing \
  com.tigercraft4.goose
```

---

## First run — onboarding and permissions

On first launch, Goose walks you through:

1. **Profile setup** — enter your name, height, weight, and other biometric details.
2. **Permissions** — grant Bluetooth access (required for WHOOP), HealthKit access (optional, for Apple Health sleep/workout import), and notification permission (optional).

Bluetooth permission is mandatory. Without it the app cannot scan for or connect to WHOOP devices.

---

## Connecting to your WHOOP device

Goose supports **WHOOP 5.0** and **WHOOP 4.0**. Other WHOOP generations are not supported.

1. Open the **Home** tab.
2. Tap **Scan** in the device panel.
3. Make sure your WHOOP band is on your wrist and its companion app is closed or backgrounded.
4. Goose discovers nearby WHOOP devices and connects automatically.
5. Once the status reads **ready**, the app starts receiving biometric data over BLE.

The connection is maintained in the background while the app is running. Auto-reconnect is attempted if the band goes out of range and comes back.

---

## Self-hosted server setup (optional)

The server persists decoded biometric streams from your iPhone in a TimescaleDB database. Skip this section if you want to use the app standalone.

### 1. Configure environment variables

```bash
cd server
cp .env.example .env
```

Open `.env` and set the two required values:

```bash
# A secret shared between the server and the iOS app.
# Generate a strong value: openssl rand -hex 32
GOOSE_API_KEY=change_me

# PostgreSQL password for the goose database user.
GOOSE_DB_PASSWORD=change_me
```

The `.env.example` file documents all available variables. The defaults for `GOOSE_DB_NAME`, `GOOSE_DB_USER`, and `GOOSE_INGEST_PORT` (8770) are suitable for a single-user self-hosted deployment.

### 2. Start the Docker stack

```bash
cd server
docker compose up -d --build
```

This starts two containers:
- `goose-db` — TimescaleDB (PostgreSQL 16) with hypertables for biometric stream data.
- `goose-ingest` — FastAPI ingest service, published on host port `8770` by default.

The schema is bootstrapped automatically on first start. Verify the stack is healthy:

```bash
curl -s localhost:8770/healthz
```

Expected response: `{"status":"ok"}`

### 3. Configure the iOS app

In Goose, go to **More > Settings > Remote Server** and fill in:

| Field | Value |
|---|---|
| Server URL | Base URL of your server (e.g. `https://goose.example.com`, `http://goose.local:8770`, or `http://192.168.1.10:8770`). Must include a scheme (`http://` or `https://`). Public hostnames require `https://`; private IP ranges (RFC 1918) and `.local`/`localhost` hostnames allow `http://`. |
| Bearer token | The `GOOSE_API_KEY` value from your `.env` file. |
| Enable Upload | Toggle on. |

The screen shows **Server reachable** when the app can reach `/healthz` on the configured URL. Uploads begin automatically after each BLE data batch is written to local SQLite.

For the full list of server environment variables and iOS configuration options, see [docs/guides/configuration.md](configuration.md).

---

## Common setup issues

**Rust build fails with "target not found"**
Run `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios` and rebuild.

**Xcode cannot find `libgoose_core.a`**
The static library is built by the Xcode build phase on every build. Do not commit or copy pre-built `.a` files from another machine — the platform may not match. Clean the build folder (⇧⌘K) and rebuild.

**App does not scan for Bluetooth devices**
Bluetooth permission must be granted. Go to **Settings > Privacy & Security > Bluetooth** and confirm Goose is listed and enabled.

**Server URL is rejected by the iOS app**
The URL must include a scheme (`http://` or `https://`) and a host. Private IP addresses (RFC 1918: `10.x.x.x`, `172.16-31.x.x`, `192.168.x.x`) are allowed with `http://`. Public hostnames require `https://` to satisfy App Transport Security. `.local` and `localhost` hostnames work with `http://`.

**`docker compose up` fails with "GOOSE_DB_PASSWORD is not set"**
Copy `.env.example` to `.env` in the `server/` directory and set at minimum `GOOSE_API_KEY` and `GOOSE_DB_PASSWORD`.

**Metrics show as empty after connecting**
The app needs time to accumulate data. Packet data flows into local SQLite as BLE notifications arrive. Health metric views update as the Rust core processes incoming frames. Leave the app connected with the screen on for a few minutes.

---

## Next steps

- [docs/guides/configuration.md](configuration.md) — all server environment variables and iOS runtime settings.
- [docs/architecture/overview.md](../architecture/overview.md) — system architecture, data flow, and component responsibilities.
- `server/README.md` — server API reference, database schema, and end-to-end verification steps.
- `README.md` — project overview, contributing guidelines, and data privacy notes.
