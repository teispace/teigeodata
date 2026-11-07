---
title: "ADR-0013: One exact region index for timezone, admin and perspective lookup"
status: accepted
updated: 2026-11-07
---

# ADR-0013: One exact region index for timezone, admin and perspective lookup

Status: accepted, 2026-11-06 (proven by spike S2)

## Context

Point → timezone and point → admin unit must be exact (identical to the source polygons), fast (~100–200 ns), and compact.

## Decision

An adaptive quadtree with Morton-coded leaves. Leaves wholly inside one region store its ID; leaves on a border store the edge fragments clipped to that cell at full source resolution, tested with robust geometric predicates. One engine serves IANA timezone, reverse geocoding and boundary perspectives. Exactness is proven by differential testing against a reference point-in-polygon. Confirmed or revised by [timezone and elevation](../../02-research/notes/timezone-and-elevation.md) and [format research](../../02-research/notes/format-compression-indexing.md).

Refined by [format, compression and indexing](../../02-research/notes/format-compression-indexing.md) (2026-11-06): an S2ShapeIndex-style cell index (centre label plus clipped edges and a crossing count) with exact integer predicates on cell-local coordinates and a defined tie-break for points exactly on a boundary. "Exact" means exact against the source **quantised once to 1e-7°** ([ADR-0029](ADR-0029-coordinate-grid.md)). `tzf-rs` with full data is the speed rival; `timezonefinder` is the correctness reference.

Further refined by [timezone and elevation](../../02-research/notes/timezone-and-elevation.md) (2026-11-06): border cells keep the **original source edges** that cross them (not re-clipped fragments), as S2's ShapeIndex does; a point in a border cell is classified by counting crossings along the segment from the cell centre, whose region is precomputed. Predicates are exact `i128` integer arithmetic; edges are straight in lon/lat (S2's geodesic edges would not match the source).

Confirmed by [spike S2](../../02-research/benches/s2-region-index.md) (2026-11-06): no mismatches inside the domain on 40,000 points (uniform and 1 cm from borders) against an independent ray cast. Added rules: each cell's reference point lies strictly inside the cell and never on one of its edges; zones covering a cell are a set (source zones overlap, for example Shanghai/Urumqi and Kathmandu/Kolkata slivers); a lookup returns every containing zone, and the single-zone answer follows a documented precedence (Q-013); out-of-domain points are rejected. About 64 edges per leaf with edge-ID runs is the size starting point (≈43 MB estimated for the full-resolution world with oceans).

Refined by B9 (2026-11-07, D-083): borders are stored once as **shared arcs**, each with the set of regions it separates, and a leaf lists its edges as runs of consecutive edge numbers. Leaves are stored in Morton order under a **dense top grid**; a top cell held whole by an edge-free leaf stores its region set, so most queries are one read. A point on a border is read as nudged by (ε, ε²) (simulation of simplicity), which makes the answer equal an even–odd ray cast to the east; this is the half-open rule. **Precedence** is the region ID order: the lowest ID is the single answer, and the forge orders the IDs (Q-013). `tzf-rs` full data stores coordinates at 1e-5°, so it is a speed rival only, not an exactness reference ([REGION rivals](../../02-research/notes/region-rivals.md)). Layout and rules: [pack format, REGION](../../04-architecture/pack-format.md#region); measurements: [B9](../../02-research/benches/phase-2.md#b9-region).

## Consequences

- A world timezone pack works standalone.

## Alternatives considered

Simplified polygons (inexact at borders); H3 cell tables (clipping to hexagons is awkward, inexact at edges).

## Evidence

Research thread of 2026-11-06.
