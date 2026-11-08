---
title: "Geometry: ARCS, POLYGONS, POLY_INDEX and the forge topology stage (B10)"
status: research
updated: 2026-11-08
owner: maintainer's agents
revisit: 2027-05-07
---

# Geometry: ARCS, POLYGONS, POLY_INDEX and the forge topology stage (B10)

## 1. Question

Phase 3, benchmark B10: design the ARCS (kind 7), POLYGONS (kind 8) and POLY_INDEX (kind 9)
sections and the forge stage that produces them. The input is admin boundaries from the country
down to the lowest level (Nepal: 7 provinces, 77 districts, 753 local levels plus 22
protected-area units, 6,743 wards), at several levels of detail, on the 1e-7° grid
([ADR-0029](../../03-decisions/adr/ADR-0029-coordinate-grid.md)). Packs are read in place on
every platform, wasm and mobile included. The design must hold for the whole world: 10⁵ to 10⁶
polygons and 10⁸ or more vertices.

This note builds on these and does not repeat them:
[format note §3.6–3.7](format-compression-indexing.md#36-spatial-indexes-region-index-and-predicates),
[forge note §3.4](forge-pipeline.md#34-topology-for-boundaries),
[indexes note](indexes.md) (D-082, D-083) and the
[pack format](../../04-architecture/pack-format.md) (column tables, PLACE_INDEX, REGION).

## 2. Summary and recommendation

1. **Vertex coding.** Store ARCS vertices as zigzag deltas within each arc, packed by the
   existing B1 codec with FOR coding, plus each arc's first vertex in two FOR columns. No codec
   change is needed. On Nepal admin3 (593,610 arc vertices) this is **3.98 B per vertex**,
   against 4.90 for REGION's absolute FOR, 4.43 for B1 delta over arcs placed back to back,
   4.09 for varint, 4.43 for StreamVByte and 3.93 for zstd-19, which has no random access
   (§3.1). On India ADM2 (1,122,129 vertices) it is 4.34 B, against 5.13 B for FOR.
   - geoBoundaries India is published on a 1e-5° grid: 98% of its 1e-7° values are multiples
     of 100. An optional **per-arc scale exponent** (deltas stored ÷ 10^k) brings it to **2.68 B
     per vertex**, 38% less.
   - Patched exceptions save 1–2% more and are not worth a codec change.
2. **LOD.** Use one arc store with a **per-vertex minimum level** (3 bits, 8 levels). Each
   vertex gets a Douglas–Peucker importance in metres, computed per arc with the junction nodes
   fixed. The level column costs **0.44 B per vertex (+11%)**. Separate arc sets for full
   resolution plus six LODs would hold 1.84× the vertices. A topology guard tests the kept
   segments for crossings and adds vertices back until none remain. On clean Nepal admin3 it
   found 2–3 crossings at 100 m and 1 km and fixed them by adding 5–6 vertices. DP beats VW for
   the stored level because its tolerance is a distance bound, and a distance bound can be a
   gate (§3.2).
3. **One arc store.** An admin REGION refers to ARCS by vertex number instead of keeping its own
   X and Y. The arcs are the same: REGION's arc rule matches the topology node rule. On Nepal
   that is 4.47 B per vertex shared, against 9.3 B for two copies. At world scale it saves
   about 0.5 GB per 10⁸ vertices (**estimate**). The cost is one extra checkpoint column pair,
   so a REGION run can start decoding at a block. The timezone REGION keeps its own vertices,
   because it comes from a different source geometry (§3.3).
4. **One arc set for every admin level.** COD-AB Nepal's upper levels miss admin3's vertices at
   0.41–0.43% of their vertices, each at most **1 cm** away: rounding noise from the source
   dissolve. Taken as given, the four levels make 72 crossings at full resolution and about 1,000
   at every LOD. The forge therefore derives each parent level by dissolving its children along
   shared arcs, and checks the source's parent geometry against the result. All four Nepal levels
   then share admin3's 593,610 vertices: **33.8% of the 1,754,996 ring vertices** (§3.4).
5. **POLYGONS** is a column table of polygon → parts → rings → signed arc refs, plus the PLACES
   row, a polylabel point and the area. It has no bbox column, because POLY_INDEX's leaf boxes
   are the bboxes. A reader rebuilds a ring at any LOD with an allocation-free iterator (about
   1.5 KB of state) or into a caller buffer (§3.5).
6. **POLY_INDEX** is PLACE_INDEX's tree code with one change: the leaf level is stored (u16
   boxes inside the parent), because polygons are boxes, not points in PLACES. Queries are
   "boxes intersecting" and "boxes containing a point" (§3.6).
7. **Point → admin unit stays on REGION** (40–50 ns, D-083). POLYGONS exists for geometry
   output, box queries, area and labels. It also offers an exact `contains`, used as the test
   oracle and as the fallback when a pack has no REGION (§3.7).
8. **Robustness.** Use `i_overlay` 9.1.0 with the **i64 engine**. Its default i32 engine accepts
   only ±2³⁰, and world longitudes on our grid reach ±1.8 × 10⁹. Runtime predicates stay our
   exact integer ones (§3.8).
9. **Output.** The core emits grid-integer iterators and a flat GeoArrow-style buffer. It also
   has small dependency-free WKB and GeoJSON writers behind a feature. GeoJSON prints each
   integer ÷ 10⁷ as an exact decimal, so the output is byte-identical on every platform (§3.9).

## 3. Findings

All measurements below are from `topo.py` and `nest.py` (pure Python 3.15.2, run 2026-11-07),
which model the B1 codec's byte cost exactly: 8 B of header per column, and per 128-value block
4 B of base, 4 B of offset, 1 B of width and 16·w B of payload. They are **sizes only**; every
speed claim is an estimate until B10 measures it in Rust. Inputs:

- **COD-AB Nepal v02** GeoJSON (`codab-np-geojson`, SHA-256 matches `data/sources.lock`,
  CC BY-IGO 3.0). Levels: admin0 (1 feature), admin1 (7), admin2 (77) and admin3 (775:
  753 local levels and 22 protected areas). Wards are not in COD-AB.
- **geoBoundaries gbOpen IND ADM2** (736 districts, build 2023-12-12, commit `9469f09`,
  **ODbL 1.0**, source Pathways Data / lgdirectory.gov.in, SHA-256 `8bef6929…46db40`,
  48.3 MB). Downloaded for measurement only. It is not ingested and does not feed any build.

### 3.1 Vertex coding (question 1)

Nepal admin3 after quantising: 778 rings with 1,116,955 vertices, cut into 2,315 arcs at 1,539
nodes. The arcs hold **593,610 vertices, 53.1% of the ring vertices**. Arc length: median 188,
p90 512, max 3,263.

| Coding, arcs in Hilbert order of first vertex | Bytes | B/vertex |
|---|---:|---:|
| raw i32 x, y | 4,748,880 | 8.00 |
| zstd-19 of raw i32 | 3,932,722 | 6.63 |
| **FOR, absolute x and y (REGION today)** | 2,911,564 | **4.90** |
| B1 delta, arcs back to back | 2,631,500 | 4.43 |
| B1 delta, arcs chained end to start (greedy) | 2,483,084 | 4.18 |
| **zigzag delta per arc, FOR blocks + per-arc base columns** | 2,365,250 | **3.98** |
| same, with patched exceptions (PFOR-style) | 2,332,150 | 3.93 |
| varint (LEB128) zigzag delta, x and y interleaved | 2,428,832 | 4.09 |
| StreamVByte zigzag delta | 2,627,000 | 4.43 |
| zstd-19 of the varint stream (no random access) | 2,333,785 | 3.93 |

- **Why plain B1 delta loses.** The jump from one arc's end to the next arc's start is one
  outlier delta, and that outlier sets the bit width of its whole 128-value block. This is the
  effect B9 saw ("arcs jump between blocks"), so REGION kept FOR. Chaining arcs end to start
  removes part of the effect: 4.18 B. Putting a zero at each arc start and keeping the arc's
  absolute start in its own column removes all of it: 3.98 B. The per-arc base columns cost
  0.07 B per vertex.
- **Arc order no longer matters** with a per-arc base: Hilbert order and chain order give the
  same 3.98 B. ARCS can therefore use REGION's order (first-vertex key) at no cost, which §3.3
  needs.
- **Delta widths.** The x deltas are mostly 11–14 bits wide: 12 bits ≈ 4,096 units ≈ 40 m
  between vertices. Fewer than 0.6% exceed 17 bits.
- **Patched exceptions** (one width per block plus outliers at 5 B each) save only 1.2%, so
  they do not justify a fourth B1 coding.
- **Varint and StreamVByte** are byte-aligned. They lose to bit-packing, and neither has
  O(block) random access without an extra index. zstd-19 matches per-arc FOR deltas only after
  a whole-stream decode, so it is ruled out for in-place reads.
- **India ADM2 (geoBoundaries):** the order of the options is the same, but zstd does much
  better there. §3.10 explains why.

### 3.2 LOD: per-vertex level against separate arcs (question 2)

Each arc is simplified on its own, with its two end nodes fixed. Importance is measured in
metres on a local equirectangular scale for each arc.

- **DP importance** is the distance at which the vertex is removed in the recursive DP split,
  capped by its parent's importance so that levels are monotone (mapshaper's scheme).
