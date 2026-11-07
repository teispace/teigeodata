---
title: "Roadmap"
status: living
updated: 2026-11-07
---

# Roadmap

v1.0 is the whole product ([ADR-0003](../03-decisions/adr/ADR-0003-v1-is-everything.md)). The
road to it is nine phases with milestone exit criteria. Phases overlap where dependencies
allow; each phase starts with its research notes ([ADR-0020](../03-decisions/adr/ADR-0020-research-loop.md)).
After v1.0, coverage grows by data waves.

## Phases to v1.0

| Phase | Name | Depends on | Exit milestone |
|---|---|---|---|
| [0](phases/phase-0.md) | Foundations and research | — | **M0**: docs system, research notes, spikes measured, baseline accuracy of existing data for Nepal, CI fast-check green |
| [1](phases/phase-1.md) | Core and format | 0 | **M1**: `core` types and codecs, `.tgd` reader/writer with every section kind, fuzzed, benchmarked, specified |
| [2](phases/phase-2.md) | Indexes | 1 | **M2**: spatial index, exact region index (world timezone pack proven exact), search index with the phonetic key; all A/B-benchmarked (ID maps were done in B5, D-075) |
| [3](phases/phase-3.md) | Forge and Nepal data | 1, 2 | **M3**: Nepal main and lite builds reproducible; geo-fix, conflation, hierarchy, names lexicon, postal, timezone; every QA gate; `ACCURACY.md` for Nepal |
| 4 | Engine, baseline and sync | 2, 3 | **M4**: layered engine, embedded baseline, catalog, download and storage managers, delta updates (section re-download first), the 15 sync gates of ADR-0037, CLI `tgd` complete |
| 5 | Boundary and bindings | 4 | **M5**: C ABI, IDL, generators, every binding with packaging (Swift privacy manifest, Android `noCompress` for bundled packs), the parity gate and the X benchmarks green across all |
| 6 | Servers | 4 | **M6**: MCP server (latest revision) and HTTP server, hosted-ready seams |
| 7 | Docs site | 5, 6 | **M7**: Fumadocs site, generated reference, guides per language, per-country pages, playground |
| 8 | Hardening and release | all | **M8 = v1.0.0**: fuzzing campaign, performance pass, security review, docs complete, first data release (Nepal) |

## Data waves after v1.0

| Wave | Coverage | Notes |
|---|---|---|
| W1 | India | per state, tiled; village/town level |
| W2 | South Asia | Bangladesh, Bhutan, Sri Lanka, Pakistan, Maldives, Afghanistan |
| W3 | Asia | |
| W4 | USA | strong public-domain sources; lite build at full quality expected |
| W5 | Rest of the world | depth per country by data quality and population threshold |

Every wave passes the same gates as Nepal. Each wave starts with a per-country research note in
`docs/05-data/countries/`.

## How work runs

Up to 5–6 agents in parallel, at most 1–2 heavy ([AGENTS.md](../../AGENTS.md) §7). Typical
parallel lanes: research notes, docs, one heavy build lane, one test or benchmark lane.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-06 | Roadmap created | Project start |
| 2026-11-07 | M0 met; phase 1 file | D-069 |
| 2026-11-07 | M1 met; Phase 2 next, research first | D-080 |
| 2026-11-07 | Phase 2 file; M2 no longer lists ID maps | D-081; done in B5 (D-075) |
