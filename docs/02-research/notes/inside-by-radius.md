---
title: "Inside a place with no outline: nearest of its kind within a measured radius"
status: accepted
updated: 2026-11-12
owner: maintainer's agents
revisit: 2027-05-12
---

# Inside a place with no outline

## 1. Question

TG-19 asks `reverse` to say when a point is *in* a place, not only near it: "a tap 10 m from a
village names the village; one 1.9 km away returns 'near' with the distance and bearing". Countries
and admin units have outlines, and `Engine::mark_inside` already answers for them (D-235). Villages,
towns and toles are points. What rule says a point is inside one, at what cost, for the whole
planet?

## 2. Summary and recommendation

A point is **inside** a settlement (or a locality) with no outline when that place is the
**nearest place of its kind** among the answers **and** lies within a **radius set by its kind**:
750 m for a settlement and 700 m for a locality. These are the 90th-percentile distances from a
building to its nearest named place, measured on Nepal's Google and Microsoft footprints (§3).
The nearest-of-kind test is the Voronoi assignment the forge already uses for settlement cores
(D-096), so the engine and the data agree on which village a building belongs to. It needs no
extra bytes and one pass over the answer. Settlements and localities each have their own
Voronoi diagram, so a point can be in a tole and in its village at once.

A radius stored per place (option b) is the upgrade if other countries turn out to need very
different radii. Re-measure with this note's query when each new country lands (§10).

## 3. Findings

