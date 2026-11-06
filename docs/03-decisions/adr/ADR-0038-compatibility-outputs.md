---
title: "ADR-0038: Compatibility outputs: Nominatim-style JSON and GeoJSON"
status: accepted
updated: 2026-11-06
---

# ADR-0038: Compatibility outputs: Nominatim-style JSON and GeoJSON

Status: accepted, 2026-11-06. Applies to the HTTP server and the CLI ([ADR-0025](ADR-0025-hosted-ready.md)).

## Context

Many apps already talk to Nominatim or Photon. Switching should take a URL change, not a rewrite.

## Decision

- The HTTP server offers three output modes:
  - **Native:** our schema, with every field and its uncertainty.
  - **Nominatim-compatible JSON:** `/search`, `/reverse` and `/lookup`, with the common parameters and result fields.
  - **GeoJSON:** a FeatureCollection.
- The CLI exposes the same modes.
- Compatibility modes are mapped from the native result and never add data the native mode lacks.
- Fields we cannot fill are left out instead of being faked.
- A conformance test runs a corpus of Nominatim client requests against the server.

## Consequences

- A mapping table in the server docs.
- The conformance corpus becomes part of the server's tests.

## Alternatives considered

- Native only: a higher switching cost for existing users.
- A full Nominatim clone: it would tie our schema to theirs.

## Evidence

[Competitive analysis](../../02-research/notes/competitive-analysis.md), 2026-11-06.
