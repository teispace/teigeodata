---
title: "The playground of the docs site"
status: accepted
updated: 2026-11-16
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
- **The map is Leaflet 1.9.4 over OpenStreetMap's tiles** (revised 2026-11-16, D-356; §10):
  the maintainer asked for a familiar map that pins what a search or a click finds. A layer
  switch keeps the first design available: "Pack only" draws every country's outline from the
  pack with no tiles at all. Results, the pin, outlines, measured lines and radii are SVG with a
  class, so the site's theme colours them through CSS.
- **Only tiles leave the page.** Every other request is to the site's own origin; the check
  stubs `tile.openstreetmap.org` and fails on any other host. With "Pack only", nothing leaves.
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
| A raster or vector basemap (OpenStreetMap tiles, a hosted style) | Familiar map | A third party sees every view; tile usage policies; not our data | Rejected 2026-11-15; **chosen 2026-11-16** with a pack-only switch (§10) |
| The engine in a Web Worker | The page never waits on a query | Queries take micro- to milliseconds (phone bench: keystroke p99 0.2 ms here); a message per query costs more than most queries | Not now; revisit if a query blocks |

## 5. What we build ourselves, and why

- `xtask/src/playground.rs`: `cargo xtask playground [--fetch]` builds the full package (the
  `package-wasm` code path), takes the pinned pack (the `docs-examples` code path, SHA-256
  checked), and writes `site/public/playground/` (git-ignored): the package's browser files, its
  `LICENSE` and `NOTICE`, the pack, and `manifest.json` (release, file, bytes,
  SHA-256, package version).
- `site/app/(tool)/playground/` (a layout with no footer, so the map fills the window) and
  `site/components/playground/`: the page.
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
| leaflet | 1.9.4 (latest stable; 2.0.0-beta.4 not stable) | 2026-11-16 | npm |
| @types/leaflet | 1.9.22 | 2026-11-16 | npm |
| playwright | 1.65.1 (2026-11-14) | 2026-11-15 | npm |
| Pinned pack | data-2026.11.14.3 `world-np-lite.tgd` | 2026-11-15 | `site/data.json` |

## 9. Sources

