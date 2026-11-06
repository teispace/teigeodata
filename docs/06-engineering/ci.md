---
title: "CI"
status: draft
updated: 2026-11-06
---

# CI

The repository is public, so GitHub Actions is free and runs all CI
([ADR-0019](../03-decisions/adr/ADR-0019-ci.md)). `act` is not used for this repository.
Runner specs and limits are recorded in the [bindings and MCP research](../02-research/notes/bindings-and-mcp.md)
and checked again before each workflow change.

## Workflows

| Workflow | Trigger | Jobs | Budget |
|---|---|---|---|
| `fast-check` | every push and PR | fmt, clippy, unit and property tests on fixtures, `check-docs`, `check-data` (country folders and the source lock), `check-workspace`; the same fmt, clippy and tests for the `forge/` workspace in its own job; `cargo deny` for both workspaces; typos | ≤ 10 min |
| `verify` | push to `main` | full tests on Linux x64/arm64, macOS arm64, Windows x64; doc tests; golden; parity (fast set) | ≤ 30 min |
| `bindings` | push to `main`, release | cross-compile matrix (Android ABIs, iOS slices, WASM variants, desktop targets), binding tests per language | ≤ 45 min |
| `nightly` | schedule | fuzzing, Miri, sanitizers, mutants, coverage, differential, benchmarks against baselines | ≤ 6 h per job |
| `data-build` | schedule, manual | sharded matrix: one job per pack or tile; content-addressed source cache; a final verify-sign-publish job | per shard ≤ 6 h |
| `release` | tag | crates.io, npm, PyPI, pub.dev, Maven Central, NuGet, SwiftPM, CLI binaries; signing, SBOM, provenance attestations | |
| `docs` | push to `main` | the Fumadocs site with generated pages, to GitHub Pages | ≤ 10 min |

Workflows are added when the code they test exists. `fast-check` exists from Phase 0.

## Rules

- Actions are pinned by commit SHA; Renovate updates them.
- Least-privilege `permissions:` per workflow; `pull_request_target` is not used with untrusted code.
- Caches are keyed by lockfiles; large data intermediates go to release assets, not the Actions cache.
- Pushes are batched; a PR body says which checks ran.
