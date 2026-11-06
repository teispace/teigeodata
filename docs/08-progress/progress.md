---
title: "Progress"
status: living
updated: 2026-11-06
---

# Progress

## Where we are

- **Phase:** 0, foundations and research ([phase file](../07-planning/phases/phase-0.md)).
- **Now:** Phase 0.
  - **Done:** 0.1 and 0.2; the 0.3 research notes applied (ADR-0035 to ADR-0038); schema and
    pack-format drafts; spikes S1 and S2.
  - **World-general (P16):** every country lives in its own folder,
    `data/countries/<cc>/` (profile, lexicon, curated golden queries), validated by
    `cargo xtask check-data`.
  - **Golden set:** Nepal v0 has 524 queries (gap: `colloquial` has 14 of a target 30).
- **Next:** pkey pair sets; spike S3 (search on the golden set); S5 (adds OSM short and
  alternate names, which fill the colloquial gap); S6; S4.
- **Blocked:** nothing.

## Session log

| Date | What happened | Next step |
|---|---|---|
| 2026-11-06 | Discussion and decisions (see the [decision log](../03-decisions/log.md)). Repository initialised; AGENTS.md, conventions, vision, principles, scope, 26 ADRs, roadmap, Phase 0 plan. Research agents launched: Nepal sources, source licences, format/compression/indexing, bindings and MCP, names and search, timezone and elevation. | Finish 0.1; collect the research notes |
| 2026-11-06 | Foundation files, workspace, xtask gates and hooks; bindings and format research applied; ADR-0027 (JNI), ADR-0028 (MCP baseline), ADR-0029 (1e-7° grid) | Commit, create repo, apply the other notes |
| 2026-11-06 | Spike S2 made exact (3 bugs found by set-based differential testing; source overlaps found, Q-013); leaf-size sweep; forge, offline-loading and competitive notes landed; benchmarking research and catalogue (D-042) | Push, apply the new notes, then S3 |
| 2026-11-06 | Lexicon and data checks; per-country folders and profiles (P16); world-general search fold; golden set v0 for Nepal (524 queries, reproducible) | S3 search spike |
