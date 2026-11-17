---
title: "A country's deepest admin level from OpenStreetMap (Nepal's wards)"
status: accepted
updated: 2026-11-17
owner: teigeodata maintainers
revisit: 2027-05-17
---

# A country's deepest admin level from OpenStreetMap (Nepal's wards)

## 1. Question

An app names "my location" down to the ward (TG-27, D-363). Where do outlines for a
country's deepest admin level come from, and how do they join the boundary source's levels
without breaking the shared-arc topology (B10) and its parent check (G-topo)?

## 2. Summary and recommendation

- No openly licensed ward layer was found for Nepal (§3.1). OpenStreetMap has nearly all
  of them under ODbL, so wards go into the **main build only**. The licence gate (G-lic)
  keeps them out of lite.
- A new stage, `deepest`, reads one OSM admin level from the pinned extract and assembles
  its relations into rings.
- It overlays the areas and the boundary source's finest units in one planar arrangement
  (`topo::faces`): every ring is snap-rounded together, noded, and walked into faces. Each
  face is labelled by its unit and by the area holding it.
- It gives narrow faces to the neighbouring area and keeps wider gaps as fillers. A ward is
  the union of its faces, so neighbours share their edges exactly. It then writes the result
  as one more level of the boundary set.
- The topology and the build read the derived set as they read COD-AB, so every level
  still nests exactly and shares one arc set.
- **Correction, 2026-11-17:** neither build had a single ward before this. The 2,622 (lite)
  and 2,806 (main) level-4 places are the former village development committees (GeoNames
  `ADM4H`, D-110), not wards.

## 3. Findings

### 3.1 Sources

| Source | Wards | Licence | Verdict |
|---|---|---|---|
| OCHA COD-AB Nepal v02 (our boundary source) | none: levels 0–3 (775 level-3 units) | CC BY-IGO 3.0 | keeps levels 0–3 |
| OpenStreetMap, `boundary=administrative` + `admin_level=9` | 6,741 relations (Overpass count, 2026-11-17) of 6,743 | ODbL 1.0 | main build |
| Open Knowledge Nepal, LocalBoundaries | none: provinces, districts, local levels | CC BY 4.0 | no wards |
| geoBoundaries NPL (HDX) | ADM0–ADM3 only | ODbL | no wards |
| A GitHub ward shapefile (SaugatPdl) | wards | no licence, no source stated | unusable |
| geonepal.info ward layer | wards | no licence stated | unusable |

### 3.2 How OSM draws Nepal's wards

These were sampled with Overpass on 2026-11-17 and measured with shapely.

- **Tags:** wards are tagged `ward=<n>` and named `<local level>-<nn>` ("Kirtipur-03").
  Local levels are `admin_level=7`.
- **Kirtipur:** its 10 wards tile the OSM local level exactly. There is no gap and no
  overlap, because they share ways.
- **Distance from COD-AB:** the OSM outline of Kirtipur lies up to 89 m from COD-AB's
  (Hausdorff distance). The symmetric difference is 0.21 km² of 14.78 km².
- **Kathmandu:** it has 32 wards. 0.33 km² of the OSM local level is covered by no ward,
  so a relation is missing or broken. OSM's and COD-AB's outlines differ by up to 234 m.

So wards cannot be used as drawn. Their outer edges disagree with COD-AB by tens to
hundreds of metres, and some leave holes.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. Wards as a separate layer, not in the topology | small change | a point can fall in a ward of one local level and inside another local level; two arc sets; `admin_at` disagrees with itself | no |
| B. OSM as the whole boundary source (levels 1–4) | one consistent source | replaces COD-AB's P-codes (IDs) and its official lines; ODbL for every level | no |
| C. Cut OSM wards to COD-AB's local levels, fill the rest, and dissolve the parents from them | levels nest exactly; COD-AB lines and P-codes kept; one arc set | ward lines move to COD-AB's along local-level borders | **chosen** |

## 5. What we build ourselves, and why

- **`read::osm_areas`:** reads the relations of one admin level. It takes three bounded
  passes over the extract (relations, then their ways, then their nodes), so memory holds
  only boundary geometry, never the whole node table. It joins member ways into closed
  rings by node ID and reads them even–odd. A relation whose ways do not close is
  reported and left out.
- **`topo::faces`:** one planar arrangement of every ring, the areas' and the units'
  together. Rings are snap-rounded on one grid and noded, half-edges are sorted around each
  vertex by exact integer angle, and faces are walked. Each clockwise cycle (an island's
  outside) becomes a hole of the smallest face of another component that encloses it.
  `rings_of` turns a set of faces back into rings by cancelling twin edges.
