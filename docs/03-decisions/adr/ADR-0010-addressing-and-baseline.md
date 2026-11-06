---
title: "ADR-0010: Deterministic pack addressing and an embedded baseline"
status: accepted
updated: 2026-11-06
---

# ADR-0010: Deterministic pack addressing and an embedded baseline

Status: accepted, 2026-11-06

## Context

An app must know which file covers a query without a network round trip, and it must always answer something offline. The Swiss Ephemeris does this with date-addressed files (`sepl_18.se1`) and a built-in fallback theory.

## Decision

Packs have **deterministic addresses** derived from coverage: an admin code first (`np`, `ae-du`), split into fixed grid tiles for very large units (`in-br-t07`), plus the component (`ae-du.places.tgd`). A small **embedded baseline** (countries, first-level admin units, large cities, world timezones, and a locator mapping any point to its pack address) is compiled in, so every query has an answer offline. Every answer states its precision tier. Installed packs override the baseline automatically. Packs are found through an ordered **pack path** (app bundle, downloaded folder, developer folders, custom storage).

Refined by [offline loading and trust](../../02-research/notes/offline-loading-and-trust.md) (2026-11-06): the physical file name is `address.component.release.hash8.tgd`. Each version gets a new name and is never written over a file that may be mapped.

## Consequences

- `reverse()` can say which pack is missing for a point.
- The baseline size is budgeted and gated.

## Alternatives considered

Opaque pack IDs that need a catalog lookup for every query.

## Evidence

Maintainer direction, 2026-11-06.
