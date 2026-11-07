---
title: "Phase 1: core and format"
status: living
updated: 2026-11-07
---

# Phase 1: core and format

**Goal:** the core types and codecs, and the `.tgd` container: written, read, validated,
fuzzed, benchmarked and specified. Exit: milestone **M1** ([roadmap](../roadmap.md)).

**Research:**
- [core and format](../../02-research/notes/core-and-format.md): this phase's decisions;
- [format, compression and indexing](../../02-research/notes/format-compression-indexing.md): the
  codecs and the A/B list B1–B14;
- [S1](../../02-research/benches/s1-codecs.md): the first codec measurements.

**What M1 means by "every section kind"** (D-071):
- Every kind in the [pack format](../../04-architecture/pack-format.md) has its number, its frame,
  its validation and a round trip.
- These kinds carry real payloads now: IDMAP, PLACES, HIERARCHY, NAMES, COMPONENTS, BLOBS,
  BLOCK_HASHES and REDIRECTS.
- These payloads come later, inside the same frame:
  - Phase 2: SEARCH, PLACE_INDEX, POLY_INDEX, REGION, POSTAL;
  - with their benchmarks: ARCS and POLYGONS (B10), DEM (B11).

## Steps

### 1.0 Research
- [x] [Core and format](../../02-research/notes/core-and-format.md): crate split, `zerocopy` views, the TLV manifest, the stable ID, reproducible bytes, bolero, criterion and gungraun (D-070, D-071)

### 1.1 `teigeodata-core`
- [ ] Stable ID: namespace and serial, the text form `tgd:<cc>/<serial>` (Crockford base32), parse and format, round-trip tests on every edge
- [ ] Grid points at 1e-6° and 1e-7°, with checked conversion from degrees (range, NaN, rounding)
- [ ] Hilbert index on the grid, the row order of every pack (S1), proven against a reference implementation

### 1.2 `teigeodata-format`: the container
- [ ] Header, section table and footer as `zerocopy` structs, sizes asserted at compile time
- [ ] The TLV manifest: canonical order, the critical bit, nested values
- [ ] Writer (`write` feature): 64-byte alignment, BLAKE3 per section, deterministic UUID and time, the first-16-KiB rule
- [ ] Reader: every check of "verification on open", typed errors, no panic on any input
- [ ] Signature (`signature` feature): Ed25519 `verify_strict` over BLAKE3(header ‖ table ‖ manifest)
- [ ] bolero harnesses: raw bytes into `open`; a model through the writer and back; the manifest alone; each proven to find a planted bug
- [ ] The [pack format](../../04-architecture/pack-format.md) updated to what was built

### 1.3 Benchmarks
- [ ] `teigeodata-bench`: criterion locally, gungraun in CI; L1 (open) and L3 (signature)
- [ ] B13: checked against unchecked access; decides whether `forbid(unsafe_code)` holds

### 1.4 Codecs and their sections (each with its A/B benchmark)
- [ ] B1 integer blocks: frame-of-reference bit-packing, delta and zigzag → PLACES and HIERARCHY
- [ ] B5 ID to row: sorted search, Eytzinger, MPHF → IDMAP; REDIRECTS
- [ ] B6 rank and select → COMPONENTS (presence bitmaps)
- [ ] B3 and B4 names: FSST, zstd blocks (`ruzstd` against C zstd), lz4 → NAMES, BLOBS
- [ ] B12 integrity: eager against lazy hashing, 16 against 64 KiB blocks → BLOCK_HASHES; L2

### 1.5 Hardening
- [ ] Nightly CI: libFuzzer through `cargo bolero test`, Miri on the reader tests
- [ ] Determinism gate: every test pack built twice, bytes compared
- [ ] Sizes (Z2) of `core` and `format` per feature set

### 1.6 Exit review
- [ ] M1 criteria met; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Phase 1 planned | M0 met (D-069) |
