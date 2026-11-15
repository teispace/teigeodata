---
title: "Phase 7: docs site"
status: accepted
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
- [x] 7.5b Measured pages from the gates (D-326): `package-wasm`, `sizes`, `load` and the new
  `phone` command write a stamped record with `--record`; `cargo xtask data-pages` renders
  them into `data/measured.mdx` and checks it. Accuracy stays on its page and in each
  release's summary until the forge writes its gate figures as JSON

### 7.6 The playground
- [x] The WebAssembly build over the public layer, in the browser, with MapLibre drawing the
  pack's own outlines: search, reverse, admin, timezone, elevation, nearby, boundary (D-328;
  [research](../../02-research/notes/docs-playground.md)). `cargo xtask playground` writes its
  files; `site/scripts/check-playground.mjs` runs it in Chromium in the docs workflow. Building
  it found world timezone areas answering no zone (D-327, fixed in the engine)

### 7.7 Exit review
- [x] M7 criteria met (2026-11-15, D-330), each against the [roadmap](../roadmap.md):
  - **Fumadocs site:** Fumadocs 16.19.3 on Next.js 16.4.6, a static export of 39 pages under
    `/teigeodata/` with static search, `llms.txt` and per-page Markdown; every build checks its
    1,844 internal links and holds the search index to gzip 300 KiB (291,390 B, D-329); the
    deploy job is ready behind `DOCS_DEPLOY`; `docs` green at `c1236d8` (run 43669973165);
  - **guides per language:** eleven languages and the command line, written by
    `site/scripts/guides.py` from one copy of the shared prose (`--check` in `docs`); every
    sample is a region of a tour that ran on the released pack and every language prints the
    same output (D-322), in CI: fast-check's bindings and examples jobs, `wasm`, `dotnet`;
    writing them found and fixed a missing result reader in eight languages (D-322), the
    boundary missing from the language API (D-324) and two server bugs;
  - **generated reference:** the API from each binding's own declaration macro, the HTTP and
    MCP reference from the servers' definitions, each failing on drift (D-323);
  - **per-country pages:** the coverage page and a page per country folder from the pinned
    pack and `data/countries/`, checked in fast-check (D-325); the measured page from the
    gates' stamped records (D-326);
  - **playground:** the commit's WebAssembly engine on the pinned pack, a map of the pack's own
    outlines, every answer with its code and timing, nothing sent anywhere; run in Chromium in
    `docs` against the tours' answers (D-328), which found world timezone areas answering no
    zone (D-327, fixed in the engine).
- [x] Not blocking M7, recorded:
  - the site is not public until the maintainer enables GitHub Pages (source: GitHub
    Actions) and sets `DOCS_DEPLOY` to `true`; the registry install tabs wait for the first
    release (5.10c);
  - the .NET tour runs only in CI (no local SDK, D-299);
  - a release's gate figures (G-pip, G-tz, heights QA) stay in its summary until the forge
    writes them as JSON ([data pages](../../02-research/notes/docs-data-pages.md) §7);
  - the first measured records were taken under a load average of 7 to 25 from other
    projects' builds, shown beside each; rerun on a quiet machine before a release;
  - the playground loads 16.3 MB; a smaller playground pack once a release names one
    ([playground](../../02-research/notes/docs-playground.md) §7).

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Phase 7 drafted from the docs-site research | M6 met (D-321) |
| 2026-11-15 | A reader in every language added to 7.2 | The tours found most bindings cannot read a result (D-322) |
| 2026-11-15 | 7.1 and 7.2 done; the guides generated from one script; the playground link waits for 7.6; the reference page hand-written until 7.3 generates it | the guides share their prose, and a link must not point at nothing |
| 2026-11-15 | 7.3 done: the API reference generated from the bindings' own declaration macros, the HTTP and MCP reference from the servers' definitions | a second copy of each signature or route would drift (D-323) |
| 2026-11-15 | 7.5 split into 7.5a (country pages) and 7.5b (measured pages); 7.5a done | the country pages need only the pinned pack and the data folders; the measured pages need each gate to write a record (D-325) |
| 2026-11-15 | 7.4 done: boundary and distance in every language; the GeoJSON writer moved to the engine | the playground and every app draw outlines from the bindings, and one writer keeps them equal to the servers (D-324) |
| 2026-11-15 | 7.5b done: the measured page rendered from the gates' records; `cargo xtask phone` | a number on the site needs its date, machine and commit, and one renderer (D-326) |
| 2026-11-15 | 7.6 done: the playground, its files from `cargo xtask playground`, its browser check in the docs workflow; the WebAssembly tools one composite action | the page runs the commit's engine from the site's own origin; the pack cannot come from the release (D-328); a timezone area is its own zone (D-327) |
| 2026-11-15 | 7.7: M7 met; the search index held to its budget | the exit review measured the index over the budget the research set (D-329, D-330) |
