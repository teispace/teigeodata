---
title: "Spike S4: one safe API, an extracted IDL, generated bindings"
status: research
updated: 2026-11-07
owner: bindings
revisit: 2027-02-07
---

# Spike S4: one safe API, an extracted IDL, generated bindings

## 1. Question

How do we get the same API into C, Swift, Dart, Python, Node and the JVM with one source of
truth, no hand-copied signatures, and a call cost close to each runtime's floor? This is the
[bindings note](bindings-and-mcp.md)'s spike S1 and Phase 0 step S4; it decides whether
ADR-0016 is accepted as written.

## 2. Summary and recommendation

**Result (2026-11-07):** built and measured as recommended below. Six functions reach C,
Swift, Dart, Python, Node and the JVM, and every language passes one parity suite. The C ABI
adds 0.3 ns per call, and a new function takes one regeneration. The shipped library unwinds
(176 bytes, D-065). Numbers: [S4 results](../benches/s4-bindings.md).


- **The source of truth is a safe Rust API crate,** `teigeodata-api`, written by hand: plain
  `pub fn`s with doc comments, using only the types the boundary allows. Nothing else is
  written per language except tests, benchmarks and a thin idiomatic layer.
- **The IDL is extracted from it with `syn`** into `bindings/idl.json` (checked in). Rust
  signatures carry the semantics (`&str` in, `String` out, a blob out, a callback), which the
  C signatures would lose (pointer and length pairs).
- **Every mechanical layer is generated from the IDL** by `cargo xtask bindings`:
  - the C ABI itself (`teigeodata-ffi`, Rust `extern "C"` wrappers) and its header;
  - Swift and Dart wrappers over the C ABI;
  - the PyO3, napi-rs and JNI shims (Rust), plus the Java class.
  - Generated files are checked in, and `cargo xtask bindings --check` fails on any drift
    (a CI gate).
  - The header is cross-checked against cbindgen's output for the generated ABI crate.
- **Results cross as one columnar blob** (`teigeodata-blob`): one aligned buffer of typed
  columns, decoded as views in each language. Format v0 is our own minimal layout; the
  format comparison is a separate spike (the bindings note's S2).
- **Unsafe code lives in generated, audited boundary modules only.** Those crates copy the
  workspace lints with `unsafe_code = "deny"`, allowed in one module with a reason;
  `check-workspace` enforces it.

## 3. Findings

**Versions** (checked live 2026-11-07): napi 3.16.1, napi-derive 3.7.0, napi-build 2.6.0,
`@napi-rs/cli` 3.10.12, PyO3 0.30.1, maturin 1.16.0, jni 0.22.4, syn 3.0.9, quote 1.0.47,
minijinja 3.0.5, cbindgen 0.29.4, Dart `ffi`/`hooks`/`code_assets` 2.2.0. Toolchains on the
build machine: Node 26.13, Dart 3.13.7, Python 3.15.2, Java 25.0.1, Swift 6.4.2.

**Why not extract from the C ABI:** a C signature `tg_fold(const uint8_t*, size_t,
tg_string*)` does not say the input is UTF-8 or who frees the output. The PyO3, napi and JNI
shims call the core directly and need exactly that meaning. Extracting from the safe API
keeps one meaning for both kinds of backend.

**Panics:** the workspace `dist` profile set `panic = "abort"`. With it, `catch_unwind` at the
boundary cannot catch anything, and a panic takes the host process down. Library lints forbid
panics, but a dependency could still panic. Measured: `unwind` costs 176 bytes on a 517 KB
library, so `dist` now unwinds (D-065).

## 4. Options compared

| IDL source | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| (A) Hand-written IDL file (JSON/TOML) | Simple to parse | A second copy of every signature; drifts from the code | No |
| (B) `syn` extraction from a safe API crate | One source; doc comments come free; no build step needed | We own a small parser; it accepts only the boundary types | **Use** |
| (C) A proc-macro emitting metadata at compile time | Sees resolved types | Needs a build to produce the IDL; macro on every function | Later, if (B) is too limited |
| (D) UniFFI or Diplomat | Mature | Rejected in ADR-0016 (serialised compounds; missing targets) | No |

## 5. What we build

1. `crates/teigeodata-api`: the slice. `version`, `noop`, `fold` (the real search fold),
   `sample_rows(n)` → blob, `progress(n, callback)`.
2. `crates/teigeodata-blob`: the encoder (`no_std` + alloc) and a reference decoder.
3. `xtask bindings`: the extractor and the generators (minijinja templates under
   `xtask/templates/`).
4. Generated: `crates/teigeodata-ffi` (C ABI), `bindings/c/teigeodata.h`,
   `bindings/swift`, `bindings/dart`, `bindings/python` (PyO3), `bindings/node` (napi),
   `bindings/jvm` (JNI and Java).
5. Per language: a parity test (the same inputs give the same outputs as Rust) and a
   benchmark printing ns per call.

**Measured:**
- generated and hand-written lines per language;
- ns per call for `noop`, `fold("Kathmandu")`, `sample_rows(1000)` with a decode and sum, and
  a 1,000-call callback;
- the Rust direct-call baseline;
- library size with `panic = "abort"` and with `panic = "unwind"`;
- the time to add a sixth function.

## 6. Impact on the plan

- ADR-0016 is revised on the results; it is accepted after the result-format spike (D-066).
- A boundary-crate page (ABI rules) and an IDL page go in `docs/04-architecture/`.
- New gates: bindings drift (`--check`), the header diff against cbindgen, and parity in each
  language.

## 7. Risks and open items

- Out of scope for this spike: async job handles, Go, .NET, WASM and Android packaging.
  These are the bindings note's S3–S9.
- Threading: callbacks run on the calling thread, synchronously. Cross-thread callbacks
  need the threading contract first.
- Dart `@Native` with build hooks needs the library as a code asset; the spike may load it
  with `DynamicLibrary.open` if hooks block it, and say so.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| napi / napi-derive / napi-build | 3.16.1 / 3.7.0 / 2.6.0 | 2026-11-07 | crates.io |
| PyO3 | 0.30.1 | 2026-11-07 | crates.io |
| jni | 0.22.4 | 2026-11-07 | crates.io |
| syn / quote / proc-macro2 | 3.0.9 / 1.0.47 / 1.0.107 | 2026-11-07 | crates.io |
| minijinja | 3.0.5 | 2026-11-07 | crates.io |
| cbindgen | 0.29.4 | 2026-11-07 | crates.io |
| maturin | 1.16.0 | 2026-11-07 | PyPI |
| Dart ffi / hooks / code_assets | 2.2.0 | 2026-11-07 | pub.dev |

## 9. Sources

- [Bindings and MCP note](bindings-and-mcp.md), 2026-11-06, and its sources.
- [ADR-0016](../../03-decisions/adr/ADR-0016-bindings.md), [ADR-0027](../../03-decisions/adr/ADR-0027-jni-for-jvm-and-android.md).
- [syn 3 documentation](https://docs.rs/syn), accessed 2026-11-07.
- [PyO3 user guide](https://pyo3.rs), [napi-rs](https://napi.rs), [jni crate](https://docs.rs/jni), accessed 2026-11-07.
