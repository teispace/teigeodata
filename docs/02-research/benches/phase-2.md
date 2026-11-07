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
(`place_within`, `place_nearest`), which does not depend on load.

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
