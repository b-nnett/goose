# Pitfalls Research

**Domain:** WHOOP iOS BLE app — v10.0 feature additions (haptics, packet parsers, notifications, SQLite, DI)
**Researched:** 2026-06-12
**Confidence:** HIGH — derived entirely from live codebase inspection + seed files; no speculative content

---

## Critical Pitfalls

### Pitfall 1: Calling buzz() or any haptic BLE write from @MainActor inline

**What goes wrong:**
`writeValue(_:for:type:)` blocks the calling thread while CoreBluetooth serialises the ATT packet. If `buzz(loops:)` is called directly from a SwiftUI button action (which runs on the main actor), you block the main thread for the duration of the BLE write, which causes dropped frames and a UI freeze perceptible to the user. If Breathe or Interval Timer calls `buzz` on a timed schedule (e.g., every 4 seconds), the cumulative main-thread pressure is even worse.

**Why it happens:**
The haptic seed shows `activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)` as the implementation. Every existing command write in `GooseBLEClient+Commands.swift` (`writeAlarmCommand`, `writeSensorStreamCommands`, `writeClockCommand`) guards against this with the pattern:
```swift
if !Thread.isMainThread {
  DispatchQueue.main.async { [weak self] in ... }
  return
}
```
But this guard only redirects *off-main* calls back to main, it does not dispatch *from* main to a background queue. The writes themselves execute on main. For one-shot commands this is acceptable. For the Breathe timer calling `buzz` at 4–8 Hz over a 4-minute session, this accumulates 60–120 main-thread write calls, each holding the lock until CoreBluetooth ACKs or times out.

**How to avoid:**
Introduce `canSendHaptic: Bool` computed property on `GooseBLEClient` mirroring the existing `canWriteAlarm` pattern. Gate the haptic write with: connected + commandCharacteristic set + connectionState == "ready" + NOT isHistoricalSyncing. For timed patterns (Breathe, Interval Timer), schedule buzz calls via `DispatchQueue.main.asyncAfter` with a cancellable `DispatchWorkItem`, never from a Swift `Timer` or `Task.sleep` that doesn't dispatch back to main first. Keep writes fire-and-forget (`.withoutResponse`) for haptic commands — no pending-command tracking needed since there is no strap ACK for 0x13.

**Warning signs:**
- Breathe or Interval Timer timer fires on a non-main queue and calls `buzz` directly.
- `writeValue` is called from inside `Task { ... }` without an explicit `DispatchQueue.main.async` wrapper.
- UI jank appears during a Breathe session in the simulator.

**Phase to address:**
HAP-01 (buzz primitive) — get the threading model right before Breathe/Interval Timer build on top of it.

---

### Pitfall 2: Haptic command not gated during historical sync — corrupts sync state machine

**What goes wrong:**
`GooseBLEClient` gates every existing write type (`canWriteAlarm`, `canSyncClock`, `canSyncHistorical`) with `!isHistoricalSyncing`. If `buzz(loops:)` is implemented without this guard, a user tapping "Breathe" mid-sync will inject a cmd 0x13 frame into the BLE pipe while the strap is mid-history-stream. The strap processes it as an interleaved command response, which may cause the historical sync state machine (`GooseBLEClient+HistoricalHandlers.swift`) to misinterpret the next command-response packet — specifically where `puffinCommandResponse` (type 38) is checked as a sequenced ack.

**Why it happens:**
The haptic seed says "fire-and-forget" and "no pending-command tracking needed." This is correct for the ACK side, but the seed does not mention the `isHistoricalSyncing` gate. Because buzz is simpler than alarm/clock writes, implementers may skip the guard thinking it doesn't matter.

**How to avoid:**
Add `canSendHaptic: Bool` that includes `&& !isHistoricalSyncing`, exactly as `canWriteAlarm` does. Silently drop the buzz call (log it) when syncing is active. Surface "Haptic unavailable during sync" in the Breathe UI.

**Warning signs:**
- `historicalSyncStatus` flips to a parse-error state immediately after a Breathe session started mid-sync.
- `lastHighFrequencyHistorySyncResponse` shows an unexpected sequence number.
- `isHistoricalSyncing` is not included in `canSendHaptic`.

**Phase to address:**
HAP-01 — add the guard before HAP-02 (Breathe) depends on it.

