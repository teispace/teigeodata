---
title: "ADR-0002: Licences: Apache-2.0 code, ODbL main data, CC-BY lite data"
status: accepted
updated: 2026-11-06
---

# ADR-0002: Licences: Apache-2.0 code, ODbL main data, CC-BY lite data

Status: accepted, 2026-11-06

## Context

OpenStreetMap is the freshest source for local names and settlements, and it is ODbL: any database derived from it must be ODbL. Some users cannot accept share-alike.

## Decision

- **Code:** Apache-2.0 (patent grant; matches Teispace practice).
- **Main data build:** ODbL 1.0, with full attribution in `ATTRIBUTION.md` and in every pack's manifest.
- **Lite data build:** CC-BY 4.0. It is **clean-room**: it never reads an ODbL, share-alike or non-commercial input at any step, not even to decide merges. A provenance gate fails the build otherwise ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md)). Lite is produced per country, for the components whose every input passes the licence gate.
- **Docs:** CC-BY 4.0.
- GADM and other non-commercial sources are never used.

## Consequences

- Every field carries provenance (source, version, licence class), see [ADR-0023](ADR-0023-provenance.md).
- Lite and main share the schema and IDs, so switching is drop-in.
- Licence facts are researched per source: [source licences](../../02-research/notes/source-licences.md).

## Alternatives considered

Main only (excludes share-alike-averse users); lite only (loses OSM's local detail); a permissive main build (impossible with OSM).

## Evidence

Research thread of 2026-11-06; [source licences](../../02-research/notes/source-licences.md).
