---
title: "ADR-0006: Nepal perspective for disputed boundaries; perspectives as data"
status: accepted
updated: 2026-11-06
---

# ADR-0006: Nepal perspective for disputed boundaries; perspectives as data

Status: accepted, 2026-11-06

## Context

Nepal and India both claim Kalapani, Lipulekh and Limpiyadhura; other disputes exist worldwide. Showing a boundary is a statement.

## Decision

The default boundary set follows **Nepal's official map (2020)**. Disputed areas are stored as data with a `perspective` dimension, so other views (for example `in`, `de-facto`) can be added without format changes and selected by the developer. The default is declared on the docs site and in every pack manifest.

## Consequences

- The region index answers under the selected perspective.
- Docs warn app developers about local map regulations; the choice is theirs.

## Alternatives considered

A neutral or de-facto default (rejected by the maintainer); a single hard-coded view (inflexible).

## Evidence

Maintainer decision, 2026-11-06; [Nepal sources](../../02-research/notes/nepal-sources.md).
