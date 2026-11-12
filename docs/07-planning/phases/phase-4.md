---
title: "Phase 4: engine, baseline and sync"
status: draft
updated: 2026-11-10
---

# Phase 4: engine, baseline and sync

**Goal:** an app opens any set of packs as one dataset, always has an offline answer from the
embedded baseline, and can find, download, verify, store, update and repair packs. An end user
picks "Dubai" and the library does the rest. Exit: milestone **M4** ([roadmap](../roadmap.md)).

**Research:**
- [offline loading and trust](../../02-research/notes/offline-loading-and-trust.md): TUF
  subset, hosting, platform transports, atomic install, eviction, the 15 sync gates (ADR-0037);
- [engine and catalog](../../02-research/notes/engine-and-catalog.md): layers, precedence,
  cross-pack queries, the baseline and its locator, catalog fields, discovery by name;
- [download and delta](../../02-research/notes/download-and-delta.md): the seekable zstd
  transport, measured delta sizes, resume, background limits, the storage formula;
- [block hashes](../../02-research/notes/block-hashes.md): per-block verification (D-078),
  used while downloading and on first read.

Order: the engine first, on packs that are already on disk, so every later step installs into a
running engine. Then the baseline, then the catalog and trust, then bytes on the wire. Nothing in
it names a country (P16); every network call is behind the `sync` feature and an explicit API
call (ADR-0012).

## Steps

### 4.0 Research
- [x] The two notes above, beside offline loading and trust and block hashes (2026-11-09)
- [ ] Decision-log rows for what the notes recommend, as each is confirmed by its step

### 4.1 Engine
- [x] `teigeodata-engine`: mapped packs (B13: where `unsafe` for mmap lives, SIGBUS on a
  truncated file), lazy verification (small sections whole, large per block, D-078). Done
  2026-11-09 (D-113, [engine](../../04-architecture/engine.md)): `unsafe` only in the engine's
  `map.rs`; open 90 µs warm on a 5.3 MB pack; planted check bugs fail `tests/verify.rs`
- [x] Layers, precedence and coverage rules
  ([engine and catalog](../../02-research/notes/engine-and-catalog.md) §3.4); tombstones and
  redirects across layers. Done 2026-11-09 (D-114)
- [x] Routing and k-way merges for `get`, `reverse`, `admin_at`, `tz_at`, box queries,
  `search`; per-answer tier; caller-owned scratch (M1: 0 warm allocations, search 1 for the
  folded query). Done 2026-11-09 (D-114); 3 layers on 200k synthetic places: get 394 ns,
  reverse k=10 20 µs, search 69 µs ([benchmarks](../../02-research/benches/phase-4.md))
- [x] Search on the measured pipeline, scored as the product (`pack-eval` with
  `S3_ENGINE=product`). Done 2026-11-09 (D-119): first-letter fuzzy with a visible-match
  fallback, parent context through HIERARCHY; Nepal recall@1 0.535 → 0.652, search 72.6 → 11.1 µs
- [x] Search parity with the spike: the country's query rules in the pack (D-120) and scored
  by the engine. Done 2026-11-09 (D-121): Nepal recall@1 0.797 against the spike's 0.801,
  recall@5 0.932 for both, p50 6.7 µs against 7.2 µs
- [ ] The golden gate on the product's search in CI (with step 3.7). Written (D-128):
  `pack-eval` with `S3_GATE` fails when any class loses a hit against
  `tests/golden/<cc>-baseline-<build>.tsv`; the `data-build` workflow runs it once the mirror
  exists
- [x] Radius queries (`within_radius`, `nearby`) and `parent`/`ancestors`/`children` in a
  place's own pack (D-140, 2026-11-09): Nepal `within_radius` 1 km 2.8 µs, 10 km 12.0 µs;
  `reverse` on 3 layers 23.3 → 16.0 µs
- [x] `children`/`ancestors` across packs (D-143, 2026-11-09): by ancestor stubs, no format
  change; the metamorphic gate compares the hierarchy on synthetic and Nepal splits. Nepal,
  1/3/10 layers: `ancestors` 307/967/2,396 ns, `children` 6.9/34.6/104.6 µs
- [x] A search context (`Lahachowk, Kaski`) split-invariant (2026-11-09): it matches ancestors
  in the row's own pack (D-119), which every pack holds as stubs (D-143); the metamorphic gate
  now asks "Name, Parent" and "Name, Pare" queries on synthetic and Nepal splits, and a context
  bonus planted to apply in one layer only fails it
- [x] Forge: split builds (city packs, grid tiles) write their places' ancestor stubs (D-143),
  gated by check-pack (2026-11-11, D-191): `forge make --split` by `split.yaml`, proved
  against the country pack by `split::check`; Nepal by province 7 parts, 1.16× the pack.
  Grid tiles for units over a budget are W1, with India's sizes
- [x] Snapshot swap: `arc-swap` against `RwLock` A/B in L5 (D-141, 2026-11-09): with 8
  readers and a writer, `arc-swap` 510–555 ns per `get` on 3 layers, a held `RwLock`
  1,665–1,864 ns (its writer waited up to 26 ms), a `Mutex` 4,990–5,283 ns; `Shared` built on
  `arc-swap`
- [x] Metamorphic gate: splitting or reordering packs never changes an answer; differential
  against one compacted pack. Done 2026-11-09: synthetic in 2, 3 and 5 parts, the Nepal main
  pack in 2 and 3 (46.7 s, ignored test with `TGD_PACK`); planted bugs fail it (D-114)
