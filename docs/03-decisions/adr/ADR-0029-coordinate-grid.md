---
title: "ADR-0029: The canonical coordinate grid is 1e-7 degrees"
status: accepted
updated: 2026-11-06
---

# ADR-0029: The canonical coordinate grid is 1e-7 degrees

Status: accepted, 2026-11-06

## Context

Exact answers (point in region, topology without gaps) need one agreed set of coordinates.
Floating-point source coordinates cannot be compared exactly across tools, and a format that
stores them as floats wastes space.

## Decision

All geometry is **quantised once, at ingest, to a 1e-7 degree integer grid** (about 1.1 cm at
the equator; the same resolution OpenStreetMap stores), held as `i32` longitude and latitude.
Every later step (topology, simplification, the region index, packs) works in these integers
with exact predicates. "Exact" in our claims means exact against this quantised source. Packs
may store coarser grids per column (for example point positions at 1e-6 degree) when the column's
descriptor says so and the accuracy model allows it; the elevation codec guarantees an error
bound of 1 m or less.

## Consequences

- Region-index exactness is testable with zero mismatches against a reference on the same grid.
- The quantisation error (≤ 0.56 cm per axis) is far below any source's accuracy.
- The accuracy model ([ADR-0007](ADR-0007-accuracy-model.md)) states the grid and the DEM bound.

## Alternatives considered

`f64` everywhere (inexact comparisons, larger); 1e-6 degree (11 cm, too coarse for shared
boundaries between small wards).

## Evidence

[format, compression and indexing](../../02-research/notes/format-compression-indexing.md).