---

### Pitfall 3: R22 HR from 0x10 and R17 HR from 0x9a/0x9b deduplicated wrong — WHOOP 5.0 counts twice

**What goes wrong:**
The seed for BLE5-01 notes that the BTSnoop capture shows WHOOP 5.0 streams R17 on `0x0027` (packet type `0x9a`/`0x9b`) AND R22 on `0x0022` (packet type `0x10`) simultaneously. If both are added as trusted sources in `trusted_frames_for_summary_kinds` without a mutual-exclusion or source-priority rule, the HR pipeline will receive two samples per second during overlap periods — one from R17, one from R22. This produces doubled RR interval counts and inflates RMSSD, which corrupts HRV for WHOOP 5.0 users.

**Why it happens:**
The Rust parser currently only handles R17, so this was never a problem. Once R22 is added, both packet types pass validation and route into the same HR/RR pipeline. The data model has no concept of "same physical sample from two channels."

**How to avoid:**
When both R17 and R22 are present, treat R22 as authoritative (it is the WHOOP 5.0 primary channel) and demote R17 to fallback. Implement this in the Rust trusted-source priority logic: if an R22 sample arrives within 1.5s of an R17 sample with the same BPM ±2, suppress the R17. Add a `r22_whoop5_hr` vs `r17_optical_or_labrador_filtered` source label to every HR sample in the database so the dedup can be audited. Add a test fixture with interleaved R17 + R22 frames.

**Warning signs:**
- HR sample rate doubles from ~1/s to ~2/s for WHOOP 5.0 users after adding R22 support.
- RMSSD on WHOOP 5.0 reads 40–60% higher than expected.
- The diagnostic counter shows both R17 and R22 sample counts increasing together.

**Phase to address:**
BLE5-01 (R22 parser) — the dedup rule must be in the same phase as the parser, not deferred.

---

### Pitfall 4: v18 historical timestamp conversion runs through the same stale-clock path as v7/v9/v12 — silent time-series corruption

**What goes wrong:**
The v18 decode seed identifies that `historical_sync.rs` converts device-epoch timestamps to wall-clock using an offset. When the strap RTC is stale (lost power, battery replacement), the offset is garbage and produces timestamps years in the past or future. Rows with corrupted timestamps insert successfully (SQLite has no timestamp constraints), and then corrupt the sleep staging, strain, and HRV pipelines that window by wall-clock time.

The additional risk: EVENT (type-48) packets have timestamps that are already native RTC unix seconds. If the v18 decode feeds EVENT-style timestamps through the same epoch→wall-clock offset path, they get double-offset and produce timestamps ±decades off.

**Why it happens:**
The stale-clock dedup (300s grid snap) and the EVENT bypass are both mentioned in the seed as fixes needed in `historical_sync.rs`. When implementing the v18 arm in `protocol.rs`, implementers may stop at the field offsets and forget to wire the timestamp through the corrected converter, especially if the corrected converter doesn't exist yet because the stale-clock fix hasn't been written.

**How to avoid:**
Implement the stale-clock fix (86400s threshold → 300s grid snap) and the EVENT type-48 bypass as the first sub-task of BLE5-02, before touching any field parsing. Then implement v18 field parsing as the second sub-task, using the already-fixed converter. Add a Rust test: construct a v18 frame with a stale RTC offset >86400s, assert that the output timestamp is snapped to a 300s grid.

**Warning signs:**
- After a historical sync on a WHOOP 5.0 that recently lost battery, HR or sleep rows appear with timestamps before 2020 or after 2030.
- Sleep staging returns no overnight sessions for a WHOOP 5.0 user even though the sync completed successfully.
- `rr_interval_samples` count increases but HRV score remains stale.

**Phase to address:**
BLE5-02 — stale-clock fix must be the entry condition for v18 parsing, not a follow-up.

---

### Pitfall 5: SQLite schema migration not bumped — `open_existing_current` hard-fails on user devices

