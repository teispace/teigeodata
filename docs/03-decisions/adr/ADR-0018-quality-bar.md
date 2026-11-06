---
title: "ADR-0018: The quality bar is gated"
status: accepted
updated: 2026-11-06
---

# ADR-0018: The quality bar is gated

Status: accepted, 2026-11-06

## Context

An SDK people build on is judged on correctness, speed and size over years. Unmeasured claims drift; ungated measurements regress.

## Decision

[The quality bar](../../06-engineering/quality-bar.md) is binding. A step is done only when its applicable gates are green. Every optimisation is preceded by a profile and followed by a measurement recorded in the commit body. Generated pages (`ACCURACY.md`, `SIZES.md`, `BENCHMARKS.md`, `CONFORMANCE.md`) are the public record.

## Consequences

- Test infrastructure (counting allocator, benchmark baselines, golden sets, differential harnesses) is built before the first domain module.

## Alternatives considered

Best-effort testing.

## Evidence

[Principles](../../00-vision/principles.md) P2.
