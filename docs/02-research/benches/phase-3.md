---
title: "Phase 3 benchmarks: geometry and elevation"
status: research
updated: 2026-11-08
---

# Phase 3 benchmarks: geometry and elevation

Measured with `crates/teigeodata-bench`. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md); the design is in the
[geometry note](../notes/geometry-topology.md), the [DEM note](../notes/dem-section.md) and the
[phase file](../../07-planning/phases/phase-3.md).

**Machine:** M2 Pro, 16 GB, macOS, release profile, single thread, 2026-11-08, load average
about 3 (shared machine: indicative numbers; node counts do not depend on load).

## B10: POLY_INDEX and ring reads

```
cargo run --release -p teigeodata-bench --example b10-poly-index
```

Input: the Nepal main pack from `forge make np` (COD-AB, 860 polygons in 4 levels of 1, 7, 77
and 775; 2,315 arcs, 593,610 vertices). Queries: 20,000 points uniform over the country's box
(41% fall inside Nepal), and 2,000 viewports of 0.05° and of 1°. A node is one decode of up to
node-size boxes.

### Leaf order and node size

Leaves must follow POLYGONS order, which groups the polygons by admin level so a row maps to its
polygon by a binary search per group; the other orders are measured to see what that costs.

| Order | Node | B per polygon | Nodes per point, levels 0 / 1 / 2 / 3 | Nodes per point, all levels | 0.05° view | 1° view |
|---|---:|---:|---|---:|---:|---:|
| level-first, row order (stored) | 8 | 10.13 | 4.0 / 4.0 / 4.9 / 5.5 | 8.6 | 8.9 | 15.3 |
| **level-first, row order (stored)** | **16** | **9.51** | 3.0 / 3.0 / 4.3 / 4.5 | 6.5 | 6.7 | 10.2 |
| level-first, row order (stored) | 32 | 9.20 | 2.0 / 2.0 / 2.9 / 2.7 | 4.1 | 4.2 | 6.1 |
| level-first, box-centre order | 16 | 9.51 | 3.0 / 3.0 / 4.3 / 4.4 | 6.4 | 6.6 | 10.1 |
| one Hilbert sort, levels mixed | 16 | 9.51 | 5.7 / 5.7 / 5.7 / 5.7 | 5.7 | 5.9 | 9.3 |

- **Level-first wins the query that matters,** one admin level at a point: 3 to 4.5 nodes
  against 5.7, because a level filter skips every subtree outside its group, and the coarse
  levels are a single path. A mixed tree is 12% cheaper only for queries over every level at
  once.
- **Row order is as good as box-centre order** within a level (PLACES rows are in Hilbert order
  of the place's point, close to the box centre), so POLYGONS' order stays.
- **Node 16** as PLACE_INDEX (D-082). Node 32 reads fewer nodes but decodes twice the boxes per
  node; the index is 8 KB, so its size does not decide.

### Point → admin unit through POLYGONS

`PolyIndex::containing` per level: the index's candidates, then `Polygons::contains` on every
ring vertex.

| Level | Polygons | Candidates per point | Found per point | ns per lookup |
|---:|---:|---:|---:|---:|
| 0 (country) | 1 | 1.00 | 0.41 | 354,330 |
| 1 (province) | 7 | 0.86 | 0.41 | 99,624 |
| 2 (district) | 77 | 0.79 | 0.41 | 23,027 |
| 3 (local level) | 775 | 0.83 | 0.41 | 9,323 |

The index is not the cost (fewer than one candidate per point); the ring test is, at 6.6 ns per
vertex. This confirms the note's §3.7: point → admin unit is REGION's job (41–50 ns, D-083), and
`containing` is its oracle and the fallback of a pack without REGION.

### Admin REGION on the shared arcs

One REGION over all 860 polygons (region `i` = polygon `i`), D-083's defaults (leaf 32, top
grid 10), built once with its own X and Y as D-083 lays it out and once on the pack's ARCS,
where each run stores its first vertex and reads the rest as a slice of ARCS's deltas.

| Vertices | Bytes | Of which vertices | Leaves | Runs | Build | ns per query (uniform) |
|---|---:|---:|---:|---:|---:|---:|
| own X and Y | 3,627,624 | 2,916,528 (X, Y) | 63,619 | 47,233 | 490 ms | 153 |
| **on ARCS** | **977,344** | 264,280 (run first vertices) | 63,619 | 47,233 | 274 ms | **155** |

