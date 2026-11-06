---
title: "ADR-0033: Timezone data: boundary-builder with oceans, and our own tzdb bundle"
status: accepted
updated: 2026-11-06
---

# ADR-0033: Timezone data: boundary-builder with oceans, and our own tzdb bundle

Status: accepted, 2026-11-06. Refines [ADR-0022](ADR-0022-timezone-and-postal.md).

## Context

Exact timezone lookup needs current boundaries and current rules. The Rust tzdb crates ship stale copies (`jiff-tzdb` 2026e misses this month's Cuba change; `chrono-tz` has 2025b).

## Decision

- Boundaries: timezone-boundary-builder **with oceans** (2026f: 444 zones, 1,356 outer rings, 792 holes, about 8.3 M vertices), quantised to the 1e-7° grid ([ADR-0029](ADR-0029-coordinate-grid.md)). Main build only (ODbL).
- Rules: **our own TZif bundle built from the matching tzdb release** (about 200 KB), loaded through `jiff` (0.2.39 checked), so rules are never older than the boundaries. The tzdb and boundary versions are in every pack's manifest; a data release follows each tzdb release.
- Lite: timezone areas from admin polygons and tzdb zone mappings, flagged as admin-resolution.

## Consequences

- A tz freshness check in the data build: the bundle's tzdb version must equal the latest release.
- Estimated exact world pack: 15–25 MB (to be measured in spike S2).

## Alternatives considered

Using a crate's bundled tzdb (stale); simplified boundaries (inexact).

## Evidence

[Timezone and elevation](../../02-research/notes/timezone-and-elevation.md), 2026-11-06.