- **VW importance** is effective area, made monotone the same way. Its threshold is tol² m².
- A closed one-arc ring (an island, or a unit with no neighbour) also pins its farthest vertex,
  so no ring falls below 3 vertices.

Vertices kept, Nepal admin3 (593,610 arc vertices):

| Tolerance m | DP kept | DP % | VW kept (area tol²) | VW % |
|---:|---:|---:|---:|---:|
| 1 | 506,339 | 85.30 | 551,243 | 92.86 |
| 2 | 475,994 | 80.19 | 540,124 | 90.99 |
| 5 | 302,518 | 50.96 | 512,560 | 86.35 |
| 10 | 198,029 | 33.36 | 429,239 | 72.31 |
| 20 | 126,843 | 21.37 | 280,490 | 47.25 |
| 50 | 69,546 | 11.72 | 140,894 | 23.74 |
| 100 | 43,320 | 7.30 | 80,255 | 13.52 |
| 200 | 26,085 | 4.39 | 44,499 | 7.50 |
| 500 | 13,243 | 2.23 | 20,297 | 3.42 |
| 1,000 | 8,335 | 1.40 | 11,555 | 1.95 |
| 2,000 | 5,903 | 0.99 | 7,147 | 1.20 |
| 5,000 | 4,790 | 0.81 | 4,978 | 0.84 |

