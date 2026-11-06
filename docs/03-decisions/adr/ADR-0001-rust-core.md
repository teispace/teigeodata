---
title: "ADR-0001: Rust core, one workspace"
status: accepted
updated: 2026-11-06
---

# ADR-0001: Rust core, one workspace

Status: accepted, 2026-11-06

## Context

The SDK must be fast, memory-safe, portable to native, mobile and WASM, and bindable from every major language.

## Decision

The core and all tooling are Rust (edition 2024, latest stable toolchain pinned in `rust-toolchain.toml`, MSRV recorded in `Cargo.toml`). One Cargo workspace holds the runtime crates, the boundary crate, the CLI, the MCP and HTTP servers, and `xtask`. The data pipeline (`forge/`) is a separate workspace so that its heavy dependencies never reach the runtime's dependency graph. The runtime `core` crate is `no_std` + `alloc`.

## Consequences

- Workspace lints are strict (`unsafe_code = forbid` except in a small, audited zero-copy module; pedantic clippy; no `unwrap`/`expect`/`panic`/indexing in library code).
- The runtime keeps a minimal, vetted dependency set (`cargo deny`, `cargo vet`).

## Alternatives considered

C++ (no memory safety, harder WASM and packaging), Zig (immature ecosystem for our bindings), Go (GC and FFI cost).

## Evidence

The maintainer's direction; [principles](../../00-vision/principles.md) P5, P6.