**What goes wrong:**
`GooseStore::open_existing_current` returns `Err` if `PRAGMA user_version != CURRENT_SCHEMA_VERSION`. This means if v10.0 adds new tables (DATA-01: journal, workout, appleDaily, metricSeries) without bumping `CURRENT_SCHEMA_VERSION` from 19 to 20 (or higher), the next `GooseStore::open_or_create` will run `migrate()` but `open_existing_current` calls from the health pipeline will reject the database. Conversely, if the version is bumped but the `migrate()` block doesn't include all 4 new `CREATE TABLE` statements, existing databases that already ran an earlier migration will silently skip the new tables (because `CREATE TABLE IF NOT EXISTS` was already in the committed batch).

The current pattern: all DDL is in a single `migrate()` block executed as one `execute_batch`. If any new table's DDL is syntactically invalid (missing comma, wrong type), the entire batch rolls back and the database is left at v19 with no schema changes.

**Why it happens:**
The monolithic migration style (all versions in one batch with `INSERT OR IGNORE INTO goose_schema_migrations(version)` to mark each version applied) works for fresh installs but has a subtle flaw for upgrades: there are no per-version migration blocks. Every schema change must use `IF NOT EXISTS` or `ADD COLUMN IF NOT EXISTS` to be idempotent. Developers adding a new table sometimes forget that existing prod databases already ran the v1–v19 batch and won't get the new table unless it is added to a new migration branch.

**How to avoid:**
Add a conditional migration arm: after the monolithic `IF NOT EXISTS` batch, check `schema_version()` and if it equals 19, execute a dedicated v20 DDL block, then `PRAGMA user_version = 20`. Bump `CURRENT_SCHEMA_VERSION` to 20. Write a Rust test that: (1) opens an in-memory store, (2) manually sets `PRAGMA user_version = 19` and does NOT create the new tables, (3) re-runs `migrate()`, (4) asserts the new tables exist and `user_version = 20`.

**Warning signs:**
- App launches on a device with an existing database and immediately crashes/hangs (bridge returns schema mismatch error).
- New tables (`journal`, `workout`, `appleDaily`, `metricSeries`) are missing from the database after migration even though the app started successfully.
- Tests pass on in-memory databases (always fresh) but fail on the real device database.

**Phase to address:**
DATA-01 (4 new SQLite tables) — migration correctness is the entry condition for all new table features.

---

### Pitfall 6: Local notification fired from the BLE notification queue — permission state unverified

**What goes wrong:**
`UNUserNotificationCenter.add(_:withCompletionHandler:)` must be called from any thread, but it silently no-ops if the app does not have notification permission. If the sleep summary notification fires from a BLE completion callback (off-main) and permission was never granted (user skipped onboarding), the notification is dropped with no error surfaced to the UI. Worse: the existing onboarding flow in `OnboardingView.swift` already requests `[.alert, .badge, .sound]` and stores `notificationPermissionHandled` in `@AppStorage`. If v10.0 adds a second call to `requestAuthorization` from a different code path (e.g., a "Enable Notifications" button in the More tab), iOS will not re-prompt the user — it returns the current status silently — but the second call site may interpret the silent return as "denied" and incorrectly disable the feature.

**Why it happens:**
iOS grants the notification permission dialog exactly once. Any subsequent `requestAuthorization` call returns the current status without showing a dialog. Developers building the FEAT-03 notification feature may not know about the onboarding flow that already made the request, and may add a second request thinking it is the first.

**How to avoid:**
Centralise all notification work behind a single `GooseNotificationScheduler` type that (1) checks `getNotificationSettings` before attempting to schedule, (2) logs the permission status, and (3) never calls `requestAuthorization` — that remains exclusively in `OnboardingView.swift`. The scheduler fires from `DispatchQueue.main.async` even if called from a background queue, so the completion handler is predictable. Add a `notificationsEnabled: Bool` published property to `GooseAppModel` that refreshes from `getNotificationSettings` on foreground.

**Warning signs:**
- A second `requestAuthorization` call appearing outside `OnboardingView.swift`.
- Notification scheduling attempted without a prior `getNotificationSettings` check.
- Sleep summary notification never appears even when BLE sync completes successfully.

**Phase to address:**
FEAT-03 (iOS local notifications) — architecture review before any notification scheduling code is written.

---

### Pitfall 7: GooseBLEHistoricalManager extracts state that GooseBLEClient still mutates — dual ownership race