- [docs-site research](docs-site.md) §2 (the playground's outline) and §3 (GitHub Pages limits).
- OpenStreetMap Foundation, Tile Usage Policy, <https://operations.osmfoundation.org/policies/tiles/>,
  read 2026-11-16.
- Leaflet 1.9.4 reference, <https://leafletjs.com/reference.html>.
- [Node and WebAssembly packaging](node-wasm-packaging.md): the package's files and `init()`.
- GitHub Docs, "About GitHub Pages": usage limits.

## 10. Revision 2026-11-16: Leaflet over OpenStreetMap's tiles

The maintainer asked for OpenStreetMap and Leaflet, with a pin where a search or a click lands,
and for a full-screen, full-featured playground (D-356). What changed and why:

- **Leaflet 1.9.4 replaces MapLibre GL 6.18.0.** Raster tiles need no WebGL, no worker file and
  no glyph server; the bundle loses MapLibre's 296 kB module and its 147 kB worker, and
  `cargo xtask playground` no longer copies a worker. Leaflet is loaded by `import()` only on
  this page.
- **The OpenStreetMap tile policy** (read 2026-11-16) asks for: visible attribution (bottom
  right, never under the panel; the panel's footer credits it too), a valid Referer (the site
  sets no Referrer-Policy, so browsers send the origin), caching by the server's headers (the
  browser's cache), no bulk download or prefetch (Leaflet loads only the tiles in view), and no
  no-cache headers (none sent). A playground's traffic is light; a hosted tier would use its
  own tile source.
- **Privacy:** the tile server sees which tiles a viewer loads, so the intro says so, and the
  "Pack only" basemap draws the pack's own country outlines with no tiles, as before.
- **Dark mode** inverts the tiles with a CSS filter (`.dark .tg-map-tiles`), as tile styles
  have no dark variant on the standard server.
- **The page:** a full-bleed map; a floating panel (search, point, place, measure, radius) that
  becomes a bottom sheet on a phone; tools Inspect, Measure and Radius; a layers card (basemap,
  country outlines, the outline's level of detail, fill, stroke, children, lines to the nearest
  places); a code dock with each call and its time; settings kept in `localStorage`; links by
  hash (`#q=`, `#at=`, `#place=`). Fits and flights leave room for the panel.
- **Proved:** `check-playground.mjs` stubs the tiles and requires Leaflet's container with tiles
  drawn; with tiles never added it fails ("the map is drawn (Leaflet, with tiles)").


## 11. Revision 2026-11-16: packs at run time, a card on the map

The maintainer asked for a click's answer on the map itself, any pack downloadable while the
playground runs, a more professional location and nearby dots, and a real recording of the
playground on the landing page (D-357). What was built, measured and why:

- **Where a pack can come from.** A release asset cannot be fetched by a page (§3: no CORS
  header on the redirect), so the site hosts copies, as it does for the pinned pack. Hosting
  Nepal's full lite pack (87.5 MB, 68.4 MB of it terrain) for every visitor was rejected; the
  options compared were (a) publishing smaller packs as a new data release, (b) cutting them at
  build time with `tgd extract` from a pinned asset. (b) won: no release for a page, and the
  cut is **deterministic** (the same SHA-256 on every run, measured twice), so
  `site/packs.json` pins each output and `cargo xtask playground` fails on any difference
  (proved: a changed digit fails it).
- **Terrain by region needed a fix first.** `tgd extract --bbox` copied the whole terrain grid
  (the Kathmandu Valley out of Nepal's pack weighed 70.1 MB); it now keeps the tiles that touch
  the box, codes copied as stored (`dem::mask`), so heights inside are exact: 1.5 MB for the
  valley with its places, 466 kB for its terrain alone, the same height at the centre and
  corners as the full pack (1,310.9 m at 27.7172, 85.3240). A component extract of a pack with
  parts or block hashes failed ("a section kind was given twice") and now keeps every part.
- **The catalog (2026-11-16):** Nepal's places of `data-2026.11.15` without terrain
  (18,985,344 B: 95,716 places with population, search inside words, outlines at four levels of
  detail) and 31 m NASADEM terrain for the Kathmandu Valley (466,244 B), Pokhara and Annapurna
  (1,254,560 B), Everest and Khumbu (1,518,048 B), Chitwan (668,864 B) and Lumbini
  (138,964 B). The lite Nepal pack has no postal section; the card shows a postal code only
  where a pack has one.
- **Adding a pack:** download with progress, SHA-256 in the browser, kept in Cache Storage
  (keyed by file and digest, so a new cut is a new key) and opened with
  `openBytesWith(bytes, { priority: 1 })`: the engine's precedence (developer priority first)
  makes the added pack answer where both hold a place; `remove(layer)` takes it out, and
  "forget" drops the kept copy. On the next visit, kept packs open from the store before the
  first answer, with no download (the check counts one request). Measured locally: the
  terrain opened in 14 ms, Nepal's 19 MB in 143 ms; from the store, ready in 1.9 s with both.
- **The card on the map** is Leaflet's popup restyled as the site's frosted surface, its content
  rendered by React through a portal: the nearest place with distance and direction, the units
  holding the point, the height with its grid and source, local time and offset, a postal code
  where a pack has one, the coordinates to copy, and, where the base pack answers from the
  464 m grid and the catalog has terrain, an offer to add it. A place picked on the map opens
  in a card where it is, without moving the map. A card waits for a flight to end, so it is
  placed where the map stops, clear of the panel and the tool bar.
- **Flights:** Leaflet scales vector layers with the map during `flyTo`, so a dot became a
  blurred disc many times its size; the drawing now fades out for a flight and back in, sharp.
  Search fits the map to its results once typing pauses (350 ms), not on every keystroke.
- **Location** draws a dot inside a circle as wide as the fix's accuracy and opens the card
  there; a refusal says how to allow it. Dots are sized by kind (country and unit largest,
  locality smallest, natural features green) with their names beside them while 12 or fewer.
- **Proved:** `check-playground.mjs` now also requires the card at the tours' point with the
  tours' height, adds the valley's terrain from the card and requires the 31 m grid to answer,
  then reloads and requires the pack to answer again with no second download. With the store's
  lookup broken the check fails ("the added pack was not opened again after a reload").
- **The landing page's recording:** `site/scripts/video.mjs` drives the built playground in
  Playwright with a drawn cursor (a search, a place, a click and its card, terrain added, a
  radius, a measured line), records it, trims the loading, crops below the site's bar and
  encodes 1280×750 H.264 (CRF 26) and VP9 (CRF 36), about 2 MB per theme for 26 s, with a
  poster from the first frame. The page loads only the theme's video, only near the screen,
  pauses it off screen, never starts it with Reduce Motion, and has a pause button (WCAG 2.2.2).
  It replaces the screenshot and `shots.mjs`.
