---
title: "Progress"
status: living
updated: 2026-11-06
---

# Progress

## Where we are

- **Phase:** 0, foundations and research ([phase file](../07-planning/phases/phase-0.md)).
- **Now:** 0.1 done locally (first commit pending push). All ten research notes have landed:
  bindings and MCP, format, Nepal sources, licences, names and search, timezone and elevation,
  forge, offline loading and trust, competitive analysis, and benchmarking. The last four still
  need their "impact on the plan" applied (0.3): the forge and ID-stability ADRs, keys and
  hosting (Q-006: web apps need a CORS mirror), and the DIGIPIN licence correction (Apache-2.0)
  in `source-licences.md`. Spikes S1 and S2 are done; S2 is exact
  ([results](../02-research/benches/s2-region-index.md)). The benchmark catalogue is in
  [benchmarks](../06-engineering/benchmarks.md).
- **Next:** push and get `fast-check` green; apply the four new notes; lexicon schema check and
  the Nepal golden set; schema and pack-format drafts; then spikes S3 to S6, one at a time.
- **Blocked:** nothing.

## Session log

| Date | What happened | Next step |
|---|---|---|
| 2026-11-06 | Discussion and decisions (see the [decision log](../03-decisions/log.md)). Repository initialised; AGENTS.md, conventions, vision, principles, scope, 26 ADRs, roadmap, Phase 0 plan. Research agents launched: Nepal sources, source licences, format/compression/indexing, bindings and MCP, names and search, timezone and elevation. | Finish 0.1; collect the research notes |
| 2026-11-06 | Foundation files, workspace, xtask gates and hooks; bindings and format research applied; ADR-0027 (JNI), ADR-0028 (MCP baseline), ADR-0029 (1e-7° grid) | Commit, create repo, apply the other notes |
| 2026-11-06 | Spike S2 made exact (3 bugs found by set-based differential testing; source overlaps found, Q-013); leaf-size sweep; forge, offline-loading and competitive notes landed; benchmarking research and catalogue (D-042) | Push, apply the new notes, then S3 |
