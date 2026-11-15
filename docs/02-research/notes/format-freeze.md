---
title: "The format freeze before 1.0"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# The format freeze before 1.0

## 1. Question

Can v1.0.0 freeze the pack format, setting `format_major` to 1 (Q-028)? D-029 freezes it only
once all 14 format benchmarks (B1–B14) of the
[format note](format-compression-indexing.md) are recorded. Once frozen, a 1.x reader must read
every 1.x pack, so the freeze also asks how the format can still grow without a new major.
This note feeds phase 8.4 ([plan](../../07-planning/phases/phase-8.md)).

## 2. Summary and recommendation

**Freeze at 1.0.** Twelve of the fourteen benchmarks are recorded with a decision. The other two
cannot change the bytes, so they are retired from the freeze gate with their reasons (§3.2):

- **B11 outside Nepal:** every DEM codec parameter is stored in the section.
- **B14, codec selection:** the frozen format fixes one codec per column, chosen by B1–B13.

The audit found one real hazard, and it was fixed before the freeze. Every section refused a
column it did not know, so adding an optional column in a format minor would have made every
older 1.x reader refuse the whole pack. **D-335** reserves column IDs from `0x8000` up as
extension columns that a reader skips. The audit also found that `tgd verify --deep` skipped
`TZ_RULES`; it no longer does.

After this note, the freeze sets:

- `FORMAT_MAJOR = 1` and `READER_MAJOR = 1`;
- `pack-format.md` to accepted, its codecs no longer candidates.

The fixtures and the seeds are written again. A 1.x reader still reads every format-0 pack
already published, since their `min_reader_major` is 0 and their bytes are the same layout.

## 3. Findings

### 3.1 The benchmarks, one by one