- Half the source vertices lie within 5 m of the DP line through their neighbours: the source is
  densified well beyond its accuracy. A 1 m level already drops 15%.
- A web-map pixel at latitude 28° is about 138,200 / 2^z m: 34 m at z12, 540 m at z8, 8.6 km at
  z4. A level ladder of **full, 5, 20, 100, 500, 2,000, 5,000 m** (3 bits, the 8th value for
  pinned vertices) covers z4 to z14+ with the 1–5 m step that analysis wants.
- **The level column**, in B1 FOR blocks in arc order, costs **0.440 B per vertex** (261,110 B).
  The levels are 0 for 49% of vertices and 1 for 30%; blocks of fine detail pack at 2 bits.
- **Separate arc sets** for full resolution plus the six tolerances hold **1.84×** the vertices
  (India 2.03×). Decimated deltas are also wider, so each vertex costs more. Coded as its own
  per-arc delta set:

  | Tolerance m | 5 | 20 | 100 | 500 | 2,000 | 5,000 |
  |---|---:|---:|---:|---:|---:|---:|
  | B/vertex | 4.28 | 4.67 | 5.28 | 6.48 | 8.25 | 8.91 |
  | KB | 1,295 | 593 | 229 | 86 | 49 | 43 |

  The six sets together are 2.29 MB, as much as the full-resolution arcs (2.37 MB). The
  per-vertex level costs 0.26 MB (+11%) and stores every vertex once.
