---
title: "Forge: the data pipeline (readers, conflation, geo-fix, topology, determinism, orchestration, corrections)"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-06
---

# Forge: the data pipeline

## 1. Question

How should the forge, the separate Rust workspace that turns sources into `.tgd` packs, be built? It has to:

- read every source class we admit ([ADR-0030](../../03-decisions/adr/ADR-0030-source-admission.md)): OSM PBF, Overture GeoParquet, Wikidata, GeoNames, official boundaries, census tables, the Copernicus DEM (NASADEM for lite), building footprints and timezone boundaries;
- run geo-fix, conflation, hierarchy, names, postal, timezone, QA gates, the pack writer and signing;
- be **reproducible**: the same `data/sources.lock` gives byte-identical packs;
- be **streaming and tiled** with a disk budget. It must fit a 16 GB / ~50 GB-free laptop and sharded GitHub Actions jobs (4 vCPU, 16 GB, 14 GB SSD guaranteed, each job under 6 h);
- keep **per-field provenance** ([ADR-0023](../../03-decisions/adr/ADR-0023-provenance.md));
- run a clean-room **lite** build that never reads ODbL inputs ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md));
- keep contributor **corrections as data** that survive rebuilds.

The answers feed a future ADR for the forge architecture, `docs/04-architecture/forge.md`, the accuracy model ([ADR-0007](../../03-decisions/adr/ADR-0007-accuracy-model.md)), stable IDs ([ADR-0008](../../03-decisions/adr/ADR-0008-stable-ids.md)), CI ([ADR-0019](../../03-decisions/adr/ADR-0019-ci.md)), the open question Q-005 and the design of spike **S5**.

## 2. Summary and recommendation

**The forge is a Rust DAG of pure stages over Parquet intermediates, each keyed by content hash and sharded by country (then by tile inside large countries).** Every stage is a function `(inputs by hash, stage version, config) → outputs`. It is deterministic by construction: integer coordinates on the 1e-7° grid ([ADR-0029](../../03-decisions/adr/ADR-0029-coordinate-grid.md)), a total order on every output, no `RandomState`, and fixed-shape parallel reductions. We write the orchestrator ourselves; it is small. We do not adopt Bazel, Nix, DVC or Dagster.

- **Readers.** Use `osmpbf` 0.3.8 for OSM. It has a parallel `par_map_reduce`, an `IndexedReader`, mmap blob readers, and support for `LocationsOnWays` (`WayNodeLocation`). Multipolygon and boundary assembly is ours, a port of the libosmium area-assembler logic. `osmium-tool` is a test oracle and an optional pre-filter, never a build dependency. Read Overture with `parquet`/`arrow` 60.1 plus `geoarrow`/`geoparquet` 0.10, over HTTP range requests (`object_store` 0.14), pruning row groups by the `bbox` column. Read the DEM with `tiff` 0.11, per-tile windows, checked against GDAL. For Wikidata, a pinned **per-country SPARQL snapshot** (QLever or WDQS) saved as a hashed, mirrored file is the default. A dump-filter job is the periodic full rebuild. GeoNames is plain TSV in a zip (`csv`, `zip`).
- **DuckDB 2.0** (`duckdb` crate 1.20000.1, released 2026-10-21/28) is the **analysis and QA tool**, used for spike S5, reports and ad-hoc checks. It is not in the build path. The reasons are a heavy C++ build, the spatial extension downloaded at runtime, and output order and float aggregates that are not deterministic unless every query is written defensively.
- **Conflation** is a Fellegi–Sunter model, reimplemented in Rust in the way Splink does it. The comparison vector covers distance band, name similarity (after our normalisation and transliteration), feature class compatibility, admin compatibility, population ratio and shared external IDs. u comes from random sampling, m from EM over several blocking sessions, and term-frequency adjustments apply to names. **Blocking** uses spatial cells (H3 or a geohash ring with adaptive radius per class) × a phonetic or normalised-name key, plus hard links through shared IDs (OSM `wikidata=*`, GeoNames ID in Wikidata P1566, Overture bridge files). **Clustering** is union-find over pairs above the threshold, processed in descending weight order and refusing any union that would join two records with a cannot-link: same source and same ID space, a correction, or incompatible classes. Ambiguous clusters go to a QA queue.
- **Geo-fix.** The consensus point is a weighted **Weiszfeld geometric median** (breakdown point 0.5) in a local tangent plane. Rounded sources are down-weighted by a **rounding detector that is tested against a uniform baseline**. For settlements with buildings, the **mean-shift peak of building-centroid density** serves as an extra "source". polylabel (crate 3.3.1) is used for areas. Elevation is sampled from the DEM at the consensus point, and the uncertainty is propagated as σ_h·tan(slope).
- **Topology.** We build our own TopoJSON-style **arc topology on the integer grid**. Rings are noded with snap rounding (`i_overlay` 9.1 for robust integer boolean operations), shared arcs are found by vertex hashing as mapshaper does, slivers and gaps are fixed by assigning each to the neighbour with the longest shared edge, and each LOD is simplified **per arc** with fixed nodes and an intersection guard. No Rust crate does this (`topojson` 0.5.1 has types only and was last released in 2022).
- **Point-in-polygon at scale:** a packed static R-tree (`geo-index` 0.4, Hilbert/STR-packed and deterministic, or `rstar` 0.14 bulk load) over polygon bounding boxes, then exact `geo` 0.33 `Contains`/`PreparedGeometry` with robust predicates (`robust` 1.2).
- **Corrections** are YAML files (one per correction) with a target stable ID, a **guard** (the source record ID plus a hash of the source value the correction was written against), an operation, evidence and an evidence licence class. On each build a correction is either applied, retired (the upstream now agrees), or sent to the QA queue as stale (the upstream changed in some other way). It is never silently dropped and never silently applied over new upstream data.
- **CI sharding.** Inputs are mirrored to our own release assets by SHA-256 (each under 2 GiB). Inputs are never re-fetched from moving `latest` URLs. Intermediates move between jobs as workflow artifacts within a run, and finished per-shard outputs are cached under their content hash. The matrix runs one job per country or tile shard, and a final job assembles, runs the gates, writes and signs.

Measured on 2026-11-06 for Nepal (S5 inputs): the Geofabrik extract is 414.9 MB; GeoNames `NP.zip` is 3.46 MB, with 148,210 rows, 87,926 of them populated places (77,913 `PPLL`) and 754 `ADM3`; OSM has 21,123 `place=*` nodes and 749 `admin_level=7` relations; Wikidata has 9,586 items with `P17=Nepal` and a coordinate. **Unverified** items are marked inline.

## 3. Findings

### 3.1 Readers

**OSM PBF in Rust.**

- `osmpbf` 0.3.8 (2025-10-24, MIT/Apache-2.0, MSRV 1.88). API checked on docs.rs: `ElementReader` with `par_map_reduce` (parallel decode across cores), `IndexedReader` (filters ways and relations and resolves their dependencies), `MmapBlobReader`, `DenseNode*`, and `WayNodeLocation`/`WayNodeLocationsIter`, so it reads `LocationsOnWays` PBFs produced by `osmium add-locations-to-ways`. The README claims performance through "parallelization and lazy-decoding" but publishes no numbers.
- `osmpbfreader` 0.19.1 (2025-05-26). It has `par_iter` and `get_objs_and_deps` (the dependency closure for selected objects, held in memory as a `BTreeMap`). Its README times are on `france-latest` (3.3 GB) with an i7-4702HQ and rustc 1.14: about 4 min 25 s to read 416 M objects, 1 min 10 s to count admin_level 8, and 5 min 10 s with dependencies. These are a 2016-era baseline only.
- `osmio` 0.16.1 (2026-07) reads and writes PBF, XML and OPL. It is smaller and less used.
- **osmium (libosmium, C++, v2.23.1 2026-04-01)** is the reference for speed and for multipolygon assembly. There are no maintained Rust bindings to libosmium (header-only C++ templates are hard to bind), so "osmium via FFI" in practice means shelling out to `osmium-tool`. That works for `tags-filter`, `extract`, `add-locations-to-ways` and `export`, but it brings a C++ toolchain and a second implementation into the reproducible path.
- **Relative speed is unverified.** We found no current, like-for-like benchmark of osmpbf against osmpbfreader against osmium. Benchmark B1 in §6 measures it on `nepal-latest` (415 MB).

