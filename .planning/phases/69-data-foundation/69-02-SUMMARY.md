---
plan: 69-02
phase: 69
status: complete
completed: 2026-06-12
---

# Plan 69-02 Summary: GooseStrainAccumulator Wiring (DATA-02)

## What Was Built

Added realtime Edwards Zone Load strain accumulation to GooseAppModel, wired through the existing
BLE heart rate callback pipeline.

**Files modified:**
- `GooseSwift/GooseStrainAccumulator.swift` — new file (created in prior commit): `actor GooseStrainAccumulator` with `ingest(bpm:date:)`, `pollIfReady(now:)`, `reset()`, `freeze()`, `setMaxHR(_:)` methods and `nonisolated static let` constants (`defaultMaxHR=190`, `publishInterval=3`, `maxSampleGap=30`)
- `GooseSwift/GooseAppModel.swift` — added `var liveWorkoutStrain: Double = 0` stored property; added `let strainAccumulator = GooseStrainAccumulator()` stored property; wired accumulator tap into `ble.onLiveHeartRate` closure with guard on `activeActivityPersistence != nil`, `ingest` + `pollIfReady` + `Task { @MainActor }` publication
- `GooseSwift/GooseAppModel+ActivityRecording.swift` — added `Task { await self.strainAccumulator.reset() }` in `beginActivityRecording` after `activeActivityPersistence` is set; added `Task { await self.strainAccumulator.freeze() }` and `liveWorkoutStrain = 0` in `finishActivityRecording` after `activeActivityPersistence = nil`

## Key Design Decisions

- `strainAccumulator` is declared with `let` (no `private`) so extension files can call it — Swift `private` is file-scoped and would block access from `GooseAppModel+ActivityRecording.swift`
- `liveWorkoutStrain` is declared `var` (not `private(set)`) for the same cross-file write reason
- Guard `activeActivityPersistence != nil` keeps accumulation strictly session-scoped — no strain at rest
- `freeze()` on workout end makes `ingest` a no-op for the remainder of the session completion path; zero clears the tile immediately
- `maxSampleGap = 30s` prevents stale BLE-reconnect samples inflating load across gaps

## Verification

- `GooseSwift/GooseStrainAccumulator.swift` exists and declares `actor GooseStrainAccumulator`
- `GooseAppModel.swift` contains `liveWorkoutStrain` and `strainAccumulator` stored properties
- `GooseAppModel+ActivityRecording.swift` contains `strainAccumulator.reset()` in begin and `strainAccumulator.freeze()` + `liveWorkoutStrain = 0` in finish
- `onLiveHeartRate` closure contains `strainAccumulator.ingest` + `pollIfReady` + `Task @MainActor` publication chain
- xcodebuild BUILD SUCCEEDED — zero compiler errors or warnings introduced
