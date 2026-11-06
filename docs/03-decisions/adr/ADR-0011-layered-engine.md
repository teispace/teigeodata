---
title: "ADR-0011: Layered packs merged live"
status: accepted
updated: 2026-11-06
---

# ADR-0011: Layered packs merged live

Status: accepted, 2026-11-06

## Context

Apps combine bundled data with downloaded regions and later updates.

## Decision

The engine holds an ordered set of packs and answers every query across all of them, merging by stable ID. A newer release wins by default; the developer can set priorities. Tombstones and redirects apply across layers. Adding or removing a pack swaps an immutable snapshot atomically; queries already running finish on the old snapshot. Optional compaction writes the layers into one pack. Packs declare dependencies (for example places → admin) so hierarchy IDs resolve.

## Consequences

- Metamorphic tests: splitting or reordering packs must not change answers.
- k-way merge for kNN and search across layers.

## Alternatives considered

Merging at load time into one in-memory database (slow start, memory spikes).

## Evidence

Research thread of 2026-11-06.