**Extracting what we need from OSM.**

- Place nodes are `place=*`, plus `name:*`, `ele`, `population`, `wikidata`, `is_in*`. They come from one streaming pass and are cheap.
- Admin relations are `boundary=administrative` and `admin_level=*`, plus `boundary=disputed` for perspectives ([ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md)).
- Multipolygon assembly is three passes, or two with `LocationsOnWays`:
  1. Relations: collect member way IDs and roles.
  2. Ways: collect node refs for the member ways only.
  3. Nodes: resolve the coordinates of the needed node IDs only, into a sorted `Vec<(id, i32, i32)>` with binary search. A dense array over 13 G node IDs is not needed.
- The assembler joins ways end to end by node ID into closed rings. It **ignores roles for classification** and classifies outer and inner rings by containment, as libosmium does, because `role` tags are often wrong. It reports open rings and self-intersections as QA problems. For Nepal's roughly 7.5 k admin relations (749 at level 7, about 6.7 k at level 9 per the Nepal sources note) the needed node set is small. At planet scale the per-country shards keep it bounded.

**Overture (GeoParquet) in Rust.**

- `parquet`/`arrow` 60.1.0 (2026-10-28) are the Apache Arrow Rust implementation.
- `geoarrow` and `geoparquet` 0.10.0 (released 2026-11-06, MSRV 1.95) come from the geoarrow-rs project. They decode the WKB or native GeoArrow geometry columns and read GeoParquet metadata. The project moves fast and breaks often (0.7 → 0.10 within 2026), so pin it exactly.
- Overture's latest release is `2026-10-21.0` (STAC catalog `https://stac.overturemaps.org/catalog.json`, 2026-11-06). It ships a `bbox` struct column, so a range-request reader can prune row groups by bounding box without downloading whole files. `object_store` 0.14 provides the S3/Azure/HTTP access.
- Licences matter per theme: places are mostly CDLA-Permissive 2.0, Apache-2.0 (Foursquare) or CC0, while divisions, buildings, base and transportation are ODbL. Lite may read Overture places only, and only with the per-source licence column checked (ADR-0030).
- GERS IDs are random 128-bit UUIDs (since `2025-06-25.0`) with a registry, a changelog, and bridge files back to source IDs (OSM among them). Bridge files give us **free hard links** from OSM to Overture.

**DuckDB via `duckdb-rs`.**

- The `duckdb` crate is at 1.20000.1 (2026-10-28), which encodes DuckDB 2.0.0 (released 2026-10-21). The `bundled` feature compiles DuckDB's C++ sources, which is slow and large.
- The spatial extension (`ST_Read`, `ST_ReadOSM`, GEOS-backed predicates) is normally downloaded at `INSTALL`/`LOAD` time from the extension repository. For reproducibility it would have to be vendored and pinned by hash. Whether the extension can be statically linked through the crate is **unverified**.
- Result order is undefined without `ORDER BY`, and parallel float aggregates are not bit-stable.
- Verdict: excellent for S5 analysis, QA dashboards and contributor exploration. Kept out of the pack build path.
- SedonaDB (`sedona` 0.5.0, Apache-2.0, DataFusion-based) and `geodatafusion` 0.6 are pure-Rust alternatives for later. They are immature today.

**GeoTIFF/COG in pure Rust.**

- `tiff` 0.11.3 (image-rs, 2026-02) decodes tiled and striped TIFF with DEFLATE and LZW, and it reads individual chunks (tiles), which gives windowed reads for free. Whether it supports predictor 3 (floating-point), which Copernicus COGs may use, is **unverified**. Test it in B3. **B3 result (2026-11-07):** it does; the Copernicus tiles (DEFLATE, predictor 3, 1024² tiles) decode bit-identically to libtiff in 0.33 s per tile (D-061).
- `geotiff` 0.1.0 (georust, 2025-06) adds GeoKeys and the model transform. It is thin, and reading the `ModelTiepoint` and `ModelPixelScale` tags ourselves is about 50 lines.
- `async-tiff` 0.4.0 (2026-09) reads COGs over object stores by HTTP range.
- `gdal` 0.19 is the full GDAL binding: a system library, heavy and non-reproducible across GDAL versions. Use it as a **test oracle** only.
- Copernicus GLO-30 tiles are 1°×1° COGs on `s3://copernicus-dem-30m`. `Copernicus_DSM_COG_10_N27_00_E085_00_DEM.tif` is 44.9 MB. Nepal's bounding box N26–N30 × E080–E088 covers 45 tiles, so about 2 GB at most for Nepal. The DEM is a DSM, so building and canopy bias applies; the timezone-and-elevation note covers its accuracy.

**Wikidata.**

- `latest-all.json.bz2` is 103.6 GB (2026-10-27). `latest-truthy.nt.bz2` is 43.6 GB (2026-11-06). Neither fits on a 14 GB runner, so either would have to be stream-filtered over HTTP.
- Decompression is the bottleneck. A single-threaded bzip2 decoder runs at tens of MB/s, which puts a full JSON pass near or over the 6 h job limit. Whether Wikimedia's dumps are multi-stream (which would allow a parallel decode) is **unverified**.
- The subset we need is small: items with P625 (coordinate) in our countries, plus their P31, P131, P1566 (GeoNames ID), P402 (OSM relation), labels, P1082 population and P2044 elevation. Nepal has 9,586 items with P17=Q837 and P625, from a WDQS count on 2026-11-06. A transitive `P31/P279*` filter **timed out** on WDQS (502), so class filters must run offline or on QLever (v0.6.0, 2026-08-13, which hosts Wikidata with no such timeouts).
- Plan: a per-country SPARQL extraction against QLever or WDQS, saved as N-Triples or JSON, hashed, mirrored as our own release asset and pinned in `sources.lock`. The endpoint is not reproducible; the snapshot is. A quarterly "dump-filter" job (streaming bz2 → filter → Parquet) cross-checks the snapshots. Licence: CC0, which is fine for lite.

**GeoNames.**

- Daily dumps under `download.geonames.org/export/dump/`: `allCountries.zip` 422.5 MB, `alternateNamesV2.zip` 205.2 MB, `hierarchy.zip` 2.1 MB, `NP.zip` 3.46 MB (all modified 2026-11-06).
- The format is 19-column TSV. The `elevation` column is sparse (56 of 87,926 NP populated places have it). The `dem` column (SRTM3 or GTOPO30) is filled for all of them.
- Licence: CC BY 4.0, so it is lite-eligible under ADR-0030.
- GeoNames keeps no dated archive, so we mirror the pinned file ourselves. That is required for reproducibility.
- First look at `NP.txt` (2026-11-06, SHA-256 `e14643d6…29b0fd`): 87,926 class `P` rows (77,913 `PPLL`, 9,795 `PPL`); class `A` has 7 `ADM1`, 77 `ADM2`, 754 `ADM3` (dated 2019-10-16), 3,981 `ADM4H` and 36,035 `ADM5H` (historical units).

