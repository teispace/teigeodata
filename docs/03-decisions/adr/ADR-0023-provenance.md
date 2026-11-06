---
title: "ADR-0023: Provenance for every field"
status: accepted
updated: 2026-11-06
---

# ADR-0023: Provenance for every field

Status: accepted, 2026-11-06

## Context

Attribution, the lite build, corrections and trust all need to know where each value came from.

## Decision

Every field value records its source, source version and licence class, and the rule that chose it. Provenance is an optional component in packs (on in the full build) and always present in the forge.

## Consequences

- The licence gate and `ATTRIBUTION.md` are generated from provenance.

## Alternatives considered

Provenance per record only (too coarse for mixed-licence records).

## Evidence

[Principles](../../00-vision/principles.md) P8.
