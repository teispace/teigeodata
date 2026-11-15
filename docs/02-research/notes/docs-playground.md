---
title: "The playground of the docs site"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# The playground of the docs site

## 1. Question

Phase 7.6: a page where a reader tries every query on a released pack in the browser, with a
map, before installing anything. The [docs-site note](docs-site.md) §2 fixed the outline: the
`@teispace/teigeodata-wasm` build over the public layer, opened with `Engine.openBytes`, MapLibre
drawing results over a map made from the pack itself, nothing sent anywhere. This note settles
what that leaves open: where the browser gets the pack and the engine, how MapLibre runs in a
static export without a tile or glyph server, what the page shows, and what proves it works.

## 2. Summary and recommendation

- **The site carries its own copies** of the engine (the full WebAssembly package, built from
  the same commit) and of the pinned pack (`site/data.json`, checked by SHA-256), written to
  `site/public/playground/` by one command, `cargo xtask playground [--fetch]`. A release asset
  cannot be fetched from a page: GitHub answers its download URL with a redirect that carries no
  `Access-Control-Allow-Origin` (measured below), and the WebAssembly package is not on npm
  until the first release (5.10c).
- **The engine is loaded at run time from those files** (`import()` of the package's own
  `index.js`, which loads `teigeodata_wasm_bg.wasm` beside it), not bundled: the page runs the
  exact files a user installs, and the 254 kB module is fetched only by the playground.
- **The map is drawn from the pack.** MapLibre GL JS 6.18.0 with a style of no external source:
  a background, every country's outline from `byIso` and `boundary` (237 countries, 283,408
  vertices, built in 80 ms on Node.js, measured below), the selected place's outline, and the
  results as circles. No labels, because labels need a glyph server; names are in the panel and
  in popups. MapLibre's worker is copied beside the engine and named with `setWorkerUrl`, so it
  is a file of the site, not a `blob:` built from the bundle's URL.
- **Nothing leaves the page.** Every request is to the site's own origin; the check below fails
  on any other.
- **The pack is loaded on request**, with its size on the button (15.6 MB), a progress bar, and
  its SHA-256 checked in the browser (`crypto.subtle.digest`) against `site/data.json` before it
  is opened; the engine then checks every section's BLAKE3 as it reads it.
