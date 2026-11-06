---
title: "ADR-0012: Offline-first loading: catalog, downloads, storage"
status: accepted
updated: 2026-11-06
---

# ADR-0012: Offline-first loading: catalog, downloads, storage

Status: accepted, 2026-11-06

## Context

An end user may pick a region in an app ("Dubai"), and the library must download, verify, store and load it, with minimal work for the app developer.

## Decision

The SDK provides a **catalog** (region tree, names, sizes per component, versions, dependencies; signed, cached offline), a **download manager** (queue, progress, pause/resume through range requests, retries, per-section verification, atomic install, delta updates) and a **storage manager** (installed packs, quota, eviction policy, pinning, repair). Transport, storage and mirror are pluggable with good defaults; platform bindings add OS background-download hooks (Android WorkManager, iOS background URLSession). **The library never touches the network unless the app calls the catalog or download APIs.** This lives in an optional `sync` capability.

## Consequences

- A future hosted tier is just another mirror ([ADR-0025](ADR-0025-hosted-ready.md)).
- Trust uses a TUF-style signed metadata chain.

## Alternatives considered

Leaving downloads to every app (poor developer experience, unsafe verification).

## Evidence

Maintainer direction, 2026-11-06.
