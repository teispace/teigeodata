---
title: "Progress"
status: living
updated: 2026-11-07
---

# Progress

## Where we are

- **Phase:** 0, foundations and research ([phase file](../07-planning/phases/phase-0.md)).
- **Now:** Phase 0.
  - **Done:** 0.1 and 0.2; the 0.3 research notes applied (ADR-0035 to ADR-0038); schema and
    pack-format drafts; spikes S1, S2 and S3.
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
- **S6 in progress:** ICESat-2 ATL08 through SlideRule is pinned as a POST tileset, with the
  EGM2008 geoid; `forge dem-ref` measures Copernicus at LE90 18.7 m inside Nepal (497,717
  segments; at most 32.6 m in any stratum), so the DEM meets the vertical target by itself
  (S6.1, S6.2, D-063; [results](../02-research/benches/s6-elevation.md)).
- **Next:** S6.3 (GEDTM30 through COG range reads; NASADEM needs the maintainer's Earthdata
  login or an OpenTopography key); then S4 bindings; then the M0 review. Phase 2 carries the S5
  follow-ups: the settlement core, geofix-consensus, and growing the colloquial class.
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