Both give the same regions at all 20,000 points, and the own-vertex one agrees with
`Polygons::contains` at all of them. The rest of the on-ARCS section: leaves 368 KB, top grid
150 KB, runs 177 KB. Sharing ARCS saves 2.65 MB (27% of the main pack) for 1% of query time,
so D-088's choice stands and the fallback to own vertices is not needed (D-100).

Leaf size and top grid, on ARCS (a second run, so the default row differs from the table above
by machine noise):

| Leaf max | Top | Bytes | Leaves | Runs | Uniform ns | Places ns |
|---:|---:|---:|---:|---:|---:|---:|
| 8 | 10 | 3,238,524 | 306,619 | 164,705 | 144 | 265 |
| 16 | 10 | 1,727,504 | 139,558 | 86,478 | 142 | 269 |
| **32** | **10** | **977,344** | 63,619 | 47,233 | 184 | 298 |
| 64 | 10 | 600,444 | 28,762 | 27,209 | 187 | 362 |
| 16 | 12 | 3,913,008 | 139,558 | 86,478 | 109 | 204 |
| 32 | 12 | 3,176,496 | 63,619 | 47,233 | 126 | 238 |

- Admin borders are dense: almost no top cell is free of edges, so the timezone index's
  one-read answer (41 ns on Nepal places, D-083) rarely applies, and a query costs 150–300 ns,
  more on places (they sit where borders are) than on uniform points.
