---
title: "ADR-0021: Repository tooling in Rust through xtask"
status: accepted
updated: 2026-11-06
---

# ADR-0021: Repository tooling in Rust through xtask

Status: accepted, 2026-11-06

## Context

Contributors and CI should need one toolchain.

## Decision

Every repository task (docs check, generators, gates, size reports, cleanup, releases) is a `cargo xtask` subcommand. Scripts in other languages are not added to the repository's required path.

## Consequences

- `cargo xtask help` lists every task.

## Alternatives considered

Shell and Python scripts (more toolchains to install, less portable).

## Evidence

Research thread of 2026-11-06.
