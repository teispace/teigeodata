---
title: "Machine resources and cleanup"
status: accepted
updated: 2026-11-06
---

# Machine resources and cleanup

The development machine is shared (16 GB RAM, 10 cores, limited free disk). Rules, from
[AGENTS.md](../../AGENTS.md) §7:

- At most 1–2 heavy jobs at a time (cargo builds of the workspace, data builds, benchmarks).
- `CARGO_BUILD_JOBS=4` when another heavy job runs.
- Keep free disk above 30 GB. Check with `df -h /` before heavy work.
- After every heavy step: `cargo xtask clean` (default: removes incremental caches and benchmark
  and data scratch, and keeps the built packs in `data/build/`; `--all` also removes
  `data/build/` and runs `cargo clean`). Kill stray processes you started.
- The dev profile uses line-table debug info for workspace crates and none for dependencies,
  to keep `target/` small.
- Data builds stream and tile with a disk budget, and refuse to start if it won't fit.