- **What the level costs at read time:** reading a coarse level still decodes every vertex of
  the arcs it touches, because the filter runs after decode. At about 1 ns per value
  (**estimate** from B1's block decode), a whole-world render at z0–z4 would decode 10⁸
  vertices, about 0.1–0.3 s. A separate **overview** arc set for the two coarsest levels holds
  about 1% of the vertices (0.81–0.99% on Nepal). It costs 91 KB on Nepal, 3.8% of ARCS, and
  removes that cost. B10 decides it as an A/B.
- **The topology guard** (DP, Nepal admin3): kept segments are tested for crossings with exact
  integer orientation tests, using a segment grid. For each crossing segment, the forge adds
  back the most important removed vertex and tests again.

  | Tolerance m | Kept | Crossings | Vertices added | Left |
  |---:|---:|---:|---:|---:|
  | 0 (full) | 593,610 | 0 | 0 | 0 |
  | 10 | 198,029 | 0 | 0 | 0 |
  | 100 | 43,320 | 2 | 6 | 0 |
  | 1,000 | 8,335 | 3 | 5 | 0 |

  India ADM2 shows the same pattern at a larger scale:

  | Tolerance m | Kept | Crossings | Vertices added | Left |
  |---:|---:|---:|---:|---:|
  | 0 (full) | 1,122,129 | 4 | 0 | 4 |
  | 10 | 442,732 | 7 | 7 | 3 |
  | 100 | 169,827 | 43 | 71 | 2 |
  | 1,000 | 35,445 | 98 | 185 | 2 |

  - The 4 crossings at full resolution are defects in the source. They are the forge's
    validation job (snap and repair), not the simplifier's.
  - The crossings left after repair are those same defects. The guard adds back 0.5% of the
    kept vertices at 1 km.

  Shared arcs with fixed nodes rule out gaps and overlaps between neighbours by construction.
  The guard catches what remains: an arc crossing a neighbouring arc, or crossing itself, where
  units are narrow.
- **DP or VW.** For the same vertex budget VW looks smoother, which is why mapshaper defaults to
  weighted VW. DP's importance is a distance, so a level is a stated bound ("every removed
  vertex is within 100 m of the kept line") that `ACCURACY.md` can gate. Store DP. The level
  format does not care which algorithm produced it, so a later change to VW is a forge change,
  not a format change.

### 3.3 One arc store for REGION and ARCS (question 3)

- **REGION's writer** joins edges into arcs through vertices where exactly two edges of one
  region set meet (D-083). That is the topology node rule (degree ≠ 2, or the owner set
  changes). An admin REGION over the same polygons therefore has the same arcs as ARCS.
- **Size on Nepal admin3:**

  | Layout | B/vertex | Nepal MB | World at 10⁸ vertices (estimate) |
  |---|---:|---:|---:|
  | Separate: REGION X/Y FOR (4.90) + ARCS deltas and level (4.42) | 9.32 | 5.5 | 0.93 GB |
  | **Shared: ARCS deltas, level and checkpoints; REGION refers to vertex numbers** | 4.47 | 2.7 | 0.45 GB |

  The checkpoint pair (the absolute x and y at each 128-vertex block start, FOR) adds about
  0.05 B per vertex. With it, any vertex is at most one block decode away, which REGION's run
  reads need.
- **Cost of sharing:**
  - A REGION run read decodes from its block start instead of reading FOR values one by one.
    B9 measured that a short run read value by value costs a fifth of a block decode
    (`SHORT_RUN`), so a short run may become slower. B10 must measure REGION query time with
    delta arcs on Nepal places and uniform points against D-083's 41/50 ns.
  - A REGION-only pack must carry ARCS, but without the level column.
- **The timezone REGION** comes from timezone-boundary-builder, whose geometry does not share
  vertices with our admin arcs, so it keeps its own X and Y. Once admin boundaries are built
  from the same topology, timezones could be built from admin arcs where they follow them. That
  is a later forge option, not a format need.
- **Recommendation: share.** This follows the DRY rule of AGENTS §11 and saves half of the
  admin geometry bytes. The REGION format gains an "arcs external" flag (or a column absent
  means "use ARCS"). It can fall back to its own X and Y if B10's timing fails D-083's budget.

### 3.4 Nesting across admin levels (forge)

All four COD-AB Nepal levels, taken as published:

- 860 polygons, 863 rings, 1,754,996 ring vertices.
- Of admin0's, admin1's and admin2's edges, 99.19%, 99.15% and 99.13% are also admin3 edges.
- The 0.41–0.43% of upper-level vertices that are not admin3 vertices are at p50 0.7 cm and
  max 1.0 cm from an admin3 edge (`nest.py`). The levels were dissolved from admin3 and lost a
  digit on export. They nest, but not to the grid.
- Built as one topology, that sub-grid noise produces 5,394 arcs, 598,752 vertices (34.1%), **72
  crossings at full resolution**, and about 1,000 at every LOD (nearly parallel duplicate
  lines). The guard cannot fix those in 50 rounds: 70 remain.

Rule for the forge, recorded per source:

- When a source's levels are meant to nest, the forge builds every parent from its children by
  dissolving along shared arcs. A parent ring is then a list of the children's arcs.
- The forge checks the source parent against the derived one by area difference and Hausdorff
  distance (here ≤ 1 cm), and reports any unit that differs by more than the snap tolerance.
- On Nepal, all four levels then use admin3's 593,610 vertices: 33.8% of the ring vertices,
  2.96× fewer. Wards nest the same way under local levels.

When levels come from different sources (OSM wards under COD-AB local levels), snap rounding
and noding come first ([forge §3.4](forge-pipeline.md#34-topology-for-boundaries)). Then comes
a dissolve or clip of the children to the parent, chosen by the source's precedence.

### 3.5 POLYGONS layout and ring rebuild (question 4)

**Polygons are PLACES rows.** Every admin unit is a place, so a polygon is listed by its PLACES
row.

- Polygons are sorted by (admin level, row). Within a level they follow the rows' Hilbert order,
  which is close to the Hilbert order of the box centres that POLY_INDEX wants (§3.6).
- Row → polygon is a binary search inside the level's range.

**Ring model (TopoJSON):**

- A ring is a list of arc refs `2·arc + reversed`.
- Each arc after the first drops its first vertex, which repeats the previous arc's last.
- The first ring of a part is its outer ring and the rest are holes. Orientation is not used.
  The forge writes outer rings counter-clockwise and holes clockwise, as RFC 7946 recommends,
  and the deep check verifies it.

**Ring rebuild, allocation-free:**

- `Polygons::ring(poly, part, ring, lod)` returns an iterator of grid points. It holds the
  current ref, one decoded block of dx, dy and level (3 × 128 u32, 1.5 KB), and the running
  x and y.
- **Forward arcs** start from the arc base and sum the deltas.
- **Reversed arcs** start from the block checkpoint, decode the block forward, and walk it
  backwards. Without checkpoints, reversing an arc needs its end value: a stored END column or
  one summing pass.
- Vertices whose level is below `lod` are skipped. Arc ends are always kept, so neighbouring
  polygons stay coincident at every LOD.
- `copy_ring(…, out: &mut [[i32; 2]]) -> Result<usize, Needed>` fills a caller buffer.
  `ring_len(…, lod)` sizes it: it reads only the level column, or a per-arc count per level if
  B10 shows that matters.

**Label point and area:**

- The label point is polylabel's pole of inaccessibility, computed by the forge (`polylabel`
  3.3.1, BlueOak-1.0.0) on the largest part at full resolution.
- Area is in hectares, rounded, as u32. 2³² ha = 4.3 × 10⁷ km², above any single unit (Russia
  1.7 × 10⁷ km²). It is computed on the ellipsoid by the forge.

**Size on Nepal:**

- As published (4 levels, 15,916 arc refs): refs 23.5 KB, the start columns 3 KB.
- Admin3 alone: 4,479 refs, 5.8 KB.
- POLYGONS is about 1% of ARCS.

### 3.6 POLY_INDEX (question 5)

- PLACE_INDEX stores no leaf level, because its leaves are PLACES points (D-082). Polygons are
  boxes, so POLY_INDEX stores a leaf level: four u16 offsets per polygon inside its leaf node's
  box. Everything else is PLACE_INDEX's code: node size, exact root, u16 boxes rounded outward,
  top-down quantising, the canonical deep check, and the antimeridian split.
- **Size:** about 8 B × n × (1 + 1/15), so 9 MB at 10⁶ polygons (**estimate**; B1 packs u16
  offsets below 16 bits when boxes are small in their parent).
- **Queries:**
  - `intersecting(box)`: boxes overlapping a viewport, then an optional exact clip test.
  - `containing(point)`: candidates for `Polygons::contains`.
  - kNN on boxes is not needed in v1.
- **Code change:** generalise the reader over a leaf source, points from PLACES or a stored box
  level, so both indexes share one implementation (DRY).
- **Mixed levels:** a country box overlaps every district box. Sorting by level first keeps each
  level's leaves in their own subtrees, and queries take a level filter. B10 compares this with
  one Hilbert sort over all polygons, counting nodes visited.

### 3.7 Point in polygon: REGION or POLYGONS (question 6)

- **REGION** answers point → every region holding it, exactly, in 41–50 ns, with sets of up to
  64 regions (D-083, D-084). One admin REGION over all levels returns country, province,
  district, local level and ward in one query: 5 regions, well under `MAX_SET`.
- **POLYGONS** point-in-polygon means POLY_INDEX candidates plus an even–odd crossing count over
  every ring vertex. A Nepal local level averages about 1,440 ring vertices, so this costs
  several µs (**estimate**), 100× REGION.
- **Recommendation:** reverse geocoding and point → admin use REGION. POLYGONS is for geometry
  (export, rendering, area, label, clip). `Polygons::contains` stays as the oracle for REGION's
  admin tests and as the fallback in packs without REGION. Admin REGION goes into Phase 3 with
  B10, because ARCS is the vertex store it would share.

### 3.8 Robustness and how others do it (question 7)

- **`i_overlay` 9.1.0** (2026-10-14, MIT or Apache-2.0, MSRV 1.88): integer boolean operations
  with i16, i32 and i64 engines. Intersections are rounded to the grid, and "snapping selects an
  existing endpoint" (its integer-range audit). It has `EdgeOverlay` with per-edge payloads,
  which keep provenance through splits (useful for owner sets), and an `ogc` option for valid
  output.
  - **Pitfall:** an N-bit engine needs every coordinate in −2^(N−2)..2^(N−2)−1. For i32 that is
    ±1,073,741,823, but our longitudes reach ±1,800,000,000. **Use the i64 engine**, or
    translate each tile to a local origin. Its default is i32, and integer APIs do not check the
    range.
  - It is not Hobby/Hershberger snap rounding, which also moves vertices near a hot pixel. The
    forge's snap step (tolerance per source) must come before the overlay.
- **`geo` 0.33.1** (2026-04-20) builds its `BooleanOps` on `i_overlay`.
  - `SimplifyVwPreserve` keeps topology only within one geometry. Neighbours simplified apart
    open gaps, which is why we simplify arcs, not polygons.
  - **`geo-types` 0.7.20** (2026-08-02) is the type for a `geo` conversion feature.
  - **`robust`** 1.2.0 (orient2d, `no_std`) for the forge's float inputs. At runtime we use exact
    i64/i128 on integers, as REGION already does.
- **mapshaper** builds arcs first, then simplifies with weighted Visvalingam by default (DP and
  plain VW are options), with `interval=` in distance units. It repairs the intersections that
  simplification introduces, and `-clean gap-width=` fills slivers. Shared arcs are simplified
  once (its reference, accessed 2026-11-07).
- **TopoJSON:** quantised arcs, delta-coded. A negative ref `~i` means reversed. An arc's first
  point equals the previous arc's last and is dropped on rebuild. `toposimplify` stores a weight
  per point, the per-vertex level idea.
- **Tippecanoe:**
  - `--detect-shared-borders` simplifies borders shared between polygons identically.
    `--no-simplification-of-shared-nodes` keeps shared nodes, but it is not effective with
    `--detect-shared-borders` or `--coalesce`.
  - Without the flag, polygons simplify apart and gaps appear at low zooms.
  - Vector tiles store a separate geometry per zoom, our rejected option B, and pay for it in
    tile size.
  - felt/tippecanoe is the active fork.
- **Overture divisions:**
  - `division_area` polygons, plus `division_boundary` lines for shared borders, with
    `division_ids` [left, right] by line direction.
  - `is_disputed` and `perspectives` (`accepted_by` / `disputed_by`).
  - So Overture also stores a shared border once with its sides, the arc model. Perspectives sit
    on the boundary, which fits ADR-0006: a perspective's polygons use their own arcs where
    claims differ (Nepal: Kalapani–Lipulekh, Susta).
- **geoBoundaries:**
  - It publishes full and `_simplified` files per country and level, and its licences vary by
    country (IND ADM2 ODbL; CHE swisstopo licence; BGD CC BY-IGO).
  - How it simplifies is **unverified**; it is said to use mapshaper.
- **Known pitfalls:**
  - Simplifying in degrees instead of metres: east–west tolerance shrinks with cos(latitude).
    We scale per arc.
  - Rings collapsing below 3 vertices: we pin vertices.
  - Small islands vanishing at coarse levels: keep them by an area policy, or drop them on
    purpose and flag it.
  - Quantising after building topology breaks it: the grid comes first (ADR-0029).
  - Upper levels that nest only approximately: see §3.4.
  - Antimeridian polygons (Fiji, Chukotka) stay split at ±180°, as RFC 7946 §3.1.9 requires;
    REGION already reads 180 as −180.
  - Float GeoJSON output that differs across platforms: see §3.9.

### 3.9 Output formats (question 8)

| Output | For | Core? |
|---|---|---|
| Iterator of `[i32; 2]` grid points per ring, at a LOD | everything else is built on it; zero-copy | core, always |
| Flat buffers: coordinates (i32 or f64, separated x and y) plus ring, part and polygon offsets, GeoArrow 0.2 layout | JS `Float64Array` over wasm memory, Python `shapely.from_ragged_array`, Arrow, deck.gl; one copy, no per-vertex objects | core, feature `geometry` |
| WKB (ISO, little-endian, MultiPolygon, f64) into a caller buffer | PostGIS, GDAL, GEOS, shapely, DuckDB | core, feature `geometry-io`, our own writer (about 100 lines) |
| GeoJSON text (RFC 7946) | web, debugging, `teigeodata` CLI | core, feature `geometry-io`; coordinates printed as integer ÷ 10⁷, exact and the same on every platform |
| `geo-types` conversion | Rust users of the georust stack | optional feature `geo` |
| TWKB, encoded polyline, MVT | niche or tiling | bindings or a later crate |

`wkb` 0.9.3 and `geojson` 1.0.0 exist, but each would add `std` and allocation to the reader
path. Our writers fit in a few hundred lines with no dependencies. The crates serve as test
oracles: their parsers must read our output.

### 3.10 India ADM2 (geoBoundaries)

The input: 735 polygons, 1,072 rings and 1,995,009 ring vertices, cut into 2,592 arcs that hold
1,122,129 vertices (**56.2%**). Arc length: median 250, p90 974, max 19,164.

| Coding (Hilbert arc order) | 1e-7° grid B/vertex | Source's 1e-5° grid B/vertex |
|---|---:|---:|
| FOR, absolute x and y | 5.13 | 3.47 |
| B1 delta, back to back (chained) | 4.60 (4.46) | n/a |
| **per-arc zigzag delta, FOR blocks + bases** | **4.34** | **2.68** |
| patched exceptions | 4.27 | n/a |
| varint / StreamVByte | 4.56 / 4.56 | n/a |
| zstd-19 of varint (no random access) | 2.78 | 2.38 |

- **What zstd found** is the source's grid: 98.2% of x values are multiples of 100 units, so
  every delta carries about 6.6 zero bits that B1 stores. That is a property of the source, not
  of zstd.
- **The fix:** an optional per-arc scale exponent `k`, where the deltas are stored ÷ 10^k and
  the base stays absolute. This recovers most of the gap, 4.34 → 2.68 B, which is 13% above
  zstd's 2.38 B on the same grid.
- **The forge sets `k`** to the largest power of ten that divides every vertex of the arc. An
  arc that snapping or noding moved off the source grid gets `k = 0`. On a shared border
  between sources of different precision, `k` is the smaller of the two.
- **Effect on the world estimate:** many world sources (geoBoundaries, some national datasets)
  are at 1e-5° or 1e-6°, so this matters there. COD-AB Nepal is at full precision (only 10% of
  values are multiples of 10), so it gains nothing.
- **LOD on India:** DP keeps 39.5% of vertices at 10 m, 15.1% at 100 m and 3.2% at 1 km. India
  is denser than Nepal at fine tolerances: DP keeps 67% at 1 m against Nepal's 85%. The level
  column costs 0.442 B per vertex, the same as Nepal's.

## 4. Options compared

| Question | Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|
| Vertex coding | Absolute FOR x, y | O(1) access, built | 4.90 B/v | REGION fallback only |
| | B1 delta, arcs back to back (or chained) | built, O(block) access | 4.43 (4.18 chained) B/v | no |
| | **Per-arc zigzag delta in FOR blocks + arc bases + block checkpoints** | 3.98 (+0.05) B/v, no codec change, order-free | O(block) access, not O(1) | **adopt** |
| | + per-arc scale exponent | India 4.34 → 2.68 B/v on its 1e-5° source grid; 0 on Nepal | one small column, forge must track the grid | **adopt (optional column)** |
| | Patched exceptions | 3.93 B/v | new B1 coding for 1% | no |
| | varint / StreamVByte | simple | 4.09 / 4.43 B/v, no block access | no |
| | zstd | 3.93 B/v | whole-stream decode, not in place | no |
| LOD | **Per-vertex level (3 bits)** | each vertex once; +0.44 B/v; any level is a filter | coarse reads decode fine vertices | **adopt** |
| | Separate arc sets per LOD | fast coarse reads | 1.84× vertices, wider deltas | overview only (A/B) |
| Simplifier | **DP with fixed nodes + crossing guard** | distance bound, gateable | less smooth than VW | **adopt** |
| | Weighted VW (mapshaper default) | visually smoother | area bound, not distance | later forge option |
| Arc store | **Shared ARCS for admin REGION** | 4.47 against 9.32 B/v | REGION run decode is block-based | **adopt if B10 timing holds** |
| | Separate | REGION untouched | 2× admin geometry | fallback |
| Point → admin | **REGION** | 41–50 ns exact | builder work done | **adopt** |
| | POLYGONS PIP | no extra section | µs per query | oracle and fallback |
| Overlay | **`i_overlay` 9.1.0, i64 engine** | robust integer overlay, edge payloads | not full snap rounding; i32 range trap | **forge** |
| | `geo` BooleanOps | same engine, f64 API | floats in, scaling hidden | no |

Licences: `i_overlay`, `geo`, `geo-types`, `robust`, `wkb` and `geojson` are MIT or
Apache-2.0; `polylabel` is BlueOak-1.0.0 (permissive). All are used by the forge or as test
oracles; the reader gains no dependency.

## 5. What we build ourselves, and why

The reader for each section is our own. It reads in place, is `no_std`, does not allocate, and
uses the B1 codec and column table, as every Phase 1–2 section does. No crate offers shared arcs
with per-vertex levels in place: `topojson` 0.5.1 dates from 2022 and owns its memory. The forge
uses `i_overlay` (i64), `robust` and `polylabel`. It does its own arc building, dissolve, DP
importance and crossing guard, which are small and must be deterministic.

### Draft section layouts (column tables, encoding 1)

**ARCS (kind 7)**, `item_count` = number of arcs. Vertices are numbered across all arcs; arc
`a` holds vertices `START[a]` to `START[a+1] − 1`, at least 2.

| ID | Column | Values |
|---:|---|---|
| 1 | START | arcs + 1 values: first vertex of each arc, then the vertex count (ramp or FOR) |
| 2 | BASE_X | per arc: longitude of its first vertex, 1e-7°, biased |
| 3 | BASE_Y | per arc: latitude, biased |
| 4 | DX | per vertex: zigzag(x − previous x in the arc); 0 at an arc's first vertex (FOR) |
| 5 | DY | per vertex: the same for y |
| 6 | CHK_X | per 128-vertex block: absolute x of the block's first vertex (optional; required when REGION refers to ARCS) |
| 7 | CHK_Y | the same for y |
| 8 | LEVEL | optional, per vertex: highest LOD that keeps it, 0 to 7; 7 = always (arc ends, pins); absent = full resolution only |
| 9 | TOLERANCE | optional, 6 values: DP tolerance in cm of levels 1 to 6 (default ladder 500, 2,000, 10,000, 50,000, 200,000, 500,000); LOD `k` keeps vertices with LEVEL ≥ `k` |
| 10 | SCALE | optional, per arc: k, deltas of this arc are stored ÷ 10^k (0 to 7; absent = 0) |

Deep check: the bases equal the decoded first vertices, the checkpoints equal the decoded
values, levels are monotone with arc ends at 7, and vertices are on the domain.

**POLYGONS (kind 8)**, `item_count` = number of polygons, sorted by (admin level, row).

| ID | Column | Values |
|---:|---|---|
| 1 | ROW | per polygon: its PLACES row |
| 2 | PARTS | polygons + 1: first part |
| 3 | RINGS | parts + 1: first ring; a part's first ring is the outer one |
| 4 | REFS_START | rings + 1: first arc ref |
| 5 | REFS | per ref: 2·arc + reversed |
| 6 | LABEL_X | per polygon: polylabel longitude, 1e-7°, biased |
| 7 | LABEL_Y | latitude |
| 8 | AREA | per polygon: ellipsoidal area in hectares, rounded |
| 9 | LEVEL_START | optional: per admin level present + 1, the first polygon of that level |

Deep check: refs in range, consecutive arcs meet end to start, rings close, outer rings are
CCW and holes CW, and each hole lies in its outer ring. That last test is O(vertices) and run by
the forge and the tests.

**POLY_INDEX (kind 9)**, `item_count` = POLYGONS'. These are PLACE_INDEX's columns 1–3, with
leaf boxes included as level 0 of `boxes`: four u16 offsets per polygon inside its node's box.

## 6. Impact on the plan

Decisions the plan should record (D-numbers to be assigned):

1. **ARCS coding:** per-arc zigzag deltas in B1 FOR blocks, arc bases, block checkpoints, an
   optional per-arc decimal scale exponent, and a 3-bit per-vertex DP level. No new B1
   coding.
2. **One arc store:** an admin REGION refers to ARCS vertices. The timezone REGION keeps its
   own X and Y. If B10's timing fails D-083's budget, REGION falls back to its own FOR X and Y.
3. **The forge derives parent levels by dissolving children when a source nests,** and gates the
   difference from the source parents. Measured on COD-AB: ≤ 1 cm.
4. **`i_overlay` runs with the i64 engine** in the forge. Add it to
   [versions](../../06-engineering/versions.md).
5. **Point → admin uses REGION.** POLYGONS is geometry, and admin REGION is built in Phase 3.
6. **The core's geometry outputs:** iterator, GeoArrow-style flat buffers, and our own WKB and
   GeoJSON writers.

Pages to update when B10 lands: the [pack format](../../04-architecture/pack-format.md) rows 7–9
and REGION's "keeps its own vertices" line; the
[forge note §3.4](forge-pipeline.md#34-topology-for-boundaries) (i64 engine, dissolve rule, DP
over VW); and the phase 3 plan.

**B10 must still measure in Rust:**

- ARCS bytes per vertex on Nepal (all levels, plus OSM wards when ingested) and India, against
  this note's model numbers.
- Ring decode time per vertex, forward and reversed, at full resolution and at each level.
- Whole-world coarse render time with and without an overview set.
- REGION query time with external delta arcs against D-083's 41/50 ns, on Nepal places and
  uniform points.
- POLY_INDEX size and nodes visited: level-first order against one Hilbert sort.
- `Polygons::contains` time against REGION.
- The crossing guard's output on all Nepal levels after the dissolve, and at world scale on a
  large country.
- WKB and GeoJSON round trips through `wkb` and `geojson` as oracles.

### Outcome (2026-11-08, D-097, D-098)

Built in Rust and measured on COD-AB Nepal, all four levels: 2,315 arcs and 593,610 vertices
(33.8% of the ring vertices) as modelled; ARCS 4.48 B per vertex (model 3.98 + 0.44 level +
0.05 checkpoints = 4.47); the dissolved parents lie within 1 cm of the source's; the guard
found 8 crossings over all levels and fixed them with 29 vertices. Checkpoints became required
and the label is our own polylabel (see D-097, D-098). POLY_INDEX (D-099, [B10](../benches/phase-3.md#b10-poly_index-and-ring-reads)): 9.5 B per
polygon (the estimate was 8.5); level-first order reads 3 to 4.5 nodes per point per level
against 5.7 for one Hilbert sort; ring reads cost 6.6 ns per vertex at full resolution, and a
coarse level costs as much as the full one because every vertex is still decoded (an open item
for 3.3). `containing` through POLYGONS takes 9 µs (local level) to 354 µs (country), so
point → admin stays on REGION as §3.7 recommends. The admin REGION on the shared arcs
(D-100): 977 KB against 3.63 MB with its own vertices, the same answers, 155 against 153 ns, so
the shared store wins as §3.7 expected. Still to measure: India.

## 7. Risks and open items

- **The scale exponent breaks** wherever snapping or noding moves a vertex off the source grid.
  Such arcs get `k = 0`, and B10 measures how many lose it after noding.
- **Pack downloads** may also be zstd-compressed in transport (India 2.78 against 4.34 B/v),
  so the B1 layout costs nothing extra on the wire. Whether transport compression is used is
  for the [offline-loading note](offline-loading-and-trust.md) to say; it is not checked here.
- **Short-run REGION reads may slow down with delta arcs.** Mitigation: keep the FOR fallback
  (decision 2).
- **The guard is O(segments) per round with a grid.** On 10⁸ vertices it must run per tile or
  per country, in bounded memory. Only cross-border arcs need a global pass.
- **Wards** come from a different source (OSM `admin_level=9`) than local levels (COD-AB), so
  snap and clip come first (§3.4). Their nesting must be measured (Q-009).
- **Disputed boundaries:** perspective-specific arcs and polygons per ADR-0006. The arc model
  supports this, but it has not been designed in detail.
- **All speed numbers here are estimates.** All sizes are modelled in Python, not built.
- **geoBoundaries' simplification method** is unverified.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `i_overlay` | 9.1.0 (2026-10-14), MSRV 1.88 | 2026-11-07 | crates.io API, README |
| `geo` | 0.33.1 (2026-04-20) | 2026-11-07 | crates.io API |
| `geo-types` | 0.7.20 (2026-08-02) | 2026-11-07 | crates.io API |
| `robust` | 1.2.0 (2025-05-10) | 2026-11-07 | crates.io API |
| `polylabel` | 3.3.1 (2026-11-06), BlueOak-1.0.0 | 2026-11-07 | crates.io API |
| `geo-index` | 0.4.0 (2026-09-15) | 2026-11-07 | crates.io API |
| `wkb` | 0.9.3 (2026-10-14) | 2026-11-07 | crates.io API |
| `geojson` | 1.0.0 (2026-03-16) | 2026-11-07 | crates.io API |
| `geozero` | 0.15.1 (crate updated 2025-12-11) | 2026-11-07 | crates.io API |
| `flatgeobuf` | 6.0.1 (crate updated 2025-12-28) | 2026-11-07 | crates.io API |
| `topojson` (Rust) | 0.5.1 (2022-06-25), stale | 2026-11-07 | crates.io API |
| GeoArrow spec | 0.2 | 2026-11-07 | geoarrow.org |
| COD-AB Nepal | v02, HDX resource of 2026-08-14 | 2026-11-07 | `data/sources.lock` |
| geoBoundaries IND ADM2 | gbOpen, build 2023-12-12, commit 9469f09 | 2026-11-07 | geoboundaries.org API |

## 9. Sources

- [iOverlay README and integer range audit](https://github.com/iShape-Rust/iOverlay), iShape-Rust, accessed 2026-11-07.
- [crates.io API](https://crates.io/api/v1/crates/i_overlay) for every crate in §8, accessed 2026-11-07.
- [mapshaper command reference](https://mapshaper.org/docs/reference.html), M. Bloch, accessed 2026-11-07.
- [TopoJSON specification](https://github.com/topojson/topojson-specification), M. Bostock, accessed 2026-11-07.
- [Tippecanoe README](https://github.com/mapbox/tippecanoe), Mapbox; active fork [felt/tippecanoe](https://github.com/felt/tippecanoe), accessed 2026-11-07.
- [Overture divisions guide](https://docs.overturemaps.org/guides/divisions/) and [division_boundary schema](https://docs.overturemaps.org/schema/reference/divisions/division_boundary/), accessed 2026-11-07.
- [geoBoundaries API, gbOpen IND ADM2](https://www.geoboundaries.org/api/current/gbOpen/IND/ADM2/), William & Mary geoLab, accessed 2026-11-07.
- [OCHA COD-AB Nepal](https://data.humdata.org/dataset/cod-ab-npl), HDX, resource of 2026-08-14.
- [GeoArrow format 0.2](https://geoarrow.org/format.html), accessed 2026-11-07.
- [RFC 7946, GeoJSON](https://www.rfc-editor.org/rfc/rfc7946), IETF, 2016.
- Douglas and Peucker, *Algorithms for the reduction of the number of points required to represent a digitized line*, Cartographica 1973.
- Visvalingam and Whyatt, *Line generalisation by repeated elimination of points*, Cartographic J. 1993.
- Saalfeld, *Topologically consistent line simplification with the Douglas–Peucker algorithm*, CaGIS 1999.
- Hobby, *Practical segment intersection with finite precision output*, CGTA 1999; Hershberger, *Stable snap rounding*, CGTA 2013.
- Lemire and Boytsov, *Decoding billions of integers per second through vectorization*, SPE 2015 (BP128, PFOR).
