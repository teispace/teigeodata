---
title: "Decision log"
status: living
updated: 2026-11-06
---

# Decision log

Every decision, newest last. Architecture-shaping decisions link their ADR ([conventions](../CONVENTIONS.md) §5).

## ADRs

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](adr/ADR-0001-rust-core.md) | Rust core, one workspace | accepted |
| [ADR-0002](adr/ADR-0002-licences.md) | Licences: Apache-2.0 code, ODbL main data, CC-BY lite data | accepted |
| [ADR-0003](adr/ADR-0003-v1-is-everything.md) | v1 is the whole product; coverage grows by data releases | accepted |
| [ADR-0004](adr/ADR-0004-monorepo.md) | One public monorepo: teispace/teigeodata | accepted |
| [ADR-0005](adr/ADR-0005-standalone.md) | teigeodata stands alone | accepted |
| [ADR-0006](adr/ADR-0006-perspective.md) | Nepal perspective for disputed boundaries; perspectives as data | accepted |
| [ADR-0007](adr/ADR-0007-accuracy-model.md) | Accuracy model: consensus position and elevation with uncertainty | accepted |
| [ADR-0008](adr/ADR-0008-stable-ids.md) | Stable place identifiers with redirects and tombstones | accepted |
| [ADR-0009](adr/ADR-0009-pack-format.md) | The .tgd pack format | proposed |
| [ADR-0010](adr/ADR-0010-addressing-and-baseline.md) | Deterministic pack addressing and an embedded baseline | accepted |
| [ADR-0011](adr/ADR-0011-layered-engine.md) | Layered packs merged live | accepted |
| [ADR-0012](adr/ADR-0012-offline-loading.md) | Offline-first loading: catalog, downloads, storage | accepted |
| [ADR-0013](adr/ADR-0013-region-index.md) | One exact region index for timezone, admin and perspective lookup | accepted |
| [ADR-0014](adr/ADR-0014-names-and-search.md) | Names: English plus local; canonical spelling by lexicon; cross-script search | accepted |
| [ADR-0015](adr/ADR-0015-tree-shaking.md) | Tree-shakable everywhere | accepted |
| [ADR-0016](adr/ADR-0016-bindings.md) | Bindings: designed C ABI, our own IDL and generators, parity gate | proposed |
| [ADR-0017](adr/ADR-0017-mcp.md) | MCP server from day 0 | accepted |
| [ADR-0018](adr/ADR-0018-quality-bar.md) | The quality bar is gated | accepted |
| [ADR-0019](adr/ADR-0019-ci.md) | CI on GitHub Actions, including sharded data builds | accepted |
| [ADR-0020](adr/ADR-0020-research-loop.md) | Research, compare, then build, and keep improving | accepted |
| [ADR-0021](adr/ADR-0021-rust-tooling.md) | Repository tooling in Rust through xtask | accepted |
| [ADR-0022](adr/ADR-0022-timezone-and-postal.md) | Exact IANA timezone and postal codes with validity periods | accepted |
| [ADR-0023](adr/ADR-0023-provenance.md) | Provenance for every field | accepted |
| [ADR-0024](adr/ADR-0024-docs-site.md) | Docs site with Fumadocs | accepted |
| [ADR-0025](adr/ADR-0025-hosted-ready.md) | Ready for a hosted tier, without one now | accepted |
| [ADR-0026](adr/ADR-0026-governance.md) | Open-source governance | accepted |
| [ADR-0027](adr/ADR-0027-jni-for-jvm-and-android.md) | JVM and Android share one generated JNI layer | proposed |
| [ADR-0028](adr/ADR-0028-mcp-baseline.md) | MCP baseline: revision 2026-07-28 on rmcp | accepted |
| [ADR-0029](adr/ADR-0029-coordinate-grid.md) | The canonical coordinate grid is 1e-7 degrees | accepted |
| [ADR-0030](adr/ADR-0030-source-admission.md) | Source admission rules per build | accepted |
| [ADR-0031](adr/ADR-0031-canonical-spelling.md) | Canonical Latin spelling: curated precedence and a per-country lexicon | accepted |
| [ADR-0032](adr/ADR-0032-text-search.md) | Text normalisation and the search index | accepted |
| [ADR-0033](adr/ADR-0033-timezone-data.md) | Timezone data: boundary-builder with oceans, and our own tzdb bundle | accepted |
| [ADR-0034](adr/ADR-0034-elevation-method.md) | Elevation method: position first, interval QA, a Nepal validation | accepted |
| [ADR-0035](adr/ADR-0035-forge-architecture.md) | Forge architecture: a content-addressed DAG over Parquet | accepted |
| [ADR-0036](adr/ADR-0036-corrections-as-data.md) | Corrections as reviewed data with guards | accepted |
| [ADR-0037](adr/ADR-0037-keys-and-hosting.md) | Signing keys and metadata hosting | accepted |
| [ADR-0038](adr/ADR-0038-compatibility-outputs.md) | Compatibility outputs: Nominatim-style JSON and GeoJSON | accepted |

