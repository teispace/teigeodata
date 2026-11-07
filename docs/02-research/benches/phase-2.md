---
title: "Phase 2 benchmarks: indexes"
status: research
updated: 2026-11-07
---

# Phase 2 benchmarks: indexes

Measured with `crates/teigeodata-bench`. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md); the design is in the
[indexes note](../notes/indexes.md) and the [phase file](../../07-planning/phases/phase-2.md).

**Machine:** M2 Pro, 16 GB, macOS, release profile, single thread, 2026-11-07. The machine was
shared, with load averages of 4.4 to 5.1, which is above the catalogue's local limit of 2. These are
indicative numbers. The PR gate is the instruction count in `benches/counts.rs`
(`place_within`, `place_nearest`, `region_at`), which does not depend on load.

## B8: PLACE_INDEX

```
cargo run --release -p teigeodata-bench --example b8-place-index
```

The layout is specified in the [pack format](../../04-architecture/pack-format.md#place_index). Each
time is the median of 5 runs of 20,000 queries. A query starts at a place moved by up to 0.05° in
each axis, as a user's position would be. A city box is 0.05° wide and a district box 1°.

The rivals are an in-memory R*-tree (`rstar` 0.14.0) and flatbush (`geo-index` 0.4.0, f64 boxes,
Hilbert sort, node 16). Both rank by planar distance in degrees, which is cheaper and wrong near the
poles and the antimeridian. Ours ranks by great-circle (haversine) distance and splits boxes at the
antimeridian. The `rstar` size is an estimate of its heap use; it has no serialised form.

**Nepal (`GeoNames`):** 148,210 places. A city box holds 43.7 places on average, a district box
11,571.

| Index | B per place | city box ns | district box ns | k = 1 ns | k = 10 ns |
|---|---:|---:|---:|---:|---:|
| ours, node 8 | 1.19 | 1,903 | 16,208 | 3,082 | 4,440 |
| **ours, node 16** | **0.56** | 1,782 | 14,185 | 3,538 | 4,810 |
| ours, node 32 | 0.27 | 2,033 | 13,779 | 4,704 | 6,234 |
| `geo-index` 0.4, f64 | 38.40 | 1,022 | 29,874 | 2,106 | 3,268 |
| `rstar` 0.14 (in memory) | about 24 | 1,091 | 74,588 | 819 | 2,052 |

**World, uniform, 10⁶ places.** A city box holds 0.3 places on average, a district box 16.

| Index | B per place | city box ns | district box ns | k = 1 ns | k = 10 ns |
|---|---:|---:|---:|---:|---:|
| ours, node 8 | 1.18 | 1,072 | 1,696 | 7,815 | 11,603 |
| **ours, node 16** | **0.55** | 1,192 | 1,604 | 9,652 | 12,398 |
| ours, node 32 | 0.27 | 1,385 | 1,826 | 18,265 | 21,879 |
| `geo-index` 0.4, f64 | 38.40 | 444 | 994 | 2,199 | 3,299 |
| `rstar` 0.14 (in memory) | about 24 | 365 | 988 | 809 | 2,052 |

**World, uniform, 10⁷ places.** A city box holds 0.6 places on average, a district box 156.

| Index | B per place | city box ns | district box ns | k = 1 ns | k = 10 ns |
|---|---:|---:|---:|---:|---:|
| ours, node 8 | 1.18 | 1,594 | 3,854 | 12,057 | 19,794 |
| **ours, node 16** | **0.55** | 1,540 | 3,669 | 16,639 | 20,692 |
| ours, node 32 | 0.27 | 2,093 | 4,157 | 67,001 | 75,863 |
| `geo-index` 0.4, f64 | 38.40 | 735 | 2,684 | 2,649 | 3,749 |
| `rstar` 0.14 (in memory) | about 24 | 847 | 4,343 | 1,301 | 2,613 |

**What it shows:**

- **Size.** Storing u16 boxes relative to the parent costs 0.55 B per place at node 16. That is 70
  times smaller than `geo-index`'s f64 boxes, and the index is read in place from the pack. At
  10⁷ places the index is 5.5 MB; `geo-index` needs 384 MB.
- **Box queries** take 1.2 to 1.8 µs for a city box, within 2 to 3 times of the rivals. On
  district boxes with many hits, ours is the fastest: 14 µs on Nepal against 30 and 75 µs. A box
  contained in the query box lists its whole run of rows without testing each one.
- **Nearest places** are 4 to 12 times slower than the in-memory rivals at 3.5 to 17 µs. Each
  node expansion decodes its children's boxes from bit-packed columns, and every candidate is ranked
  by great-circle distance. Node 16 is the default: node 32 halves the size again, but its
  nearest-place search overflows the fixed heap on 10⁷ places and falls back to depth-first.
- **Latitude.** On 10⁶ world places, k = 1 by query latitude took 4.3 µs for 0–30°, 5.2 µs for
  30–60°, 7.2 µs for 60–75°, 16 µs for 75–85° and 42 µs above 85°. Near the poles a box's
  longitude gap shrinks toward zero distance, so more boxes stay open.

**What was tried (k = 1, 10⁶ world places, node 16):**

| Change | Effect | Kept |
|---|---|---|
| Depth-first search, children sorted by bound | up to 450 expansions per query | replaced |
| Best-first search on a fixed heap (Hjaltason and Samet) | about 10 expansions, 150 heap pushes | yes |
| `Packed::run`: decode a block at a time, not value by value | removes 65–82 ns per run | yes |
| Integer gates from the k-th distance, before any float bound | 11.8 → 9.7 µs; above 85°: 71 → 42 µs | yes |
| A greedy first descent to make the k-th distance tight early | 0–30°: 4.3 → 6.1 µs; above 85°: 42 → 21 µs | no: the common case got slower |

**Correctness.** Every query is compared with brute force over every row in
`tests/place_index.rs`: 7 sizes × 4 node sizes, and a 60,000-place pack of depth 4. A property
test checks 200,000 cases of the distance lower bound, and the overflow path is compared bit for
bit with brute force. The fuzz harness (`teigeodata_format::fuzz`) checks every index that passes
its check against brute force. A test edits 400 stored offsets by ±1 and expects each edit to be
refused by the canonical check.

**Mutation testing** (`cargo mutants` on `place_index.rs`, 507 mutants) found two gaps, both now
closed:

- **A longitude-gate bug.** When the k-th circle just reaches a pole (sin r = cos φ), the ratio
  given to asin rounds to just under 1. The gate then shut out rows across the pole: with k = 7,
  a south-pole row at Δλ = 91.7° was dropped in favour of a later tied row. The gate is now off
  for ratios above 1 − 10⁻⁶. A property test of 300,000 cases (a third near a pole, some exactly
  on it) fails on the old gate. The brute-force test now also checks that ties go to the lower
  row.
- **The format's rounding.** The test now checks the floor and ceiling of every offset exactly,
  not only that boxes contain their rows.

The remaining survivors only change how much is pruned (for example, a bound of 0 or a gate
that is always open). Results stay exact and only speed changes, so the instruction-count gate
covers them.

## B9: REGION

```
cargo run --release -p teigeodata-bench --example b9-region
```

The layout is specified in the [pack format](../../04-architecture/pack-format.md#region); the
rivals are researched in [REGION rivals](../notes/region-rivals.md). The input is
timezone-boundary-builder 2026f `timezones-with-oceans`: 444 zones and 8,296,476 ring vertices
on the 1e-7° grid, with Asia/Kathmandu given the lowest ID (Q-013). Each time is the median of 5
runs of 20,000 queries (uniform world points) or of all 148,210 `GeoNames` Nepal places.

**Size and build** (top level 10 unless stated):

| Leaf max | Top | Build s | MB | Leaves | Runs | Vertices | x + y MB |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 8 | 10 | 3.9 | 37.1 | 2,266,405 | 1,176,392 | 4,143,757 | 20.5 |
| 16 | 10 | 3.5 | 29.5 | 1,097,533 | 620,119 | 4,143,757 | 20.5 |
| **32** | **10** | 3.2 | **25.7** | 534,973 | 331,186 | 4,143,757 | 20.5 |
| 16 | 8 | 3.3 | 28.8 | 1,097,533 | 620,119 | 4,143,757 | 20.5 |
| 16 | 12 | 4.8 | 35.1 | 1,097,533 | 620,119 | 4,143,757 | 20.5 |

**Exactness** against `region::even_odd`, the reference that tests every ring of every zone
(leaf 16, top 10):

| Point set | Points | In 2+ zones | Mismatches |
|---|---:|---:|---:|
| uniform world | 20,000 | 65 | **0** |
| 1 unit (1e-7°, 1.1 cm) from an edge | 19,998 | 307 | **0** |
| `GeoNames` Nepal places | 148,210 | 2 | **0** |

**Lookup** (`Region::region`, the single answer):

| Index | Uniform world ns | Nepal places ns | Agreement with `tzf-rs` (uniform / Nepal) |
|---|---:|---:|---:|
| ours, leaf 8, top 10 | 46 | 40 | 99.9990% / 100% |
| ours, leaf 16, top 10 | 46 | 40 | 99.9990% / 100% |
| **ours, leaf 32, top 10** | **50** | **41** | 99.9990% / 100% |
| ours, leaf 16, top 8 | 75 | 83 | 99.9990% / 100% |
| ours, leaf 16, top 12 | 30 | 28 | 99.9990% / 100% |
| `tzf-rs` 2.3.0, full data, `get_tz_name` | 111 | 78 | n/a |

**What it shows:**

- **Size.** Shared arcs store each border once: 4.1 million vertices of the 8.3 million ring
  vertices, 20.5 MB of the default's 25.7 MB, against S2's 43 MB estimate. The vertices are
  absolute FOR-coded x and y; delta coding and interleaving x with y were tried and were larger
  (FOR keeps one base per block, and arcs jump between blocks).
- **Speed.** Every configuration from top 10 up is under 100 ns and about twice as fast as
  `tzf-rs` with full data, which also loads its geometry into 213 MiB of memory (its
  CHANGELOG); ours is read in place from the pack. A point in a top cell with no edges is
  answered from the TOP_ZONES column with one directory read.
- **Default leaf 32, top 10.** Leaf 16 saves 4 ns uniform and 1 ns on Nepal for 3.8 MB more;
  top 12 saves another 16–20 ns for 9.4 MB more. A pack that wants the speed can be built with
  `Options { leaf_max: 16, top: 12 }`; the reader handles all of them.
- **Agreement with `tzf-rs`.** It stores 1e-5° coordinates (about 1.1 m) and allows a point on
  a shared edge in both zones, so it is a speed rival, not an exactness reference. The 0.001%
  of uniform points where it disagrees are its rounding at a border.

**What was tried:**

| Change | Effect | Kept |
|---|---|---|
| A pointer quadtree (spike S2) | 190 ns uniform, cache misses per level | replaced by the top grid and Morton leaves |
| TOP_ZONES: the answer of an edge-free top cell, read before the leaf search | uniform 4–6× faster than without | yes |
| `Packed::run` short spans read value by value (`SHORT_RUN` 24) | a run of a few edges no longer decodes a whole block | yes |
| Interleaving x and y in one column | 35 MB, against 20.5 MB for two columns | no |
| Splitting until a leaf holds `leaf_max` edges | collinear overlapping borders split to level 24 (a 47 s test build) | replaced by the stuck rule: a cell of at most 16 edges stays a leaf when one child would keep them all |
| `check` re-testing every edge of a run for every leaf | 10 s for the test suite in debug | replaced by a cache of the last 64 edges tested |

**Correctness.** `tests/region.rs` compares every answer with `even_odd` on a jittered
partition with shared borders, overlapping stars with holes and cancelling rings, the world
grid, and tiny and empty inputs, at several leaf sizes and top levels, on every vertex, every
edge midpoint and the domain's corners. The encoding is deterministic. Every edit of a
structural column (TOP, DIR, KEY, LEVEL, RUNS) is refused by `open` or `check`, as are column ID
0, an unknown column and a reference candidate past 4,096. The fuzz harness builds random
regions and compares every pack that passes its check with `even_odd`.

**Mutation testing** (`cargo mutants` on `region.rs`, 365 of 507 mutants before its time
limit) found one real gap: `even_odd`, the test oracle, used the reader's `normalise`, so a
mutant there changed both sides. The oracle now reads the domain's edges on its own. The writer
and the reader share the cell, Morton, reference-point and crossing code, so a change there can
keep the round trip exact while changing the format: the bytes and shape of three packs are now
pinned, as are `index` at the cell edges and the reference candidates, and planted mutants of
each (reference, `max1`, `index`, `normalise`) fail. Two `check_sets` refusals were added. The
`|` → `^` survivors in `spread`, `compact` and `morton` are equivalent: the bits combined never
overlap.

