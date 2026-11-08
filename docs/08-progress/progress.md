---
title: "Progress"
status: living
updated: 2026-11-08
---

# Progress

## Where we are

- **Phase:** 3, forge and Nepal data ([phase file](../07-planning/phases/phase-3.md)); research done in four notes (D-088 to D-091), order in D-092.
- **M2 met** on 2026-11-07 (D-087), evidence in [phase 2 §2.4](../07-planning/phases/phase-2.md#24-exit-review); POSTAL moved to Phase 3.
- **Now:** step 3.1, the vertical slice. `forge build np` writes, signs and reads back the Nepal
  pack: 96,844 places in 7.05 MB in 3.0 s, identical bytes on two builds; lite 91,266 places in
  4.26 MB ([forge build](../04-architecture/forge.md#build-a-pack-build)). The ID registry
  keeps IDs across releases and shares them between main and lite (D-094). `forge make np`
  runs every stage from `build.yaml` with a content-keyed cache: cold 7.8 s, warm 19 ms
  (D-095). **3.1 done.**
- **3.2 built** (D-096): building readers for OSM, Microsoft and Google v3 (47 M Nepal
  buildings in under 10 s), the settlement core and weighted Weiszfeld consensus (88,649 of
  98,020 places with a core; Google-only against Microsoft-only P90 146 m, gate 180 m, fatal),
  the 301-place labelling sample (Q-017, maintainer). `forge make np` cold 30 s; main pack
  7.05 MB, lite 4.48 MB. DEM elevation at the consensus point moved to 3.4.
- **3.3 in progress** (D-097 to D-102): ARCS, POLYGONS and POLY_INDEX sections and the
  forge's topology stage are built; the Nepal packs carry 860 admin polygons on 2,315 shared
  arcs (4.48 B per vertex, 8 levels of detail, parents within 1 cm of COD-AB's) and their index
  (9.5 B per polygon, one tree implementation with PLACE_INDEX) and ADMIN_REGION, REGION on the
  ARCS vertices (D-100: 977 KB against 3.63 MB with its own, same answers and speed). A fine
  grid over each pack's extent (D-101) halves admin queries on places (296 → 153 ns) for
  89 KB, and an overview of the coarse levels (D-102) reads levels 4 to 6 3.3 to 3.5 times
  faster for 95 KB; main 10.99 MB, lite 8.34 MB ([B10](../02-research/benches/phase-3.md#b10-poly_index-and-ring-reads)).
  Next: the India measurement, then 3.4 (DEM).
- **Phase 2 done:** 2.1 B8 PLACE_INDEX, a packed Hilbert R-tree over the PLACES rows with u16
  boxes inside the parent: 0.55 B per place, a city box in 1.2–1.8 µs, the nearest place by
  great-circle distance in 3.5–17 µs up to 10⁷ places, exact against brute force (D-082,
  [results](../02-research/benches/phase-2.md#b8-place_index)).
  2.2 B9 REGION, an exact point-in-region index on a top grid and Morton-sorted leaves with
  shared arcs: the world timezones with oceans in 25.7 MB, 50 ns uniform and 41 ns on Nepal
  places, twice as fast as `tzf-rs` with full data, zero mismatches against even–odd (D-083);
  Q-013 answered by ID order (D-084)
  ([results](../02-research/benches/phase-2.md#b9-region)).
  2.3 B7 SEARCH, one section for the exact, loose and phonetic maps on our own checked FST with
  a bit-parallel fuzzy NFA over code points: 1.50 MB on Nepal against S3's 3.63 MB, one-edit
  walks 6× faster than `fst`, golden recall unchanged, p99 41.2 µs unanchored against S3's
  71.2 µs, native one-letter typos 0.659 → 0.976 (D-085, D-086)
  ([results](../02-research/benches/phase-2.md#b7-search)).
- **M1 met** on 2026-11-07 (D-080), evidence in [phase 1 §1.6](../07-planning/phases/phase-1.md#16-exit-review). Done in Phase 1:
  - 1.1 `teigeodata-core`: the stable ID and its text form, grid points, and a Hilbert index
    4–5× faster than the classic loop and proven equal to it;
  - 1.2 `teigeodata-format`: format 0.1. A canonical, validating zero-copy reader (`no_std`, no
    allocation), a streaming-friendly deterministic writer, Ed25519 signatures, and four fuzz
    properties under `cargo test`, each proven on planted bugs (D-072). The research is in
    [core and format](../02-research/notes/core-and-format.md) (D-070, D-071).
  - 1.3 `teigeodata-bench`: open costs 0.71 µs at any size (head only), eager hashing
    1.76 GiB/s, a signature check 34 µs ([results](../02-research/benches/phase-1.md)).
  - 1.4 so far, B1 and B13: the integer codec decodes 8.5–11 G/s in safe code, 22–44% faster
    than the `unsafe` `bitpacking` crate; delta halves sorted IDs; 128-value blocks chosen (D-073).
    PLACES and HIERARCHY on one shared column table: 5.2 and 3.0 B per place on Nepal, reads
    3–10 ns, CSR children below the Elias–Fano bound (D-074).
    B5: IDMAP and REDIRECTS, sorted IDs with a new ramp coding: 0.35 B per place, 82–91 ns per
    hit; PtrHash is 12× bigger for 3–27 ns and stays the upgrade path (D-075).
    B6: COMPONENTS, presence as a poppy rank bitmap or sorted rows, the smaller per component
    (crossover near 10%); rank 6.9 ns, a field read 7–20 ns (D-076).
    B3: NAMES with FSST, our own decoder and deterministic trainer, per-script tables:
    10.6 B per place on Nepal, a canonical name in 65 ns (D-077).
    B12: BLOCK_HASHES, 16 KiB BLAKE3 chaining values per section of 1 MiB or more, merged into
    the table hash; at 100 MB a proof takes 237 µs and a first read 8.8 µs, against 55.6 ms
    eager (D-078).
    B4: BLOBS, zstd frames of whole blobs (C zstd 19 in the forge, `ruzstd` behind the `zstd`
    feature): 0.19 of raw on Nepal rows, a random blob in 43 µs; the dependency-`unsafe` policy
    is in the quality bar (D-079).
  - 1.5: nightly libFuzzer (8 targets, green) and Miri (four parallel jobs, tests bounded under
    `cfg(miri)`); the `counts` gate fails a PR above +1% instructions, proven on a planted +26%; all eight harnesses run on every PR from `teigeodata_format::fuzz`;
    a determinism gate. Z2 (`cargo xtask sizes`): the whole reader adds 171 KiB to a wasm32
    module, 53 KiB gzipped ([results](../02-research/benches/phase-1.md#z2-library-sizes-per-feature-set)).
- **M0 met** on 2026-11-07 (D-069): each criterion with its evidence in
  [phase 0 §0.5](../07-planning/phases/phase-0.md#05-exit-review). The maintainer's review is
  asynchronous. Phase 0 in short:
  - **Done:** 0.1 to 0.4: foundation, ten research notes applied, schema and pack-format
    drafts, spikes S1 to S6.
  - **World-general (P16):** every country lives in its own folder,
    `data/countries/<cc>/` (profile, lexicon, curated golden queries), validated by
    `cargo xtask check-data`.
  - **Golden set:** Nepal v0 has 512 queries (gap: `colloquial` has 14 of a target 30).
  - **Search (S3):** the recall gates are met on Nepal: recall@1 0.953, recall@5 0.986,
    3.6 MB; p99 78–116 µs, depending on load ([results](../02-research/benches/s3-search.md), D-052). The misses left are aliases
    and conflation, which are S5 and forge work.
- **S5 done:** sources pinned in `data/sources.lock`; the forge fetches them and reads
  OSM, GeoNames and Wikidata into Parquet (S5.1, S5.2); `forge pip` assigns points to COD-AB units
  exactly, and Q-010 is answered: COD-AB excludes the 2020 claim (S5.3, D-054). `forge geofix`
  measures rounding, stacking and snap against measured nulls (S5.4, D-055;
  [results](../02-research/benches/s5-nepal.md)). `forge links` measures disagreement on the
  sources' own links: villages disagree by P50 360–1,100 m; 31% of `GeoNames`–Wikidata pairs
  are copies (D-056). `forge conflate` (Fellegi–Sunter) recovers 83–88% of those links at
  precision 0.991–1.000 without using them (D-059); `teigeodata-names` now exists (D-057).
  `forge aliases` carries cluster names into search: S3 recall@1 0.959, recall@5 0.994
  (D-060). `forge dem` samples Copernicus GLO-30, pinned as a tileset: stated elevations are P90
  100–430 m off, so elevation comes from the DEM at the conflated point (S5.6, D-061). S5 is done:
  Q-005 is answered by the S5.7 summary, with horizontal targets set by terrain slope (D-062).
- **S6 done:** ICESat-2 ATL08 through SlideRule (no account), pinned as a POST tileset with
  response checks; EGM2008 and EGM96 geoids; `forge dem-ref`. Inside Nepal (497,717 segments):
  Copernicus LE90 18.7 m, NASADEM 17.7 m. Copernicus is better except under forest; both meet
  the vertical target alone (D-063, D-064; [results](../02-research/benches/s6-elevation.md)).
  GEDTM30 waits for an independent reference (Q-016).
- **S4 done:** `crates/teigeodata-api` is the single source of every binding; `cargo xtask
  bindings` extracts the IDL with `syn` and generates the C ABI and header, Swift, Dart, PyO3,
  napi-rs and JNI layers. All six languages pass one parity suite; the C ABI adds 0.3 ns per
  call; a new function takes one regeneration. Gates: drift, the header against cbindgen,
  boundary lint tables (D-065 to D-068; [results](../02-research/benches/s4-bindings.md)). The
  name fold got a 10× ASCII fast path on the way.
- **Next:** Phase 3 (M3): the forge builds Nepal's packs end to end with every section,
  carrying the S5 and S6 follow-ups (the settlement core, geofix consensus, land-cover-aware DEM
  fusion, the colloquial class) and POSTAL (D-087). The result-format spike decides blob v0's
  successor and zero-copy blobs for Python, the JVM and Dart.
- **Blocked:** nothing.

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
