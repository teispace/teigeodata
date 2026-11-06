---
title: "ADR-0034: Elevation method: position first, interval QA, a Nepal validation"
status: accepted
updated: 2026-11-06
---

# ADR-0034: Elevation method: position first, interval QA, a Nepal validation

Status: accepted, 2026-11-06. Refines [ADR-0007](ADR-0007-accuracy-model.md).

## Context

Research shows the 50 m vertical target depends mostly on position: an arc-minute-rounded coordinate on a 20° slope alone gives about 195 m of vertical error. Copernicus DEM's headline accuracy (2.17 m) excludes mountains; its rock-and-ice class is about 14 m, and no Nepal-specific validation exists.

## Decision

- **Position before height:** elevation is computed only after the consensus position; records with low-precision positions carry a correspondingly wide elevation uncertainty.
- **Consensus needs at least 3 independent sources** to be robust; with fewer, the uncertainty widens and the record is flagged.
- **Uncertainty model** = DEM error for the land-cover class + terrain relief within the position uncertainty + interpolation error.
- **QA uses height intervals, not fixed tolerances:** a reported height is checked against the DEM range within the position uncertainty, with slope bands of 15/31/61/97 m, and detectors for feet, ellipsoidal heights and old datums.
- **Datum:** EGM2008 orthometric heights; an embedded 2.5′ EGM2008 grid (52 KB for Nepal, 1.07 MB for South Asia) converts to ellipsoidal heights.
- **First elevation deliverable:** our own ICESat-2 evaluation of Copernicus GLO-30 (and NASADEM, GEDTM30) over Nepal, published with the accuracy report.
- Lite elevation candidates: NASADEM (CC0) and GEDTM30 (CC BY).

## Consequences

- Gates: P90 vertical target, interval calibration, interpolation and peak tests.

## Alternatives considered

A single fixed tolerance (wrong on slopes); trusting reported heights (feet and datum errors are common).

## Evidence

[Timezone and elevation](../../02-research/notes/timezone-and-elevation.md), 2026-11-06.
