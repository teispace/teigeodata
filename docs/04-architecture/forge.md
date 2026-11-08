---
title: "The forge (data pipeline)"
status: draft
updated: 2026-11-08
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
| build (pack writer, sign, read back) | **built** (3.1) | below |
| make (every stage, content-keyed cache) | **built** (3.1) | below |
| buildings (OSM, Microsoft, Google) | **built** (3.2) | below |
| core (settlement core, weighted Weiszfeld consensus) | **built** (3.2) | below |
| labels (hand-labelled truth set) | **built** (3.2) | below |
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
- A fetch prefers `mirror_url`, our release asset, when one is set.
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

**Moving sources.** GeoNames dumps and Wikidata snapshots change in place and have no dated
archive. They carry `moving = true`, and a release build must read them from our mirror. The
mirror is created in Phase 2 with the first data release. Until then, spikes read the local
store and publish only statistics.

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
  or a populated place and an admin unit.

It writes `data/build/conflate/<sources>/clusters.parquet` (`id`, `cluster`, named by the
smallest member) and a `summary.md`. The summary holds the model's weights, the clusters by
source composition and the silver-label evaluation. Names are compared with
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

## Build a pack (`build`)

`forge build <cc> --release <v> --admin <boundary> [--tz <tz>] [--lite] [--key <seed-file>] [--ids <registry>] <sources…>`
writes `data/build/pack/<cc>/<cc>-places-<main|lite>.tgd`, `summary-<main|lite>.md` and the
ID registry `<cc>-ids-<main|lite>.tsv` from the `conflate` output of the same sources:
- **Licence gate (G-lic).** `--lite` refuses any source whose licence class is not
  public-domain, CC0, CC BY, CC BY-IGO, ODC-By or GODL. The timezone source (tbb, ODbL) is
  main only, so a lite pack has no timezone REGION yet.
- **Places.** The country (from its Wikidata item), the admin units of every COD-AB level
  below it (a point inside each polygon, its parent the unit of the level above), one place
  per conflated cluster (a settlement when any record is a populated place, else a locality;
  the parent is the deepest admin unit holding it), and one `timezone_area` place per IANA
  zone that meets the country's box (D-093). A place keeps the names that fit the search key
  limit; a place with none is left out and counted.
- **Sections.** Rows in Hilbert order (ties by key), so IDs follow rows (D-075): IDMAP,
  PLACES, HIERARCHY, NAMES, SEARCH, PLACE_INDEX, the timezone REGION linked to its places
  (column 16), and REDIRECTS when the registry has retired serials. Importance is provisional (kind, admin level, log population).
- **Determinism (G-det).** The build ID is BLAKE3 of the forge version, `sources.lock` and
  the arguments; the creation time is `SOURCE_DATE_EPOCH` or 0. Two builds give identical
  bytes.
- **Sign and read back.** `--key` signs with an Ed25519 seed (64 hex digits). The pack is then
  opened with the runtime reader: hashes and signature are checked, every section is opened
  and passes `check`, and every place keeps its ID, point, parent and canonical name, is found
  by an exact search of that name and is its own nearest place. Every zone holds its own
  point and links to its row. Every row has the registry's ID, and REDIRECTS passes `check`
  and gives every retired ID the registry's fate.

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

## Run every stage (`make`)

`forge make <cc> --release <v> [--lite] [--key <seed-file>] [--ids <registry>] [--force]` runs a
country's stages from its recipe, `data/countries/<cc>/build.yaml` (checked by
`cargo xtask check-data`): `read` for each points source, `conflate` over them, `buildings`
for each building source the build may read, `core`, then `build`
(D-095, [ADR-0035](../03-decisions/adr/ADR-0035-forge-architecture.md)).
- **Keys.** A stage's key is a BLAKE3 hash of the stage name, the forge binary, its pinned
  inputs (ID, SHA-256 and size from `sources.lock`), its configuration (the profile, the
  arguments, the ID registry, the signing seed) and the keys of the stages it reads. The whole
  binary is the code version: any forge change reruns everything, which is always correct.
- **Stamps.** A stage that succeeds writes `.key` (`.key-main`, `.key-lite` in the pack
  directory) into its output directory: the key, then the BLAKE3 and size of every file it
  owns. A stage is skipped only when both match, so an output changed by hand, or a stage
  run by hand with other arguments, is rebuilt. The stamp is removed before a stage starts
  and written atomically after it ends, so a killed run resumes from the first unfinished
  stage.
- **Licence gate.** `--lite` never schedules a reader for a source outside the lite classes.

Measured on Nepal on 2026-11-08 (release build, Apple M2 Pro), with the building and core
stages:

| Run | Stages run | Time |
|---|---:|---:|
| cold (or a new forge binary) | 9 | 30.0 s, 1.43 GB peak |
| nothing changed | 0 | 270 ms (outputs hashed, 432 MB of building points) |
| lite after main | 3 (its conflate, core and build; reads and buildings are shared) | 14.3 s, 0.92 GB |

Before the building stages (2026-11-07): cold 7.8 s, warm 19 ms; a changed profile reran 2
stages in 3.4 s and a hand-changed GeoNames output 1 stage in 3.1 s.

Not yet: per-stage code versions, the disk budget check, and the stages after `build`
(they join the recipe as they land).

**Clean-up.** `cargo xtask clean` removes build output from both workspaces and keeps the
store: fetched sources are pinned inputs, not build output.
