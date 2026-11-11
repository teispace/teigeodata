---
title: "Progress"
status: living
updated: 2026-11-11
---

# Progress

## Where we are

A short status. The numbers and evidence live in each phase file and the
[decision log](../03-decisions/log.md); this page links to them instead of repeating them.

| Phase | State | Evidence |
|---|---|---|
| 0 Foundations and research | **M0 met** 2026-11-07 (D-069) | [phase 0 §0.5](../07-planning/phases/phase-0.md#05-exit-review) |
| 1 Core and format | **M1 met** 2026-11-07 (D-080) | [phase 1 §1.6](../07-planning/phases/phase-1.md#16-exit-review) |
| 2 Indexes | **M2 met** 2026-11-07 (D-087) | [phase 2 §2.4](../07-planning/phases/phase-2.md#24-exit-review) |
| 3 Forge and Nepal data | in progress: 3.1–3.3 and 3.5 done; 3.4 DEM and 3.6 names in progress; 3.7–3.8 open | [phase 3](../07-planning/phases/phase-3.md) |
| 4 Engine, baseline and sync | started in parallel with Phase 3: 4.1 engine and 4.4 transport | [phase 4](../07-planning/phases/phase-4.md) |
| 5–8 | not started (bindings prototyped in S4, D-065 to D-068) | [roadmap](../07-planning/roadmap.md) |

- **Now (Phase 3):** the Nepal main pack has 100,358 places with heights (D-111), admin
  records linked (D-110), same-place merges (D-116), the first corrections (D-117), modelled
  unit populations (D-123), settlement rank (D-124) and a source's duplicates joined (D-125);
  golden on the product's search recall@1 0.859, pass 0.924; every pack carries its sources'
  notices (D-126); the `data-build` workflow runs G-det, G-lic and the golden gate per pack
  (D-128), `forge make` gates its budget (D-129), and the stages gate topology and place assignment
  (D-130), and conflation precision and recall (D-131); lite has heights from NASADEM (D-132); heights are their own ELEVATION section (D-133) and a place's timezone its own 444-byte PLACE_TZ section (D-134); every section belongs to one component and `check-pack` gates wire bytes per component (G-wire, D-135: points-only Nepal 522 kB); the engine opens a pack installed with any component set and names a missing one (D-136), 4.1b; a Cargo feature per component cuts the points-only engine to 60.9 KiB gzip (D-137); lite has timezones from tzdb (D-138), and G-tz checks them against GeoNames (D-139). Next: 3.7 (the lite build from clean-room sources
  only, `ACCURACY.md`). The CI data build needs the input mirror (D-127): the maintainer
  approves publishing the `sources` release, then `cargo xtask mirror` uploads about 1.45 GB (NASADEM added, D-132).
- **In parallel (Phase 4):** the engine (4.1, D-113, D-114; Nepal timings in
  [phase-4 benches](../02-research/benches/phase-4.md#nepal-main-pack)) and the transport
  (4.4, D-112) are on `main`; the product's search is at parity with the spike (D-121:
  recall@1 0.797, p50 6.7 µs); radius queries and the in-pack hierarchy (D-140), `Shared`
  snapshots (D-141), L1/L4/L5 measured to 10 layers and 1 GiB, open O(head) with block-hash
  lists proved on first read (D-142); `parent`, `ancestors` and `children` across packs by
  ancestor stubs (D-143), search contexts split-invariant; baseline sources researched
  (D-144); Nepal's baseline slice built by `forge baseline` (D-145: 53 rows, 159 kB main),
  level-1 units now under their country (D-146); the world part's countries and cities by
  `forge world` (D-147: 3.77 MB, 98.46% of cities answered by `admin_at`), the topology stage
  at world scale (D-148); the locator (`countries_near`, `missing_packs`) by distance on the
  stored outlines, no country missed over 109,659 border probes at 1 km, 11.9 µs a query
  (D-149); world admin 1 cut to the Nepal view (D-150: 4,571 units, world part 5.73 MB,
  cities under their unit); world timezones from the units, no geometry (D-151: 361 zones,
  30 kB, `tz_at` agrees with a city's own zone at 97.57% of cities); slices keep the names
  their world row would (D-152: Nepal's slice 73 kB, −54%); the baseline ships as one merged
  file (D-153: `forge embed`, 5.90 MB, the engine's `baseline` feature), and the world meets each
  pack country on its slice's line (D-154: every place answers as the layers do); main's
  world admin 0 and 1 come from Overture's OSM-derived divisions (D-155): the world's land
  thinned as one partition, units owned by the Nepal view where it differs by at least 15 km
  (D-156), islets under the tolerance's square left out at 2 km (D-157: 5.60 MB merged, 3,877
  units, locator radius 3 km, 0 probes missed), and a region ranked below the city of its name
  (D-158); thinning on sorted, banded records (122 s, 2.46 GB peak); the TUF client
  `sync::trust` (D-159), the catalog's `custom` schema v1 (D-160) and the differential against
  python-tuf (D-161), sizes that match G-wire and SHA-256 targets (D-162), and the repository
  tooling `xtask tuf` with the timestamp workflow, off until the first ceremony (D-163), and
  discovery: the engine as the catalog's `Resolver`, `Catalog::find` (D-164), and the
  differential against `tough` (D-165); 4.5: the layout from the head frame alone (D-166) and
  the sans-I/O installer that checks every block before writing it (D-167), and the
  `Transport` trait with its `ureq` default (D-168), and resume by frame with the staging file
  as its own record (D-169), and mirrors with demotion (D-170), and the store: atomic install, journal and
  recovery, crash-tested at every step (D-171); 4.6: the storage manager (quota, LRU eviction
  with pins and dependency protection, journal version 2) and the snapshot swap
  `Shared::follow` (D-172), and offline `verify(deep)` and repair by resuming over a copy
  (D-173); 4.7: updates reuse every unchanged frame of the replaced release (D-174); 4.8: the
  gate register, packs refuse an older release, and gate 12 measured red on a generated world
  (1.42 MB gzip, D-175); the catalog delegated by country, gate 12 green (largest role 220 KB
  gzip, D-176), both oracles agreeing over the country roles (D-177); 4.9: the CLI researched (D-178) and the catalog refresh `sync::refresh` with the timestamp as the commit point (D-179), `tgd`'s pack commands (D-180), its queries with native, GeoJSON and Nominatim output (D-181), its store commands from `init` to `remove`, end to end over HTTP (D-182), and its completions, guide and timings (D-183); gate 14's seam, installs in passes over the files a platform downloaded, with macOS `URLSession` keeping `Range` in the background and across a redirect (D-184), the iOS simulator surviving an app killed mid-download (D-185), and Android UIDT and the browser's OPFS doing the same (D-186): sync gate 14 green by hand; gate 8 with `tgd` itself killed mid-install, which made installs stream frame by frame (D-187); a world-size install whose block-hash list spans frames, run nightly (D-188); a partial install takes only its sections' space on disk, holes punched where APFS would fill them (D-189), Windows too, with a Windows CI job (D-190). Next: the two-release delta measurement needs two Nepal builds; the release upload and the first
  ceremony wait for the maintainer; the world byte-identity check after the thinning change
  waits for the inputs to be fetched again.
- **Size ideas to measure:** the timezone REGION (17% of the main pack) and the finest admin
  arcs ([ideas](../01-product/ideas.md)).
- **Waiting on the maintainer:** review of `data/countries/np/corrections/` (unreviewed), the labelled sample (Q-005, needed for `ACCURACY.md` and M3),
  the postal licence (Q-009), Copernicus for lite (Q-008).

## Session log

| Date | What happened | Next step |
|---|---|---|
| 2026-11-06 | Discussion and decisions (see the [decision log](../03-decisions/log.md)). Repository initialised; AGENTS.md, conventions, vision, principles, scope, 26 ADRs, roadmap, Phase 0 plan. Research agents launched: Nepal sources, source licences, format/compression/indexing, bindings and MCP, names and search, timezone and elevation. | Finish 0.1; collect the research notes |
| 2026-11-06 | Foundation files, workspace, xtask gates and hooks; bindings and format research applied; ADR-0027 (JNI), ADR-0028 (MCP baseline), ADR-0029 (1e-7° grid) | Commit, create repo, apply the other notes |
| 2026-11-06 | Spike S2 made exact (3 bugs found by set-based differential testing; source overlaps found, Q-013); leaf-size sweep; forge, offline-loading and competitive notes landed; benchmarking research and catalogue (D-042) | Push, apply the new notes, then S3 |
| 2026-11-06 | Lexicon and data checks; per-country folders and profiles (P16); world-general search fold; golden set v0 for Nepal (reproducible) | S3 search spike |
| 2026-11-06 | Spike S3: recall gates met on Nepal, p99 depends on load (D-052); own Damerau automaton about 10× faster than the `fst` DFA; phonetic key at low weight; pkey pair sets; COD-AB join bug fixed and golden set regenerated (512 queries) | S5 baseline accuracy |
| 2026-11-07 | S5 done: matcher, aliases into search (S3 recall@1 0.959), DEM tilesets and `forge dem` (S5.6), the summary and accuracy targets (S5.7, D-058 to D-062) | S6 elevation validation |
| 2026-11-07 | S6.1–S6.2: elevation-reference research; POST tilesets with response checks; SlideRule decoder; EGM2008 geoid; `forge dem-ref`: Copernicus LE90 18.7 m in Nepal (D-063) | S6.3 GEDTM30 |
| 2026-11-07 | S6.3: NASADEM via Planetary Computer tokens and EGM96; paired comparison with Copernicus (D-064); GEDTM30 circularity recorded (Q-016) | S4 bindings |
| 2026-11-07 | S4: safe API crate, syn IDL, generated C ABI, header, Swift, Dart, Python, Node and JVM layers; parity in all; benchmarks; cbindgen and lint-table gates; `dist` unwinds (D-065 to D-068); ASCII fold fast path | M0 review |
| 2026-11-07 | CI's binding suites green on Linux (Node test path fixed); M0 review (D-069); Phase 1 research note and plan: crate split, TLV manifest, stable ID text form, bolero, REDIRECTS (D-070, D-071) | 1.1 `teigeodata-core` |
| 2026-11-07 | 1.1 `teigeodata-core`; 1.2 `teigeodata-format` (format 0.1): reader, writer, signatures, fuzz properties; a writer bug found by the round-trip harness; bolero dropped (D-072) | 1.3 benchmarks |
| 2026-11-07 | 1.3 benchmarks (L1–L3, core); B1 integer codec and B13: 128-value blocks, safe decoder 8.5–11 G/s, beats `bitpacking`; 1024 layout dropped (D-073) | B1 sections, then B5 |
| 2026-11-07 | B1 on sections: column table, PLACES, HIERARCHY, `PlaceKind`; WASM decode 4 G/s; all B1 targets met (D-074) | B5 IDMAP and REDIRECTS |
| 2026-11-07 | B5: research note, IDMAP and REDIRECTS, ramp coding, interpolated search; PtrHash compared out of tree (D-075) | B6 rank and select |
| 2026-11-07 | B6 COMPONENTS, B3 NAMES, B12 BLOCK_HASHES, B4 BLOBS (D-076 to D-079); 1.5 nightly fuzz and Miri, the counts gate, Z2 sizes; M1 met (D-080) | Phase 2 research |
| 2026-11-07 | Phase 2: indexes note (D-081); B8 PLACE_INDEX (D-082); B9 REGION, Q-013 answered (D-083, D-084) | B7 SEARCH |
| 2026-11-07 | B7 SEARCH: own FST, bit-parallel NFA on code points, FOR lists by importance, block maxima; 1.50 MB, p99 41.2 µs unanchored (D-085, D-086); M2 met, POSTAL to Phase 3 (D-087) | Phase 3 research |
| 2026-11-07 | Phase 3 research: geometry (D-088), DEM (D-089), postal and Q-001 (D-090), settlement core and building licences (D-091); plan and order (D-092) | 3.1 vertical slice |
| 2026-11-07 | 3.1: `forge build` writes, signs and reads back the Nepal pack (96,844 places, 7.05 MB, 3.0 s, deterministic); lite 4.26 MB; `interior_point` for admin points; REGION place link (D-093) | Stage cache, then 3.2 |
| 2026-11-07 | 3.1: the ID registry (D-094): lite keeps 91,153 of 91,266 main IDs, a rebuild keeps all, retirement writes REDIRECTS | Stage cache, then 3.2 |
| 2026-11-07 | 3.1 done: `forge make` and `build.yaml`, content-keyed stages with stamped outputs (D-095) | 3.2: building readers, settlement core, consensus |
| 2026-11-08 | 3.2: building readers, `forge core` with a fatal agreement gate (P90 146 m) proved on a planted mismatch, the border rule, `forge labels` and the 301-place sample (D-096) | 3.3: B10 geometry |
| 2026-11-08 | 3.3: ARCS (a518086), POLYGONS (07cbe5b), topology stage and pack geometry (faebfbc); D-097, D-098 | POLY_INDEX, admin REGION A/B |
| 2026-11-08 | 3.3: POLY_INDEX on the shared tree, in the packs; B10 page (D-099) | admin REGION A/B, coarse LOD reads |
| 2026-11-08 | 3.3: ADMIN_REGION on the shared arcs, cross-checked in the forge (D-100) | per-pack top grid, coarse LOD reads |
| 2026-11-08 | 3.3: fine grid (D-101), ARCS_OVERVIEW (D-102) | the India measurement |
| 2026-11-08 | 3.3 done: snap rounding in the topology stage (D-103), `forge topology --geoboundaries`, India measured | 3.4: B11 DEM |
| 2026-11-08 | 3.4: the DEM section and `forge dem-section` (D-104) | the 45-tile download (approval) |
| 2026-11-09 | 3.5: POSTAL on place runs, Nepal codes in the main pack (D-105); CI fixes for the `no_std` build and a debug overflow | 3.6 names and corrections |
| 2026-11-09 | 3.6: display names, the romaniser and its Nepali profile (D-106) | D-060 alias rules in the build; corrections |
| 2026-11-09 | 3.6: D-060's alias rules in the build (D-107); stage keys by code and a `fast` profile (D-108); DEM tiles fetched | pack evaluator; duplicate clusters; corrections |
| 2026-11-09 | 3.4: place elevations from the DEM at the consensus point, the `dem` stage in `forge make` (D-111) | the DEM component in the packs; lite elevation |
| 2026-11-09 | D-110 admin records, D-111 place elevations merged; development speed-ups (D-115): Phase 4 started in parallel (engine, transport), build split into modules, Miri under nextest, this page shortened | Duplicate clusters and G-conf; merge the Phase 4 branches |
| 2026-11-09 | 4.2: the baseline slice of a pack (`extract`, `forge baseline`, G-base, D-145); level-1 units under the country (D-146) | the world part of the baseline; the locator |
| 2026-11-10 | 4.2: the world part of the baseline (`forge world`, Natural Earth admin 0 and `cities15000`, D-147); world-scale label search (D-148) | world admin 1 and timezones; the locator |
| 2026-11-10 | 4.2: the locator by distance on the stored outlines (D-149); world admin 1 cut to the Nepal view (D-150); a 12 GB memory blow-up in G-topo's parent check found and fixed, memory now watched by footprint (AGENTS.md §7) | world timezones; the shipped baseline and the 8 MB gate |
| 2026-11-10 | 4.2: world timezones from the units and the cities' zones (D-151); `extract` keeps PLACE_TZ; the locator's chunk index behind an `alloc` feature (the no_std build) | the shipped baseline and the 8 MB gate |
| 2026-11-10 | 4.2: slice names cut to the country's languages and English, SEARCH rebuilt from them (D-152): Nepal's slice 157,532 → 72,988 B | the shipped baseline (layers, `baseline` feature) and the 8 MB gate |
| 2026-11-10 | 4.2: the baseline as one file (D-153): `extract::merge`, `forge embed` with Z1, the engine's `baseline` feature; layers measured and rejected (search 34×, reverse 98× at 200 slices) | cut the world's neighbours to pack outlines; 4.3 TUF trust |
| 2026-11-10 | 4.2: the world part meets each pack country on its slice's line (D-154): neighbours cut, gaps given, arcs pinned; the shipped-baseline gate strict (0 of 39,395 differ) | 4.3 TUF trust |
| 2026-11-10 | 4.2: main's world part built from Overture (D-155 to D-158): ownership by the view, islets and 2 km to fit Z1, locator radius 3 km, region below its city; every world, locator, Z1 and golden gate green | snap rounding on sorted records; 4.3 TUF trust |
| 2026-11-11 | 4.8: installs stream frame by frame with a SIGKILL gate (D-187); the world-size install, its list over two frames, cut and resumed, nightly (D-188); 4.1b: partial installs punch holes, points only 1.50 of 2.85 MB on disk (D-189) | two-release delta (needs Nepal builds); M4 exit review |