## Decisions

| # | Date | Decision | Home |
|---|---|---|---|
| D-001 | 2026-11-06 | Project name `teigeodata`, under the GitHub org `teispace`; names checked free on crates.io, npm, PyPI, pub.dev and NuGet | [ADR-0004](adr/ADR-0004-monorepo.md) |
| D-002 | 2026-11-06 | Accuracy order: coordinates and elevation, then names, then population; optional fields may be null | [ADR-0007](adr/ADR-0007-accuracy-model.md) |
| D-003 | 2026-11-06 | P90 vertical error of 50 m is acceptable for v1, to be tightened later | [ADR-0007](adr/ADR-0007-accuracy-model.md) |
| D-004 | 2026-11-06 | Store boundaries (polygons) for map highlighting, at several levels of detail | [ADR-0009](adr/ADR-0009-pack-format.md) |
| D-005 | 2026-11-06 | Names: English/Latin plus local official plus sourced endonyms; no full world-language set | [ADR-0014](adr/ADR-0014-names-and-search.md) |
| D-006 | 2026-11-06 | Main data ODbL, lite data CC-BY 4.0 (clean-room, per country by licence gate) | [ADR-0002](adr/ADR-0002-licences.md) |
| D-007 | 2026-11-06 | Nepal perspective by default | [ADR-0006](adr/ADR-0006-perspective.md) |
| D-008 | 2026-11-06 | Everything ships in v1; countries come as data releases | [ADR-0003](adr/ADR-0003-v1-is-everything.md) |
| D-009 | 2026-11-06 | MCP from day 0 at the latest revision | [ADR-0017](adr/ADR-0017-mcp.md) |
| D-010 | 2026-11-06 | Rollout: Nepal, India, South Asia, Asia, USA, rest of the world | [ADR-0003](adr/ADR-0003-v1-is-everything.md) |
| D-011 | 2026-11-06 | Fumadocs docs site; docs and agent system like the maintainer's other projects | [ADR-0024](adr/ADR-0024-docs-site.md) |
| D-012 | 2026-11-06 | Exact IANA timezone lookup built in | [ADR-0022](adr/ADR-0022-timezone-and-postal.md) |
| D-013 | 2026-11-06 | Postal codes current and former, with validity periods | [ADR-0022](adr/ADR-0022-timezone-and-postal.md) |
| D-014 | 2026-11-06 | Tree-shakable code, data and builds | [ADR-0015](adr/ADR-0015-tree-shaking.md) |
| D-015 | 2026-11-06 | One public monorepo `teispace/teigeodata` | [ADR-0004](adr/ADR-0004-monorepo.md) |
| D-016 | 2026-11-06 | No hosted API or MCP now; hosted-ready design for a possible paid tier | [ADR-0025](adr/ADR-0025-hosted-ready.md) |
| D-017 | 2026-11-06 | teigeodata stands alone, with no dependency on other Teispace projects | [ADR-0005](adr/ADR-0005-standalone.md) |
| D-018 | 2026-11-06 | CI and sharded data builds on GitHub Actions (public repo); local builds for debugging | [ADR-0019](adr/ADR-0019-ci.md) |
| D-019 | 2026-11-06 | Research and comparison before every implementation; continuous plan improvement | [ADR-0020](adr/ADR-0020-research-loop.md) |
| D-020 | 2026-11-06 | Deterministic pack addressing and an embedded baseline (Swiss Ephemeris model) | [ADR-0010](adr/ADR-0010-addressing-and-baseline.md) |
| D-021 | 2026-11-06 | Offline-first dynamic loading with catalog, downloads and storage | [ADR-0012](adr/ADR-0012-offline-loading.md) |
| D-022 | 2026-11-06 | Up to 5–6 parallel agents, at most 1–2 heavy; regular cleanup | [AGENTS.md §7](../../AGENTS.md) |
| D-023 | 2026-11-06 | Code licence Apache-2.0; Contributor Covenant 3.0; DCO | [ADR-0026](adr/ADR-0026-governance.md) |
| D-024 | 2026-11-06 | Node and WASM bindings are generated napi-rs and wasm-bindgen shims; Python is generated PyO3; other targets use the C ABI | [ADR-0016](adr/ADR-0016-bindings.md) |
| D-025 | 2026-11-06 | Swift ships through SwiftPM only; CocoaPods trunk goes read-only 2026-12-02 | [ADR-0016](adr/ADR-0016-bindings.md) |
| D-026 | 2026-11-06 | MCP baseline 2026-07-28 on rmcp, fallback 2025-11-25 | [ADR-0028](adr/ADR-0028-mcp-baseline.md) |
| D-027 | 2026-11-06 | Exactness is defined against the source quantised once to 1e-7° (i32 grid) | [ADR-0029](adr/ADR-0029-coordinate-grid.md) |
| D-028 | 2026-11-06 | The pack reader is our own small crate; heavy crates only in the builder or as test oracles; codecs adopted only after their A/B benchmark | [ADR-0009](adr/ADR-0009-pack-format.md) |
| D-029 | 2026-11-06 | Format freeze requires all 14 benchmarks of the format research recorded | [ADR-0009](adr/ADR-0009-pack-format.md) |
| D-030 | 2026-11-06 | Main and lite both refuse CC BY-SA and non-commercial sources; geoBoundaries admitted per layer | [ADR-0030](adr/ADR-0030-source-admission.md) |
| D-031 | 2026-11-06 | Lite elevation uses NASADEM (CC0) until Copernicus terms are cleared for lite; main uses Copernicus with its notice | [ADR-0030](adr/ADR-0030-source-admission.md) |
| D-032 | 2026-11-06 | Servers attach ODbL attribution to responses from main data (bulk responses may be substantial extracts) | [ADR-0030](adr/ADR-0030-source-admission.md) |
| D-033 | 2026-11-06 | Canonical spelling by curated precedence; losing spellings kept as alt names; per-country lexicon (Chowk, Gaun…) | [ADR-0031](adr/ADR-0031-canonical-spelling.md) |
| D-034 | 2026-11-06 | Search: FST per normalised form + Levenshtein + our phonetic key; golden set of ≥500 queries per country as a release gate | [ADR-0032](adr/ADR-0032-text-search.md) |
| D-035 | 2026-11-06 | Pack rows are stored in Hilbert order; point positions on a 1e-6° column grid, geometry on 1e-7° (spike S1) | [S1](../02-research/benches/s1-codecs.md) |
| D-036 | 2026-11-06 | Nepal admin skeleton: OCHA COD-AB v02 (CC BY-IGO, from Survey Department data) for provinces, districts and the 753 local levels; CBS codes are the national key (e.g. Kathmandu Metropolitan City 30608) | [Nepal sources](../02-research/notes/nepal-sources.md) |
| D-037 | 2026-11-06 | Nepal wards in main from OSM admin_level=9 (6,744 relations); lite ships wards as codes without geometry | [Nepal sources](../02-research/notes/nepal-sources.md) |
| D-038 | 2026-11-06 | Nepal perspective in main: the OSM Nepal relation joined with the disputed relation 13559363 (claimed_by=NP); COD-AB geometry to be checked for the 2020 claim area | [ADR-0006](adr/ADR-0006-perspective.md) |
| D-039 | 2026-11-06 | Kosi (CC BY-NC-SA), nepalboundariespy (unknown data source) and geoBoundaries NPL ADM2 (2006 districts) are excluded | [ADR-0030](adr/ADR-0030-source-admission.md) |
| D-040 | 2026-11-06 | Region index stores original source edges per cell with a crossing count from the cell-centre reference point; exact i128 predicates on the 1e-7° grid; straight lon/lat edges | [ADR-0013](adr/ADR-0013-region-index.md) |
| D-041 | 2026-11-06 | Region index proven exact (spike S2: 0 in-domain mismatches on 40,000 points incl. 1 cm from borders). Rules: reference points strictly inside their cell and never on an edge; covering zones are a set; lookups return every containing zone; out-of-domain queries are rejected | [S2](../02-research/benches/s2-region-index.md) |
| D-042 | 2026-11-06 | Benchmarks: one workload definition run by every harness and binding; gungraun instruction counts gate PRs, criterion wall-clock runs nightly, tango-bench for research A/B; own JSON results and generated BENCHMARKS.md; CodSpeed and Bencher evaluated in Phase 1 | [Benchmarks](../06-engineering/benchmarks.md) |
| D-043 | 2026-11-06 | The forge is our own content-addressed DAG over Parquet/GeoArrow, sharded by country; DuckDB for QA only; inputs mirrored by SHA-256 | [ADR-0035](adr/ADR-0035-forge-architecture.md) |
| D-044 | 2026-11-06 | Corrections are reviewed YAML with guards; each build applies, retires or flags each one as stale | [ADR-0036](adr/ADR-0036-corrections-as-data.md) |
| D-045 | 2026-11-06 | Catalog trust is a TUF subset with our own client (2-of-3 offline root, 14-day timestamp); metadata on GitHub Pages, packs on immutable releases | [ADR-0037](adr/ADR-0037-keys-and-hosting.md) |
| D-046 | 2026-11-06 | Q-006 answered: GitHub Releases is enough for native apps; browser apps need a CORS mirror (provider pending an account, Q-014) | [ADR-0037](adr/ADR-0037-keys-and-hosting.md) |
| D-047 | 2026-11-06 | IDs follow WOF-style supersedes/superseded_by plus a registry, changelog and bridge files; split and merge always traceable | [ADR-0008](adr/ADR-0008-stable-ids.md) |
| D-048 | 2026-11-06 | The HTTP server and CLI offer native, Nominatim-compatible JSON and GeoJSON outputs | [ADR-0038](adr/ADR-0038-compatibility-outputs.md) |
| D-049 | 2026-11-06 | Positioning: "the open, offline gazetteer you are allowed to keep"; rivals are compared on measured pages only | [Competitive analysis](../02-research/notes/competitive-analysis.md) |
| D-050 | 2026-11-06 | Standing instruction: work autonomously without pauses; push regularly; keep main the single branch | [AGENTS.md §11](../../AGENTS.md) |
| D-051 | 2026-11-06 | Everything is world-general: schema, algorithms, checks and tools work for every country and script; country specifics are per-country data (profiles, lexicons), never code (principle P16) | [principles](../00-vision/principles.md) |
| D-052 | 2026-11-06 | Search design from spike S3: three FST maps (exact, loose, phonetic); own anchored Damerau automaton with an unanchored fallback; prefix; one additive score (match bonus + importance) for every country; phonetic key at low weight. Meets the recall gates on Nepal (recall@1 0.953, recall@5 0.986, 3.6 MB); p99 78–116 µs depending on load, so the automaton is optimised in Phase 2 | [S3](../02-research/benches/s3-search.md), [ADR-0032](adr/ADR-0032-text-search.md) |
| D-053 | 2026-11-06 | Every input is pinned in `data/sources.lock` (URL, version, SHA-256, size, licence class) and validated by `check-data`. The forge workspace exists: `fetch` keeps only matching bytes in a content store; readers for OSM, GeoNames and Wikidata write deterministic Parquet record tables. Wikidata is queried per country from one world-general template | [forge](../04-architecture/forge.md) |
| D-054 | 2026-11-06 | Q-010 answered: the COD-AB Nepal outline (valid 2024-03-14) follows the pre-2020 line and **excludes** the Limpiyadhura claim. OSM places Kuti, Gunji, Nabi, Garbyāng, Rongkang and Nabhidang fall outside it; Changru, Tinkar and Kalapani fall inside. Main keeps the OSM relation joined with relation 13559363 for admin0 (D-038); COD-AB serves levels 1–3, and places in the claim area get their admin units from the claim polygon. Lite: Q-015 | [forge](../04-architecture/forge.md#point-in-polygon-pip) |
| D-055 | 2026-11-06 | Geofix priors from measured rounding: a point's σ per axis is at least step/√12 of its detected precision (32 m at 3 decimals, 321 m at 2, 535 m in latitude on the 1′ grid), and the points of one stack in one source count as one observation. S5 found Wikidata the most rounded (35% at ≤ 3 decimals, 23% on the 1″ grid, 8% stacked), `GeoNames` NP a quarter at 4 decimals, OSM almost unrounded | [S5](../02-research/benches/s5-nepal.md#rounding-stacking-and-centroid-snap-s54) |
| D-056 | 2026-11-06 | Source dependence and stated links: two points of different sources within 1 m are one copied observation (31% of linked `GeoNames`–Wikidata pairs are copies; OSM is independent). A stated `same_as` link is evidence for the matcher, not truth: links beyond the class's distance gate are cut and listed for upstream fixing. Village positions disagree by P50 360–1,100 m and P90 2–5 km between independent sources, so the settlement core (buildings) is the lever for Q-005 | [S5](../02-research/benches/s5-nepal.md#stated-links-and-position-disagreement-s55-first-part) |
| D-057 | 2026-11-06 | `teigeodata-names` starts in Phase 0 with the S3 fold and the phonetic keys, `no_std` + `alloc`, lint-clean and built for a bare-metal target in CI. The S3 spike, the forge and `xtask` use it: one name code path. Proven identical (golden set SHA-256 unchanged; S3 recall@1 0.953, pkey 0.789 / 0.00010). A profile names its key (`phonetic_key: indic-v0`), validated by `check-data` | [overview](../04-architecture/overview.md) |
| D-058 | 2026-11-07 | Entity model: a populated place and an admin unit are never merged (a conflation cannot-link), even when they share a name and a place. The unit carries a `seat` link to its populated place (OSM `admin_centre` role, Wikidata P36), and `admin_centre` is that place's point | [schema](../05-data/schema.md), [S5](../02-research/benches/s5-nepal.md#the-matcher-s55-second-part) |
| D-059 | 2026-11-07 | The matcher as built in S5. Blocking is on a join or phonetic key within neighbouring 0.1° cells, plus stated links. Fellegi–Sunter: u from 1e6 seeded random pairs; m from EM sessions, each conditioned on its blocking field; a global λ. Union-find runs by descending weight under source and kind-family cannot-links. Without the ID field it gives recall 0.83–0.88 at precision 0.991–1.000 against the stated links. Term frequencies and the checked-in m/u table follow in Phase 2 | [S5](../02-research/benches/s5-nepal.md#the-matcher-s55-second-part) |
| D-060 | 2026-11-07 | Names from other cluster members reach a record through the `aliases` stage only when the member is corroborated (it shares a name) and the alias does not collide (by fold or phonetic key) with the primary name of an admin unit anywhere or of another record within 30 km. With them S3 improves to recall@1 0.959, recall@5 0.994, pass 0.988, with no class worse. Golden queries are never generated from forge outputs, so the evaluation stays independent | [S5](../02-research/benches/s5-nepal.md#aliases-into-search-s55-third-part) |
| D-061 | 2026-11-07 | DEM tiles are pinned as a tileset: the lock pins a manifest (`data/sources/<id>.tiles`: tile, SHA-256, size, or `absent`), and the manifest pins every tile, checked by `check-data` and `forge fetch --verify`. Copernicus GLO-30 is decoded with `tiff` 0.11.3, bit-identical to libtiff (B3). Elevation is sampled from the DEM at the conflated point; stated elevations are a QA check only (P90 100–430 m off against Copernicus, while `GeoNames`' SRTM3 field agrees at P90 32 m) | [S5](../02-research/benches/s5-nepal.md#dem-disagreement-s56) |
| D-062 | 2026-11-07 | Q-005 answered: horizontal targets follow from the vertical target (P90 ≤ 50 m), since the vertical error is nearly all position error times the terrain gradient (P90 27 m/km below 5°, 246–563 m/km above, measured in S5). A populated place is P90 within 500 m of its settlement core below 5° of slope and within 150 m at 5° or steeper; an admin unit's point is inside its own polygon, always. Measured per country and class against held-out ground truth in `ACCURACY.md` | [S5](../02-research/benches/s5-nepal.md#summary-accuracy-targets-s57-q-005) |
| D-063 | 2026-11-07 | The elevation reference is ICESat-2 ATL08 through SlideRule's public `atl08x` endpoint (no account), pinned as a POST tileset. A response with a server error is retried and never pinned; a beam reported unreadable is accepted only when `h5p` confirms its granule has no `land_segments` for it. Heights become EGM2008 with GeographicLib's 5′ grid (bilinear, ≤ 0.48 m). Result: Copernicus LE90 18.7 m inside Nepal (497,717 segments), 32.6 m in the worst stratum, so the DEM alone meets the vertical target and position spends the rest of the budget | [S6](../02-research/benches/s6-elevation.md), [note](../02-research/notes/elevation-reference.md) |
