---
title: "ADR-0003: v1 is the whole product; coverage grows by data releases"
status: accepted
updated: 2026-11-06
---

# ADR-0003: v1 is the whole product; coverage grows by data releases

Status: accepted, 2026-11-06

## Context

The maintainer requires every feature in v1, with no v2 deferrals, and a country rollout Nepal → India → South Asia → Asia → USA → world.

## Decision

**Software v1.0 ships the whole feature set in [scope](../../00-vision/scope.md).** Country coverage grows through **data releases** versioned by CalVer (`YYYY.MM[.N]`), each readable by any v1 reader whose format version it declares. Software follows SemVer.

## Consequences

- The roadmap is phases and milestones to v1.0, plus data waves after it.
- The format carries a version and a minimum reader version from the first pack.

## Alternatives considered

Versioned feature tranches (v1 core, v2 bindings): rejected by the maintainer.

## Evidence

Maintainer decision, 2026-11-06.
