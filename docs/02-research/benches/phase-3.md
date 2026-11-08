---
title: "Phase 3 benchmarks: geometry"
status: research
updated: 2026-11-08
---

# Phase 3 benchmarks: geometry

Measured with `crates/teigeodata-bench`. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md); the design is in the
[geometry note](../notes/geometry-topology.md) and the
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
