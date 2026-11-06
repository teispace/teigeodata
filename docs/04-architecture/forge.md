---
title: "The forge (data pipeline)"
status: draft
updated: 2026-11-06
---

# The forge (data pipeline)

The forge turns pinned sources into packs ([ADR-0035](../03-decisions/adr/ADR-0035-forge-architecture.md)).
It is a separate Cargo workspace, `forge/`, so its heavy dependencies never reach the runtime
crates ([ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md)). The research is the
[forge note](../02-research/notes/forge-pipeline.md). Stages land as the spikes and phases need
them; this page describes what exists and the planned order.

## Stages

```
sources.lock ─► fetch ─► read/<source> ─► normalise ─► geofix-detect ─► block ─► score ─► cluster
   ─► corrections ─► geofix-consensus ─► topology ─► hierarchy ─► names ─► postal ─► timezone
   ─► QA gates ─► pack writer ─► sign ─► catalog
```

| Stage | State | Notes |
|---|---|---|
| fetch | **built** (S5) | below |
| read/osm, read/geonames, read/wikidata | **built** (S5) | below |
| read/codab | S5 | boundaries for point in polygon |
| geofix-detect, block, score, cluster | S5 | rounding, stacking and centroid-snap flags; Fellegi–Sunter |
| geofix-consensus | S5 | weighted Weiszfeld; DEM sampling |
| the rest | Phase 2 | |

## Sources and `fetch`

Every input is listed in `data/sources.lock` (schema `teigeodata/sources/v1`) with:
- its upstream URL, its version and the date it was retrieved;
- its SHA-256 and size;
- its licence and licence class ([`CLEAN_ROOM.md`](../../CLEAN_ROOM.md) rule 1);
- the countries it covers and what uses it.

`cargo xtask check-data` validates the file in CI and in the pre-commit hook. It rejects an
unpinned (all-zero) hash, a `latest` URL not marked `moving`, an unknown licence class, and a
country with no folder.

`forge fetch [--pin] [--verify] [<id>…]` works like this:
- It downloads each source into the content store `data/cache/store/sha256/<hex>`, streaming
  through SHA-256, and moves the file into place only when the bytes match the pin. A mismatch is
  an error that names both hashes.
- A stored file is not downloaded again. `--verify` re-hashes it.
- `--pin` records new bytes in the lock (the hash, size and date lines only; comments are
  kept). Use it for a first fetch, or for a deliberate update of a moving source.
- A `sparql` source posts its query template (`forge/queries/*.rq`) with the parameters from
  the lock. Templates are world-general: a country's Wikidata item comes from its profile.
- A fetch prefers `mirror_url`, our release asset, when one is set.

`forge path <id>` prints where a fetched source is stored.

**Moving sources.** GeoNames dumps and Wikidata snapshots change in place and have no dated
archive. They carry `moving = true`, and a release build must read them from our mirror. The
mirror is created in Phase 2 with the first data release. Until then, spikes read the local
store and publish only statistics.

## Readers

`forge read <osm|geonames|wikidata> <source-id> [<source-id>…]` writes three Parquet tables
to `data/build/read/<source-id>/`:

| Table | Columns |
|---|---|
| `records.parquet` | source, ID, source class, kind, display name, lat/lon on the 1e-7° grid, stated elevation, stated DEM elevation, population, end date, licence class |
| `names.parquet` | ID, text, language, kind (`name`, `label`, `alias`, `official`, `alternate`, `old`, `short`, `colloquial`) |
| `links.parquet` | ID, `parent` or `same_as`, target |

**Built for planet-size inputs:**
- Every reader streams its input. The rows it keeps are held and sorted in memory, which is
  bounded by the shard: the forge shards by country (ADR-0035). A shard larger than memory
  would need an external sort.
- The OSM reader tests tags lazily and allocates only for places and boundaries.
- `GeoNames` lines are split into fixed arrays, with no allocation per line.
- Tables are written in bounded 128 Ki-row batches.
- Rows are sorted and the writer settings are fixed, so a second run gives identical bytes.

Measured on Nepal, 2026-11-06 (M2 Pro):

| Reader | Input | Rows | Time | Peak RSS |
|---|---|---|---|---|
| osm | 415 MB PBF | 31,326 places and boundaries | 1.2 s (parallel) | 128 MB |
| geonames | NP dump plus a stream through all 205 MB of world alternate names, and the hierarchy | 148,210 records, 182,406 names | 3.4 s | 201 MB |
| wikidata | QLever snapshot | 9,586 records, 69,451 names | 0.07 s | 49 MB |

**Clean-up.** `cargo xtask clean` removes build output from both workspaces and keeps the
store: fetched sources are pinned inputs, not build output.
