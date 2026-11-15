---
title: "Phase 7: docs site"
status: draft
updated: 2026-11-15
---

# Phase 7: docs site

**Goal:** a public site where a developer in any supported language goes from nothing to a
working query in minutes, and finds every function, every country's data and every measured
number: guides per language whose examples ran, a reference generated from the API, pages
generated from the data and the gates, and a playground in the browser. Exit: milestone **M7**
([roadmap](../roadmap.md)).

**Research:** [docs site](../../02-research/notes/docs-site.md) (2026-11-15); ADR-0024.

Order: the site and its deploy, then the guides with their executed examples (the largest
part), the reference, the boundary in the language API (a gap the guides found), the generated
pages, the playground. Nothing names a country in code (P16): country pages come from data.

## Steps

### 7.0 Research
- [x] The docs-site note: stack and versions live, deploy, examples, reference, playground
  (2026-11-15)

### 7.1 The site
- [ ] `site/`: Fumadocs on Next.js as a static export under `/teigeodata/`, static search,
  `llms.txt`, the landing page, a navigation for concepts, guides, reference, data, servers
- [ ] `docs.yml`: build on every push touching the site; type check; links checked; deploy to
  Pages when the maintainer has enabled it (`DOCS_DEPLOY`)

### 7.2 Guides, with examples that ran
- [ ] Concepts: places and IDs, packs and components, layers, the store and the catalog,
  results (blobs and readers), accuracy, licences and attribution
- [ ] Getting started; the CLI guide (`tgd`, every command, with its real output)
- [ ] A guide per language (Rust, C, C++, Swift, Kotlin and Java, Dart and Flutter, Python,
  Node.js, the browser and other WebAssembly hosts, Go, .NET): install (today and from the
  registry), open a pack, every query, read results, errors, threads, bundling and
  downloading packs, platform notes; each backed by a program in `site/examples/<lang>/`
- [x] `cargo xtask docs-examples [--check]`: every example run on the pinned public pack, its
  output compared with the checked-in output (D-322); Rust, Python, Node.js, WebAssembly, Go, Swift,
  Dart and the CLI so far
- [ ] A result `Reader` of one shape in every language (D-322): Node.js, WebAssembly, Go, Python,
  Swift and Dart done; Kotlin and Java, C, C++, .NET
- [ ] The tours in C, C++, Java, Kotlin and .NET; `docs-examples` in CI
  per language
- [ ] The servers: `tgd serve` (every route, Nominatim compatibility, deployment) and
  `tgd mcp` (clients' configuration, tools, resources, installs)

### 7.3 The reference
- [ ] Generated from the IDL by `cargo xtask bindings`: every function, method, record, enum
  and error with its documentation and its signature in each language; `--check` fails on drift
- [ ] The HTTP API (routes, parameters, answers, errors) and the MCP tools from the servers'
  own definitions

### 7.4 The boundary in the language API
- [ ] Research, then `Engine.boundary` (an outline as GeoJSON at a level of detail) and the
  distance between places, in every language, with parity values

### 7.5 Pages from the data and the gates
- [ ] Per-country pages from `data/countries/` and the released packs: sources and licences,
  coverage, admin levels, lexicon, accuracy
- [ ] Measured pages from the gates' reports: sizes, benchmarks, accuracy, load

### 7.6 The playground
- [ ] The WebAssembly build over the public layer, in the browser, with MapLibre drawing the
  pack's own outlines: search, reverse, admin, timezone, elevation, nearby, boundary

### 7.7 Exit review
- [ ] M7 criteria met, each with its evidence; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Phase 7 drafted from the docs-site research | M6 met (D-321) |
| 2026-11-15 | A reader in every language added to 7.2 | The tours found most bindings cannot read a result (D-322) |
