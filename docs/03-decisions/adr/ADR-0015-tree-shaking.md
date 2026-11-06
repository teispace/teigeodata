---
title: "ADR-0015: Tree-shakable everywhere"
status: accepted
updated: 2026-11-06
---

# ADR-0015: Tree-shakable everywhere

Status: accepted, 2026-11-06

## Context

Apps must pay only for what they use, in binary size and in data.

## Decision

- **Code:** capabilities are Cargo features (`search`, `reverse`, `tz`, `tz-rules`, `boundary`, `dem`, `postal`, `sync`, `mcp`, `server`), with lean defaults.
- **Data:** components (column groups) and levels of detail are chosen at build or download time.
- **Native and WASM:** prebuilt variants (`min`, `standard`, `full`) plus custom builds; JS packages are ESM with `sideEffects: false` and one file per type.
- Every variant has a size budget gate.

## Consequences

- Generators emit per-variant bindings; the parity gate runs per variant.

## Alternatives considered

One fat build (simple but heavy).

## Evidence

Maintainer direction, 2026-11-06.
