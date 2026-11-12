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
| 3 Forge and Nepal data | in progress: 3.1–3.3 and 3.5 done (3.5 lite codes wait on Q-009); 3.4 DEM and 3.6 names in progress; 3.7–3.8 open | [phase 3](../07-planning/phases/phase-3.md) |
| 4 Engine, baseline and sync | in progress; now 4.9c, the offline-first embedded layer for Tithipatra (D-213); everything separable done (D-255) | [phase 4](../07-planning/phases/phase-4.md) |
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
- **First consumer brief (Tithipatra, D-193, [reply](../01-product/consumers/tithipatra.md),
  phase 4 §4.9b):** done: a place's box and outline (TG-04, D-194), a height names its DEM
  product, spacing and datum (TG-07, D-195), zone rules from the outlines' own tzdb release
  (TG-03, D-196), ISO 3166 codes (TG-02, D-197), the release policy and the `v0.1.0` tag
  (TG-01, D-198), zones before 1970 researched (TG-08: backzone adds nothing for South Asia).
  Point → postal code in the engine and `tgd postal` (3.5, D-200); substring search (TG-06,
  D-199: Nepal SUBSTRING 2.56 MB, golden substring class 9 of 9 at rank 3). Waiting: Nepal and
  India data releases (TG-05, maintainer steps).
- **Terrain (D-192):** packs carry the DEM masked to their units and the tiles around every
  place with a height; `elevation_at` and `tgd elevation` answer any point; Nepal main DEM
  64.2 MB of a 78.8 MB pack, points-only download 536 kB. Summits are `natural` places with
  their surveyed heights where they agree with the DEM (D-201: 2,726, Everest 8,849 m); names
  carrying their context are dropped (D-202). Lite: 80.2 MB pack (NASADEM DEM 68.4 MB),
  points-only download 501 kB, golden recall@1 0.829, 210 s and 1.56 GB peak to build.
- **Split builds (D-191):** `forge make <cc> --split` writes one pack per province with its
  ancestor stubs, proved against the country pack; Nepal 7 parts, 1.16× the pack. Tiles for
  India are W1.
- **Size ideas to measure:** the timezone REGION (17% of the main pack) and the finest admin
  arcs ([ideas](../01-product/ideas.md)).
