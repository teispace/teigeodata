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
  ownership where it disagrees with Overture by more than the two sources' disagreement: a
  unit keeps its own code's country, and the view takes its parts at least 15 km wide (§3.6).
- **Pre-simplify the world as one partition, keeping the topology.** Land regions are
  29.5 M vertices, against about 1 M in Natural Earth admin 1. All units become shared arcs
  (`topo::arcs::build`), each simplified within 100 m before the world stage: 31.2 M
  vertices become 3.6 M, and a shared border stays one line (§3.5).
- **Ship 2 km and leave out islets under the tolerance's square.** OSM has 79,949 land parts
  to Natural Earth's 4,253, and they, not the vertices, broke Z1 (17.0 MB at 1 km). An islet
  that is its area's largest part or holds a city stays. Main's world part is 5.55 MB, merged
  5.60 MB; the locator's radius becomes 3 km (§3.7).
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

### 3.5 The world's land as one partition (measured 2026-11-10)

Overture's land areas are not a partition as given, and making them one is most of the work:

- **Regions do not tile their country.** A country's land and its regions' land are clipped
  to slightly different coasts: 99,743 parts of countries' land outside every region are at
  least 1 m wide (Spain 7,160, Ireland 6,976). Kept at 100 m mean width (the thinning
  tolerance), 20,938 remain, mostly islets no region holds; the large ones are lakes no
  region holds (Lake Malawi 20,516 km², Lake Victoria's Tanzanian part 37,069 km²) and
  Greenland's national park (863,043 km²).
- **Claims overlap.** A country's land includes its claims: Morocco's holds Western Sahara,
  India's and China's each other's claims, both Argentina's and Chile's regions the Southern
  Patagonian Ice Field, both Canada's and the United States' land Machias Seal Island. Every
  overlapping area goes to the earlier country code (zones, `read::overture::land_units`);
  the Nepal view reassigns ownership later.
- **One country's regions can overlap.** Morocco has Dakhla-Oued Ed-Dahab (MA-12) and a
  code-less "Western Sahara" region over it. Checked per country with one overlay (union
  area against summed areas); a coded region keeps the overlap.
- **Overlays round.** The cut pieces' new vertices are rounded to the grid, so they meet the
  regions they border away from those regions' vertices. Snap rounding (D-103) nodes the
  world (17,108 bad pairs, 47,326 vertices added); it can then turn a sub-pixel overlap into
  two units running one edge the same way, or a narrow neck into a zero-width bridge. Such
  folds are undone by exact overlays on the snapped, noded rings (1,177 units); an overlap at
  least 1 m wide is refused as data.

Result: 31,039 land units, 31,470,017 vertices thinned to 3,578,426 at 100 m in 191,202
shared arcs, the guard leaving nothing; 250 s, peak 4.1 GB (snap rounding's maps dominate:
the next thing to make lean).

### 3.6 Ownership by the Nepal view (measured 2026-11-10)

Natural Earth's lines are too far from OSM's to own units by area alone. Giving each unit to
the Nepal-view country holding most of it would move Liechtenstein's municipalities to Austria
(Planken 72%), San Marino's to Italy and Slovenia's Hodoš to Hungary (62%). A difference
against Natural Earth's default view is no reference either: both of its views give Western
Sahara's Free Zone, east of the berm, to Western Sahara, while Overture puts all of it in
Morocco's regions, so a default-to-Nepal delta never touches it.

The rule (`topo::pov::own`): a unit keeps the country its own code names (several Natural
Earth features with one code: the one holding most of it, else the nearest). A code no
Nepal-view feature holds (Overture's disputed areas `X…`, Taiwan, Kosovo, a dependency drawn
inside its sovereign) goes to the feature holding most of it; one no feature holds at all (an
islet off Natural Earth's coarser coast) goes where most of its code's other units went. Then
the view takes every part of a unit at least `WIDTH` wide on average that another of its
countries holds, with the unit's narrower leftovers.

The width is measured. With it at 2 km, 482 parts move; their mean widths run continuously
from 2 km to 11 km (desert and mountain lines: Tibesti, Arunachal, Bhutan, Kyrgyzstan), then
jump to 22 km. Every part from 22 km up is a real dispute:

| Unit (Overture) | To (Nepal view) | Area | Mean width |
|---|---|---:|---:|
| Laâyoune-Sakia El Hamra, Dakhla-Oued Ed-Dahab (MA) | Western Sahara | 264,716 km² | 137–152 km |
| Gilgit-Baltistan (PK) | India | 66,760 km² | 85 km |
| Western Bahr el Ghazal (SS: Kafia Kingi) | Sudan | 13,074 km² | 44 km |
| Guelmim-Oued Noun (MA) | Western Sahara | 3,706 km² | 24 km |
| Azad Kashmir (PK) | India | 11,891 km² | 23 km |

`WIDTH` is 15 km, in that gap. Kalapani (3.3 km) falls under it; Nepal's pack outline draws it
(D-154). Five disputed islets no Nepal-view feature holds stay unowned (Liancourt's claim
extent, Senkaku, the Tunbs, the Paracels); Kingman Reef's Overture land (2,481 km² of lagoon)
and a 1,786 km² strip of Greenland's coast lie mostly outside Natural Earth's land and are
reported. Seven Natural Earth features own no land and have no row: Ashmore and Cartier, the
Brazilian Island, the Coral Sea Islands, Dhekelia, Scarborough Reef, Guantanamo, Akrotiri
(each narrower than `WIDTH` against Overture's units, or off its coast).

Building main's world part this way (`forge world`, 2026-11-10): 3,877 units and 21,911
fillers; 34,023 of 34,162 cities under a unit (Natural Earth: 33,623), 56 falling back to
their code's feature (517), 2 in no country (15); 0.400% of cities' people state a zone other
than their unit's (0.645%); 417 s, peak 3.9 GB. Thinning left three kinds of degenerate piece,
each now handled: an islet whose lake collapsed under 100 m (cut out of the piece around it), a
sliver between a pack's outline and its neighbour pressed under 1 m (dropped), and holes
touching at a vertex (G-topo counts holes in one normal form).

### 3.7 Size: islets and the tolerance (measured 2026-11-10)

Built at the world part's 1 km default, main from Overture was 17,011,088 B against Z1's
8,000,000 B for the whole merged baseline (Natural Earth: 5,839,988 B): ARCS 8.86 MB and
ADMIN_REGION 4.90 MB. A coarser tolerance alone barely helps (2 km: 12.9 MB, ARCS 5.95 MB),
because the cost is in the number of rings, not their vertices. OSM maps every islet; Natural
Earth's cartographers keep a few:

| Land parts (countries and dependencies) | Overture | Natural Earth (Nepal view) |
|---|---:|---:|
| under 0.1 km² | 32,709 | 83 |
| 0.1–1 km² | 29,543 | 217 |
| 1–4 km² | 8,554 | 189 |
| 4–25 km² | 5,703 | 873 |
| 25–100 km² | 1,861 | 1,250 |
| 100 km² and more | 1,579 | 1,641 |
| **all** | **79,949** | **4,253** |

So the world part leaves out an islet: a part of a piece smaller than the tolerance's square,
unless it is its area's largest part (no unit and no island country vanishes: Tokelau, Nauru)
or holds a listed city (`world_overture::Islets`). Pack countries are untouched: their slice
is drawn by their own pack. Measured (main, Nepal's slice merged):

| Tolerance | Islets under | Arcs | ARCS | ADMIN_REGION | World part | Islets left out |
|---|---|---:|---:|---:|---:|---|
| 1 km (500 m level) | none | 194,117 | 8,859,932 | 4,896,564 | 17,011,088 | – |
| 2 km | none | 194,117 | 5,948,732 | 3,694,328 | 12,889,616 | – |
| 1 km | 1 km² | 38,973 | 5,262,396 | 2,574,204 | 10,512,400 | 102,632 parts, 17,332 km²; 3 kept for a city |
| 1 km | 4 km² | 25,640 | 4,853,852 | 2,283,524 | 9,762,320 | 112,758 parts, 37,255 km²; 13 kept |
| **2 km** | **4 km²** | **25,640** | **1,818,148** | **1,110,520** | **5,545,296** | the same |

Main ships 2 km with islets under 4 km² left out: 5,601,192 B merged, smaller than Natural
Earth's. OSM's border within 2 km is still nearer the ground than Natural Earth's within 1 km
(0.6–10 km off, §3.4). Every city, unit and zone result is unchanged by the islets (34,023 of
34,162 cities under a unit; 0.400% of people in another zone); `admin_at` agrees with the
city's country for 98.91% (Natural Earth 98.46%) and its unit for 98.72%.

The locator's radius follows (D-149): at 2 km, 16 of 100,104 border probes are missed, all
found by 2.5 km. A level's tolerance bounds each vertex's own Douglas–Peucker importance, but
importances are capped by their parent split's so that levels nest, so a border can stray past
it. `LOCATOR_RADIUS_M` is 3 km: 0 missed, 1.41 countries per probe, 98.55% of cities get
exactly one country.

Overture's units carry English names, which exposed a ranking gap: a first-level unit's base
importance (90) is above a national capital's (54 plus 5·log10 of its population), so
"Tokyo" found the prefecture 東京都 before the city, as "Berlin" or "Seoul" would. World cities
also had no settlement rank; they now take it from the city list's feature code (`P.PPLC`,
`P.PPLA`). A first-level unit ranks just below a city of its own name inside it (D-158).
Applied to every level, it cost Nepal's golden set 8 hits at 1 ("Damak" and "Kamalamai" want
the municipality, which is the city); at level 1 the golden set is unchanged (recall@1 0.859).

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
3. **Pre-simplify the world as one partition:** every unit worldwide (regions, and each
   country's land not covered by its regions) becomes shared arcs, each arc simplified with
   pinned ends within 100 m under the existing guard, rings rebuilt (`topo::thin`). Per country
   would not do: two neighbours would thin their shared border differently and leave gaps and
   overlaps up to 100 m. The world is about 36 M vertices, and `arcs::build` kept three hash
   maps (edge owners, directed edges, vertex neighbours) at about 200 B a vertex, about 7 GB.
   It now finds topology nodes by sorting flat records instead: one record per ring vertex
   (the vertex and its two neighbours, 24 B), where a vertex is a node if it occurs three or
   more times, or twice with different neighbours; the overlap checks sort one record per edge
   the same way. For a valid partition this is the old rule (a vertex with other than two
   neighbours, or whose two edges have different owners), so it replaces it, one code path;
   the gate is the world part byte-identical before and after (BLAKE3 `fcb1f666…`).
4. **Point of view (§3.6):** a unit keeps its own code's country; the Nepal view takes its
   parts at least 15 km wide that another country holds. Countries dissolve from their units;
   Natural Earth supplies the country set, codes, names in 26 languages, Wikidata and
   population. A pack country takes its slice's outline: its neighbours are cut by it and
   its own pieces outside it go to the neighbour sharing the most border (D-154).
5. **Gates:** the world gates as they are (ID gate, `admin_at` chains, the locator's border
   probe, Z1), plus G-topo against Overture's own country outline, and a count of slivers and
   fillers in the summary, so a rise is seen.

## 6. Impact on the plan

- D-155 records the source and the admin 0 change for main; D-156 the ownership rule; D-157
  the tolerance, the islets and the locator radius; D-158 the ranking of a region below its
  city.
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
- The ownership width (step 4 of §5) is a tuned constant, measured in §3.6: noise reaches
  11 km, the narrowest real dispute 22 km. A new Overture release or Natural Earth version is
  re-measured; the summary lists every moved part, so a new one is seen.

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
