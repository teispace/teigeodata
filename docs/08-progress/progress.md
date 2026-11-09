---
title: "Progress"
status: living
updated: 2026-11-09
---

# Progress

## Where we are

A short status. The numbers and evidence live in each phase file and the
[decision log](../03-decisions/log.md); this page links to them instead of repeating them.

| Phase | State | Evidence |
|---|---|---|
| 0 Foundations and research | **M0 met** 2026-11-07 (D-069) | [phase 0 §0.5](../07-planning/phases/phase-0.md#05-exit-review) |
| 1 Core and format | **M1 met** 2026-11-07 (D-080) | [phase 1 §1.6](../07-planning/phases/phase-1.md#16-exit-review) |
| 2 Indexes | **M2 met** 2026-11-07 (D-087) | [phase 2 §2.4](../07-planning/phases/phase-2.md#24-exit-review) |
| 3 Forge and Nepal data | in progress: 3.1–3.3 and 3.5 done; 3.4 DEM and 3.6 names in progress; 3.7–3.8 open | [phase 3](../07-planning/phases/phase-3.md) |
| 4 Engine, baseline and sync | started in parallel with Phase 3: 4.1 engine and 4.4 transport | [phase 4](../07-planning/phases/phase-4.md) |
| 5–8 | not started (bindings prototyped in S4, D-065 to D-068) | [roadmap](../07-planning/roadmap.md) |

- **Now (Phase 3):** the Nepal main pack has 101,550 places with heights (D-111), admin
  records linked (D-110), same-place merges (D-116) and the first corrections (D-117); golden
  pass 0.879 on the built pack (`pack-eval`). Next: rank a town over its same-name twins, then
  3.7 (gates in CI, attribution, lite, `ACCURACY.md`).
- **In parallel (Phase 4):** the engine (4.1, D-113, D-114; Nepal timings in
  [phase-4 benches](../02-research/benches/phase-4.md#nepal-main-pack)) and the transport
  (4.4, D-112) are on `main`; the product's search is at parity with the spike (D-121:
  recall@1 0.797, p50 6.7 µs). Next: radius and cross-pack hierarchy, snapshot swap, TUF trust.
- **Size ideas to measure:** the timezone REGION (17% of the main pack) and the finest admin
  arcs ([ideas](../01-product/ideas.md)).
- **Waiting on the maintainer:** review of `data/countries/np/corrections/` (unreviewed), the labelled sample (Q-005, needed for `ACCURACY.md` and M3),
  the postal licence (Q-009), Copernicus for lite (Q-008).

## Session log

| Date | What happened | Next step |
|---|---|---|
| 2026-11-06 | Discussion and decisions (see the [decision log](../03-decisions/log.md)). Repository initialised; AGENTS.md, conventions, vision, principles, scope, 26 ADRs, roadmap, Phase 0 plan. Research agents launched: Nepal sources, source licences, format/compression/indexing, bindings and MCP, names and search, timezone and elevation. | Finish 0.1; collect the research notes |
| 2026-11-06 | Foundation files, workspace, xtask gates and hooks; bindings and format research applied; ADR-0027 (JNI), ADR-0028 (MCP baseline), ADR-0029 (1e-7° grid) | Commit, create repo, apply the other notes |
| 2026-11-06 | Spike S2 made exact (3 bugs found by set-based differential testing; source overlaps found, Q-013); leaf-size sweep; forge, offline-loading and competitive notes landed; benchmarking research and catalogue (D-042) | Push, apply the new notes, then S3 |
| 2026-11-06 | Lexicon and data checks; per-country folders and profiles (P16); world-general search fold; golden set v0 for Nepal (reproducible) | S3 search spike |
| 2026-11-06 | Spike S3: recall gates met on Nepal, p99 depends on load (D-052); own Damerau automaton about 10× faster than the `fst` DFA; phonetic key at low weight; pkey pair sets; COD-AB join bug fixed and golden set regenerated (512 queries) | S5 baseline accuracy |
| 2026-11-07 | S5 done: matcher, aliases into search (S3 recall@1 0.959), DEM tilesets and `forge dem` (S5.6), the summary and accuracy targets (S5.7, D-058 to D-062) | S6 elevation validation |
| 2026-11-07 | S6.1–S6.2: elevation-reference research; POST tilesets with response checks; SlideRule decoder; EGM2008 geoid; `forge dem-ref`: Copernicus LE90 18.7 m in Nepal (D-063) | S6.3 GEDTM30 |
| 2026-11-07 | S6.3: NASADEM via Planetary Computer tokens and EGM96; paired comparison with Copernicus (D-064); GEDTM30 circularity recorded (Q-016) | S4 bindings |
| 2026-11-07 | S4: safe API crate, syn IDL, generated C ABI, header, Swift, Dart, Python, Node and JVM layers; parity in all; benchmarks; cbindgen and lint-table gates; `dist` unwinds (D-065 to D-068); ASCII fold fast path | M0 review |
| 2026-11-07 | CI's binding suites green on Linux (Node test path fixed); M0 review (D-069); Phase 1 research note and plan: crate split, TLV manifest, stable ID text form, bolero, REDIRECTS (D-070, D-071) | 1.1 `teigeodata-core` |
| 2026-11-07 | 1.1 `teigeodata-core`; 1.2 `teigeodata-format` (format 0.1): reader, writer, signatures, fuzz properties; a writer bug found by the round-trip harness; bolero dropped (D-072) | 1.3 benchmarks |
| 2026-11-07 | 1.3 benchmarks (L1–L3, core); B1 integer codec and B13: 128-value blocks, safe decoder 8.5–11 G/s, beats `bitpacking`; 1024 layout dropped (D-073) | B1 sections, then B5 |
| 2026-11-07 | B1 on sections: column table, PLACES, HIERARCHY, `PlaceKind`; WASM decode 4 G/s; all B1 targets met (D-074) | B5 IDMAP and REDIRECTS |
| 2026-11-07 | B5: research note, IDMAP and REDIRECTS, ramp coding, interpolated search; PtrHash compared out of tree (D-075) | B6 rank and select |
| 2026-11-07 | B6 COMPONENTS, B3 NAMES, B12 BLOCK_HASHES, B4 BLOBS (D-076 to D-079); 1.5 nightly fuzz and Miri, the counts gate, Z2 sizes; M1 met (D-080) | Phase 2 research |
| 2026-11-07 | Phase 2: indexes note (D-081); B8 PLACE_INDEX (D-082); B9 REGION, Q-013 answered (D-083, D-084) | B7 SEARCH |
| 2026-11-07 | B7 SEARCH: own FST, bit-parallel NFA on code points, FOR lists by importance, block maxima; 1.50 MB, p99 41.2 µs unanchored (D-085, D-086); M2 met, POSTAL to Phase 3 (D-087) | Phase 3 research |
| 2026-11-07 | Phase 3 research: geometry (D-088), DEM (D-089), postal and Q-001 (D-090), settlement core and building licences (D-091); plan and order (D-092) | 3.1 vertical slice |
| 2026-11-07 | 3.1: `forge build` writes, signs and reads back the Nepal pack (96,844 places, 7.05 MB, 3.0 s, deterministic); lite 4.26 MB; `interior_point` for admin points; REGION place link (D-093) | Stage cache, then 3.2 |
| 2026-11-07 | 3.1: the ID registry (D-094): lite keeps 91,153 of 91,266 main IDs, a rebuild keeps all, retirement writes REDIRECTS | Stage cache, then 3.2 |
| 2026-11-07 | 3.1 done: `forge make` and `build.yaml`, content-keyed stages with stamped outputs (D-095) | 3.2: building readers, settlement core, consensus |
| 2026-11-08 | 3.2: building readers, `forge core` with a fatal agreement gate (P90 146 m) proved on a planted mismatch, the border rule, `forge labels` and the 301-place sample (D-096) | 3.3: B10 geometry |
| 2026-11-08 | 3.3: ARCS (a518086), POLYGONS (07cbe5b), topology stage and pack geometry (faebfbc); D-097, D-098 | POLY_INDEX, admin REGION A/B |
| 2026-11-08 | 3.3: POLY_INDEX on the shared tree, in the packs; B10 page (D-099) | admin REGION A/B, coarse LOD reads |
| 2026-11-08 | 3.3: ADMIN_REGION on the shared arcs, cross-checked in the forge (D-100) | per-pack top grid, coarse LOD reads |
| 2026-11-08 | 3.3: fine grid (D-101), ARCS_OVERVIEW (D-102) | the India measurement |
| 2026-11-08 | 3.3 done: snap rounding in the topology stage (D-103), `forge topology --geoboundaries`, India measured | 3.4: B11 DEM |
| 2026-11-08 | 3.4: the DEM section and `forge dem-section` (D-104) | the 45-tile download (approval) |
| 2026-11-09 | 3.5: POSTAL on place runs, Nepal codes in the main pack (D-105); CI fixes for the `no_std` build and a debug overflow | 3.6 names and corrections |
| 2026-11-09 | 3.6: display names, the romaniser and its Nepali profile (D-106) | D-060 alias rules in the build; corrections |
| 2026-11-09 | 3.6: D-060's alias rules in the build (D-107); stage keys by code and a `fast` profile (D-108); DEM tiles fetched | pack evaluator; duplicate clusters; corrections |
| 2026-11-09 | 3.4: place elevations from the DEM at the consensus point, the `dem` stage in `forge make` (D-111) | the DEM component in the packs; lite elevation |
| 2026-11-09 | D-110 admin records, D-111 place elevations merged; development speed-ups (D-115): Phase 4 started in parallel (engine, transport), build split into modules, Miri under nextest, this page shortened | Duplicate clusters and G-conf; merge the Phase 4 branches |