| # | Benchmark | Record | Decision |
|---|---|---|---|
| B1 | Integer blocks | [phase 1](../benches/phase-1.md#b1-integer-blocks-with-b13), on sections as well | Own FOR bit-pack in 128-value blocks (D-073) |
| B2 | Row order | [spike S1](../benches/s1-codecs.md): Hilbert 22% smaller than source order, 7% smaller than Morton | Rows in Hilbert order |
| B3 | Names | [phase 1](../benches/phase-1.md#b3-names-fsst) | FSST with per-script tables (D-077) |
| B4 | zstd decoder | [phase 1](../benches/phase-1.md#b4-blobs-zstd) | `ruzstd` for BLOBS only (D-079) |
| B5 | ID → row | [phase 1](../benches/phase-1.md#b5-id-to-row-idmap-and-redirects) | Sorted IDs and a permutation (D-075) |
| B6 | Rank/select | [phase 1](../benches/phase-1.md#b6-presence-components) | Poppy bitmap or sorted rows, the smaller (D-076) |
| B7 | Top-k completion | [phase 2](../benches/phase-2.md#b7-search) | Own FST with block maxima (D-085) |
| B8 | Place index | [phase 2](../benches/phase-2.md#b8-place_index) | Packed Hilbert R-tree, relative u16 boxes (D-082) |
| B9 | Region index | [phase 2](../benches/phase-2.md#b9-region) | Quadtree over shared arcs (D-083) |
| B10 | Polygons | [phase 3](../benches/phase-3.md#b10-poly_index-and-ring-reads) | Shared arcs, per-vertex LOD (D-088, D-097) |
| B11 | DEM at ±1 m | [phase 3](../benches/phase-3.md#b11-dem) and the [DEM note](dem-section.md) §3–4 on four Nepal tiles, Terai plain included, against LERC 4.2.0, zstd 19, JPEG-LS and B1 | LS6 + static rANS (D-089, D-104); open outside Nepal, retired below |
| B12 | Integrity | [phase 1](../benches/phase-1.md#b12-integrity-block_hashes) | Lazy BLAKE3 over 16 KiB blocks (D-078) |
| B13 | Zero-copy checks | [phase 1](../benches/phase-1.md#b1-integer-blocks-with-b13): safe code reaches 8.66 G values/s | Safe readers; `forbid(unsafe_code)` holds in the format |
| B14 | Codec selection | Not run | Retired below |

POSTAL, `TZ_RULES`, ELEVATION, `PLACE_TZ`, `ISO_CODES` and SUBSTRING came after the B list.
Each was measured where it was designed, in [phase 3](../benches/phase-3.md) and its notes.

**Platforms.** The format note asked for each benchmark on x86-64, aarch64 and WASM. Most A/B
runs were on aarch64 (M2 Pro) and were not repeated on the others. The platform changes speed,
not which codec is smaller, so the size decisions hold everywhere. Speed is gated per platform
by the permanent benchmarks ([benchmarks](../../06-engineering/benchmarks.md)), not by these
A/B runs.

### 3.2 The two retired from the gate

- **B11 outside Nepal.** The open question is whether the codec suits a desert, a coast or a
  tile above 50°.
  - The DEM section stores every parameter its decoder uses:
    - per grid, the sample spacing, the height step, the tile side (16 to 64) and the datum;
    - the model family (64 models of 64 frequencies);
    - the seven context thresholds;
    - per tile, the predictor's coefficients and the model of each context.
  - So another terrain changes the numbers the forge writes, never the layout a reader
    decodes. The 1.5″ longitude spacing above 50° is a grid's `LON_STEP`.
  - The measurement stays useful, for the size budget and the forge's thresholds. It moves to
    the data release of the first non-Nepal country
    ([rollout](../../07-planning/roadmap.md)), with the tiles named in the DEM note §6.
- **B14, codec selection.**
  - The format note suggested a builder that samples codecs per column. In the format as
    built, each section's layout fixes each column's codec, and B1–B13 picked each one. So
    there is nothing left to select within a section.
  - The builder's own choices, such as FOR, delta or ramp, and bitmap or sorted rows, are
    already made per column by size (D-075, D-076).
  - A better codec found later arrives as a new section kind, an extension column (D-335) or
    a new major. Never as a silent reinterpretation.
  - B14 measured the builder, not the bytes, so it does not gate the freeze.

### 3.3 How the format grows after the freeze

| Change | Old 1.x reader | Mechanism |
|---|---|---|
| A new section kind | Skips it | Unknown kinds are kept and skipped (`Kind`) |
| A new manifest tag | Skips it, unless its critical bit is set | TLV with `0x8000` "must understand" |
| A new optional column in a section | Skips it, any codec | **Extension columns, D-335** |
| A new codec for an existing column | Refuses the pack | New kind or new major |
| A section-level encoding other than RAW and TABLE | Refuses that section | Each section's `open` checks `Encoding::TABLE` |
| Anything a reader must understand | Refuses the pack, typed `Error::Version` | `min_reader_major` above its own |

**Before D-335, the third row was "refuses the pack".** Every one of the 17 column-table
sections refused an unknown ID. DEM's `SOURCE` column had been added that way before the
freeze, where it cost nothing. After the freeze, the same change would have broken every
older reader. COMPONENTS uses `component << 8 | field` over the whole u16, so its components
are now 1 to 127, and the writer refuses more. No build had written a component.

### 3.4 What else the audit checked

- **Every section's `open`** refuses an unknown column below `0x8000`, an unknown codec there,
  and an unknown encoding. `tests/extension.rs` plants an unknown own column in every
  column-table section of the four binding fixtures; each refuses it.
- **The deep check** (`check::sections`) covers every kind a reader has, now `TZ_RULES` too.
  The test above found the gap: `TZ_RULES` answered `Ok(false)`, "no check".
- **Version handling** (`read.rs`): a `min_reader_major` above the reader's is refused with
  `Error::Version`, and one above the pack's own `format_major` is a malformed header. The
  writer writes `min_reader_major = FORMAT_MAJOR`, so a 1.0 pack is refused by every 0.x
  reader, which is right for a format whose 0.x readers predate the freeze.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| **Freeze at 1.0, with extension columns** | 1.0's semver promise covers packs. Optional columns, kinds and tags still grow in minors | A column codec change needs a new kind or a major | **Recommended** |
| Freeze at 1.0 as it was | No code change | A minor that adds a column breaks every older 1.x reader | Rejected |
| Stay at format 0 past 1.0 | No constraint | 1.0 packs could change under a minor: the semver page could promise nothing about them | Rejected |
| Run B11 elsewhere and B14 first | Two more numbers | Neither can change the bytes (§3.2); B14 needs the world pack's build | Moved to the data releases |
| Extension columns by a flag bit per directory entry | Any ID could be optional | The entry's reserved byte would carry meaning; IDs already sort, so a range costs nothing | Rejected |

## 5. What we build ourselves, and why

- **`table::EXTENSION` and `Table::extensions`** (`teigeodata-core`): a few lines. Since IDs
  increase, the section's own columns are a prefix of the directory, and `ids()` stays an
  exact-size iterator.
- **`fuzz::bodies` and `fuzz::write_again`**: one path to rewrite a pack, shared by the
  engine's fuzz harness and the extension test.

## 6. Impact on the plan

- D-335, and `pack-format.md` (column tables, COMPONENTS).
- Phase 8.4, the format-freeze step: done with this note and the version bump.
- The DEM note §6 item 1: the non-Nepal tiles move to the first non-Nepal data release.
- `docs/06-engineering/semver.md` cites §3.3 as the format's compatibility promise.

## 7. Risks and open items

- **A column whose codec must change** (for example, a better DEM coder) costs a new kind, a
  DEM v2 beside the old one, or a major. This is accepted: the reader stays strict, and the
  old kind keeps working.
- **B11 outside Nepal:** a terrain where the codec is weak would show up as a size above
  budget, not as a broken pack. It is measured in the first non-Nepal data release.
- **B14 never measured:** a world build might show a column where another existing codec
  wins. It is then a writer change within the same layout when the codec is already allowed
  (FOR, delta and ramp are all codec 0), or a new kind.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| teigeodata format | 0.1 → 1.0 | 2026-11-15 | `crates/teigeodata-format/src/lib.rs` |
| LERC (B11 comparison) | 4.2.0 | 2026-11-08 | [DEM note](dem-section.md) |

## 9. Sources

- [The format note's A/B list](format-compression-indexing.md), B1–B14, 2026-11-06.
- The benchmark records in `docs/02-research/benches/` (phase 1, 2 and 3; spike S1), read
  2026-11-15.
- [ADR-0009, the pack format](../../03-decisions/adr/ADR-0009-pack-format.md), and D-029.
- The reader's source, `crates/teigeodata-format/src/` and `crates/teigeodata-core/src/table.rs`,
  at `455832c`, 2026-11-15.
