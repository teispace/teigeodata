---
title: "Architecture overview"
status: draft
updated: 2026-11-09
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
        ├─ safe Rust API (teigeodata-api) → IDL (syn) → generated layers:
        │     C ABI (ffi) → C/C++, Swift, Dart/Flutter, .NET, Go
        │     shims → Python (PyO3), Node (napi-rs), JVM/Android (JNI), WASM
        ├─ CLI (tgd)
        ├─ MCP server
        └─ HTTP server (self-hosted; hosted-ready)
```

## 2. Crates (planned)

| Crate | Role | `no_std` | Decision |
|---|---|---|---|
| `teigeodata-core` | Stable IDs and their text form (D-070), grid points (`E6`, `E7`), Hilbert order; `PlaceKind`; `bitpack`, the integer codec (D-073), and `table`, the column table (D-074), encoders with `alloc`; the other codecs as Phase 1 adds them. No dependencies | yes | [ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md) |
| `teigeodata-format` | `.tgd` container: a canonical validating reader (no allocation), the PLACES and HIERARCHY views, a deterministic writer (`write`), Ed25519 (`signature`), fuzz harnesses (`fuzz`). Depends on `zerocopy`, `blake3`, `teigeodata-core` | yes | [ADR-0009](../03-decisions/adr/ADR-0009-pack-format.md) |
| `teigeodata-index` | Spatial index, region index, search index, perfect-hash ID maps | yes | [ADR-0013](../03-decisions/adr/ADR-0013-region-index.md), [ADR-0014](../03-decisions/adr/ADR-0014-names-and-search.md) |
| `teigeodata-names` | Normalisation, transliteration, phonetic key, lexicon | yes | [ADR-0014](../03-decisions/adr/ADR-0014-names-and-search.md) |
| `teigeodata-tz` | Timezone lookup over the region index; optional tzdb rules | yes | [ADR-0022](../03-decisions/adr/ADR-0022-timezone-and-postal.md) |
| `teigeodata-dem` | Elevation raster codec and `elevation_at` | yes | [ADR-0007](../03-decisions/adr/ADR-0007-accuracy-model.md) |
| `teigeodata-engine` | Layered pack sets, federated queries, snapshots, baseline ([engine](engine.md)). Mapped packs with lazy verification; the one crate allowed `unsafe` (D-113) | partly | [ADR-0010](../03-decisions/adr/ADR-0010-addressing-and-baseline.md), [ADR-0011](../03-decisions/adr/ADR-0011-layered-engine.md) |
| `teigeodata-sync` | Catalog, downloads, storage, trust metadata, deltas; the [transport asset](transport.md) | the transport reader | [ADR-0012](../03-decisions/adr/ADR-0012-offline-loading.md) |
| `teigeodata-api` | The safe public API: the single source of every binding ([bindings](bindings.md)) | no | [ADR-0016](../03-decisions/adr/ADR-0016-bindings.md) |
| `teigeodata-blob` | The columnar result blob, encoder and validating reader | yes | D-068 |
| `teigeodata-ffi` | The C ABI, generated from the API ([C ABI](abi.md)) | no | [ADR-0016](../03-decisions/adr/ADR-0016-bindings.md) |
| `bindings/*` (own workspace) | Generated PyO3, napi-rs and JNI shims; Swift and Dart wrappers | no | D-067 |
| `teigeodata-mcp` | MCP server | no | [ADR-0017](../03-decisions/adr/ADR-0017-mcp.md) |
| `teigeodata-server` | HTTP server | no | [ADR-0025](../03-decisions/adr/ADR-0025-hosted-ready.md) |
| `teigeodata-cli` (`tgd`) | build, extract, inspect, verify, diff, merge, tiles, export | no | |
| `xtask` | every repository task and gate, including the IDL extractor and generators | no | [ADR-0021](../03-decisions/adr/ADR-0021-rust-tooling.md) |
| `forge/*` (own workspace) | the data pipeline and source adapters | no | [ADR-0001](../03-decisions/adr/ADR-0001-rust-core.md) |

Crates are created when their phase starts, not before. Two exceptions started in Phase 0:
- `teigeodata-names`, with the fold and the phonetic keys from spike S3, because the forge's
  matcher must compare names exactly as search does (D-057);
- the binding slice from spike S4 (`teigeodata-api`, `teigeodata-blob`, `teigeodata-ffi`,
  `bindings/`), which the façade grows from in Phase 1 (D-066).

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

`region-index.md`, `search.md`, `timezone.md`,
`offline-loading.md`, `mcp.md`, `security.md`, `performance.md`: each written when
its research note lands.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Binding crates from spike S4; the IDL is extracted from the safe API, not the C ABI; no `teigeodata-idl` crate (it lives in `xtask`) | [S4](../02-research/benches/s4-bindings.md), D-066 |
| 2026-11-06 | First draft | Project start |
| 2026-11-06 | `teigeodata-names` started early | One name code path for the forge and search (D-057) |
| 2026-11-07 | `teigeodata-core` and `teigeodata-format` created (Phase 1) | [core and format](../02-research/notes/core-and-format.md) |
| 2026-11-09 | `teigeodata-engine` created (step 4.1); [engine](engine.md) page | D-113, D-114 |
