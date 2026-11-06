---
title: "Architecture overview"
status: draft
updated: 2026-11-06
---

# Architecture overview

Draft from the decisions of 2026-11-06. The research notes of Phase 0 confirm or revise each
part; the ADR each part comes from is linked.

## 1. The big picture

```
 sources (OSM, Overture, Wikidata, GeoNames, official boundaries, census, DEMs, buildings, tz)
        │  pinned in data/sources.lock, licence-classed
        ▼
 forge (separate workspace) ── ingest → geo-fix → conflate → hierarchy → names → postal/tz
        │                       → QA gates → pack writer → sign            (main and lite builds)
        ▼
 .tgd packs  ── deterministic addresses (np.places, ae-du.boundary-lod1, in-br-t07.places)
        │       published as release assets; catalog signed
        ▼
 runtime: core → format → index → engine (+ baseline, + sync)
        │
        ├─ Rust façade (teigeodata)
        ├─ C ABI (ffi) → IDL → generated bindings: C/C++, Node, WASM, Dart/Flutter, Python,
        │                       JVM, Android, Swift, .NET, Go
        ├─ CLI (tgd)
        ├─ MCP server
        └─ HTTP server (self-hosted; hosted-ready)
```

## 2. Crates (planned)

| Crate | Role | `no_std` | Decision |
|---|---|---|---|
| `teigeodata-core` | IDs, kinds, quantized coordinates, errors, codecs (bit-packing, varints, strings, arcs) | yes | [ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md) |
| `teigeodata-format` | `.tgd` reader and writer: header, manifest, sections, components, checksums, signature | yes | [ADR-0009](../03-decisions/adr/ADR-0009-pack-format.md) |
| `teigeodata-index` | Spatial index, region index, search index, perfect-hash ID maps | yes | [ADR-0013](../03-decisions/adr/ADR-0013-region-index.md), [ADR-0014](../03-decisions/adr/ADR-0014-names-and-search.md) |
| `teigeodata-names` | Normalisation, transliteration, phonetic key, lexicon | yes | [ADR-0014](../03-decisions/adr/ADR-0014-names-and-search.md) |
| `teigeodata-tz` | Timezone lookup over the region index; optional tzdb rules | yes | [ADR-0022](../03-decisions/adr/ADR-0022-timezone-and-postal.md) |
| `teigeodata-dem` | Elevation raster codec and `elevation_at` | yes | [ADR-0007](../03-decisions/adr/ADR-0007-accuracy-model.md) |
| `teigeodata-engine` | Layered pack sets, federated queries, snapshots, baseline | partly | [ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md), [ADR-0011](../03-decisions/adr/ADR-0011-layered-engine.md) |
| `teigeodata-sync` | Catalog, downloads, storage, trust metadata, deltas | no | [ADR-0012](../03-decisions/adr/ADR-0012-offline-loading.md) |
| `teigeodata` | The Rust façade: the API every binding mirrors | no | [ADR-0016](../03-decisions/adr/ADR-0016-bindings.md) |
| `teigeodata-ffi` | The C ABI boundary, result blobs | no | [ADR-0016](../03-decisions/adr/ADR-0016-bindings.md) |
| `teigeodata-idl` | API description extractor and per-language emitters | no | [ADR-0016](../03-decisions/adr/ADR-0016-bindings.md) |
| `teigeodata-mcp` | MCP server | no | [ADR-0017](../03-decisions/adr/ADR-0017-mcp.md) |
| `teigeodata-server` | HTTP server | no | [ADR-0025](../03-decisions/adr/ADR-0025-hosted-ready.md) |
| `teigeodata-cli` (`tgd`) | build, extract, inspect, verify, diff, merge, tiles, export | no | |
| `xtask` | every repository task and gate | no | [ADR-0021](../03-decisions/adr/ADR-0021-rust-tooling.md) |
| `forge/*` (own workspace) | the data pipeline and source adapters | no | [ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md) |

Crates are created when their phase starts, not before. The one exception is
`teigeodata-names`: it was started in Phase 0 with the fold and the phonetic keys from spike S3,
because the forge's matcher must compare names exactly as search does (D-057).

## 3. Capabilities and features

Capabilities map to Cargo features and to native/WASM variants
([ADR-0015](../03-decisions/adr/ADR-0015-tree-shaking.md)): `search`, `reverse`, `tz`, `tz-rules`,
`boundary`, `dem`, `postal`, `sync`, `mcp`, `server`. The default set is lean.

## 4. Query surface (draft)

`search`, `autocomplete`, `reverse`, `nearby`, `within_radius`, `within_bbox`, `get`, `get_many`,
`children`, `ancestors`, `boundary(lod)`, `tz_at`, `tz_offset_at`, `elevation_at`, `postal_lookup`,
`postal_of`, `distance`, `contains`, `packs`, `coverage`, `missing_pack_for(point)`. The surface
and its parameters are specified in `docs/01-product/api.md` (Phase 1).

## 5. Pages to come

`pack-format.md`, `engine.md`, `region-index.md`, `search.md`, `timezone.md`,
`offline-loading.md`, `bindings.md`, `mcp.md`, `security.md`, `performance.md`: each written when
its research note lands.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-06 | First draft | Project start |
| 2026-11-06 | `teigeodata-names` started early | One name code path for the forge and search (D-057) |