### 3.2 Conflation

**Fellegi–Sunter, as Splink implements it.**

- The model goes back to Fellegi & Sunter (1969). For each record pair, a comparison vector γ is computed: a level per field, for example "name exact / JW ≥ 0.92 / JW ≥ 0.8 / else".
- Per field and level, **m** = P(γ level | match) and **u** = P(γ level | non-match). The Bayes factor is K = m/u. Under conditional independence the match weight is additive: M = log2(λ/(1−λ)) + Σ log2 Kᵢ, and P(match) = 2^M/(1+2^M). Splink's theory pages say that m "is largely a measure of data quality" and u "a measure of coincidence".
- **Training as in Splink:**
  - λ comes from deterministic rules with an assumed recall, for example `recall=0.7`. A wrong recall guess moves λ only modestly.
  - **u** comes from random record pairs, assumed to be almost all non-matches (`max_pairs` around 1e7).
  - **m** comes from EM over blocked pairs. Each EM session blocks on some fields, so it cannot estimate those fields. Several sessions with different blocking rules are averaged, with u and λ fixed to anchor EM.
- **Term-frequency adjustment:** for an exact match on a value v, the generic u is replaced by the frequency of v. A common name ("Bhanjyang", "Tole") then earns less evidence than a rare one. Splink exposes `tf_adjustment_weight` (0–1) and a floor `tf_minimum_u_value` (0.001). Fuzzy matches use the larger of the two values' frequencies. The exact Splink formula is in an image on its page; our working form is weight_tf = log2(m / max(tf(v), u_min)), which is **unverified** against Splink's code.
- **What we reimplement in Rust** (Splink itself is Python over SQL backends, v5.0.0, 2026-09-18):
  - comparison levels as a small enum per comparator;
  - u by seeded random sampling (seeded ChaCha, fixed seed in config);
  - EM over sufficient statistics: counts per γ pattern, so EM iterates over at most a few thousand distinct patterns, not millions of pairs. This is cheap and deterministic;
  - the TF tables;
  - weight and probability computation.
  The estimated m/u table is written to a versioned, checked-in file and reviewed like code. Retraining is a deliberate act, not a side effect of a build. That matters for byte-identical rebuilds.
- `fastLink` (Enamorado, Fifield & Imai 2019, *APSR*) is the same model with missing-data handling. It is a useful reference for nulls such as a missing population.

**Blocking for gazetteers.**

- Blocking surveys (Papadakis et al. 2020, *ACM CSUR*) describe the standard approach: unions of blocking keys with redundancy, then block purging and filtering.
- For places, key on a **spatial cell** whose size depends on class, since a city can be 10–20 km from another source's point while a hamlet is within about 1–2 km. Use an H3 cell plus its k-ring (`h3o` 0.11) or the 1e-7° grid bucketed into Web Mercator tiles. Combine it with a **name key**: the normalised Latin form from `teigeodata-names`, then a phonetic key (Double Metaphone or Beider–Morse from `rphonetic` 4.0, or our own South-Asian-aware key, since generic English phonetics handle Nepali transliteration variants poorly: Chowk/Chok, -pur/-pura, bh/b). Also block on **shared IDs** (OSM `wikidata` tag, Wikidata P1566 GeoNames ID, P402 OSM relation, Overture bridge files).
- The ID links are near-certain but not trusted blindly. A shared ID is a comparison level with high m/u, not an override, because wrong `wikidata` tags exist.

**Constrained clustering.**

- Pairwise scores are not transitive. Approaches compared in Hassanzadeh et al. 2009 (*VLDB*) are connected components (fast, prone to chaining), centre and merge-centre clustering, and correlation clustering.
- Our choice is **union-find with cannot-link constraints, processed by descending weight**. Each component keeps a set of "exclusive keys", for example (source, source ID space). Two GeoNames records are never merged automatically, because an intra-source duplicate is a QA finding, not a silent merge. Neither are two different OSM nodes, nor records with contradictory corrections. A union is refused when the key sets intersect.
- Components above a size or diameter limit, or with a low minimum internal edge weight, go to review. This is deterministic given a total order on edges: (weight desc, id_a, id_b).

**Prior art.**

- **Who's On First** keeps one GeoJSON file per record in Git. Duplicates are deprecated and superseded (`wof:superseded_by`/`wof:supersedes`, `mz:is_current`), and `wof:concordances` maps to GeoNames, Wikidata, OSM and others. A 2024 venue de-duplication with vector embeddings deprecated about 45,000 duplicates and derived over 100,000 concordances with Overture places. The method details are in the post body, which we did not read; **unverified**.
- **Overture GERS:** matching new data is "a conflation job against the current release", and monthly changes need maintenance. The bridge files expose Overture's own conflation.
- **Pelias** de-duplicates at query time. A 2022 API change matches records whose concordance maps intersect. Transitive chains (three or more records) were left for later.
- **Toponym matching literature:** Recchia & Louwerse 2013 compared string measures for toponyms and found no single best measure; the best depends on the language. Santos et al. 2018 (*IJGIS* 32(2)) trained a neural toponym matcher that beat classic measures on GeoNames alternate names. Sehgal, Getoor & Viechnicki 2006 (ACM GIS) combined name, type and location features for geospatial entity resolution. Christen 2012, *Data Matching*, is the reference text.
- **Evaluation:** a hand-labelled pair set, stratified by weight band (most information sits near the threshold), with precision, recall and F1 per band and per class. Clusters are evaluated with B-cubed or pairwise F1. Labels are checked-in data under `data/labels/`, with the labeller and the evidence recorded.

### 3.3 Geo-fix

- **Weiszfeld geometric median.**
  - The iteration is y ← Σ wᵢxᵢ/‖xᵢ−y‖ / Σ wᵢ/‖xᵢ−y‖. It fails when y lands on a data point; Vardi & Zhang (2000) give the fix.
  - The geometric median's breakdown point is 0.5 (Lopuhaä & Rousseeuw 1991), so one wild source among three cannot move it far.
  - With **two** sources the median is any point on the segment. Use the higher-weight source and flag a disagreement when the distance exceeds a class limit.
  - Compute it in a local ENU or azimuthal-equidistant plane around the coordinate-wise median. For spans of a few kilometres the projection error is negligible.
  - Run a fixed number of iterations (for example 64) or stop at a tolerance on the integer grid, so the output is deterministic.
  - Uncertainty: the weighted median distance to the consensus, plus each source's prior σ.
- **Rounded-coordinate detection.**
  - Test each point against grids: 1′, 0.1′, 1″, and k decimal places. **Always compare against the uniform baseline.** At a tolerance of ±0.5″ on the minute grid, the chance a random point hits on both axes is (2·0.5/60)² ≈ 2.8e-4.
  - On GeoNames NP populated places (2026-11-06), 65 of 87,926 (0.074%) fall on the minute grid, which is mild over-representation. 949 (1.08%) fall within ±0.05″ of the second grid, against a 1.0% baseline. The share with four or fewer decimals (≈ 9.8%) matches the 10% expected from dropped trailing zeros.
  - So **NP GeoNames is not obviously grid-rounded**. Its weakness is likely elsewhere: point placement, stale units, the mass of `PPLL`. The detector must also look for **stacked points** (identical coordinates shared by many records) and points snapped to an admin centroid. S5 measures all three.
  - A point found to be rounded gets σ_h = cell/√12 per axis, for example about 535 m for 1′.
