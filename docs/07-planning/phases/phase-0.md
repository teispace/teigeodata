---
title: "Phase 0: foundations and research"
status: living
updated: 2026-11-06
---

# Phase 0: foundations and research

**Goal:** everything the build phases need is researched, measured and written down, and the
repository has its working machinery. Exit: milestone **M0** ([roadmap](../roadmap.md)).

## Steps

### 0.1 Repository foundation
- [x] `git init`; `AGENTS.md`, `CLAUDE.md`, `docs/CONVENTIONS.md`
- [x] Docs tree, vision, principles, scope
- [x] ADR-0001 to ADR-0026 and the decision log
- [x] Roadmap, this phase file, progress, open questions
- [x] Community files: README, CONTRIBUTING, GOVERNANCE, SECURITY, CLEAN_ROOM, NOTICE, CODEOWNERS, issue and PR templates
- [x] Cargo workspace with the lint wall, `rust-toolchain.toml`, `deny.toml`, formatting and typos config
- [x] `xtask`: `check-docs`, `check-commit`, `clean`, `help`; both checks proven red
- [x] Git hooks: commit message (Conventional Commits, DCO, no AI attribution), pre-commit (fmt, docs)
- [x] GitHub repository `teispace/teigeodata` (public), settings (squash merge only, delete merged branches, private vulnerability reporting), ruleset `protect-main` (no deletion, no force-push; required checks and PRs once contributors arrive), the `fast-check` workflow green (2026-11-06)

### 0.2 Research notes (parallel, light)
- [x] [Nepal sources](../../02-research/notes/nepal-sources.md)
- [x] [Source licences](../../02-research/notes/source-licences.md)
- [x] [Format, compression and indexing](../../02-research/notes/format-compression-indexing.md)
- [x] [Bindings and MCP](../../02-research/notes/bindings-and-mcp.md)
- [x] [Names and search](../../02-research/notes/names-and-search.md)
- [x] [Timezone and elevation](../../02-research/notes/timezone-and-elevation.md)
- [x] [Offline loading and trust](../../02-research/notes/offline-loading-and-trust.md) (catalog, downloads, TUF-style metadata, background downloads per OS)
- [x] [Data pipeline (forge)](../../02-research/notes/forge-pipeline.md): streaming and tiling, conflation (Fellegi–Sunter), building-density centres, reproducibility
- [x] [Competitive analysis](../../02-research/notes/competitive-analysis.md): GeoNames, Who's On First, Pelias, Photon, Nominatim, Overture, commercial geocoders, offline SDKs
- [x] [Benchmarking](../../02-research/notes/benchmarking.md): harnesses, CI method and bindings; catalogue in [benchmarks](../../06-engineering/benchmarks.md)

- [x] Golden query set for Nepal v0: 512 queries (500 generated, seeded and reproducible, from GeoNames, COD-AB and the country folder; 12 curated with sources), checked by `cargo xtask check-data`. Known gap: `colloquial` has 14 sourced queries (target 30); filled from OSM `short_name`/`alt_name` in S5 and curated review ([ADR-0032](../../03-decisions/adr/ADR-0032-text-search.md))
- [x] pkey pair sets for S3: 303 positive pairs from the lexicons, 34.5 million negative pairs (distinct names under one parent), bucket sizes; measured in the S3 run ([results](../../02-research/benches/s3-search.md))
- [x] Lexicon starter `data/countries/np/lexicon.yaml` and `in`, plus country profiles, with schema check ([ADR-0031](../../03-decisions/adr/ADR-0031-canonical-spelling.md)): `cargo xtask check-data` (YAML via `serde-saphyr` 2.1.1; NFC, scripts, variant collisions, evidence); it found four merged India terms, now split

### 0.3 Revise the plan from research
- [x] Each note's "impact on the plan" applied: ADR statuses (`proposed` → `accepted` or superseded), architecture pages, this roadmap (ADR-0035 to ADR-0038; ADR-0013 accepted; ADR-0009 and ADR-0016 stay proposed until format freeze and spike S4)
- [x] [Data schema v1 draft](../../05-data/schema.md) and the [pack format draft](../../04-architecture/pack-format.md) (freeze after B1–B14)