- **`topo::deepest`:** labels and groups the faces.
  - An area's number comes from its tag (`ward`), or failing that from the trailing digits
    of its name, within 1–99 (OSM holds `ward=1433`).
  - A face takes the unit it lies in, and the area that holds most of it within that unit.
    Two areas with the same number in one unit become one ward.
  - A face narrower than `SLIVER_M` (60 m) on average (mean width `2A/P`) joins the
    neighbour it shares the longest border with, in up to eight passes.
  - Any other face with no area becomes a filler (code `~…`): no place, but the unit stays
    equal to the union of its children.
  - Each ward's code is the unit's P-code plus the 2-digit number: Nepal's ward code, which
    is also the ward's postal part (D-090).
  - Its English name follows the recipe's pattern (`{parent}-{number}`). The build adds the
    parent's other names with the number in that script's digits (CLDR default numbering).
- **Why one arrangement, not an overlay per ward:** the first build cut each ward with
  `pov::cut` (D-150). Overlays run piece by piece snap their edges differently, so two
  neighbours could overlap by a hair and an edge could border three units; G-topo failed.
  With one arrangement every edge exists once, and the topology stage passes at 0.021 m
  maximum deviation.
- **The derived set:** written to `data/build/deepest/<admin>+<source>/` as the boundary
  source's own GeoJSON levels plus the new one. The topology stage reads it with a parent
  tolerance measured on the overlay's noise, not COD-AB's 1 m. The build reads it as its
  admin source.
- **Measured on Nepal, 2026-11-17:** 6,723 OSM areas read and 18 left out (their ways do
  not close); 6,717 numbered wards; 32 areas with no number. Fillers cover 12,240 km²: the
  22 national parks and 7,666 km² of mountain areas inside municipalities with wards. The
  stage takes about 60 s and a 481 MB peak. Topology: 8,081 polygons on 25,187 arcs.
- **What must prove it:**
  - G-topo passes, with every local level equal to its wards plus fillers.
  - Ward count and coverage are reported per run.
  - `admin_at` at a sample of points answers the ward that OSM's own polygon holds, away
    from borders.
  - Pack and wire sizes are measured at 50 m and 100 m tolerance.

## 6. Impact on the plan

- **D-363:** wards from OSM in the main build; app data as two packs.
- **`build.yaml`:** a `deepest` key for a country's deepest level (world-general: a source,
  the OSM `admin_level`, the number tag, our level, the name pattern).
- **Phase 8 §8.5b:** the TG-27 step.
- **Q-035:** stays open for lite.

## 7. Risks and open items

- **Moved lines:** a ward's outer line becomes COD-AB's local-level line. Inside a local
  level its lines are OSM's.
- **Gaps:** where OSM lacks a ward, the area stays a filler. `admin_at` answers the local
  level there, and the summary counts these areas.
- **Ward numbers:** a ward's number from OSM could be wrong. Duplicates and missing numbers
  are reported per local level.
- **Changed parents:** settlements' parents become wards in the main build. This is the
  hierarchy to the lowest level the vision asks for; the IDs of existing places do not
  change.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| OSM extract | Geofabrik nepal-261105 (pinned `geofabrik-np`) | 2026-11-17 | `data/sources.lock` |
| COD-AB Nepal | v02, HDX resource of 2026-08-14 | 2026-11-17 | `data/sources.lock` |
| Overpass API | 0.7.62.11 | 2026-11-17 | overpass-api.de |
| `osmpbf` | 0.3.8 | 2026-11-17 | `forge/Cargo.lock` |
| `i_overlay` | 9.1.0 | 2026-11-17 | `forge/Cargo.lock` |

## 9. Sources

- [OpenStreetMap: Nepal's admin levels](https://wiki.openstreetmap.org/wiki/Tag:boundary%3Dadministrative#10_admin_level_values_for_specific_countries), OSM wiki, accessed 2026-11-17.
- [Overpass API](https://overpass-api.de/), counts and geometry, 2026-11-17.
- [OCHA COD-AB Nepal](https://data.humdata.org/dataset/cod-ab-npl), HDX, accessed 2026-11-17.
- [Open Knowledge Nepal, LocalBoundaries](https://github.com/openknowledgenp/localboundaries), CC BY 4.0, accessed 2026-11-17.
- [geoBoundaries Nepal on HDX](https://data.humdata.org/dataset/geoboundaries-admin-boundaries-for-nepal), accessed 2026-11-17.
- [SaugatPdl/nepal-administrative-boundary-shapefiles](https://github.com/SaugatPdl/nepal-administrative-boundary-shapefiles), no licence, accessed 2026-11-17.