- **Settlement centres from buildings.**
  - Bin building centroids onto a grid (for example 25 m cells, integer counts), smooth with a separable Gaussian kernel (bandwidth 100–300 m by class), and take the **mean-shift** mode (Comaniciu & Meer 2002) reached from the candidate consensus point.
  - Binning makes this deterministic and O(cells). The result is one more "source" in the median, with its own σ.
  - Lite uses Google Open Buildings v3 (CC BY 4.0 option; Nepal is covered; confidence 0.65–1.0) or Microsoft footprints from source (CDLA-Permissive 2.0 per ADR-0030). Main may add OSM buildings.
  - `geo` 0.33 also has `Dbscan` and `KMeans`, which are useful for splitting multi-core settlements.
- **polylabel** (pole of inaccessibility, Mapbox 2016; crate `polylabel` 3.3.1) gives a label point guaranteed inside an area. It is used for admin areas and as a fallback place point. `geo`'s `InteriorPoint` is an alternative.
- **Point-in-polygon at scale.**
  - `rstar` 0.14 (2026-11-01) bulk-loads with OMT (an STR-like top-down packing).
  - `geo-index` 0.4 is a static packed R-tree (flatbush port: Hilbert or STR sort, flat buffers). It is deterministic and memory-light.
  - Filter by bounding box, then run exact `Contains` on `geo::PreparedGeometry` (available in geo 0.33) on integer-grid coordinates converted to `f64`. Every i32 converts exactly, and orientation tests are exact through `robust` (Shewchuk's adaptive predicates).
  - Boundary cases (a point exactly on a shared arc) need a tie rule: the lower region ID, or the "left of the directed arc" rule. The same rule must be used by the runtime region index (ADR-0013).

### 3.4 Topology for boundaries

- **The shared-arc model.** TopoJSON (Bostock) and mapshaper (Bloch, v0.7.101, 2026-10-29) represent a polygon layer as a set of arcs: maximal shared polylines between nodes. Each polygon is a list of signed arc IDs. Simplifying arcs once keeps neighbours coincident, and the stored size roughly halves because shared borders are stored once.
- **Building the arcs.** Quantise first (we already have the 1e-7° grid). Find every vertex whose neighbour set differs between the rings passing through it, or that is used by three or more rings; those are the **nodes**. Cut the rings at the nodes and de-duplicate the arcs by canonical direction (lower start-vertex first).
- This works only if neighbouring rings share **identical vertices**. Between sources (Survey Department/COD-AB vs OSM) they do not, so a **noding and snap-rounding** step comes first:
  - Snap rounding (Hobby 1999; Halperin & Packer 2002) rounds every intersection and near-vertex to a grid hot pixel, so the arrangement is consistent.
  - `i_overlay` 9.1 (integer, robust boolean operations; `geo`'s `BooleanOps` is built on it) does the integer overlay. Snap tolerance is a config value per source (for example 1–5 m), recorded in provenance.
- **Slivers and gaps.**
  - After overlaying admin units from one source, small overlap and gap faces appear. Detect them by area and thinness (Polsby–Popper 4πA/P²) below thresholds.
  - Assign each sliver to the adjacent polygon with the **longest shared edge**, as mapshaper's `-clean` gap fill does.
  - Report every reassignment, with its area, as a QA metric. Overlaps between *different* perspectives are not errors; they are perspective data (ADR-0006).
- **Simplification per LOD.**
  - Simplify arcs, not polygons, with Visvalingam–Whyatt (effective area; `geo` has `SimplifyVwPreserve`) or Douglas–Peucker, keeping nodes fixed.
  - Saalfeld (1999) shows DP can be made topologically consistent by checking that no other vertex flips side. Our guard: after simplifying each LOD, test for arc–arc intersections with an R-tree over segments, and re-add the removed vertices of offending arcs until the test is clean.
  - Never let a ring drop below 3 distinct vertices. Keep small islands at coarse LODs only above an area threshold, by policy.

### 3.5 Reproducibility and determinism in Rust

- **Hashing.** `std::collections::HashMap` uses `RandomState`, so iteration order changes per process. Never iterate a hash map into output.
  - Use `BTreeMap`/sorted `Vec` for anything emitted.
  - Use `hashbrown` 0.17 / `rustc-hash` 2.1 / `foldhash` 0.2 with fixed seeds for internal lookups only.
  - `xxhash-rust` 0.8 or `blake3` 1.8 for stable content hashes.
- **Sorting.** Every output table has a declared total order (for example `(country, kind, stable_id)`). `sort_unstable_by` is fine when the key is total and unique. Ties are a bug, so the writer asserts uniqueness. Floats sort with `total_cmp`.
- **Floats.**
  - IEEE-754 basic operations are deterministic on x86-64 and aarch64, and Rust does not contract a*b+c into an FMA on its own.
  - Transcendental functions (`sin`, `cos`, `atan2`, `exp`) call the platform libm, which may differ between glibc and macOS. Use the pure-Rust `libm` 0.2.16 in the forge wherever a result reaches output.
  - Quantise to the integer grid as early as possible, and do the decisions (PIP, topology) in integers.
- **Parallelism with `rayon` 1.12.**
  - `par_iter().map().collect::<Vec<_>>()` preserves input order and is deterministic.
  - `reduce`/`sum` over floats is **not** bit-stable, because the work-stealing split points vary.
  - Use fixed-size chunks (`par_chunks(N)`), reduce each chunk sequentially, then combine the partials in index order. Alternatively, accumulate in integers or fixed point.
  - Never let completion order decide anything: no channels feeding a writer in arrival order.
- **Randomness.** Only seeded generators (for example `rand_chacha`), with the seed in the stage config, so u-sampling is reproducible.
- **Writers.**
  - Parquet written by `parquet` 60 embeds `created_by = "parquet-rs version X"` and no timestamp by default. That is stable at a pinned version, but verify it with a double-write test. Fix the row-group size, compression level and dictionary settings in config.
  - Arrow IPC is simpler still.
  - Zip and tar archives need fixed mtimes and a fixed order.
  - The `.tgd` writer is ours and is tested byte for byte.
- **Content-addressed caches.**
  - Stage key = BLAKE3(stage name ‖ stage code version ‖ canonical config ‖ sorted input keys).
  - Outputs are stored under `cache/<key[0..2]>/<key>/`.
  - A cache hit means skip. Mismatched bytes for the same key (for example from a cache restored on another runner) is a determinism failure, and the gate fails.
- **Gate:** the "double build" check, the same idea as Debian's reproducible-builds practice. Build Nepal twice, on two machines (local macOS arm64 and the Linux x64 runner), and compare the pack hashes.

### 3.6 Orchestration

- **The DAG.** Stages are nodes. Edges are typed artefacts (Parquet datasets with a schema version).
  - Incremental rebuilds come from the content keys in §3.5: change one source's lock entry, and only its descendants rebuild.
  - In-process incremental frameworks (`salsa` 0.28) are aimed at compilers and do not manage multi-GB on-disk artefacts. Make-like tools (make, Bazel) would duplicate what our keys already give.
  - Our runner is a few hundred lines: topological order, key computation, disk-budget check, execute, verify, record.
- **Disk budget.** Each stage declares an estimate as a function of input sizes (for example 3× input for OSM assembly). The runner refuses to start when the free disk (from `fs4`/`sysinfo`) minus the estimate falls below a floor. The runner deletes intermediates as soon as their last consumer finishes, unless `--keep` is given.
- **Sharding.**
  - The unit is a **country shard**. Large countries split further into admin-1 or tile sub-shards, with a **halo** (for example 20 km) for conflation and an **ownership rule**: a record belongs to the shard containing its consensus point.
  - Cross-shard work (national boundary topology, national search index) runs in the country assembly job.
  - A `forge/manifest.json` (generated, checked in) lists shards, inputs and expected sizes, and the GitHub Actions matrix is generated from it. The limit is 256 matrix jobs per workflow run and 20 concurrent jobs on the Free plan.
- **GitHub Actions transport** (docs read 2026-11-06):

  | Mechanism | Limits | Use |
  |---|---|---|
  | Workflow artifacts (`upload-artifact` v4+) | immutable; 500 per job; retention 1–90 days (default 90); compression level settable (use 0 for zstd Parquet) | stage-to-stage inside one run |
  | Actions cache | 10 GB per repository, evicted when unused | cross-run reuse of shard outputs keyed by content hash; best effort only |
  | Release assets | each file < 2 GiB; up to 1,000 per release; no total or bandwidth cap | **mirrored pinned inputs** (an `inputs-YYYY-MM` prerelease) and published packs |

  The 500 MB artifact-storage quota on the Free plan is a private-repository billing limit. That it does not apply to public repositories is **unverified**.
- **Runner disk.** Standard public Linux runners guarantee 14 GB SSD (4 CPU, 16 GB RAM, x64 or arm64). The runner image usually has more free space on `/` and `/mnt` after cleanup (**unverified**, varies by image). Budget for 14 GB and treat anything more as headroom.
- **Planet-scale inputs** cannot be downloaded on one runner: the planet PBF is 95.7 GB and the Wikidata JSON is 103.6 GB. OSM comes as pinned per-country extracts mirrored to release assets, split under 2 GiB where needed. Geofabrik's daily `latest` changes and its dated archive is not guaranteed long-term (**unverified** retention), hence the mirror.
- **Resumability.**
  - Every stage writes to `tmp/` and renames atomically on success, then writes a `done` record (key, output hashes, timings, peak RSS).
  - A rerun skips completed keys, and a failed matrix job can be re-run alone (`gh run rerun --failed`).
  - Each stage stays well under 6 h. A stage that might not is split by the manifest.

### 3.7 Corrections as data and QA queues

- **How others do it.**
  - OSM and Wikidata edit the source itself, with changesets, history and discussion. Our corrections of their errors should also be *reported upstream*.
  - Who's On First edits records through Git pull requests and handles merges with `wof:superseded_by`.
  - Overture takes no direct edits. Corrections go to the upstream sources, and the GERS changelog shows the effects.
  - Nominatim has no correction layer of its own (**unverified**). Commercial geocoders keep override tables.
  - Our model is closest to WOF (reviewed files in Git) combined with an override layer that is re-checked every build.
- **Schema** (`data/corrections/<country>/<id>.yaml`):

  ```yaml
  id: corr-np-000123            # stable, never reused
  target: tgd:np/place/01HX...  # stable place ID (ADR-0008); or a source record for pre-merge fixes
  op: set                       # set | unset | same-as | not-same-as | split | retire | move
  field: position               # for set/unset
  value: {lat: 28.2096, lon: 83.9856}
  guard:                        # the world the correction was written against
    - source: geonames
      record: "1282898"
      field: position
      value_hash: blake3:5f1c...  # hash of the canonical source value at writing time
  evidence:
    - kind: survey | official-doc | imagery | local-knowledge | url
      ref: https://...            # plus an archived copy hash where possible
      licence_class: cc-by        # decides main/lite admission (CLEAN_ROOM.md rule 3)
  author: <DCO sign-off name>
  created: 2026-11-06
  review: {status: accepted, by: <maintainer>, date: 2026-11-07}
  ```

- **Surviving rebuilds.** For each correction the build evaluates the guard against the current source state:
  - guard holds → **apply**;
  - source now equals the corrected value → **retire** (it was fixed upstream; mark the correction as upstreamed);
  - source changed to something else → **stale**, not applied, and pushed to the QA queue with a diff;
  - target ID redirected (ADR-0008) → follow the redirect and re-check the guard;
  - target tombstoned → stale.
- `same-as` and `not-same-as` feed the clusterer as must-link and cannot-link constraints.
- The lite build ignores corrections whose evidence class is refused in lite.
- **QA queues** are generated Parquet or JSON reports per build: stale corrections, ambiguous clusters, disagreements above limits, rounded or stacked points, sliver reassignments, PIP failures. Their counts are gated (§6).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `osmpbf` | parallel decode, indexed deps reader, mmap, LocationsOnWays | no area assembly; no published benchmarks | MIT/Apache-2.0 | 0.3.8, 2025-10 | **Use** (pending B1) |
| `osmpbfreader` | `get_objs_and_deps`, long history | deps held in a `BTreeMap` (memory); older design | WTFPL/MIT (**unverified**) | 0.19.1, 2025-05 | B1 comparison only |
| `osmium-tool` (C++) | fastest known (**unverified**), reference area assembler | C++ toolchain, second implementation | BSL-1.0 / GPL-3 tool (**unverified**) | libosmium 2.23.1, 2026-04 | **Oracle** and optional pre-filter |
| `parquet`/`arrow` + `geoarrow`/`geoparquet` | pure Rust, range reads, bbox pruning | geoarrow API churn | Apache-2.0 / MIT-Apache | 60.1 / 0.10.0 | **Use**, pin exactly |
| DuckDB (`duckdb`) + spatial | SQL, fast, reads everything | C++ build, runtime extension download, non-deterministic order and float aggregates | MIT | crate 1.20000.1 = DuckDB 2.0.0 | **Analysis/QA only** |
| SedonaDB / geodatafusion | pure Rust SQL | immature | Apache-2.0 / MIT | 0.5.0 / 0.6.0 | Revisit 2027 |
| `tiff` (+ own GeoKeys) | pure Rust, chunk reads | float predictor support unverified | MIT | 0.11.3 | **Use** (B3 passed) |
| `gdal` | complete | system dependency, version drift | MIT (bindings) | 0.19.0 | **Oracle** |
| Wikidata SPARQL snapshot (QLever/WDQS) | tiny, fast, current | endpoint not reproducible; WDQS timeouts | CC0 data | QLever v0.6.0 | **Use**, snapshot mirrored |
| Wikidata dump filter | complete, reproducible by dump date | 103.6 GB, hours of bz2 | CC0 | weekly dumps | Quarterly cross-check |
| Splink (Python) | proven Fellegi–Sunter + EM + TF | Python/SQL runtime outside our toolchain | MIT | 5.0.0, 2026-09 | **Reimplement** its model; use for S5 cross-check |
| Union-find + cannot-link | simple, deterministic, explainable | chaining needs diameter guards | own | — | **Build** |
| `rstar` | dynamic, mature | bulk-load structure version-dependent | MIT/Apache-2.0 | 0.14.0 | Use for dynamic queries |
| `geo-index` | static packed tree, flat buffers | younger | MIT/Apache-2.0 | 0.4.0 | **Use** for bulk PIP |
| `i_overlay` | robust integer overlay | no arc topology | MIT | 9.1.0 | **Use** for noding/overlay |
| `topojson` crate | types | stale (2022), no topology builder | MIT/Apache-2.0 | 0.5.1 | No |
| mapshaper (JS) | best-in-class arc topology, clean, simplify | Node runtime | MPL-2.0 | 0.7.101 | **Oracle** for topology tests |
| Own DAG runner | exactly our keys and budgets | we maintain it | own | — | **Build** |
| Bazel / Nix / DVC / Dagster | mature | heavy, a second language, poor fit for GB artefacts in GH Actions | various | — | No |

## 5. What we build ourselves, and why

1. **The DAG runner and content-addressed store.** Keys, disk budget, atomic outputs, resumability, and matrix generation from the manifest. Proof: a deliberate one-line change to the GeoNames lock entry rebuilds only GeoNames descendants, and a killed run resumes without redoing completed stages.
2. **OSM area assembler** (a port of the libosmium logic). Proof: for all Nepal admin relations, ring count, area and validity are identical to `osmium export` on the same PBF (differential test), and failures match osmium's problem reports.
3. **Fellegi–Sunter engine** (comparators, EM over pattern counts, TF tables, a checked-in m/u table). Proof: on the S5 Nepal pairs, the match weights agree with Splink 5.0 on the same features within 0.01 bits, and precision and recall are measured on a labelled set of at least 1,000 pairs.
4. **Constrained clusterer.** Proof: property tests show that no cannot-link pair ever shares a cluster and that output is invariant under input permutation.
5. **Geo-fix** (weighted Weiszfeld, rounding and stacking detector with baseline, mean-shift settlement core). Proof: on synthetic data with known truth and injected outliers, the median error stays at or below the best source's error while up to 49% of the weight is corrupted. On S5, the P90 position disagreement shrinks against every single source.
6. **Arc topology builder + LOD simplifier.** Proof: zero gaps and overlaps at every LOD (exact integer check). The arc count is within 1% of mapshaper's on the same quantised input. A segment-intersection check is clean after simplification.
7. **Corrections engine** (guard evaluation, apply/retire/stale, redirects). Proof: golden tests for each of the five outcomes, and a corrupted guard fails the build.
8. **Deterministic Parquet writer settings and the double-build gate.** Proof: two builds on macOS arm64 and Linux x64 give identical pack hashes, and a deliberately injected `HashMap` iteration makes the gate fail.

## 6. Impact on the plan

- **New ADR (proposed): "Forge architecture".** Content-addressed DAG, Parquet intermediates with the GeoArrow encoding, country shards with halos, inputs mirrored as release assets, DuckDB kept out of the build path.
- **New ADR (proposed): "Corrections as data".** The schema in §3.7 and its five outcomes. It extends ADR-0008 and ADR-0023.
- **ADR-0007:** add the rounding/stacking detector and the mean-shift building source as inputs to σ_h.
- **ADR-0019:** the matrix comes from `forge/manifest.json`; artifacts within a run, cache across runs, release assets for inputs. Each job stays under 6 h and 14 GB.
- **`data/sources.lock`** gains `mirror_url` (our release asset) alongside `upstream_url`, plus the SPARQL query text and hash for Wikidata snapshots.
- **New page `docs/04-architecture/forge.md`** follows from §7 below.
- **Benchmarks to add:**
  - B1: osmpbf vs osmpbfreader vs osmium-tool, reading `nepal-latest` (time, peak RSS).
  - B2: admin multipolygon assembly against osmium (time, differences).
  - B3: Copernicus COG tile decode with `tiff` against GDAL (bit-identical values, time).
  - B4: Weiszfeld and conflation throughput per 100 k records.
- **Gates** (to `quality-bar.md`):
  - **G-det:** double build gives identical bytes (two machines).
  - **G-lic:** the lite licence gate (CLEAN_ROOM rule 4) also runs at the *ingest* stage, so the DAG cannot even schedule an ODbL reader in lite.
  - **G-topo:** zero gaps and overlaps per LOD, zero self-intersections.
  - **G-pip:** every place has exactly one region per admin level where coverage exists. Unassigned places are listed and capped.
  - **G-conf:** precision ≥ 0.98 and recall ≥ 0.95 on labelled pairs (initial targets; set from S5).
  - **G-corr:** zero unreviewed stale corrections older than one release.
  - **G-budget:** peak disk and RSS per stage within the declared budget.
  - **G-time:** every CI shard runs 30% or more under 6 h.
- **Q-005** (horizontal accuracy targets) is answered from the S5 measurements below.

### Spike S5: GeoNames vs OSM vs Wikidata for Nepal

**Goal:** measure how good and how different the three point sources are for Nepal, so we can set σ priors, weights, accuracy targets (Q-005) and the conflation thresholds. It also checks COD-AB's claim-area extent (Q-010).

**Inputs** (pinned by SHA-256 in a spike lock file; sizes checked 2026-11-06):

| Input | URL | Size | Licence class |
|---|---|---|---|
| OSM Nepal extract | `https://download.geofabrik.de/asia/nepal-latest.osm.pbf` (pin the dated file) | 414.9 MB | odbl (main) |
| GeoNames Nepal | `https://download.geonames.org/export/dump/NP.zip` | 3.46 MB | cc-by |
| GeoNames alternate names | `https://download.geonames.org/export/dump/alternateNamesV2.zip` (filter to NP IDs) | 205.2 MB | cc-by |
| GeoNames hierarchy | `https://download.geonames.org/export/dump/hierarchy.zip` | 2.07 MB | cc-by |
| Wikidata Nepal snapshot | SPARQL on QLever/WDQS: items with `P17 wd:Q837` and `P625`, plus P31, P131, P1566, P402, P1082, P2044, P576, labels (en, ne) | ~9.6 k items, a few MB (**unverified** size) | cc0 |
| Official local levels | HDX COD-AB Nepal v02 (`https://data.humdata.org/dataset/cod-ab-npl`) | ~26 MB | cc-by-igo |
| Comparison boundaries | geoBoundaries NPL ADM3 (774 units, CC BY-SA 4.0: **not admissible**, used for the measurement only, never ingested) | 46.8 MB | share-alike |
| DEM | Copernicus GLO-30 COGs, 45 tiles over N26–N30 × E080–E088 (`s3://copernicus-dem-30m/…`), about 45 MB each | ≤ ~2 GB | custom (main) |
| DEM, lite comparison | NASADEM over the same tiles | ~1 GB (**unverified**) | cc0 |
| Buildings (optional) | Google Open Buildings v3, Nepal cells | **unverified** | cc-by |

The total stays under 5 GB, within one laptop or runner job. Whether measuring with a share-alike source counts as "reading" it under CLEAN_ROOM rule 2 is part of Q-012. Treat the spike as a non-shipping analysis whose outputs are only statistics.

**Metrics:**

1. **Duplicate rate per source:**
   - intra-source: the share of a source's records that the conflation clusters with another record *of the same source* at P(match) ≥ 0.9;
   - cross-source: the share of each source's records matched to the others;
   - plus a labelled sample of 500 pairs to measure precision and recall at the chosen threshold.
2. **Rounded-coordinate rate:** the share on the 1′, 0.1′ and 1″ grids and with ≤ k decimals, **minus the uniform baseline**, with 95% binomial intervals. Also the **stacked-point rate** (shared identical coordinates) and the **centroid-snap rate** (within 50 m of an admin polygon's centroid or polylabel point).
3. **Position disagreement:** distance from each source point to the consensus point (P50/P90/P99), by class (city, town, village, hamlet/locality) and by terrain (Terai, hill, mountain, from DEM elevation bands).
4. **DEM disagreement:**
   - stated elevation (GeoNames `elevation`/`dem`, OSM `ele`, Wikidata P2044) minus Copernicus at the *source* point, and minus Copernicus at the *consensus* point (P50/P90/P99);
   - the expected vertical error σ_h·tan(slope) beside the observed error;
   - EGM96 and EGM2008 datums reconciled, as in the timezone-and-elevation note.
5. **Hierarchy staleness against the 753 local levels:**
   - **coverage:** each source's local-level count against 753 (GeoNames shows 754 `ADM3` dated 2019-10-16; OSM shows 749 `admin_level=7` relations);
   - **staleness:** the share of place records whose stated parent (GeoNames admin codes and `hierarchy.txt`, Wikidata P131, OSM `is_in*`/relation membership) is a pre-2017 unit (GeoNames `ADM4H`/`ADM5H`; Wikidata items with P576 dissolved before 2017-03-10, a date that is **unverified**);
   - **mismatch:** the share whose stated parent differs from point-in-polygon against COD-AB.
6. **Name agreement:** the share of matched clusters whose normalised Latin names agree, and the Devanagari coverage per source.
7. **COD-AB claim area (Q-010):** whether the national outline contains the Kalapani–Lipulekh–Limpiyadhura polygon (OSM relation 13559363 for the comparison, in main only).

**Implementation plan in the forge** (spike code lives in `forge/` with the real stage boundaries, so it is not thrown away):

1. Write a spike `sources.lock` and the `fetch` stage, which downloads, verifies SHA-256 and writes to the content store.
2. Build the readers: `osm-places`, `osm-admin` (with the assembler, checked against `osmium export` for B2), `geonames`, `wikidata-snapshot`, `codab`, `dem-sample`. Each outputs Parquet with a common `source_record` schema (source, source_id, class, names, i32 lat/lon, stated elevation, stated parents, licence class).
3. Run `hierarchy-pip`: a `geo-index` packed tree over the COD-AB local levels plus exact `Contains`.
4. Run `geofix-detect`: the rounding, stacking and centroid-snap flags and the baseline statistics.
5. Run `conflate`: blocking (H3 res 7 k-ring + name key + shared IDs), Fellegi–Sunter with EM, union-find with cannot-links. Cross-check with Splink 5.0 run on the same Parquet as an oracle, outside the forge.
6. Run `geofix-consensus`: weighted Weiszfeld, then DEM sampling at the source and consensus points.
7. Produce the report: DuckDB SQL over the Parquet outputs gives the metric tables in §S5 and writes `docs/02-research/notes/s5-nepal-results.md` with the numbers.
8. Run the double-run check on steps 2–6, which must give identical Parquet bytes.

Budget: about 4 GB of disk and under 4 GB RSS on the laptop.

## 7. Risks and open items

- **Recommended forge architecture** (feeds `docs/04-architecture/forge.md`):

  ```
  sources.lock ─► fetch (mirror, SHA-256) ─► read/<source> (Parquet: source_record, source_geom)
     │ licence gate at schedule time (lite never schedules odbl/share-alike readers)
     ▼
  normalise (names via teigeodata-names, quantise to 1e-7° i32)
     ▼
  geofix-detect ─► block ─► score (F–S) ─► cluster (UF + cannot-link) ─► corrections(apply/retire/stale)
     ▼
  geofix-consensus (Weiszfeld + mean-shift + DEM) ─► topology (snap-round, arcs, slivers, LODs)
     ▼
  hierarchy (packed R-tree PIP) ─► names (canonical spelling, translit) ─► postal ─► timezone
     ▼
  QA gates (reports + thresholds) ─► pack writer (.tgd) ─► sign ─► catalog
  ```

  - Intermediates are **Parquet (zstd, fixed settings) with GeoArrow/WKB geometry**. They are inspectable with DuckDB, range-readable and compact.
  - Arrow IPC is used only for in-machine hand-offs where decode cost matters. That needs measuring; it may never be needed.
  - The `.tgd` format is used only at the end. We do not invent a third intermediate format.
  - Every row carries `prov` (source, source_id, source_version, stage, rule) for ADR-0023.

- **Risks:**
  - geoarrow-rs API churn, so pin exact versions.
  - DuckDB is tempting to slip into the build path. G-det would catch that.
  - Geofabrik's dated-extract retention (mirror mitigates).
  - WDQS rate limits and timeouts (QLever as fallback).
  - The Nepal phonetic key needs its own design (names note).
  - EM may converge to a degenerate solution on small countries. Fix the m priors from a larger region, or set them by hand, as Splink advises.
  - Using share-alike boundaries in measurement only (Q-012).
- **Open items:**
  - B1–B3 benchmarks.
  - Whether `tiff` supports the float predictor.
  - The exact Splink TF formula.
  - The WOF embedding de-duplication method.
  - The artifact quota for public repositories.
  - The effective free disk on runners.
  - The licence of `osmpbfreader`.
  - The effective date of Nepal's local-level structure (2017-03-10, **unverified**).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| osmpbf | 0.3.8 (2025-10-24) | 2026-11-06 | crates.io API |
| osmpbfreader | 0.19.1 (2025-05-26) | 2026-11-06 | crates.io API |
| osmio | 0.16.1 | 2026-11-06 | crates.io API |
| libosmium | v2.23.1 (2026-04-01) | 2026-11-06 | GitHub releases API |
| arrow / parquet / arrow-ipc | 60.1.0 (2026-10-28) | 2026-11-06 | crates.io API |
| geoarrow / geoparquet / geoarrow-array | 0.10.0 (2026-11-06, MSRV 1.95) | 2026-11-06 | crates.io API |
| duckdb (crate) | 1.20000.1 (2026-10-28) = DuckDB v2.0.0 (2026-10-21) | 2026-11-06 | crates.io, GitHub |
| sedona / geodatafusion | 0.5.0 / 0.6.0 | 2026-11-06 | crates.io API |
| object_store | 0.14.2 | 2026-11-06 | crates.io API |
| tiff / geotiff / async-tiff / gdal | 0.11.3 / 0.1.0 / 0.4.0 / 0.19.0 | 2026-11-06 | crates.io API |
| geo / geo-types | 0.33.1 / 0.7.20 | 2026-11-06 | crates.io API, docs.rs |
| rstar / geo-index / robust / polylabel | 0.14.0 / 0.4.0 / 1.2.0 / 3.3.1 | 2026-11-06 | crates.io API |
| i_overlay / topojson / geos | 9.1.0 / 0.5.1 (2022) / 11.3.1 | 2026-11-06 | crates.io API |
| h3o / s2 / kiddo / spade | 0.11.1 / 0.2.0 / 6.3.0 / 2.15.1 | 2026-11-06 | crates.io API |
| rayon / libm | 1.12.0 / 0.2.16 | 2026-11-06 | crates.io API |
| hashbrown / rustc-hash / foldhash / ahash / xxhash-rust / blake3 | 0.17.1 / 2.1.3 / 0.2.0 / 0.8.12 / 0.8.19 / 1.8.7 | 2026-11-06 | crates.io API |
| rphonetic / strsim / jaro_winkler / unicode-normalization / icu_normalizer | 4.0.0 / 0.11.1 / 0.2.1 / 0.1.26 / 2.4.0 | 2026-11-06 | crates.io API |
| petgraph / union-find / salsa | 0.8.3 / 0.4.4 / 0.28.5 | 2026-11-06 | crates.io API |
| serde_yaml / serde-saphyr / serde_yml / serde_norway | 0.9.34+**deprecated** / 2.1.1 / 0.0.13 / 0.9.42 | 2026-11-06 | crates.io API (YAML crate choice: serde-saphyr is maintained; serde_yaml deprecated) |
| csv / zip / bzip2 / zstd / flate2 | 1.4.0 / 9.4.1 / 0.6.1 / 0.14.3 / 1.1.11 | 2026-11-06 | crates.io API |
| fs4 / sysinfo / tempfile / camino | 1.1.0 / 0.40.1 / 3.27.0 / 1.2.6 | 2026-11-06 | crates.io API |
| Splink | v5.0.0 (2026-09-18) | 2026-11-06 | GitHub releases API |
| mapshaper | v0.7.101 (2026-10-29) | 2026-11-06 | GitHub releases API |
| QLever | v0.6.0 (2026-08-13) | 2026-11-06 | GitHub releases API |
| Overture release | 2026-10-21.0 | 2026-11-06 | STAC catalog |
| Geofabrik nepal-latest.osm.pbf | 414,863,945 B, modified 2026-11-05 | 2026-11-06 | HTTP HEAD |
| GeoNames NP.zip / allCountries / alternateNamesV2 / hierarchy | 3,459,538 / 422,528,725 / 205,188,519 / 2,073,538 B | 2026-11-06 | HTTP HEAD |
| Wikidata latest-all.json.bz2 / latest-truthy.nt.bz2 | 103,565,615,653 B (2026-10-27) / 43,634,466,369 B | 2026-11-06 | HTTP HEAD |
| OSM planet PBF | planet-261102, 95,715,394,709 B | 2026-11-06 | HTTP HEAD |
| Copernicus GLO-30 tile N27E085 | 44,874,244 B | 2026-11-06 | HTTP HEAD |
| geoBoundaries NPL ADM1/ADM2/ADM3 | 7 (CC BY-IGO) / 75 (2006, PD) / 774 (CC BY-SA 4.0) | 2026-11-06 | geoBoundaries API |

## 9. Sources

- [Fellegi & Sunter, "A Theory for Record Linkage"](https://doi.org/10.1080/01621459.1969.10501049), *JASA* 64(328), 1969.
- [Splink: Fellegi–Sunter model](https://moj-analytical-services.github.io/splink/topic_guides/theory/fellegi_sunter.html), MoJ Analytical Services, accessed 2026-11-06.
- [Splink: model training rationale](https://moj-analytical-services.github.io/splink/topic_guides/training/training_rationale.html), accessed 2026-11-06.
- [Splink: term-frequency adjustments](https://moj-analytical-services.github.io/splink/topic_guides/comparisons/term-frequency.html), accessed 2026-11-06.
- [Linacre et al., "Splink: Free software for probabilistic record linkage at scale"](https://doi.org/10.23889/ijpds.v7i3.1794), *IJPDS* 7(3), 2022.
- [Enamorado, Fifield & Imai, "Using a Probabilistic Model to Assist Merging of Large-Scale Administrative Records"](https://doi.org/10.1017/S0003055418000783), *APSR* 113(2), 2019 (fastLink).
- [Papadakis et al., "Blocking and Filtering Techniques for Entity Resolution: A Survey"](https://doi.org/10.1145/3377455), *ACM CSUR* 53(2), 2020.
- [Hassanzadeh et al., "Framework for Evaluating Clustering Algorithms in Duplicate Detection"](https://doi.org/10.14778/1687627.1687771), *PVLDB* 2(1), 2009.
- [Recchia & Louwerse, "A Comparison of String Similarity Measures for Toponym Matching"](https://doi.org/10.1145/2534848.2534850), COMP@SIGSPATIAL 2013.
- [Santos et al., "Toponym matching through deep neural networks"](https://doi.org/10.1080/13658816.2017.1390119), *IJGIS* 32(2), 2018.
- [Sehgal, Getoor & Viechnicki, "Entity resolution in geospatial data integration"](https://doi.org/10.1145/1183471.1183486), ACM GIS 2006.
- P. Christen, *Data Matching*, Springer, 2012.
- [Overture GERS overview](https://docs.overturemaps.org/gers/) and [bridge files](https://docs.overturemaps.org/gers/bridge-files/), Overture Maps Foundation, accessed 2026-11-06.
- [Overture attribution and licences](https://docs.overturemaps.org/attribution/), accessed 2026-11-06.
- [Overture STAC catalog](https://stac.overturemaps.org/catalog.json), accessed 2026-11-06.
- [Who's On First: de-duplicating venues with vector embeddings](https://whosonfirst.org/blog/2024/08/), 2024-08, accessed 2026-11-06.
- [Pelias API: concordance-based deduplication PR](https://github.com/pelias/api/pull/1606), 2022, accessed via search 2026-11-06.
- [Weiszfeld (1937); Vardi & Zhang, "The multivariate L1-median and associated data depth"](https://doi.org/10.1073/pnas.97.4.1423), *PNAS* 97(4), 2000.
- [Lopuhaä & Rousseeuw, "Breakdown points of affine equivariant estimators…"](https://doi.org/10.1214/aos/1176347978), *Ann. Statist.* 19(1), 1991.
- [Comaniciu & Meer, "Mean shift: a robust approach toward feature space analysis"](https://doi.org/10.1109/34.1000236), *IEEE TPAMI* 24(5), 2002.
- [Agafonkin, "A new algorithm for finding a visual center of a polygon"](https://blog.mapbox.com/a-new-algorithm-for-finding-a-visual-center-of-a-polygon-7c77e6492fbc), Mapbox, 2016.
- [Leutenegger, Lopez & Edgington, "STR: a simple and efficient algorithm for R-tree packing"](https://doi.org/10.1109/ICDE.1997.582015), ICDE 1997.
- [Shewchuk, "Adaptive Precision Floating-Point Arithmetic and Fast Robust Geometric Predicates"](https://doi.org/10.1007/PL00009321), *DCG* 18, 1997.
- [Hobby, "Practical segment intersection with finite precision output"](https://doi.org/10.1016/S0925-7721(99)00021-8), *CGTA* 13(4), 1999.
- [Saalfeld, "Topologically consistent line simplification with the Douglas–Peucker algorithm"](https://doi.org/10.1559/152304099782424901), *CaGIS* 26(1), 1999.
- [Visvalingam & Whyatt, "Line generalisation by repeated elimination of points"](https://doi.org/10.1179/000870493786962263), *Cartographic Journal* 30(1), 1993.
- [TopoJSON specification](https://github.com/topojson/topojson-specification), Bostock, accessed 2026-11-06.
- [mapshaper wiki and command reference](https://github.com/mbloch/mapshaper/wiki), Bloch, accessed 2026-11-06.
- [osmpbf](https://github.com/b-r-u/osmpbf) and [docs.rs/osmpbf](https://docs.rs/osmpbf), accessed 2026-11-06.
- [osmpbfreader-rs](https://github.com/TeXitoi/osmpbfreader-rs), accessed 2026-11-06.
- [libosmium area assembly](https://osmcode.org/libosmium/), osmcode, accessed 2026-11-06.
- [docs.rs/geo 0.33](https://docs.rs/geo) (item list), accessed 2026-11-06.
- [GitHub Actions limits](https://docs.github.com/en/actions/reference/limits), GitHub, accessed 2026-11-06.
- [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), GitHub, accessed 2026-11-06.
- [About releases (asset limits)](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases), GitHub, accessed 2026-11-06.
- [actions/upload-artifact](https://github.com/actions/upload-artifact), GitHub, accessed 2026-11-06.
- [Google Open Buildings](https://sites.research.google/gr/open-buildings/), Google Research, accessed 2026-11-06.
- [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), accessed 2026-11-06.
- [GeoNames dump directory and readme](https://download.geonames.org/export/dump/), accessed 2026-11-06.
- [Wikidata entity dumps](https://dumps.wikimedia.org/wikidatawiki/entities/), Wikimedia, accessed 2026-11-06; [WDQS](https://query.wikidata.org/) count query run 2026-11-06.
- [Overpass API](https://overpass-api.de/) count queries (place nodes; admin_level=7 relations in Nepal), run 2026-11-06.
- [geoBoundaries API, NPL](https://www.geoboundaries.org/api/current/gbOpen/NPL/ADM3/), accessed 2026-11-06.
- [Nepal COD-AB](https://data.humdata.org/dataset/cod-ab-npl), OCHA/HDX, accessed via the Nepal sources note 2026-11-06.
- [Copernicus DEM on AWS](https://copernicus-dem-30m.s3.amazonaws.com/tileList.txt), accessed 2026-11-06.
- [Reproducible Builds](https://reproducible-builds.org/docs/), accessed 2026-11-06 (practice reference).