**What goes wrong:**
`GooseBLEClient` currently owns all historical sync state: `isHistoricalSyncing`, `historicalSyncStatus`, `historicalSyncRunID`, `pendingAutomaticHistoricalSyncReason`, and all the handler callbacks in `GooseBLEClient+HistoricalHandlers.swift`. When BLE5-03 extracts this into `GooseBLEHistoricalManager`, if the refactor is done incrementally (manager takes some state, client keeps some state), there will be a window where both types read and write overlapping fields. Because `GooseBLEClient` is `@Observable` and `@unchecked Sendable`, concurrent mutations from different queues (coreBluetoothQueue vs. the manager's queue) will cause a data race that only manifests as sporadic `isHistoricalSyncing` staying `true` after a sync completes.

**Why it happens:**
The `GooseBLEClient` extension pattern splits behaviour across files but shares state on the parent class. When extracting a manager, the natural first step is to copy the handler functions into the new type and leave the state on the client. This "partial extraction" is the dangerous middle state.

**How to avoid:**
Do the extraction in a single atomic commit: (1) move all historical state fields from `GooseBLEClient` to `GooseBLEHistoricalManager`, (2) make `GooseBLEClient` hold a `GooseBLEHistoricalManager` instance, (3) proxy `isHistoricalSyncing` and `historicalSyncStatus` as forwarded computed properties on the client for backwards compatibility with the 15+ call sites that reference `ble.isHistoricalSyncing`. Do not leave the extraction half-done across a phase boundary.

**Warning signs:**
- `ble.isHistoricalSyncing` reads `true` in the UI after a successful sync that produced packets.
- `canWriteAlarm` / `canSyncClock` / `canSendHaptic` remain blocked after historical sync finishes.
- Two files both write to a field named `isHistoricalSyncing` on different types.

**Phase to address:**
BLE5-03 — must be a single atomic refactor, not spread across multiple phases.

---

### Pitfall 8: DI protocol extraction without test targets — protocols become dead abstraction

**What goes wrong:**
The service-layer-di seed is explicit: "Do not extract protocols as a pure refactor with no tests to back them." If `GooseBLEManaging`, `GooseRustBridging`, and `GooseAppServicing` protocols are extracted but no Swift test target exists, the protocols are never exercised by mocks and drift out of sync with the concrete types. The most common form: a new method is added to `GooseBLEClient` for HAP-01, `GooseBLEManaging` is never updated, and the build still passes because `GooseBLEClient` conforms to the protocol via an existing method with the same signature by coincidence.

**Why it happens:**
Protocol extraction is low-risk in isolation. The pain only appears when a mock is used in a test and the test fails to compile because the protocol is stale. Without a test target, this feedback loop never closes.

**How to avoid:**
ARCH-01 must add a Swift test target (`GooseSwiftTests`) as its first deliverable. The test target must contain at least one test that instantiates `GooseBLEClientMock` and calls a method on it — this is the compile-time canary that ensures `GooseBLEManaging` stays in sync. Use `#if DEBUG` for mock types in the main target, test target for actual test functions.

**Warning signs:**
- ARCH-01 is marked complete but no test target exists in `GooseSwift.xcodeproj`.
- `GooseBLEManaging` has fewer methods than `GooseBLEClient` has public BLE write methods.
- `GooseBLEClientMock` compiles but is never instantiated in any test function.

**Phase to address:**
ARCH-01 — test target creation is the entry condition, not the exit condition.

---

### Pitfall 9: DI circular dependency — GooseAppServicing wraps GooseBLEClient which closes over GooseAppModel

**What goes wrong:**
The seed proposes `GooseAppServicing` wraps `GooseBLEClient` and `HealthDataStore`. But `GooseBLEClient` already has a callback `onConnectionStateChange` that `GooseAppModel` sets (making the client aware of its owner). If `GooseAppServicing` is injected into `GooseAppModel` and `GooseAppServicing` in turn holds a reference to `GooseBLEClient` which holds `onConnectionStateChange: ((String) -> Void)?` (a closure capturing `GooseAppModel`), there is a reference cycle: `GooseAppModel → GooseAppServicing → GooseBLEClient → closure → GooseAppModel`. This cycle prevents deallocation and leaks all three objects.

**Why it happens:**
The callback pattern (`onConnectionStateChange`) was explicitly chosen over Combine to match the rest of the codebase. It works without a service layer. When a service layer is added as a wrapper, the closure capture creates an implicit retain cycle that `[weak self]` on the closure alone does not break, because the closure is stored on the client (not the model), and the model holds the service, which holds the client.

**How to avoid:**
Make all callbacks on `GooseBLEClient` use `[weak self]` captures that reference `GooseAppModel` weakly. Verify with Xcode Memory Graph Debugger after ARCH-01 that `GooseAppModel` deallocates when the view is dismissed in a preview. The service protocol should be a value-type composition (protocol only; the actual instances remain owned by `GooseAppModel`) rather than a reference-type container.

**Warning signs:**
- Xcode Memory Graph shows `GooseBLEClient` retained after disconnection.
- `GooseAppModel` `deinit` never fires during preview/test teardown.
- `onConnectionStateChange` closure is set without `[weak self]` on the `GooseAppModel` capture.

**Phase to address:**
ARCH-01 — memory graph check is a required step before the phase is complete.

---

### Pitfall 10: Swift-side BLE validator rejects valid Gen5 frames due to packet type whitelist

**What goes wrong:**
BLE5-04 adds a Swift-side validator before Rust/SQLite ingestion. If the validator uses a hard-coded whitelist of known packet types (e.g., `[0x9a, 0x9b, 0xaa, 0x26, 0x38]`) and R22 (`0x10`) is not added to that whitelist when BLE5-01 lands, all R22 frames from WHOOP 5.0 will be dropped at the Swift gate before reaching the Rust parser that was built to handle them. This produces the same symptom as if BLE5-01 was never implemented: blank metrics for WHOOP 5.0 users.

**Why it happens:**
BLE5-04 (validator) and BLE5-01 (R22 parser) may be implemented in different phases or treated as independent work items. The validator's whitelist is not automatically updated when the Rust parser gains support for a new packet type. `OvernightRawNotificationStorageClassifier` in `NotificationFrameParsing.swift` already has a `Set<UInt8>` of packet types — a new validator modelled on that will inherit the same coupling.

**How to avoid:**
Do not use a whitelist in the Swift validator. Instead, validate structural invariants only: minimum frame length, magic byte (`0xaa` header presence for Gen4/Gen5 packets, or first byte `0x10` for R22), plausible payload length range (1–512 bytes). Let the Rust parser be the authority on packet types — it already returns `warnings` for unknown types. The Swift validator's job is to prevent obviously malformed bytes (empty data, length overflow) from reaching Rust, not to gatekeep packet types.

**Warning signs:**
- WHOOP 5.0 R22 frames are logged as "dropped by validator" even after BLE5-01 is shipped.
- The validator contains a `Set<UInt8>` of expected packet type bytes.
- BLE5-04 is shipped before BLE5-01 with no plan to coordinate their allowed-type lists.

**Phase to address:**
BLE5-04 — define the validator contract (structural only, no type whitelist) before implementation begins.

---

### Pitfall 11: Realtime strain accumulator mutated from BLE notification queue and read from @MainActor — unsynchronised shared state

**What goes wrong:**
DATA-02 (realtime strain accumulation) requires updating a running strain total from every HR sample that arrives on the BLE notification queue (which runs on `notificationIngestQueue`, a private `DispatchQueue`). If the accumulator is a plain `var` on `GooseAppModel` (which is `@Observable` and therefore implicitly `@MainActor` for mutation), mutating it from the notification queue is a data race. If it is moved to a private property with an `NSLock`, every BLE sample incurs a lock acquisition — which is acceptable at 1 Hz but becomes a concern during historical sync where HR samples arrive in bursts.

**Why it happens:**
`GooseAppModel`'s `@Observable` properties are implicitly main-actor isolated. Existing pipelines (e.g., `WhoopDataSignalPipeline`) accept data on their own queue and dispatch results to main via `Task { @MainActor in ... }`. A new strain accumulator that skips this pattern will compile without warnings (Swift does not statically detect `@unchecked Sendable` races) but will race at runtime.

**How to avoid:**
Model the realtime strain accumulator as a separate `actor` type (`GooseStrainAccumulator`) that accepts HR samples via `async func ingest(hr: Int, timestamp: Date)` and publishes the running total via an `AsyncStream<Double>` consumed on `@MainActor`. This keeps accumulation logic off the main thread, serialised by the actor, and the published value arrives on main automatically.

**Warning signs:**
- Strain accumulator property is declared on `GooseAppModel` with no actor isolation boundary.
- BLE notification handler calls `appModel.currentStrain += delta` directly without dispatching to main.
- Thread Sanitiser (TSan) reports a data race on the strain field during a workout session.

**Phase to address:**
DATA-02 — actor design must be decided before any accumulation logic is written.

---

### Pitfall 12: HR decimation applied to the persistence layer instead of the chart view model — destroys raw data fidelity

**What goes wrong:**
The hr-decimation seed is explicit: "The remaining problem is chart render performance at high zoom-out." The fix belongs in `GooseHRDecimator` applied before chart views, not in `HeartRateSeriesStores.swift`'s underlying storage or in the Rust bridge query. If decimation is applied at the store insert layer, raw 1-second samples are permanently discarded. Sleep staging (Cole-Kripke) and HRV (RMSSD) both require the raw 1s samples — destroying them produces wrong metrics.

**Why it happens:**
The seed mentions `maxSamples = 100_000` and `prune()` already on `HeartRateSeriesStores`. A developer seeing this may extend `prune()` to also decimate, thinking it is consistent with existing memory management. The distinction between "prune old samples" (time-based) and "decimate fine samples" (resolution-based) is easy to blur.

**How to avoid:**
`GooseHRDecimator` operates only on the `[HRSample]` array passed to SwiftUI Charts. It returns a decimated array for rendering and never writes to `HeartRateSeriesStore`. `HeartRateSeriesStore` continues to hold the full `maxSamples` window of raw 1s data. The SQLite persistence layer (Rust) is never touched by DATA-04.

**Warning signs:**
- `HeartRateSeriesStore.samples.count` decreases after decimation runs (means data was destroyed).
- RMSSD changes after DATA-04 is shipped (means raw RR data was affected).
- `GooseHRDecimator` modifies `HeartRateSeriesStore` directly instead of returning a new array.

**Phase to address:**
DATA-04 — scope check: "does this touch the store?" is the acceptance criterion.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| `buzz` with no pending-command tracking | Simple implementation | Cannot detect strap ignore or firmware rejection | Acceptable — 0x13 is fire-and-forget per seed; add only if strap ACK is confirmed |
| Monolithic `migrate()` with single version bump | Fast to write | Any DDL syntax error rolls back entire migration | Must add per-version conditional arms starting with v20 |
| `GooseBLEHistoricalManager` proxy properties on `GooseBLEClient` | Zero call-site changes | Extra indirection layer; can confuse future readers | Acceptable as a transition shim; document with a comment |
| DI protocols without `associatedtype` constraints | Simpler protocol | Mock cannot enforce call ordering | Acceptable for v10.0 mocks; add XCTest spy pattern |
| `GooseHRDecimator` using averaging instead of LTTB | 30 lines vs. 150 | Chart visual quality degrades at high zoom-out on overnight views | Only acceptable if LTTB adds >2h implementation time |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| R22 + Rust bridge | Adding `r22_whoop5_hr` to trusted sources without source priority | Add R22 priority rule that suppresses R17 when both arrive within 1.5s |
| v18 → existing tables | Feeding v18 RR intervals directly to `rr_interval_samples` without device-type tag | Tag all v18-derived rows with `device_generation = "5"` for audit |
| `UNUserNotificationCenter` | Calling `requestAuthorization` in `GooseNotificationScheduler` | Permission request stays in `OnboardingView.swift` only; scheduler uses `getNotificationSettings` |
| `GooseBLEHistoricalManager` | Importing `CoreBluetooth` directly into the manager | Manager receives `Data` payloads, not `CBCharacteristic`; keeps CoreBluetooth coupling on `GooseBLEClient` |
| SQLite v20 migration | Using only `CREATE TABLE IF NOT EXISTS` without a version-conditional arm | Add `if schema_version == 19 { execute v20 DDL; set user_version = 20 }` arm in `migrate()` |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Breathe timer firing `buzz` via `Task.sleep` without main-thread dispatch | Main thread stalls every 4s during Breathe session | Use `DispatchQueue.main.asyncAfter` with cancellable `DispatchWorkItem` | First Breathe session longer than 30s |
| Realtime strain recalculated via full Rust bridge call on every HR sample | 1–2ms FFI overhead per sample; during historical sync HR arrives in bursts | Swift-side accumulator only; Rust bridge called for persistence, not accumulation | Historical sync with >3600 HR samples |
| 28,800-point HR array passed to SwiftUI Charts without decimation | Dropped frames on older devices when zoomed out to overnight view | `GooseHRDecimator` applied in chart view model layer only | Users with overnight BLE capture >8h |
| `GooseAppServicing` protocol surface grows to mirror `GooseBLEClient` | Mocks require implementing 30+ methods; maintenance cost explodes | Narrow protocol to only what tests need (ISP) | When first test suite requires >5 BLE mock methods |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Biometric data in notification body (e.g., "Your HRV is 42 ms") | Lock screen notification exposes health data to bystanders | Generic body ("Your WHOOP summary is ready"); detail only inside the app |
| `GooseRustBridgeMock` included in production target | Mock data returned to real users; no actual Rust processing | `#if DEBUG` guard on mock files; mock types only in `GooseSwiftTests` target |
| Haptic command payload bytes hard-coded without logging | Future firmware update may change byte meaning; unauditable | Log every haptic command frame hex in diagnostics |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Breathe buzz disabled mid-session because historical sync started automatically | User's paced breathing interrupted without explanation | Prevent auto-sync during active Breathe session, or surface "Sync started — haptics paused" |
| iOS notification permission denied in onboarding, no re-prompt possible | User misses sleep insights with no recovery path | Surface "Enable notifications" Settings deep link in More tab when `authorizationStatus == .denied` |
| Realtime strain counter resets on app background because accumulator is in-memory only | Strain reading jumps back to 0 mid-workout | Persist accumulator checkpoint to `UserDefaults` every 60s; restore on foreground |
| New SQLite tables visible in export but undocumented | User confused by unknown data in privacy export | Add "What's in your export" summary to `MorePrivacyView` listing all tables |

## "Looks Done But Isn't" Checklist

- [ ] **HAP-01 buzz:** Verify `canSendHaptic` includes `!isHistoricalSyncing` guard — trigger a buzz during an active historical sync and confirm the buzz is silently dropped with a log entry, not attempted.
- [ ] **BLE5-01 R22:** Verify WHOOP 5.0 R17 and R22 dedup is active — connect WHOOP 5.0, confirm `r22_whoop5_hr` source appears in the diagnostic counter and `r17_optical_or_labrador_filtered` count does NOT increase simultaneously.
- [ ] **BLE5-02 v18:** Verify stale-clock guard is active before v18 field parsing — use a synthetic frame with unix timestamp offset >86400s and assert output timestamp is snapped to 300s grid.
- [ ] **DATA-01 migration:** Verify v20 migration runs on an existing v19 database — open a Rust test store with `user_version = 19` set manually, run `migrate()`, assert all 4 new tables exist and `user_version = 20`.
- [ ] **FEAT-03 notifications:** Verify notification permission is NOT requested a second time — after onboarding, trigger "Enable Notifications" in More tab and confirm the iOS permission dialog does NOT reappear; only Settings is opened.
- [ ] **ARCH-01 DI:** Verify memory cycle is absent — run the app, trigger a BLE connect/disconnect cycle, open Xcode Memory Graph Debugger and confirm zero `GooseBLEClient` instances remain after disconnect.
- [ ] **DATA-04 decimation:** Verify raw store is unaffected — after `GooseHRDecimator` runs, confirm `HeartRateSeriesStore.samples.count` equals the pre-decimation count.
- [ ] **BLE5-04 validator:** Verify validator passes R22 frames — send a 4-byte R22 frame (`0x10 0x50 0x31 0x05`) through the validator and confirm it reaches the Rust parser without being rejected.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| buzz called from @MainActor causing UI freeze | LOW | Wrap buzz trigger in `DispatchQueue.main.async { [weak self] in ... }` — one-line fix |
| Historical sync corruption from un-gated haptic | MEDIUM | Reset `isHistoricalSyncing = false` from recovery path; add `canSendHaptic` gate; user re-triggers sync |
| R22 + R17 double-counting in RR pipeline | HIGH | Rust fix + SQLite cleanup of duplicate rows; affected users need manual HRV recalculation |
| SQLite migration failure on device | HIGH | Ship `GooseStore.repair()` bridge method that drops new v20 tables and re-runs migration; expose in More tab debug tools |
| Notification permission second-request (silent, no dialog) | LOW | Remove duplicate `requestAuthorization` calls; add Settings deep link for denied state |
| DI memory cycle | MEDIUM | Add `[weak self]` to all callbacks; run Memory Graph; refactor ownership if cycle persists |
| v18 stale-clock corruption | HIGH | Write a Rust tool to identify rows with timestamps outside 2020–2030 and DELETE them; re-run historical sync |
| Decimation applied to store (raw data lost) | CRITICAL | No recovery — raw samples gone; requires full re-sync from strap. Prevent only. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| buzz threading / @MainActor | HAP-01 | Breathe timer fires for 60s; no UI jank; Instruments shows no main-thread stall >16ms |
| haptic not gated during sync | HAP-01 | Buzz triggered mid-sync; `canSendHaptic` returns false; sync completes normally |
| R22 + R17 dedup | BLE5-01 | WHOOP 5.0 connected; HR sample rate stays at ~1/s not ~2/s; RMSSD unchanged from baseline |
| v18 stale-clock | BLE5-02 | Synthetic stale-offset frame test passes; real device sync timestamps fall in 2024–2026 range |
| SQLite v20 migration | DATA-01 | Migration test on v19 seed database passes; `user_version == 20` after open |
| Notification permission double-request | FEAT-03 | No second permission dialog on any code path; denied state shows Settings deep link |
| Historical manager dual ownership | BLE5-03 | Single atomic commit; no `isHistoricalSyncing` field on both types simultaneously |
| DI protocols without tests | ARCH-01 | Test target created before protocol extraction; at least one test compiles and runs |
| DI circular retain | ARCH-01 | Memory Graph after disconnect shows zero `GooseBLEClient` instances retained |
| Validator type whitelist | BLE5-04 | Validator code review confirms no `Set<UInt8>` packet-type gate; R22 passes through |
| Strain accumulator data race | DATA-02 | TSan clean during 10-minute BLE capture session in simulator |
| HR decimation destroys store | DATA-04 | `HeartRateSeriesStore.samples.count` identical before and after decimation |

## Sources

- `GooseSwift/GooseBLEClient+Commands.swift` — existing alarm/clock/sensor command guards (`isHistoricalSyncing`, `canWriteAlarm`, threading pattern)
- `GooseSwift/GooseBLEClient.swift` — `canSendHello`, `canWriteAlarm`, `isHistoricalSyncing`, `@Observable` declaration, `onConnectionStateChange` callback
- `GooseSwift/GooseBLEClient+HistoricalHandlers.swift` — puffin command response handling, sync state machine
- `GooseSwift/NotificationFrameParsing.swift` — R17 packet type routing, `OvernightRawNotificationStorageClassifier` packet type Set
- `Rust/core/src/protocol.rs` — `7 | 9 | 12 | 18` arm discarding v18, R17 parse path, absence of type-0x10 handling
- `Rust/core/src/store.rs` — `CURRENT_SCHEMA_VERSION = 19`, `open_existing_current` version check, monolithic `migrate()` pattern
- `GooseSwift/OnboardingView.swift` — single `requestAuthorization` call site with `notificationPermissionHandled` guard
- `.planning/seeds/haptic-buzz-primitive.md` — cmd 0x13 payload, fire-and-forget design, dependents list
- `.planning/seeds/whoop5-r22-packet-support.md` — R17/R22 dual-stream finding from BTSnoop
- `.planning/seeds/whoop5-v18-historical-decode.md` — stale-clock dedup, EVENT type-48 bypass, multi-file timestamp converter warning
- `.planning/seeds/hr-decimation.md` — chart-layer-only scope, existing `maxSamples` + `prune()` note
- `.planning/seeds/service-layer-di.md` — "do not extract without tests" constraint, callback reference cycle risk

---
*Pitfalls research for: WHOOP iOS BLE app — v10.0 protocol parity, haptics, notifications, SQLite, DI*
*Researched: 2026-06-12*
