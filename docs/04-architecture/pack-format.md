---
title: "The .tgd pack format v1 (draft)"
status: draft
updated: 2026-11-08
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
- **Section encodings:** 0 RAW (stored as is) and 1 TABLE (a column table, below). Both have
  `decoded_length = length`.
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
| 1 | IDMAP | each row's stable ID; ID → row | `core` | column table (D-075): sorted IDs and the row permutation; 0.35 B per place |
| 2 | PLACES | point (1e-6° grid), kind, admin level, class code, flags; rows in Hilbert order | `core` | column table (D-074): 5.2 B per place on Nepal |
| 3 | HIERARCHY | parent row; children lists (CSR) | `core` | column table (D-074): 3.0 B per place; below Elias–Fano |
| 4 | NAMES | name records (lang, script, type, validity, rule), text | `names` | column table (D-077): records + FSST text with per-script tables; 10.6 B per place on Nepal, a canonical name in 65 ns |
| 5 | SEARCH | tagged name keys (exact, loose, phonetic) → lists of rows by importance; fuzzy and top-k prefix | `search` | column table (D-085): own FST + FOR lists + block maxima; 1.50 MB on Nepal, [below](#search) |
| 6 | PLACE_INDEX | packed Hilbert R-tree whose leaves are the PLACES rows | `core`; nearest needs `spatial` | column table (D-082): u16 boxes inside the parent; 0.55 B per place at node size 16 |
| 7 | ARCS | shared arcs, delta coordinates on 1e-7°, per-vertex LOD level | `geometry` | column table (D-088, D-097): zigzag deltas per arc in B1 FOR blocks, arc bases, block checkpoints, 3-bit LOD level per vertex, [below](#arcs) |
| 8 | POLYGONS | polygons → parts → rings → signed arc refs; PLACES row, label point, area | `geometry` | column table (D-088, D-097; boxes live in POLY_INDEX), [below](#polygons) |
| 9 | POLY_INDEX | packed R-tree over polygon boxes | `geometry` | as 6, with a stored leaf level (D-088) |
| 10 | REGION | exact point → regions (timezones, later admin units): shared-arc borders, a dense top grid, quadtree leaves in Morton order with edge-ID runs, region sets | `region` | column table (D-083): [below](#region) |
| 11 | POSTAL | scheme + code → postal records (place, kind, validity, redirect) | `postal` | column table on `search::fst` (D-090) |
| 12 | COMPONENTS | per component: presence (none, a bitmap or sorted rows) + dense field columns | `components` | column table (D-076): a bitmap costs 0.129 B per row, sparse sorted rows far less |
| 13 | DEM | tile directory + coded tiles at ±1 m | `dem` | 64 × 64 tiles at ±1 m, per-tile least-squares predictor + static rANS (D-089, B11) |
| 14 | BLOBS | cold data (attribution text, long strings) | `blobs` | column table (D-079): zstd frames of whole blobs, an optional dictionary, `ruzstd` decode; 22 B per 116 B row on Nepal, a random blob in 43 µs |
| 15 | BLOCK_HASHES | per-block BLAKE3 for large sections (lazy verification) | `verify` | raw (D-078): the BLAKE3 chaining value of each 16 KiB block of every section of 1 MiB or more |
| 16 | REDIRECTS | retired IDs: each to its survivor, or to none for a tombstone ([ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)) | `core` | column table (D-075): sorted ID pairs |

### Column tables (encoding 1)

Every section made of integer columns (PLACES, PLACE_INDEX, REGION, HIERARCHY, IDMAP, REDIRECTS
and COMPONENTS) is one column table (D-074, `teigeodata_core::table`):

```text
columns u16, reserved u16 (0)
directory [{ id u16, codec u8 (0 bitpack, 1 bitmap, 2 bytes), reserved u8 (0), len u32 }; columns]
bodies    each column's bytes in directory order, back to back
```

- IDs are strictly increasing; every length is a multiple of 4, so every column starts 4-byte
  aligned; the lengths add up to the rest of the section exactly.
- Codec 0 is the B1 integer codec (D-073, `teigeodata_core::bitpack`): blocks of 128 values in
  4 interleaved lanes, one base and bit width per block, FOR or zigzag delta per column:

  ```text
  count u32, lanes u8 (4), coding u8 (0 FOR, 1 delta, 2 ramp), reserved u16 (0)
  bases   [u32; blocks]
  offsets [u32; blocks]   word offset of each block in the payload
  widths  [u8; blocks], zero padding to a multiple of 4
  payload [u32]           block after block, w × 4 words each
  ```

  Value `j` of a block is in lane `j % 4`, row `j / 4`. FOR: `base + packed`. Delta: `base` is
  the block's first value and the packed values are zigzag deltas from the previous one. Ramp:
  `base + j + packed` (D-075). The writer takes the smaller of FOR and ramp (both O(1) to
  read), and delta only when it is at least 10% smaller than that.
- Codec 1 is a bitmap with a rank directory (D-076, `teigeodata_core::bits`), after poppy:

  ```text
  len u32 (bits), reserved u32 (0)
  words     [u64; ceil(len / 64)]     bit i is bit i % 64 of word i / 64; bits past len are 0
  directory [u64; ceil(len / 2048)]   ones before the block (low 32 bits), then the ones of
                                      its first three 512-bit sub-blocks (10 bits each)
  ```

  Overhead 3.125%; a rank reads one entry and popcounts at most eight words.
- Codec 2 is raw bytes whose meaning the section defines, zero-padded to a multiple of 4
  (D-077): NAMES' symbol tables and text.
- A FOR block's base and width bound its values (`base` to `base + 2^w − 1`), which serves as
  a zone map for free.
- Opening a section checks the frame and every column's structure (O(blocks)); a deep check of
  the values is the forge's and the tests' (O(rows)). Reads never panic either way.

### PLACES

The section's `item_count` is the number of places; every column has one value per place.
Columns not listed are refused, so a new column comes with a format minor version.

| ID | Column | Values | |
|---:|---|---|---|
| 1 | lat | latitude on the 1e-6° grid, biased to u32 (`i32::MIN` → 0) | required |
| 2 | lon | longitude, the same | required |
| 3 | kind | kind code: 1 country, 2 admin, 3 settlement, 4 locality, 5 natural, 6 postal area, 7 timezone area ([schema](../05-data/schema.md#kind)) | required |
| 4 | admin level | 0 for a country or a place that is not an admin unit | optional |
| 5 | class | class dictionary code, 0 = none | optional |
| 6 | flags | none defined yet | optional |

An optional column is left out when every value is 0, and reads as 0. The deep check also
checks that rows are in Hilbert order of their points, that every point is on the grid and
every kind known, and that only admin units have a level.

### SEARCH

Name keys to places, for exact, prefix and fuzzy search (D-085, D-086,
[ADR-0032](../03-decisions/adr/ADR-0032-text-search.md),
[research](../02-research/notes/search-index.md), measured in
[B7](../02-research/benches/phase-2.md#b7-search)). Code: `teigeodata_format::search`.
`item_count` is the number of rows (places); a listed row is below it.

| ID | Column | Values |
|---:|---|---|
| 1 | LENGTH | one value: the automaton's length in bytes |
| 2 | AUTOMATON | raw bytes: the automaton, zero-padded to a multiple of 4 |
| 3 | STARTS | per key and one more: its list's first entry in IDS, from 0, strictly increasing |
| 4 | IDS | every list in key order: rows by importance descending, then row ascending |
| 5 | IMPORTANCE | per row: 0 to 255 |
| 6 | BLOCK_MAX | per 128 keys in ordinal order: the highest importance in their lists |

A column ID of 0 or above 6 is refused.

- **Keys** are a tag byte, then the folded name in UTF-8: 1 exact, 2 loose, 3 phonetic. The
  forge defines each fold. A key is at most 255 bytes. A key's ordinal is its position in
  sorted byte order across all maps, so one map's keys are one ordinal range.
- **The automaton** is an acyclic FST whose path outputs sum to the ordinal, minimal up to the
  writer's registry (below). Nodes are
  written children first and read backwards from their address (the offset of their last
  byte); the root is the last byte, and a transition's target is `address − delta` with
  `delta ≥ 1`, so every walk ends. Four forms, little-endian:

  ```text
  leaf     header: bit 7 final, bits 0–6 zero
  next     label, header (bit 7 final, bit 6 set): one transition to address − 2
  one      delta [wt bytes], label, header (bit 7 final, bit 5 set, bits 0–1 wt − 1)
  general  outputs [(n − 1) × wo], deltas [n × wt], labels [n] strictly increasing,
           widths (wt 1–4 in bits 0–3, wo 0–4 in bits 4–7), [n − 1 if n ≥ 31],
           header (bits 0–4 n from 2 to 30, or 31 for the escape)
  ```

  A transition's output is the number of keys before it in its node (1 if the node is final,
  plus the keys under earlier transitions); the first is implied by the final bit. The writer
  is canonical: the next form whenever it applies, else the one form for one transition, and
  the smallest widths.
- **Queries:** `get(tag, key)` and `range(tag, prefix)` give ordinals; `rows_of` gives a key's
  rows, most important first; `best_in(range, floor, visit)` visits only keys and blocks whose
  maximum beats a rising floor (top-k prefix); `walk(tag, matcher, anchor, budget, visit)`
  visits in key order the keys a matcher accepts, with at most `budget` transitions. The
  matcher in `search::fuzzy` is a bit-parallel OSA NFA over code points: up to 2 edits and 63
  characters. Queries never allocate or panic, on any bytes.
- **Open** checks the directory, the column lengths against the key and row counts, the padding
  and the root node in O(1). **`check`** walks the whole automaton with a fixed stack: every
  node canonical and inside the bytes, labels increasing, each key's ordinal the running count,
  every transition leading to a key (so work is bounded by keys × 255), no key longer than 255
  bytes, the key count exact. It then checks every list start, list order, row bound,
  importance and block maximum: O(automaton + entries). A well-formed node that no transition
  reaches is not detected; it changes no answer and the signature guards the bytes.
- **The writer** (`search::encode`, feature `write`) takes the sorted map of tagged keys to row
  sets and every row's importance, refuses an empty key or list, a key over 255 bytes and a row
  past the end, and is deterministic. Its node registry is a bounded direct-mapped table of
  2^18 slots, so a missed share costs size, never memory.

### PLACE_INDEX

A packed Hilbert R-tree over the PLACES points (D-081, D-082,
[indexes note](../02-research/notes/indexes.md#31-place_index-over-the-existing-row-order)).
PLACES rows are already in Hilbert order, so they are the tree's leaves and only the boxes
above them are stored. `item_count` equals PLACES'. Columns other than these three are refused.

| ID | Column | Values |
|---:|---|---|
| 1 | node size | one value: children per box, 4 to 32 (`MIN_NODE`, `MAX_NODE`); 16 by default |
| 2 | root | four values: the south, north, west and east edge of the root box on the 1e-6° grid, biased as PLACES' lat; exactly the bounds of the rows; empty when there are no places |
| 3 | boxes | four values per box, for every box but the root, level 0 first: its south, north, west and east edge as offsets 0 to 65,535 (`QUANTA`) across its parent's decoded box |

- **Levels:** with `n` rows and node size `m`, level 0 has ⌈n / m⌉ boxes (box `i` covers rows
  `i·m` to `i·m + m − 1`), and each level above has ⌈below / m⌉, up to a level of one box, the
  root. 1 to `m` rows give the root alone and an empty boxes column; 0 rows give no levels. At
  most 16 levels (4¹⁶ = 2³² rows).
- **Offsets:** for a parent edge range `[p_lo, p_hi]` of span `s = p_hi − p_lo`, a child's
  exact range `[lo, hi]` is stored as `⌊(lo − p_lo) · 65535 / s⌋` and `⌈(hi − p_lo) · 65535 / s⌉`
  (0 and 0 when `s = 0`), and decodes to `p_lo + ⌊q_lo · s / 65535⌋` and
  `p_lo + ⌈q_hi · s / 65535⌉`. Rounding outward makes every decoded box contain the rows below
  it. A child is quantised against its parent's decoded box, not its exact one, so encoding runs
  top down. The B1 codec packs the offsets: 0.55 B per place at node size 16.
- **Opening** (`PlaceIndex::open`) refuses an item count other than PLACES', a node size column
  without exactly one value in range, a tree deeper than 16 levels, a root column without four
  values (zero for no places), a boxes column without four values per stored box, and an
  unknown column.
- **The deep check** (`PlaceIndex::check`) walks the tree once in row order, O(rows) with no
  allocation: every box has children, the root is exactly the rows' bounds, and every stored
  offset equals the one the writer derives from the exact bounds below it and its parent's
  decoded box. Offsets are compared, not decoded boxes: on a narrow parent several offsets
  decode to the same edge and only one is canonical. A checked index is canonical.
- **Box query** (`within`): every row whose point is in a box, edges included, in no set
  order, stopping when the caller breaks. A box with west above east crosses the antimeridian
  and is searched as two, `[west, 180°]` and `[−180°, east]`; south above north matches
  nothing. A tree box wholly inside the query yields its rows without testing them. Depth-first
  on fixed buffers, no allocation.
- **Nearest** (`nearest`, feature `spatial`): the `k` rows nearest a point by great-circle
  distance on a sphere of radius 6,371,008.8 m (`EARTH_RADIUS_M`, the IUGG mean), nearest first,
  ties by row; fewer only when the pack has fewer places. Each result carries the haversine of
  the central angle, compared exactly; `Neighbour::meters` turns it into metres. Distances use
  `libm`, so every platform gives the same bits. The search is best-first on a fixed heap
  of 409 boxes (16 KiB), falling back to a depth-first walk of a subtree when the heap is full;
  boxes are pruned by integer latitude and longitude gates and a trigonometry-free lower bound
  (D-082).
- **On an index that fails the check,** queries still never panic or allocate and only return
  rows below the row count, but their answers are not guaranteed; the forge and the tests run
  the check.

### ARCS

Every polygon border stored once, as arcs between topology nodes (D-088, D-097,
[geometry note](../02-research/notes/geometry-topology.md)). Code: `teigeodata_format::arcs`.
The item count is the number of arcs; vertices are numbered across all arcs.

| ID | Column | Values | |
|---:|---|---|---|
| 1 | START | arcs + 1 values: each arc's first vertex, then the vertex count | required |
| 2, 3 | BASE_X, BASE_Y | per arc: its first vertex, biased | required |
| 4, 5 | DX, DY | per vertex: zigzag(Δ ÷ 10^scale) from the previous vertex of the arc; 0 at an arc's first vertex | required, FOR |
| 6, 7 | CHK_X, CHK_Y | per 128-vertex block: the absolute point of its first vertex | required |
| 8 | LEVEL | per vertex: the coarsest level of detail that keeps it, 0 to 7; 7 = pinned (arc ends, a closed arc's anchor) | optional |
| 9 | TOLERANCE | 6 values: the Douglas–Peucker tolerance of levels 1 to 6 in cm, increasing | with LEVEL |
| 10 | SCALE | per arc: `k`, its deltas stored ÷ 10^k, the largest power of ten dividing them all | optional |

- **Reads.** A vertex is one block decode away: a piece of an arc starts from the arc's base or
  from its block's checkpoint, so an arc reads backwards (a ring that walks it reversed) at the
  same cost as forwards. The reader yields grid points through an iterator holding one decoded
  block (about 1.2 KB) and never allocates.
- **Level of detail `l`** keeps the vertices whose level is at least `l`. Arc ends are pinned, so
  neighbours stay coincident at every level.
- **Rules** (`Arcs::check`, O(vertices)): arcs of at least 2 vertices; no repeated consecutive
  vertex; every point on the grid; checkpoints equal to the decoded points; ends pinned; a closed
  arc has at least 4 vertices and, with LEVEL, a pinned vertex inside, so no ring falls below a
  triangle; tolerances positive and increasing; scales up to 7. The checkpoints are required
  (the note had them optional): they cost about 0.05 B per vertex and make reversed reads and
  REGION's shared use one code path.

### POLYGONS

Every polygon as parts, rings and signed references to ARCS (D-088, D-097). Code:
`teigeodata_format::polygons`. The item count is the number of polygons.

| ID | Column | Values | |
|---:|---|---|---|
| 1 | ROW | per polygon: its PLACES row, increasing within a group | required |
| 2 | PARTS | polygons + 1: each polygon's first part | required |
| 3 | RINGS | parts + 1: each part's first ring, its outer ring | required |
| 4 | REFS_START | rings + 1: each ring's first reference | required |
| 5 | REFS | per reference: `2·arc + reversed` | required, FOR |
| 6, 7 | LABEL_X, LABEL_Y | per polygon: a point inside it (polylabel), biased | required |
| 8 | AREA | per polygon: ellipsoidal area in hectares, rounded | required |
| 9 | GROUPS | groups + 1: each group's first polygon (admin levels, coarse to fine) | optional, absent = one group |

- **Rings** follow `TopoJSON`: each arc after the first drops its first vertex, and the last arc
  ends where the first began. Outer rings run counter-clockwise, holes clockwise (RFC 7946).
  `Polygons::ring(arcs, ring, lod)` yields a closed ring at any level of detail without
  allocating.
- **Row → polygon** is a binary search in each group.
- **`contains`** is exact with REGION's border rule (a point is read as `p + (ε, ε²)`); it is
  REGION's test oracle and the fallback of a pack without REGION, not the point → admin path.
- **Rules** (`Polygons::check(arcs, places)`): offsets increasing; rows below the PLACES count,
  increasing within a group and unique; references to existing arcs that join end to start and
  close; rings of at least 4 vertices and non-zero area, outer rings counter-clockwise and holes
  clockwise; each label inside its polygon. Not checked: that a hole lies inside its outer ring
  and that rings do not cross; the forge's topology stage guarantees both.

### REGION

Which regions (timezones now, admin units later) hold a point, exactly against the source
quantised once to the 1e-7° grid (D-083, [ADR-0013](../03-decisions/adr/ADR-0013-region-index.md),
[indexes note](../02-research/notes/indexes.md#32-region), method proven in
[spike S2](../02-research/benches/s2-region-index.md), measured in
[B9](../02-research/benches/phase-2.md#b9-region)). Code: `teigeodata_format::region`.
`item_count` is the number of regions; a region ID is below it. Edges are straight in
lon/lat. A column ID of 0 or above 16 is refused.

- **Domain and grid:** the closed world rectangle, longitude −1,800,000,000 to 1,800,000,000
  and latitude −900,000,000 to 900,000,000 in 1e-7° units. A quadtree over it goes down to
  level 24 (`LEVELS`). At level `l`, cell edge `i` across an axis with minimum `min` and span
  `span` is `min + ⌊i · span / 2^l⌋`, so a cell at level 24 is 214 or 215 units wide and 107 or
  108 high. A cell includes its edges when edges are tested against it. A point belongs to the
  level-24 column (row) of the last edge at or before it, clamped to the last column (row): a
  point on a shared cell edge belongs to the cell east (north) of it.
- **Morton keys:** a cell's key is its level-24 column and row interleaved, column bits in the
  even positions (48 bits). A leaf at level `l` starts at a key that is a multiple of
  `4^(24 − l)`. The top grid is level `g` (`MIN_TOP` 8 to `MAX_TOP` 12), and top cell
  `key >> 2(24 − g)` is in Morton order too.

| ID | Column | Values |
|---:|---|---|
| 1 | TOP | one value: the top-grid level `g` |
| 2 | DIR | 4^g + 1 values: the number of leaves that start before each top cell, then the leaf count |
| 3 | KEY | per leaf: its key minus its top cell's first key (below 2³² because `g` ≥ 8) |
| 4 | LEVEL | per leaf: its level, up to 24 |
| 5 | ZONES | per leaf: the region set at its reference point (the whole leaf's set if it has no edges) |
| 6 | NUDGE | optional, per leaf: which candidate is its reference point; absent reads as 0, and the writer leaves it out when every value is 0 |
| 7 | RUNS | per leaf and one more: its first run |
| 8 | RUN_EDGE | per run: its first edge |
| 9 | RUN_LEN | per run: its number of edges, at least 1 |
| 10 | RUN_ZONES | per run: the region set its edges toggle |
| 11 | X | per vertex: longitude in 1e-7° units, biased (`teigeodata_core::bitpack::bias`) |
| 12 | Y | per vertex: latitude, biased |
| 13 | SET_START | per region set and one more: its first entry in SET_ZONES, from 0 |
| 14 | SET_ZONES | the region IDs of every set, each set strictly increasing, at most 64 (`MAX_SET`) |
| 15 | TOP_ZONES | per top cell: 1 + the region set of the edge-free leaf that holds the whole cell, or 0 |
| 16 | PLACE | optional, per region: the PLACES row of its place (a timezone area, later an admin unit), added by `region::link` (D-093) |

- **Region sets:** set `z` is the single region `z` for every region ID `z`, so a leaf or run of
  one region is read without the set tables. Sets of no region or of several follow, numbered
  by the writer's first use. Source regions overlap, so a leaf holds a set, not one ID.
- **Arcs:** each border is stored once, with every region it separates in one set. X and Y hold
  arcs back to back; edge `e` joins vertices `e` and `e + 1`, and the last vertex of an arc
  starts no edge. A leaf lists the edges that touch its closed cell as runs of consecutive edge
  numbers; the edges of a run lie on one arc, so they share its set. REGION keeps its own
  vertices and does not refer to ARCS (row 7).
- **Reference point:** candidate `k` of a cell `[x0, x1] × [y0, y1]`, with `w = x1 − x0`,
  `h = y1 − y0`, `hx = max(1, ⌊w/2⌋)` and `hy = max(1, ⌊h/2⌋)`, is
  `(x0 + ⌊w/2⌋ + (7 + 31k) mod hx, y0 + ⌊h/2⌋ + (13 + 17k) mod hy)`, strictly inside the cell.
  The writer takes the first `k` below 4,096 whose point lies on the line of no edge of the
  parent cell (all edges, for the root), and stores it in NUDGE. A leaf without edges stores 0.
- **Query** (`Region::regions_at`, and `Region::region` for the single answer):
  1. Normalise: longitude 180° is read as −180° (the same meridian), latitude 90° as
     899,999,999 units.
  2. Find the point's key and top cell `c`. If `TOP_ZONES[c]` is not 0, the answer is set
     `TOP_ZONES[c] − 1`: one read.
  3. Otherwise binary-search KEY between `DIR[c]` and `DIR[c + 1]` for the last leaf starting
     at or before the point; with none, the leaf is `DIR[c] − 1`, a leaf coarser than the top
     grid that started in an earlier top cell.
  4. Start from the leaf's ZONES set. For each of its runs, count the edges crossed by the
     segment from the leaf's reference point to the query point; an odd count toggles the
     run's set. Vertices are decoded 128 at a time into a fixed buffer.
  5. Return every region, in increasing ID order. A query never panics or allocates; it keeps
     at most 64 regions.
- **Points on a border:** the query point `p` is read as `p + (ε, ε²)` for an infinitesimal ε
  (simulation of simplicity), so it is never on an edge, and the reference point is on no
  edge's line. Every predicate is exact `i128` arithmetic. A point on a border belongs to the
  regions just east-north-east of it: east of a border that runs north–south, north of
  one that runs east–west. The answer equals an even–odd
  ray cast to the east that counts an edge when its ends lie on opposite sides of the point's
  latitude (an end exactly on it counts as south) and it crosses strictly east of the point;
  `region::even_odd` is that ray cast over the input rings, the oracle of the tests.
- **Precedence:** region IDs are in precedence order. Where regions overlap, the lowest ID is
  the single answer, and the forge orders the IDs (Q-013).
- **Opening** (`Region::open`) refuses a missing required column, a top level outside 8 to 12,
  a column of the wrong length (DIR 4^g + 1; KEY, LEVEL, ZONES and NUDGE one per leaf; RUNS one
  more; the run columns one per run; Y as long as X; TOP_ZONES 4^g; SET_ZONES the last
  SET_START), and a column ID of 0 or above 15.
- **The deep check** (`Region::check`), one pass over each column:
  - sets: starts increase from 0, each set has at most 64 members in strictly increasing order
    below the item count, and set `z` is `{z}`;
  - leaves: the directory counts the leaves in order, levels are at most 24, and the keys tile
    the world exactly in Morton order, each aligned to its level; every set reference is in
    range;
  - a nonzero TOP_ZONES entry has a holding leaf no deeper than the top grid whose edges all
    miss the closed top cell;
  - runs: in range, non-empty, ending inside the vertices, in increasing order within a leaf
    and never adjacent (the writer's runs are maximal); every edge has two distinct ends,
    touches its leaf's closed cell, and its line misses the leaf's reference point; NUDGE is
    below 4,096, and 0 for a leaf without edges.

  It does **not** re-derive the region sets (ZONES, RUN_ZONES, TOP_ZONES) from the geometry,
  nor that NUDGE is the writer's first good candidate, that NUDGE is left out when every value
  is 0, or that runs are split where one arc ends and the next begins, so a pack that passes can still answer
  wrongly if it was edited. The pack signature guards those; answers are exact on a pack built
  by `region::encode`.
- **The writer** (`region::encode`, feature `write`) takes each region's rings (outer rings and
  holes alike, since membership is even–odd), as `[lon, lat]` on the 1e-7° grid, closed or not,
  and refuses a vertex outside the domain. It keeps every distinct edge with the regions that
  use it an odd number of times (a ring given twice cancels), joins edges into arcs through
  vertices where exactly two edges of one set meet, and orders arcs by the key of their first
  vertex. It finds the root's sets by an east ray cast, then splits top down: a cell with more
  than `leaf_max` edges splits, down to level 24, and a child's set is its parent's toggled by
  the edges crossed between their reference points. The writer is deterministic.
  - **Options:** `leaf_max` (at least 1, 32 by default) and `top` (8 to 12, 10 by default).
  - **Stuck rule:** a cell with at most 16 edges stays a leaf when one child would keep all of
    them while another child has edges too, so edges that meet at a point or overlap do not
    split down to level 24.
  - **Refusals:** a leaf whose set and run sets together hold more than 64 regions (`MAX_SET`),
    and a cell with no good reference point among the 4,096 candidates.

### HIERARCHY

`item_count` equals PLACES'. Three required columns:

| ID | Column | Values |
|---:|---|---|
| 1 | parent | parent row + 1; 0 for a root |
| 2 | child end | for each row, the running end of its children in column 3 |
| 3 | children | child rows, grouped by parent row, ascending within a group |

The children of row `r` are `children[end[r − 1] .. end[r]]`, from 0 for row 0. A chain of
parents is at most 32 long (`MAX_DEPTH`), which bounds every walk up the tree. The deep check
proves that the columns describe one forest: each row's children are exactly the rows that
name it as parent, and no chain is a cycle.

### IDMAP and REDIRECTS

An ID is a u64 and the codec is u32, so each ID column is a pair: the high and the low 32 bits.
IDMAP's `item_count` equals PLACES'; all five columns have one value per row:

| ID | Column | Values |
|---:|---|---|
| 1, 2 | ID high, low | each row's ID |
| 3, 4 | sorted high, low | the IDs in increasing order |
| 5 | order | the row of each sorted ID |

REDIRECTS has one value per retired ID, in increasing order of the retired ID:

| ID | Column | Values |
|---:|---|---|
| 1, 2 | old high, low | the retired ID |
| 3, 4 | new high, low | its survivor, a current place of the pack; 0 and 0 for a tombstone |

- **Lookup:** interpolation, then binary search, over the sorted column's block firsts (O(1)
  each); one block unpacked and searched; one read of `order`; and a check that the row holds
  the ID, so even a pack that skipped the deep check never answers with the wrong place. A miss
  in IDMAP looks in REDIRECTS (one hop: chains are flattened).
- **Deep checks:** sorted IDs strictly increasing and each at the row `order` names (which
  makes `order` a permutation); every ID valid; retired IDs not current; survivors current.

### NAMES

`item_count` equals PLACES'. A place's records are adjacent, its canonical name (the schema's
required `name`) first, so a display name is one O(1) read and one decode.

| ID | Column | Values | |
|---:|---|---|---|
| 1 | first | per place and one more: its first record; at least one record per place | required, O(1) coding |
| 2 | text end | per record: where its codes end in the heap (they start where the previous ones end) | required, O(1) coding |
| 3 | table | per record: its symbol table | required |
| 4 | lang | BCP 47 language, up to 4 ASCII letters packed little-endian into a u32; 0 = undetermined | optional |
| 5 | script | ISO 15924 script, packed the same way; 0 = unknown | optional |
| 6 | type | 1 official, 2 local, 3 alt, 4 colloquial, 5 historical, 6 former official; 0 = unspecified | optional |
| 7, 8 | valid from, to | a biased day since 1970-01-01; 0 = open | optional |
| 9 | rule | the precedence step behind a canonical name; 0 = none | optional |
| 10 | tables | codec 2: count u32 (1 to 16), then each FSST symbol table | required |
| 11 | heap | codec 2: every record's codes, back to back, zero-padded to a multiple of 4 | required |

- **FSST** (`teigeodata_core::fsst`): a table of up to 255 symbols of 1 to 8 bytes; code 255
  escapes one literal byte; a record is at most 255 codes, so a 2,040-byte buffer holds any
  name. Table layout:

  ```text
  symbols u8, reserved [u8; 7] (0)
  lens    [u8; symbols], zero padding to a multiple of 8
  bytes   [[u8; 8]; symbols]   zero past each symbol's length
  ```
- **Tables:** table 0 is trained on every text; each script with at least 16 KiB of text and
  less than half of all text gets its own (largest first, up to 15 more). Each record takes the
  table that codes it shorter. Training is deterministic.
- **Text end and first** always take an O(1) coding (FOR or ramp): delta saved 6% on Nepal but
  added 50 ns to every read.
- **Deep check:** places start at record 0, each has records, the last ends at the last record;
  text spans increase and stay in the heap, whose padding is zero and under 4 bytes; every text
  decodes to UTF-8 with a valid table; tags and types are valid.

### COMPONENTS

Optional per-place fields, grouped into the schema's
[components](../05-data/schema.md#components). `item_count` equals PLACES'. Column IDs are
`component << 8 | field`; component 0 is reserved.

| Field | Column | |
|---:|---|---|
| 0 | presence: which rows have the component | absent: every row; codec 1: a bitmap of one bit per row; codec 0: the present rows, strictly increasing |
| 1–255 | a field: one value per present row, in row order | codec 0 |

- **The writer picks the smallest presence form** per component. B6 measured the crossover
  near 10% of rows: below it the sorted rows win (70× at 0.1%), above it the bitmap does.
- **Lookup:** a row's value is at its index among the present rows: its rank in the bitmap
  (O(1)), its position in the sorted rows (the IDMAP search), or the row itself.
- **Opening** checks every column's codec, structure and length (a field has as many values as
  the component has present rows); **the deep check** proves each bitmap's directory and that
  sorted rows increase and stay below the row count.
- What each component and field number means is assigned with the forge (Phase 3), from the
  schema.

### BLOBS

Cold byte strings: attribution text, licences, long descriptions (D-079,
[names storage](../02-research/notes/names-storage.md)). `item_count` is the number of blobs.

| ID | Column | |
|---:|---|---|
| 1 | frame ends in the heap | required |
| 2 | each frame's decoded end in the blobs' concatenation | required |
| 3 | each blob's end in the concatenation | required |
| 4 | the dictionary's end (one value): the zstd dictionary is the heap's start | optional |
| 5 | heap: the dictionary, then the zstd frames, padded to 4 | required |

- **Frames hold whole blobs**, are non-empty and decode to at most 8 MiB; a blob larger than the
  frame target gets a frame of its own. With a dictionary, every frame uses it, and frames carry
  no dictionary ID or checksum (the section hash covers them).
- **Frame size is the forge's choice:** 16 KiB by default (B4: 0.19 of raw, a random blob in
  43 µs); 4 KiB with a trained dictionary where blobs are read per result (0.21, 13 µs).
- **The forge compresses** with C zstd at level 19; the writer (`blobs::encode`) reads every blob
  back through the reader before it accepts the section.
- **Reading** (feature `zstd`, `ruzstd`) finds a blob's frame by binary search and decodes it,
  keeping the last frame. Output is bounded by the frame's stated length, so a crafted frame
  cannot expand past it; the window is capped at 8 MiB.
- **Opening** checks the columns' structure and the dictionary's span; **the deep check** checks
  every span, and `Decoder::check` decodes every frame.

### BLOCK_HASHES

Lazy verification of large sections (D-078, [note](../02-research/notes/block-hashes.md)). Every
section of **1 MiB or more** (other than this one) has an entry, and no other section does: the
coverage is canonical, checked on open, and the writer refuses a pack without it. `item_count` is
the number of chaining values.

| Part | Bytes | Content |
|---|---:|---|
| header | 16 | `block_log2` u8 (14: 16 KiB), 3 reserved zero bytes, `entries` u32, `cvs` u64 |
| directory | 16 per entry | `kind` u32, reserved zero u32, `first` u64 (the index of the section's first CV); kinds strictly increasing |
| chaining values | 32 per block | the BLAKE3 chaining value of each 16 KiB block, the last one short |

- **The list needs no trust root of its own:** the CVs of a section are exactly the leaves of
  its BLAKE3 tree at 16-chunk granularity, so merging them in tree order (`blake3::hazmat`)
  gives the section hash in the table, which the head digest covers. `Pack::blocks(kind)` proves
  a section's slice once (78 ns per block, 237 µs for 50 MB); `Blocks::verify(i)` then hashes one block on its
  first read (8.8 µs), and `Blocks::bytes(range)` verifies the blocks a range touches.
- **Memo:** `Blocks::with_memo` takes a caller's slice of `AtomicUsize` (`memo_words` long) as a
  bitmap of verified blocks, so a block is hashed once; the reader allocates nothing.
- **Opening** checks the header, the directory and the coverage against the section table;
  the writer proves every entry's content before it assembles.

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
  small packs (`Pack::open_verified`, 55.6 ms per 100 MB);
- `Pack::blocks(kind)` for a section of 1 MiB or more: its block hashes proved once, then each
  16 KiB block verified on first read ([BLOCK_HASHES](#block_hashes), D-078). The engine (Phase 4)
  reads large sections this way and verifies small ones whole.

## Physical file names and installs

`address.component.release.hash8.tgd` ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)).
A new version always gets a new file. A file that may be mapped is never written over.

## Open items

- The 14 benchmarks B1–B14 (catalogue in [benchmarks](../06-engineering/benchmarks.md)).
- Payloads that follow in later phases inside the frame fixed in Phase 1 (D-071): POSTAL in
  Phase 3 (D-087); ARCS, POLYGONS and POLY_INDEX with B10 (D-083); DEM with B11.
- Whether the reader crate keeps `#![forbid(unsafe_code)]` with mmap outside it (B13).