- [x] L1 at 1 GB mapped, L4, L5 at 1, 3 and 10 layers. Done 2026-11-09
  ([benchmarks](../../02-research/benches/phase-4.md#ten-layers-l5)): 10 layers cost 5.5× one
  layer for `get` and 1.3× for `admin_at`; opening a 1 GiB pack was 4.9 ms, now 52 µs because
  block-hash lists are proved on first read (D-142)

### 4.1b Components: download only the data asked for (ADR-0015, D-133)
Research: [components](../../02-research/notes/components.md). The section is the unit; one
signed pack per address; a client holds any subset of its sections.
- [x] Format: ELEVATION (kind 19) out of PLACES into its own section; the forge writes it and
  checks it against the DEM on read-back; the engine refuses a layer whose ELEVATION row count
  differs from PLACES' (2026-11-09)
- [x] Format: a place's timezone in its own section, PLACE_TZ (kind 20, D-134), proved equal to
  REGION at the place's point; the engine's `tz_of(place)`. A place's admin units come from
  HIERARCHY (G-pip), so no admin column (2026-11-09)
- [x] Format: one table mapping each section kind to its component and dependencies,
  `teigeodata_format::component` (D-135), checked against the spec (2026-11-09)
- [x] Forge writes them; `check-pack` prints wire bytes per component from the real transport
  asset, and gates them (G-wire, D-135) (2026-11-09)
- [x] The partial file: head, installed sections at their offsets, the footer rebuilt from the
  head (`footer_of`), opened with `Pack::open_head` (D-136, 2026-11-09)
- [x] Storage on disk: the partial file as a sparse file, the journal's component set as the
  record of what is present (D-171); APFS fills gaps under 16 MiB, so the store punches what a
  partial install never writes (`F_PUNCHHOLE`, `fallocate`): points only 1.50 MB of a 2.85 MB
  pack on disk (2026-11-11, D-189; [sparse installs](../../02-research/notes/sparse-installs.md))
- [x] Windows sparse files: `FSCTL_SET_SPARSE`, `FSCTL_SET_ZERO_DATA`, and a `windows` job in
  `fast-check` for the store and the CLI (2026-11-11, D-190); no compact-file layout is needed
  elsewhere
- [x] Engine: a layer opens with any closed subset; a query that needs a missing component
  returns `MissingComponent`; `Engine::elevation` (D-136, 2026-11-09)
- [x] Sync: `Frames::plan_components(set)` plans only those sections' frames and the head
  (D-136); the request API on it comes with the download manager (4.5)
- [x] Cargo features per component; a binary without one carries none of its readers
  (D-137: points-only engine 97.3 KiB native, 60.9 KiB gzip, 2026-11-09)
- [x] Gates: points-only Nepal at most 0.55 MB on the wire (522,192 B main, 493,757 B lite,
  2026-11-09); components sum to the asset (G-wire)
- [x] Gate: each component set answers its queries as the full pack does, holes zeroed
  (`tests/components.rs`, D-136)

### 4.2 Baseline and locator
Research: [baseline](../../02-research/notes/baseline.md) (D-144, 2026-11-09).
- [x] Forge: a country with a pack contributes a slice of that build (`forge baseline`, a `make`
  stage; `extract::rows`; G-base; level-1 units under the country, D-146) (D-145)
- [x] Forge: the world part's countries and cities (`forge world`): Natural Earth admin 0 in
  Nepal's view, `cities15000` under the feature that contains each, IDs per namespace and a
  pack country's from its pack (ID gate); 3.77 MB main (D-147)
- [x] Forge: world admin 1: Natural Earth admin 1 cut to the Nepal-view countries (overlay,
  fillers, one arc set), units under their country and cities under their unit; 5.73 MB main
  with admin 1 (D-150). Per-country admin-1 sources (OSM extract, gbOpen) replace it later
- [x] Forge: world timezones in the baseline: a zone per country, unit and city from tzdb and
  the cities' own zones, `tz_at` by the deepest unit, no geometry (30 kB); slices keep their
  pack's zones (D-151)
- [x] Forge: a slice keeps the names its world row would (the country's languages and
  English) and rebuilds SEARCH from them: Nepal's slice 157,532 → 72,988 B (D-152)
- [x] The baseline as shipped: one file (`forge embed`, `extract::merge`), not layers: layers
  cost search 34× and reverse 98× at 200 slices; world + Nepal 5.88 MB, answers equal to the
  layers' for every ID (D-153)
- [x] The world part's neighbours cut to each pack country's outline (the slice's own
  vertices, pinned), so a pack border and its neighbours share one line: 0 places differ
  (D-154)
- [x] Pin `geonames-cities15000` (D-145) and `ne-admin0-nep` (D-147)
- [x] Pin `ne-admin1` (D-150)
- [x] Research main's OSM admin 1: Overture `divisions`, not a planet extract; main's admin 0
  from the same lines, Natural Earth's Nepal view as the owner of disputed units (D-155,
  [world admin 1 from OSM](../../02-research/notes/world-admin1-osm.md))
- [x] Pin `overture-divisions` (a tileset of the release's nine part files, 5.04 GB,
  2026-10-21.0)
- [x] `read::overture`: streamed row groups, land countries, dependencies and regions, WKB on
  the 1e-7° grid, Wikidata and OSM relation from `division` (3.2 s, 1.19 GB; equal to
  `DuckDB`'s counts)
- [x] Pre-simplification of the world as one partition, shared arcs within 100 m
  (`topo::thin`; `arcs::build` on sorted records, world part byte-identical): claims to the
  earlier code, snap rounding, folds undone; 31.47 M → 3.58 M vertices, guard left 0, 250 s,
  4.1 GB ([§3.5](../../02-research/notes/world-admin1-osm.md#35-the-worlds-land-as-one-partition-measured-2026-11-10))
- [x] Crossings, snap rounding and unfolding on sorted records, banded and parallel: the
  world's thinning 286 s → 122 s, peak 4.05 → 2.46 GB, the same result (2026-11-10;
  the cells of a 4,096-wide grid made dense coasts quadratic). World byte identity re-checked
  when disk allows (the Overture tiles were removed to free space)
- [x] ~~Pin `ne-admin0`~~ dropped: Natural Earth's default view also gives Western Sahara's
  Free Zone to Western Sahara, so a delta against it misses that dispute (D-156)
- [x] Ownership by the Nepal view (`topo::pov::own`, D-156): a unit keeps its code's country;
  the view takes its parts at least 15 km wide (noise reaches 11 km, the narrowest dispute
  22 km; [§3.6](../../02-research/notes/world-admin1-osm.md#36-ownership-by-the-nepal-view-measured-2026-11-10))
- [x] `forge world` main on Overture: countries dissolved from units, pack outlines pinned;
  islets under the tolerance's square left out, 2 km (D-157); 5,601,192 B merged (Z1), world,
  locator (3 km, 0 of 100,104 probes missed), ID and baseline gates green
  ([§3.7](../../02-research/notes/world-admin1-osm.md#37-size-islets-and-the-tolerance-measured-2026-11-10))
- [x] A first-level unit ranks below a city of its name inside it; world cities ranked by
  feature code (D-158; Nepal's golden set unchanged)
- [x] The data-build job fetches the pinned Overture tilesets for main's world part
- [x] The locator: `countries_near` and `missing_packs` by distance on the stored outlines
  (no buffered REGION, so no bytes), `LOCATOR_RADIUS_M` = 1 km then, 3 km since D-157;
  border-probe gate: no missed country over about 110,000 source vertices (D-149,
  [locator](../../02-research/notes/locator.md))
- [x] Size gate Z1 (8 MB) in `forge embed` and `tests/baseline.rs`; the engine's `baseline`
  feature (`BASELINE`, `Engine::with_baseline`); WASM loads the file with
  `LayerOptions::baseline()` (D-153)

### 4.3 Catalog and trust
- [x] TUF subset client (`sync::trust`): root chain, timestamp, snapshot, targets, canonical
  JSON, persisted versions, length caps, the offline-first expiry rule (2026-11-10, D-159,
  [§3.7](../../02-research/notes/offline-loading-and-trust.md#37-the-client-as-built-2026-11-10)):
  `no_std` + `alloc`, 17 tests on a generated repository; nine planted faults each fail a test;
  builds for `thumbv7em-none-eabihf` in fast-check
- [x] Catalog `custom` schema v1 with a checked-in JSON schema (2026-11-10, D-160,
  [§3.10](../../02-research/notes/engine-and-catalog.md#310-the-catalog-schema-as-built-2026-11-10)):
  `sync::catalog::Entry`, `schema/catalog-custom-v1.json`; 466 B of `custom` an entry; eight planted
  faults each fail a test
- [x] Differential gate against python-tuf (2026-11-10, D-161): 27 cases, 66 steps, one
  recorded difference (key IDs against keys); the corpus is checked in and the `tuf-oracle` job
  regenerates it; five planted faults and a falsified verdict each fail
- [x] The differential against `tough`, in its own crate outside the workspace: repositories written at run time, since tough reads the system clock (2026-11-10, D-165)
- [x] Repository tooling ([research](../../02-research/notes/catalog-repository-tooling.md),
  2026-11-10, D-162, D-163): `sync::repo` writes and signs the roles with the client's canonical
  JSON; `xtask tuf` keygen, pubkey, root, sign, release (assets and catalog entries from packs),
  timestamp, verify; the rotation drill as a test; python-tuf reads what it writes; five planted
  faults each fail
- [x] The timestamp workflow `catalog-timestamp` (daily, 14-day expiry, fails 7 days before
  expiry), off until the maintainer's first ceremony; the [ceremony
  document](../../06-engineering/catalog-keys.md) (2026-11-10, D-163)
- [ ] Upload to sharded immutable releases (at most 1,000 assets each) and the first ceremony
  (the maintainer: keys, the `catalog` branch and Pages, the environment secret)
- [x] Discovery: `catalog.find(name)` over the baseline (and the gazetteer pack when present),
  then the locator, then the catalog; `Resolver` trait (2026-11-10, D-164)

- [x] Targets delegated by country (D-175, D-176, 2026-11-11): spec 1.0 delegations in
  `sync::trust` (one level, `paths` patterns, terminating), `sync::repo` and `tuf release`
  writing one role per country, `Catalog::trusted` over the roles a client fetched; gate 12
  green on the generated world (top-level 3.8 KB, snapshot 13 KB, largest role 220 KB gzip)
- [x] Both oracles (python-tuf, tough) extended to delegated roles (2026-11-11, D-177): python-tuf
  judges 43 cases (17 on delegation) and reads the written repository's country role; tough
  agrees on 23 repositories written in the delegated layout

### 4.4 Transport assets
- [x] Seekable zstd writer (frames on the head and sections, ≤ 1 MiB at 16 KiB boundaries) with
  a byte-for-byte self-check; read by `zeekstd` and the `zstd` CLI (2026-11-09, D-112,
  [transport](../../04-architecture/transport.md)). Nepal main, 11,748,636 B: 8,227,234 B at
  level 19 in 23 frames, +0.99 % over whole-file zstd (8,146,743 B); level 9 +0.85 %, level 3
  +0.39 %. `zstd -d` and `zeekstd` 0.6.3 give the pack back byte for byte; `ruzstd` decodes it
  at 258–320 MB/s. Each section is one request: NAMES 917,602 B in 3 frames, PLACES 602,351 B,
  the head 785 B ([§3.5](../../02-research/notes/download-and-delta.md#35-the-transport-as-built-measured-2026-11-09)).
  Planted faults in the writer (a frame one byte short, a changed byte) fail its self-check
- [x] Seek-table reader and frame planner, fuzzed (lying tables, oversized frames, frames across
  sections) (2026-11-09, D-112): `teigeodata-sync::transport`, `no_std` + `alloc`; harnesses
  `transport_table`, `transport_asset`, `transport_frame` in `cargo test` and nightly libFuzzer.
  With the decoded-size check or the asset-length check removed, the harnesses fail

### 4.5 Download manager
- [x] A head-only parser in `teigeodata-format`, so the layout comes from the head frame alone:
  `PackHead`, `SeekTable::frames_from_head` (2026-11-10, D-166)
- [x] Sans-I/O planner and installer on `transport::Frames`: `sync::install` (2026-11-10, D-167)
- [x] `Transport` trait with a `ureq` default (binary size gate against `reqwest`): `ureq`
  1.28 MB against `reqwest` 2.34 MB for one ranged GET (2026-11-10, D-168)
- [x] Verify-then-write per block (2026-11-10, D-167)
- [x] Resume by frame with ETag and length checks (the host ignores `If-Range`): every frame
  left in staging is checked against the signed hashes and kept, only the rest fetched
  (2026-11-10, D-169)
- [x] Mirrors with demotion: `fetch::Mirrors`, `install_from`; each attempt on one mirror,
  resumed by the next (2026-11-10, D-170)
- [x] Staging, fsync, rename, journal commit, crash recovery: `sync::store`, the crash gate at
  every step (2026-11-10, D-171)
- [x] Snapshot swap on install: the engine reopens from the journal while readers keep the old
  mapping: `Shared::follow` (2026-11-11, D-172)
- Platform transport prototypes for gate 14; packaging stays in Phase 5
  ([platform transports](../../02-research/notes/platform-transports.md), D-184):
  - [x] (a) The seam: installs in passes over `fetch::Downloads`, `FetchError::Needs`, `tgd
    install --downloads` (D-184)
  - [x] (b) macOS `URLSession`, foreground and background, `Range` kept across a redirect (D-184)
  - [x] (c) The iOS simulator: a background session, the app killed mid-download and relaunched
    by the system, the range filed and the pack committed (D-185)
  - [x] (d) Android UIDT on the emulator: the job restarted by the system after its process is
    killed (D-186)
  - [x] (e) The browser: ranged `fetch` into OPFS in a worker across origins, resumed from OPFS
    after the browser is killed (D-186; the CORS mirror for the real hosts is Q-014)

### 4.6 Storage manager
- [x] Installed list, quota, LRU eviction with pinning and dependency protection, the disk-peak
  formula ([download and delta](../../02-research/notes/download-and-delta.md) §3.4):
  `store::plan_room`, `Store::{need, make_room, remove, pin, mark, touch}`, journal version 2
  ([storage manager](../../02-research/notes/storage-manager.md); 2026-11-11, D-172)
- [x] `verify(deep)` and repair by section: `Store::verify`, `Store::repair` over the resume
  path, `follow` reopening a repaired file (2026-11-11, D-173)

### 4.7 Delta updates
- [x] Section re-download over frames (S3): `fetch::update_from`, used by `Store::install`
  over the release it replaces; unchanged frames read from the old file and checked against
  the new signed hashes (2026-11-11, D-174)
- [ ] Two real consecutive Nepal releases: bytes for section re-download against delta packs
  (and zstd patches, dropped unless this disagrees); decision recorded
- [ ] If delta packs win: forge emission, compaction policy, L5 with delta layers

### 4.8 The 15 sync gates
- [ ] Each gate of ADR-0037 (register: [sync gates](../../06-engineering/sync-gates.md); 11
  green, 12 red until 4.3's delegation, 11 and 14 open) (listed in [offline loading and
  trust](../../02-research/notes/offline-loading-and-trust.md) §6) built, first shown to fail on
  a deliberate violation, then green in CI
- [x] A world-size install: a section over 512 MiB, so `BLOCK_HASHES` spans frames and its
  one-fetch rule (D-170) is exercised; a planted split list fails it (2026-11-11, D-188). It
  runs in `nightly`, not the data build: it needs no data, and the data build waits on the
  input mirror

### 4.9 CLI `tgd`
Research: [the CLI and the catalog refresh](../../02-research/notes/cli.md) (D-178).
- [x] (a) `Transport::fetch` (a capped whole-file GET) and `sync::refresh`: the TUF client
  workflow over the store, metadata kept under `meta/` by version with the timestamp as the
  commit point, crash-safe at every write (D-179)
- [x] (b) Crate `teigeodata-cli` (`tgd`, clap): `inspect`, `verify`, `diff` (sections and
  records), `extract` (by component and by box), `export` (NDJSON, GeoJSON, streamed) (D-180)
- [x] (c) Queries `get`, `reverse`, `search`, `near`, `admin`, `tz`; outputs text, `--json`,
  `--geojson`, `--nominatim` (ADR-0038) from one mapping module (D-181)
- [x] (d) Sync: `init`, `refresh`, `catalog`, `find`, `locate`, `install`, `update`, `remove`,
  `pin`, `repair`, `status`, end to end against a repository written as `tuf release` writes it,
  served on localhost with range requests (D-182)
- [x] (e) Every command run and its output read; `--help`, completions, the docs (the crate's
  README until Phase 7); timings of open, query and install (D-183)
- [ ] `merge` (compaction) once 4.7 decides on delta packs

### 4.9b Consumer requests

The first consumer brief (Tithipatra, 2026-11-11, D-193); the reply and statuses are in
[consumers/tithipatra.md](../../01-product/consumers/tithipatra.md). Each is a general feature.

- [x] TG-04: `Engine::bbox_of(place)` and `Engine::boundary(place, lod, &mut Outline)` over the
  stored outline boxes and arcs (D-194, 2026-11-11)
- [x] TG-07: the DEM section names its source; `elevation_at` returns metres with source,
  resolution in metres and datum (D-195, 2026-11-11)
- [x] TG-02: ISO 3166-1 and 3166-2 codes on countries and level-1 units, from data files with
  their source: the `ISO_CODES` section (kind 21, `admin`), `data/countries/<cc>/iso3166.yaml`,
  gate G-iso in the pack stage and the shipped baseline, `Engine::iso_code` and
  `Engine::by_iso`, called on each entry `admin_at` and `ancestors` return (D-197, 2026-11-11).
  Open: the CLDR differential of G-iso (CLDR not yet pinned), the world part's codes (Natural
  Earth's are stale; needs CLDR to know which), India's file with the four 2023 renames
- [x] TG-03: zone rules in the pack: compiled TZif for every zone the outlines name, from the
  same tzdb release, its version in the manifest; `tz_offset_at`, `tz_local_candidates`,
  `tz_version`; a gate that every zone has rules (D-196, 2026-11-11)
- [x] TG-06: substring search of 3+ characters across scripts, measured for size: the
  `SUBSTRING` section (kind 23) in component `substring`, the forge's word-suffix keys, the
  engine tier at bonus 20, golden class `substring` (D-199, 2026-11-11). Open: size and p99 on
  the real Nepal pack (measured on a GeoNames proxy: 19.9 B per place), and the p99 of
  3-code-point queries
- [x] TG-01: `CHANGELOG.md` and a `v0.x` tag; a consumer builds the engine from the tag
  (D-198, `v0.1.0` 2026-11-11: a scratch project built `teigeodata-engine` and
  `teigeodata-sync` from the tag through `git`, wrote a pack and answered a `get`)
- [x] TG-08: research note on zones before 1970 (`backzone`) for South Asia
  ([note](../../02-research/notes/zones-before-1970.md): backzone adds no South Asian zone)

### 4.9c Consumer requests: offline first (addendum 1, D-213)

Order as in [embedded layer](../../02-research/notes/embedded-layer.md); statuses on the
[consumer page](../../01-product/consumers/tithipatra.md).

- [x] TG-11: `cargo xtask pack-sizes` (components on disk and wire and per place, columns,
  names by language and script, the names cut to a profile's languages, outlines by admin
  level and level of detail); tables in [sizes](../../05-data/sizes.md) (2026-11-11)
- [x] Everything separable (D-213): names by language and outlines by admin level and level
  of detail as their own components, each sized in the catalog; the profile and the client
  pick any set ([research](../../02-research/notes/separable-components.md),
  [ADR-0040](../../03-decisions/adr/ADR-0040-section-parts.md)). Steps:
  - [x] 1. Section parts in the format: the table's `part` key, (kind, part) order, block
    hashes by part, reader and writer refusals; packs without parts unchanged (D-246,
    2026-11-12)
  - [x] 2. Names and search by language: the forge writes NAMES, SEARCH and SUBSTRING parts
    (base: canonical and undetermined names; a part per language of 128 KiB or more, measured
    against 16 KiB; `*` for the rest); the engine reads every installed part, a row scored
    once at its best tier; golden lite and G-golden-tithipatra unchanged with every part;
    `pack-sizes --langs` matches `ne-Deva` by its primary subtag (D-248, 2026-11-12). The
    English-only install gate moves to step 5, with installs of parts
  - [x] 3. Outlines by admin level: one whole part per level *k* (every outline section over
    levels 0 to *k*), not arc groups; `LayerOptions::outline_level`; gate G-outline-parts,
    `admin_at` on each part equals the full chain cut at *k* at 427,014 points (D-249,
    2026-11-12)
  - [x] 4. Levels of detail as parts: 20 m and 100 m, a whole part per admin level each;
    Nepal lite every level 0.64 / 0.26 MB wire instead of 3.29 MB; G-outline-parts within
    the tolerance, 0 beyond (D-250, 2026-11-12)
  - [x] 5. The catalog lists each part's wire size; `tgd install np --names en --outlines
    2@20m` (admin levels by number, world-general); profiles pick parts; the engine's installed
    set by part (done: `Parts`,
    `LayerOptions::installed_parts`, the English-only gate, D-251; installs and the store by
    part, D-252; the catalog's per-part sizes and `size_of(selection)`, D-253; `tgd install
    --names --outlines`, D-254; profiles keep cutting at build time, D-255; 2026-11-13)
- [x] Derive SUBSTRING on the device: `Store::derive` checked against the head's hash, engine feature `derive`; Nepal lite in 0.50 s, byte for byte (D-218, 2026-11-11)
- [x] TG-12: `forge embed --profile` from `data/embed/<name>.yaml`; `baseline` byte-identical, `tithipatra` 7.32 MB wire, `points-en` 1.17 MB (D-214, 2026-11-11)
- [x] Nepal in full in the embedded layer: all 95,726 places, IDs kept (D-214). Zone rules: built for the layer's every zone (D-224, 2026-11-11). Zones at a point by the layer's units, no REGION; gate G-tz-layer (D-245, 2026-11-12)
- [x] TG-13: world cities of 5,000 with heights and their source: `forge world --cities
  geonames-cities5000` (69,972 cities), ELEVATION's SOURCE column, `Engine::elevation_source`
  (D-219, 2026-11-11)
- [ ] TG-14: Nepali and English names for every place: research done (D-215). Steps:
  - [x] GeoNames' Latin-script `ne` names are never taken as Nepali: every stored name
    carries the script of its text, and a profile asks for `ne-Deva` (D-215, 2026-11-11)
  - [x] World places: Wikidata `ne`, then `hi`, labels by `wikidata:`/`geonames:` key,
    `forge world --labels`, two pinned CC0 sources; 13,724 world places named (D-221,
    2026-11-11)
  - [ ] sources pinned: the CBS code list (official names), Wikidata labels for Nepal's
    places by key, CLDR 49 `ne.xml` for country names
  - [ ] the chain in the forge, each name's origin in its record (`rule`), with a coverage
    report per step (TG-11's names table). Done: origins in `RULE` (D-227), tier 6 (the
    canonical name transliterated, marked machine, searched apart; 99.0% of Nepal named,
    D-228, 2026-11-12), tier 1 (official names by a reviewed code crosswalk with an alignment
    gate: 715 local levels and 71 districts, D-232, D-234, 2026-11-12); tier 2 is in already
    (Nepal's Wikidata snapshot carries every language's labels); the 6 districts the CBS file
    loses (Nawalparasi East and West, Rukum East and West, Kailali, Kanchanpur) named from
    Wikidata once Rukum East and Nawalparasi West link by outline (D-239). Open: tier 4
    for India (Nepal's built, D-236), the `machine_names` profile option. Done: the 38 renamed
    local levels take their current Nepali names, the CBS name kept as the former official one
    (D-244); a unit's labels lose their type word and a query may end in one
    (D-243); names distinct by text and language (D-242)
  - [x] the transliterator: a generic engine over `data/xlit/<from>-<to>.yaml`, its character
    model trained in the forge on the build's licence class; gate G-xlit with floors
    (`forge xlit-eval ne-Latn-Deva`: cbs 44.7%, wd 36.2% folded; D-222, 2026-11-12). Open: the
    §5.3 v1 targets (on main, with the D-220 fold: cbs 50.3%, wd 42.3% folded; wd meets its
    target)
  - [x] a profile's languages are complete: every place of the layer has each, or the build
    names the gap (the layer summary's table per country, 2026-11-12; `tithipatra`: Nepal
    1.1% `ne-Deva`, D-227)
- [x] TG-16: outline tolerance measured (Nepal stays at 20 m), outlines snapped to their level's decimal grid, 64-edge index leaves: `tithipatra` 9.21 MB wire (D-229, D-230)
- [ ] TG-15: India: research done (D-216, D-217); then towns
- [x] TG-19: `Near::bearing_deg` and `Engine::mark_inside` (D-223, 2026-11-11); `reverse_in_country`, the border rule (D-237, 2026-11-12); inside a settlement or locality with no outline as the nearest of its kind within 750 m or 700 m (D-238, 2026-11-12, [research](../../02-research/notes/inside-by-radius.md)). Data follow-up done: places outside every unit beyond the border reach are left out (D-239: 10 in Nepal, Chhipaharmai among them)
- [x] TG-18 and the strict-tier option: the Brahmic search fold of nasal conjuncts, vowel
  length and candra vowels (D-220); the focus point and the strict-tier option, `search_with`
  (D-226); the व/ब variant as a spelling rule (D-233); gate G-golden-tithipatra, 60 cases from
  the MVP's search (brief §1.2), all passing on the `tithipatra` layer, with the prefix tier
  from 2 code points, donor names and the f/ph, ee/i, oo/u rules (D-236, 2026-11-12). Open:
  add the MVP's own test cases to the file if Tithipatra sends them
- [x] TG-17: phone-class bench and gate G-phone ([note](../../02-research/notes/phone-class-bench.md),
  D-235, 2026-11-12): every target met on the `tithipatra` layer; deriving SUBSTRING 1.04 →
  0.38 s (121 → 54 MiB heap), the first height 63 → 0.42 ms by per-tile DEM checks. Open:
  calibrate the factor on a real low-end phone; G-phone on CI once the data build runs there
- [x] TG-10: coarse elevation layer: `DEM_COARSE`, component `dem-coarse`, 15″ at 8 m steps,
  518 kB on the wire for Nepal; `elevation_at` falls back to it; G-dem-coarse (D-225, 2026-11-12)

### 4.10 Exit review
- [ ] M4 criteria met, each with its evidence; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-09 | Phase 4 drafted from the research notes | [engine and catalog](../../02-research/notes/engine-and-catalog.md), [download and delta](../../02-research/notes/download-and-delta.md) |
| 2026-11-09 | 4.4 done: the transport asset in `teigeodata-sync::transport` (D-112); the planner lives there, not in a separate `sync::plan`; a head-only parser in `teigeodata-format` is added to 4.5 | building it: a ranged client holds only the head frame ([download and delta §3.5](../../02-research/notes/download-and-delta.md#35-the-transport-as-built-measured-2026-11-09)) |
| 2026-11-09 | Step 4.1: engine, layers, queries and the metamorphic gate done; radius and cross-pack hierarchy split into their own item | D-113, D-114 |
| 2026-11-09 | 4.1: the product's search scored and brought towards the spike; parity its own item | D-119: the golden gate measured the spike, not the engine |
| 2026-11-09 | 4.1: parity done; the CI gate on the product its own item | D-121 |
| 2026-11-09 | 4.1: the golden gate written against a checked-in baseline (D-128) | a CI gate needs a number to hold |
| 2026-11-09 | 4.1b: components as their own item, sized on Nepal | the maintainer wants points-only downloads (lat, lon, altitude) |
| 2026-11-09 | 4.1b: ELEVATION is its own section; the format item split in two | D-133 step 1 done |
| 2026-11-09 | 4.1b: PLACE_TZ instead of a COMPONENTS field; no admin column | D-134 |
| 2026-11-09 | 4.1b: the component table and G-wire; the per-component metamorphic gate its own line | D-135 |
| 2026-11-09 | 4.1b: the engine on installed components, and its gate | D-136 |
| 2026-11-09 | 4.1b: the partial file and the component plan; disk storage joins 4.5/4.6 | D-136: a points-only install lacks the footer and BLOCK_HASHES |
| 2026-11-09 | 4.2: the forge item split into the pack slice (done) and the world part; pins split | D-145, D-146 |
| 2026-11-10 | 4.2: the world part split into countries and cities (done), admin 1, timezones, and the shipped form | D-147 |
| 2026-11-10 | 4.2: the locator is a distance query on the outlines, not a buffered REGION | D-149 |
| 2026-11-10 | 4.2: world admin 1 from Natural Earth cut to the Nepal view for both builds; the pin split from the OSM extract job | D-150 |
| 2026-11-10 | 4.2: world timezones from the units and the cities' zones, not simplified TBB | D-151 |
| 2026-11-10 | 4.2: slice names cut to the world part's languages, so slices fit the budget | D-152 |
| 2026-11-10 | 4.2: the baseline ships as one merged file; neighbours cut to pack outlines added | D-153 |
| 2026-11-10 | 4.2: neighbours cut to pack outlines done | D-154 |
| 2026-11-10 | 4.2: the OSM extract item becomes Overture divisions for main's admin 0 and 1, in five steps | D-155 |
| 2026-11-10 | Main's world part: ownership by the Nepal view at 15 km instead of the `ne-admin0` delta; 2 km and islets left out to fit Z1; locator radius 3 km; regions below their city | [world admin 1 §3.6–3.7](../../02-research/notes/world-admin1-osm.md#36-ownership-by-the-nepal-view-measured-2026-11-10), D-156 to D-158 |
| 2026-11-10 | 4.2: thinning on sorted records, banded crossings | the measured 3.9 GB peak and 132 s in `crossings` |
| 2026-11-10 | 4.3: the root cap is python-tuf's 512,000 B, not 64 KiB | D-159 |
| 2026-11-10 | 4.3: the catalog gives sizes per component and the head hash, not one target per component | D-160 |
| 2026-11-10 | 4.3: the differential split: python-tuf done, `tough` its own item | D-161 |
| 2026-11-10 | 4.3: repository tooling split into the writer and `xtask tuf`, and the workflow and ceremony | [catalog repository tooling](../../02-research/notes/catalog-repository-tooling.md) |
| 2026-11-10 | 4.3: catalog sizes count the head, seek table and BLOCK_HASHES; targets SHA-256 only; tooling and workflow done, upload and the ceremony their own item | D-162, D-163 |
| 2026-11-10 | 4.3: discovery done; the engine is the catalog's `Resolver` behind its feature `catalog` | D-164 |
| 2026-11-10 | 4.3: the `tough` differential runs on repositories written at run time, not python-tuf's fixed-time corpus | D-165 |
| 2026-11-10 | 4.5: the first item split into the head-only parser (done), the planner and installer, and the transport | D-166 |
| 2026-11-10 | 4.5: the installer and verify-then-write done; resume and mirrors their own item | D-167 |
| 2026-11-10 | 4.5: the transport and its driver done, with the size measurement | D-168 |
| 2026-11-10 | 4.5: resume done, the staging file its own record; mirrors split into their own item | D-169 |
| 2026-11-10 | 4.5: the store done with sync gate 8 (crash at every step); the snapshot swap split into its own item, built with 4.6 | D-171 |
| 2026-11-10 | 4.5: mirrors done; 4.8 gains a world-size install for multi-frame block-hash lists, which no unit-size pack reaches | D-170 |
| 2026-11-11 | 4.6 first item and the 4.5 snapshot swap done together; the catalog's disk sizes now count section padding | D-172: a whole install's parts were 48 B short of the file |
| 2026-11-11 | 4.6 done: verify and repair | D-173: repair is resume over a copy of the installed file, so it needs no code of its own to fetch only what fails |
| 2026-11-11 | 4.7 first item done: section re-download at frame granularity, reusing the resume check | D-174; the real two-release measurement stays open until two Nepal releases are built |
| 2026-11-11 | 4.3 gains delegation by country; the 4.8 register written; packs refuse an older release (gate 1) and fast-forward recovery has its own test (gate 2) | D-175: one targets file for the world is 1.42 MB gzip, 2.7 times gate 12's budget |
| 2026-11-11 | 4.3: delegation by country built and gate 12 green; extending the oracles is its own item | D-176 |
| 2026-11-11 | 4.3: both oracles over delegated roles | D-177 |
| 2026-11-11 | 4.9 split into (a) to (e) from its research; the catalog refresh comes first, in `sync`; `merge` waits on 4.7 | D-178, [CLI note](../../02-research/notes/cli.md) |
| 2026-11-11 | 4.9a done: metadata stored by version, the timestamp written last as the commit point | a refresh cut off mid-way left a timestamp ahead of its snapshot and an empty offline catalog (D-179) |
| 2026-11-11 | 4.9b done; `extract` also cuts by box (the format's slice already existed); the deep check is a format module | D-180 |
| 2026-11-11 | 4.9c done; a query with no result exits 1 | D-181 |
| 2026-11-11 | 4.9d done; the store gains `init` and `config.json`; the asset measurement moves into `sync::measure` | D-182: the end-to-end test found two store bugs the unit tests missed |
| 2026-11-11 | 4.9e done; timings are an ignored test (median of 15 runs) instead of `hyperfine`; the guide is the crate's README until the docs site | D-183 |
| 2026-11-11 | Gate 14 split into (a) to (e); platforms install in passes over one installer | D-184, [platform transports](../../02-research/notes/platform-transports.md) |
| 2026-11-11 | Gate 14 met by prototypes on macOS, the iOS simulator, the Android emulator and headless Chrome | D-185, D-186 |
| 2026-11-11 | 4.8: the world-size install runs in `nightly`, not the data build | D-188: it needs no data; the data build waits on the input mirror (D-127) |
| 2026-11-11 | 4.1b: storage on disk by punching holes, not a compact file; Windows its own item | D-189: APFS fills gaps under 16 MiB, so a partial install saved no disk |
| 2026-11-11 | Split builds: zones cut to each part, outlines of own places only, outside places to the nearest unit | D-191: parts were 2.3× the pack with whole zone rings; a "rest" part is not findable by point |
| 2026-11-11 | 4.9b: the first consumer brief's requests placed (TG-01 to TG-08) | D-193 |
| 2026-11-11 | 4.9b TG-02 done: ISO codes in their own `admin` section; `split.yaml` takes part addresses from `iso3166.yaml`; the world part's codes wait for CLDR | D-197 |
| 2026-11-11 | 4.9b TG-06 done: substring search in its own section and component | D-199 |
| 2026-11-11 | 4.9c: Tithipatra's addendum 1 placed; TG-11 done; indexes may be derived on the device | D-213 |
| 2026-11-11 | 4.9c: TG-12 and Nepal in full done; TG-14 and TG-15 researched | D-214 to D-217 |
| 2026-11-11 | 4.9c: SUBSTRING derived on the device | D-218 |
| 2026-11-12 | 4.9c: TG-14's transliterator and gate G-xlit built; the CBS code list pinned | D-222 |
| 2026-11-12 | 4.9c: TG-10 coarse DEM; TG-18 focus and strict order; name origins; Nepal's names filled by transliteration | D-225 to D-228 |
| 2026-11-12 | 4.9c: TG-16 done; the shipped baseline carries zone rules again | D-229 to D-231 |
| 2026-11-12 | 4.9c: TG-14 tier 1, official names joined by code, with an alignment gate | D-232 |
| 2026-11-12 | 4.9c: TG-18's व/ब variant as a SEARCH spelling rule; Biratnagar's official Nepali name as the city writes it | D-233 |
| 2026-11-12 | 4.9c: 29 more districts' official names from the misaligned CBS rows, by the list's own text | D-234 |
| 2026-11-12 | 4.9c: TG-17 done, the phone-class bench and gate G-phone; faster derive and per-tile DEM checks | D-235 |
| 2026-11-12 | 4.9c: TG-18 done, gate G-golden-tithipatra; TG-14 tier 4 (donor names) for Nepal; prefix tier from 2 code points | D-236 |
| 2026-11-12 | 4.9c: TG-19 reopened against the brief's criteria; the border rule built (`reverse_in_country`); inside by a radius open | D-237 |
| 2026-11-12 | 4.9c: TG-19 done; inside by radius (nearest of its kind within a measured radius), researched against building extents | D-238 |
| 2026-11-12 | 4.9c first, before phases 5 and 6; the 6 districts named, places across the border left out, `forge make` keeps IDs by default | D-239, D-240, D-241 |
| 2026-11-12 | 4.9c: TG-14, a unit's type word cut and read in queries; the 38 renamed local levels named | D-242 to D-244 |
| 2026-11-12 | 4.9c: zones at a point on the embedded layer by its units, no REGION; gate G-tz-layer | D-245 |
| 2026-11-12 | 4.9c: everything separable planned in five steps; step 1, section parts in the format | D-246 |
| 2026-11-12 | 4.9c: step 2, names and search by language; a language's part from 128 KiB, not 16 KiB (measured); the English-only gate moves to step 5 | D-248 |
| 2026-11-12 | 4.9c: step 3, outlines by admin level as whole copies, not arc groups; step 4 follows the same shape | D-249 |
| 2026-11-12 | 4.9c: step 4, levels of detail as outline parts at 20 m and 100 m, made by the merge's geometry path; outline keys bounded | D-250 |
| 2026-11-13 | 4.9c: step 5 split in two; first the part selection (`Parts`) and the engine reading it, gated English-only | D-251 |
| 2026-11-13 | 4.9c: installs and the store by part; parts kept out of file names; journal version 3 only when needed | D-252 |
| 2026-11-13 | 4.9c: the catalog lists each part's wire and disk bytes and outline tolerance; `size_of` by selection | D-253 |
| 2026-11-13 | 4.9c: `tgd install --names --outlines`; outline levels by number, not a country's word for them | D-254 |
| 2026-11-13 | 4.9c: everything separable done; embed profiles keep cutting at build time (script-level), clients pick parts | D-255 |
