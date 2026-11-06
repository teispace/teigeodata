---
title: "ADR-0004: One public monorepo: teispace/teigeodata"
status: accepted
updated: 2026-11-06
---

# ADR-0004: One public monorepo: teispace/teigeodata

Status: accepted, 2026-11-06

## Context

Contributors must see code, docs, decisions, data rules and gates in one place.

## Decision

One public repository, `teispace/teigeodata`: runtime crates, boundary crate, bindings, CLI, servers, forge, data rules (`data/`), docs (`docs/`), site (`site/`), fuzz targets, fixtures. Raw sources and built packs are never committed; packs are published as release assets.

## Consequences

- CODEOWNERS per area and per country.
- CI is free (public repository) and runs on GitHub Actions ([ADR-0019](ADR-0019-ci.md)).

## Alternatives considered

A hub repository with separate code repositories (more coordination, harder for contributors).

## Evidence

Maintainer decision, 2026-11-06.
