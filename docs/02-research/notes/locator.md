---
title: "The locator: which country packs a point needs"
status: accepted
updated: 2026-11-10
owner: engine
revisit: 2027-05-01
---

# The locator: which country packs a point needs

## 1. Question

Given a point, which country packs does an app need to answer there in full, and which of
them are not installed? This is the locator of ADR-0010 and
[phase 4 §4.2](../../07-planning/phases/phase-4.md#42-baseline-and-locator). The answer must
never miss a pack (a point near a border may belong to either side, since the baseline's
outlines are simplified), should rarely add a neighbour that is not needed, must cost little
or nothing in the 8 MB baseline, and must work on every installed layer, not only the baseline.

## 2. Summary and recommendation

**A distance query on the outlines every layer already has** (D-149), not a separate buffered
REGION:

- `Polygons::within(arcs, p, at, r)` (the reference) and `within_chunked` (the fast path,
  §3.4): polygon `p` holds `at`, or one of its edges is within `r` of it. One pass over the polygon's rings: the even–odd crossing test and the point–segment
  distance on a plane local to `at` (longitudes scaled by the cosine of its latitude, the
  antimeridian wrapped). Segments with both ends beyond `r` on one side are skipped before the
  distance.
- `Engine::countries_near(at, r, out)`: every layer's `POLY_INDEX` gives the polygons whose box
  meets the box of the circle; the country polygons among them that pass `within` are kept,
  each ID once, as its winning layer answers it.
- `Engine::missing_packs(at, r, out)`: the countries near `at` that no installed pack answers
  for. A pack's address is its country's ID namespace (`np`); a pack of part of a country
  (`ae-du`) counts only where its coverage box holds the point. Features without an ISO code
  (Natural Earth's disputed areas, namespace `world`) have no pack and are left out.
- `LOCATOR_RADIUS_M = 1000`: the world part's outlines are within 1 km of the source (D-147).
  An outline simplified within `t` of the true border has every true border point within `t`
  of it, so `r ≥ t` never misses the country.

Measured on the world part (main, 2026-11-10): no country missed over 109,659 border probes at
1 km (173 missed at 500 m), 1.25 countries per probe, 99.28% of cities get exactly one, 11.9 µs
per query with the chunk boxes (§3.4). The distance test found a bug in itself (§3.5).

## 3. Findings

### 3.1 The guarantee

The stored outline keeps a subset of the source vertices, and each dropped stretch of the
source border lies within `t` of the chord that replaces it (the Douglas–Peucker bound; the
arc levels of D-097 are built the same way). The set of points within `t` of a chord is
convex (a stadium), so the closed curve of a dropped stretch and its chord lies in it, and so
does everything it encloses. Each point of the true country is therefore inside the stored
outline or within `t` of the stored border, and `within(r ≥ t)` finds its country. The
argument holds on the plane; on the sphere and after the grid snapping of the topology stage
it holds up to a few metres, which the gate checks on real data instead of assuming.

### 3.2 The border-probe gate

`forge world` writes `world-probes-<build>.tsv`: every k-th vertex of every Natural Earth
feature's source outline at full resolution with its feature's ID (k chosen for about 100,000
probes). `tests/world.rs::the_locator_finds_the_country_of_every_border_probe` asks the built
world part for the countries near each probe and fails on any probe whose own country is
missing at `LOCATOR_RADIUS_M`. Smaller radii are measured to show the margin.

Main build, 2026-11-10, 109,659 probes (every 5th of 548,292 source vertices), the world part
alone, release build on the shared Mac (load 15–30):

| Radius | Probes whose country is missed | Countries per probe | µs per query |
|---:|---:|---:|---:|
| 0 | 29,054 (26.5%) | 0.920 | – |
| 250 m | 11,522 (10.5%) | 1.122 | 12.1 |
| 500 m | 173 (0.16%) | 1.251 | 11.8 |
| **1 km** | **0** | **1.253** | **11.9** |
| 2 km | 0 | 1.254 | 15.3 |

A quarter of the border vertices fall outside their own country's stored outline, so
`admin_at` alone is not a locator. 500 m still misses 173: the stored level is chosen at 500 m
but the arcs and the grid add a little, so the radius is the build tolerance (1 km), not the
level's. Beyond 1 km the candidates barely grow, since a probe sits on a border and so already
has its neighbour.

### 3.3 Extra candidates away from borders

Of the 34,147 world cities with a country, 33,900 (99.28%) get exactly one country at 1 km;
89 (0.26%) get none. Those are coastal points more than 1 km off the 1:10m coastline, the
cities whose parent fell back to their code's feature in D-147: offshore, no pack is needed.
The gate allows at most 0.5%.

### 3.4 Speed: chunk boxes

The first version tested every vertex of every candidate country: **240 µs** per query, because
countries with far-flung parts (France, the United States, Russia) have boxes that span the
world and so are candidates almost everywhere. `ArcChunks` stores, per arc and per 128-vertex
block, the chunk's box and its two end vertices (40 B each, 278 kB in memory for the world
part, built on the first query in about a millisecond). `within_chunked` uses them:

- a chunk north or south of the point, or west of it, adds no ray crossing;
- a chunk wholly east of it adds the crossing parity of its two ends (the crossings of a
  polyline with the ray's line telescope to its ends' sides);
- only a chunk whose box holds the point's latitude and longitude, or lies within `r`, is
  decoded.

Result: **11.9 µs** per query at 1 km, 20× faster, same answers. The gates:
`within_chunked_matches_within` (spiky stars of 40–900 vertices, holes, the antimeridian; a
planted parity bug, a halved radius and a wrong skip each fail it) and a differential on the
world part: 11,655,504 checks of every polygon against every 7th probe at 0, 250 m and 1 km,
one difference: a probe exactly on a border at r = 0, where the direction a segment is read in
changes the rounding of a zero distance.

### 3.5 A bug found by the differential

The first `within` wrapped each vertex's longitude around the point on its own. An edge across
the meridian opposite the point then had one end near +180° and the other near −180° on the
local plane, a segment through the point: Indonesia was "within 1 km" of a point in Brazil.
Each segment's second end is now placed from its first by their raw difference (the format's
segments are planar in longitude and split at the antimeridian).
`an_edge_across_the_opposite_meridian_is_far` is the regression gate.

