---
title: "The embedded baseline: sources, IDs and size"
status: accepted
updated: 2026-11-10
owner: forge
revisit: 2027-05-01
---

# The embedded baseline: sources, IDs and size

## 1. Question

Step 4.2 builds the embedded baseline (ADR-0010): countries, first-level units, large cities
and world timezones, plus the locator that maps a point to a pack address. Which world sources
can it use under the Nepal perspective (ADR-0006) and both licences (main ODbL, lite CC BY
4.0)? How do its IDs stay the IDs of the country packs that override it? Does it fit the
proposed 8 MB? This feeds the first unchecked items of
[phase 4 §4.2](../../07-planning/phases/phase-4.md#42-baseline-and-locator).
[Engine and catalog §3.6](engine-and-catalog.md#36-the-baseline-and-the-locator) holds the
design and the first estimates; this note pins the sources.

## 2. Summary and recommendation

- **One rule for every country with a pack: the baseline is a slice of that pack's build.** Its
  country, first-level units and cities above the threshold come from the same build, so their
  IDs, names and outline are the pack's. Nepal's outline, with the 2020 claim, is the main
  pack's (D-054); the baseline never redraws a border that a pack already draws.
- **Countries without a pack:**
  - Admin 0 from **Natural Earth 5.1.1, the Nepal point of view** (`ne_10m_admin_0_countries_nep`).
    It is public domain, so both builds may read it. Its other disputes follow Nepal's view as
    Natural Earth records it.
  - Admin 1 from **OpenStreetMap** for main (current, the source the main build already reads).
    Lite needs a clean-room source: Natural Earth admin 1 is stale in places (below), so lite
    takes geoBoundaries gbOpen per country where its licence passes the gate (**unverified**
    per country), else Natural Earth, and the build reports which.
  - Cities from **GeoNames `cities15000`** (CC BY 4.0, 34,208 rows on 2026-11-09), both builds.
- **Timezones:** main simplifies TBB (as the country packs do, D-134). Lite uses tzdb over the
  admin-0 outline for the 216 one-zone countries (D-138); the 31 countries with several zones
  get no lite zone until their table exists. Natural Earth's time zones are rejected (below).
- **IDs:** each place is keyed into its country's registry (`forge/.../build/ids.rs`) by its
  GeoNames ID and Wikidata ID, which Natural Earth carries (`gn_id`, `wikidataid`) and
  which the country packs' admin and city records carry too (D-110). When a country's pack is
  built later, it matches the same serials, so the baseline's IDs survive.
- **Size:** about 1–2 MB of boundaries and 2–4 MB of cities: within 8 MB (measurements below).
  The timezone and locator shares remain estimates until the first build.

## 3. Findings

### 3.1 Natural Earth's Nepal point of view

Natural Earth 5 ships 31 point-of-view variants of admin 0; the latest release is v5.1.2
(2022-05-13), and the files themselves say 5.1.1. `ne_10m_admin_0_countries_nep.zip` is
4,921,134 B, SHA-256 `4755295d…6d365`, fetched 2026-11-09 from naciscdn.org. A stdlib
shapefile check against the default view (`ne_10m_admin_0_countries.zip`, SHA-256
`ce1ac703…a7f6`):

| View | Nepal area (km², equirectangular) | Kalapani (30.205 N, 80.985 E) in | Northernmost point west of 81.2 E |
|---|---:|---|---:|
| Default | 147,582 | India | 30.210 |
| Nepal | 147,637 | Nepal | 30.255 |

The Nepal view moves Kalapani into Nepal but adds only about 55 km². The 2020 official map
claims much more, up to Limpiyadhura, so **Natural Earth's Nepal view is not Nepal's 2020
map.** That is why the rule in §2 takes Nepal's outline from the Nepal pack and uses Natural
Earth only for countries without a pack.

### 3.2 Natural Earth admin 1 is stale in places

`ne_10m_admin_1_states_provinces` (5.1.1, 14,909,524 B, SHA-256 `efc59726…2b05`): 4,596 units,
all with a Wikidata ID and 4,556 with a GeoNames ID. For Nepal it lists the **14 zones
abolished in 2015** (Mechi, Koshi, … Mahakali), not the 7 provinces. Any country whose units
changed after about 2020 may be stale the same way, so the build must not trust it blindly:
countries with a pack use the pack, and the lite build reports each country's admin-1 source.

### 3.3 Natural Earth time zones are offsets

`ne_10m_time_zones` (5.0.0, 1,972,370 B): 120 polygons by UTC offset. Only 87 name an IANA
zone (`tz_name1st`), and the polygons date from 2012 (field `iso_8601`). This confirms the
rejection in [lite timezones](lite-timezones.md#4-options-compared): it cannot answer an IANA
ID.

### 3.4 geoBoundaries

The CGAZ global composite (v6.0.0, updated 2023-09-13) replaces disputed areas with US State
Department lines, which contradicts ADR-0006, so CGAZ is out. The per-country gbOpen files are
not clipped that way. Their licences are set per country (many CC BY 4.0, some from OSM);
**unverified** until the lite build's licence gate reads each file's metadata.

### 3.5 Measured vertex counts (2026-11-09)

Douglas–Peucker per ring on a local equirectangular projection (a stdlib script, so rings are
simplified independently; the forge simplifies shared arcs once, which only lowers these
counts):

| Layer | Features | Rings | Raw vertices | 250 m | 500 m | 1 km |
|---|---:|---:|---:|---:|---:|---:|
| Admin 0, Nepal view | 248 | 4,271 | 548,292 | 371,183 | 271,530 | 181,668 |
| Admin 1 | 4,596 | 8,646 | 1,295,319 | 764,395 | 551,397 | 372,470 |

At 3.4–4.5 B per vertex in ARCS (D-097, D-103), admin 1 at 1 km is at most 1.3–1.7 MB counted
per ring. Admin 1 shares every border with admin 0 and with its neighbours, so stored as arcs
it holds both levels in about that budget. At 500 m it is at most 1.9–2.5 MB.

### 3.6 Cities

`cities15000.zip` (3,366,548 B, SHA-256 `955ad298…f42f4`, modified 2026-11-09): 34,208 rows,
all cities over 15,000 people plus capitals. `cities5000` is 5,726,119 B zipped (about 50,000
rows by GeoNames' readme). At Nepal's 55 B per place for PLACES, NAMES, SEARCH and indexes,
plus more names per large city, 34,208 cities take about 2–4 MB; `cities5000` would push the
baseline past 8 MB.

### 3.7 The first slice, measured (D-145)

Nepal's slice of its 2026.11.0 build at 500 m: the country, 7 provinces and the 45 places of
the 46 `cities15000` rows (Panauti is listed twice), 53 rows. Main 158,784 B, lite 150,336 B,
about 80 kB on the wire. The 4,096-cell region grid is the A/B winner
([forge](../../04-architecture/forge.md#the-baseline-slice-baseline)). Building it showed that
level-1 units were HIERARCHY roots, not children of the country (fixed, D-146).

### 3.8 The world part, measured (D-147)

Natural Earth admin 0 (Nepal view) as the tagged GitHub GeoJSON (`v5.1.2`, 13,242,878 B) is
noded: the topology stage needs no snapping, and keeps 480,114 vertices on 4,624 arcs, 239,502
at the 500 m level and 100,814 at 2 km. With `cities15000` outside Nepal (34,162 cities, 244
codes), the main build is 3.77 MB, 2.76 MB on the wire; with Nepal's slice (0.16 MB) the
baseline is about 3.9 MB, half the 8 MB budget, before admin 1 and timezones. The tolerance
A/B is in D-147. Codes the list uses that Natural Earth's view folds into another feature
(TW, XK, and France's overseas departments) take the feature that contains them.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Natural Earth admin 0, Nepal view | Public domain; a POV build per country; simple | Not Nepal's 2020 map (§3.1) | Public domain | 5.1.1 (2022) | **admin 0 where no pack exists** |
| Natural Earth admin 1 | Public domain; Wikidata and GeoNames IDs on every unit | Stale for Nepal (2015 reform missing) | Public domain | 5.1.1 | lite fallback only |
| OSM admin level 4 | Current; the main build's source already | A world input is planet scale (the planet file is tens of GB, **unverified** today): needs a boundary-only extract made in CI, not on this machine | ODbL | live | **main admin 1** |
| geoBoundaries gbOpen ADM1 | Per country, more current than NE | Licence per country; quality varies | mixed (**unverified**) | 6.0.0 | **lite admin 1 where licence allows** |
| geoBoundaries CGAZ | One file | US State Department lines for disputes | CC BY 4.0 | 6.0.0 | rejected (ADR-0006) |
| GeoNames `cities15000` | IDs shared with the packs' records; CC BY | Population thresholds vary in quality | CC BY 4.0 | daily | **cities** |
| GeoNames `cities5000` | Twice the towns | Over budget | CC BY 4.0 | daily | rejected (size) |
| Natural Earth time zones | Public domain | Offsets, 2012, few IANA names | Public domain | 5.0.0 | rejected |

## 5. What we build ourselves, and why

- **`forge baseline`:** a build whose recipe lists countries. For a country with a pack recipe
  it reads that build's output (records, IDs, outline) and keeps the country, level-1 units and
  places above the threshold. For the others it reads the sources above, keys each place by
  its GeoNames and Wikidata IDs into that country's registry, and simplifies with the same
  ARCS code at a tolerance from the recipe.
- **The locator:** a REGION over each address's coverage polygon, simplified and buffered by
  the same tolerance (engine and catalog §3.6).
- **Gates:** Z1 size (8 MB), the ID gate (a country's baseline IDs equal its pack's IDs), the
  border probe for the locator, G-lic per source and build.

## 6. Impact on the plan

- D-144 records the source choices and the slice rule; D-145 the slice as built; D-147 the
  world part as built.
- Phase 4.2 gains the slice rule and the per-country admin-1 source report.
- Pinned: `ne-admin0-nep` (2026-11-10) and `geonames-cities15000` (2026-11-09). Still to pin
  with the admin-1 step: `ne-admin1`, and OSM admin 1 extracts for main.

## 7. Risks and open items

- Main's OSM admin 1 for the world needs a boundary-only extract (`osmium tags-filter` on the
  planet) in a CI job with the disk for it; until that job exists, main uses the lite admin-1
  sources too and says so in its report.
- Natural Earth is updated rarely (last release 2022). Countries without a pack inherit its
  age until their pack exists; the baseline states its precision tier on every answer.
- Natural Earth's Nepal view encodes other disputes as Natural Earth reads Nepal's position.
  Nepal's official view of disputes elsewhere is **unverified**.
- The lite claim polygon for Nepal is still Q-015; until it is answered, lite's baseline Nepal
  outline is the lite pack's (claim area disputed, no admin unit).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Natural Earth vector | 5.1.2 release, files 5.1.1 (time zones 5.0.0) | 2026-11-09 | GitHub `nvkelso/natural-earth-vector`, naciscdn.org |
| geoBoundaries CGAZ | 6.0.0 (2023-09-13) | 2026-11-09 | Earth Engine catalog |
| GeoNames `cities15000` | dump of 2026-11-09 | 2026-11-09 | download.geonames.org |

## 9. Sources

- [Natural Earth disputed boundaries policy](https://www.naturalearthdata.com/about/disputed-boundaries-policy/)
  and the files on naciscdn.org, accessed 2026-11-09.
- [geoBoundaries ADM1 v6.0.0](https://developers.google.com/earth-engine/datasets/catalog/WM_geoLab_geoBoundaries_600_ADM1)
  and [the geoBoundaries project](https://github.com/wmgeolab/geoboundaries), accessed 2026-11-09.
- GeoNames `export/dump/readme.txt` and `cities15000.zip`, accessed 2026-11-09.
- [ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md),
  [ADR-0008](../../03-decisions/adr/ADR-0008-stable-ids.md),
  [ADR-0010](../../03-decisions/adr/ADR-0010-addressing-and-baseline.md), D-054, D-110, D-138.
