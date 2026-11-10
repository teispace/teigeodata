---
title: "World admin 1 from OSM for the main build"
status: research
updated: 2026-11-10
owner: forge
revisit: 2027-02-10
---

# World admin 1 from OSM for the main build

## 1. Question

Step 4.2 left one item open: main's first-level units from OSM, as "a boundary-only extract
made in CI" ([baseline note §7](baseline.md#7-risks-and-open-items), D-144). Until it exists,
main uses Natural Earth's admin 1 cut to the Nepal view, as lite does (D-150). Natural Earth
admin 1 is stale in places: Nepal's 14 zones of before 2015, not its 7 provinces. What is the
best way to get current OSM admin 1 for the world into `forge world`, at what cost, and what
does it change in the world part?

## 2. Summary and recommendation

- **Read Overture Maps' `divisions` theme, not the planet.** Overture builds it from OSM
  (4,195 of 4,201 country, dependency and region divisions, 2026-10-21.0) with valid polygons,
  ISO codes, Wikidata IDs and the OSM relation of every row. The `division_area` files are
  4.46 GB and `division` 0.58 GB for the world. The planet file is about 80 GB and needs two
  passes for a boundary extract. A tileset of the nine part files, pinned by hash
  (`query_params.list`), needs no new fetch code and no extract job of our own.
- **Main takes admin 0 from the same source.** Natural Earth's borders sit 0.6–2.5 km from
  OSM's in most of the 20 countries sampled (§3.4). Cutting OSM units to Natural Earth's
  countries would leave slivers about a kilometre wide along every border. Main's countries are
  therefore dissolved from Overture's units. Natural Earth's Nepal-view admin 0 only decides
  which country owns a disputed unit: majority area, then a cut only where the overhang is
  wider than the two sources' disagreement.
