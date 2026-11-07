---
title: "Spike S4: one safe API, an extracted IDL and generated bindings in six languages"
status: research
updated: 2026-11-07
---

# Spike S4: one safe API, an extracted IDL and generated bindings in six languages

**Question:** can one Rust API reach C, Swift, Dart, Python, Node and the JVM with no
hand-copied signatures, the same behaviour everywhere, and a call cost near each runtime's
floor? The method is in the [S4 note](../notes/bindings-spike-s4.md); the design that came out
of it is in [bindings](../../04-architecture/bindings.md) and the [C ABI](../../04-architecture/abi.md).

**Answer: yes.** Six functions reach six languages and C from one crate,
`crates/teigeodata-api`, through `cargo xtask bindings`. Every language passes the same parity
suite (`bindings/parity.json`, itself checked against Rust). The extra cost per call is 0.3 to
25 ns in five of the six runtimes and about 20 ns on the JVM. The largest costs are strings into
and out of Node and the JVM, and JVM callbacks.

## What was built

| Part | Where | Lines (non-blank) |
|---|---|---:|
| The safe API: `version`, `noop`, `fold`, `sample_rows`, `progress`, `distance_m` | `crates/teigeodata-api` | 330 |
| The result blob, format v0: encoder and validating reader, `no_std` | `crates/teigeodata-blob` | 589 |
| The `syn` extractor, the driver, the cbindgen cross-check | `xtask/src/bindings*` | 881 |
| Templates (minijinja), one per generated file | `xtask/templates/` | 865 |

Per language (lines in checked-in files; "hand" excludes tests):

| Target | Mechanism | Generated | Hand | Tests |
|---|---|---:|---:|---:|
| C ABI and header | Rust `extern "C"` + `teigeodata.h` | 212 | 173 (runtime: buffers, last error, panic guard) | 183 |
| Python | PyO3 0.30 module, abi3 (3.10+), stubs | 176 | 16 | 69 |
| Node | napi-rs 3, `index.d.ts` | 172 | 7 | 64 |
| JVM | jni 0.22 + Java class and exceptions | 315 | 0 | 153 |
| Swift | over the C ABI (SwiftPM, module map) | 113 | 33 | 71 |
| Dart | over the C ABI (`dart:ffi`) | 193 | 4 | 99 |

**Adding the sixth function** (`distance_m`, four `f64` parameters and a range error) took 2
minutes of wall-clock time on 2026-11-07. That covered one Rust function with its test, one
`cargo xtask bindings` (11 files rewritten), one parity assertion per language, and a rebuild
and rerun of every suite. No template changed.

## Gates, each proven to fail

- `cargo xtask bindings --check`:
  - fails when a generated file differs from the API (a hand edit to the header was caught);
  - fails on an unsupported type, with its line (`noop(x: Vec<u8>)` gave
    `lib.rs:76:16: noop: parameter type is not a boundary type`);
  - compares every declaration of the generated header with cbindgen 0.29.4's reading of the
    ABI crate. A deliberate template bug (`uint32_t` for a length) was reported with both
    signatures.
- `cargo xtask check-workspace` keeps the lint tables of `bindings/` and the ABI crate equal to
  the main one, except `unsafe_code`, which they may only lower from `forbid` to `deny`.
  Setting `panic = "allow"` in the ABI crate failed it.
- Parity in every language, from one file. The Python, Node, JVM and Dart tests also check that
  an exception thrown inside a callback reaches the caller unchanged (the same object).

## Call cost

`bindings/bench.sh 3`, 2026-11-07, M2 Pro. The machine was shared: load averages 12–15 on 10
cores, from a VM running other work. The table shows the best of 3 runs; each run takes the best
of 5 rounds. Each cell is ns per call; the callback column is ns per callback within one
`progress(1000)` call.

