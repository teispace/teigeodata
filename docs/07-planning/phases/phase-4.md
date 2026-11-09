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
- [ ] `teigeodata-engine`: mapped packs (B13: where `unsafe` for mmap lives, SIGBUS on a
  truncated file), lazy verification (small sections whole, large per block, D-078)
- [ ] Layers, precedence and coverage rules
  ([engine and catalog](../../02-research/notes/engine-and-catalog.md) §3.4); tombstones and
  redirects across layers
- [ ] Routing and k-way merges for `get`, `reverse`, `admin_at`, `tz_at`, box and radius
  queries, `search`; per-answer tier; caller-owned scratch (M1 zero allocations)
- [ ] Snapshot swap: `arc-swap` against `RwLock` A/B in L5
- [ ] Metamorphic gate: splitting or reordering packs never changes an answer; differential
  against one compacted pack
- [ ] L1 at 1 GB mapped, L4, L5 at 1, 3 and 10 layers

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
- [ ] Seekable zstd writer (frames on the head and sections, ≤ 1 MiB at 16 KiB boundaries) with
  a byte-for-byte self-check; read by `zeekstd` and the `zstd` CLI
- [ ] Seek-table reader and frame planner, fuzzed (lying tables, oversized frames, frames across
  sections)

### 4.5 Download manager
- [ ] Sans-I/O planner and installer; `Transport` trait with a `ureq` default (binary size gate
  against `reqwest`)
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