- A finer top grid helps (−20% at level 12) but the grid covers the whole world, so it costs
  2.2 MB more for one country. A grid over the pack's own extent gives that speed for a few
  KB: [below](#a-fine-grid-over-the-packs-extent).
- Leaf 32 stays the default: smaller leaves buy little time for much size.

### A fine grid over the pack's extent

The top grid covers the world, so its level is bound by its world size. The fine grid (D-101)
covers only the box of the regions' vertices, at the finest level up to 16 whose window fits a
cell budget, and its cells answer at once or start a gallop at their first leaf. The same
20,000 uniform points and 20,000 places, every row giving the same regions as the first (load
average 15–27 during the run, so compare rows rather than read absolute times):

| Leaf max | Top | Fine budget | Fine grid | Bytes | Leaves | Uniform ns | Places ns | Check ms |
|---:|---:|---:|---|---:|---:|---:|---:|---:|
| 32 | 10 | 0 | none | 977,344 | 63,619 | 150 | 296 | 40 |
| 32 | 12 | 0 | none | 3,176,496 | 63,619 | 119 | 230 | 303 |
| 32 | 8 | 0 | none | 838,256 | 63,619 | 185 | 324 | 23 |
| 32 | 8 | 2^14 | level 12, 94 × 94 | 857,508 | 63,619 | 120 | 229 | 24 |
| 32 | 8 | 2^16 | level 13, 187 × 187 | 910,720 | 63,619 | 95 | 188 | 27 |
| **32** | **8** | **2^18** | **level 14, 372 × 374** | **1,066,180** | 63,619 | **91** | **153** | 37 |
| 32 | 8 | 2^20 | level 15, 742 × 747 | 1,555,120 | 63,619 | 74 | 131 | 82 |
| 16 | 8 | 2^16 | level 13, 187 × 187 | 1,663,456 | 139,558 | 86 | 166 | 42 |
| 16 | 8 | 2^18 | level 14, 372 × 374 | 1,830,196 | 139,558 | 74 | 131 | 55 |
| 64 | 8 | 2^18 | level 14, 372 × 374 | 676,208 | 28,762 | 104 | 243 | 36 |

- A level-12 fine grid over Nepal matches the world level-12 grid's speed for 20 KB instead of
  2.2 MB: the world grid's cost was its extent, not its level.
- The default is leaf 32, top 8 and a 2^18-cell budget: places 296 → 153 ns (−48%), uniform
  150 → 91 ns, for 89 KB more (+9%). The coarse top grid shrinks to level 8 because the fine
  grid answers inside the extent. A 2^20 budget gains 15% more for 489 KB more; leaf 16 gains
  the same for 764 KB more.
- On the world timezones with oceans (B9's input, `b9-region`, same load): the old default
  (leaf 32, top 10) 25.7 MB, 138 ns uniform and 94 ns on Nepal places; the new one 25.5 MB,
  90 and 56 ns (its window is the world, at level 9); zero mismatches against even–odd on all
  three exactness sets. Leaf 16 with a world level-12 top grid stays the fastest there (46 and
  29 ns) for 35.1 MB.
- In the main pack: ADMIN_REGION 977 → 1,066 KB, the timezone REGION about 90 KB larger;
  the pack 10.72 → 10.90 MB, lite 8.16 → 8.25 MB.

### Ring decode speed by level of detail

Every ring of every polygon read through `Polygons::ring` (shared arcs are read once per ring
that uses them: 1.76 M ring vertices from 594 K arc vertices).

| Level of detail | Vertices read | ns per vertex read |
|---:|---:|---:|
| 0 | 1,755,855 | 6.64 |
| 1 | 854,265 | 14.39 |
| 2 | 356,253 | 23.49 |
| 3 | 118,871 | 49.63 |
| 4 | 32,679 | 155.95 |
| 5 | 11,389 | 432.94 |
| 6 | 8,251 | 594.51 |

A coarse read still decodes every vertex of the arcs it reads (deltas are cumulative), so its
cost is that of level 0: about 4.9 ms for all of Nepal at any level. A level filter skips
output, not work. Rendering a coarse world view needs a way to skip the fine vertices: an
overview arc set (a second ARCS at the coarse levels), or per-block level bounds with the block
checkpoints. That is planned in 3.3 with the whole-world render measurement.

### Coarse reads from an overview

An overview (D-102) is a second ARCS holding, for every arc, only the vertices of level `f` and
coarser, written by `arcs::overview`. `B10_OVERVIEW=1` runs only this part. Every ring read from it equals the ring read
from ARCS at each level from `f` to 6 (asserted). ARCS: 2,659,828 B, 593,610 vertices. Ring
reads read shared arcs once per ring (8,251 ring vertices at level 6 from 2,315 arcs):

| Overview from level | Vertices | Bytes | % of ARCS | ns per vertex read at its level | at level 6 |
|---:|---:|---:|---:|---:|---:|
| ARCS itself | 593,610 | 2,659,828 | 100% | — | 612 |
| 1 | 302,929 | 1,446,976 | 54.4% | 7.97 | 388 |
| 2 | 127,014 | 660,836 | 24.8% | 10.26 | 270 |
| 3 | 43,404 | 255,488 | 9.6% | 19.48 | 223 |
| **4** | **13,268** | **94,780** | **3.6%** | **49.40** | **175** |
| 5 | 5,911 | 54,796 | 2.1% | 102.66 | 138 |

ARCS read at each level for comparison: 7.3 (level 0), 15.0, 23.9, 52.5, 163.0, 451.3 and
611.9 ns per vertex read (levels 1 to 6). A load average of 15–40 makes single rows noisy
(±20%); the trend holds across three runs.

- **From level 4:** levels 4 to 6 read 3.3 to 3.5 times faster (163 → 49 ns per vertex at
  level 4; all of Nepal at level 6 in 1.4 ms instead of 5.0 ms) for 95 KB. From level 3
  costs 2.7 times the bytes for a faster level 3 only; from level 5 saves 40 KB but slows
  level 4 down to ARCS's speed. The forge writes the overview from level 4.
- **What remains is per arc, not per vertex:** reading one overview arc at level 6 costs
  110–150 ns for 2–3 vertices, about 55 ns of which is making its iterator (it copies the
  section handle and clears a 1.2 KB block buffer). Reading short pieces value by value
  instead of by whole-block decodes was tried and gained nothing measurable, so it was not
  kept. A lighter iterator is the next step if a whole-world render needs it.
- The note's estimate (an overview of the two coarsest levels at 3.8% of ARCS) holds: from
  level 4, 3.6%.


### India ADM2 (geoBoundaries)

```
forge topology --geoboundaries data/cache/gb-ind-adm2/geoBoundaries-IND-ADM2.geojson gb-ind-adm2
B10_PACK=data/build/topology/gb-ind-adm2/geometry.tgd cargo run --release -p teigeodata-bench --example b10-poly-index
```

Input: geoBoundaries gbOpen IND ADM2, commit `9469f09` (ODbL; measurement only, not ingested):
735 districts, 1,995,009 ring vertices, mostly on a 1e-5° grid. It is the first source that is
not noded: 4 edge pairs cross by a few millimetres, so the stage snap rounds it (D-103,
[note](../notes/snap-rounding.md)): 3 crossing pixels, 11 edges routed, 27 vertices added, no
ring lost. The stage runs in 4.3 s with 640 MB of RAM. The geometry-only pack has 736 polygons
(the country dissolved from the districts, and the districts); each is its own row in Hilbert
order of its label, as PLACES numbers rows. Measured 2026-11-08 at a load average of 15–21, so
timings are ±20%; sizes and node counts are exact.

| Section | Bytes | Per unit |
|---|---:|---|
| ARCS (2,592 arcs, 1,122,132 vertices, 56.2% of the ring vertices) | 3,804,472 | 3.39 B per vertex |
| POLYGONS | 18,388 | 25 B per polygon |
| POLY_INDEX (node 16, 3 levels) | 7,136 | 9.7 B per polygon |
| ADMIN_REGION (127,522 leaves, 94,457 runs, fine grid level 12: 334 × 691) | 2,109,444 | 1.88 B per arc vertex |
| ARCS_OVERVIEW from level 4 (60,672 vertices) | 291,720 | 7.7% of ARCS |

- **ARCS at 3.39 B per vertex**, against 4.48 on Nepal: 2,197 of the 2,592 arcs (990,081
  vertices) are stored on a coarser decimal grid (scale above 0). The note's model gave 2.68 B
  for x and y on the 1e-5° grid; levels add 0.44 and checkpoints and bases the rest. Snapping
  cost the scale of at most the 11 routed edges' arcs.
- **Levels of detail:** 49% of the vertices are kept at 5 m, 31% at 20 m, 15% at 100 m, 5.4%
  at 500 m and 0.9% at 5 km. The guard fixed 240 crossings and 2 degenerate rings over all
  levels with 478 vertices; none was left.
- **POLY_INDEX:** in row order, 5.2 nodes per point at node 16 (5.0 in box order, 5.2 for one
  Hilbert sort): the stored order holds on India. Before the measurement pack numbered rows by
  label Hilbert order, rows in `shapeID` order read 26.7 nodes per point: the index relies on
  rows being in spatial order, which PLACES guarantees.
- **ADMIN_REGION**, 20,000 points uniform over India's box (32% inside) and the 736 label points:

  | Leaf max | Top | Fine cells | Fine grid | Bytes | Uniform ns | Labels ns | Check ms |
  |---:|---:|---:|---|---:|---:|---:|---:|
  | 32 | 10 | 0 | none | 1,853,452 | 101 | 142 | 65 |
  | 32 | 8 | 0 | none | 1,732,188 | 131 | 212 | 48 |
  | 32 | 8 | 65,536 | 11: 168 × 346 | 1,855,320 | 83 | 69 | 53 |
  | **32** | **8** | **262,144** | **12: 334 × 691** | **2,110,020** | **67** | **51** | **64** |
  | 32 | 8 | 1,048,576 | 13: 666 × 1,381 | 2,896,220 | 62 | 48 | 108 |
  | 64 | 8 | 262,144 | 12: 334 × 691 | 1,306,516 | 78 | 52 | 49 |

  The default (D-101) is again the knee: against no fine grid, 2 times faster on uniform
  points and 4 times at label points, for 22% more bytes. Every one of 20,000 points gives the same regions as `Polygons::contains`. On the
  arcs it is 2.11 MB against 7.30 MB with its own vertices (D-100 holds).
- **Rings through POLYGONS** (`containing`) cost 21 µs per district and 1.8 ms for the
  country's 1,072-ring outline, so point → admin stays on ADMIN_REGION.
- **Overview:** from level 4 it is 7.7% of ARCS (3.6% on Nepal, whose borders are smoother at
  coarse levels), and reads level 6 in 77 ns per vertex against 334 for ARCS (4.4 times
  faster). From level 3 it would be 19% of ARCS for 104 ns, and from level 5, 3.3% for 62 ns.
  Level 4 stays the default.

## B11: DEM

```
forge dem-section copernicus-glo30-np <west,south,east,north> [--no-refine] [--thresholds a,…,g]
cargo run --release -p teigeodata-bench --example b11-dem
```

The stage codes the 1° Copernicus GLO-30 tiles in a box into a DEM section on 64 × 64 tiles at
±1 m (D-104, [layout](../../04-architecture/pack-format.md#dem)), checks it whole, compares
20,000 random points with the source's bilinear heights, and writes a pack of the section alone
(`data/build/dem-section/<tileset>/dem.tgd`; not a pinned source, it feeds no build). Inputs:
the four research tiles of the [DEM note](../notes/dem-section.md) (§3.1). Measured 2026-11-08
at a load average of 12–77: sizes are exact, timings are the fastest of 6 alternating runs
(preemption only adds time) and still about ±15%.

### Size against the model

Bits per sample over the DEM tiles that lie wholly inside the source tile:

| Source tile | Terrain | Rust (refined) | Rust (least squares only) | Note's model |
|---|---|---:|---:|---:|
| N26 E085 | Terai | 1.112 | 1.125 | 1.11–1.12 |
| N27 E085 | mid-hills | 2.704 | 2.711 | 2.69–2.73 |
| N27 E086 | mountains | 2.935 | 2.943 | 2.93–2.97 |
| N28 E084 | high mountains | 2.755 | 2.766 | 2.74–2.78 |

- The built codec lands inside the note's model on every tile. Over each whole 1° box (its
  tiles at the edges included) the section is 1.83, 4.42, 4.83 and 4.52 MB.
- Every probe is within 1.0000 m of the source: the bound holds by construction.
- **Refinement** of the least-squares coefficients by integer descent on the coded cost gains
  0.4–1.2% and makes encoding 4–6 times slower (about 4 s per 1° tile on this machine); it
  stays on, since encoding is a forge cost paid once.
- **Thresholds:** six sets (spread from `0,1,2,4,7,12,20` to `1,3,6,10,16,26,42`) are within
  0.1% of each other, except when the first threshold is above 0: then flat ground loses its
  own context and the Terai grows 6%. The default `0,1,3,6,11,20,36` stays.
- **Forge:** the two-tile box (25.9 M samples) is coded in 5.9 s with refinement and read back
  in 0.35 s, peak memory 206 MB.

### Decode speed

On the two-tile section (N27 E085 and E086, 9.21 MB, 6,441 tiles):

| Tiles | Bytes | Whole tiles | Cold query | Warm query (path) |
|---|---:|---:|---:|---:|
| **64 × 64** | **9,210,288** | **13.6 ns per sample (56 µs per tile)** | **31 µs** | **0.43 µs** |
| 32 × 32 | 9,707,408 (+5.0%)¹ | 13.8 ns per sample (14 µs per tile) | 8.9 µs | 0.57 µs |

¹ Both sizes without refinement (64 × 64: 9,241,540 B); the timings do not depend on it.

- **Cold** is 20,000 random points, almost every one in a new tile: it decodes about half a
  tile. **Warm** is a path of points about 30 m apart, so the scratch goes on in the same tile.
- The decoder went from 19.5 to 13.6 ns per sample (cold 46 to 31 µs) by two changes, both
  measured A/B: inside a row the six neighbours roll in registers, so a new sample never waits
  for the one just stored; and the symbol search starts from a 64-bucket table per context, so
  it rarely loops. A branch-free count of the cumulative table (to replace the scan) was twice
  as slow. Keeping the coder's state in locals changed nothing (the compiler already did).
- The note's 3–6 ns per sample was an unverified estimate. The chain from one sample to the
  next (prediction, context, symbol, state) is serial, so a second rANS state would not
  shorten it.
- **32 × 32 tiles** answer a cold query 3.3 times faster for 4–8% more bytes (Terai +7.7%,
  hills +4.1–6.0%), mostly the 17-byte tile header. 64 × 64 stays the default (D-104); the
  side is per grid, so a pack can change it without a format change.

### Not yet measured

- All 45 Nepal tiles, the Nepal total against the note's 58 MB, and 2–3 tiles outside Nepal (a
  desert, a coast, above 50°): the download (1.84 GB) waits for approval.
- LERC and zstd on the same tiles; wasm and a phone.
