---
title: "ADR-0009: The .tgd pack format"
status: proposed
updated: 2026-11-06
---

# ADR-0009: The .tgd pack format

Status: proposed, 2026-11-06

## Context

Developers need small, fast, offline, verifiable data that works on native, mobile and WASM, and that merges at runtime.

## Decision

A custom binary format, `.tgd`: self-describing (manifest, coverage, schema, dependencies), little-endian, sectioned and aligned for memory mapping and zero-copy reads, componentised (optional column groups with presence bitmaps, so absent data costs about 1 bit), with levels of detail for geometry, per-section BLAKE3 checksums, and an Ed25519 signature. A format version and a minimum reader version are in the header. Standard formats (GeoJSON, GeoParquet, FlatGeobuf, PMTiles, SQLite, CSV) are **exports**, not the runtime format. The section layout and codecs are decided by [the format research](../../02-research/notes/format-compression-indexing.md) and A/B benchmarks.

Refined by [format, compression and indexing](../../02-research/notes/format-compression-indexing.md) (2026-11-06): 64-byte aligned sections; little-endian unaligned types read through `zerocopy`; everything a reader needs to plan reads in the first 16 KiB; a footer; per-column encoding descriptors; codecs chosen by the builder per column from measured samples. The reader is our own small crate (almost no crate can read a layout we define in place); heavy crates run only in the builder or as test oracles. Runtime dependencies are limited to `zerocopy`, `blake3`, `ed25519-dalek`, `fst` and, if the benchmark pays, `ruzstd`. Codecs stay *candidates* until their A/B benchmark passes; the format freezes only when all 14 benchmarks of the note are recorded.

## Consequences

- Readers validate every offset and length before use; decoders are fuzzed.
- The format specification is a document in `docs/04-architecture/pack-format.md`, versioned with the code.

## Alternatives considered

SQLite (larger, harder in WASM, no zero-copy), Parquet/Arrow (heavy readers, not built for point queries), FlatGeobuf (geometry only).

## Evidence

Research thread of 2026-11-06.
