# ADR: Android JNI Bridge Architecture

**Status:** Accepted  
**Date:** 2026-06-03  
**Authors:** Goose Core Team

---

## Context

The Goose Rust core (`goose-core`) already ships as a `cdylib` (dynamic library) for iOS via a C FFI pair: `goose_bridge_handle_json` and `goose_bridge_free_string`. Android uses the Java Native Interface (JNI) instead of C FFI for calling native code from Kotlin/Java, requiring a different calling convention and function naming scheme.

The goal of this ADR is to document the decisions made to enable Android cross-compilation and expose the existing bridge as a JNI-callable entry point, while keeping the Android-specific code isolated from the iOS build.

---

## Decision

### 1. Why cdylib + JNI shim (not a separate crate)

The `goose-core` crate already declares `crate-type = ["rlib", "staticlib", "cdylib"]` in `Cargo.toml`. Adding Android JNI support does not require a separate crate — a `#[cfg(target_os = "android")]` module appended to `bridge.rs` is sufficient. This module imports the `jni` crate (version 0.21) and wraps the existing `handle_bridge_request_json` function with JNI calling conventions.

This approach avoids workspace complexity, keeps the bridge contract in one file, and lets the same JSON-over-FFI protocol work identically on iOS and Android.

### 2. Panic strategy

The `[profile.release]` section of `Cargo.toml` sets `panic = "unwind"` (required so that the FFI boundary's `catch_unwind` in `bridge.rs` can convert Rust panics into structured JSON errors instead of aborting the process). On Android, an **unhandled** panic that unwinds through a JNI boundary causes undefined behavior — but the JNI shim wraps every call in `catch_unwind`, so panics are caught before they reach the JNI boundary. All error paths in the shim return error JSON or `null_mut()` rather than panicking, so no unwind crosses into Java. A `.cargo/config.toml` override to `panic = "abort"` is not needed and would break the `catch_unwind` safety net in the iOS build.

### 3. MUTF-8 string handling policy

Android's JNI layer uses Modified UTF-8 (MUTF-8), not standard UTF-8. The `jni` crate's `JNIEnv::get_string` method handles MUTF-8 decoding automatically. Any conversion failure (malformed input) is caught at the `match` boundary and returns a structured JSON error string `{"ok":false,"error":"jni_string_conversion_error"}` to the caller. The Goose bridge protocol is JSON-over-string, so as long as `get_string` succeeds, the payload is standard UTF-8 for the rest of the processing pipeline.

### 4. rusqlite bundled target limitation

The `rusqlite` crate is used with the `bundled` feature, which compiles SQLite from source using `cc`. This works reliably on `aarch64-linux-android` (ARM64). Known upstream bugs in `rusqlite bundled` affect `x86_64-linux-android` and `armv7-linux-androideabi` on certain NDK versions. For this reason, only `aarch64-linux-android` is a supported Android target. x86_64 and armv7 Android targets are explicitly excluded until the upstream `rusqlite` issue is resolved.

### 5. JNI naming convention

The JNI function naming follows the standard convention derived from the Java package and class:

- Java package: `com.goose.core`
- Java class: `GooseBridge` (mirrors the iOS `GooseRustBridge` naming pattern)
- JNI symbol: `Java_com_goose_core_GooseBridge_handle`

The function is declared with `extern "system"` ABI (the correct ABI for JNI on Android, not `extern "C"`), and uses `#[unsafe(no_mangle)]` as required by Rust 2024 edition.

---

## Consequences

### What this enables

- `cargo ndk -t arm64-v8a build --release --lib` produces a `libgoose_core.so` containing the `Java_com_goose_core_GooseBridge_handle` symbol, ready to be bundled into an Android APK/AAR.
- A future Android app written in Kotlin can call `GooseBridge.handle(requestJson)` using exactly the same JSON protocol as the iOS Swift app, with no changes to the Rust core logic.
- The CI `android-build` job in `rust-core-ci.yml` validates every pull request targeting `Rust/core/**` against the Android cross-compilation target.

### What keeps the door open for a future Android app

The `cdylib` output type, the JSON-over-string bridge protocol, and the `com.goose.core.GooseBridge` naming convention are all stable contracts that a future Android app layer can build on. The Kotlin/Java wrapper is a thin `System.loadLibrary("goose_core")` call followed by a `native fun handle(request: String): String` declaration — no further Rust changes are needed.

### Limitations

- Only `aarch64-linux-android` is supported; x86_64 and armv7 Android emulators cannot use this build until the upstream `rusqlite bundled` issue is resolved.
- The JNI shim exposes a single entry point (`handle`). Future methods (e.g., a dedicated `free` or streaming interface) would follow the same `Java_com_goose_core_GooseBridge_<methodName>` naming pattern.

---

## Appendix

### JNI function signature summary

```
pub extern "system" fn Java_com_goose_core_GooseBridge_handle(
    env: JNIEnv,
    _class: JClass,
    request_json: JString,
) -> jstring
```

The function takes a Java `String` (the JSON request), delegates to the existing `handle_bridge_request_json` Rust function, and returns a Java `String` (the JSON response). All error paths return a valid JSON string rather than `null` or a panic.

### Build command

```
cargo ndk -t arm64-v8a build --release --lib
```

Requires: `cargo-ndk 4.1.2`, Android NDK r29.