| | noop | fold("Kathmandu") | sample_rows(1000) + sum of `lat` | callback |
|---|---:|---:|---:|---:|
| Rust, direct (the floor) | 0.0 | 36.2 | 18,512 | 0.3 |
| C ABI, from Rust | 0.3 | 41.9 | 18,401 | 0.9 |
| C | 3.7 | 45.2 | 16,912 | 1.2 |
| Swift | 1.8 | 56.6 | 17,187 | 5.6 |
| Dart (AOT) | 6.4 | 87.9 | 23,689 | 28.5 |
| Python 3.15 | 27.9 | 98.4 | 32,121 | 44.8 |
| Node 26 | 18.3 | 230.2 | 26,863 | 62.6 |
| JVM 25 | 20.1 | 507.8 | 19,236 | 234.3 |

**Findings:**
- **The C ABI costs almost nothing:** 0.3 ns per call and 6 ns per string round trip over the
  direct call. Swift adds 15 ns per string (UTF-8 view and `String` creation).
- **Blobs:**
  - Swift receives them as `Data` with no copy, freed by the library, and Node as an external
    `Buffer`, also with no copy.
  - Python and the JVM copy once (into `bytes` and `byte[]`), and Dart copies into a
    `Uint8List`. At 37 KB the copy is a few µs.
  - Python's extra 14 µs is mostly the sum in Python over a `memoryview`, not the binding.
  - Zero-copy for Python (buffer protocol), the JVM (direct `ByteBuffer` with a cleaner) and Dart
    (a native finalizer) is follow-up work for the result-format spike.
- **Strings into the JVM and Node cost the most:** about 470 ns and 190 ns over the floor. The
  JVM converts through modified UTF-8 both ways; napi reads a string's length and then its bytes.
  Search returns blobs, not strings, so this matters only for single-name calls. It is measured
  again with the real search API in Phase 1.
- **Callbacks:**
  - Swift's are close to free (5.6 ns) and Dart's (`NativeCallable.isolateLocal`) cost 28.5 ns.
  - Python's cost 45 ns and Node's 63 ns.
  - The JVM's cost 234 ns even with the method resolved once per call. The first version looked
    the method up on every callback and cost 644 ns. Progress callbacks fire per batch, not per
    row, so this is acceptable.
- **Two speed-ups came from measuring:**
  - The ASCII fast path in `teigeodata-names::fold` made it 10× faster (376 ns → 36 ns). A
    differential test proves it equal to the full fold on every 1–2 character ASCII string and
    200,000 random ones.
  - Allocation-free row names halved `sample_rows`.
- **A stale library once gave wrong numbers:** `cargo build --example` does not relink the
  cdylib, so Dart first measured an old build (fold 375 ns). `bench.sh` now builds every
  artifact explicitly.

## Panics: `abort` or `unwind`

The C ABI library built with the `dist` profile (fat LTO, stripped), macOS arm64:

| `panic` | Size | gzip -9 |
|---|---:|---:|
| `abort` | 517,280 B | 241,255 B |
| `unwind` | 517,456 B | 243,975 B |

Unwinding costs 176 bytes, or 2.7 KB compressed, because the prebuilt standard library carries
unwind tables either way. With `unwind`, the boundary guard turns a panic into
`TG_ERROR_INTERNAL` with a message, and the host keeps running (a test proves it). With `abort`,
the host process would die. The `dist` profile now unwinds (D-065). Most of the 517 KB is Unicode
normalisation tables from the name fold.

## What follows

- ADR-0016's generator half is confirmed. The design changed in two ways: the IDL comes from the
  safe Rust API, not from the C ABI, and the shims live in their own workspace (D-066, D-067).
  The ADR stays `proposed` until the result-format spike.
- Blob v0 (D-068) is the baseline the result-format spike must beat. It compares Arrow IPC,
  FlatBuffers and v0 on real search results.
- Out of scope here, and still to come (the bindings note's S3–S9): async job handles, WASM, Go,
  .NET, Android packaging, Dart build hooks, and free-threaded Python (`abi3t`).
