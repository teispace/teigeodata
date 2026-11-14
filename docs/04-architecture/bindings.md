---
title: "Bindings: one safe API, an extracted IDL, generated layers"
status: draft
updated: 2026-11-14
---

# Bindings: one safe API, an extracted IDL, generated layers

Every language gets the same functions, errors and results from one Rust crate, without
hand-copied signatures ([ADR-0016](../03-decisions/adr/ADR-0016-bindings.md), D-066). Measured in
[spike S4](../02-research/benches/s4-bindings.md).

## 1. The flow

```
crates/teigeodata-api/src/lib.rs      the safe API, written by hand (pub fn + doc comments)
        │  cargo xtask bindings  (syn → bindings/idl.json, checked in)
        ▼
xtask/templates/*.j2                  one minijinja template per generated file
        ├─ ffi.rs.j2      → crates/teigeodata-ffi/src/generated.rs   C ABI (Rust extern "C")
        ├─ c.h.j2         → bindings/c/teigeodata.h                  its header
        ├─ swift.j2       → bindings/swift/Sources/TeiGeodata/…      over the C ABI
        ├─ dart.j2        → bindings/dart/lib/src/bindings.g.dart    over the C ABI
        ├─ python.rs.j2   → bindings/python/src/lib.rs (+ .pyi)      PyO3, calls the API
        ├─ node.rs.j2     → bindings/node/src/lib.rs (+ .d.ts)       napi-rs, calls the API
        └─ jni.rs.j2, java*.j2 → bindings/jvm/…                      JNI + Java, calls the API
```

- **Two kinds of backend from one IDL:**
  - C, Swift and Dart (later .NET and Go) sit on the C ABI;
  - Python, Node and the JVM (later WASM) use Rust shims that call the API directly, so their
    strings and buffers never pass through C.
- **Generated files are checked in** and formatted (`rustfmt` for Rust). `cargo xtask bindings
  --check` fails on any drift, and compares the header with cbindgen's reading of the ABI
  crate, which is an independent check of the header. Both run in CI's `bindings` job.
- **The shims are their own Cargo workspace** (`bindings/Cargo.toml`), like the forge, so PyO3,
  napi-rs and jni never enter the runtime dependency graph (D-067).

## 2. Boundary types

`teigeodata-api` may use only these in a `pub fn`. The extractor rejects anything else and names
the file, line and column.

| Rust | C ABI | Python | Node | JVM | Swift | Dart |
|---|---|---|---|---|---|---|
| `bool`, `i32`, `i64`, `f64` | same | `bool`, `int`, `float` | `boolean`, `number` | `boolean`, `int`, `long`, `double` | `Bool`, `Int32`, `Int64`, `Double` | `bool`, `int`, `double` |
| `u32`, `u64` | same | `int` (overflow checked) | `number` | `long`, range checked | `UInt32`, `UInt64` | `int` |
| `&str` in | `const uint8_t *`, `size_t` | `str` | `string` | `String` | `String` | `String` |
| `&[u8]` in | `const uint8_t *`, `size_t` | `bytes` | `Buffer` | `byte[]` | `Data` | `Uint8List` |
| `String` out | `tg_bytes` | `str` | `string` | `String` | `String` | `String` |
| `Blob` out | `tg_bytes` | `bytes` | `Buffer` (no copy) | `byte[]` | `Data` (no copy) | `Uint8List` |
| a handle (`pub struct` with private fields; its `impl` gives the methods) | opaque `tg_x *`, `tg_x_new`, `tg_x_free` | class, freed with the object | class | `TeiGeodata.X`, `AutoCloseable` plus a `Cleaner` | `final class`, `Sendable`, `deinit` frees | `Finalizable` class, `close()` |
| a record (`pub struct` of pub scalar or `Option<scalar>` fields, `Default`; D-276) | `tg_x` struct by `const tg_x *` (`NULL` for defaults), `has_f` beside each optional | keyword-only class | plain object, every field optional; the parameter optional | `TeiGeodata.X` record, `DEFAULTS` and `withF` | `struct`, every `init` argument defaulted | `const` class, named parameters |
| an enum (unit variants valued 0, 1, 2 …, `#[default]` on 0; D-277), as a record field | `uint32_t`, `TG_X_Y` constants, checked | enum, `X.Y` members | string union (`"y"`) | Java enum | `enum X: UInt32` | enhanced enum |
| `&mut dyn FnMut(scalars) -> bool` | function pointer + context | callable | function | `Callback…` interface | closure | function |
| `Result<T>` | status + `tg_last_error` | `…Error` exception | `Error` with `code` | `TeiGeodataException.…` | `throws(TeiGeodataError)` | `…Exception` |

