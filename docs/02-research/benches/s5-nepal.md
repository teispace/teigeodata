---
title: "Spike S5: GeoNames, OSM and Wikidata for Nepal"
status: research
updated: 2026-11-06
---

# Spike S5: GeoNames, OSM and Wikidata for Nepal

**Question:** how good and how different are the three point sources for Nepal, so we can set
the σ priors, weights, accuracy targets (Q-005) and conflation thresholds? The plan and the
metrics are in the [forge note §S5](../notes/forge-pipeline.md#spike-s5-geonames-vs-osm-vs-wikidata-for-nepal);
the stages are described in [forge](../../04-architecture/forge.md). This page fills in as the
steps land.

Inputs are the pinned sources in `data/sources.lock` (OSM `nepal-261105`, the GeoNames NP dump,
the QLever Wikidata snapshot and COD-AB, all retrieved 2026-11-06). Every table below is
reproducible: run `forge read`, then the stage named under the heading. All outputs are
byte-identical on a second run.

## Boundaries and the claim area (S5.3)

`forge pip codab-np-geojson <level> geofabrik-np`. COD-AB levels 0–3 assign the same 21,125
of 21,686 OSM points, and no point falls in two units, so the levels nest. Of the 561 points
outside: 531 are across the southern, western or eastern border (the extract is cut with a
buffer), 10 across the northern border, and 20 in the Limpiyadhura claim area.

**Q-010:** the COD-AB outline (valid 2024-03-14) excludes the claim area. Result: D-054.

| Place (OSM) | Lat, lon | COD-AB |
|---|---|---|
| Kuti | 30.3079, 80.7599 | outside |
| Rongkang | 30.2033, 80.8316 | outside |
| Nabi | 30.2050, 80.8373 | outside |
| गूंजी (Gunji) | 30.1866, 80.8508 | outside |
| Garbyāng | 30.1251, 80.8575 | outside |
| Nabhidang Camp | 30.2391, 80.9760 | outside |
| छाङ्ग्रु (Changru), control | 30.1295, 80.8833 | Byas (NP0775401) |
| कालापानी (Kalapani) | 30.2233, 80.9122 | Byas (NP0775401) |
| तिंकर (Tinkar), control | 30.1355, 80.9848 | Byas (NP0775401) |

## Rounding, stacking and centroid snap (S5.4)

`forge geofix <source> codab-np-geojson`. The method is in the stage's module docs. In short:
- **Native precision:** the finest decimal count that at least 1% of a source's points hold.
- **Decimal null:** exact under uniformity at that precision.
- **Minute-grid null:** the same test on a grid shifted by 0.3536 of its spacing, so precision
  and spatial clustering cancel. Points with ≤ 2 decimals are left out of the grid rows,
  because 0.1° and 0.01° fall on minute grids too.
- **Snap:** within 50 m of a COD-AB area centroid at any of the 860 units of levels 0–3; the
  null shifts each point by 0.02°.

Intervals are 95% Wilson. Measured 2026-11-06, each run takes 0.4 s with a peak RSS of at most
88 MB.

| Rate | OSM (21,686) | `GeoNames` (148,210) | Wikidata (9,574) |
|---|---:|---:|---:|
| Native precision | 7 decimals | 5 decimals | 6 decimals |
| One decimal fewer than native (null 1%) | 2.29% (2.10–2.49) | **24.90%** (24.68–25.12) | **66.06%** (65.11–67.01) |
| ≤ 3 decimals (null ≤ 0.01%) | 0.02% (0.01–0.05)¹ | 0.31% (0.28–0.34) | **34.79%** (33.84–35.75) |
| On the 1′ grid, > 2 decimals (null 0.00%) | 0.00% | 0.09% (0.08–0.11) | **9.40%** (8.83–10.00) |
| On the 1″ grid, > 2 decimals (null: 0.00% OSM, 0.15% GeoNames, 0.00% Wikidata) | 0.00% | 0.28% (0.26–0.31) | **22.80%** (21.97–23.65) |
| Stacked: ≥ 2 records on one point | 0.01% | 0.07% (0.06–0.09) | **8.01%** (7.48–8.57) |
| Stacked: ≥ 5 records on one point | 0.00% | 0.00% | 0.29% (0.20–0.42) |
| Within 50 m of a COD-AB centroid | 0.00% (null 0.01%) | 0.01% (null 0.01%) | 0.00% (null 0.00%) |

¹ For OSM the row is "≤ 4 decimals", three below native.

**Findings:**
- **OSM is close to unrounded:** 98% of points carry all 7 decimals.
- **`GeoNames` NP is a mix of 5-decimal and 4-decimal points:** a quarter hold 4 decimals
  against a 1% null. It is not rounded to DMS grids (excess at most 0.14%), which confirms the
  earlier finding that `GeoNames` NP is not obviously grid-rounded. Its stacks are rare (106
  points).
- **Wikidata is the most rounded source:**
  - A third of its points hold at most 3 decimals: 3,123 hold exactly 2 decimals (0.01°,
    about 1.1 km).
  - About a quarter of the points with more than 2 decimals were typed as degrees, minutes and
    seconds (1′ or 1″ grids).
  - 8% are stacked. The largest stack is nine village items (Daleli, Harakpur, Bihibare and
    others) on 26.6167, 87.4833, the 1′-rounded point of one parent unit.
- **No source snaps to COD-AB area centroids.** A source that snaps to its own polygon's
  centroid or label point would not show up here. The check against each record's own unit
  polygon, with polylabel points, comes with the conflation in S5.5.

**Consequence for geofix-consensus (D-055):** a point's σ prior per axis is at least the
quantisation noise of its detected precision, step/√12. That is 32 m per axis at 3 decimals,
321 m at 2 decimals, and for the 1′ grid 535 m in latitude. Points in a stack of two or more
in the same source count as one observation, so a shared parent coordinate cannot outvote an
independent source.

## Stated links and position disagreement (S5.5, first part)

`forge links geofabrik-np geonames-np wikidata-np`. The sources' own `same_as` links (OSM
`wikidata=*`, Wikidata P1566 and P402, `GeoNames` `wkdt`) are joined by union-find, so an
OSM–`GeoNames` pair stated only through Wikidata counts too. Each cross-source pair's
great-circle distance measures position disagreement without depending on our matcher.
Measured 2026-11-06: 0.06 s, 54 MB peak RSS, 179,470 points.