**How others do it.** Nominatim treats a place node with no area as covering the addresses
nearest to it, within a diameter set by the place's search rank. Its `reverse_place_diameter`
SQL function filters candidates with `distance <= reverse_place_diameter(rank_search)`, and they
are ordered by distance
([commit 646fa53](https://git.openstreetmap.org/nominatim.git/patch/646fa53b44e1ed3624b3af532dfd9a773560eb1e),
[FAQ](https://nominatim.org/release-docs/5.2/api/Faq/)). That is the same shape as this rule:
nearest, capped by class.

**Measured extent, Nepal, 2026-11-12.** For a 5% Bernoulli sample (seed 11) of building
centroids, the nearest settlement or locality in the `np` lite pack, found on a 0.02° grid. Only
buildings within 3 km of a place are kept. The figures are distances in metres.

| Source | Kind | Importance | Buildings | P50 | P90 | P95 | P99 |
|---|---|---|--:|--:|--:|--:|--:|
| Google v3 | Locality | 30 | 2,563 | 319 | 652 | 785 | 1,201 |
| Google v3 | Settlement | 30 | 613,965 | 241 | 753 | 1,352 | 2,610 |
| Google v3 | Settlement | 31–49 | 5,316 | 206 | 637 | 916 | 2,364 |
| Google v3 | Settlement | 50+ | 5,870 | 280 | 637 | 767 | 995 |
| Microsoft | Locality | 30 | 1,595 | 350 | 699 | 858 | 1,565 |
| Microsoft | Settlement | 30 | 341,061 | 227 | 592 | 726 | 1,062 |
| Microsoft | Settlement | 31–49 | 2,997 | 210 | 547 | 701 | 1,047 |
| Microsoft | Settlement | 50+ | 2,423 | 293 | 642 | 764 | 959 |

A first pass measured each settlement's own extent instead (the r90 of its assigned buildings).
For settlements with importance 30 that gave P50 332 m and P90 687 m, with no buildings at 20,412 of
89,678. Per-place r90 at P90 for localities was 826 m.

What the numbers say:

- The two sources agree within about 150 m at P90. Google's longer tail (P95 1,352 m) is
  buildings in unnamed hamlets that sit farther than any named place's extent. Those should
  answer "near", not "inside".
- Importance barely changes the P90 (637–753 m). In Nepal, 99% of settlements carry importance
  30, and larger towns are admin units with outlines, which `mark_inside` already handles. A
  band by importance buys nothing here.
- 750 m keeps the brief's two cases apart by a wide margin: 10 m is inside, and 1.9 km is
  near at 2.5 times the radius.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| (a) Nearest of its kind, radius by kind (engine constants) | 0 bytes; one pass; agrees with the forge's Voronoi assignment; easy to explain | One radius per kind for every country, measured on Nepal | **Chosen** |
| (b) A per-place radius column from the forge (r90 of its buildings) | Fits each place | About 1 byte per place, about 90 kB for Nepal's settlements; 23% of settlements have no buildings, so they need a default anyway; it costs a format change | The upgrade if (a) fails elsewhere |
| (c) A radius only, no nearest test | Simplest | Two villages 600 m apart both claim the point, and the answer contradicts the forge's assignment | Rejected |
| (d) Synthesised outlines (Voronoi cells clipped to the radius) stored as polygons | Exact under `admin_at` | Megabytes per country; it rebuilds what (a) computes for free | Rejected |

## 5. What we build ourselves, and why

`Engine::mark_inside` gains the rule for `Settlement` and `Locality`. `Near::inside` becomes
`Some(true)` for the nearest place of its kind among the answers when it is within the radius,
and `Some(false)` for every other place of that kind. Places of other kinds without an outline
(natural, postal, timezone) stay `None`. The radii are public constants,
`SETTLEMENT_RADIUS_M` and `LOCALITY_RADIUS_M`, so an app can draw the same circle. The nearest
test runs over the answers the caller passes. `reverse` sorts its answers by distance, so the
nearest of a kind is always among them when any of that kind is.

## 6. Impact on the plan

TG-19 closes with this rule (D-238). No format change, no forge stage. Phase 4 lists the
re-measure as a step of each new country's data release.

## 7. Risks and open items

- One radius per kind is measured on Nepal only. Dense Indian towns and sprawling US towns
  may need other radii: re-measure (§10) with each country, and add option (b) if needed.
- The nearest test runs over the answers the caller passes. An app that filters `near` before
  calling `mark_inside` changes which place is nearest.
- Near-duplicate settlements (three Asurkot within 1 km on the `np` pack) split one village's
  Voronoi cell. That is a forge dedupe issue, listed in progress.

## 8. Versions checked

- Google Open Buildings v3 and Microsoft Global ML Building Footprints, as pinned in
  `data/sources.lock` on 2026-11-12; `np` lite pack 2026.11.0; DuckDB 1.x (local).

## 9. Sources

- Nominatim, place-node radius by rank:
  [commit 646fa53](https://git.openstreetmap.org/nominatim.git/patch/646fa53b44e1ed3624b3af532dfd9a773560eb1e),
  [FAQ](https://nominatim.org/release-docs/5.2/api/Faq/).
- [Settlement core](settlement-core.md) for the Voronoi assignment (D-096).

## 10. Re-measuring

Re-run this for each country as coverage grows (India next). If a country's settlement P90 falls
outside 500–1,000 m, revisit option (b) or a per-country default in the profile. `rows.csv` is
the pack's places (row, kind, importance, lat, lon), and `points.parquet` is the building
centroids the forge's buildings stage writes. It runs in DuckDB in a few minutes with 3 GB.

```sql
create table p as select row, kind, importance, lat, lon,
  floor(lat/0.02)::int cy, floor(lon/0.02)::int cx
  from read_csv('rows.csv') where kind in ('Settlement','Locality');
create table pc as select p.*, dy, dx from p,
  (select unnest(range(-1,2)) dy), (select unnest(range(-1,2)) dx);
with bc as (select row_number() over () bid, lat_e7/1e7 lat, lon_e7/1e7 lon,
    floor(lat_e7/1e7/0.02)::int cy, floor(lon_e7/1e7/0.02)::int cx
    from read_parquet('points.parquet') using sample 5 percent (bernoulli, 11)),
  cand as (select bc.bid, pc.row, 6371000*sqrt(power(radians(bc.lat-pc.lat),2)
    + power(radians(bc.lon-pc.lon)*cos(radians(pc.lat)),2)) d
    from bc join pc on bc.cy=pc.cy+pc.dy and bc.cx=pc.cx+pc.dx),
  n as (select bid, arg_min(row,d) row, min(d) d from cand group by bid)
select kind, count(*) buildings, quantile_cont(d,0.5) p50, quantile_cont(d,0.9) p90,
  quantile_cont(d,0.95) p95, quantile_cont(d,0.99) p99
  from n join p using(row) where d <= 3000 group by all order by 1;
```
