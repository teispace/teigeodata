---
title: "Phase 1 benchmarks: core types and the container"
status: research
updated: 2026-11-07
---

# Phase 1 benchmarks: core types and the container

Measured with `crates/teigeodata-bench` (criterion 0.8.2):

```
cargo bench -p teigeodata-bench --bench core --bench format -- --warm-up-time 1 --measurement-time 3
```

The workloads come from fixed seeds (`teigeodata_bench::{world_points, pack_of_size}`), so every
harness and, later, every binding runs the same inputs. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md).

**Machine:** M2 Pro, 16 GB, macOS, rustc 1.99.1, release profile, single thread, 2026-11-07.
The machine was shared: load averages were 5 to 14 during the runs. That is above the catalogue's
local limit of 2, so these are indicative numbers. The PR gate will use gungraun's instruction
counts in CI, which do not depend on load.

## Core types

| Case | Time | Notes |
|---|---:|---|
| Hilbert index, prefix-scan version | 9.4 ns per point (106 M/s) | 100,000 world points, 1e-7° grid |
| Hilbert index, classic loop (the definition) | 44.9 ns per point (22 M/s) | the same points |
| `PlaceId` to text, 1 / 3 / 10 base32 digits | 9.1 / 11.2 / 17.9 ns | no allocation |
| `PlaceId` from text, 1 / 3 / 10 digits | 5.9 / 8.7 / 21.2 ns | |

The prefix-scan Hilbert index is **4.75× faster** than the loop it is proven equal to.

## Container (L1 to L3)

Packs of 1, 10 and 100 MB: three sections of pseudo-random bytes, a manifest and a signature.

| ID | Case | 1 MB | 10 MB | 100 MB |
|---|---|---:|---:|---:|
| L1 | `Pack::open`: every structural check and the head digest | 0.71 µs | 0.70 µs | 0.71 µs |
| L2 | `verify_sections`: BLAKE3 of every section, eager | 0.55 ms (1.76 GiB/s) | 5.5 ms (1.76 GiB/s) | 55.5 ms (1.76 GiB/s) |
| L3 | `verify_signature` (Ed25519 `verify_strict`) | 34.4 µs | | |

