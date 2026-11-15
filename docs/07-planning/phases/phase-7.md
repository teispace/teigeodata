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
- [x] `site/`: Fumadocs on Next.js as a static export under `/teigeodata/`, static search,
  `llms.txt`, the landing page, a navigation for concepts, guides, reference, data, servers
  (2026-11-15)
- [x] `docs.yml`: build on every push touching the site; type check; links checked; deploy to
  Pages when the maintainer has enabled it (`DOCS_DEPLOY`) (2026-11-15)

### 7.2 Guides, with examples that ran
- [x] Concepts: places and IDs, packs and components, layers, the store and the catalog,
  results (blobs and readers), accuracy, licences and attribution (2026-11-15)
- [x] Getting started; the CLI guide (`tgd`, every command, with its real output) (2026-11-15)
- [x] A guide per language (Rust, C, C++, Swift, Kotlin and Java, Dart and Flutter, Python,
  Node.js, the browser and other WebAssembly hosts, Go, .NET): install (today and from the
  registry), open a pack, every query, read results, errors, threads, bundling and
  downloading packs, platform notes; each backed by a program in `site/examples/<lang>/`;
  written by `site/scripts/guides.py` from one copy of the shared prose, `--check` in
  `docs.yml` (2026-11-15)
- [x] `cargo xtask docs-examples [--check]`: every example run on the pinned public pack, its
  output compared with the checked-in output (D-322); Rust, Python, Node.js, WebAssembly, Go, Swift,
  Dart, Java, Kotlin, C, C++, .NET and the CLI; .NET in CI only (no local SDK), Kotlin run here
  with Gradle's cached compiler
- [x] A result `Reader` of one shape in every language (D-322): Node.js, WebAssembly, Go, Python,
  Swift, Dart, Java, Kotlin, C, C++ and .NET (D-322)
- [x] Every tour in CI: the bindings job of fast-check, an `examples` job (Rust, CLI),
  `wasm.yml`, `dotnet.yml`
- [x] The servers: `tgd serve` (every route, Nominatim compatibility, deployment) and
  `tgd mcp` (clients' configuration, tools, resources, installs); the HTTP examples run in the
  CLI tour against a live `tgd serve` (2026-11-15). Writing them found two server bugs, fixed:
  `/packs` gave bytes as `places`, and error bodies lacked the final newline

### 7.3 The reference
- [x] Generated from the IDL by `cargo xtask bindings`: every function, method, record, enum
  and error with its documentation and its signature in each language; `--check` fails on drift
  (D-323). Each language's declaration comes from the macro its binding is written with
  (`decl` in each template), so the page and the code cannot disagree
- [x] The HTTP API (routes, parameters, answers, errors) and the MCP tools from the servers'
  own definitions (D-323): `http::ENDPOINTS`, held to the parser by a test that fails on a
  parameter read but not listed, listed but not read, or marked required but not needed; the
  MCP tools and resource templates as `tools/list` gives them. `tests/reference.rs` renders
  both pages and fails on drift (`TG_BLESS=1` writes them); every example was run on the
  released pack

### 7.4 The boundary in the language API
- [x] Research ([language-boundary](../../02-research/notes/language-boundary.md)), then
  `Engine.boundary` (an outline as a GeoJSON Feature at a level of detail) and the distance
  between places, in every language, with parity values (D-324). One GeoJSON writer, moved
  from the server into the engine; `bearing_deg` added; every value proven checked by
  `parity-mutants` in eight languages; every tour gained a `boundary` section

### 7.5 Pages from the data and the gates
- [x] 7.5a Per-country pages from `data/countries/` and the released pack
  ([research](../../02-research/notes/docs-data-pages.md), D-325): `cargo xtask data-pages`
  writes the coverage page (components, every country's places) and a page per country folder
  (places, admin levels, heights, timezones, names, lexicon, sources and licences); checked in
  fast-check. The release's gate figures stay in its summary until the forge writes them as
  JSON (open item of the note)
- [ ] 7.5b Measured pages from the gates: sizes, benchmarks, accuracy, load

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
| 2026-11-15 | 7.1 and 7.2 done; the guides generated from one script; the playground link waits for 7.6; the reference page hand-written until 7.3 generates it | the guides share their prose, and a link must not point at nothing |
| 2026-11-15 | 7.3 done: the API reference generated from the bindings' own declaration macros, the HTTP and MCP reference from the servers' definitions | a second copy of each signature or route would drift (D-323) |
| 2026-11-15 | 7.5 split into 7.5a (country pages) and 7.5b (measured pages); 7.5a done | the country pages need only the pinned pack and the data folders; the measured pages need each gate to write a record (D-325) |
| 2026-11-15 | 7.4 done: boundary and distance in every language; the GeoJSON writer moved to the engine | the playground and every app draw outlines from the bindings, and one writer keeps them equal to the servers (D-324) |
