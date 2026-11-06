---
title: "ADR-0037: Signing keys and metadata hosting"
status: accepted
updated: 2026-11-06
---

# ADR-0037: Signing keys and metadata hosting

Status: accepted, 2026-11-06. Refines [ADR-0012](ADR-0012-offline-loading.md) and [ADR-0025](ADR-0025-hosted-ready.md). Answers Q-006.

## Context

Apps download packs from hosts we do not control end to end. Trust must not depend on the host. The hosting must also work for native apps and for browser apps.

## Decision

- **Catalog trust:** a TUF 1.0 subset with four top-level roles (root, timestamp, snapshot, targets).
  - No delegations; consistent snapshots; pack details go in each target's `custom` field.
  - **Root:** offline keys, threshold 2 of 3.
  - **Timestamp:** an online key in a scheduled CI job, signing a 14-day timestamp.
  - Snapshot and targets are signed at release.
  - Our own small client; `tough` and python-tuf are differential oracles in tests.
- **Pack signatures:** each pack also carries its own Ed25519 signature ([ADR-0009](ADR-0009-pack-format.md)), so bundled and side-loaded packs verify without a catalog.
- **Expiry:** expired metadata blocks installs and updates. It never blocks queries on packs that are already installed.
- **Hosting:**
  - Metadata on GitHub Pages, which sends CORS headers and ETags.
  - Packs on immutable GitHub Releases, sharded across releases (each holds at most 1,000 assets).
- **Web:** GitHub release assets send no CORS headers, so browser apps need a CORS-enabled mirror.
  - The mirror serves the same bytes, and trust never depends on it.
  - The provider (for example an object store with free egress) needs an account, which only the maintainer can provide.
  - Until then, web developers can proxy or self-host the release assets.
- **Release provenance:** Sigstore attestations on immutable releases. This is a supplement; it is not the client's trust path.

## Consequences

- Key ceremony documentation and a key-rotation drill before the first release.
- Fifteen sync gates from the research: rollback, freeze, mix-and-match, endless data, root rotation, crash during install, resume against a host that ignores `If-Range`, bad mirror, and the others listed in the note.
- The client must check ETag, length and hashes itself on resume, because the GitHub asset host ignores `If-Range`.

## Alternatives considered

- minisign or signify: no rotation, no expiry, no rollback defence.
- Sigstore as the client trust path: needs online checks.
- Pages for packs: size limits.

## Evidence

[Offline loading and trust](../../02-research/notes/offline-loading-and-trust.md), 2026-11-06, including measured GitHub behaviour.
