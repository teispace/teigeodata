---
title: "Phase 4: engine, baseline and sync"
status: draft
updated: 2026-11-09
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
- [ ] Radius queries (`within_radius`, `nearby`) and `children`/`ancestors` across packs
  (also makes a search context split-invariant: the split gate leaves comma queries out)
- [ ] Snapshot swap: `arc-swap` against `RwLock` A/B in L5
- [x] Metamorphic gate: splitting or reordering packs never changes an answer; differential
  against one compacted pack. Done 2026-11-09: synthetic in 2, 3 and 5 parts, the Nepal main
  pack in 2 and 3 (46.7 s, ignored test with `TGD_PACK`); planted bugs fail it (D-114)
- [ ] L1 at 1 GB mapped, L4, L5 at 1, 3 and 10 layers (1 and 3 layers on a synthetic pack
  measured 2026-11-09; Nepal at 1 and 3 layers measured the same day; 10 layers and 1 GB
  still to run)

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
- [ ] Storage on disk: the partial file as a sparse file (or offsets in a compact file where a
  platform has no holes), a sidecar of present and verified sections; with 4.5 and 4.6
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
- [ ] Forge: the baseline build (countries, admin1, cities above a threshold, simplified world
  timezones), same ID registry, thresholds and tolerances as data
- [ ] The locator: a REGION over buffered coverage polygons; `missing_pack_for(point)`;
  border-probe gate (no missed pack; extra-candidate rate reported)
- [ ] Size gate (proposed 8 MB, Z1); `baseline` feature; WASM loads it as a file (Z3)

### 4.3 Catalog and trust
- [ ] TUF subset client (`sync::trust`): root chain, timestamp, snapshot, targets, canonical
  JSON, persisted versions, length caps, the offline-first expiry rule
- [ ] Catalog `custom` schema v1 with a checked-in JSON schema
- [ ] Differential gate against `tough` and python-tuf
- [ ] Repository tooling in `xtask`: sign, publish metadata to Pages, upload to sharded immutable
  releases, the scheduled timestamp job; key ceremony and rotation drill documented
- [ ] Discovery: `catalog.find(name)` over the baseline (and the gazetteer pack when present),
  then the locator, then the catalog; `Resolver` trait

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
- [ ] Sans-I/O planner and installer on `transport::Frames`; `Transport` trait with a `ureq`
  default (binary size gate against `reqwest`); a head-only parser in `teigeodata-format`, so
  the layout comes from the head frame alone
- [ ] Verify-then-write per block; resume by frame with ETag and length checks (the host ignores
  `If-Range`); mirrors with demotion
- [ ] Staging, fsync, rename, journal commit, crash recovery; snapshot swap on install
- [ ] Platform transport prototypes for gate 14 (iOS background `URLSession`, Android UIDT,
  web `fetch` + OPFS); packaging stays in Phase 5

### 4.6 Storage manager
- [ ] Installed list, quota, LRU eviction with pinning and dependency protection, the disk-peak
  formula ([download and delta](../../02-research/notes/download-and-delta.md) §3.4)
- [ ] `verify(deep)` and repair by section

### 4.7 Delta updates
- [ ] Section re-download over frames (S3)
- [ ] Two real consecutive Nepal releases: bytes for section re-download against delta packs
  (and zstd patches, dropped unless this disagrees); decision recorded
- [ ] If delta packs win: forge emission, compaction policy, L5 with delta layers

### 4.8 The 15 sync gates
- [ ] Each gate of ADR-0037 (listed in [offline loading and
  trust](../../02-research/notes/offline-loading-and-trust.md) §6) built, first shown to fail on
  a deliberate violation, then green in CI

### 4.9 CLI `tgd`
- [ ] `inspect`, `verify`, `diff` (sections and records), `extract`, `merge` (compaction),
  `export`; `catalog`, `find`, `install`, `update`, `remove`, `pin`, `locate`
- [ ] Every command run and its output read; documented in `--help` and the docs

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