### 0.4 Spikes (measured; heavy lane, one at a time)
- [x] S1 Codecs: coordinates, elevation and names on GeoNames Nepal ([results](../../02-research/benches/s1-codecs.md)); rank/select presence moves to benchmark B6
- [x] S2 Region index: world timezone exactness (differential, zero mismatches) and ns/lookup against tzf-rs ([results](../../02-research/benches/s2-region-index.md)): exact; packed layout and zone precedence (Q-013) go to Phase 2
- [x] S3 Search: FST maps, own Damerau automaton, prefix and the phonetic key on the Nepal golden set ([results](../../02-research/benches/s3-search.md)): recall@1 0.953, recall@5 0.986, MRR@10 0.969, 3.6 MB: recall gates met; p99 78–116 µs, depending on load (100 µs gate: faster automaton in Phase 2). The misses left are data (aliases, conflation) for S5 and the forge
- [ ] S4 Bindings: C ABI + IDL + generator on one slice into Node, Dart, Python, JNI and Swift; marshalling cost
- [x] S5 Baseline accuracy: GeoNames vs OSM vs Wikidata for Nepal (duplicates, rounded coordinates, DEM disagreement); check COD-AB geometry for the 2020 claim (Q-010) ([results so far](../../02-research/benches/s5-nepal.md))
  - [x] S5.1 `data/sources.lock` (8 sources pinned, checked by `check-data`), the forge workspace and `forge fetch` (content store, SHA-256, `--pin`, `--verify`; a wrong pin proven to fail) ([forge](../../04-architecture/forge.md))
  - [x] S5.2 readers for OSM, GeoNames (with alternate names and hierarchy) and Wikidata: deterministic Parquet record tables
  - [x] S5.3 COD-AB boundary reader and the `pip` stage (exact point in polygon); Q-010 answered (D-054)
  - [x] S5.4 geofix detectors: rounding, stacking, centroid snap
  - [x] S5.5 links and conflation: cross-source pairs, position disagreement, the matcher, aliases into search (D-056, D-059, D-060)
  - [x] S5.6 DEM disagreement (Copernicus GLO-30)
  - [x] S5.7 results page: the summary and the accuracy targets (Q-005, D-062)
- [ ] S6 Elevation validation: ICESat-2 ATL08 against Copernicus GLO-30, NASADEM and GEDTM30 over Nepal ([ADR-0034](../../03-decisions/adr/ADR-0034-elevation-method.md)) ([note](../../02-research/notes/elevation-reference.md))
  - [x] S6.1 the reference: ATL08 through SlideRule pinned as a POST tileset, the EGM2008 geoid, `forge dem-ref` (D-063)
  - [x] S6.2 Copernicus GLO-30 against ATL08 over the Nepal box, by slope, land cover, snow, light and height ([results](../../02-research/benches/s6-elevation.md)): LE90 18.7 m inside Nepal
  - [ ] S6.3 GEDTM30 (range reads of its global COG) and NASADEM (needs an Earthdata login or an OpenTopography key: maintainer)

### 0.5 Exit review
- [ ] M0 criteria met; progress updated; maintainer review

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-06 | Phase 0 planned | Project start |
| 2026-11-06 | S3 done; the COD-AB join key ignores spaces (a conflation bug found by S3), golden set regenerated (512 queries) | Spike S3 |
| 2026-11-06 | Country folders (`data/countries/<cc>/`: profile, lexicon, curated golden) and `check-data`; golden set v0 | Maintainer: everything world-general (P16) |
| 2026-11-06 | Benchmarking note and catalogue added; S2 done | Maintainer asked for proper benchmarks across features, cases and bindings |
| 2026-11-07 | S5 done (S5.6 DEM disagreement, S5.7 targets, D-061, D-062); S6 split into S6.1–S6.3 after the elevation-reference research (D-063) | Spike S5; elevation-reference note |
