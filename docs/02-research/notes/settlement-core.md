---
title: "Settlement core and consensus point for populated places"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Settlement core and consensus point for populated places

## 1. Question

How does the forge find a populated place's **settlement core** (the centre of its buildings) and
its **consensus point**, in the main build and in the clean-room lite build, at world scale?

It feeds Phase 3 geo-fix ([forge-pipeline §3.3](forge-pipeline.md#33-geo-fix)) and the
accuracy target of Q-005: a populated place is P90 within 500 m of its settlement core below 5°
of slope and within 150 m at 5° or steeper (D-062). S5 found that independent sources put
villages P50 360–1,100 m and P90 2–5 km apart (D-056,
[S5](../benches/s5-nepal.md#stated-links-and-position-disagreement-s55-first-part)), so the
buildings are the lever. Five sub-questions:

1. Which building sources, under which licence, for main and for lite.
2. Which method finds the core, with which bandwidth, and how a peak is assigned to one of
   several nearby places; what happens with no buildings.
3. Measured on a Nepal sample.
4. How the core enters the weighted Weiszfeld consensus, and what truth validates it.
5. World scale: streaming, bounded memory, determinism, cost.

## 2. Summary and recommendation

- **Sources.** Main: OSM buildings + Microsoft Global ML Building Footprints + Google Open
  Buildings v3, each read from its own publisher. Lite: Google Open Buildings v3 (CC BY 4.0
  option) + Microsoft (CDLA-Permissive 2.0), read direct, never through Overture or VIDA.
  Population rasters (WorldPop, GHSL) are not used for the core: they are coarser (100 m and
  more) and WorldPop R2025A lists OSM roads (ODbL) among its covariates, so under CLEAN_ROOM
  rule 6 it is main-only.
- **Method.** Gaussian mean-shift on building centroids binned onto a 25 m grid, bandwidth
  h = 150 m for villages (75 m for hamlets, 300 m and more for towns, by class), **restricted to
  the place's own assignment cell**. The assignment cell is the Voronoi cell of the place's
  pre-core consensus point among all conflated populated places. This is the decisive step.
  Nepal's `GeoNames` has a populated place every 423 m (P50 nearest-neighbour spacing), and
  without the restriction 41% of village cores land nearer a different place.
- **Measured (Nepal, 2026-11-07, 600 `GeoNames` places, three building sources):**
  - The restricted core moves a `GeoNames` point P50 122–143 m and P90 294–304 m.
  - The core from one building source agrees with the core from another at P50 22–28 m and
    P90 131–179 m.
  - From a `GeoNames` seed and from the linked OSM seed, the same core is found within 25 m in
    94–100% of pairs (n = 30–32, small).
  - Unrestricted variants are worse. The other options (a highest peak within 1 km, a DBSCAN
    centroid, the nearest building) are unstable or meaningless.
  - Only 26–33% of places have one unambiguous density peak within 1 km.
- **No buildings.** No core. That was 3–9% of the sample, depending on the building source. The
  consensus then uses the sources alone and widens σ. Never invent a core from a population
  raster or polylabel.
- **Consensus.** The core is one more observation in the weighted Weiszfeld median, with
  σ ≈ 75 m per axis (**estimate**, from the cross-source P90). The core is also the reference of
  the Q-005 target, so **`ACCURACY.md` must not be measured against our own core**. Truth is a
  held-out, hand-labelled golden set (§3.4).
- **World scale.** Process per tile with a halo, on dense integer rasters of counts, then a
  separable Gaussian blur and steepest ascent. This is O(cells), bounded memory and
  bit-deterministic. It is about 10 CPU-hours for the planet's buildings (**estimate**).

## 3. Findings

### 3.1 Building sources and licences (checked live 2026-11-07)

- **OSM buildings**: ODbL 1.0. From the pinned extract `geofabrik-np` (2026-11-05) we
  extracted **8,296,736 building ways** for Nepal. That took 13 s and 4.1 GB peak with DuckDB
  2.0.0 `ST_ReadOSM`, centroid = vertex mean. Main only.
- **Microsoft Global ML Building Footprints**: the README and LICENSE now say "licensed by
  Microsoft under the CDLA Permissive 2.0" (the LICENSE file misspells it "CLDA").
  - The last refresh was 2026-08-13: 30,340 tiles in 225 regions, about 1.4 B buildings from
    2014–2024 imagery. Nepal was added on 2024-03-26: 7 M edits from 2019–2023 imagery.
  - Nepal is 52 quadkey-9 tiles, 548 MB gzipped GeoJSON lines. Each row carries `confidence`
    (−1 for older rows) and `height`.
  - South Asia evaluation: precision 94.8%, recall 76.7% (README).
  - Terms (CDLA-P 2.0): §2.1 share the agreement text with shared Data; §3.1 "does not impose
    any restriction or obligations with respect to the use, modification, or sharing of
    Results". Both builds.
  - Overture's attribution page (updated 2026-05-15) still lists this source as ODbL. That label
    applies to Overture's copy, so read Microsoft's own blob store, as
    [source-licences](source-licences.md) already says.
  - Hosting moved twice (2024-11-07, 2026-07-24). The file URLs in `dataset-links.csv` now
    point at `bfppub.z5.web.core.windows.net`. The source must be pinned per tile, as for DEM
    tiles (D-061).
- **Google Open Buildings v3**: "As the user, you can pick which of the two licenses you
  prefer": CC BY 4.0 or ODbL v1.0.
  - It has 1.8 B detections, inference May 2023, filtered to confidence ≥ 0.65. Per-S2-level-4
    thresholds for 80/85/90% precision are in `score_thresholds_s2_level_4.csv`.
  - v3 is still current: no v4, and the objects are dated 2023-06-23.
  - Nepal is covered. Files come as S2 level 4 or level 6 cells, as polygons or points. The
    points file for level-6 cell `3995` (central Nepal) is 67.6 MB with 2,692,010 rows
    (`lat, lon, area_in_meters, confidence, plus_code`).
  - Main may take either option. Lite takes the CC BY option and cites Sirko et al. 2021.
- **Open Buildings 2.5D Temporal v1**: annual rasters for 2016–2023 with building presence,
  height and fractional count. Effective resolution is 4 m; files are 50 cm. Dual licence
  CC BY 4.0 / ODbL; Nepal is covered. Earth Engine `GOOGLE/Research/open-buildings-temporal/v1`
  and GCS `open-buildings-temporal-data/v1`; citation arXiv:2310.11622.
  - Useful later for the temporal "is this place still inhabited" check and for counts where
    vectors miss buildings.
  - Not needed for the core: v3 vectors are finer. Lite-eligible.
- **Overture buildings** (release 2026-10-21.0, next 2026-11-18.0): theme licence **ODbL**.
  - Its sources are OSM (ODbL), Microsoft (listed ODbL), Google (CC BY 4.0), Esri Community
    Maps (CC BY 4.0), USGS 3DEP, Qian Shi et al. (CC BY 4.0) and BTN 2024 ign.es (CC BY 4.0).
  - Main could use it as a convenience, but it drops ML buildings that overlap OSM, so
    per-source evidence is lost. Lite: no.
- **VIDA combined Google–Microsoft** (v2.0, 2024-09-04): 2,579,035,323 footprints. Its
  Source Cooperative page says "shared under the … ODbL v1.0". A search excerpt claimed a dual
  licence, but the page itself says ODbL only. The Google–Microsoft–OSM variant is ODbL. Not
  used: same inputs as the direct reads, with a worse licence.
- **Population rasters** (option for the core, rejected):
  - **WorldPop Global2 R2025A** (September 2025, alpha, 100 m) is CC BY 4.0. Its covariate
    metadata (`global2_covariates_metadata_R2025A_v1.xlsx`, read 2026-11-07) lists "Distance to
    OSM major roads", "… road intersections" and "… waterbodies", with the source
    "OpenStreetMap … Open Database License (ODbL)". It also lists Microsoft footprints taken
    in 2023, when they were ODbL.
    - Which covariates the final model kept ("a reduced selection") is **unverified**.
    - By CLEAN_ROOM rule 6 it is treated as ODbL: main only.
  - **GHSL** (GHS-POP R2023A; GHS-SMOD R2023A v1.2, dated 2026-06-10): "Reuse of this data is
    authorised with proper acknowledgment of the source" (EC reuse policy).
    - The exact licence string per product and whether its training used OSM or Microsoft
      footprints are **unverified**.
    - Resolution is 100 m (POP) and 1 km (SMOD).
  - Both are far coarser than a 25 m building grid, and both are modelled from buildings
    anyway.

**Coverage in the study area** (two Microsoft tiles, 123130331 for Kaski–Syangja–Tanahun hills
and 123131222 for Chitwan–Makwanpur, clipped to lon < 84.93°, about 10,000 km²):

| Source | Buildings | Places with none within 1 km (n = 600) |
|---|---:|---:|
| Google v3 (≥ 0.65) | 1,169,469 | 0.8% |
| OSM (2026-11-05) | 655,549 | 2.0% |
| Microsoft (2026-08-13) | 567,756 | 0.2% |

Google detects about twice as many buildings as the others. It counts small structures and has
some false positives (**unverified** split). Microsoft has the fewest buildings but the fewest
empty places.

### 3.2 Methods for the core

| Method | Idea | Known behaviour |
|---|---|---|
| Mean-shift (Comaniciu & Meer 2002) | hill-climb the Gaussian kernel density of building centroids from a seed | Finds the mode of the basin the seed is in. Bandwidth h sets the scale; deterministic with a fixed iteration rule |
| Highest mode within R | all modes within R; take the densest | Jumps to the biggest neighbour |
| DBSCAN / HDBSCAN (Ester 1996; Campello 2013) | connected building clusters; take the centroid | Terai villages chain into km-long components; the centroid can fall outside a crescent village |
| Nearest building to the median | snap | A snap, not a centre |
| Population raster peak (WorldPop, GHSL) | argmax of the 100 m population | Coarse; licence lineage (§3.1) |

**Bandwidth by class:** h = 75 m for a hamlet or isolated dwelling, 150 m for a village, 300 m
for a town, 600 m or more for a city. A city's "core" is its central business area, which
buildings alone do not mark well. For cities the seed and the admin-centre link (D-058) matter
more. For towns, use a coarse-to-fine pass (300 m, then 100 m) inside the assignment cell.

**Assignment among close places:** each candidate place owns the buildings whose nearest place,
among all conflated populated places, is it. This is the Voronoi cell of the pre-core consensus
points. Mean-shift runs on those buildings only.

- Two alternatives were not measured here:
  - a weighted (power) Voronoi with the radius growing with class or population;
  - a one-to-one assignment of modes to places (Hungarian algorithm on distance).
- Both are candidates for the Phase 3 bench.

### 3.3 Measurement on Nepal (2026-11-07)

**Setup:**
- Scripts and output are in the scratchpad (not committed): `prep.py`, `core.py`, `summ.py`,
  `raster.py`. Pure Python 3, no numpy.
- Study area: the two Microsoft tiles above.
- Places: 8,051 `GeoNames` P-class records (PPL, PPLA*, PPLL).
- Sample: 300 PPL and 300 PPLL, seeded `Random(20261107)`.
- OSM–`GeoNames` links: same folded name within 3 km, unique both ways. Only 42 links: OSM
  has 702 village, hamlet, town and city nodes in the area, against 8,051 `GeoNames` places.
- Grid: 25 m cells on a local equirectangular plane at 27.7° N.
- Slope proxy: `GeoNames` `dem` < 400 m (Terai and inner valleys, n = 78) against ≥ 400 m
  (hills, n = 522). This is a proxy, not the DEM slope.
- Run: 194 s and 322 MB for all methods on three sources.

**Crowding.** `GeoNames` nearest-neighbour spacing: P10 213 m, P50 423 m, P90 777 m. 63% of
places have another place within 500 m, and 96.7% within 1 km. Within 1 km of a place there
are P50 4 density modes (h = 150 m). Only **26–33%** of places have a single mode at least half
as dense as the densest.

**Displacement from the `GeoNames` point (all 600; Google buildings; Microsoft and OSM within
±15 m at P50):**

| Method | P50 | P90 | No core | Core lands nearer another place |
|---|---:|---:|---:|---:|
| Mean-shift h = 75 m | 126 m | 291 m | 8.8% | — |
| Mean-shift h = 150 m | 247 m | 598 m | 2.8% | **41%** |
| Mean-shift h = 300 m | 500 m | 1,230 m | 1.3% | — |
| Coarse to fine (300 → 100 m) | 535 m | 1,229 m | 1.3% | — |
| Highest mode within 1 km | 628 m | 925 m | 1.3% | — |
| DBSCAN centroid (ε 50 m) | 117 m | 478 m | 0.8% | — |
| Nearest building | 36 m | 212 m | 0% | — |
| **Assignment-cell mean-shift, h = 150 m** | **143 m** | **304 m** | 4.8% | 0% by construction |

- **By class** (assignment-cell mean-shift, Google): PPL P50 68 m and P90 242 m; PPLL P50
  208 m and P90 337 m. Hamlets sit in the most crowded cells.
- **"No core"** (assignment-cell mean-shift, no buildings in the cell within 3h of the seed):
  OSM 9.3%, Google 4.8%, Microsoft 3.2%.

**Agreement between building sources** (same seed, two building sources):

| Method | OSM vs Google | OSM vs Microsoft | Google vs Microsoft |
|---|---|---|---|
| Mean-shift 150 m | P50 43 m, P90 477 m | 37 m, 418 m | 32 m, 420 m |
| Coarse to fine | 73 m, 1,135 m | 57 m, 1,083 m | 27 m, 791 m |
| Highest mode | 52 m, 1,043 m | 49 m, 954 m | 32 m, 891 m |
| **Assignment-cell mean-shift** | **27 m, 179 m** (84% < 100 m) | **28 m, 159 m** (86%) | **22 m, 131 m** (88%) |

**Seed independence** (core from the `GeoNames` point against core from the linked OSM point;
the raw points are P50 402 m and P90 1,561 m apart, n = 42):

| Method | Same core (< 25 m), OSM / Google / Microsoft buildings |
|---|---|
| Mean-shift 75 m | 38% / 46% / 44% |
| Mean-shift 150 m | 64% / 72% / 69% |
| Mean-shift 300 m, coarse to fine | 81–86% |
| Highest mode | 64–67% |
| DBSCAN | 40–48% |
| Nearest building | 0% |
| **Assignment-cell mean-shift** | **94% / 100% / 94%** (n = 30–32: no core when the OSM point falls outside the cell) |

The assignment-cell result is partly built in: both seeds share the same cell, defined by the
`GeoNames` point. So it shows the start point no longer matters inside the cell. It does not
show the cell is right. The wide-bandwidth variants agree with each other because they pull both
seeds to the same large neighbouring cluster: high agreement, with a P50 displacement of 500 m.

**Raster equivalence** (`raster.py`, 115 places, Google): steepest 8-neighbour ascent on a
25 m count raster blurred by a separable Gaussian (σ = 150 m) lands within 25 m of exact
mean-shift for 89.6% of places and within 50 m for 92.2%. The rest (P99 783 m) climbed into a
neighbouring basin. The forge defines the core by the raster method, which is deterministic and
O(cells), and checks it with a golden test, not against exact mean-shift.

**Raw points against the core** (unrestricted coarse-to-fine core as a rough reference, linked
pairs): `GeoNames` points are within 500 m of the core in 48–55% of cases, OSM points in
62–72%. Read this as a size of the problem, not an accuracy figure (§3.4).

**What the measurement says:**
- The bandwidth matters less than the assignment.
- A plain density peak is unambiguous for under a third of Nepali places.
- A core computed inside the place's own cell is stable across building sources (P90 131–179 m)
  and across seeds.

### 3.4 The consensus point and its validation

- **Observations:**
  - every source point, with σ from D-055: precision prior step/√12, stacks counted once,
    copies (< 1 m) counted once (D-056);
  - plus the core, with σ_core ≈ 75 m per axis.
- **Where σ_core comes from:** for a 2-D normal error, P90 of the radial error = 2.146 σ. The
  cross-building-source P90 of 131–179 m gives σ ≈ 61–83 m for the difference between two
  cores. That is about 45–60 m per core if the two errors were independent. 75 m is
  conservative. It is fitted on the golden set before release (**estimate**).
- **Order of computation:**
  1. Seed: the weighted Weiszfeld median of the source points, as in forge-pipeline §3.3
     (fixed 64 iterations, Vardi–Zhang fix, ENU plane).
  2. Assignment cells: Voronoi of all seeds in the tile.
  3. Core inside the place's own cell, by class bandwidth.
  4. Final Weiszfeld with the core added.
  5. Accept the core only if:
     - it is within a class gate of the seed (1 km for a village, an **estimate**);
     - its cell holds at least k buildings (k = 5, an **estimate**);
     - the mode is at least 0.5 × the densest mode in the cell.
  6. Otherwise record "core rejected" with the reason in provenance and widen σ_h.
- **Why one pass, not more:** a second assignment pass using the final points could move cells.
  One pass is chosen for determinism and to avoid feedback loops (**decision proposed**).
- **Elevation:** sampled at the final point (D-061).
- **Independent truth:** the core defines the Q-005 target, so evaluating against our own core
  is circular. Options:
  1. **Hand-labelled golden set** (recommended).
     - 300 Nepal places, stratified by class and slope.
     - The labeller marks the inhabited centre that carries the name, using the three
       footprint layers, Sentinel-2 imagery (Copernicus free, full and open data, with
       attribution) and local knowledge.
     - Two labellers on 20% of the set to measure agreement.
     - Labels are CC0, kept in `tests/golden/`, and never produced from forge outputs (the D-060
       rule).
     - **Lite caveat:** labels drawn on OSM footprints must not enter the lite pipeline. They
       are used only as a pass or fail number, like the timezone QA isolation in
       [source-licences](source-licences.md).
  2. **Field GPS points** from people in Nepal. Truly independent, but a small n. Use it as an
     audit of the golden set.
  3. **Leave-one-source-out:** drop OSM (or `GeoNames`) from the consensus and measure the
     distance to the dropped source's point. Cheap and repeatable, but noisy (sources disagree
     P50 360 m). Useful as a regression gate, not as accuracy.
  4. **Cross-building-source agreement** (lite core from Google + Microsoft against main core
     from OSM). Measures the stability of the core, not where the name belongs.
  5. Survey Department topographic sheets (names placed at settlements). The licence is
     **unverified** and likely restrictive. Evaluation only, if at all.

### 3.5 World scale

- **Volume:**
  - Google v3: 1.8 B rows, 48 GB of points.
  - Microsoft: about 1.4 B rows in 30,340 tiles.
  - OSM buildings: on the order of 600 M+ (**unverified**; Nepal alone 8.3 M).
- **Streaming:** each source is streamed once, and each centroid is binned into the tile it
  falls in.
  - Tile = Microsoft quadkey level 9 (about 78 × 69 km at 28° N), plus a 2 km halo written to
    the neighbouring tiles' bins.
  - Per tile: a dense `u16` count raster at 25 m, about 3,100 × 2,800 cells, which is 17 MB.
  - There are three sources plus one blurred `f32` raster (35 MB), so peak memory is about
    0.1 GB per worker. A count over 65,535 per 25 m cell is impossible for real buildings;
    saturate and flag it.
- **Determinism:**
  - Integer counts.
  - A fixed-order separable blur with a fixed kernel table (f32 or fixed-point i32).
  - Steepest ascent with a tie rule (the lowest (y, x) index on equal values).
  - A Voronoi tie broken by the lower place ID.
  - Output snapped to the 1e-7° integer grid.
  - No random seeds are needed. Same inputs give the same bytes.
- **Cost** (**estimate**):
  - Binning in pure Python ran at 0.14 M rows/s (Microsoft GeoJSON) to 1.4 M rows/s (CSV). Rust with streaming CSV, Parquet
    or PBF readers should reach 20–50 M/s, so about 4 B centroids take minutes of CPU plus I/O.
    Download and decompression dominate: Google points are 48 GB.
  - Blur: 8.7 M cells × 2 passes × 37 taps ≈ 0.6 G FMA per tile, about 0.3 s. About 30 k
    tiles ≈ 3 CPU-hours. Most tiles are sparse, and a coarse occupancy mask can skip empty
    blocks.
  - Per place: Voronoi cell (from an R-tree of seeds) plus ascent ≈ microseconds. Tens of
    millions of places take minutes.
  - Total about 10 CPU-hours for the planet, shardable by tile across CI jobs ([ci](../../06-engineering/ci.md)).
  - Per country: Nepal is 52 tiles, about 1–2 CPU-minutes plus 0.6 GB (Microsoft) + 0.2 GB
    (Google points) of download.
- **OSM building extraction:** reuse the forge's OSM reader node-location store. DuckDB is a
  spike tool only.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Google Open Buildings v3 | Most buildings in Nepal (1.17 M in sample area); points files small; per-cell precision thresholds | 2023 inference, no updates since; false positives at 0.65 | CC BY 4.0 or ODbL (user's choice) | v3, objects 2023-06-23 | **Main + lite** |
| Microsoft Global ML | Fewest empty places (0.2%); refreshed 2026-08-13; confidence field | Recall 76.7% in South Asia; hosting moves | CDLA-P 2.0 | 2026-08-13 | **Main + lite** |
| OSM buildings | 8.3 M in Nepal; human-verified; current | Uneven coverage (2.0% empty in sample) | ODbL | extract 2026-11-05 | **Main only** |
| Overture buildings | One conflated file | Drops overlapping ML buildings; ODbL theme | ODbL | 2026-10-21.0 | No |
| VIDA combined | One file | Same inputs; ODbL | ODbL (v2.0) | 2024-09-04 | No |
| Open Buildings 2.5D Temporal | Annual 2016–2023, counts and heights | Raster, 4 m effective | CC BY 4.0 or ODbL | v1 | Later (abandonment check) |
| WorldPop R2025A | Population per 100 m | Coarse; OSM covariates | CC BY 4.0, OSM lineage | R2025A alpha | No (main-only if ever) |
| GHSL POP / SMOD | Global, consistent | 100 m / 1 km; lineage unverified | EC reuse with acknowledgement | R2023A | No |
| Unrestricted mean-shift 150 m | Simple | 41% of cores land nearer another place | — | — | No |
| Wide bandwidth / highest mode | Seed-independent | Pulls to the neighbour: P50 500–630 m | — | — | No |
| DBSCAN centroid | No bandwidth | Unstable (40–48% same core); chains in Terai | — | — | Splitting only |
| Nearest building | Always on a building | Not a centre (0% same core) | — | — | Snap only |
| **Assignment-cell mean-shift, class bandwidth** | Stable across sources (P90 131–179 m) and seeds (94–100%) | Depends on candidate set quality; 3–9% no core | — | — | **Chosen** |

## 5. What we build ourselves, and why

No library does "the building core of a named place among crowded neighbours". The forge builds:

1. a tile binner: one streaming pass per building source into `u16` 25 m rasters with a halo;
2. a deterministic blur and steepest ascent;
3. assignment cells from the seeds;
4. acceptance gates with the reason in provenance.

**Gates to prove it:**
- a golden test of the raster core on fixtures (a hand-built village layout with two nearby
  names);
- the cross-building-source agreement gate (P90 ≤ 180 m, Nepal, Google vs Microsoft);
- the golden-set accuracy in `ACCURACY.md` per class and slope band;
- a deliberate-violation test: shuffle the place IDs and the agreement gate must fail.

## 6. Impact on the plan

- **Decisions to record:**
  1. Building sources: main = OSM + Microsoft + Google; lite = Google (CC BY option) +
     Microsoft (CDLA-P 2.0), both read direct; Overture and VIDA buildings are not read.
  2. The core is mean-shift within the place's assignment (Voronoi) cell, with bandwidth by
     class, computed as raster ascent; no core without buildings.
  3. WorldPop R2025A is treated as ODbL-lineage (main only) under CLEAN_ROOM rule 6.
  4. `ACCURACY.md` for Q-005 is measured against a hand-labelled golden set, never against the
     forge's own core.
- **Phase 3 plan:**
  - add the binner and core stage to geo-fix;
  - add the source pins `msbuildings-np` (per-tile manifest, like D-061) and
    `google-open-buildings-v3-np` (S2 cells `3995`, `3993`, `39eb` and others);
  - the golden-set labelling task (300 places);
  - a bench for the assignment variants (power Voronoi, Hungarian).
- **forge-pipeline §3.3:** its "mean-shift peak … reached from the candidate consensus point"
  needs the assignment-cell restriction. It also says lite may use Microsoft "from source
  (CDLA-Permissive 2.0)", which is confirmed.
- **ADR-0030 / source-licences:** add WorldPop's OSM covariates as evidence.

## 7. Risks and open items

- **The candidate set decides the cells.** `GeoNames` NP has many PPLL stubs and former units.
  A wrong or duplicate place steals buildings from its neighbour. Run deduplication (the
  matcher, D-059) before the cells. Measure how much the cells change between the
  `GeoNames`-only and the conflated candidate sets.
- **Small link sample.** The seed-independence result rests on 42 links (30–32 with a core).
  Repeat with the conflated clusters in Phase 3.
- **Slope.** Strata here use `GeoNames` elevation as a proxy; Phase 3 uses Copernicus slope.
- **Microsoft's licence history.** It was ODbL until a change before 2026-08 (the date of the
  change is **unverified**). Pin the README and LICENSE text with the tile manifest.
- **Google v3 false positives** at 0.65 confidence. Test the per-cell 90%-precision threshold
  against 0.65 on the agreement gate.
- **Towns and cities:** buildings saturate. The core of a city should come from the admin
  centre, the market or the old core. Not measured here (6 towns in the link set).
- **The 1 km acceptance gate, k = 5 and σ_core = 75 m** are estimates to fit on the golden set.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Microsoft Global ML Building Footprints | refresh 2026-08-13, CDLA-P 2.0 | 2026-11-07 | GitHub README, LICENSE, `dataset-links.csv` |
| Google Open Buildings | v3 (objects 2023-06-23) | 2026-11-07 | sites.research.google, GCS listing |
| Open Buildings 2.5D Temporal | v1 (2016–2023) | 2026-11-07 | sites.research.google/…/temporal |
| Overture Maps | 2026-10-21.0, schema v2.0.0 | 2026-11-07 | docs.overturemaps.org |
| VIDA Google–Microsoft | v2.0 (2024-09-04) | 2026-11-07 | source.coop |
| WorldPop Global2 | R2025A v1 (September 2025) | 2026-11-07 | release statement, covariate metadata |
| GHSL GHS-SMOD | R2023A v1.2 (2026-06-10) | 2026-11-07 | GHSL download page |
| OSM Nepal extract | 2026-11-05 (`geofabrik-np`) | 2026-11-07 | data/sources.lock |
| DuckDB (spike only) | 2.0.0 | 2026-11-07 | local |

## 9. Sources

- [microsoft/GlobalMLBuildingFootprints README and LICENSE](https://github.com/microsoft/GlobalMLBuildingFootprints), Microsoft, accessed 2026-11-07.
- [dataset-links.csv 2026-08-13](https://bfppub.blob.core.windows.net/%24web/2026-08-13/dataset-links.csv), Microsoft, accessed 2026-11-07.
- [Community Data License Agreement – Permissive 2.0](https://cdla.dev/permissive-2-0/), Linux Foundation, accessed 2026-11-07.
- [Open Buildings](https://sites.research.google/gr/open-buildings/), Google Research, accessed 2026-11-07.
- [Open Buildings 2.5D Temporal](https://sites.research.google/gr/open-buildings/temporal/), Google Research, accessed 2026-11-07.
- [open-buildings-data bucket listing](https://storage.googleapis.com/storage/v1/b/open-buildings-data/o?prefix=v3/&delimiter=/), Google, accessed 2026-11-07.
- [Overture attribution](https://docs.overturemaps.org/attribution/) (updated 2026-05-15) and [release calendar](https://docs.overturemaps.org/release-calendar/), Overture Maps Foundation, accessed 2026-11-07.
- [Google-Microsoft Open Buildings, combined by VIDA](https://source.coop/vida/google-microsoft-open-buildings), Source Cooperative, accessed 2026-11-07.
- [WorldPop Global2 Release Statement R2025A v1](https://data.worldpop.org/repo/prj/Global_2015_2030/R2025A/doc/Global2_Release_Statement_R2025A_v1.pdf) and [covariate metadata](https://data.worldpop.org/repo/prj/Global_2015_2030/R2025A/doc/covariates/global2_covariates_metadata_R2025A_v1.xlsx), WorldPop, September 2025, accessed 2026-11-07.
- [GHSL download page](https://human-settlement.emergency.copernicus.eu/download.php), European Commission JRC, accessed 2026-11-07.
- Sirko et al., "Continental-scale building detection from high resolution satellite imagery", arXiv:2107.12283, 2021.
- Sirko et al., "High-Resolution Building and Road Detection from Sentinel-2", arXiv:2310.11622, 2023.
- Comaniciu & Meer, "Mean shift: a robust approach toward feature space analysis", IEEE TPAMI 24(5), 2002.
- Ester et al., "A density-based algorithm for discovering clusters", KDD 1996; Campello, Moulavi & Sander, "Density-based clustering based on hierarchical density estimates", PAKDD 2013.
- Vardi & Zhang, "The multivariate L1-median and associated data depth", PNAS 97(4), 2000.
- [S5 Nepal bench](../benches/s5-nepal.md) and decisions D-055, D-056, D-061, D-062 in [the log](../../03-decisions/log.md).
