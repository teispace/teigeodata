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

## The matcher (S5.5, second part)

`forge conflate np geofabrik-np geonames-np wikidata-np`; the method is in the module docs
and the [forge note](../notes/forge-pipeline.md). Measured 2026-11-07: 0.43 s, 180 MB peak
RSS, 179,470 points; the clusters are byte-identical on a second run.

- **Blocking:** a shared join key or `indic-v0` phonetic key (named by the profile) within
  neighbouring 0.1° cells. That gives 98,967 cross-source pairs. 54,448 of them pair a
  populated place with an admin unit, a cannot-link, which leaves 44,519. The stated links
  add 3,808 pairs between compatible kinds.
- **Model:** u from 1,000,000 seeded random cross-source pairs. m from three EM sessions, each
  conditioned on the field its blocking selected on (name, then ID, then name). The global
  prior λ is 1.0×10⁻⁵.

| Field | Level weights in bits (strongest agreement first) |
|---|---|
| name | fold equal +14.3; join key +10.4; phonetic key +6.6; none −3.0 |
| distance | < 250 m +16.0; < 1 km +12.2; < 3 km +9.4; < 10 km +5.5; farther −3.0 |
| kind | same family +0.5; one side vague 0.0; different −17.8 |
| ID | stated link +16.2; none −0.1 |

**Silver-label evaluation.** A model **without** the ID field is scored against the stated
links. 3,039 links are left out because they are farther than 10 km apart or join a populated
place to an admin unit (D-056).
- Recall is the share of links whose records end in one cluster.
- Precision covers the merged cross-source pairs whose records both carry stated links to
  the other source: the share the links agree with.

| Source pair | Links | Recall | Merged pairs with links on both sides | Precision |
|---|---:|---:|---:|---:|
| OSM–Wikidata | 492 | 0.882 | 434 | 1.000 |
| `GeoNames`–Wikidata | 3,231 | 0.832 | 2,713 | 0.991 |

The full model clusters 1,107 places across all three sources, 12,469 OSM–`GeoNames`,
4,354 `GeoNames`–Wikidata and 402 OSM–Wikidata.

**What the misses are.** A sample of the stated OSM–Wikidata links the full model leaves
apart:
- **A town node against its eponymous municipality item:** Kawasoti, Tokha, "Ilam
  Municipality". The cannot-link keeps these two entities apart by design. They are joined
  by the unit's `seat` (D-058).
- **A wrong or loose stated link:** मैत्री नगर tagged as Sitapaila, बैतडी as Khalanga.

The misses have not been counted by class yet. That needs the labelled 500-pair sample of the
note, which is maintainer review work.

**Not yet done here:** term-frequency adjustment for common names (Chisapani, Bhanjyang);
the checked-in m/u table; Splink as an oracle. These are Phase 2 forge work (the plan is in
the note).

## Aliases into search (S5.5, third part)

`forge aliases np geonames-np geofabrik-np+geonames-np+wikidata-np` carries the names of every
OSM and Wikidata member of a cluster to its `GeoNames` record. Two rules apply (module docs):
- **Corroboration:** a member must share a name with the record.
- **Collision:** an alias that is, by fold or phonetic key, the primary name of an admin unit
  anywhere, or of another record within 30 km, is not carried.

Measured 2026-11-07: 1.6 s, 251 MB peak RSS, byte-identical output on a second run.

| Run | Aliases carried | Records gaining names | Members not corroborated | Aliases held back as collisions |
|---|---:|---:|---:|---:|
| No rules | 32,163 | 7,109 | – | – |
| Corroboration only | 30,409 | 6,633 | 483 | – |
| Both rules (kept) | 20,017 | 5,207 | 483 | 11,421 |

**Why the rules exist.** Two errors entered search through the first run, and both are fixed
by rules that apply to any country:
- OSM node 303021945 at Lukla is named "नेपाल". It is linked to Lukla's Wikidata item, so
  the merge is right but the name is wrong, and a search for नेपाल ranked Lukla first.
- Wikidata lists हेटौंडा (Hetauda) as an alias of Makwanpur District, whose seat it is, so
  हेटौंडा ranked the district above the city.