- **Maintainer answers (2026-11-11, D-203 to D-212):** mirror approved; lite keeps NASADEM;
  postcodes main only until Nepal Post opens them; lite claim area digitised from the 2020
  map; ODbL comparisons allowed as gates; corrections approved; labels delegated to agents;
  full key ceremony; all registry names reserved (the maintainer signs in); GitHub private
  reporting; the browser host deferred, GitHub Releases only (D-212). **Waiting on the maintainer:**
  the key ceremony, registry sign-ins (later, before the first public release), sending the Nepal Post request.

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
| 2026-11-11 | Tithipatra's brief planned (D-193); TG-04 extent (D-194), TG-07 height source (D-195), TG-01 release policy (D-198); four research notes (ISO codes, zone rules, substring search, zones before 1970) by parallel agents; zone rules and ISO codes being built in worktrees | Merge TG-03 and TG-02, tag v0.1.0, TG-06, then 3.5 |
| 2026-11-11 | TG-03 and TG-02 merged, `v0.1.0` tagged (D-198); 3.5 point → postal code in the engine and `tgd postal` (D-200) | TG-06 merge, 3.4 summit heights |
| 2026-11-11 | TG-06 merged (D-199); NASADEM fetched upstream when no mirror; 3.4 DEM section sized on Nepal and summit heights (D-201); context-carrying names dropped (D-202); golden baselines regenerated, lite built and gated | 3.7 |
| 2026-11-11 | Maintainer answers D-203 to D-212; data build green after the world test reads the pack country's slice; Sentinel-2 chips for labels (D-209), cloud checked per chip; Tithipatra's addendum 1 accepted ahead of labels (D-213); TG-11 done: `cargo xtask pack-sizes`, [sizes](../05-data/sizes.md): Devanagari names on 9.1% of Nepal's places; SUBSTRING derives on the device byte for byte in 0.38 s | 4.9c: everything separable (names by language, outlines by level), derive SUBSTRING on the device, TG-12 profile, TG-14 Nepali names |
| 2026-11-11 | TG-12 profiles (D-214), D-215 to D-218 (Nepali names, India, SUBSTRING derived on the device); TG-13 done (D-219): 69,972 world cities of 5,000, each height marked `surveyed` or `model`; the `tithipatra` layer is 170,892 places, 8.28 MB on the wire; Devanagari-only `ne-Deva` names in profiles; the Brahmic search fold of nasal conjuncts, vowel length and candra vowels (D-220, golden unchanged at recall@1 0.829); Wikidata `ne`, then `hi`, labels on 13,724 world places (D-221), the layer 8.69 MB on the wire; the transliterator (TG-14) being built in a worktree | merge the transliterator, origin marker, transliterated names in packs; TG-16 outlines; TG-19 reverse |
| 2026-11-12 | Rebuilt packs and layers after `xtask clean` removed them; plain clean keeps `data/build` now; TG-18: focus point and strict order in search (D-226) | transliterator (TG-14) and coarse DEM (TG-10) merge, then TG-16 |
| 2026-11-12 | Merged the transliterator (D-222) and the coarse DEM (TG-10, D-225); name origins (D-227) with `name_in`; Nepal's names filled by transliteration, 1.1% → 99.0% (D-228); `tithipatra` 10.30 MB wire | TG-16 outline tolerance (back under 10 MB), TG-14 CBS official names, TG-17 |
| 2026-11-12 | TG-16 done: outlines snapped to their level's decimal grid (D-229), 64-edge admin-index leaves (D-230), the shipped baseline compiles zone rules again (D-231); `tithipatra` 10.30 → 9.21 MB wire, baseline 5.00 MB | TG-14 CBS official names and Wikidata `ne` labels for Nepal's places, TG-17 phone benches |
| 2026-11-12 | TG-14 tier 1: official names by code from CBS and the federal list (D-232), 42 districts and 715 local levels, an alignment gate that caught the CBS district file's shifted Nepali column; towns named like their unit take its official name; golden r@1 0.829 → 0.835; `tithipatra` 9.21 MB wire | G-xlit `cbs` set without the shifted rows, districts 408+ and renamed units from a sound source, TG-17 phone benches |
| 2026-11-12 | D-233: Biratnagar is विराटनगर as the city writes it (crosswalk `prefer`, CBS's spelling kept as an alternate); SEARCH rule kind 5, spelling conventions, with Nepal's v-for-b: `viratnagar` finds Biratnagar (first when focused in Nepal), `vishnu` Bishnu | districts 408+ and renamed units from a sound source, TG-17 phone benches, the MVP's golden cases |
| 2026-11-12 | D-234: 29 more districts take their official name from the misaligned CBS rows (the list's own text, checked to be in the list and by the alignment gate): 71 of 77 districts official | the 38 renamed local levels and 6 districts need a current open list; TG-17 phone benches; the MVP's golden cases |
| 2026-11-12 | TG-17 done (D-235): gate G-phone, the phone-class bench scaled to a Helio G85 class phone; every target met on the `tithipatra` layer; SUBSTRING derive 1.04 → 0.38 s and 121 → 54 MiB; the first height 63 → 0.42 ms by per-tile DEM checks | the MVP's golden cases (TG-18), TG-15 India, the 38 renamed units and 6 districts, a fold into a reused buffer (derive) |
| 2026-11-12 | TG-18 done (D-236): gate G-golden-tithipatra, 60 MVP cases on the `tithipatra` layer, all pass (57 before); prefix tier from 2 code points; donor names (tier 4 of TG-14, G-donor: Sanskrit, Awadhi, Doteli kept); 641 more Nepali names from a transliteration fix; f/ph, ee/i, oo/u spelling rules; golden lite r@1 0.837 | TG-19 (reverse: distance, bearing, inside), TG-15 India, the 38 renamed units and 6 districts |
| 2026-11-12 | TG-19 reopened and its border rule built (D-237): `reverse_in_country`, by Birgunj names Birgunj not Raxaul; point query p99 0.45 ms on the phone estimate | TG-19's inside-by-radius (research first), TG-15 India, the 38 renamed units and 6 districts |
| 2026-11-12 | TG-19 done (D-238): inside a settlement or locality with no outline = the nearest of its kind within 750 m or 700 m, the P90 building-to-place distance on Google and Microsoft footprints ([note](../02-research/notes/inside-by-radius.md)) | TG-15 India (maintainer), the 78 out-of-unit seed points, the 38 renamed units and 6 districts; near-duplicate settlements (three Asurkot within 1 km, tgd:np/1jmx, 1jmy, 1jms) |
| 2026-11-12 | Nepal data fixes (D-239): all 77 districts named in Nepali (Rukum East and Nawalparasi West link to Wikidata by outline); 10 settlements with points 1.3–888 km outside Nepal left out (`abroad-lite.tsv`); `forge make` keeps the ID registry by default (D-240); consumer requests before bindings (D-241) | TG-14's 38 renamed local levels and unit labels with a type word; separable components (D-213); TG-15 India |
| 2026-11-12 | Type words on unit names (D-243): 427 labelled names lose a trailing type word ("कैलाली जिल्ला" → "कैलाली"); a query may still end in a unit type's word (favours that type) or a level's native or English word (dropped); G-golden-tithipatra 67 of 67, golden lite r@1 0.839 kept; `tithipatra` 9.24 MB wire | TG-14's 38 renamed local levels (a source for current Nepali names); separable components (D-213); TG-15 India |
| 2026-11-12 | TG-14's 38 renamed local levels named (D-244): current Nepali names from Nepali Wikipedia's list, the CBS name kept as the former official one; 753 of 753 local levels official; G-golden-tithipatra 78 of 78; `tithipatra` 9.24 MB wire | TG-14: tier 4 for India, `machine_names`; zone outlines in the embedded merge; separable components (D-213); TG-15 India |
| 2026-11-12 | Zones at a point on the embedded layer (D-245): no REGION in a lite layer, `tz_at` by its units equals Nepal's pack within the 20 m tolerance; gate G-tz-layer (`examples/tz-agree`, 427,014 points, 9 at the border, 0 beyond; planted faults fail) | separable components (D-213); TG-14: tier 4 for India, `machine_names`; TG-15 India |
| 2026-11-12 | Everything separable (D-213), research and ADR-0040; step 1, section parts in the format (D-246): a `part` key per table entry, (kind, part) order, block hashes by part, planted faults refused; the baseline and the three layers rebuilt byte for byte | step 2: names and search by language; then outlines by level group, levels of detail, catalog and installs |
| 2026-11-12 | Data releases as GitHub pre-releases until the catalog is live (D-247): `data-2026.11.12`. Step 2, names and search by language (D-248): Nepal lite in 3 parts (base, `*`, `ne`), golden lite and G-golden-tithipatra unchanged, search p50 +55% with every part held | step 3: outlines by admin level group; then levels of detail, catalog and installs of parts |
| 2026-11-12 | Step 3, outlines by admin level (D-249): whole parts per level; Nepal lite to districts 1.31 of 3.29 MB wire; gate G-outline-parts 0 differences at 427,014 points per level | data release with `v0.2.0`; step 4, levels of detail; step 5, catalog and installs of parts |
| 2026-11-12 | Step 4, levels of detail (D-250): outline parts at 20 m and 100 m for every level; Nepal lite every level 0.64 / 0.26 MB wire instead of 3.29; G-outline-parts 0 beyond the tolerance; CI fixed (no_std `alloc` gate, 0.2.0 in the forge and tool lockfiles) | `v0.2.0` tag and the data release; step 5, catalog and installs of parts |
| 2026-11-13 | Code tag `v0.2.0` at `ecb8561` (CI green); data release `data-2026.11.13` (Nepal pack with parts, three layers, generic names); a new project on the tag reads the published layer | step 5, installs of parts (D-251) |
| 2026-11-13 | Step 5, first half (D-251): `Parts` chooses languages and one outline part; the engine reads only those (`LayerOptions::installed_parts`); English only keeps every Latin class of golden lite, −31% names wire | installer and catalog by part, `tgd install --names --outlines` |
| 2026-11-13 | Installs of some parts (D-252): the installer plans by `Selection`, the store records and hollows by part, journal v3 when needed; the parts test fails on a planner that ignores parts | the catalog's per-part sizes, `tgd install --names --outlines` |
| 2026-11-13 | The catalog's per-part sizes (D-253): `parts` grouped by component with each outline's tolerance; `Entry::size_of(selection)` equals the bytes an install of those parts fetches; Nepal lite's 28 parts in 1,484 B | `tgd install --names --outlines`, profiles pick parts |
| 2026-11-13 | `tgd install --names LANGS --outlines LEVEL[@Mm]` (D-254): the smallest outline part that holds the level within the tolerance; `catalog --json` and `status` show parts; the CLI test fails on an installer that ignores `--outlines` | profiles pick parts; then TG-14 India |
| 2026-11-13 | Everything separable done (D-255): profiles keep their build-time cut (script-level), downloads pick parts | TG-14 (`machine_names`, India tier 4), TG-15 India |
| 2026-11-13 | `machine_names: keep \| drop` (D-256): `tithipatra` with `drop` 8.29 MB wire instead of 9.24, Nepal's `ne-Deva` 3.7% instead of 99.9%; `keep` byte-identical | TG-14 tier 4 for India, TG-15 India |
