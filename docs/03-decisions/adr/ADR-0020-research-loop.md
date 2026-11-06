---
title: "ADR-0020: Research, compare, then build, and keep improving"
status: accepted
updated: 2026-11-06
---

# ADR-0020: Research, compare, then build, and keep improving

Status: accepted, 2026-11-06

## Context

The maintainer requires research and analysis for every implementation, and continuous improvement of plans and docs.

## Decision

Every phase and every step touching a new algorithm, library, format, source or platform starts with a research note that compares options (benchmarks where it matters). Plans, ADRs and docs are revised when research or work shows a better way, with changelog rows. See [AGENTS.md](../../../AGENTS.md) §2 and §4.

## Consequences

- Research notes have a `revisit` date; the progress log tracks overdue revisits.

## Alternatives considered

Building first and researching when problems appear.

## Evidence

Maintainer direction, 2026-11-06.
