---
title: "ADR-0035: Forge architecture: a content-addressed DAG over Parquet"
status: accepted
updated: 2026-11-06
---

# ADR-0035: Forge architecture: a content-addressed DAG over Parquet

Status: accepted, 2026-11-06. Refines [ADR-0019](ADR-0019-ci.md) and [ADR-0023](ADR-0023-provenance.md).

## Context

The data build (the forge) turns pinned sources into packs. It must be reproducible byte for byte, licence-gated at ingest, resumable, sharded across CI jobs under 6 h and 14 GB, and small enough for contributors to run one country on a laptop.

## Decision

- **Our own Rust DAG of pure stages.** Each stage is `(inputs by hash, stage version, config) → outputs`. Its outputs are keyed by a BLAKE3 hash of all three and reused when the key is unchanged. No Bazel, Nix, DVC or Dagster.
- **Deterministic by construction:**
  - integer coordinates on the 1e-7° grid ([ADR-0029](ADR-0029-coordinate-grid.md));
  - a total order on every output;
  - no `RandomState`;
  - fixed-shape parallel reductions and recorded seeds.
- **Intermediates are Parquet with GeoArrow geometry.** Work is sharded by country, then by tile with a halo inside large countries.
- **Readers:**
  - OSM: `osmpbf`, with our own port of libosmium's area assembly (osmium-tool is a test oracle only).
  - Overture: `parquet`/`arrow` with `geoarrow`, using HTTP range reads.
  - DEM: `tiff`, by tile window, checked against GDAL.
  - Wikidata: a pinned, mirrored per-country SPARQL snapshot.
  - GeoNames: TSV.
- **DuckDB is for analysis and QA only**, never in the build path. Its output order and float aggregates are not deterministic, and its spatial extension downloads at runtime.
- **Conflation:**
  - a Fellegi–Sunter model in Rust (Splink-style; Splink is an oracle outside the build);
  - candidate pairs from spatial cells × a name key, plus shared IDs;
  - union-find clustering that refuses cannot-links;
  - ambiguous clusters go to a QA queue.
- **Position and boundaries:**
  - consensus point: a weighted Weiszfeld geometric median;
  - rounding detection tested against a uniform baseline;
  - the building-density peak used as an extra source;
  - polylabel for areas;
  - our own arc topology on the integer grid (`i_overlay` for noding), simplified per arc for each level of detail.
- **Licence gate at ingest:** the lite DAG cannot schedule a reader whose source is refused in lite ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md) rule 2).
- **Inputs are mirrored** to our own release assets by SHA-256. `data/sources.lock` records `upstream_url` and `mirror_url`, and for Wikidata the SPARQL text with its hash. Moving `latest` URLs are never fetched in a build.
- **CI:**
  - the job matrix comes from `forge/manifest.json`;
  - workflow artifacts carry intermediates within a run;
  - the cache carries finished shards across runs, keyed by content hash;
  - a final job assembles the packs, runs the gates, writes and signs.

## Consequences

New data gates in [the quality bar](../../06-engineering/quality-bar.md):

| Gate | What it checks |
|---|---|
| Determinism | A double build gives identical bytes |
| Licence at ingest | Lite never schedules a refused reader |
| Topology | Zero gaps, overlaps and self-intersections at every level of detail |
| Point in polygon | Every place lies in exactly one region per covered admin level |
| Conflation | Precision and recall on labelled pairs |
| Corrections | No stale corrections left unreviewed |
| Budget | Each stage stays within its disk and memory budget |
| Time | Every CI shard runs at least 30 % under 6 h |

- Spike S5 is built in `forge/` with the real stage boundaries.
- The forge benchmarks FB1–FB4 are listed in [benchmarks](../../06-engineering/benchmarks.md).

## Alternatives considered

- Bazel or Nix: heavy for contributors, and they do not understand data shards.
- DuckDB as the engine: not deterministic, and it downloads at runtime.
- osmium as a build dependency: a C++ toolchain in every build.

## Evidence

[Data pipeline (forge)](../../02-research/notes/forge-pipeline.md), 2026-11-06.
