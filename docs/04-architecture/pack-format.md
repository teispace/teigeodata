---
title: "The .tgd pack format v1 (draft)"
status: draft
updated: 2026-11-06
---

# The .tgd pack format v1 (draft)

The byte-level contract between the builder (forge) and every reader. Decided by
[ADR-0009](../03-decisions/adr/ADR-0009-pack-format.md). The research and its 14 A/B benchmarks
are in [format, compression and indexing](../02-research/notes/format-compression-indexing.md).
The logical model is the [data schema](../05-data/schema.md).

**Status:** draft. Codecs marked *candidate* stay candidates until their benchmark (B1–B14) is
recorded. The format freezes only when all are (D-029). Until then `format_major = 0`.

## Rules for the whole file

- Every integer is little-endian. Every struct is read through unaligned `zerocopy` views, so
  alignment never affects correctness. Sections start on 64-byte boundaries for speed and for
  SIMD decoders.
- All offsets are u64 from the start of the file. **Every offset and length is validated
  against the file length before use.** A reader never panics on any input; decoders are fuzzed.
- **First 16 KiB rule:** the header, section table, manifest and signature fit in the first
  16 KiB. One range request plans every later read, including the section-delta update
  ([ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md)).
- **Deterministic:** the same inputs and builder version give the same bytes. Rows are sorted in
  Hilbert order of their point (spike S1), ties broken by ID.
- **Stable section boundaries:** a section whose data did not change between releases keeps its
  bytes and hash, so an update downloads only changed sections.
- **Forward compatible:** readers skip unknown section kinds by the table. A reader refuses a
  file whose `min_reader_major` is above its own.

## Layout

```
HEADER (128 B)
  magic[8]            0x89 'T' 'G' 'D' '\r' '\n' 0x1A '\n'
  format_major u16, format_minor u16, min_reader_major u16, flags u16
  pack_uuid[16], build_id u64 (hash of builder version + sources.lock), created_unix u64
  section_table_off u64, section_count u32, manifest_off u64, manifest_len u32
  signature_off u64, signature_len u32
  reserved (zero)
  header_hash[16]     truncated BLAKE3 of the bytes above
SECTION TABLE: section_count × 80 B
  kind u32, encoding u16, flags u16, offset u64, length u64, decoded_length u64,
  item_count u64, blake3[32], reserved[8]
MANIFEST: our minimal TLV (own reader; no general CBOR/JSON parser in the runtime)
SIGNATURE: key_id[8], Ed25519 over BLAKE3(header ‖ section table ‖ manifest)
── first 16 KiB ends ──
SECTIONS (64-byte aligned, any order, each hashed)
FOOTER (32 B): magic repeat, file length u64, header_hash[16]
```

The section table's hash goes in the catalog entry (`custom.sections`), so a client can compare
installed and new packs section by section.

## Manifest

| Entry | Content |
|---|---|
| identity | address (`np`, `ae-du`, `in-br-t07`), component, release, pack UUID ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)) |
| coverage | bbox, region IDs covered, precision tier |
| schema | schema version, the components present, the levels of detail present |
| dependencies | other packs (address + hash) this pack refers to |
| versions | tzdb release, boundary release, source versions |
| perspective | the default perspective and the perspectives present ([ADR-0006](../03-decisions/adr/ADR-0006-perspective.md)) |
| licence | `odbl` (main) or `cc-by-4.0` (lite), with the attribution text reference |
| columns | per-column encoding descriptors: codec ID, grid scale, block size, parameters |

## Sections

| # | Kind | Content | Feature | Codec (candidate until benchmarked) |
|---:|---|---|---|---|
| 1 | IDMAP | stable u64 IDs; ID → row | `core` | FOR bit-packed IDs; MPHF or sorted search (B5) |
| 2 | PLACES | point (1e-6° grid, delta in Hilbert order), kind and class codes, flags | `core` | delta + zigzag + bit-packed blocks of 128 with zone maps (S1: 4.06 B per place; B1) |
| 3 | HIERARCHY | parent row; children lists; validity | `core` | bit-packed parents; Elias–Fano children |
| 4 | NAMES | name records (lang, script, type), text | `names` | zstd blocks of 64 names or FSST per script group (S1: 1.15 MB against 2.06 MB for Nepal; B3, B4) |
| 5 | SEARCH | FST over normalised keys and phonetic keys; postings; scores; top-k structure | `search` | `fst` + Elias–Fano postings + block RMQ (B7; spike S3) |
| 6 | PLACE_INDEX | packed Hilbert R-tree over points | `spatial` | flatbush-style layout with i32 boxes (B8) |
| 7 | ARCS | shared arcs, delta coordinates on 1e-7°, per-vertex LOD level | `geometry` | bit-packed deltas (B10) |
| 8 | POLYGONS | ring and part tables (signed arc refs), bbox, label point | `geometry` | — |
| 9 | POLY_INDEX | packed R-tree over polygon boxes | `geometry` | as 6 |
| 10 | REGION | region index: top grid, quadtree in Morton order, leaf table, edge-ID runs, region ID → (kind, target) | `region` | spike S2 design (below) |
| 11 | POSTAL | FST over codes → postal records | `postal` | `fst` |
| 12 | COMPONENTS | per component: presence bitmap + rank directory + dense columns | `components` | rank9-style directory (B6) |
| 13 | DEM | tile directory + coded tiles at ±1 m | `dem` | MED prediction + entropy coder (B11) |
| 14 | BLOBS | cold data (attribution text, long strings) | `zstd` | zstd blocks with a dictionary, `ruzstd` decode (B4) |
| 15 | BLOCK_HASHES | per-block BLAKE3 for large sections (lazy verification) | `verify` | 16 or 64 KiB blocks (B12) |

### REGION (from spike S2)

The design proven exact in [spike S2](../02-research/benches/s2-region-index.md):

- **Grid and edges:** source edges are quantised to the 1e-7° grid, and edges are straight in
  lon/lat. Edge geometry is shared with ARCS where the source allows (B9).
- **Leaves:** a leaf holds up to about 64 edges. It stores:
  - its reference point (strictly inside the cell, never on an edge);
  - the zone parity at that point for each zone with edges in the leaf;
  - the **set** of zones covering it, because source zones overlap;
  - its edges, as runs of consecutive edge IDs.
- **Edge-free leaves** store only their covering set. Runs of identical leaves collapse.
- **Layout:** a flat top-level grid, then Morton-sorted leaves, with each leaf's edges stored
  next to it. The target is under 100 ns per lookup on every distribution.
- **Lookups:**
  - a lookup returns every containing zone, and the single answer follows the precedence rule
    (Q-013);
  - points outside lat [−90, 90] and lon [−180, 180] are rejected;
  - points exactly on an edge follow a documented half-open rule.

## Verification on open

1. Check the header magic, the header hash and the footer (detects truncation).
2. Check every section table entry: `offset + length ≤ file length`, alignment, and that
   sections do not overlap.
3. Verify the signature, when a trusted key is configured or required.
4. Verify each section's BLAKE3: eagerly for small packs, or lazily on first use per block
   (B12 decides the default).

## Physical file names and installs

`address.component.release.hash8.tgd` ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)).
A new version always gets a new file. A file that may be mapped is never written over.

## Open items

- The 14 benchmarks B1–B14 (catalogue in [benchmarks](../06-engineering/benchmarks.md)).
- The manifest TLV tag list and its fuzz corpus.
- Whether the reader crate keeps `#![forbid(unsafe_code)]` with mmap outside it (B13).
