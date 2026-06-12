---
plan: 68-02
phase: 68-ble-manager-refactor-data-validator
status: complete
started: 2026-06-12
completed: 2026-06-12
requirements: [BLE5-04]
commits:
  - 54b2848
---

## What Was Built

Created `GooseBLEDataValidator` — a value-type struct that gates structurally invalid BLE frames before they reach the Rust bridge. Injected into the notification pipeline. Added `invalidFrameCount` debug counter visible in More > Debug.

### Key Decisions

- **Type:** `struct GooseBLEDataValidator` (value type per CONTEXT.md) — not a class, no @Observable
- **Invariants (structural only):** (1) deviceID != nil, (2) payload != nil, (3) !payload.isEmpty (length >= 1). No packet-type whitelist — explicitly documented in a file-level comment.
- **OSLog:** category "ble", subsystem "com.goose.swift" — distinct warning message per failing invariant
- **Callback:** `var onInvalidFrame: (() -> Void)?` — wired on GooseBLEClient in init; callback hops to main via `Task { @MainActor in }` before incrementing `invalidFrameCount`
- **Ownership:** `var dataValidator = GooseBLEDataValidator()` on GooseBLEClient (var, not let — struct mutation requires var storage for onInvalidFrame assignment)
- **Hex overload:** `validate(frameHex:deviceID:)` decodes hex to [UInt8] and forwards to the byte-based overload — nil/empty hex, odd-length hex, and non-hex chars all treated as invalid frames with distinct warning messages

### Pipeline Injection

In `GooseAppModel+NotificationPipeline.swift`, `parseNotificationFrames(_:event:context:)`:
- Before `parser.parseBatch`, frames are filtered through `ble.dataValidator.validate(frameHex:deviceID:)`
- Only accepted frames have their hex strings passed to `parseBatch`
- Rejected frames are counted via the `onInvalidFrame` callback → `invalidFrameCount` on GooseBLEClient

### Debug UI

`MoreDebugViews.swift`: Added "Invalid Frames" `MoreInfoRow` adjacent to the "Historical" row:
- Value: `"\(model.ble.invalidFrameCount) rejected this session"`
- systemImage: `"xmark.circle"`
- status: `.ready` (count == 0) / `.blocked` (count > 0)

### Files Modified

- **GooseSwift/GooseBLEDataValidator.swift** (new) — struct with byte-based + hex-based validate overloads, OSLog warnings, onInvalidFrame callback
- **GooseSwift/GooseBLEClient.swift** — added `var dataValidator = GooseBLEDataValidator()` ownership + `var invalidFrameCount = 0` + onInvalidFrame callback wiring in init
- **GooseSwift/GooseAppModel+NotificationPipeline.swift** — validator filter before parseBatch
- **GooseSwift/MoreDebugViews.swift** — "Invalid Frames" MoreInfoRow
- **GooseSwift.xcodeproj/project.pbxproj** — registered GooseBLEDataValidator.swift in GooseSwift target (4 references)

## Self-Check: PASSED

- `struct GooseBLEDataValidator` at GooseBLEDataValidator.swift:9 ✓
- `func validate(payload:deviceID:)` and `func validate(frameHex:deviceID:)` present ✓
- No packet-type whitelist code (2 grep hits are comments documenting the decision) ✓
- `logger.warning` on each invalid frame variant ✓
- `var invalidFrameCount = 0` on GooseBLEClient ✓
- No @Published on invalidFrameCount (GooseBLEClient is @Observable, not ObservableObject) ✓
- `invalidFrameCount += 1` in callback ✓
- pbxproj references: 4 ✓
- Validator called before `parser.parseBatch` in pipeline (line 387 < parseBatch line 389) ✓
- "Invalid Frames" row in MoreDebugViews with `invalidFrameCount` reference ✓
- No packet-type filtering in pipeline change ✓
- BUILD SUCCEEDED ✓

key-files:
  created:
    - GooseSwift/GooseBLEDataValidator.swift
  modified:
    - GooseSwift/GooseBLEClient.swift
    - GooseSwift/GooseAppModel+NotificationPipeline.swift
    - GooseSwift/MoreDebugViews.swift
    - GooseSwift.xcodeproj/project.pbxproj
