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