- **What the page shows:** search as you type (with the map's centre as focus, optional), a
  point's admin units, timezone with its offset now, elevation and nearest places (click the map),
  and a place's record, ancestors, children, nearby places and outline. Each panel shows the
  calls that produced it, as code to copy, and how long they took. The state is in the URL hash
  (`#q=`, `#place=`, `#at=`), so a view can be linked.
- **Proof it runs:** `site/scripts/check-playground.mjs` serves the built site, opens the
  playground in headless Chromium (Playwright 1.65.1, the version the WebAssembly browser tests
  pin), loads the pack, checks a search, a place and a point against the tour's answers, and fails
  on a console error or a request to any other origin. The docs workflow runs it on every build.

## 3. Findings

- **Release assets and CORS (2026-11-15):** `curl -I -H 'Origin: https://teispace.github.io'`
  on `releases/download/data-2026.11.14.3/world-np-lite.tgd` answers `302` with no
  `Access-Control-Allow-Origin`, then `200` from `release-assets.githubusercontent.com`. A
  browser blocks the redirect, so the pack must be on the site's origin.
- **GitHub Pages:** a file may be up to 100 MB and a site 1 GB (docs-site note); the pack is
  15,641,408 B. Pages' soft bandwidth limit is 100 GB a month, about 6,400 loads of the pack; the
  load is on request, never on page view.
- **The world outline (measured on Node.js 26, `target/wasm/full`, 2026-11-15):** `openBytes`
  10.6 ms; `byIso` over every alpha-2 code finds 237 countries, every one with an outline;
  `boundary(id, 6)` for all of them: 283,408 vertices, 7.07 MB of GeoJSON, 80 ms. Levels 4, 5
  and 6 answer the same vertices: this lite pack stores one level of detail. `byIso` rejects a
  code ISO 3166 does not assign (`InvalidArgument`), so the page asks only codes the package
  accepts and treats that error as "no such country".
- **Building it found a bug (D-327):** `tzAt` at New York answered the `America/New_York` area
  with `tz` null. The world pack leaves the timezone areas' own `PLACE_TZ` rows empty and the
  engine read only `PLACE_TZ`; the tours ask in Kathmandu, where Nepal's pack records its area,
  so no check saw it. A timezone area is now its own zone.
- **Timing in a browser:** `performance.now()` is coarsened to 0.1 ms without cross-origin
  isolation, and most queries take microseconds; the page repeats a call faster than 1 ms for
  about 3 ms and shows the mean (queries only read).
- **The u64 columns arrive as `BigInt`** (`vertices`), as the Reader documents; the page converts
  them where it does arithmetic.
- **MapLibre GL JS 6.18.0** (npm, published 2026-11-11, BSD-3-Clause): ESM only
  (`dist/maplibre-gl.mjs`, 296 kB gzip -9; the worker `maplibre-gl-worker.mjs`, 147 kB, self
  contained; CSS 10 kB). It finds its worker beside `import.meta.url` unless `setWorkerUrl`
  names one; once bundled, that file is not beside the chunk. Labels (`symbol` layers with
  `text-field`) need `glyphs`, a font server.
- **Bundling the engine:** the package's `init()` loads the `.wasm` beside its own module. A
  bundler would have to copy that file and rewrite the URL; loading the package's own files at
  run time with `import(/* webpackIgnore: true */ url)` avoids both, and keeps the module out of
  every other page.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Copy the engine and the pack into the site at build time, checked | Same origin; the engine of the commit; works offline once loaded | The docs job builds the WebAssembly package (Rust, wasm-bindgen, binaryen) | **Chosen** |
| Fetch the pack from the release | No copy | Blocked by CORS (§3) | Rejected |
| Engine from npm or a CDN | No Rust in the docs job | Not published yet; a CDN is a third party | Rejected until 5.10c; a CDN never |
| Check the built package into git | No build | Binary drift on every API change | Rejected |
| A raster or vector basemap (OpenStreetMap tiles, a hosted style) | Familiar map | A third party sees every view; tile usage policies; not our data | Rejected |
| The engine in a Web Worker | The page never waits on a query | Queries take micro- to milliseconds (phone bench: keystroke p99 0.2 ms here); a message per query costs more than most queries | Not now; revisit if a query blocks |

## 5. What we build ourselves, and why

- `xtask/src/playground.rs`: `cargo xtask playground [--fetch]` builds the full package (the
  `package-wasm` code path), takes the pinned pack (the `docs-examples` code path, SHA-256
  checked), and writes `site/public/playground/` (git-ignored): the package's browser files, its
  `LICENSE` and `NOTICE`, the pack, MapLibre's worker, and `manifest.json` (release, file, bytes,
  SHA-256, package version).
- `site/app/(home)/playground/` and `site/components/playground/`: the page.
- `site/scripts/check-playground.mjs`: the browser check.
- `.github/actions/wasm-tools`: the wasm32 target, wasm-bindgen and binaryen as one composite
  action, used by the wasm and docs workflows instead of two copies.

## 6. Impact on the plan

7.6 as planned; the docs workflow gains Rust, the WebAssembly tools and Chromium, and runs on
changes to the crates and bindings too, since the playground runs them.

## 7. Risks and open items

- The pack is 15.6 MB; a phone on a slow link waits. A smaller playground pack (the baseline with
  one country in full) is possible once releases name one; the button states the size.
- When the WebAssembly package is on npm (5.10c), the site could depend on it like any app; the
  copy from the commit stays, because the playground should run the code it documents.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| maplibre-gl | 6.18.0 (2026-11-11) | 2026-11-15 | npm |
| playwright | 1.65.1 (2026-11-14) | 2026-11-15 | npm |
| Pinned pack | data-2026.11.14.3 `world-np-lite.tgd` | 2026-11-15 | `site/data.json` |

## 9. Sources

- [docs-site research](docs-site.md) §2 (the playground's outline) and §3 (GitHub Pages limits).
- MapLibre GL JS 6.18.0, `dist/maplibre-gl.mjs` (worker URL resolution, `setWorkerUrl`) and its
  style specification (`glyphs` for `text-field`).
- [Node and WebAssembly packaging](node-wasm-packaging.md): the package's files and `init()`.
- GitHub Docs, "About GitHub Pages": usage limits.