- 6,762 stated links between records with points form 6,045 clusters.
- **Copies:** a pair closer than 1 m is one point copied between sources, not two
  observations, so it is counted and kept out of the percentiles.
  - **31% of the linked `GeoNames`–Wikidata pairs are copies** (1,959 of 6,235).
  - Of these, 1,290 are former VDC units (`ADM4H`), most with no P31 class: bot items
    created from `GeoNames`.
  - OSM is independent of both: 3 copies in 984 pairs.
- **Granularity mismatch:** most OSM–`GeoNames` pairs join an OSM village node to a `GeoNames`
  former VDC (`ADM4H`), through a Wikidata item that stands for both. That is two entities
  joined by a link, not a position error, so pairs are reported by the kinds of both ends.

| Pair (both populated places) | Pairs | Copies | P50 | P90 | P99 | > 10 km |
|---|---:|---:|---:|---:|---:|---:|
| OSM–`GeoNames` | 145 | 0 | 360 m | 2.1 km | 90.0 km | 4 |
| OSM–Wikidata | 489 | 2 | 884 m | 3.6 km | 38.7 km | 14 |
| `GeoNames`–Wikidata | 477 | 18 | 1.1 km | 4.7 km | 171.1 km | 25 |
| `GeoNames` former VDC–Wikidata place (admin / populated place) | 2,908 | 274 | 795 m | 2.8 km | 13.0 km | 44 |

**Findings:**
- **Villages disagree by hundreds of metres between independent sources:** P50 360–1,100 m
  and P90 2–5 km. A village is spread out, and each source puts its point somewhere
  different. The point is not the problem the rounding detector solves: a settlement-core
  estimate (the mean-shift building source of the note) is what can bring P90 down. This
  feeds Q-005.
- **The tail is wrong links,** not noise. Examples: Mahādevtār joined to a same-named place
  90 km away, and pairs at 300 km and 702 km inside an 880 km country. The matcher must check
  links rather than trust them; a stated link is evidence, not truth.
- **Duplicates inside a source:** clusters with two or more records of one source number 233
  for Wikidata, 74 for `GeoNames` and 4 for OSM. In Wikidata, 234 `GeoNames` IDs are each held
  by two items. Typically one is curated (Sangachok, Q486972) and one is a classless Cebuano
  bot item ("conseho sa kaayuhan sa balangay", that is, VDC).

**Consequence (D-056):** cross-source copies count as one observation, as stacks do (D-055).
Stated links are scored like any other evidence. A wrong link beyond the class's distance
gate is cut, and the cut is listed for upstream fixing.

## Still to come

S5.5 the probabilistic matcher (blocking, Fellegi–Sunter, evaluated against these links as
silver labels) and aliases into the golden set; S5.6 DEM disagreement; S5.7 the summary for
Q-005.
