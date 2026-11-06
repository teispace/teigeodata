---
title: "ADR-0024: Docs site with Fumadocs"
status: accepted
updated: 2026-11-06
---

# ADR-0024: Docs site with Fumadocs

Status: accepted, 2026-11-06

## Context

Developers need guides per language, a generated reference, data pages and live examples.

## Decision

The site is built with Fumadocs (Next.js), deployed as a static export to GitHub Pages. The API reference is generated from the IDL; measured pages from gates; per-country pages from the data. An in-browser playground runs the WASM build with MapLibre and real packs.

## Consequences

- Docs examples are executed in CI.

## Alternatives considered

mdBook (weaker for multi-language guides), Docusaurus, Starlight.

## Evidence

Maintainer direction, 2026-11-06.
