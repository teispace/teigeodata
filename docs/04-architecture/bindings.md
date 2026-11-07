---
title: "Bindings: one safe API, an extracted IDL, generated layers"
status: draft
updated: 2026-11-07
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
| `String` out | `tg_bytes` | `str` | `string` | `String` | `String` | `String` |
| `Blob` out | `tg_bytes` | `bytes` | `Buffer` (no copy) | `byte[]` | `Data` (no copy) | `Uint8List` |
| `&mut dyn FnMut(scalars) -> bool` | function pointer + context | callable | function | `Callback…` interface | closure | function |
| `Result<T>` | status + `tg_last_error` | `…Error` exception | `Error` with `code` | `TeiGeodataException.…` | `throws(TeiGeodataError)` | `…Exception` |

- Other items: `pub const` integers become constants everywhere, and the `ErrorCode` enum
  becomes each language's error classes.
- Names become each language's convention: `sample_rows` is `sampleRows` in Node, Swift, Dart
  and Java, and `SAMPLE_ROWS_MAX` becomes `sampleRowsMax` in Swift and Dart.
- A callback returning `false` cancels with `Cancelled`. An exception thrown inside a callback
  cancels the call and reaches the caller unchanged (Python, Node, JVM, Dart).

## 3. Results: the blob

A result with rows crosses as one blob (`teigeodata-blob`, format v0, D-068):
- a 16-byte header (magic `TGB0`, row count, column count, total length);
- 40-byte column descriptors;
- column data, each column on an 8-byte boundary.

Columns are typed (`i32`, `u32`, `i64`, `u64`, `f32`, `f64`, UTF-8 with Arrow-style offsets), so
every language views a column in place: a `Float64Array`, a `memoryview`, a `Float64List`. The
Rust reader validates every offset, length, alignment and string before it returns a view; a test
flips every byte of a blob and truncates it at every length. The result-format spike may replace
v0.

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
| Node | `cargo build --release -p teigeodata-node`, copy to `node/teigeodata.node` | `node --test test/` |
| JVM | `cargo build --release -p teigeodata-jvm`, `javac` | `java -Djava.library.path=target/release … ParityTest parity.json` |
| Swift | the C ABI library | `swift run -c release Parity ../parity.json` |
| Dart | the C ABI library | `dart test` |

`bindings/bench.sh` builds all of them and runs the benchmark. The CI job `bindings` in
`.github/workflows/fast-check.yml` runs every suite on Linux.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | First version, from spike S4 | [S4](../02-research/benches/s4-bindings.md) |
