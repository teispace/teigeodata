---
title: "Progress"
status: living
updated: 2026-11-06
---

# Progress

## Where we are

- **Phase:** 0, foundations and research ([phase file](../07-planning/phases/phase-0.md)).
- **Now:** 0.1 and 0.2 done; 0.3 notes applied (ADR-0035 forge, ADR-0036 corrections,
  ADR-0037 keys and hosting, ADR-0038 compatibility outputs; ADR-0013 accepted after S2). Spikes
  S1 and S2 done. Benchmark catalogue in [benchmarks](../06-engineering/benchmarks.md).
- **Next:** lexicon schema check in `xtask`; data schema and pack-format drafts; Nepal golden
  query set; spikes S3 (search), S5 (baseline accuracy), S6 (elevation), S4 (bindings).
- **Blocked:** nothing.

## Session log

| Date | What happened | Next step |
|---|---|---|
| 2026-11-06 | Discussion and decisions (see the [decision log](../03-decisions/log.md)). Repository initialised; AGENTS.md, conventions, vision, principles, scope, 26 ADRs, roadmap, Phase 0 plan. Research agents launched: Nepal sources, source licences, format/compression/indexing, bindings and MCP, names and search, timezone and elevation. | Finish 0.1; collect the research notes |
| 2026-11-06 | Foundation files, workspace, xtask gates and hooks; bindings and format research applied; ADR-0027 (JNI), ADR-0028 (MCP baseline), ADR-0029 (1e-7° grid) | Commit, create repo, apply the other notes |
| 2026-11-06 | Spike S2 made exact (3 bugs found by set-based differential testing; source overlaps found, Q-013); leaf-size sweep; forge, offline-loading and competitive notes landed; benchmarking research and catalogue (D-042) | Push, apply the new notes, then S3 |
