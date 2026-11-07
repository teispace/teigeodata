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
| 5 | SEARCH | FST over normalised keys and phonetic keys; postings; scores; top-k structure | `search` | `fst` + Elias–Fano postings + block RMQ (B7; spike S3) |
| 6 | PLACE_INDEX | packed Hilbert R-tree over points | `spatial` | flatbush-style layout with i32 boxes (B8) |
| 7 | ARCS | shared arcs, delta coordinates on 1e-7°, per-vertex LOD level | `geometry` | bit-packed deltas (B10) |
| 8 | POLYGONS | ring and part tables (signed arc refs), bbox, label point | `geometry` | — |
| 9 | POLY_INDEX | packed R-tree over polygon boxes | `geometry` | as 6 |
| 10 | REGION | region index: top grid, quadtree in Morton order, leaf table, edge-ID runs, region ID → (kind, target) | `region` | spike S2 design (below) |
| 11 | POSTAL | FST over codes → postal records | `postal` | `fst` |
| 12 | COMPONENTS | per component: presence (none, a bitmap or sorted rows) + dense field columns | `components` | column table (D-076): a bitmap costs 0.129 B per row, sparse sorted rows far less |
| 13 | DEM | tile directory + coded tiles at ±1 m | `dem` | MED prediction + entropy coder (B11) |
| 14 | BLOBS | cold data (attribution text, long strings) | `zstd` | zstd blocks with a dictionary, `ruzstd` decode (B4) |
| 15 | BLOCK_HASHES | per-block BLAKE3 for large sections (lazy verification) | `verify` | raw (D-078): the BLAKE3 chaining value of each 16 KiB block of every section of 1 MiB or more |
| 16 | REDIRECTS | retired IDs: each to its survivor, or to none for a tombstone ([ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)) | `core` | column table (D-075): sorted ID pairs |

### Column tables (encoding 1)

Every section made of integer columns (PLACES, HIERARCHY, IDMAP, REDIRECTS and COMPONENTS) is
one column table (D-074, `teigeodata_core::table`):

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
  small packs (`Pack::open_verified`, 55.6 ms per 100 MB);
- `Pack::blocks(kind)` for a section of 1 MiB or more: its block hashes proved once, then each
  16 KiB block verified on first read ([BLOCK_HASHES](#block_hashes), D-078). The engine (Phase 4)
  reads large sections this way and verifies small ones whole.

## Physical file names and installs

`address.component.release.hash8.tgd` ([ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md)).
A new version always gets a new file. A file that may be mapped is never written over.

## Open items

- The 14 benchmarks B1–B14 (catalogue in [benchmarks](../06-engineering/benchmarks.md)).
- Payloads that follow in later phases inside the frame fixed in Phase 1 (D-071): SEARCH,
  PLACE_INDEX, POLY_INDEX, REGION and POSTAL in Phase 2; ARCS and POLYGONS with B10; DEM with B11.
- Whether the reader crate keeps `#![forbid(unsafe_code)]` with mmap outside it (B13).
