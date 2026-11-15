---
title: "Boundaries and distance in the language API"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# Boundaries and distance in the language API

## 1. Question

Phase 7.4: the servers answer a place's outline (`/v1/boundary`, MCP `boundary`) and the
distance between two places (`/v1/distance`, MCP `distance`), but the language API (and so
every binding) has neither; it has only `distance_m` between two points. The playground (7.6)
draws outlines on a map from the WebAssembly build, so it needs the outline too. In what form
does a binding answer an outline, and how is the distance between places asked, without a
second copy of what the servers already do?

## 2. Summary and recommendation

- **`Engine::boundary(id, lod)`** answers one row: `id`, `lod` (the level read), `tolerance_m`,
  `vertices` and `geojson`, an RFC 7946 Feature (Polygon or MultiPolygon, or the place's Point
  when it has no outline). No row for an unknown ID; `InvalidArgument` for `lod` above 6;
  `MissingComponent` in a build or install without `boundaries`. GeoJSON is what every map
  library and geometry library takes as is; its cost over packed binary (about 22 against 8
  bytes a vertex) is paid once per outline, not per query.
- **One writer.** The GeoJSON geometry writer moves from the server (`output::geometry`) into
  the engine (`Outline::write_geojson`), so the HTTP answer, the MCP resource and every
  binding write the same bytes for the same outline (winding by RFC 7946's right-hand rule,
  seven decimals as stored).
- **`Engine::distance(from, to)`** between two place IDs answers one row, `meters` and
  `bearing`; no row when either is unknown. A point to a place is `distance_m` with the place's
  `lat` and `lon`, so it needs no third form.
- **`bearing_deg(lat1, lon1, lat2, lon2)`** joins `distance_m`, and both call the engine's
  `meters_between` and `bearing_deg` (libm, the same digits on every platform) instead of the
  API's own haversine, a second copy found while researching this note.
- Parity values for both in `bindings/parity.json`, checked in every language.

## 3. Findings

- **Size and parse cost** (the released `world-np-lite.tgd`, data-2026.11.14.3, measured
  2026-11-15 through `/v1/boundary`): Nepal 10,374 vertices, 226,067 B of geometry, 21.8 B a
  vertex; Bagmati 4,129 vertices, 90,012 B; Kathmandu metropolitan city 176 vertices, 3,867 B.
  Packed `i32` pairs would be 8 B a vertex (82,992 B for Nepal). Python's `json.loads` parses
  Nepal's whole Feature in 2.1 to 2.4 ms. This pack stores outlines at full resolution only,
  so every `lod` reads as 0 (`tolerance_m` 0); a pack with levels answers coarser ones.
- **What consumers take.** MapLibre GL JS takes inline GeoJSON as a source's `data` (a
  FeatureCollection, a Feature or a geometry); Leaflet's `L.geoJSON` takes a Feature or a
  geometry; Shapely's `shape()`, Turf, JTS and NetTopologySuite read GeoJSON geometry. No
  binding needs code of its own to hand an outline to a map.
- **RFC 7946** asks for exterior rings counterclockwise and holes clockwise (§3.1.6) and
  suggests six decimals (§11.2); the server already writes the right-hand rule and seven
  decimals, the precision the pack stores (E7), so a round trip loses nothing.
- **The engine already has the work:** `Engine::boundary(place, lod, &mut Outline)` decodes the
  rings, `boundary_vertices` counts them without decoding, `meters_between` and `bearing_deg`
  measure. Only the GeoJSON writer lives in the server.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| GeoJSON Feature in a result row | What every map and geometry library takes; one writer for the servers and the bindings; readable | 2.7 times the bytes of packed integers; a JSON parse per outline (2 ms for a country) | **Chosen** |
| Columns of vertices (`part`, `hole`, `lat`, `lon` per row) | Compact, read in place by the `Reader` | Each language must rebuild rings before a map can draw them: ten copies of the same assembly | Rejected |
| WKB (ISO 13249) bytes | Compact; GEOS, PostGIS, JTS read it | No browser map takes it; a second writer | Rejected |
| A bare string return | Simplest | Breaks the rule that every query answers a result (no row for an unknown ID), and loses `lod` and `vertices` | Rejected |

## 5. What we build ourselves, and why

`Outline::write_geojson` in the engine (feature `boundaries`), with the server's tests kept
and a test that a hole's winding is reversed. The API method picks no level by vertex count:
the caller names the level, and `vertices` says what it cost. The servers keep their vertex
bound, which protects a shared server, not a library on the caller's own machine.

## 6. Impact on the plan

Phase 7.4 implements this (D-324). The IDL gains two engine methods and one function; the
bindings, the guides' tours and the reference regenerate from it.

## 7. Risks and open items

- `distance_m` switching to the engine's libm haversine may move its last digits; the parity
  values allow 1e-6 relative, and the Kathmandu to Pokhara check stays within 1 m.
- A whole world-scale outline at level 0 can be megabytes; the reference says to ask a coarser
  level for display.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Pack | data-2026.11.14.3 `world-np-lite.tgd` | 2026-11-15 | GitHub release |
| RFC 7946 | August 2016 | 2026-11-15 | rfc-editor.org |

## 9. Sources

- [RFC 7946: The GeoJSON Format](https://www.rfc-editor.org/rfc/rfc7946), IETF, 2016.
- [MapLibre GL JS: GeoJSON source](https://maplibre.org/maplibre-gl-js/docs/examples/multiple-geometries/), accessed 2026-11-15.
- [Leaflet: L.GeoJSON](https://leafletjs.com/reference.html#geojson), accessed 2026-11-15.