**Findings:**
- **Opening costs the same at every size,** because `open` reads only the head (at most 16 KiB).
  A 100 MB pack is usable after 0.7 µs. Hashing is what scales with size: eager hashing of
  100 MB takes 55 ms on one core. Large sections are verified per block instead
  ([B12](#b12-integrity-block_hashes)): 0.24 ms to prove the block hashes, then 8.8 µs per block
  on its first read.
- **BLAKE3 already uses NEON:** enabling the `neon` feature changed nothing (p = 0.90). The
  runtime keeps `default-features = false`.
- **Ed25519 precomputed tables** (the `fast` feature) verify in 31.9 µs, against 34.4 µs without
  them. That saves 2.5 µs once per pack open, at the cost of larger tables in the library. The
  runtime stays without `fast`; Z2 measures the size. The forge may enable it for signing.
- **1 GB packs were not run:** a pack of that size is built in memory here. The memory-mapped
  L1 case runs when the engine maps files (Phase 4).

## B1: integer blocks (with B13)

```
cargo bench -p teigeodata-bench --bench b1 -- --warm-up-time 1 --measurement-time 4
cargo run --release -p teigeodata-bench --example b1-sizes
```

**Input:** the four integer columns of the 148,210 places in `GeoNames` Nepal, rows in Hilbert
order (S1's input): latitude and longitude on the 1e-6° grid, elevation, and source IDs sorted
(an ID map's shape). **Rivals:** LEB128 zigzag deltas (S1's baseline) and the `bitpacking`
crate 0.9.3 (`BitPacker4x`: hand-written SIMD in `unsafe` code, with a FOR base per block added).
Load average 1.5–1.8 during the run, so these numbers count.

**Size, bytes per value** (everything included: header, bases, offsets, widths):

| Column | FOR, 128 | Delta, 128 | FOR, 1024 | Delta, 1024 | LEB128 delta | `bitpacking` 4x |
|---|---:|---:|---:|---:|---:|---:|
| lat | 2.221 | **2.085** | 2.365 | 2.140 | 2.172 | 2.190 |
| lon | 2.224 | **2.095** | 2.379 | 2.169 | 2.175 | 2.193 |
| elevation | 1.314 | 1.310 | 1.386 | 1.366 | 1.508 | **1.283** |
| ids (sorted) | 1.053 | **0.530** | 1.496 | 0.723 | 1.005 | 1.022 |

**Speed, 128-value blocks** (decode all into a buffer; random `get` of 4,096 seeded indices):

| Column | FOR decode | Delta decode | LEB128 decode | `bitpacking` 4x decode | FOR `get` | Delta `get` |
|---|---:|---:|---:|---:|---:|---:|
| lat | **8.66 G/s** | 2.86 G/s | 0.46 G/s | 6.86 G/s | 5.0 ns | 23 ns |
| lon | **8.49 G/s** | 2.84 G/s | 0.45 G/s | 7.11 G/s | 4.9 ns | 23 ns |
| ids | **10.96 G/s** | 2.89 G/s | 1.40 G/s | 7.63 G/s | 4.0 ns | 22 ns |

**How the decoder got there,** each step measured on latitude:

| Step | FOR decode | Delta decode |
|---|---:|---:|
| A loop over rows, width at run time | 2.08 G/s | 1.48 G/s |
| Width a constant per block (one `match`), loop kept | 2.1 G/s (no change) | |
| Branch-free row extraction | −34% (rejected) | |
| The 32 rows written out (a macro), base added while unpacking | 4.5 G/s | 1.7 G/s |
| The words of a block as a fixed array of exactly `W` rows, so no bounds check remains (B13) | **8.66 G/s** | |
| Delta prefix sum row by row: one carry addition per row of 4 on the chain, not one per value | | **2.86 G/s** |
| Delta `get`: unpack with the vector code, then sum (associative) instead of a scalar walk | | 136 → 23 ns |

**WebAssembly** (the same latitude column, decoder built for `wasm32-unknown-unknown` with LTO
and run in Node.js 26, V8):

| Build | FOR decode | Delta decode | FOR `get` | Delta `get` |
|---|---:|---:|---:|---:|
| scalar | 3.42 G/s | 1.76 G/s | 9.6 ns | 69 ns |
| `+simd128` | **3.98 G/s** | 1.96 G/s | 9.9 ns | 36 ns |

**Against the targets** of the [format note](../notes/format-compression-indexing.md):
decode ≥ 2 G/s native (8.5–11) and ≥ 0.5 G/s in WASM (3.4–4.0); random access ≤ 5 ns
native (4.0–5.0); size within 5% of `bitpacking` (FOR: +1.4% on points, +2.4% on elevation,
+3.0% on IDs). All met.

**Code size (Z2),** the decoder alone (`parse`, `decode_all`, `get`) in a release cdylib with LTO,
minus an empty one: **50 KB native (9 KB gzipped), 98 KB wasm32 (24 KB gzipped)**. Before the
bounds checks went, the same code took 116 KB, and the 1024-value layout another 300 KB.

**Findings:**
- **128-value blocks (4 lanes) win on size for every column:** 1024-value blocks are 6–41%
  larger, because one wide value widens 1,024 values instead of 128. They decoded 2.3× faster
  while the decoder still had bounds checks; once those went, 128-value FOR reaches
  8.5–11 G/s, which is more than any query needs, so the 1024 layout and its 300 KB of code are
  dropped (D-073).
- **B13: safe code is enough.** With the block's words as a fixed-size array and the row and
  width as constants, the compiler removes every bounds check and vectorises the lanes, and the
  safe decoder is **22–44% faster than `bitpacking`'s `unsafe` SIMD** on the same layout.
  `forbid(unsafe_code)` stays.
- **Delta saves 6% on points and 50% on sorted IDs,** for a 4.6× slower `get` (23 against
  5 ns) and a 3× slower decode. The forge uses delta for a column when it saves at least 10%,
  FOR otherwise: delta for ID maps and other sorted columns, FOR for coordinates and elevation,
  where random access is common.
- **LEB128 deltas are 1.4–3.4× bigger on sorted IDs than block deltas and 6–19× slower to
  decode,** with no random access: block bit-packing replaces them everywhere.
- **Elevation is the one column where `bitpacking` is 2% smaller:** it stores no offsets.
  Offsets give O(1) access to any block, so they stay.

## B1 on sections: PLACES and HIERARCHY

```
cargo run --release -p teigeodata-bench --example b1-sections
cargo bench -p teigeodata-bench --bench b1 -- --warm-up-time 1 --measurement-time 3 B1-sections
```

**Input:** the same 148,210 places as a tree (`teigeodata_bench::geonames_tree`): the country
at the root, `ADMn` units under the unit above them with the same `GeoNames` admin codes, and
every other place under its most specific unit. 148,209 places have a parent, and the deepest
chain is 3. The codes stand in for point-in-polygon, so the shape is real enough for sizes and
speeds, not for accuracy.

**Size** (the column table's frame included):

| Section | Column | Coding (picked by the 10% rule) | B per place |
|---|---|---|---:|
| PLACES | lat | FOR | 2.221 |
| | lon | FOR | 2.224 |
| | kind | FOR | 0.319 |
| | admin level | FOR | 0.445 |
| | **total** | | **5.210** |
| HIERARCHY | parent (row + 1) | FOR | 1.825 |
| | child end (running count) | FOR | 0.185 |
| | children (rows, grouped) | delta | 1.017 |
| | **total** | | **3.027** |

**A/B:**
- **Children against Elias–Fano:** the CSR columns take 0.185 and 1.017 B per place; the
  Elias–Fano bounds for the same lists are 0.250 (the running ends, one list) and 1.116 (each
  child list on its own). Block bit-packing is smaller than Elias–Fano here because child lists
  are runs of nearby rows. Elias–Fano is not needed.
- **Parent as an offset from its row** (zigzag of parent − row): 1.921 B per place, against
  1.825 for the row itself. A unit's children spread over its whole Hilbert range, so the offset
  is no smaller. The row itself stays.
- **S1's 4.06 B per place** for the point was measured with a different delta scheme; the
  point here costs 4.445 B under FOR and 4.180 under delta. The 10% rule keeps FOR for
  coordinates (6% smaller is not worth a 4.6× slower `get`).

**Speed** (load average 2.2–2.5):

| Operation | Time |
|---|---:|
| Open PLACES and HIERARCHY (frame and every column's structure) | 4.9 µs |
| `point(row)`, random row | 9.7 ns |
| `kind(row)` | 2.9 ns |
| `parent(row)` | 4.5 ns |
| `ancestors(row)`, the whole chain | 29 ns |
| `children(row)`, the whole list, per admin unit | 12 ns |
| `Places::check` (deep, every row) | 1.6 ms (10.7 ns per row) |
| `Hierarchy::check` (deep, every row) | 5.1 ms (34 ns per row) |

At world size (about 12 million places) the deep checks take about 0.4 s, which the forge runs
once per build; opening stays in the sub-millisecond range, since it checks blocks, not rows.

## B5: ID to row (IDMAP and REDIRECTS)

```
cargo bench -p teigeodata-bench --bench b1 -- B5-idmap       # in tree, 1.5 × 10⁵ and 10⁶
```

The comparison with `ptr_hash` 2.1.3 runs out of tree, because `ptr_hash` and `ph` both pull in
crates with unmaintained advisories (`fxhash`; `derivative`, `pin-utils`) that `cargo deny`
rightly refuses in the workspace. A scratch crate depends on `teigeodata-format` by path and on
`ptr_hash`, and times both in one loop over the same 10⁶ random queries. `ptr_hash` uses
`StrongerIntHash`: its default `FastIntHash` took 10 s to build 10⁵ consecutive keys.

**Keys:** dense `np` serials 1..n, allocated in row order (the forge's rule, D-075) or shuffled
(the worst case). The rival gets a plain u32 permutation and a plain u64 ID array, its best case.
Load average 4.6–5.4 during the run (other work on the machine), so these numbers are
indicative.

| n | Serials | Ours, B per place | Ours, hit | Ours, miss | PtrHash + perm, B per place | PtrHash + perm + verify |
|---:|---|---:|---:|---:|---:|---:|
| 10⁵ | row order | **0.353** | 82 ns | 32 ns | 4.374 | 2.7 ns |
| 10⁵ | shuffled | 4.607 | 101 ns | 43 ns | 4.374 | 4.3 ns |
| 10⁶ | row order | **0.352** | 87 ns | 35 ns | 4.374 | 9.0 ns |
| 10⁶ | shuffled | 5.352 | 129 ns | 33 ns | 4.374 | 8.3 ns |
| 10⁷ | row order | **0.352** | 91 ns | 41 ns | 4.374 | 26.9 ns |
| 10⁷ | shuffled | 6.352 | 237 ns | 36 ns | 4.374 | 26.3 ns |

`id(row)` takes 5–8 ns (in tree). PtrHash alone: 1.0–2.5 ns, 2.99 bits per key, built in 0.71 s
at 10⁷.

**Findings:**
- **Size decides:** with serials in row order IDMAP is 12× smaller than the MPHF route (4 MB
  against 52 MB for the planet), at 60–80 ns more per lookup. Resolving stored IDs is rare, so
  the sorted list is adopted (D-075).
- **The forge must allocate serials in row order:** shuffled, ours is bigger and slower than the
  MPHF route on both counts.
- **The ramp coding** took IDMAP from 1.10 to 0.35 B per place and is now a codec coding
  (D-075); interpolation over block firsts took a hit from 216 to 87 ns.

## B6: presence (COMPONENTS)

```
cargo run --release -p teigeodata-bench --example b6-sizes          # sizes
cargo bench -p teigeodata-bench --bench b1 -- B6-presence            # reads, 10⁶ rows
```

**Sizes** (2026-11-07), bytes per row of the presence form, rows present at random or in runs
of mean length 64; the bitmap costs 0.129 B per row (1.03 bits) at any density:

| Density | Sorted rows, random | Sorted rows, runs | Smaller |
|---:|---:|---:|---|
| 0.1% | 0.0018 | 0.0024 | sorted (70×) |
| 1% | 0.0139 | 0.0163 | sorted (9×) |
| 3% | 0.0357 | 0.0411 | sorted (3.5×) |
| 10% | 0.0963 | 0.1172 | sorted (1.1–1.3×) |
| 50% | 0.3251 | 0.4164 | bitmap (2.5–3.2×) |
| 90% | 0.4142 | 0.4598 | bitmap (3.2–3.6×) |
| 99% | 0.2091 | 0.2656 | bitmap (1.6–2.1×) |

(10⁷ rows; 1.5 × 10⁵ rows gives the same within 1%.)

**Reads** per lookup, 10⁶ rows, random probes, load average 11–13 (indicative):

| Read | Every row | Bitmap, 70% | Sorted rows, 1% |
|---|---:|---:|---:|
| `Bits::rank` | | 6.9 ns | |
| `Component::index` | 0.5 ns | 4.7 ns | 130 ns |
| `Field::get` (index + one FOR read) | 7.4 ns | 20 ns | 130 ns |

**Findings:**
- **Both forms are needed:** neither wins everywhere, and the loss of a single form is large at
  either end (70× for a bitmap of a sparse component, 3.6× for sorted rows of a dense one). The
  writer measures both per component (D-076).
- **A field handle, not a per-read lookup:** the first API reopened the field column on every
  read (an O(blocks) validation) and took 4.8 µs; `Component::field` now returns a `Field` that
  keeps the validated column.
- **A delta-coded field costs a block unpack per read** (the first bench's values were ordered,
  so the writer picked delta, and a read took 51 ns). Fields of ordered values stay smaller
  that way; a hot field can be written FOR.

## B3: names (FSST)

```
cargo run --release -p teigeodata-bench --example b3-fsst     # trainer and decoder
cargo run --release -p teigeodata-bench --example b3-names    # the NAMES section
```

**Corpus:** `GeoNames` Nepal, 148,210 places, 183,248 names after deduplication per place,
1,892,140 B of UTF-8 ([names storage](../notes/names-storage.md) §3.1). 2026-11-07, load
average 10–26 (other work on the machine), so the times are indicative.

**Our trainer against `fsst-rs` 0.7.0** (the research note's run, same corpus):

| | Ours | `fsst-rs` |
|---|---:|---:|
| one table, text B | 1,005,961 (0.532) | 1,034,553 (0.547) |
| per script group, text B | 967,386 | 1,019,373 |
| Devanagari with its own table | 0.267 | 0.306 |
| training | 48 ms (5 generations, 1 MiB sample) | 1–3 ms (16 KiB sample) |

Decode: all names 750 MB/s; one random name 27 ns (load 26).

**The NAMES section** (records with script and type, as the writer makes them):

| Text ends | B per place | Locate a place's records | Canonical name | All names of a place | Deep check |
|---|---:|---:|---:|---:|---:|
| delta where 10% smaller | 9.99 | 11 ns | 158 ns | 166 ns | 13 ms |
| **O(1) codings (adopted)** | **10.64** | 10 ns | **65 ns** | **89 ns** | 8.6 ms |

**Findings:**
- **Our trainer is 3–5% smaller than `fsst-rs`**, because it samples 1 MiB instead of 16 KiB, and
  it is deterministic by construction (no hashing; ties broken by bytes). It runs on the forge,
  so its 48 ms do not matter.
- **Per-script tables are needed**: the shared table codes Devanagari at 1.17× its size, its own
  table at 0.27×. A table for a script holding half the text or more only duplicates the shared
  one, so it is skipped.
- **Hot offsets stay O(1):** delta-coded text ends saved 6% and more than doubled the time of a
  canonical name; `bitpack::encode_direct` is the writer's choice for them.
- **A deep check must stream:** the first `check` read each record's offsets with `get`, which on
  a delta column unpacks a block per record; streaming every column cut the test suite from 34 s
  to 4 s and makes the check O(records).
- **Open:** a canonical name takes 65 ns against the research bench's 20 ns for a bare FSST
  heap; re-profile on an idle machine (two offset reads, the table pick and the UTF-8 check are
  the candidates).

## B12: integrity (BLOCK_HASHES)

**Question:** how is a large section verified without hashing it whole on first use?
**Design:** the BLAKE3 chaining value of each 16 KiB block, merged into the table hash
([note](../notes/block-hashes.md), D-078). **Bench:** `cargo bench -p teigeodata-bench --bench
format -- L2`, sections of a half, a third and a sixth of the pack; the lazy columns read the half (IDMAP); 2026-11-07, load 6.

| Pack | Eager: hash every section | Prove the half's block hashes | First read of one block |
|---|---:|---:|---:|
| 10 MB | 5.58 ms | 23.8 µs | 8.79 µs |
| 100 MB | 55.6 ms | 237 µs | 8.86 µs |

**Findings:**
- **A first read costs one 16 KiB hash at any size:** 8.8 µs, 6,300× less than hashing the
  100 MB pack. The proof grows with the section (78 ns per block, about 5 ms per GiB) and runs
  once per open.
- **Eager hashing is unchanged** by the list (5.58 and 55.6 ms, p > 0.5), and small packs keep it.
- The 16 against 64 KiB comparison, the memo cost (0.7 ns a check) and the parallel full verify
  are in the [note](../notes/block-hashes.md); phone and wasm numbers are not measured yet.

## B4: blobs (zstd)

**Question:** how are cold strings stored so a pack stays small and one can still be read
alone? **Design:** zstd frames of whole blobs ([names storage](../notes/names-storage.md), D-079).
**Bench:** `cargo run --release -p teigeodata-bench --example b4-blobs`: each `GeoNames` Nepal row
(148,210 rows, 17.2 MB, 115.8 B each) as a blob; C zstd 19 without dictionary ID or checksum,
a 16 KiB dictionary trained by `zstd::dict::from_samples`; `ruzstd` 0.9.1 decode; 2026-11-07,
load 4.

| Frames | Dictionary | B per blob | of raw | Random blob | All in order | Encode |
|---:|---|---:|---:|---:|---:|---:|
| 1 KiB | none | 37.4 | 0.323 | 5.5 µs | 158 MB/s | 2.0 s |
| 1 KiB | 16 KiB | 29.7 | 0.256 | 4.1 µs | 207 MB/s | 2.4 s |
| 4 KiB | none | 26.2 | 0.226 | 13.6 µs | 237 MB/s | 2.3 s |
| 4 KiB | 16 KiB | 24.6 | 0.212 | 13.0 µs | 242 MB/s | 2.4 s |
| **16 KiB** | **none** | **22.4** | **0.194** | **43.9 µs** | **293 MB/s** | 2.8 s |
| 16 KiB | 16 KiB | 22.2 | 0.191 | 42.9 µs | 291 MB/s | 1.9 s |
| 64 KiB | none | 20.9 | 0.181 | 150 µs | 322 MB/s | 3.2 s |
| 64 KiB | 16 KiB | 20.9 | 0.181 | 153 µs | 302 MB/s | 1.9 s |

**Findings:**
- **A random read costs one frame:** the time grows with the frame (4 µs at 1 KiB, 150 µs at
  64 KiB), and the size falls slowly (−7% from 16 to 64 KiB). 16 KiB is the default: cold data
  is read rarely, and 43 µs is below a frame of a UI.
- **A dictionary pays only for small frames:** −21% at 1 KiB, −6% at 4 KiB, −1% at 16 KiB. The
  forge trains one only for frames of 4 KiB or less (blobs read per result).
- **`ruzstd` decodes at 160–320 MB/s** in order, against about 1.1 GB/s for C zstd in the
  research note: enough for cold data, and it keeps C out of the runtime.
- **A decompression bomb is refused at its stated length:** an 8 MiB frame of zeros (a few
  hundred bytes) stated as 100 bytes is refused after one step (`tests/blobs.rs`).

## Z2: library sizes per feature set

**Question:** what does each part of `teigeodata-core` and `teigeodata-format` add to a linked
library? **Method:** `cargo xtask sizes` builds `tools/size-probe`, a cdylib whose features each
export one entry point, natively (release, fat LTO, `panic = "abort"`, stripped) and for
`wasm32-unknown-unknown` (`opt-level = "s"`, before `wasm-opt`), and subtracts a baseline that
links what any host library already has (`std` formatting, a panic path, the allocator:
278.5 KiB native, 16.1 KiB wasm32). 2026-11-07, aarch64-apple-darwin, Rust 1.99.

| Feature set | native gzip | wasm32 | wasm32 gzip |
|---|---:|---:|---:|
| open: head checks, section lookup (BLAKE3 of the head) | 12.2 KiB | 17.4 KiB | 7.8 KiB |
| open + BLAKE3 of every section | 13.0 KiB | 18.9 KiB | 8.4 KiB |
| open + Ed25519 signature check | 27.4 KiB | 46.3 KiB | 19.2 KiB |
| open + every section view (places, hierarchy, ID map, names, components) | 35.9 KiB | 102.2 KiB | 24.9 KiB |
| open + BLOBS (`ruzstd`) | 42.9 KiB | 122.6 KiB | 33.7 KiB |
| **the whole reader** | **73.3 KiB** | **170.5 KiB** | **52.8 KiB** |
| the writer alone (forge side) | 15.9 KiB | 24.6 KiB | 10.6 KiB |

**Findings:**
- **The whole reader is 53 KiB gzipped in WASM** (171 KiB raw), before `wasm-opt` (Z3). Opening
  and verifying a pack costs 8 KiB; everything else is opt-in by what a caller touches.
- **The section views are the largest part** (85 KiB raw wasm): mostly the bit-unpacking kernels
  for every width, which B1 measured as 98 KB alone. They compress 4× (25 KiB gzipped).
- **`ruzstd` adds 105 KiB raw wasm and 26 KiB gzipped**: worth its opt-in `zstd` feature, since
  BLOBS is cold data a web client may never read.
- **Ed25519 adds 29 KiB wasm** (11 KiB gzipped), without the precomputed tables (L3).
- Native raw sizes move in 16 KiB steps (Mach-O pages on arm64), so they are left out here;
  gzip shows the code itself. Other targets (Linux, Windows, Android, iOS) are measured when
  their CI runners build the probe (Phase 4).

## Counts: instructions of the hot paths (the PR gate)

**Method:** `benches/counts.rs` under Callgrind (gungraun 0.20.0) on GitHub's `ubuntu-latest`,
run 42345045178, 2026-11-07. Each count includes its own parsing (`Packed::parse`,
`Pack::open`, the section's `open`), since a caller pays that too; inputs are synthetic and built
in setup. A pull request fails above +1% on any line (`.github/workflows/counts.yml`).

| Path | Instructions |
|---|---:|
| `Packed::get`, FOR / delta / ramp (parse a 10,000-value column, read one) | 1,000 / 3,266 / 1,005 |
| `Packed::decode_all`, delta, 10,000 values | 161,338 |
| Hilbert index of 1,000 points | 230,197 |
| `Bits::rank` (parse a 100,000-bit bitmap, one rank) | 270 |
| `Pack::open`, 1 MiB pack | 7,038 |
| `verify_sections`, 1 MiB | 2,652,311 |
| `verify_signature` (Ed25519) | 741,172 |
| first read of a 16 KiB block (`Blocks::verify`, 4 MiB pack) | 133,726 |
| ID to row (open, IDMAP open, lookup; 20,000 IDs) | 24,227 |
| canonical name (open, NAMES open, decode; 5,000 places) | 14,869 |
| one blob (open, BLOBS open, decoder, 16 KiB frame) | 511,366 |

**Findings:**
- **Counts are stable where wall clock is not:** they are the same on every run of the same code,
  so 1% is a real limit; the wall-clock pages keep their load notes.
- **A blob costs a frame:** 511k instructions, mostly the 16 KiB decode and the decoder's
  allocation; a caller reading many blobs keeps one `Decoder`.

