---
title: "ADR-0022: Exact IANA timezone and postal codes with validity periods"
status: accepted
updated: 2026-11-06
---

# ADR-0022: Exact IANA timezone and postal codes with validity periods

Status: accepted, 2026-11-06

## Context

Timezone at a point is one of the most requested lookups and must be exact, including history (Asia/Kathmandu was +05:30 until 1986). Postal codes change (Nepal is reported to be revising its scheme).

## Decision

- **Timezone:** the full IANA zone set with oceans, from the region index ([ADR-0013](ADR-0013-region-index.md)); offsets and transitions from an optional embedded tzdb; every place also carries its zone; the tzdb and boundary versions are in the manifest.
- **Postal codes:** stored with `scheme`, `valid_from`, `valid_to`, kind (area, post office, point), and links to places, so current and former codes coexist with redirects.

## Consequences

- The Nepal scheme is verified with primary sources before shipping ([Nepal sources](../../02-research/notes/nepal-sources.md)).

## Alternatives considered

Only current codes (breaks old addresses); simplified timezone polygons (wrong near borders).

## Evidence

Maintainer direction, 2026-11-06.
