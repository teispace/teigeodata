---
title: "Snap rounding for boundary sources that are not noded"
status: research
updated: 2026-11-08
owner: forge
revisit: 2027-05-08
---

# Snap rounding for boundary sources that are not noded

## 1. Question

The forge's topology stage cuts the finest admin level into shared arcs. That only works when
the source is **noded**: no two edges cross or touch away from a shared vertex. COD-AB Nepal is
noded. geoBoundaries India ADM2 is not. Its first run stopped on 4 edge pairs, and most world
sources will have such defects. How should the stage node a source, and how much does that cost?
This feeds the topology stage ([forge](../../04-architecture/forge.md#topology-topology), D-098)
and the India measurement in phase 3.3.

## 2. Summary and recommendation

- **Use our own snap rounding** (Hobby 1999) on the 1e-7° grid, built in
  `forge/crates/forge/src/topo/snap.rs`.
  - The hot pixels are the unit square around every vertex, plus each proper crossing rounded
    to the pixel that holds it.
  - Every edge that meets a hot pixel is routed through the pixel's centre.
  - No input vertex moves. An edge moves by at most half a pixel diagonal (about 8 mm).
- **Pixels are half-open**: the left and bottom sides belong to the pixel, the top and right
  sides do not. JTS and GEOS use the same rule. With closed squares, two routed pieces can
  cross at a shared pixel corner. Our property test found exactly that case (§3.2).
- **A noded source is returned unchanged**, so Nepal's packs are byte-identical.
- **India:** 4 bad pairs → 3 crossing pixels, 11 edges routed, 27 vertices added, no ring
  collapsed. The whole topology stage runs in 4.3 s and uses 640 MB of RAM.
- **Not covered here:** gaps and overlaps between neighbouring units (slivers). Snap rounding
  makes edges consistent but does not reassign faces. That is the next cleanup step; see §7.

## 3. Findings

### 3.1 What India's defects are

geoBoundaries IND ADM2 (735 districts, 1,995,009 ring vertices, commit `9469f09`) is mostly on
a 1e-5° grid, and its neighbouring districts share vertices exactly. 56.2% of the vertices end
up in shared arcs. The 4 bad pairs are edges with off-grid vertices that cross a neighbour's
edge by a few millimetres. One example is the edge (92.26092°, 26.06349°)–(92.26511°, 26.06380°)
against (92.2611448°, 26.0635066°)–(92.2664198°, 26.0639219°).

### 3.2 Correctness

- **The theorem.** Snap rounding's output never crosses: rounded pieces meet only at hot pixel
  centres or along shared pieces (Hobby 1999; Guibas and Marimont 1998). The proof needs every
  point of the plane to be in exactly one pixel, hence the half-open squares.
- **Found by test.** A random test of overlapping quadrilaterals passed 2,000 seeds with wide
  rings. With tiny rings (2 to 3 units), closed squares failed at seed 16: the pieces
  (0,3)–(1,4) and (0,4)–(1,3) crossed at the pixel corner (0.5, 3.5).
  - The half-open test passes 2,000 seeds of both sizes.
  - A planted bug (top and right sides counted as inside) makes it fail again.
- **Exact arithmetic.**
  - The pixel test works in doubled coordinates, so the square's sides are integers, and
    clips the edge to the square with Liang–Barsky parameters kept as i128 fractions.
  - A crossing's pixel is ⌊n/d + ½⌋, which is right for negative values too.
- **The stage checks its own result.** After snapping, the stage runs the crossing check
  again and refuses any pair that is left. ARCS is then re-read through the format reader and
  checked at every level of detail, as before (D-098).

### 3.3 Cost

- **Edge lookup.** Edges and hot pixels share one uniform grid, whose cell is the median edge
  length. An edge visits only the cells of its own slab, widened by one unit on each side.
  - The widening was itself a bug the test found: a pixel in the next column, within one unit
    of the edge, was missed.
  - A long edge therefore costs time linear in its length, not in its bounding box.
- **When it runs.** Snapping runs only when the crossing check finds a bad pair. Otherwise the
  stage costs one crossing check, which it already ran.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Version | Verdict |
|---|---|---|---|---|---|
| **Own Hobby snap rounding, half-open pixels** | exact integers, no new dependency, keeps every vertex, deterministic, about 300 lines with tests | we own its correctness (property test plus the stage's own re-check) | ours | — | **adopt** |
| `i_overlay` 9.1 overlay | robust integer boolean ops | two-operand overlay (subject and clip): it loses which of N units owns a face unless run per pair; its snapping "selects an existing endpoint", not Hobby's; i32 range trap (geometry note §3.8) | MIT or Apache-2.0 | 9.1.0 | no (for noding) |
| JTS/GEOS `SnapRoundingNoder` | the reference implementation, the same half-open rule | Java/C++; GEOS is LGPL and a C toolchain in the forge | EPL/EDL; LGPL | JTS 1.20 | no; used as the reference |
| Snap by tolerance (mapshaper `-snap`) | simple | can make crossings; needs a tolerance per source | MPL | 0.7.101 | no |
| Iterated or stable snap rounding (Hershberger 2013) | no vertex moves near another segment's pixel more than once | more work; drift of ordinary snap rounding is negligible at 1 cm pixels | — | — | not needed (§7) |

## 5. What we build ourselves, and why

[`snap.rs`](../../../forge/crates/forge/src/topo/snap.rs), described in its module comment.
Two measurements prove it: the property test of §3.2, and Nepal's packs staying byte-identical.

## 6. Impact on the plan

- D-103 records snap rounding in the topology stage. It closes D-098's "`i_overlay` snap
  rounding joins with the first source that is not noded".
- The forge page's topology section now says the stage nodes the source itself.
- `forge topology --geoboundaries <file> <name>` runs the stage on one geoBoundaries level for
  measurement. It also writes a pack of the geometry sections alone
  (`data/build/topology/<name>/geometry.tgd`). That file is not a pinned source and feeds no
  build. India's numbers are in [B10](../benches/phase-3.md#india-adm2-geoboundaries).

## 7. Risks and open items

- **Slivers and gaps.** Overlapping neighbours stay overlapping after rounding.
  - If two units cover the same edge in the same direction, `arcs::build` refuses it.
  - REGION gives a point in an overlap both units, and a point in a gap none.
  - The forge-pipeline note (§3.4) has the planned fix: find small faces by area and
    thinness, and give each to the neighbour with the longest shared edge.
  - India did not need it. It joins with the first source that does.
- **Drift.** Snap rounding can move an edge close to an unrelated vertex. At 1 cm pixels, this
  is far below any source's accuracy.
- **Scale exponent.** Vertices added off a source's 1e-5° grid set that arc's `k` to 0. On
  India this affects at most 11 edges.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `i_overlay` | 9.1.0 | 2026-11-08 | [geometry note](geometry-topology.md#38-robustness-and-how-others-do-it-question-7) |
| geoBoundaries IND ADM2 | gbOpen, commit `9469f09` | 2026-11-07 | [geometry note](geometry-topology.md#310-india-adm2-geoboundaries) |

## 9. Sources

- Hobby, *Practical segment intersection with finite precision output*, Computational
  Geometry 13(4), 1999.
- Guibas and Marimont, *Rounding arrangements dynamically*, IJCGA 8(2), 1998.
- Hershberger, *Stable snap rounding*, Computational Geometry 46(4), 2013.
- [NetTopologySuite `HotPixel`](https://nettopologysuite.github.io/NetTopologySuite/api/NetTopologySuite.Noding.Snapround.HotPixel.html)
  (the JTS port): the pixel holds its interior and its boundary minus the top and right
  sides. Accessed 2026-11-08.
- [JTS `SnapRoundingNoder`](https://locationtech.github.io/jts/javadoc/org/locationtech/jts/noding/snapround/SnapRoundingNoder.html),
  accessed 2026-11-08.
- [GEOS commit porting JTS's hot pixel corner-tangent fix](https://lists.osgeo.org/pipermail/geos-commits/2020-October/003317.html),
  2020-10. Its tests treat the open top and right sides as the reason a corner-tangent segment
  does not meet the pixel.
