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
- [x] Stable ID: namespace and serial, the text form `tgd:<cc>/<serial>` (Crockford base32), parse and format, round-trip tests on every edge (every namespace, serial 1 and 2⁴⁸ − 1, 200,000 random IDs, 19 rejected forms)
- [x] Grid points at 1e-6° and 1e-7°, with checked conversion from degrees (range, NaN, exact half-away rounding; 500,000 random grid values round-trip through `f64` degrees)
- [x] Hilbert index on the grid, the row order of every pack (S1): a branch-free prefix-scan version equal to the classic loop on 10⁶ random points, every edge and the first 64 × 64 cells; 8–12 ns per point against 39–47 ns for the loop (load 12, 2026-11-07); continuity and inverse tested. Each test proven to fail on a planted bug

### 1.2 `teigeodata-format`: the container
- [x] Header, section table and footer as `zerocopy` structs, sizes and the hashed prefix asserted at compile time
- [x] The TLV manifest: canonical order, the critical bit, tag 0 reserved, known values checked on open, one tag registry shared by reader, writer and a docs test (nesting reuses the parser on a value; first used by column descriptors)
- [x] Writer (`write` feature): plans from lengths and hashes so large packs stream with bounded memory; 64-byte alignment, BLAKE3 per section, deterministic UUID and time, the first-16-KiB rule; a failed call leaves it unchanged (a bug the round-trip harness found)
- [x] Reader: every check of "verification on open", typed errors, no panic on any input; canonical (an accepted file is exactly what the writer produces)
- [x] Signature (`signature` feature): Ed25519 `verify_strict` over a domain prefix and the head digest; key IDs; a 72-byte slot filled from the digest alone (offline keys)
- [x] Harnesses (D-072: plain functions, a seeded driver under `cargo test`): raw bytes into `open` with the canonical oracle; a model through the writer and back with every single-byte change and truncation detected; structure-aware mutation with resealing; the manifest alone. Ten planted bugs, each caught
- [x] The [pack format](../../04-architecture/pack-format.md) updated to what was built (format 0.1); kinds and tags checked against it by tests

### 1.3 Benchmarks
- [x] `teigeodata-bench`: shared seeded workloads, criterion suites `core` and `format`; L1 0.71 µs at 1–100 MB, L2 1.76 GiB/s, L3 34 µs; Hilbert 4.75× the loop ([results](../../02-research/benches/phase-1.md))
- [x] B13: safe code with fixed-size arrays has no bounds checks left and is 22–44% faster than `bitpacking`'s `unsafe` SIMD; `forbid(unsafe_code)` holds ([results](../../02-research/benches/phase-1.md#b1-integer-blocks-with-b13))

### 1.4 Codecs and their sections (each with its A/B benchmark)
- [x] B1 integer blocks, the codec: `teigeodata_core::bitpack`, 128-value lane-interleaved blocks, FOR or zigzag delta; 8.5–11 G/s FOR decode, `get` 5 ns (FOR) and 23 ns (delta); 50 KB of code (D-073, [results](../../02-research/benches/phase-1.md#b1-integer-blocks-with-b13))
- [x] B1 into sections: one column table for every integer section (D-074); PLACES 5.2 B and HIERARCHY 3.0 B per place on Nepal, children below Elias–Fano; reads 3–10 ns; WASM decode 4 G/s ([results](../../02-research/benches/phase-1.md#b1-on-sections-places-and-hierarchy))
- [x] B5 ID to row: sorted IDs and a permutation on the column table, with a new ramp coding; 0.35 B per place, hit 82–91 ns, miss 32–41 ns; PtrHash measured (3–27 ns, 12× bigger) and kept as the upgrade path (D-075, [note](../../02-research/notes/id-to-row.md), [results](../../02-research/benches/phase-1.md#b5-id-to-row-idmap-and-redirects)) → IDMAP; REDIRECTS
- [x] B6 presence → COMPONENTS: a poppy rank bitmap (column-table codec 1) or sorted rows, the smaller per component; crossover near 10%; rank 6.9 ns, a field read 7–20 ns; select not needed (D-076, [note](../../02-research/notes/presence.md), [results](../../02-research/benches/phase-1.md#b6-presence-components))
- [x] B3 names: FSST with our own safe decoder and deterministic trainer (3–5% smaller than `fsst-rs`), per-script tables, column-table codec 2 → NAMES; 10.6 B per place on Nepal, a canonical name 65 ns (D-077, [note](../../02-research/notes/names-storage.md), [results](../../02-research/benches/phase-1.md#b3-names-fsst))
- [ ] B4 blobs: zstd frames, `ruzstd` decode → BLOBS (research in the names note)
- [x] B12 integrity: 16 KiB BLAKE3 chaining values per section of 1 MiB or more, merged into the table hash (no separate trust root); proof 237 µs and a first read 8.8 µs at 100 MB, against 55.6 ms eager (D-078, [note](../../02-research/notes/block-hashes.md), [results](../../02-research/benches/phase-1.md#b12-integrity-block_hashes)) → BLOCK_HASHES; L2

### 1.5 Hardening
- [x] Nightly CI: cargo-fuzz (libFuzzer) on the same harness functions, Miri on the reader tests
- [ ] gungraun instruction-count benchmarks in CI (Valgrind; Linux), the load-independent PR gate
- [x] Determinism gate: every test pack built twice, bytes compared
- [ ] Sizes (Z2) of `core` and `format` per feature set

### 1.6 Exit review
- [ ] M1 criteria met; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Phase 1 planned | M0 met (D-069) |
| 2026-11-07 | 1.1 done: `teigeodata-core` | the plan held |
| 2026-11-07 | 1.2 done: the container; bolero replaced by plain harness functions | D-072 |
| 2026-11-07 | 1.3: criterion suites; gungraun moved to 1.5 with the other CI work; B13 moved into B1 | B13 needs a decoder hot loop, which B1 creates |
| 2026-11-07 | B1 codec and B13 done; the 1024-value layout dropped; B1's sections split into their own step | measured: 128-value blocks are smaller on every column, and safe code beats the `unsafe` rival |
| 2026-11-07 | B1's sections done: a shared column table; HIERARCHY as CSR, Elias–Fano dropped | measured: block bit-packing beats the Elias–Fano bound on child lists |
| 2026-11-07 | B5 done: sorted list over MPHF; ramp coding added to the codec | measured: 12× smaller at 3.4× the lookup time; size matters more offline |
| 2026-11-07 | B6 done: two presence forms instead of a bitmap only; select dropped | measured: each form loses 3.6–70× at the other end of the density range; no reader needs select |
| 2026-11-07 | B3 and B4 split; B3 done with FSST, lz4 rejected | measured: FSST is the only option both small and fast per place; zstd is 230× slower per place |
| 2026-11-07 | 1.5: nightly fuzz (8 libFuzzer targets: the 4 pack harnesses plus sections, bitpack, table, bits) and Miri; determinism gate in `tests/determinism.rs` | D-072: one harness per property, run by `cargo test` and libFuzzer |
| 2026-11-07 | B12 done: 16 KiB block CVs, coverage canonical | measured: 16 KiB reads 4× faster than 64 KiB for 0.15% more size; the hazmat merge removes a separate root |