## 4. Options compared

| Option | Strengths | Weaknesses | Bytes | Verdict |
|---|---|---|---|---|
| A buffered REGION of pack addresses (the first design, engine and catalog §3.6) | one `regions_at`, O(1) per query | a second copy of every border, buffered: 0.5–1.5 MB estimated; a new build step; buffering on the sphere is its own algorithm; only the baseline would carry it | 0.5–1.5 MB | **Not now** |
| A distance query on the stored outlines (`within`) | no new bytes; works on any layer with outlines (a pack's own full-resolution outline answers more precisely than the baseline's); the radius is a parameter, so an app can trade candidates for margin | O(vertices) of each candidate polygon per query | 0 | **Use** |
| `admin_at` alone (no radius) | exists | misses the true country near borders, where the simplified outline cuts it off | 0 | No: the gate shows misses at r = 0 |
| Snap to the nearest city's country | trivial | wrong across borders and in empty land | 0 | No |

## 5. What we build ourselves, and why

`within` and `countries_near` are ours: no library holds our outline format. The cosine for the
local plane is a Taylor series (error below 1e-6 at 90°) because `teigeodata-format` is
`no_std`. The query box widens the longitude by the radius over the cosine of the band's
latitude farthest from the equator, takes all longitudes within the radius of a pole, and
crosses the antimeridian the way POLY_INDEX reads it (`min_lon > max_lon`).

## 6. Impact on the plan

- [Phase 4 §4.2](../../07-planning/phases/phase-4.md#42-baseline-and-locator): the locator item
  is done by a distance query, not a buffered REGION; the baseline's size budget no longer
  holds a locator share.
- [Engine and catalog §3.6](engine-and-catalog.md#36-the-baseline-and-the-locator): the
  locator design there is superseded by this note.
- D-149 in the [decision log](../../03-decisions/log.md).
- CI: the world and locator gates run in the data-build job.

## 7. Risks and open items

- Sub-country packs (`ae-du`) are answered by coverage box only; when such packs exist, their
  coverage polygon (a country pack's admin unit) gives the exact answer through the same
  `within` on that unit. Revisit with the first sub-country pack.
- Grid-tile addresses (ADR-0010's `in-br-t07`) are rectangles: a box test, added with the first
  tiled country.
- Query cost: 12 µs on the world part; a full-resolution country pack has far more vertices
  per chunk-free stretch, so its first query builds a larger `ArcChunks` (about 40 B per 128
  vertices). Measure on the first large pack.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Natural Earth admin 0, Nepal's point of view | 5.1.2 | 2026-11-10 | `data/sources.lock` (`ne-admin0-nep`) |

## 9. Sources

- [Ramer–Douglas–Peucker algorithm](https://en.wikipedia.org/wiki/Ramer%E2%80%93Douglas%E2%80%93Peucker_algorithm),
  Wikipedia, accessed 2026-11-10: the bound on the distance of dropped points.
- [Point in polygon: ray casting](https://en.wikipedia.org/wiki/Point_in_polygon), Wikipedia,
  accessed 2026-11-10.
- [Engine and catalog §3.6](engine-and-catalog.md#36-the-baseline-and-the-locator), the first
  design.
