---
title: "ADR-0016: Bindings: designed C ABI, our own IDL and generators, parity gate"
status: proposed
updated: 2026-11-06
---

# ADR-0016: Bindings: designed C ABI, our own IDL and generators, parity gate

Status: proposed, 2026-11-06 (revised the same day from research)

## Context

The same API, signatures and behaviour must reach C, C++, Node, WASM, Dart/Flutter, Python, JVM, Android, Swift, .NET and Go.

## Decision

A boundary crate exposes a designed, versioned C ABI. An API description (IDL) is extracted from it, with doc-comment metadata (units, ranges, nullability, examples). **Our own generators** (in Rust, via `xtask`) emit each language's mechanical layer; thin hand-written layers make it idiomatic. Columnar results cross the boundary as a self-describing blob decoded zero-copy. Panics never cross the boundary. A parity gate runs the same golden queries in every binding. Revised by [bindings and MCP](../../02-research/notes/bindings-and-mcp.md) (2026-11-06):

- **One IDL, two kinds of backend.** C/C++, Swift, .NET, Go and Dart sit on the C ABI. **Node and WASM do not:** the generators emit napi-rs and wasm-bindgen shims (Rust) that call the core directly, from the same IDL. **Python** uses generated PyO3 shims (abi3 wheels, and `abi3t` for free-threaded 3.15+). **JVM and Android** share one generated JNI layer ([ADR-0027](ADR-0027-jni-for-jvm-and-android.md)).
- **A small ABI surface:** opaque handles, POD structs, UTF-8 slices, one columnar result blob, error codes plus a last-error accessor, callbacks as a function pointer plus a context pointer, and async only through job handles.
- **Borrow before building:** study Diplomat's HIR and UniFFI's source parser before writing the extractor. Neither is adopted as a dependency (Diplomat lacks async, Swift, Go and JNI; UniFFI serialises every compound value and its third-party backends lag).
- Distribution: Swift through SwiftPM only (CocoaPods trunk goes read-only on 2026-12-02); Dart through build hooks (stable since Dart 3.10 / Flutter 3.38).
- Accepted after spikes S4 (generators) and the columnar result format spike.

## Consequences

- The API reference on the site is generated from the IDL.
- Android needs a JNI emitter (there is no Java FFM on Android); Swift uses the C module plus a generated Swift layer.

## Alternatives considered

Per-ecosystem tools (no single source of truth), UniFFI or Diplomat alone (gaps in targets, callbacks or zero-copy results).

## Evidence

Research thread of 2026-11-06.
