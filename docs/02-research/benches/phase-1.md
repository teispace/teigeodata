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
  100 MB takes 55 ms on one core. B12 decides when sections are hashed (eager, on first use, or
  per block), and that decides the default.
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

