---
title: "The .tgd pack format v1 (draft)"
status: draft
updated: 2026-11-07
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
  against the file length before use.** A reader never panics on any input; the reader is
  fuzzed (D-072).
- **First 16 KiB rule:** the header, section table, manifest and signature fit in the first
  16 KiB. One range request plans every later read, including the section-delta update
  ([ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md)).
- **Canonical:** a reader accepts a file only if it is exactly what the writer produces for its
  content. Every byte is the head, a section, zero padding or the footer, with no slack, and a
  fuzz property checks that the writer reproduces every accepted file byte for byte.
- **Deterministic:** the same inputs and builder version give the same bytes. Rows are sorted in
  Hilbert order of their point (spike S1), ties broken by ID. `created_unix` is
  `SOURCE_DATE_EPOCH` (else 0), and `pack_uuid` is the first 16 bytes of
  BLAKE3(`teigeodata pack uuid\0` ‖ address ‖ `\0` ‖ component ‖ `\0` ‖ release ‖ `\0`), never
  random ([core and format](../02-research/notes/core-and-format.md#34-reproducibility)). A reader
  refuses a UUID that does not match.
- **Stable section bytes:** a section whose data did not change between releases keeps its
  bytes and hash, so an update downloads only changed sections. Its offset may move.
- **Forward compatible:** readers skip unknown section kinds and unknown non-critical manifest
  tags. A reader refuses a file whose `min_reader_major` is above its own.

## Layout

Format 0.1, as built in `crates/teigeodata-format` (Phase 1).

```
HEADER (128 B)                                   offset
  magic[8]           0x89 'T' 'G' 'D' '\r' '\n' 0x1A '\n'     0
  format_major u16, format_minor u16                          8
  min_reader_major u16, flags u16 (0)                         12
  pack_uuid[16]                                               16
  build_id u64       hash of builder version + sources.lock   32
  created_unix u64   SOURCE_DATE_EPOCH                        40
  table_off u64      = 128                                    48
  section_count u32, manifest_len u32                         56
  manifest_off u64   = 128 + 80 × section_count               64
  signature_off u64  = manifest_off + manifest_len, or 0      72
  signature_len u32  72, or 0 when unsigned                   80
  file_len u64                                                84
  reserved[20] (0)                                            92
  head_hash[16]      first 16 bytes of the head digest        112
SECTION TABLE: section_count × 80 B, kinds strictly increasing
  kind u32, encoding u16, flags u16 (0), offset u64, length u64, decoded_length u64,
  item_count u64, blake3[32], reserved[8] (0)
MANIFEST: canonical TLV (below)
SIGNATURE (signed packs): key_id[8], Ed25519 signature[64]
── the head ends; at most 16 KiB ──
SECTIONS in table order, each at the next 64-byte boundary after the previous, zero padding
FOOTER (32 B), right after the last section: magic, file_len u64, head_hash[16]
```

- **Head digest:** BLAKE3(header bytes 0–111 ‖ section table ‖ manifest). The table holds every
  section's hash, so the digest covers the whole file except the signature.
- **Signature:** Ed25519 over `teigeodata pack signature v0\0` ‖ head digest, checked with
  `verify_strict`. The key ID is the first 8 bytes of BLAKE3 of the public key. The writer
  leaves a 72-byte slot and the signer fills it from the digest alone, so an offline key never
  sees the pack.
- **Raw sections** (`encoding` 0) have `decoded_length = length`.
- The section table's hash goes in the catalog entry (`custom.sections`), so a client can
  compare installed and new packs section by section.

## Manifest

The encoding is our own canonical TLV
([core and format](../02-research/notes/core-and-format.md#32-the-manifest)):
- **An entry** is a tag (u16 LE), a length (u32 LE) and that many value bytes.
- **Canonical:** tags are strictly increasing, so a manifest has exactly one encoding. A reader
  refuses repeated or out-of-order tags.
- **Unknown tags:** a reader skips an unknown tag, unless its high bit (`0x8000`) is set. That
  bit means "must understand": a reader that does not know the tag refuses the pack, so a field
  that changes meaning is never ignored silently.
- **Known tags are checked on open**, so their typed getters cannot fail.
- **Tag 0** (with or without the critical bit) is reserved, as is section kind 0.
- **Nesting:** a structured value, such as a column descriptor, is itself a TLV list.

Tags known to format 0.1. A test fails if a tag in the code is missing here.

| Tag | Name | Value | Required |
|---|---|---|---|
| `0x0001` | `address` | 1–64 of `a-z 0-9 -`, starting alphanumeric: `np`, `ae-du`, `in-br-t07` ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)) | yes |
| `0x0002` | `component` | as `address`: `places`, `boundary-lod1` | yes |
| `0x0003` | `release` | as `address`, dots allowed: `2026.11.0` | yes |
| `0x0004` | `schema` | u32: the data schema version | |
| `0x0005` | `licence` | as `address`: `odbl` (main) or `cc-by-4.0` (lite) | |
| `0x0006` | `bbox` | 4 × i32 on the 1e-7° grid: min lat, min lon, max lat, max lon; min lon above max lon crosses the antimeridian | |
| `0x0007` | `perspective` | as `address`: the default perspective, a country code ([ADR-0006](../03-decisions/adr/ADR-0006-perspective.md)) | |

Still to add, with the sections that need them: dependencies (other packs by address and hash),
versions (tzdb, boundary and source releases), the perspectives present, the components and
levels of detail present, and per-column encoding descriptors (codec ID, grid scale, block size,
parameters).

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
| 16 | REDIRECTS | retired IDs: each to its survivor, or to none for a tombstone ([ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)) | `core` | sorted ID pairs, as IDMAP (B5) |

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

`Pack::open` checks, in order, and refuses on the first failure:
1. the length (at least a header and a footer), the magic at both ends, the versions, zero flags
   and reserved bytes, and that the header, the footer and the real length agree;
2. the head layout (table, manifest and signature contiguous, within 16 KiB), then the head
   digest against the header and the footer;
3. the manifest (canonical order, known values, no unknown critical tag, the required tags) and
   the UUID against the identity;
4. every table entry: kinds strictly increasing, each section at the next 64-byte boundary, zero
   padding, inside the file; the footer right after the last section.

Then, on demand:
- `Pack::verify_signature(keys)` against a set of trusted keys;
- `Pack::verify_sections()` or `Section::verify()` for the BLAKE3 of each section: eagerly for
  small packs (`Pack::open_verified`), or lazily on first use (B12 decides the default and the
  per-block hashes of large sections).

## Physical file names and installs

`address.component.release.hash8.tgd` ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)).
A new version always gets a new file. A file that may be mapped is never written over.

## Open items

- The 14 benchmarks B1–B14 (catalogue in [benchmarks](../06-engineering/benchmarks.md)).
- Payloads that follow in later phases inside the frame fixed in Phase 1 (D-071): SEARCH,
  PLACE_INDEX, POLY_INDEX, REGION and POSTAL in Phase 2; ARCS and POLYGONS with B10; DEM with B11.
- Whether the reader crate keeps `#![forbid(unsafe_code)]` with mmap outside it (B13).