**S3 with the aliases in the index** (`S3_ALIASES`, same golden set, 512 queries):

| Run | recall@1 | recall@5 | pass@max_rank |
|---|---:|---:|---:|
| `GeoNames` only (the S3 result) | 0.953 | 0.986 | 0.980 |
| + aliases, no rules | 0.943 | 0.994 | 0.984 |
| + aliases, both rules | **0.959** | **0.994** | **0.988** |

With both rules no class is worse than the baseline:
- colloquial: recall@5 0.857 → 1.000;
- historical: recall@1 0.932 → 0.977;
- exact_native: 0.952 as before.

The misses left are two Newa-script queries (no source has those names yet), ranking ties,
and one variant. The collision rule is precision-first; tuning it against the golden set is
Phase 2 work.

**The golden set stays independent of the forge (D-060).** Golden queries are never generated
from the aliases this stage produces, because that would test the stage against its own
output. The colloquial class (14 of a target 30) grows from curated queries with independent
sources.

## DEM disagreement (S5.6)

`forge dem copernicus-glo30-np geofabrik-np geonames-np wikidata-np` samples Copernicus
GLO-30 (45 pinned tiles, D-061) at every record's point. Run 2026-11-07, M2 Pro: 179,470
points in 7.0 s, peak RSS 278 MB. Heights are bilinear between pixel centres; slope is Horn's
3×3 method. Geoid differences between sources are not corrected (EGM96 against EGM2008 is a
few metres here, below every effect below).

**The sampler is checked against an independent DEM.** `GeoNames` states an SRTM3-derived
`dem` for every record. It agrees with Copernicus at P50 7.0 m and P90 32.3 m over 148,198
points. With the grid shifted by half a pixel (the PixelIsArea reading of a PixelIsPoint
tile) the agreement is worse, P90 35.5 m, so the registration in the tags is read correctly.
Twelve points fall on voids.

**Stated elevations** (stated minus Copernicus at the record's own point, metres; bias is the
median signed difference):

| Source | Points | Bias | P50 | P90 | P99 | > 50 m |
|---|---:|---:|---:|---:|---:|---:|
| OSM `ele` | 238 | +3.9 | 15.6 | 102.6 | 687.5 | 19.3% |
| `GeoNames` `elevation` | 186 | +26.6 | 40.4 | 314.2 | 1,902.5 | 45.2% |
| Wikidata P2044 | 601 | +21.6 | 50.1 | 430.0 | 2,333.5 | 50.1% |
| `GeoNames` `dem` (SRTM3) | 148,198 | +2.6 | 7.0 | 32.3 | 300.3 | 6.9% |

On steep ground the error grows, and it is biased upwards. For Wikidata, P50 is 15.1 m below
5° of slope and 116.1 m above 30°, with a bias of +102.7 m. A stated height often belongs to
somewhere else in the place, a ridge or the highest point of the area, not the point given.

**Position disagreement costs height.** For the stated links that are not copies, the
Copernicus height at one source's point minus the height at the other's:

| Pair | Pairs | P50 | P90 | P99 | > 50 m |
|---|---:|---:|---:|---:|---:|
| `GeoNames`–Wikidata | 4,227 | 26.9 | 362.0 | 1,552.4 | 43.8% |
| OSM–`GeoNames` | 426 | 25.3 | 445.1 | 1,376.7 | 41.5% |
| OSM–Wikidata | 543 | 23.8 | 440.4 | 1,657.3 | 40.5% |

**What follows (D-061):**
- Elevation is sampled from the DEM at the conflated point. A source's stated elevation is
  never the value; it is a QA cross-check, and a disagreement beyond 50 m queues the record.
- The vertical target (P90 ≤ 50 m) is limited by the horizontal one in the hills: the DEM itself
  is good to a few metres, but a point a few hundred metres off is hundreds of metres off in
  height. Position comes first, as the accuracy order says, and geofix-consensus and the
  settlement core are the levers (D-056).

## Still to come

S5.7 the summary for Q-005.
