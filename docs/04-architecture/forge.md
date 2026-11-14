---
title: "The forge (data pipeline)"
status: draft
updated: 2026-11-13
---

# The forge (data pipeline)

The forge turns pinned sources into packs ([ADR-0035](../03-decisions/adr/ADR-0035-forge-architecture.md)).
It is a separate Cargo workspace, `forge/`, so its heavy dependencies never reach the runtime
crates ([ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md)). The research is the
[forge note](../02-research/notes/forge-pipeline.md). Stages land as the spikes and phases need
them; this page describes what exists and the planned order.

## Stages

```
sources.lock ─► fetch ─► read/<source> ─► normalise ─► geofix-detect ─► block ─► score ─► cluster
   ─► corrections ─► geofix-consensus ─► topology ─► hierarchy ─► names ─► postal ─► timezone
   ─► QA gates ─► pack writer ─► sign ─► catalog
```

| Stage | State | Notes |
|---|---|---|
| fetch | **built** (S5) | below |
| read/osm, read/geonames, read/wikidata | **built** (S5) | below |
| read/boundaries, pip | **built** (S5) | below |
| geofix-detect | **built** (S5) | below |
| links | **built** (S5) | below |
| conflate (block, score, cluster) | **built** (S5) | below |
| aliases | **built** (S5) | below |
| dem | **built** (S5) | below |
| dem-ref, geoid | **built** (S6) | below |
| dem-section (the DEM section from a tileset) | **built** (3.4); run by `make` as `dem`, sampled by `build` for place elevations (D-111) | below |
| build (pack writer, sign, read back) | **built** (3.1) | below |
| make (every stage, content-keyed cache) | **built** (3.1) | below |
| buildings (OSM, Microsoft, Google) | **built** (3.2) | below |
| core (settlement core, weighted Weiszfeld consensus) | **built** (3.2) | below |
| labels (hand-labelled truth set) | **built** (3.2) | below |
| topology (shared arcs, dissolve, levels of detail, polygons) | **built** (3.3) | below |
| postal (codes from country data, checked against their source) | **built** (3.5), inside `build` | [below](#build-a-pack-build) |
| names (display names, lexicon respelling, romanisation) | **built** (3.6), inside `build`; `forge romanise` for review | [below](#build-a-pack-build) |
| the rest | Phase 3 | [phase 3](../07-planning/phases/phase-3.md) |

**ID allocation (pack writer, D-075, D-094):** a country's first build allocates serials in
the Hilbert order of its rows, and later builds give new places the next serials through the
[ID registry](#ids-across-releases). ID order then follows row order, which makes IDMAP
0.35 B per place instead of 4.6–6.4 (B5). Redirect chains are flattened, so every survivor in
REDIRECTS is a current place.

## Sources and `fetch`

Every input is listed in `data/sources.lock` (schema `teigeodata/sources/v1`) with:
- its upstream URL, its version and the date it was retrieved;
- its SHA-256 and size;
- its licence and licence class ([`CLEAN_ROOM.md`](../../CLEAN_ROOM.md) rule 1);
- the countries it covers and what uses it.

`cargo xtask check-data` validates the file in CI and in the pre-commit hook. It rejects an
unpinned (all-zero) hash, a `latest` URL not marked `moving`, an unknown licence class, and a
country with no folder.

`forge fetch [--pin] [--verify] [<id>…]` works like this:
- It downloads each source into the content store `data/cache/store/sha256/<hex>`, streaming
  through SHA-256, and moves the file into place only when the bytes match the pin. A mismatch is
  an error that names both hashes.
- A stored file is not downloaded again. `--verify` re-hashes it.
- `--pin` records new bytes in the lock (the hash, size and date lines only; comments are
  kept). Use it for a first fetch, or for a deliberate update of a moving source.
- A `sparql` source posts its query template (`forge/queries/*.rq`) with the parameters from
  the lock. Templates are world-general: a country's Wikidata item comes from its profile.
- A pinned fetch prefers `mirror_url`, our content-addressed mirror (D-127): `{sha256}` in it
  becomes the pinned hash of the file, or of each tile of a tileset; a mirrored file is a plain
  GET whatever the source's kind. Pinning (`--pin`) always reads the upstream.
- A `tileset` source (a DEM) pins a manifest, `data/sources/<id>.tiles`, with one line per
  tile: name, SHA-256 and size, or `absent` where upstream has no tile (the sea). The lock pins
  the manifest's hash. The tile names come from the `bbox` and `tile` parameters, and the url
  carries `{tile}`. Four tiles download at once; `--pin` writes the manifest (D-061).
- A tileset with a `query` posts one request per tile to a subsetting API instead: the body is
  the template rendered with `query_params` and the tile's corners (`{{W}}`, `{{S}}`, `{{E}}`,
  `{{N}}`). `query_params.parallel` sets how many run at once (`1` for a shared public service).
  The stored bytes are the raw response. ICESat-2 ATL08 through SlideRule is fetched this way
  (D-063). With `query_params.response = "sliderule"`, a response that reports a server error
  is fetched again and never pinned.
- `query_params.token_url` names an endpoint that hands out short-lived anonymous read tokens
  (an Azure SAS, as for NASADEM on Planetary Computer). One token is fetched per run and
  appended to each tile url; it is never stored or printed.
- Tile names take `{NS}`/`{EW}` or lower-case `{ns}`/`{ew}`.

`forge path <id>` prints where a fetched source is stored.

**Moving sources.** GeoNames dumps, Wikidata snapshots, the Microsoft building index and
SlideRule subsets change in place and have no dated archive. They carry `moving = true` and
must have a `mirror_url` (`check-data`): the GitHub release `sources` of this repository, one
asset per file named by its SHA-256 (`…/releases/download/sources/{sha256}`).
`cargo xtask mirror [<id>…]` uploads the stored bytes of the named sources (every mirrored
source by default), skipping assets already there, and lists the sources' notices in the
release notes (D-127).

## Readers

`forge read <osm|geonames|wikidata> <source-id> [<source-id>…]` writes three Parquet tables
to `data/build/read/<source-id>/`:

| Table | Columns |
|---|---|
| `records.parquet` | source, ID, source class, kind, display name, lat/lon on the 1e-7° grid, stated elevation, stated DEM elevation, population, end date, licence class |
| `names.parquet` | ID, text, language, kind (`name`, `label`, `alias`, `official`, `alternate`, `old`, `short`, `colloquial`) |
| `links.parquet` | ID, `parent` or `same_as`, target |

**Built for planet-size inputs:**
- Every reader streams its input. The rows it keeps are held and sorted in memory, which is
  bounded by the shard: the forge shards by country (ADR-0035). A shard larger than memory
  would need an external sort.
- The OSM reader tests tags lazily and allocates only for places and boundaries.
- `GeoNames` lines are split into fixed arrays, with no allocation per line.
- Tables are written in bounded 128 Ki-row batches.
- Rows are sorted and the writer settings are fixed, so a second run gives identical bytes.

Measured on Nepal, 2026-11-06 (M2 Pro):

| Reader | Input | Rows | Time | Peak RSS |
|---|---|---|---|---|
| osm | 415 MB PBF | 31,326 places and boundaries | 1.2 s (parallel) | 128 MB |
| geonames | NP dump plus a stream through all 205 MB of world alternate names, and the hierarchy | 148,210 records, 182,406 names | 3.4 s | 201 MB |
| wikidata | QLever snapshot | 9,586 records, 69,451 names | 0.07 s | 49 MB |

`read::overture` (D-155) is not a `forge read` stage: `forge world` calls it on the pinned
`overture-divisions` tiles. It streams each `GeoParquet` file with a row filter on `subtype` and
`class`, so only land areas of countries, dependencies and regions are decoded (WKB, checked
counts), and joins `division` for Wikidata and the OSM relation. The world's 4,201 land areas
and 4,201 divisions: 3.2 s, peak 1.19 GB (2026-11-10), counts equal to `DuckDB`'s.

## Point in polygon (`pip`)

`forge pip <boundary-source> <level> <points-source>` reads one level of a zipped GeoJSON
boundary set (COD-AB layout: `…admin<level>.geojson`, properties `adm<level>_pcode` and
`adm<level>_name`; the generalised `_em` files are skipped) and assigns every point of a read
output to the unit containing it. Output: `data/build/pip/<points>/<boundary>-adm<level>.parquet`
(`id`, `pcode`, `name`; empty when outside every unit). The summary counts points outside every
unit and points inside more than one, which is a topology error in the source.

- Polygons are quantised to the 1e-7° grid. The even-odd test uses exact `i128` cross
  products, so no point is misclassified by rounding; a point on an edge is inside.
- A uniform 0.1° grid over bounding boxes picks the candidate polygons; points run in parallel.

Measured on Nepal, 2026-11-06 (M2 Pro), 21,686 OSM points:

| Level | Units | Time | Peak RSS |
|---|---:|---:|---:|
| 0 | 1 | 0.36 s | 29 MB |
| 1 | 7 | 0.27 s | 27 MB |
| 2 | 77 | 0.15 s | 30 MB |
| 3 | 775 | 0.32 s | 63 MB |

Every level assigns the same 21,125 points and none to two units, so the levels nest. The 561
points outside are mostly just across the border in India and China (the Geofabrik extract is
cut with a buffer), plus 20 in the Limpiyadhura claim area, which COD-AB excludes (D-054).

## Geofix detectors (`geofix`)

`forge geofix <points-source> [<boundary-source>]` flags every point that was rounded, stacked
or snapped to a unit centroid, and writes `data/build/geofix/<points>/flags.parquet` (`id`,
`decimals`, `grid_1min`, `grid_01min`, `grid_1sec`, `stack`, `snap_level`, `snap_m`) and a
`summary.md` of rates against a measured null, with 95% intervals. The method and the Nepal
results are in [S5](../02-research/benches/s5-nepal.md#rounding-stacking-and-centroid-snap-s54).

- Every grid test is integer arithmetic on the 1e-7° grid.
- Stacks come from one parallel sort of the points, not a hash map of strings.
- Centroids are exact area centroids (holes subtracted whatever the ring orientation), in a
  0.01° cell index for the 50 m query. The levels come from the boundary set itself.

## Stated links (`links`)

`forge links <points-source> <points-source> […]` joins every `same_as` link of the given read
outputs with union-find and writes `data/build/links/<sources joined by +>/pairs.parquet`
(`a`, `b`, `stated`, `metres`) and a `summary.md`. The summary gives position disagreement by
source pair and by the kinds of both ends, counts copies (< 1 m) apart, and counts clusters
holding two records of one source. Results: [S5](../02-research/benches/s5-nepal.md#stated-links-and-position-disagreement-s55-first-part).

## Conflation (`conflate`)

`forge conflate <cc> <points-source> <points-source> […]` matches records across sources. The
method follows the forge note:
- **Blocking:** a shared name key within neighbouring 0.1° cells; the phonetic key is named by
  the country's profile.
- **Model:** Fellegi–Sunter, with u from seeded random pairs and m by EM over pattern counts.
- **Clustering:** union-find by descending weight; it never joins two records of one source,
  or a populated place and an admin unit. Two exceptions, both from stated links: a source's
  duplicates (two records stating the same record of another source, of its kind, sharing a
  name key within 10 km) join first (D-125); a populated place and a former unit stated as one
  join after the model (D-118).

It writes `data/build/conflate/<sources>/clusters.parquet` (`id`, `cluster`, named by the
smallest member) and a `summary.md`. The summary holds the model's weights, the clusters by
source composition and the silver-label evaluation. G-conf (D-131) gates that evaluation:
every source pair with 100 links or more keeps precision at least 0.98, and recall at least
the profile's `conflation.min_recall` (Nepal 0.80; measured 0.832 and 0.882), or the stage
fails. Names are compared with
`teigeodata-names`, the code path search uses. Results:
[S5](../02-research/benches/s5-nepal.md#the-matcher-s55-second-part).

## Aliases (`aliases`)

`forge aliases <cc> <anchor-source> <a+b+…>` carries every name of the other members of a
conflation cluster to the anchor record, subject to the corroboration and collision rules
(D-060). It writes `data/build/aliases/<run>/<anchor>.tsv` (anchor, text, language, kind,
member) and the list of uncorroborated members, a QA queue for upstream fixes. Results:
[S5](../02-research/benches/s5-nepal.md#aliases-into-search-s55-third-part).

## DEM sampling (`dem`)

`forge dem <tileset-source> <points-source> […]` samples the tileset at every record's point
and writes `data/build/dem/<tileset>/<source>.parquet` (`id`, `dem_m`, `slope_deg`,
`stated_m`, `source_dem_m`) and a `summary.md`:
- stated elevations against the DEM, overall and by slope band;
- for every stated link of the same sources (a `links` run), the height difference between
  the two points.

- Points are grouped by tile and each tile is decoded once, four at a time, so memory is
  bounded whatever the extent (about 52 MB per Copernicus tile).
- The grid comes from the GeoTIFF tags, raster type included, so a PixelIsPoint tile is not
  shifted by half a pixel. Heights are bilinear and slope is Horn's method in metres.
- Measured on Nepal: 179,470 points in 7.0 s, 278 MB. Results:
  [S5](../02-research/benches/s5-nepal.md#dem-disagreement-s56).

## DEM section (`dem-section`)

`forge dem-section <tileset> <west,south,east,north> [--step-mm 2000] [--no-refine]
[--thresholds a,b,c,d,e,f,g]` codes the source tiles of a 1″ tileset over a box of whole
degrees into a [DEM section](pack-format.md#dem) (D-104).

- **Streaming:** strips of 3 source columns, in bands of 64 rows from north to south. Only the
  source tiles the current band needs stay decoded, as quantised i32, so memory is bounded by
  the strip, not the box (206 MB for two tiles). Tiles are coded in parallel.
- **Coverage:** samples the source does not cover are filled by repeating the nearest row and
  column, so every tile in the box is present; a country mask comes with the pack integration.
- **Checks:** the writer decodes every tile back; the section is then checked whole
  (`Dem::check`), read back from a pack, and 20,000 random points are compared with bilinear
  sampling of the source: they must agree within half a step.
- **Output:** `data/build/dem-section/<tileset>/dem.tgd` (a pack of the section alone, for
  B11; not a pinned source) and `summary.md`. Results:
  [B11](../02-research/benches/phase-3.md#b11-dem).

## DEM against a ground reference (`dem-ref`) and the geoid

`forge dem-ref <dem-tileset> <reference-tileset> <geoid-source>` compares the DEM with ICESat-2
ATL08 terrain heights, the method of the
[elevation-reference note](../02-research/notes/elevation-reference.md):
- Each reference tile is a SlideRule response. The decoder takes the Parquet file out of its
  records and rejects a response holding any server error, so a tile with a failed granule or
  beam is never measured as complete.
- ATL08 heights are ellipsoidal, so they become EGM2008 heights (the DEM's datum) by H = h − N.
  N comes from `geoid`, which interpolates GeographicLib's 5′ EGM2008 grid bilinearly.
- A DEM on another geoid names it (`query_params.geoid`, `egm96-5` for NASADEM) and is moved
  per point: H = H_dem + N_dem − N_ref. Float32 and Int16 rasters are read.
- An optional boundary source restricts the measurement to its area (the finest level, exact
  point in polygon).
- Tiles run four at a time, each with its one DEM tile. Statistics go into fixed 0.1 m
  histograms per stratum (slope, land cover, snow, day or night, height), so memory does not
  grow with the number of points.
- Output: the summary, plus every tile's points in `data/build/dem-ref/<dem>/<reference>/`
  for calibrating σ_DEM.

`forge geoid <source> <lat> <lon> […]` prints N at points. On 2026-11-07 it agreed with
GeographicLib's online egm2008-1 values within 0.12 m at five points (Kathmandu, Everest,
Kalapani, the eastern Terai, Sydney); the grid's documented maximum is 0.48 m.

## Buildings (`buildings`)

`forge buildings <osm|msbuildings|google> <source-id> <W,S,E,N>` writes one point per
building to `data/build/buildings/<source-id>/points.parquet` (`lon_e7`, `lat_e7`), sorted by
(lat, lon) so the bytes are the same on every run (D-096). Tiles and blocks are read in
parallel; nothing holds a whole source in memory except the output points.

| Reader | Source | Point |
|---|---|---|
| `osm` | a PBF extract, ways tagged `building` | the way's first node (two passes: ways, then their nodes) |
| `msbuildings` | Microsoft Global ML Building Footprints, gzipped GeoJSON lines per quadkey | vertex mean of the outer ring |
| `google` | Google Open Buildings v3 points, gzipped CSV per S2 level-6 cell | the stated point |

The Microsoft and Google tilesets are pinned by a tile list (`data/sources/<id>.list`,
`tile<TAB>url`), fetched like any tileset. Measured on Nepal on 2026-11-08 (release, Apple
M2 Pro):

| Source | Buildings in the extent | Time | Peak memory |
|---|---:|---:|---:|
| OSM (geofabrik-np) | 8,296,729 | 3.2 s | 621 MB |
| Microsoft (52 quadkeys) | 6,971,297 | 1.7 s | 150 MB |
| Google v3 (17 cells, 1.42 GB) | 32,086,286 of 56,634,864 | 4.6 s | 556 MB |

## Settlement core and consensus (`core`)

`forge core <points run> <buildings run>` gives each conflated populated place and locality
its consensus point (D-091, D-096; method in the
[settlement-core note](../02-research/notes/settlement-core.md) §3.4):

1. **Seed:** the weighted geometric median (Weiszfeld, Vardi–Zhang) of the members' points,
   weight 1/σ, σ the larger of the source prior and the precision floor (D-055); copies within
   1 m count once (D-056).
2. **Cells:** each building belongs to the nearest populated-place seed, so a core never
   climbs into a neighbour's village.
3. **Core:** inside its own cell, buildings on a 25 m grid, a separable Gaussian blur of the
   class bandwidth (hamlet 75 m, village 150 m, town 300 m), steepest ascent from the seed.
   Cities get none. The blur runs only over the occupied box plus its radius, bit-identical
   to the full blur; `exp` and `cos` are our own series, so the bytes do not depend on libm.
4. **Gates:** at least 5 buildings, density at the seed, a mode at least half the window's
   peak, within the class gate (500 m, 1 km, 2 km). The reason is recorded otherwise.
5. **Consensus:** the median again with the core as one observation, σ 75 m.

**Agreement gate:** with Google and Microsoft both read, the cores from each alone must agree
P90 ≤ 180 m. A failure is an error and writes no points. A deliberate violation (each Google
core paired with the next place's Microsoft core) fails it at P90 265 km.

**Border rule (`build`):** a core point outside every admin unit, for a place whose seed lies
in one, is refused; the place keeps its seed point and the summary counts it.

Measured on Nepal on 2026-11-08:

| Run | Places | Cores | Agreement P50 / P90 | Time | Peak |
|---|---:|---:|---:|---:|---:|
| main (OSM, Microsoft, Google) | 98,020 | 88,649 | 25 / 146 m | 11.9 s | 1.14 GB |
| lite (Microsoft, Google) | 90,417 | 81,646 | 35 / 160 m | 11.6 s | 0.94 GB |

A core moves the seed P50 190 m, P90 369 m. Main holds 50 places at their seed by the border
rule and lite 79. Two runs give identical bytes.

## Labels (`labels`)

`forge labels sample <cc> <points-source> <n> <seed>` draws the hand-labelled truth set of
Q-005 (D-062) into `tests/golden/<cc>-places.csv`: up to `n` populated places per stratum of
class (hamlet, village, town) and terrain (the source's DEM height below 400 m as lowland),
the lowest BLAKE3 of (seed, record ID) first, every fifth row for a second labeller. The
locator is the source point rounded to 0.01°, and nothing from the forge's outputs is shown,
so labels stay independent of the core they judge (D-060). It never overwrites a file.
`forge labels score <cc> <points run> <buildings run>` reports, per stratum, the source and
consensus distance to the labels against the targets (P90 ≤ 500 m lowland, ≤ 150 m hill).

## Topology (`topology`)

`forge topology <boundary-source>` turns a boundary source's admin units into shared arcs with
levels of detail and every unit into a polygon of arc references (B10, D-088, D-098). Code:
`forge/crates/forge/src/topo/`.

`forge topology --geoboundaries <file> <name>` runs the same stage on one geoBoundaries level
(`shapeID` as the code, `shapeGroup` as the only ancestor, so only the country is dissolved)
and also writes a pack of the geometry sections alone, `data/build/topology/<name>/geometry.tgd`,
each polygon its own row in Hilbert order of its label. It is for measurement: the file is not
a pinned source and feeds no build.

A plain (unzipped) Natural Earth admin-0 file is read as one level coded by `ADM0_A3` (D-147).
Label points are poles of inaccessibility on a local plane scaled at the ring's middle latitude,
on rings thinned to a quarter of the search's precision, within a budget of edge distances
(D-148): world admin 0 takes 6.9 s for the whole stage.

1. **Arcs.** The finest level's rings are cleaned (outer counter-clockwise, holes clockwise) and
   cut at topology nodes: a vertex whose degree is not 2, or where the units on its two edges
   change (REGION's arc rule). A ring without a node is one closed arc from its smallest vertex.
   Nodes come from sorted flat records, not hash maps (D-155): one record per ring vertex (the
   vertex and its two neighbours), a node being a vertex met three or more times or twice with
   different neighbours, and one record per edge for the overlap checks; about 24 B a vertex,
   so the world's 36 M fit. The world part is byte-identical to the hash-map build's.
   A source that is not noded (two edges cross or touch away from a shared vertex) is snap
   rounded first (D-103, [note](../02-research/notes/snap-rounding.md)): every edge that meets
   a hot pixel (a vertex, or a crossing rounded) goes through its centre, so no input vertex
   moves and edges move by at most 8 mm. A noded source is left unchanged. Any crossing left
   after that stops the stage.
2. **Dissolve.** Every coarser level the source names (the country at least) is rebuilt from
   the finest units by their stated ancestor codes (`adm<l>_pcode`): arcs used in both directions cancel, the rest
   chain into rings (the sharpest left turn at a vertex where a ring touches itself), and holes
   go to the smallest outer ring holding them. Each derived unit is compared with the source's
   own polygon of it both ways (G-topo, D-130): a source vertex more than 1 m from the derived
   border, a derived vertex more than 1 m from the source's border, or a different number of
   holes stops the stage. The second and third catch a gap between children, which leaves a
   hole in the dissolved parent whose vertices the first never looks at.
3. **Levels of detail.** Douglas–Peucker importance per vertex in metres, capped by the parent
   split; levels 1 to 6 at 5 m, 20 m, 100 m, 500 m, 2 km and 5 km; ends pinned, and a closed arc
   pins two more vertices. A guard, coarsest level first, gives back the most important removed
   vertex of every segment that crosses or touches another and of every ring that loses its area
   or orientation. A level the guard cannot fix (the summary's "Left" above 0) stops the stage.
4. **Measure.** Area on the WGS 84 authalic sphere; the label is the pole of inaccessibility
   (polylabel's method, our own code, to 1/1000 of the unit's size and at least 10 m), kept
   only if exactly inside.
5. **Verify.** The written ARCS payload is read back through `teigeodata_format::arcs`, passes
   its check, and at every level of detail has no crossing and no degenerate ring. A planted
   guard that adds nothing back fails here.

Output in `data/build/topology/<source>/`: `arcs.bin` (the ARCS payload), `polygons.tsv`
(level, P-code, label, hectares, the box of every ring vertex, parts as references) and
`summary.md`. `build --topology` links each polygon to its place (the country for level 0,
`admin:<pcode>` otherwise), writes POLYGONS grouped by level, POLY_INDEX over the boxes
(D-099) and ADMIN_REGION on the arcs (D-100), and reads them back with their checks;
POLY_INDEX's check recomputes every box from the arcs, so a wrong box in `polygons.tsv` stops
the build, and ADMIN_REGION must give the same polygons as POLY_INDEX with
`Polygons::contains` at every label point and every 97th place. G-pip (D-130): through the
written ADMIN_REGION, every settlement and locality inside the admin coverage lies in exactly
one unit of each level, and its parent is the deepest; at most 0.5% lie outside every unit
(Nepal main 180 of 96,805, 0.19%, median 536 m past the border; lite 28, 0.03%).
G-tz (D-139): each source record that states a timezone (GeoNames `timezone`) is compared with
its place's `PLACE_TZ`; at most 0.1% may differ. Nepal 2026-11-09: main 28 of 93,701 (0.030%:
places just outside Nepal on timezone-boundary-builder's map, the G-pip set); lite 9 of 93,621
(0.010%: far-western border records, Kalapani among them, that GeoNames puts in
`Asia/Kolkata`).

Zone rules (D-196, [ADR-0039](../03-decisions/adr/ADR-0039-tz-rules.md)): when a pack has
timezone areas, the forge builds `zic` from the pinned tzcode of the outlines' tzdb release
(`tzcode-2026f`; the release is the timezone source's `version` in `sources.lock`), runs
`zic -b slim` on tzdata's main files and `backward` in a scratch directory under
`data/build/tz-rules/`, removes it, and writes `TZ_RULES` with the manifest tag `tzdb`. Both
tarballs pass the licence gate and join the attribution. G-tzrules: every area has its rules
linked to its row, each zone rebuilds byte for byte as `zic` wrote it, and the tag is the
compiled release; a zone `zic` did not compile stops the build. Split parts carry the rules
of their own zones. Measured on 2026f: 93,232 B for the world's 444 outline zones; 2,492 B
for Nepal's three.

Measured on COD-AB Nepal on 2026-11-08:

| Measure | Value |
|---|---:|
| Polygons (levels 0 to 3) | 860 |
| Arcs, vertices | 2,315 arcs, 593,610 vertices (33.8% of 1,754,996 ring vertices) |
| Derived parents against the source's | 99.6% of source vertices on the border, the rest within 1 cm, area 0.0000% |
| Area against the source's `area_sqkm` | P50 0.002%, max 0.090% |
| Guard | 1 crossing at 5 m, 4 at 500 m, 3 at pinned-only; all fixed, 29 vertices added |
| ARCS | 2.66 MB, 4.48 B per vertex (model 4.47) |
| POLYGONS | 21 KB |
| POLY_INDEX | 8.2 KB, 9.5 B per polygon, 3 levels at node 16 |
| ADMIN_REGION | 977 KB, 63,619 leaves, 47,233 runs; agrees at 1,859 points |
| Time, memory | 1.5 s, 346 MB |
| Packs | main 7.05 → 10.72 MB, lite 4.48 → 8.16 MB |

## Build a pack (`build`)

`forge build <cc> --release <v> --admin <boundary> [--tz <tz>] [--tz-first <zone,…>] [--lite] [--key <seed-file>] [--ids <registry>] [--core <buildings-run>] [--dem <tileset>] <sources…>`
writes `data/build/pack/<cc>/<cc>-places-<main|lite>.tgd`, `summary-<main|lite>.md` and the
ID registry `<cc>-ids-<main|lite>.tsv` from the `conflate` output of the same sources:
- **Licence gate (G-lic).** `--lite` refuses any source whose licence class is not
  public-domain, CC0, CC BY, CC BY-IGO, ODC-By or GODL. The recipe's timezone sources are a
  priority list (D-138): main takes timezone-boundary-builder (ODbL), lite takes tzdb. The
  `--core` buildings run is gated
  too: a lite build refuses a run with a source it may not read.
- **Attribution (D-126).** Every source the gate lets the build read gives its
  `sources.lock` `attribution` line; the pack's manifest carries them once each in reading order
  (tag `attribution`, signed with the head) and `licence` `odbl` or `cc-by-4.0`;
  `attribution-<build>.md` is a copy beside the pack
  ([note](../02-research/notes/attribution.md)).
- **Places.** The country (from its Wikidata item), the admin units of every COD-AB level
  below it (a point inside each polygon, its parent the unit of the level above), one place
  per conflated cluster (a settlement when any record is a populated place, else a locality,
  else a `natural` place for a summit, hill, volcano or pass, D-201; the parent is the deepest
  admin unit holding it, or near it on the border; a current place outside every unit beyond
  the border reach, 200 m for a natural place and 500 m for any other, is the neighbour's and
  left out, listed with its distance in `abroad-<build>.tsv`, D-201, D-239),
  and one `timezone_area` place per IANA
  zone: each timezone-boundary-builder zone that meets the country's box, `--tz-first` zones
  first (D-093, D-084); or, from tzdb, the country's one `zone.tab` zone over its admin-0
  outline, refusing a country with several (D-138). A place keeps the names that fit the search key
  limit; a place with none is left out and counted.
- **Sections.** Rows in Hilbert order (ties by key), so IDs follow rows (D-075): IDMAP,
  PLACES, HIERARCHY, NAMES, SEARCH, SUBSTRING (the word suffixes of every name SEARCH keys,
  with SEARCH's importance, D-199; read back: every place is found from its canonical name's
  longest suffix), PLACE_INDEX, the timezone REGION linked to its places
  (column 16), POSTAL when the country has a `postal.yaml`, and REDIRECTS when the registry
  has retired serials. Importance is provisional (kind, admin level, log population).
- **Display names (D-106).** A settlement or locality shows the majority of its records'
  Latin names after the lexicon's respelling (ties: no diacritics, then source order), else
  the romanisation of its first name by a profile the country lists (`profile.yaml`
  `romanise`), else the first name that fits. Accents and invisible format characters are
  dropped; every source spelling stays a name. `naming-<build>.tsv` lists each place whose
  display name differs from its first source's, for review. `forge romanise <cc>
  <lang>-<Script>` romanises standard input line by line
  ([note](../02-research/notes/names-canonical.md)).
- **Names of other records (D-060, D-107).** A place's anchor is its first record with a
  name, in source priority. Another record lends its names only when one matches an anchor
  name (fold, join or phonetic key, also of its romanisation); a lent name the anchor lacks by
  fold is dropped when it is the primary name of an admin unit, or of another place within
  30 km. `forge aliases` applies the same rules (`aliases::rules`). A name with its context
  after a comma or in brackets is dropped where the part before it is another of the place's
  names or the part after the last comma is the country's or a unit's name (D-202).
- **Corrections (ADR-0036, D-117).** `data/countries/<cc>/corrections/*.yaml`: `same-as` joins
  clusters before naming; `add-name`, `remove-name`, `set-point` and `set-population` apply to
  the built places. Each is retired, applied or stale by its guard and listed in
  `corrections-<build>.tsv`; G-corr fails a build on an old stale one.
  `forge correction-guard <source> <record> <field>` prints a guard.
- **Former units (D-118).** A stated link joins a settlement and a former admin unit after
  the model (shared name key, within 10 km); former units rank below settlements in search.
- **Same place (D-116).** Same-name settlements and localities within 100 m after the core
  are merged into the one with more records; `merged-<build>.tsv` lists each merge.
- **Admin records (D-110).** A cluster of admin records only (GeoNames `A.*`) links to the
  current unit it names (name key and containment, the source's level first, else the named
  unit whose outline is nearest, within 30 km: of the source's level, or of any level for a
  record with no class such as a Wikidata item, never for a division, D-239) and gives it its
  keys and names; a former unit
  (`ADM1H`–`ADM4H`) that links to none becomes an admin place flagged former, without a
  polygon, under its successor one level up; an `ADMD` becomes a locality. The summary counts
  each outcome.
  `dropped-names-<build>.tsv` lists every dropped record or name and what it names.
- **Summits (D-201).** A natural place takes the median height its records state where it
  is within 50 m + 1 % of the DEM at its point or of the DEM's highest sample within 3″, else
  the DEM's; the read-back holds it to the same rule, and the summary gives G-PEAK.
- **Elevation (D-111).** With `--dem <tileset>`, every place but a timezone area takes the
  height of the tileset's `dem-section` (`data/build/dem-section/<tileset>/dem.tgd`) at its
  final point (the `core` consensus point, else its first record's), bilinear through the
  runtime reader, rounded half away from zero to whole metres above EGM2008, into PLACES
  column 7 (D-109). A place outside the section has none. The tileset passes the licence gate
  like any source: the Copernicus terms are main only (D-031), so the lite pack has no
  elevations yet. The summary gives the count, min, median and max, and a QA table by place
  kind: the absolute difference from what each of the place's source records states
  (`elevation_m`) and from the DEM height `GeoNames` states (`dem_m`, SRTM3); stated heights
  are never an input (D-061). The read back samples the section afresh at every row's stored
  1e-6° point and requires the stored height within 1 m, and none for zones and points
  outside it. Measured on Nepal main on 2026-11-09: 96,839 of 96,847 places have a height
  (the 6 zones and 2 Wikidata items outside the box have none), 48 to 8,644 m, median
  1,147 m; PLACES grows from 480,420 to 610,292 B (10.7 bits per place), the pack from
  11,112,860 to 11,242,716 B; the read back finds every row within 0.55 m. Against
  GeoNames' SRTM3 field (87,996 records) the height at the place's point is P50 27 m, P90
  130 m off, but at the record's own point P50 7 m, P90 26 m: the gap is the move to the
  consensus point, not the DEM. Stated `elevation_m` (446 records) is P50 21 m, P90 199 m off.
  A lat/lon swap planted in the sampling fails the unit test (every height `None`) and the
  read back on the real build (row 0: none stored, 4,880 m in the DEM).
- **Postal codes (D-105). `data/countries/<cc>/postal.yaml` (schema `teigeodata/postal/v1`)
  gives each scheme's rule and the codes files. A codes file has one row per admin unit: the
  code, the unit's P-code on the build's boundary source and its number of parts (Nepal: wards).
  Each row becomes an area record of the unit's place and a part record per part (code + a
  2-digit number). Before use the file is checked against its pinned source (`verify`: the
  same codes, each with the same part count, read from the archived GPO table); a P-code that
  is not a unit of the build stops it. A source the build may not read (the lite build and a
  source with no stated licence) leaves its codes out, with a note in the summary. The read
  back looks up every code, finds its record at the unit's place and finds that record again
  through the place index.
- **Determinism (G-det).** The build ID is BLAKE3 of the forge version, `sources.lock` and
  the arguments; the creation time is `SOURCE_DATE_EPOCH` or 0. Two builds give identical
  bytes.
- **Sign and read back.** `--key` signs with an Ed25519 seed (64 hex digits). The pack is then
  opened with the runtime reader: hashes and signature are checked, every section is opened
  and passes `check`, and every place keeps its ID, point, parent and canonical name, is found
  by an exact search of that name and is its own nearest place. Every zone holds its own
  point and links to its row. Every row has the registry's ID, and REDIRECTS passes `check`
  and gives every retired ID the registry's fate. All of this runs on the bytes in memory:
  only a pack that passes is written, and the pack and the registry each replace the old file
  through a temporary file and a rename, so a failed or killed build leaves the last good pack.

Measured on Nepal on 2026-11-07 (release build, Apple M2 Pro):

| Build | Places | Pack | Time | Peak memory |
|---|---:|---:|---:|---:|
| main (geofabrik, geonames, wikidata; tbb 2026f) | 96,844 | 7.05 MB | 3.0 s | about 670 MB |
| lite (geonames, wikidata; no timezones) | 91,266 | 4.26 MB | 1.8 s | |

Main: 2,042 clusters had no usable name; 195 places lie outside every COD-AB unit (border
points) and have the country as their only ancestor. Peak memory comes from parsing the whole
timezone GeoJSON; streaming it is follow-up work.

### IDs across releases

The registry (D-094, [ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)) is a text file,
one line per serial ever allocated in the country's namespace, sorted by serial, with three
tab-separated columns:
- the serial;
- its state: the builds whose packs hold it (`lite,main`), `>N` when it retired into serial N,
  or `x` for a tombstone;
- the source keys the place was made of (`geonames:…`, `osm:node/…`, `wikidata:Q…`,
  `admin:<pcode>`, `tz:<IANA ID>`), so it doubles as the bridge file from source IDs to ours.

A build reads the previous registry with `--ids` and writes the updated one:
1. Each place takes the live serial it shares most keys with. Ties go to a serial this build
   held before, then to the smaller serial. A split keeps the serial with the half holding
   most of its keys.
2. The other places get the next serials in row order. Serials are never reused.
3. A serial that no build holds any more retires into the live serial holding most of its
   keys (a merge), or else becomes a tombstone. Chains are flattened.
4. A build only updates the keys of the namespaces it reads, so a lite build (no OSM) keeps
   a main place's OSM keys.
5. The world part's registries are shared by its parts: each holds its serials under its own
   name, the build for the default rows (`lite`), else the build and what adds rows
   (`lite-geonames-cities5000-towns-in`), so one part never retires another's places
   (D-267). A world build that would retire more than 1% of its holder's serials fails
   unless `--allow-retire`.

The release order is main, then lite on main's registry; the lite registry is the release's
registry, the input of the next main build. Measured on Nepal on 2026-11-07:

| Run | Kept | New | Retired | Redirects |
|---|---:|---:|---:|---:|
| main, first build | 0 | 96,844 | 0 | 0 |
| lite on main's registry | 91,153 | 113 | 0 | 0 |
| main on lite's registry | 96,844 | 0 | 0 | 0 |
| main without OSM (a test of retirement) | 91,272 | 0 | 18 merged, 5,667 tombstones | 5,685 |

The registry is 3.5 MB, 0.42 MB with `zstd -19`. Publishing it with each release and pinning
it in `sources.lock` comes with the catalog (step 3.7).

### Split builds (`--split`)

`forge make <cc> … --split` (D-191, [split builds](../02-research/notes/split-builds.md)) also
writes one pack per unit of the level `data/countries/<cc>/split.yaml` names, at the address
`<cc>-<code>` (ADR-0010), beside the country pack and through the same pack stage
(`build/split.rs`). The country pack's bytes do not change with the flag. A part holds:
- its own places: those whose chain of parents reaches the unit. A place under no unit (a
  border town just outside every unit, a former region) goes to the unit whose box of own
  places is nearest; none is dropped;
- every ancestor of them as a row (the stubs, D-143), under the country pack's IDs;
- the outlines of its own places only, on just the arcs they use (a stub's outline stays with
  the country pack and the baseline);
- the timezone zones cut to its box grown by 0.05° (`geom::clip_ring`); a zone keeps its point
  when the cut zone holds it;
- its own places' postal codes and heights, the redirects to them, and every tombstone.

A part's DEM is masked to its own unit's polygon (D-192). Every mask also keeps the tiles
under each place with a height, so a place's terrain is never masked away (D-201).

G-tz fails only the country pack: a part holds a subset of the same records. After writing,
`split::check` reads every part back against the country pack: each own place is in one part
only, with the same ID, point, canonical name, chain of ancestors and timezone area, and every
row of the country pack is in a part, except a zone that holds no place. A build removes the
parts an earlier build left in its output. Nepal by province on 2026-11-11 (recipe without the
uncached buildings and DEM): 7 parts, 13.49 MB together for an 11.59 MB country pack.

## Run every stage (`make`)

`forge make <cc> --release <v> [--lite] [--key <seed-file>] [--ids <registry> | --fresh-ids] [--split] [--force] [--fetch]`
reads the newest ID registry of the country's builds in `data/build/pack/<cc>/` when no
`--ids` is named, so IDs stay stable from one run to the next, and refuses to run with none
(`cargo xtask ids restore` puts the released ones back); `--fresh-ids` numbers a first build
on purpose (D-240, D-261). It runs a
country's stages from its recipe, `data/countries/<cc>/build.yaml` (checked by
`cargo xtask check-data`): `read` for each points source, `conflate` over them, `buildings`
for each building source the build may read, `core`, `topology` for the admin source, `dem`
(the first tileset of the recipe's `dem` list the build may read, as a `dem-section` over the
tiles holding the corners of its `extent`; D-111, D-132: main Copernicus, lite NASADEM moved
from EGM96 to EGM2008), then `build`
(D-095, [ADR-0035](../03-decisions/adr/ADR-0035-forge-architecture.md)).
- **Fetch.** `--fetch` first brings every input the recipe names (points sources and their
  extras, admin, timezones, buildings, DEM, population grid, postal sources), filtered by the
  lite gate, into the store: pinned bytes only, from the mirror where there is one (D-127). It
  is not part of any key.
- **Keys.** A stage's key is a BLAKE3 hash of the stage name, the stage's code, its pinned
  inputs (ID, SHA-256 and size from `sources.lock`), its configuration (the profile, the
  arguments, the ID registry, the signing seed) and the keys of the stages it reads. The
  build stage's inputs also cover `postal.yaml`, each codes file and the pinned source each
  is checked against. The stage's code is a hash `build.rs` computes over the modules the
  stage reaches through `crate::` paths, `main.rs`, `make.rs`, the path dependencies and
  `Cargo.lock` (D-108), so a names change reruns `topology` and `build` only (12 s).
- **Fast builds.** `cargo build --profile fast` (no LTO, parallel incremental codegen)
  rebuilds the forge in about 19 s after a one-file change, for iteration;
  `forge/target/fast/forge` caches the same stages as the release binary. Benchmarks and
  published numbers use `release`.
- **Stamps.** A stage that succeeds writes `.key` (`.key-main`, `.key-lite` in the pack
  directory) into its output directory: the key, then the BLAKE3 and size of every file it
  owns. A stage is skipped only when both match, so an output changed by hand, or a stage
  run by hand with other arguments, is rebuilt. The stamp is removed before a stage starts
  and written atomically after it ends, so a killed run resumes from the first unfinished
  stage.
- **Licence gate.** `--lite` never schedules a reader for a source outside the lite classes.
- **Wire (G-wire, D-135).** `cargo xtask check-pack` writes the pack as its transport asset
  (zstd 19, as a release does) and counts each frame for its section's component
  (`teigeodata_format::component`). The parts must add up to the asset, and each component the
  recipe budgets in `wire_kb` must fit, with what it needs. Nepal budgets `point: 550`.
- **Budget (G-budget, D-129).** The stage table gives each stage's time and, on Linux, its
  peak resident memory (`VmHWM`, reset per stage through `/proc/self/clear_refs`); the run
  line gives the highest peak and the largest size of `data/build`. A recipe's
  `budget: { rss_mb, disk_mb }` makes `make` fail when a stage or the output goes over it.
  Nepal: 964 MB peak (the whole process, macOS `time -l`), 609 MB of output; budget 1,500
  and 1,000 MB.

Measured on Nepal on 2026-11-08 (release build, Apple M2 Pro), with the building and core
stages:

| Run | Stages run | Time |
|---|---:|---:|
| cold (or a new forge binary) | 10 | 32.5 s, 1.37 GB peak |
| nothing changed | 0 | 270 ms (outputs hashed, 432 MB of building points) |
| lite after main | 3 (its conflate, core and build; reads, buildings and topology are shared) | 15.1 s, 0.92 GB |

The `dem` stage (D-111) codes the 45 Copernicus tiles once, in 210 s on a loaded machine; it
reruns only when the tileset's pin, the extent or the DEM code changes, and its stamp hashes
the 139 MB section in a fraction of a second. Sampling every place adds about a second to
`build`.

Before the building stages (2026-11-07): cold 7.8 s, warm 19 ms; a changed profile reran 2
stages in 3.4 s and a hand-changed GeoNames output 1 stage in 3.1 s.

The stages after `build` join the recipe as they land. The first is `baseline`, when the
recipe has a `baseline` entry (below).

## The world part (`world`)

`forge world --release <v> [--lite] [--admin <source>] [--admin1 <source>] [--cities <source>]
[--tz <source>] [--tolerance-m <m>] [--islet-km2 <km²>] [--alternates <source>] [--populations <source> | --no-populations] [--aliases <source>]… [--labels <source>]…
[--towns <cc>]…
[--ids <dir> | --fresh-ids] [--allow-retire] [--key <seed-file>]` writes `data/build/baseline/world/world-baseline-<build>.tgd`,
its ID registries `world-ids-<build>/<namespace>.tsv` (the next build's input: without `--ids`
the stage reads them and fails when they are missing, unless `--fresh-ids` asks for a first
build, D-260; each part holds its serials under its own name and may retire at most 1% of
them without `--allow-retire`, D-267),
`summary-<build>.md` (D-147) and `world-probes-<build>.tsv`, the locator's border probes: about
100,000 source vertices at full resolution, every k-th of every feature, each with its
feature's ID (D-149). Defaults: `ne-admin0-nep`, `geonames-cities15000`, `tzdb-2026f`;
admin 1 `overture-divisions` for main (D-155) and `ne-admin1` for lite; tolerance 2 km from
Overture (D-157), else 1 km; islets the tolerance's square.

- **Unit populations** (`--populations`, default `wikidata-unit-population`, D-266). Each
  unit without a count of its own takes Wikidata's (best rank, latest dated value), which
  search importance weighs. Cities outside every unit's simplified outline are placed in the
  nearest unit of their country within 10 km.
- **Alternate names** (`--alternates`, D-265). Each city's GeoNames alternate names in the
  part's languages (English and each label source's), as historical, colloquial or alternate
  names; untagged ones are left out. The file name gains `-<source>`.
- **Rows.** One country place per admin-0 feature (names from `NAME` and its 26 `NAME_<XX>`
  columns, population, the cartographers' label point when it is inside the feature, else the
  topology's), one level-1 unit per admin-1 piece (its source name and English, D-150), and
  every city of the list outside the countries whose recipe builds a slice.
- **Main: units and countries from Overture** (`build::world_overture`, D-155 to D-157).
  Overture's land areas of every country, dependency and region; a dependency no Nepal-view
  feature has the code of is one unit of its sovereign. The land is cut into disjoint units
  and owned by the Nepal view (`topo::pov::own`: a unit keeps its code's country, the view
  takes its parts at least 15 km wide, D-156). Islets smaller than `--islet-km2` go, unless
  they are their area's largest part or hold a city. Pack countries take their slice's
  outline (as below). The whole partition is thinned to 100 m as shared arcs (`topo::thin`),
  the outlines pinned; each country is then its pieces dissolved, and a feature with no land
  has no row. A region is a unit row (`gers:<division>`, Wikidata and OSM relation) in the
  country holding most of it; its other parts and every country's land no region holds are
  fillers.
- **Lite (and main with `--admin1 ne-admin1`): admin 1 cut to the point of view**
  (`topo::pov`, D-150). The admin-1 source is drawn in
  another point of view. So each unit goes to the country holding most of its area and is cut
  only by the parts of its overhang at least about 1 m wide on average (narrower parts are the
  two sources drawing one line differently). Each country's uncovered rest becomes filler
  pieces (`~` codes) that link to no place. The topology stage dissolves the countries from
  pieces and fillers, so both levels share one arc set, with G-topo's parent limit at
  `pov::MAX_SHIFT_M` (10 m) instead of 1 m. Unrecognised units, unnamed placeholders and pack
  countries' units are dropped; their area falls to the filler.
- **Pack countries' outlines** (`topo::pov::redraw`, D-154). A pack country's outline is the
  one its slice ships (read from `data/build/baseline/<cc>/`, so `forge make <cc>` runs
  first), not Natural Earth's. Each neighbour is cut by it; each piece of Natural Earth's
  outline that the pack's leaves out goes to the neighbour sharing the most border with it;
  neighbours' units are cut by it exactly, whatever the width; and the pack country's filler is
  cut last by everything near it, so the overlay's grid rounding never leaves two pieces
  overlapping. The topology stage pins its arcs (every vertex at every level), so the world's
  neighbours meet the slice on the slice's own line. Nepal: 2 neighbours cut, 83 pieces
  given, 170 arcs pinned, +16 kB.
- **Timezones** (`build::world_tz`, D-151). Every place gets a zone, written as PLACE_TZ, and
  every zone used is a timezone-area row (`tz:<ID>`, namespace `world`, at its `zone.tab`
  point):
  - a city its own (`cities15000`'s `timezone`, through tzdb's links);
  - a unit the zone of most of its cities' people, else its country's one zone, else the
    nearest city's of its country;
  - a country its one `zone.tab` row, else its cities'.

  G-tz stops the build when more than 2% of the cities' people state a zone other than their
  unit's (0.647% measured).
- **Parents.** A unit hangs from its piece's country. A city hangs from the piece that contains
  it on the cut polygons, else from the feature that contains it (a 1° grid index, exact
  test); outside every feature, from the largest feature of its code.
- **IDs.** A registry per namespace (the ISO code, `world` without one). A pack country's row
  takes the serial its pack's registry gives its Wikidata ID: the ID gate fails the build
  otherwise, so run `forge make <cc>` for each pack country first.
- **Sections.** As a country pack's (PLACES, NAMES, SEARCH with no country rules, ELEVATION
  from the list's `elevation` or `dem` column, HIERARCHY), the polygons linked by `ADM0_A3`;
  then the whole set through `extract` at the finest level within the tolerance, with the
  slice's region index. The topology stage runs first every time (10 s with admin 1), so it is
  never stale.
- **Checks.** Read back verified, the geometry's checks, every place's ID, point, parent and
  canonical name, and its search by name.

Measured 2026-11-10 (main from Overture, D-157): 38,639 rows, 5,545,360 B, in 320 s at a
3.9 GB peak; ARCS 1,818,148 B, ADMIN_REGION 1,110,520 B, SEARCH 888,384 B, NAMES 743,060 B;
merged with Nepal's slice 5,601,192 B. From Natural Earth (D-150): 38,981 rows, 5,732,704 B (4.39 MB wire), in
18 s at a 0.97 GB peak; ARCS 2,101,428 B, ADMIN_REGION 1,242,236 B, SEARCH 797,664 B, NAMES
674,124 B, IDMAP 223,824 B. Without admin 1 it was 3,773,468 B (D-147, with the tolerance
A/B).

`tests/world.rs` runs with `TGD_WORLD`, and with `TGD_BASELINE` and `TGD_PACK` for a pack
country. It checks:

- every unit hangs from a country;
- every city hangs from a unit and its country, or from its country;
- each `admin_at` answer is one chain: every unit found hangs from the country found;
- `admin_at` at the city finds its country for at least 98% and its unit for at least 95%
  (main from Overture: 98.91% and 98.72%; Natural Earth: 98.46% and 99.76%, the rest mostly
  coastal points outside the stored coast);
- the slice answers for its country, and the pack above both.

**Memory.** The world stage once passed 12 GB: G-topo's parent check indexed each segment in
every 100 m cell of its box, and Natural Earth's straight borders run for thousands of
kilometres. It now indexes the cells along each segment (`snap::Grid::cells`) in a sorted
array, and `a_long_diagonal_border_is_indexed_along_itself` guards it. Run a new or grown
heavy stage under a memory cap first, reading the physical footprint (`footprint -p <pid>`):
on macOS, `ps` RSS leaves out compressed and swapped memory.

## The shipped baseline (`embed`)

`forge embed [--lite] [--key <seed-file>]` merges the world part and every slice of the build
(`data/build/baseline/<cc>/<cc>-baseline-<build>.tgd`, by country code) into
`data/build/baseline/baseline-<build>.tgd` and `summary-<build>.md`, with
`teigeodata_format::extract::merge` (D-153):

- **Rows** in Hilbert order. A slice's row replaces the world's row of the same ID (its
  country), and every reference to the replaced row (a parent, a zone) goes to the slice's.
- **Geometry.** Each part's arcs at their own level of detail; polygons grouped by admin level.
  The replaced country's world outline is dropped, so the slice's (the pack's) outline decides
  inside it.
- **SEARCH.** Every part's keys and importance; the query rules joined, each part's unit types
  moved past the earlier parts' (two phonetic keys, or more than 255 unit types, fail).
- **Manifest.** The world part's address, release and licence; every part's notices, each line
  once.
- **Borders.** The world part meets each pack country on its slice's line (D-154), so the one
  file answers every place exactly as the parts do as layers.
- **Gates.** Z1: over 8,000,000 B fails the stage. Every ID of every part is one row, at the
  last part's point (a planted "earlier part wins" fails it). Read back verified, geometry
  checked.

Measured 2026-11-10 (main, world + Nepal, with D-154): 39,395 rows, 5,895,268 B, in 0.5 s at
a 26 MB peak; about 18 kB less than the two parts apart.

## The baseline slice (`baseline`)

`forge baseline <cc> [--lite] [--tolerance-m <m>] [--cities <source>] [--key <seed-file>]`
writes `data/build/baseline/<cc>/<cc>-baseline-<main|lite>.tgd`, the country's slice of the
embedded baseline, and `summary-<build>.md` (D-144, D-145). `make` runs it after `build` when
the recipe says `baseline: { cities: <pinned source>, tolerance_m: <50–5,000> }`; its key
covers the pack's key, the pinned city list, the arguments and the signing seed.

- **Rows.** The country, its current level-1 units, and every row of the city list
  (GeoNames `cities15000`, column 9 = the country code) found by its `geonames:` key among the
  live serials of the build's ID registry. Same IDs, names and outline as the pack, so the
  pack overrides the slice row for row once installed.
- **Re-encoding** (`teigeodata_format::extract::rows`). IDMAP, PLACES, PLACE_INDEX,
  ELEVATION, NAMES, SEARCH and HIERARCHY for the kept rows, each row's parent being its
  nearest kept ancestor; ARCS (the used arcs only, at the finest level of detail within the
  tolerance, `extract::lod_within`), POLYGONS and POLY_INDEX remapped, ADMIN_REGION rebuilt.
  PLACE_TZ is kept with the zone rows its places use (D-151). REGION, REDIRECTS, POSTAL, DEM,
  BLOBS and EXTRA are dropped: the locator comes from the world part of the baseline.
- **Names** (D-152, `extract::rows_with` with a `NameRule`). A row keeps its canonical name,
  untagged names, names in the country's `languages` (`profile.yaml`) and English; the
  country row also Natural Earth's 26 languages, as in the world part. SEARCH is rebuilt from
  the kept names by the pack's own keying (`pack::add_keys`).
- **Region index.** A 4,096-cell fine grid instead of the packs' 2^18. A/B on Nepal's slice at
  500 m (2026-11-09):

  | Fine cells | ADMIN_REGION | `admin_at` |
  |---:|---:|---:|
  | 2^18 (packs) | 135,832 B | 163 ns |
  | none | 17,684 B | 418 ns |
  | **4,096** | **20,516 B** | **281 ns** |
  | 16,384 | 27,880 B | 206 ns |

- **G-base.** Every listed city of the country must be a row, or the build fails and names
  them. The slice is read back (`open_verified`) and its arcs, polygons and region checked
  before it is written.
- **Nepal (2026-11-09).** 46 listed cities give 45 places (Panauti is listed twice), plus the
  country and 7 provinces. Main 158,784 B, lite 150,336 B; about 80 kB on the wire. With
  D-152's names (2026-11-10): main 72,988 B, 38,000 B with zstd −19.
  `tests/extract.rs` (`the_baseline_slice_under_its_pack`, with `TGD_PACK` and
  `TGD_BASELINE`) checks that the pack wins every ID, and that the slice alone answers each
  city with its province and country, by `ancestors` and by `admin_at`, and that every name
  the slice keeps finds its row.

**Clean-up.** `cargo xtask clean` removes build output from both workspaces and keeps the
store: fetched sources are pinned inputs, not build output.