- **Pre-simplify per country, keeping the topology.** Land regions are 29.5 M vertices,
  against about 1 M in Natural Earth admin 1. Each country's units become shared arcs
  (`topo::arcs::build`), and each arc is simplified within 100 m (a tenth of the world
  part's 1 km tolerance) before the overlay. A shared border stays one line and the overlay's
  input shrinks by about an order of magnitude (**unverified**: measure it).
- **Lite is unchanged.** Overture divisions are ODbL, so lite keeps Natural Earth (public
  domain).

## 3. Findings

### 3.1 The divisions theme (2026-10-21.0, read 2026-11-10)

- Three types: `division` (a point per entity, with names, hierarchy, Wikidata), `division_area`
  (its polygons) and `division_boundary` (lines between divisions of one subtype). Schema in
  `OvertureMaps/schema`, package `overture-schema-theme-divisions`.
- `subtype` is a fixed list: country, dependency, macroregion, region, macrocounty, county,
  localadmin, locality and below. `region` is "a state, province, region, etc. Largest
  sub-country administrative unit in most countries". `admin_level` is required for country to
  county, and is the unit's place in its own country's hierarchy (added in 2026-02).
- `division_area.class` is `land` (clipped to the coastline) or `maritime` (to the territorial
  sea). The baseline needs `land`, as Natural Earth's are.
- `perspectives` (on `division`) lists the political perspectives that accept or dispute a
  division. In 2026-10-21.0, no country, dependency or region row has it: each disputed area
  appears once, as OSM maps it on the ground (Sevastopol under UA, Azad Kashmir and
  Gilgit-Baltistan under PK, Kosovo as XK, Taiwan as TW). The Nepal view must still come
  from us.
- `sources` names the record: `OpenStreetMap` `r1943188@152` (relation and version) for 4,195
  rows, `geoBoundaries` for Cyprus's 6 districts.

### 3.2 Counts and sizes (measured 2026-11-10, DuckDB 2.0.0, range reads from `s3://overturemaps-us-west-2`)

| Subtype, class | Rows | Vertices | Largest | Countries |
|---|---:|---:|---:|---:|
| country, land | 219 | 6,040,330 | 230,251 | 219 |
| dependency, land | 53 | 90,777 | 51,422 | 53 |
| region, land | 3,929 | 29,508,797 | 254,948 | 226 |
| region, maritime | 798 | 5,527,495 | 192,583 | 98 |

- Land regions at `admin_level` 1: 3,622; at 2: 307, all in dependencies (Puerto Rico 78,
  Réunion 24, Isle of Man 24, Hong Kong 19). A dependency's regions are its own first level.
- 46 countries or dependencies have no region (small states): their single unit is the
  country.
- 3,913 of 3,929 regions carry a Wikidata ID.
- Nepal: the 7 provinces with their Nepali names (3,765 to 10,110 vertices each).
- Extracting country, dependency and region areas by range reads took 416 s and wrote 467 MB
  (zstd Parquet, full resolution).

### 3.3 The planet route

The planet PBF is tens of GB (about 80 GB, **unverified** today). A boundary extract
(`osmium tags-filter r/boundary=administrative`) needs its referenced ways and nodes, which
come before the relations in the file, so it is two passes over the planet. A GitHub runner
has about 14–21 GB free, up to about 50 GB after removing preinstalled tools, so the planet
does not fit on disk and would be streamed twice per build. We would also assemble the
multipolygons (the libosmium area logic, ADR-0035) and their invalid rings at world scale.
Overture has already done this, with stable IDs.

### 3.4 Natural Earth against OSM (measured 2026-11-10)

Mean offset = area of the symmetric difference between Natural Earth's Nepal-view country and
Overture's land area, divided by Natural Earth's perimeter. It counts coastlines and dispute
lines too.

| Country | Mean offset (m) | Perimeter (km) |
|---|---:|---:|
| Japan | 583 | 15,474 |
| Germany | 732 | 5,673 |
| United States | 798 | 78,749 |
| Indonesia | 1,116 | 54,827 |
| Nepal | 1,925 | 2,600 |
| Brazil | 2,032 | 30,959 |
| China | 2,450 | 36,721 |
| India | 5,249 | 22,380 |
| France | 9,817 | 9,598 |

(20 sampled: JP, TR, DE, US, MX, SE, AU, ZA, EG, ID, AR, TH, NG, PE, KZ, NP, BR, CN, IN, FR.)
India's figure includes Natural Earth's Nepal-view lines in Kashmir and Kalapani; France's
probably its overseas parts (**unverified**). With D-150's 1 m cut rule, nearly every border
unit would be cut and gain filler slivers.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Planet + `osmium tags-filter` in CI | The source itself; any tag we want | ~80 GB, two streamed passes per build; our own multipolygon assembly at planet scale | ODbL | osmium-tool 1.x | rejected (cost, risk) |
| Geofabrik continent extracts | Smaller than the planet | Still GBs per continent, 7+ files, same assembly work | ODbL | daily | rejected |
| Overpass per country | Exact query | Rate limits; not reproducible; a public service | ODbL | live | rejected |
| Overture divisions (`division_area` + `division`) | OSM-derived, valid polygons, ISO and Wikidata, OSM relation IDs; immutable monthly files | One perspective (OSM's); 5.04 GB to pin; ODbL | ODbL (CY: geoBoundaries) | 2026-10-21.0 | **main admin 0 and 1** |
| OSM units cut to Natural Earth countries | Keeps D-147/D-150 as is | 0.6–2.5 km slivers on every border (§3.4) | — | — | rejected |
| Natural Earth admin 1, cut (D-150) | Public domain | Stale (Nepal's zones) | Public domain | 5.1.2 | **lite** (unchanged) |

## 5. What we build ourselves, and why

1. **Pin** `overture-division-area` and `overture-division` as tilesets: a checked-in list of
   the release's part files (`tile<TAB>url`) and the manifest of their hashes. A new Overture
   release is a new list, reviewed like any source bump.
2. **Read** (`read::overture`): stream each Parquet file a row group at a time with the
   `parquet` 60.1 crate already in the forge, with a projection (id, division_id, subtype,
   class, country, admin_level, names, geometry), keep `class = land` and subtype country,
   dependency or region, decode WKB into the 1e-7° grid. Join `division` for Wikidata and the
   OSM relation. Memory stays bounded by one row group plus the kept rows.
3. **Pre-simplify per country:** arcs from that country's units and its country area, each arc
   simplified with pinned ends within 100 m, rings rebuilt. Proven by a gate that the result
   is noded and that every shared border is still one arc.
4. **Point of view:** each unit goes to the Nepal-view country holding most of its area, as in
   D-150. It is cut only by overhang wider than the sources' disagreement, a width to set from
   the measured offsets. Countries dissolve from their units. Natural Earth supplies the
   country set, codes, names in 26 languages and the Nepal-view ownership.
5. **Gates:** the world gates as they are (ID gate, `admin_at` chains, the locator's border
   probe, Z1), plus G-topo against Overture's own country outline, and a count of slivers and
   fillers in the summary, so a rise is seen.

## 6. Impact on the plan

- D-155 records the source and the admin 0 change for main.
- Phase 4.2's open item becomes: pin, reader, pre-simplification, `forge world --admin1
  overture-…` for main, the data-build job.
- D-150 stays the lite rule. D-147's admin 0 for main changes from Natural Earth's geometry
  to Overture's, with Natural Earth's Nepal view as the owner of disputed areas.
- G-lic: main's attribution gains "© OpenStreetMap contributors" (already in main) and the
  Overture Maps Foundation.

## 7. Risks and open items

- Size: OSM lines simplified at 1 km may hold more vertices than Natural Earth's at 1 km (more
  detailed coastlines). Z1 (8 MB) gates it; if it fails, the world part's tolerance or the
  coastline's level moves, measured.
- Overture is OSM as of its release date, with Overture's own fixes; a unit OSM breaks for a
  month can vanish from a release. The summary reports units per country, and a fall from the
  previous build stops it (to build).
- Cyprus's 6 districts come from geoBoundaries; its licence per country is **unverified**.
  Check before shipping, or take Cyprus's units from Natural Earth.
- The ownership width (step 4 of §5) is a new tuned constant. It must be measured on the known
  disputes (Kalapani, Kashmir, Crimea, Western Sahara, Kosovo, Taiwan) and proven by planting
  a wrong owner.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Overture release | 2026-10-21.0 (latest) | 2026-11-10 | `https://stac.overturemaps.org/catalog.json` |
| Overture divisions schema | `overture-schema-theme-divisions` (main) | 2026-11-10 | GitHub `OvertureMaps/schema` |
| DuckDB | 2.0.0 (measurement only) | 2026-11-10 | Homebrew |
| parquet / arrow (Rust) | 60.1.0 | 2026-11-10 | `forge/crates/forge/Cargo.toml` |

## 9. Sources

- [Overture releases STAC catalog](https://stac.overturemaps.org/catalog.json), Overture Maps
  Foundation, accessed 2026-11-10.
- [Overture schema, divisions theme](https://github.com/OvertureMaps/schema/tree/main/packages/overture-schema-theme-divisions),
  accessed 2026-11-10.
- [Overture documentation: division_area](https://docs.overturemaps.org/schema/reference/divisions/division_area/),
  accessed 2026-11-10.
- [Overture release notes 2026-02-18](https://docs.overturemaps.org/blog/2026/02/18/release-notes/)
  (`admin_level` added), accessed 2026-11-10.
- [Freeing disk space on GitHub Actions runners](https://www.dzombak.com/blog/2024/09/freeing-disk-space-on-github-actions-runners/),
  Chris Dzombak, 2024.
- The offsets in §3.4 and the counts in §3.2: our measurement, DuckDB over the public bucket.
