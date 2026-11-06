---
title: "ADR-0036: Corrections as reviewed data with guards"
status: accepted
updated: 2026-11-06
---

# ADR-0036: Corrections as reviewed data with guards

Status: accepted, 2026-11-06. Extends [ADR-0008](ADR-0008-stable-ids.md) and [ADR-0023](ADR-0023-provenance.md).

## Context

Corrections, from contributors and from our QA, must survive monthly rebuilds. They must not be silently applied over new upstream data, and must not be silently lost.

## Decision

- **Storage:** each correction is a reviewed YAML file, one per file: `data/corrections/<country>/<id>.yaml`. Its fields are:
  - a stable, never-reused `id`;
  - a `target` (a stable place ID, or a source record for fixes made before merging);
  - an `op`: `set`, `unset`, `same-as`, `not-same-as`, `split`, `retire` or `move`;
  - `field` and `value`;
  - a **guard**: the source record and a BLAKE3 hash of the source value it was written against;
  - **evidence**, each item with its licence class;
  - author, dates and review.
- **Outcomes:** every build evaluates each guard and records one outcome:

  | Situation | Outcome |
  |---|---|
  | Guard holds | **apply** |
  | Upstream now has the corrected value | **retire**: fixed upstream, marked as upstreamed |
  | Upstream changed to something else | **stale**: not applied; goes to the QA queue with a diff |
  | Target was redirected | follow the redirect, then re-check the guard |
  | Target was tombstoned | **stale** |

- `same-as` and `not-same-as` become must-link and cannot-link constraints for the clusterer.
- **Lite:** the lite build ignores corrections whose evidence class is refused in lite ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md) rule 3).
- **Upstream:** a correction of an upstream error is also reported upstream (OSM, Wikidata, GeoNames), and the report is linked from the file.

## Consequences

- An `xtask` schema check for correction files.
- A release report of applied, retired and stale counts.
- A gate: no stale correction stays unreviewed for longer than one release.

## Alternatives considered

- Editing upstream only: too slow, and some sources do not accept edits.
- Unguarded override tables: these silently override newer upstream data.

## Evidence

[Data pipeline (forge)](../../02-research/notes/forge-pipeline.md) §3.7, 2026-11-06.
