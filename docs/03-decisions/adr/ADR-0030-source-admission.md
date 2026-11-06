---
title: "ADR-0030: Source admission rules per build"
status: accepted
updated: 2026-11-06
---

# ADR-0030: Source admission rules per build

Status: accepted, 2026-11-06. Refines [ADR-0002](ADR-0002-licences.md).

## Context

The licence research found that some sources cannot enter either build, and that several popular
sources carry surprises (per-layer licences, custom terms, inputs derived from OpenStreetMap).

## Decision

Every source adapter declares a licence class ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md)), and each
build admits classes as follows:

| Class | Main (ODbL) | Lite (CC-BY 4.0) |
|---|---|---|
| public domain, CC0 | yes | yes |
| attribution-only (CC-BY, CC-BY-IGO, ODC-By, CDLA-Permissive, GODL and similar) | yes | yes, unless the source was itself built from an ODbL input |
| ODbL | yes | **no** |
| share-alike other than ODbL (CC BY-SA) | **no** (cannot be relicensed as ODbL) | **no** |
| non-commercial (GADM, UN SALB / gbAuthoritative, SHRUG, FABDEM) | **no** | **no** |
| custom terms | case by case, recorded in the source's page | case by case |

Specific rulings (2026-11-06):

- **geoBoundaries** is admitted per layer, by the licence in that layer's metadata (for example
  India ADM2 to ADM5 are ODbL, so main only; Nepal ADM3 is CC BY-SA, so neither).
- **Kontur Population** is CC-BY but built with OpenStreetMap: main only.
- **Microsoft building footprints** are CDLA-Permissive 2.0 at the source (Overture still labels
  them ODbL): taken from the source, they are admissible in lite.
- **timezone-boundary-builder** and Overture's OSM-derived themes are ODbL: main only. Lite
  builds timezone areas from admin polygons and tzdb.
- **Copernicus DEM** has custom terms (fixed notice, liability sentence, no endorsement, binding
  downstream). Main uses it with the notice carried in every pack. Lite uses **NASADEM (CC0)**
  until a legal review clears Copernicus for lite.
- **Wikipedia text** (CC BY-SA) is not ingested; Wikidata (CC0) is.
- Population rasters (WorldPop, GHSL, HRSL) enter lite only after confirming they used no ODbL
  inputs.

## Consequences

- The licence gate enforces this table for both builds, from provenance.
- ODbL note: search results and lookups are insubstantial extracts under the OSMF geocoding
  guideline, but bulk responses from the HTTP and MCP servers can be substantial extracts; the
  servers attach ODbL attribution and the licence to responses from main data.
- Each source has a page in `docs/05-data/sources/` with its licence class and the evidence.

## Alternatives considered

Admitting CC BY-SA into main (a licence conflict); treating Copernicus as CC-BY (it is not).

## Evidence

[Source licences](../../02-research/notes/source-licences.md), 2026-11-06.
