---
title: "ADR-0019: CI on GitHub Actions, including sharded data builds"
status: accepted
updated: 2026-11-06
---

# ADR-0019: CI on GitHub Actions, including sharded data builds

Status: accepted, 2026-11-06

## Context

The repository is public, so GitHub Actions minutes are free. The local machine has limited disk and memory.

## Decision

All CI runs on GitHub-hosted runners (Linux x64/arm64, macOS, Windows). **Data builds are sharded** into one job per pack or tile so each shard fits a runner, with content-addressed source caches and a final verify-sign-publish job. Local builds use the same streaming forge with disk-budget guards, for debugging. `act` is not used for this repository.

Refined by [forge pipeline](../../02-research/notes/forge-pipeline.md) (2026-11-06): the data-build matrix comes from `forge/manifest.json`. Workflow artifacts carry data within a run, the cache carries content-hashed shards across runs, and inputs are mirrored as release assets. Each job stays under 6 h and 14 GB ([ADR-0035](ADR-0035-forge-architecture.md)). Benchmark jobs follow [benchmarks](../../06-engineering/benchmarks.md).

## Consequences

- Workflows and budgets: [CI](../../06-engineering/ci.md).

## Alternatives considered

Local-only data builds (the machine cannot hold India and building footprints).

## Evidence

Maintainer direction, 2026-11-06.