- Other items: `pub const` integers become constants everywhere, and the `ErrorCode` enum
  becomes each language's error classes.
- Names become each language's convention: `sample_rows` is `sampleRows` in Node, Swift, Dart
  and Java, and `SAMPLE_ROWS_MAX` becomes `sampleRowsMax` in Swift and Dart.
- A handle's `new` becomes the constructor; a method without `&self` becomes a static method.
  A call on a closed JVM or Dart handle throws `IllegalStateException` or `StateError`; on the
  JVM a call holds a read lock, so `close` waits for the calls in flight.
- A callback returning `false` cancels with `Cancelled`. An exception thrown inside a callback
  cancels the call and reaches the caller unchanged (Python, Node, JVM, Dart).

## 3. Results: the blob

A result with rows crosses as one blob (`teigeodata-blob`, format v1, D-068 and D-275):
- a 16-byte header (magic `TGB1`, row count, column count, total length);
- 40-byte column descriptors: name, type, the data region, an auxiliary region and a null
  bitmap;
- the regions, each on an 8-byte boundary.

Columns are typed (`u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `u64`, `f32`, `f64`, UTF-8 with
Arrow-style offsets, and dictionary strings with `u16` or `u32` codes), so every language views
a column in place: a `Float64Array`, a `memoryview`, a `Float64List`. A column with nulls carries
an Arrow-style bitmap (1 for a value). A repeated string (a timezone, a country) is a dictionary
column, stored and decoded once: strings are most of a result's decoding time in every
language (S2). The Rust reader validates every offset, length, alignment, string, code and
bitmap before it returns a view; a test flips every byte of a blob and truncates it at every
length. Readers look columns up by name, so adding a column breaks no reader. The format and
its measurements: [S2](../02-research/benches/bindings-s2-result-format.md); v0 blobs still read.

## 4. Adding a function

1. Write it in `crates/teigeodata-api/src/lib.rs` with a doc comment and a test, using boundary
   types only.
2. Run `cargo xtask bindings`.
3. Add its expected result to `bindings/parity.json`, and one assertion to each language's parity
   test.

Nothing else changes: in S4 the sixth function took 2 minutes from start to every language
passing.

## 5. Running the suites

| Language | Build | Test |
|---|---|---|
| Rust ABI | `cargo test -p teigeodata-ffi` | the same |
| C | `cargo build --release -p teigeodata-ffi --lib` | `bindings/c/parity.c` (command in its header) |
| Python | `cd bindings && PYO3_BUILD_EXTENSION_MODULE=1 cargo build --release -p teigeodata-python`, copy the library to `python/teigeodata/_native.abi3.so` | `PYTHONPATH=. python3 -m unittest discover -s tests` |
| Node | `cargo build --release -p teigeodata-node`, copy to `node/teigeodata.node` | `node --test test/parity.test.js` |
| JVM | `cargo build --release -p teigeodata-jvm`, `javac` | `java -Djava.library.path=target/release … ParityTest parity.json` |
| Swift | the C ABI library | `swift run -c release Parity ../parity.json` |
| Dart | the C ABI library | `dart test` |

`bindings/bench.sh` builds all of them and runs the benchmark. The CI job `bindings` in
`.github/workflows/fast-check.yml` runs every suite on Linux.

## 6. Handles, threads and jobs (D-274)

The contract every language keeps. It is decided so that **Rust never calls foreign code from
a thread the foreign runtime does not own**: the one place each runtime's rules differ.

**Handles.** An API type with methods (`Engine`, `Store`, `Job`) crosses as an opaque pointer to
an `Arc` (`tg_engine *`), made by a constructor that returns `Result<Self>` and released once by
`tg_<type>_free`. Methods take `&self` only, so every handle is `Send + Sync`: the engine wraps
`Shared` (D-141), and adding or removing a layer publishes a new snapshot while queries go on;
the store wraps its state in a `Mutex`. In C, freeing a handle while a call on it is in flight
is the caller's error; every generated wrapper prevents it (below).

**Calls block, and are short.** Queries (`get`, `search`, `reverse`, `tz_at`, …) run on the
calling thread from any thread, concurrently, and take microseconds to a few milliseconds, so
no language wraps them in a thread hop by default. A synchronous callback (the `FnMut` of §2)
still runs on the caller's thread before the call returns.

**Long work is a job.** Store operations that touch the network or the disk at length
(`refresh`, `install`, `update`, `repair`) return a `Job` at once and run on a thread Rust owns
(one per job; no async runtime enters the core). A job has:

| Method | Does |
|---|---|
| `state()` | running, done, failed or cancelled, without blocking |
| `progress()` | a snapshot `(done, total, seq)` in bytes, updated per frame |
| `wait(seq, timeout_ms)` | blocks until `seq` moves on or the job ends, or the timeout passes |
| `cancel()` | sets a flag checked before every request and frame; the job ends `Cancelled` and leaves the journal as it was (an install commits last) |
| `result()` | the job's value or its typed error; errors of a job come from here, never from the thread-local `tg_last_error` |

Progress reaches a language by **waiting, not by callback**: the language calls `wait` on a
thread it owns and delivers each new snapshot to the user by its own rules. Cancellation is by
flag, so the user's cancel works from any thread. One store runs its jobs one at a time; a
second job waits for the first.

| Runtime | Handle lifetime | A job awaited as | Notes |
|---|---|---|---|
| C/C++ | explicit `free` | `wait` in a loop | none |
| Swift 6 | `final class`, `deinit` frees; `Sendable` (checked by the handle's `Sync`) | `async` function; `wait` on a detached thread resumes a continuation; an `AsyncStream` of progress | Task cancellation calls `cancel` |
| Kotlin/Java | `AutoCloseable` plus a `Cleaner`; a call holds a use count, `close` waits for it | `CompletableFuture`, progress through a listener on an executor; Kotlin `suspend` over it | JVM threads are already attached; Rust attaches none |
| Dart/Flutter | `NativeFinalizer` plus `close()` | `Future` and a `Stream` from a helper isolate that calls `wait` (blocking FFI there never blocks the UI isolate) | `@Native` leaf calls for queries |
| Python | object owns the handle, freed on drop | `wait` with the GIL released; an `asyncio` wrapper runs it in the default executor | queries release the GIL only above a size threshold; `gil_used = false` for free-threaded builds, proven by a stress test |
| Node | `napi` finalizer plus `close()` | `Promise` through an `AsyncTask` on the libuv pool; progress through an async iterator over `wait` | queries synchronous on the main thread |
| .NET | `SafeHandle` (its use count prevents a free during a call) | `Task` with `TaskCreationOptions.LongRunning`; `IProgress<T>` | `CancellationToken` calls `cancel` |
| Go | `runtime.AddCleanup` plus `Close()` | a goroutine calling `wait`; a channel of progress | `context.Context` calls `cancel` |
| WASM (browser) | `FinalizationRegistry` plus `free()` | no threads: jobs are written in JS over `fetch`, feeding the same verifier and installer step by step | queries synchronous |

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-14 | §2: enums (IDL format 4) | D-277 |
| 2026-11-14 | §2: records (IDL format 3) | D-276 |
| 2026-11-14 | §2: `&[u8]` and handles (the `Engine`), IDL format 2 | phase 5.1, D-274 |
| 2026-11-14 | §3: blob v1 (nulls, dictionaries, narrow integers) | S2, D-275 |
| 2026-11-14 | §6: handles, threads and jobs | phase 5.0, D-274 |
| 2026-11-07 | First version, from spike S4 | [S4](../02-research/benches/s4-bindings.md) |
