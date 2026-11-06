---
title: "ADR-0007: Accuracy model: consensus position and elevation with uncertainty"
status: accepted
updated: 2026-11-06
---

# ADR-0007: Accuracy model: consensus position and elevation with uncertainty

Status: accepted, 2026-11-06

## Context

Accuracy of coordinates and elevation is the top priority. Sources disagree, and many coordinates are rounded to arc-minutes.

## Decision

Every place has a consensus `point` (weighted robust centre of the candidates, checked against built-up evidence) with `uncertainty_m`, plus `label_point` and, where known, `admin_centre`. Elevation comes from elevation models sampled with interpolation, with `ele_uncertainty_m` and optional footprint statistics; peaks use surveyed heights where authoritative. Heights are orthometric (EGM2008), and the API converts to ellipsoidal heights. **Targets for v1:** P90 vertical ≤ 50 m for settlements (tighter where achievable), published per country; horizontal targets are set from the Phase 0 measurement.

## Consequences

- QA gates: reported-vs-DEM, neighbour consistency, rounded-coordinate detection, point-in-parent.
- Generated `ACCURACY.md` per release.
- Algorithms and tolerances: [timezone and elevation](../../02-research/notes/timezone-and-elevation.md).

## Alternatives considered

Trusting a single source (GeoNames-style), or using DEM values with no uncertainty.

## Evidence

[Vision](../../00-vision/vision.md); maintainer decision on the 50 m tolerance, 2026-11-06.
