---
title: "Scope of v1"
status: accepted
updated: 2026-11-06
---

# Scope of v1

**v1 is the whole product** ([ADR-0003](../03-decisions/adr/ADR-0003-v1-is-everything.md)).
Nothing in this list waits for a v2. Coverage of countries grows through **data releases**
(CalVer) that any v1 reader loads.

## Software in v1

| Area | In v1 |
|---|---|
| Data model | Places with stable IDs, kinds and country-specific levels; consensus point + uncertainty; elevation + uncertainty (+ footprint min/median/max); hierarchy; names by language, script and type; boundaries at several levels of detail; IANA timezone; postal codes with validity periods; population with year and source; codes (ISO 3166-2, national codes); cross-IDs; provenance |
| Queries | Search (prefix, fuzzy, phonetic, cross-script, abbreviations, ranking, near-bias); reverse geocoding to the lowest admin unit; nearby/kNN/radius/bbox; get/children/ancestors; boundary as GeoJSON; timezone at a point; offsets and transitions (optional tzdb); elevation at any point (DEM packs); postal lookup both ways; distance and containment |
| Format | `.tgd` packs: self-describing, memory-mapped, zero-copy, signed, componentised, levels of detail, deterministic addressing |
| Engine | Layered pack sets merged live by ID; conflict policy; tombstones and redirects; snapshot swaps; compaction; embedded baseline fallback |
| Offline loading | Catalog (browse, search, plan sizes), download manager (queue, resume, verify, atomic install, delta updates), storage manager (quota, eviction, repair), pluggable transport and storage, OS background-download hooks |
| Surfaces | Rust crate; C ABI and headers; bindings for C++, Node, WASM (browser, Deno, Bun, workers), Dart, Flutter, Python, JVM, Android, Swift, .NET, Go; CLI `tgd`; MCP server; self-hostable HTTP server |
| Data pipeline | The forge: ingest, geo-fix, conflation, hierarchy, names, QA gates, pack writer, signing; corrections as data; main and lite builds |
| Docs | Fumadocs site with guides per language, generated reference, per-country pages, measured pages, an in-browser playground |
| Quality | The full [quality bar](../06-engineering/quality-bar.md) |

## Data coverage (data releases, in order)

1. **Nepal**: province → district → local level → ward → settlement/tole; peaks, passes, lakes.
2. **India**: state → district → sub-district → village/town → ward.
3. **South Asia**: Bangladesh, Bhutan, Sri Lanka, Pakistan, Maldives, Afghanistan.
4. **Asia**.
5. **USA**.
6. **The rest of the world**, with depth per country set by data quality and a population
   threshold.

Each wave meets the same gates as Nepal before it ships.

## Not in scope

- Street-level geocoding of full addresses and routing. Street and address data may come
  later as components; they are not part of the place model now.
- A hosted service. The design keeps it possible ([P15](principles.md)).
- Rendering map tiles for basemaps. We ship boundaries (GeoJSON, PMTiles), not basemaps.
