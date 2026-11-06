---
title: "ADR-0008: Stable place identifiers with redirects and tombstones"
status: accepted
updated: 2026-11-06
---

# ADR-0008: Stable place identifiers with redirects and tombstones

Status: accepted, 2026-11-06

## Context

Developers store place IDs in their databases. IDs that change break them.

## Decision

Places have a stable 64-bit ID, allocated once and **never reused**. A merge leaves a redirect from the absorbed ID to the survivor; a removal leaves a tombstone. External IDs (GeoNames, OSM, Wikidata, Overture GERS, national codes) are cross-references, never our identity. The textual form is documented and round-trips.

## Consequences

- An ID-stability gate compares every release with the previous one.
- Packs carry redirect and tombstone sections.

## Alternatives considered

Using a source ID (it breaks when the source changes) or content hashes (they change with corrections).

## Evidence

[Principles](../../00-vision/principles.md) P7.
